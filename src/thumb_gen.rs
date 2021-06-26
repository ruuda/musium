// Musium -- Music playback daemon with web-based library browser
// Copyright 2021 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Utilities for extracting thumbnails from flac files.

use std::fs;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::process;

use crate::{MetaIndex, MemoryMetaIndex};
use crate::error::{Error, Result};
use crate::prim::{AlbumId, Mtime};

/// The state of generating a single thumbnail.
enum GenThumb {
    Resizing {
        out_fname_png: PathBuf,
        out_fname_jpg: PathBuf,
        child: process::Child,
    },
    Compressing {
        out_fname_png: PathBuf,
        child: process::Child,
    },
}

impl GenThumb {
    /// Start an extract-and-resize operation.
    pub fn new(
        cache_dir: &Path,
        album_id: AlbumId,
        filename: &str,
        album_mtime: Mtime,
    ) -> Result<Option<GenThumb>> {
        let mut out_fname_jpg: PathBuf = PathBuf::from(cache_dir);
        out_fname_jpg.push(format!("{}.jpg", album_id));

        // Early-out on existing files that are more recent than the album they
        // are based on.
        if let Ok(meta) = fs::metadata(&out_fname_jpg) {
            return Ok(None);
            if meta.mtime() > album_mtime.0 {
                // The file exists and is up to date, nothing to be done here.
                return Ok(None);
            } else {
                // The file exists but is potentially outdated, delete it.
                fs::remove_file(&out_fname_jpg)?;
            }
        }

        let mut out_fname_png: PathBuf = PathBuf::from(cache_dir);
        out_fname_png.push(format!("{}.png", album_id));

        let opts = claxon::FlacReaderOptions {
            metadata_only: true,
            read_picture: claxon::ReadPicture::CoverAsVec,
            read_vorbis_comment: false,
        };
        let reader = claxon::FlacReader::open_ext(filename, opts)
            .map_err(|err| Error::from_claxon(PathBuf::from(filename), err))?;

        let cover = match reader.into_pictures().pop() {
            Some(c) => c,
            None => return Ok(None),
        };

        // TODO: Add a nicer way to report progress.
        println!("{:?} <- {}", &out_fname_jpg, filename);

        let mut convert = Command::new("convert")
            // Read from stdin.
            .arg("-")
            // Some cover arts have an alpha channel, but we are going to encode
            // to jpeg which does not support it. First blend the image with a
            // black background, then drop the alpha channel. We also need a
            // -flatten to ensure that the subsequent distort operation uses the
            // "Edge" virtual pixel mode, rather than sampling the black
            // background. If it samples the black background, the edges of the
            // thumbnail become darker, which is especially noticeable for
            // covers with white edges, and also shows up as a "pop" in the
            // album view when the full-resolution image loads.
            .args(&[
                "-background", "black",
                "-alpha", "remove",
                "-alpha", "off",
                "-flatten"
            ])
            // Resize in a linear color space, sRGB is not suitable for it
            // because it is nonlinear. "RGB" in ImageMagick is linear.
            .args(&["-colorspace", "RGB"])
            // See also the note about -flatten above. I think Edge is the
            // default, but let's be explicit about it.
            .args(&["-virtual-pixel", "Edge"])
            // Lanczos2 is a bit less sharp than Cosine, but less sharp edges
            // means that the image compresses better, and less artifacts. But
            // still, Lanczos was too blurry in my opinion.
            .args(&["-filter", "Cosine"])
            // Twice the size of the thumb in the webinterface, so they appear
            // pixel-perfect on a high-DPI display, or on a mobile phone.
            .args(&["-distort", "Resize", "140x140!"])
            .args(&["-colorspace", "sRGB"])
            // Remove EXIF metadata, including the colour profile if there was
            // any -- we convert to sRGB anyway.
            .args(&["-strip"])
            // Write lossless, we will later compress to jpeg with Guetzli,
            // which has a better compressor.
            .arg(&out_fname_png)
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .spawn()
            // TODO: Handle errors properly.
            .expect("Failed to spawn Imagemagick's 'convert'.");

        {
            let stdin = convert.stdin.as_mut().expect("Failed to open stdin.");
            stdin.write_all(cover.data()).unwrap();
        }

        let result = GenThumb::Resizing {
            out_fname_png: out_fname_png,
            out_fname_jpg: out_fname_jpg,
            child: convert
        };
        Ok(Some(result))
    }

