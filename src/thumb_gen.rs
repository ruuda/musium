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
use std::sync::Mutex;
use std::sync::mpsc::SyncSender;

use crate::build::BuildMetaIndex;
use crate::error::{Error, Result};
use crate::prim::{AlbumId, Mtime};
use crate::scan::{ScanStage, Status};
use crate::{MetaIndex, MemoryMetaIndex};

/// Tracks the process of generating a thumbnail.
struct GenThumb<'a> {
    album_id: AlbumId,
    state: GenThumbState<'a>,
}

/// The state of generating a single thumbnail.
enum GenThumbState<'a> {
    Pending {
        flac_filename: &'a Path,
    },
    Resizing {
        child: process::Child,
    },
    Compressing {
        child: process::Child,
    },
}

fn get_fname(thumb_dir: &Path, album_id: AlbumId, extension: &str) -> PathBuf {
    let mut fname = PathBuf::from(thumb_dir);
    fname.push(format!("{}{}", album_id, extension));
    fname
}

impl<'a> GenThumb<'a> {
    /// Create an extract-and-resize operation, if needed.
    ///
    /// If no thumbnail exists for the item yet, or if it does, but its mtime is
    /// older than the `album_mtime`, then this returns the task for generating
    /// the thumbnail, in the [`GenThumb::Pending`] state.
    ///
    /// If a thumnail exists, but it is outdated, this deletes the file before
    /// returning the task to regenerate it.
    pub fn new(
        thumb_dir: &Path,
        album_id: AlbumId,
        flac_filename: &'a Path,
        album_mtime: Mtime,
    ) -> Result<Option<GenThumb<'a>>> {
        let task = GenThumb {
            album_id: album_id,
            state: GenThumbState::Pending { flac_filename },
        };

        let out_fname_jpg = get_fname(thumb_dir, task.album_id, ".jpg");

        // Early-out on existing files that are more recent than the album they
        // are based on.
        if let Ok(meta) = fs::metadata(&out_fname_jpg) {
            if meta.mtime() > album_mtime.0 {
                // The file exists and is up to date, nothing to be done here.
                return Ok(None);
            } else {
                // The file exists but is potentially outdated, delete it.
                fs::remove_file(&out_fname_jpg)?;
            }
        }

        Ok(Some(task))
    }

    /// From `Pending` state, read a picture, and start resizing it.
    ///
    /// Returns `None` if the input file does not contain any pictures.
    fn start_resize(mut self, thumb_dir: &Path, flac_filename: &Path) -> Result<Option<GenThumb<'a>>> {
        let out_fname_png = get_fname(thumb_dir, self.album_id, ".png");

        let opts = claxon::FlacReaderOptions {
            metadata_only: true,
            read_picture: claxon::ReadPicture::CoverAsVec,
            read_vorbis_comment: false,
        };
        let reader = claxon::FlacReader::open_ext(flac_filename, opts)
            .map_err(|err| Error::from_claxon(PathBuf::from(flac_filename), err))?;

        let cover = match reader.into_pictures().pop() {
            Some(c) => c,
            None => return Ok(None),
        };

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
            .map_err(|e| Error::CommandError("Failed to spawn ImageMagick's 'convert'.", e))?;

        {
            let stdin = convert.stdin.as_mut().expect("Stdin should be there, we piped it.");
            stdin.write_all(cover.data()).unwrap();
        }

        self.state = GenThumbState::Resizing { child: convert };

        Ok(Some(self))
    }

    /// When in `Resizing` state, wait for that to complete, and start compressing.
    fn start_compress(mut self, thumb_dir: &Path) -> Result<GenThumb<'a>> {
        let mut convert = match self.state {
            GenThumbState::Resizing { child } => child,
            _ => panic!("Can only call start_compress in Resizing state."),
        };
        let out_fname_png = get_fname(thumb_dir, self.album_id, ".png");
        let out_fname_jpg = get_fname(thumb_dir, self.album_id, ".jpg");

        convert
            .wait()
            .map_err(|e| Error::CommandError("Imagemagick's 'convert' failed.", e))?;

        let guetzli = Command::new("guetzli")
            .args(&["--quality", "97"])
            .arg(&out_fname_png)
            .arg(&out_fname_jpg)
            .spawn()
            .map_err(|e| Error::CommandError("Failed to spawn 'guetzli'.", e))?;

        self.state = GenThumbState::Compressing { child: guetzli };

        // TODO: Insert into the database instead of writing to a file.

        Ok(self)
    }

    /// Take the next step that is needed to generate a thumbnail.
    ///
    /// When this returns `Some`, a process is running in the background, and we
    /// need to advance once more in the future to conclude.
    ///
    /// When this returns `None`, thumbnail generation is complete.
    fn advance(self, thumb_dir: &Path) -> Result<Option<GenThumb<'a>>> {
        match self.state {
            GenThumbState::Pending { flac_filename } => {
                self.start_resize(thumb_dir, flac_filename)
            }
            GenThumbState::Resizing { .. } => {
                self.start_compress(thumb_dir).map(Some)
            }
            GenThumbState::Compressing { mut child } => {
                child
                    .wait()
                    .map_err(|e| Error::CommandError("Guetzli failed.", e))?;

                // Delete the intermediate png file.
                let intermediate_file = get_fname(thumb_dir, self.album_id, ".png");
                fs::remove_file(&intermediate_file)?;

                Ok(None)
            }
        }
    }
}