    /// Wait for one step of the process, start and return the next one if any.
    pub fn poll(self) -> Option<GenThumb> {

        match self {
            GenThumb::Resizing { out_fname_png, out_fname_jpg, mut child } => {
                // TODO: Use a custom error type, remove all `expect()`s.
                child.wait().expect("Failed to run Imagemagick's 'convert'.");
                let guetzli = Command::new("guetzli")
                    .args(&["--quality", "97"])
                    .arg(&out_fname_png)
                    .arg(&out_fname_jpg)
                    // TODO: Handle errors properly.
                    .spawn().expect("Failed to spawn Guetzli.");
                let result = GenThumb::Compressing {
                    out_fname_png: out_fname_png,
                    child: guetzli,
                };
                Some(result)
            }
            GenThumb::Compressing { out_fname_png, mut child } => {
                // TODO: Handle errors properly.
                child.wait().expect("Failed to run Guetzli.");

                // Delete the intermediate png file.
                fs::remove_file(&out_fname_png).expect("Failed to delete intermediate file.");

                None
            }
        }
    }

    fn is_done(&mut self) -> bool {
        let child = match self {
            GenThumb::Resizing { ref mut child, .. } => child,
            GenThumb::Compressing { ref mut child, .. } => child,
        };
        match child.try_wait() {
            Ok(Some(_)) => true,
            _ => false,
        }
    }
}

/// Controls parallelism when generating thumbnails.
struct GenThumbs<'a> {
    cache_dir: &'a Path,
    pending: Vec<GenThumb>,
    max_len: usize,
}

impl<'a> GenThumbs<'a> {
    pub fn new(cache_dir: &'a Path, max_parallelism: usize) -> GenThumbs<'a> {
        GenThumbs {
            cache_dir: cache_dir,
            pending: Vec::new(),
            max_len: max_parallelism,
        }
    }

    fn wait_until_at_most_in_use(&mut self, max_used: usize) {
        while self.pending.len() > max_used {
            let mut found_one = false;

            // Round 1: Try to find a process that is already finished, and make
            // progress on that.
            for i in 0..self.pending.len() {
                if self.pending[i].is_done() {
                    let gen = self.pending.remove(i);
                    if let Some(next_gen) = gen.poll() {
                        self.pending.push(next_gen);
                    }
                    found_one = true;
                    break
                }
            }

            if found_one {
                continue
            }

            // Round 2: All processes are still running, wait for the oldest one.
            let gen = self.pending.remove(0);
            if let Some(next_gen) = gen.poll() {
                self.pending.push(next_gen);
            }
        }
    }

    pub fn add(&mut self, album_id: AlbumId, filename: &str, album_mtime: Mtime) -> Result<()> {
        let max_used = self.max_len - 1;
        self.wait_until_at_most_in_use(max_used);
        if let Some(gen) = GenThumb::new(self.cache_dir, album_id, filename, album_mtime)? {
            self.pending.push(gen);
        }
        Ok(())
    }

    pub fn drain(&mut self) {
        self.wait_until_at_most_in_use(0);
    }
}

pub fn generate_thumbnails(db_path: &Path, cache_dir: &Path) -> Result<()> {
    let (index, builder) = MemoryMetaIndex::from_database(db_path)?;

    // TODO: Move issue reporting to a better place. Maybe take the builder and
    // index as an argument to this method.
    for issue in &builder.issues {
        println!("{}", issue);
    }

    let max_parallelism = 32;
    let mut gen_thumbs = GenThumbs::new(cache_dir, max_parallelism);
    let mut prev_album_id = AlbumId(0);
    for &(_tid, ref track) in index.get_tracks() {
        if track.album_id != prev_album_id {
            let fname = index.get_filename(track.filename);
            let mtime = builder.album_mtimes[&track.album_id];
            gen_thumbs.add(track.album_id, fname, mtime)?;
            prev_album_id = track.album_id;
        }
    }
    gen_thumbs.drain();

    Ok(())
}