struct GenThumbs<'a> {
    tasks: Vec<GenThumb<'a>>,
    status: &'a mut Status,
    status_sender: &'a mut SyncSender<Status>,
}

impl<'a> GenThumbs<'a> {
    /// Take a task out of the queue, to call [`GenThumb::advance`] on.
    fn pop(&mut self) -> Option<GenThumb<'a>> {
        self.tasks.pop()
    }

    /// Handle the result of [`GenThumb::advance`].
    fn put(&mut self, result: Option<GenThumb<'a>>) {
        match result {
            Some(next_task) => self.tasks.push(next_task),
            None => {
                self.status.files_processed_thumbnails += 1;
                self.status_sender.send(*self.status).unwrap();
            }
        }
    }
}

pub fn generate_thumbnails(
    index: &MemoryMetaIndex,
    builder: &BuildMetaIndex,
    status: &mut Status,
    status_sender: &mut SyncSender<Status>,
) -> Result<()> {
    status.stage = ScanStage::PreProcessingThumbnails;
    status_sender.send(*status).unwrap();
    let thumb_dir = todo!("Get rid of file system operations.");

    // Determine which albums need to have a new thumbnail extracted.
    let mut pending_tasks = Vec::new();
    let mut prev_album_id = AlbumId(0);
    for &(_tid, ref track) in index.get_tracks() {
        if track.album_id != prev_album_id {
            let fname = index.get_filename(track.filename);
            let mtime = builder.album_mtimes[&track.album_id];
            for task in GenThumb::new(thumb_dir, track.album_id, fname.as_ref(), mtime)? {
                pending_tasks.push(task);
                status.files_to_process_thumbnails += 1;

                if pending_tasks.len() % 32 == 0 {
                    status_sender.send(*status).unwrap();
                }
            }
            prev_album_id = track.album_id;
        }
    }

    status.stage = ScanStage::GeneratingThumbnails;
    status_sender.send(*status).unwrap();

    let queue = GenThumbs {
        tasks: pending_tasks,
        status: status,
        status_sender: status_sender,
    };
    let mutex = Mutex::new(queue);
    let mutex_ref = &mutex;

    // Start 1 + `num_cpus` worker threads. All these threads will do is block
    // and wait on IO or the external process, but both `convert` and `guetzli`
    // are CPU-bound, so this should keep the CPU busy. When thumbnailing many
    // albums with a cold page cache, IO to read the thumb from the file can be
    // a factor too, so add one additional thread to ensure we can keep the CPU
    // busy.
    crossbeam::scope(|scope| {
        for i in 0..num_cpus::get() + 0 {
            let drain = move || {
                while let Some(task) = {
                    // This has to be in a scope, otherwise the program deadlocks.
                    let mut tasks = mutex_ref.lock().unwrap();
                    tasks.pop()
                } {
                    let result = task
                        .advance(thumb_dir)
                        // There is no simple way with the current version of
                        // Crossbeam to get a result out of the thread, so we
                        // just panic on error, it's what we would do elsewhere
                        // anyway if we could get the result out.
                        .expect("Thumbnail generation failed.");

                    mutex_ref.lock().unwrap().put(result);
                }
            };

            scope
                .builder()
                .name(format!("Thumbnail generation thread {}", i))
                .spawn(drain)
                .expect("Failed to spawn OS thread.");
        }
    });

    Ok(())
}
