// Musium -- Music playback daemon with web-based library browser
// Copyright 2021 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Utilities for extracting thumbnails from flac files.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::process::{Command, Stdio};
use std::sync::mpsc::SyncSender;
use std::sync::Mutex;

use crate::database;
use crate::database::{Connection, Transaction};
use crate::database_utils;
use crate::error::{Error, Result};
use crate::prim::{AlbumId, Color, FileId};
use crate::scan::{ScanStage, Status};
use crate::{MemoryMetaIndex, MetaIndex};

/// Tracks the process of generating a thumbnail.
struct GenThumb<'a> {
    album_id: AlbumId,
    state: GenThumbState<'a>,
}

/// The state of generating a single thumbnail.
enum GenThumbState<'a> {
    /// We haven't started this task, but we know the file we need to inspect.
    Pending {
        file_id: FileId,
        flac_filename: &'a Path,
    },
    /// Imagemagick is downscaling the cover art to a temporary file.
    Resizing {
        file_id: FileId,
        child: process::Child,
        out_path: PathBuf,
    },
    /// Imagemagick is analyzing the thumbnail to extract the dominant color.
    Analyzing {
        file_id: FileId,
        child: process::Child,
        path: PathBuf,
    },
    /// Cjpegli is compressing the thumbnail.
    Compressing {
        file_id: FileId,
        color: Color,
        child: process::Child,
        in_path: PathBuf,
    },
}

/// Return the intermediate file path where we write the resized but uncompressed thumbnail.
fn get_tmp_fname(album_id: AlbumId) -> PathBuf {
    let mut fname = std::env::temp_dir();
    fname.push(format!("musium-thumb-{}.png", album_id));
    fname
}

impl<'a> GenThumb<'a> {
    /// Create an extract-and-resize operation, if needed.
    ///
    /// If no thumbnail exists for the item yet, then this returns the task for
    /// generating the thumbnail, in the [`GenThumb::Pending`] state.
    pub fn new(
        tx: &mut Transaction,
        album_id: AlbumId,
        file_id: FileId,
        flac_filename: &'a Path,
    ) -> Result<Option<GenThumb<'a>>> {
        let task = GenThumb {
            album_id,
            state: GenThumbState::Pending {
                flac_filename,
                file_id,
            },
        };

        match database::select_thumbnail_exists(tx, album_id.0 as i64)? {
            0 => Ok(Some(task)),
            _ => Ok(None),
        }
    }

    /// From `Pending` state, read a picture, and start resizing it.
    ///
    /// Returns `None` if the input file does not contain any pictures.
    fn start_resize(
        mut self,
        album_id: AlbumId,
        file_id: FileId,
        flac_filename: &Path,
    ) -> Result<Option<GenThumb<'a>>> {
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

        let out_path = get_tmp_fname(album_id);

        let mut magick = Command::new("magick")
            // Give Imagemagick enough time to open the image, recent versions
            // are strict about it which leads to "time limit exceeded" error
            // from "fatal/cache.c". The unit is seconds.
            .args(["-limit", "time", "120"])
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
            .args(["-background", "black"])
            .args(["-alpha", "remove"])
            .args(["-alpha", "off"])
            .args(["-flatten"])
            // Resize in a linear color space, sRGB is not suitable for it
            // because it is nonlinear. "RGB" in ImageMagick is linear.
            .args(["-colorspace", "RGB"])
            // See also the note about -flatten above. I think Edge is the
            // default, but let's be explicit about it.
            .args(["-virtual-pixel", "Edge"])
            // Lanczos2 is a bit less sharp than Cosine, but less sharp edges
            // means that the image compresses better, and less artifacts. But
            // still, Lanczos was too blurry in my opinion.
            .args(["-filter", "Cosine"])
            // Twice the size of the thumb in the webinterface, so they appear
            // pixel-perfect on a high-DPI display, or on a mobile phone.
            .args(["-distort", "Resize", "140x140!"])
            .args(["-colorspace", "sRGB"])
            // Remove EXIF metadata, including the colour profile if there was
            // any -- we convert to sRGB anyway.
            .args(["-strip"])
            // Write lossless, we will later compress with a better compressor
            // than ImageMagick.
            .arg(&out_path)
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| Error::CommandError("Failed to spawn ImageMagick.", Some(e)))?;

        {
            let stdin = magick
                .stdin
                .as_mut()
                .expect("Stdin should be there, we piped it.");
            stdin.write_all(cover.data()).unwrap();
        }

        self.state = GenThumbState::Resizing {
            file_id: file_id,
            child: magick,
            out_path: out_path,
        };

        Ok(Some(self))
    }

    fn start_analyze(mut self) -> Result<GenThumb<'a>> {
        let (mut magick, file_id, out_path) = match self.state {
            GenThumbState::Resizing {
                child,
                file_id,
                out_path,
            } => (child, file_id, out_path),
            _ => panic!("Can only call start_analyze in Resizing state."),
        };

        let exit_status = magick
            .wait()
            .map_err(|e| Error::CommandError("ImageMagick's 'magick' failed.", Some(e)))?;

        if !exit_status.success() {
            // Clean up the intermediate png file on error.
            let _rm_result_ignored = std::fs::remove_file(out_path);
            return Err(Error::CommandError(
                "ImageMagick's 'magick' did not exit successfully.",
                None,
            ));
        }

        // We are going to use Imagemagick to find a dominant color in the image.
        // The process below was tuned by hand on a set of thumbnails so it's
        // optimized to work well on a wide range of cover art thumbnails, but
        // especially those that contain one larger area of a solid color. We
        // want to pick that one out.
        let magick = Command::new("magick")
            // Apply a timeout in seconds, like in the resize step.
            .args(["-limit", "time", "120"])
            .arg(&out_path)
            // Downscaling and the k-means should happen in a linear color space.
            .args(["-colorspace", "RGB"])
            // For the mode filter, for pixels outside the canvas, tile and
            // mirror the input. We want that rather than extend, otherwise the
            // edge colors would weigh relatively more.
            .args(["-virtual-pixel", "mirror"])
            // We pick the sizes so we can downscale with a simple box filter
            // where every pixel is the average of its four "parents".
            .args(["-filter", "box"])
            // We start with 72x72, it's close enough to half our 140x140 that
            // we still get a good sense of the colors, but importantly it is
            // 9 * 2 * 4, so we can downscale without creating blurry edges.
            .args(["-resize", "72x72"])
            // Pick out the 5 most important colors to work with from now on.
            .args(["-kmeans", "5"])
            // For every 18x18 area, pick the color that occurs most in that
            // area. This has the effect of "blurring", and making prominent
            // colors even more prominent. This is the main trick in the process.
            // Often it already eliminates one or two of the five colors.
            .args(["-statistic", "Mode", "18x18"])
            // Then we downscale, but that mixes the colors again so now we can
            // have more than 5, but if the source has a large-enough area of
            // one color, it will be preserved exactly.
            .args(["-resize", "18x18"])
            // Now we do more passes of "blurring" making prominent colors more
            // prominent, restricting colors, downscaling, etc.
            .args(["-statistic", "Mode", "9x9"])
            .args(["-kmeans", "3"])
            .args(["-statistic", "Mode", "9x9"])
            .args(["-statistic", "Mode", "9x9"])
            .args(["-resize", "9x9"])
            .args(["-kmeans", "3"])
            // We end up with 3 colors on a 9x9 grid. It's on purpose odd, so
            // in case of two dominant colors, there will be a winner, it can't
            // be 50/50. It can be 3/3/3 though.
            // For the final pick, we take the mode of a 9x9 region, so the
            // center pixel at (4, 4) contains the mode of the image itself.
            .args(["-statistic", "Mode", "9x9"])
            .args(["-colorspace", "sRGB"])
            // Print the color of the pixel at (4, 4) in hex to stdout.
            .args(["-format", "%[hex:p{4,4}]"])
            .stdout(Stdio::piped())
            .arg("info:-")
            .spawn()
            .map_err(|e| Error::CommandError("Failed to spawn 'magick'.", Some(e)))?;

        self.state = GenThumbState::Analyzing {
            file_id,
            // The input path to this step is the output of the previous step.
            path: out_path,
            child: magick,
        };

        Ok(self)
    }

    /// When in `Analyzing` state, wait for that to complete, and start compressing.
    fn start_compress(mut self) -> Result<GenThumb<'a>> {
        let (mut magick, file_id, path) = match self.state {
            GenThumbState::Analyzing {
                file_id,
                child,
                path,
            } => (child, file_id, path),
            _ => panic!("Can only call start_compress in Resizing state."),
        };

        let exit_status = magick
            .wait()
            .map_err(|e| Error::CommandError("ImageMagick's 'magick' failed.", Some(e)))?;

        if !exit_status.success() {
            // Clean up the intermediate png file on error.
            let _rm_result_ignored = std::fs::remove_file(path);
            return Err(Error::CommandError(
                "ImageMagick's 'magick' did not exit successfully.",
                None,
            ));
        }

        let mut color_buf = String::with_capacity(8);
        magick
            .stdout
            .expect("We piped stdout.")
            .take(8)
            .read_to_string(&mut color_buf)?;

        let color = Color::parse(&color_buf).ok_or(Error::CommandError(
            "ImageMagick did not return a valid color.",
            None,
        ))?;

        let cjpegli = Command::new("cjpegli")
            .arg("--distance=0.45")
            .arg("--progressive_level=0")
            // Input is the intermediate file.
            .arg(&path)
            // Output to stdout.
            .stdout(Stdio::piped())
            .arg("-")
            // Silence stderr because cjpegli prints by default.
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::CommandError("Failed to spawn 'cjpegli'.", Some(e)))?;

        self.state = GenThumbState::Compressing {
            file_id,
            color,
            child: cjpegli,
            // Input file for this step is the output of the previous command.
            in_path: path,
        };

        Ok(self)
    }

    /// Take the next step that is needed to generate a thumbnail.
    ///
    /// When this returns `Some`, a process is running in the background, and we
    /// need to advance once more in the future to conclude.
    ///
    /// When this returns `None`, thumbnail generation is complete.
    fn advance(self, db: &mut Connection) -> Result<Option<GenThumb<'a>>> {
        let album_id = self.album_id;

        match self.state {
            GenThumbState::Pending {
                file_id,
                flac_filename,
            } => self.start_resize(album_id, file_id, flac_filename),
            GenThumbState::Resizing { .. } => self.start_analyze().map(Some),
            GenThumbState::Analyzing { .. } => self.start_compress().map(Some),
            GenThumbState::Compressing {
                mut child,
                color,
                file_id,
                in_path,
            } => {
                let exit_status = child.wait().map_err(|e| {
                    Error::CommandError("Thumbnail compression with 'cjpegli' failed.", Some(e))
                })?;

                // Delete the intermediate png file.
                std::fs::remove_file(in_path)?;

                if !exit_status.success() {
                    return Err(Error::CommandError(
                        "'cjpegli' did not exit successfully.",
                        None,
                    ));
                }

                let mut stdout = child
                    .stdout
                    .take()
                    .expect("Stdout should be there, we piped it.");
                let mut jpeg_bytes = Vec::new();
                stdout.read_to_end(&mut jpeg_bytes)?;

                {
                    let mut tx = db.begin()?;
                    database::insert_album_thumbnail(
                        &mut tx,
                        album_id.0 as i64,
                        file_id.0,
                        &color.to_string(),
                        &jpeg_bytes[..],
                    )?;
                    tx.commit()?;
                }

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
    /// Take the next task out of the queue, to call [`GenThumb::advance`] on.
    fn pop(&mut self) -> Option<GenThumb<'a>> {
        self.tasks.pop()
    }

    /// Mark one thumbnail generation as done.
    fn increment_progress(&mut self) {
        self.status.files_processed_thumbnails += 1;
        self.status_sender.send(*self.status).unwrap();
    }
}

pub fn generate_thumbnails(
    index: &MemoryMetaIndex,
    db_path: &Path,
    status: &mut Status,
    status_sender: &mut SyncSender<Status>,
) -> Result<()> {
    status.stage = ScanStage::PreProcessingThumbnails;
    status_sender.send(*status).unwrap();

    let raw_conn = database_utils::connect_readonly(db_path)?;
    let mut conn = Connection::new(&raw_conn);
    let mut tx = conn.begin()?;

    // Determine which albums need to have a new thumbnail extracted.
    let mut pending_tasks = Vec::new();
    let mut prev_album_id = AlbumId(0);
    for kv in index.get_tracks() {
        let track_id = kv.track_id;
        let album_id = track_id.album_id();
        if album_id != prev_album_id {
            let fname = index.get_filename(kv.track.filename);
            if let Some(task) = GenThumb::new(&mut tx, album_id, kv.track.file_id, fname.as_ref())?
            {
                pending_tasks.push(task);
                status.files_to_process_thumbnails += 1;

                if pending_tasks.len() % 32 == 0 {
                    status_sender.send(*status).unwrap();
                }
            }
            prev_album_id = album_id;
        }
    }

    tx.commit()?;
    drop(conn);
    drop(raw_conn);

    status.stage = ScanStage::GeneratingThumbnails;
    status_sender.send(*status).unwrap();

    let queue = GenThumbs {
        tasks: pending_tasks,
        status: status,
        status_sender: status_sender,
    };
    let mutex = Mutex::new(queue);
    let mutex_ref = &mutex;

    // Start `num_cpus` worker threads. All these threads will do is block and
    // wait on IO or the external process, but both `magick` and `cjpegli`
    // are CPU-bound, so this should keep the CPU busy. When thumbnailing many
    // albums with a cold page cache, IO to read the thumb from the file can be
    // a factor too, so add one additional thread to ensure we can keep the CPU
    // busy. Edit: Or not, usually it's not needed.
    crossbeam::scope::<_, Result<()>>(|scope| {
        let n_threads = num_cpus::get();
        let mut threads: Vec<crossbeam::ScopedJoinHandle<Result<()>>> =
            Vec::with_capacity(n_threads);

        for i in 0..n_threads {
            let db_path_ref = db_path;
            let drain = move || {
                let raw_conn = database_utils::connect_read_write(db_path_ref)?;
                let mut conn = Connection::new(&raw_conn);

                let mut next_task = {
                    // This has to be in a scope, otherwise the program deadlocks.
                    let mut tasks = mutex_ref.lock().unwrap();
                    tasks.pop()
                };

                while let Some(task) = next_task.take() {
                    match task.advance(&mut conn) {
                        Ok(Some(next)) => {
                            next_task = Some(next);
                            continue;
                        }
                        Ok(None) => {
                            let mut tasks = mutex_ref.lock().unwrap();
                            tasks.increment_progress();
                            next_task = tasks.pop();
                        }
                        Err(Error::CommandError(msg, detail)) => {
                            match detail {
                                None => eprintln!("Thumbnail generation failed: {msg}"),
                                Some(err) => {
                                    eprintln!("Thumbnail generation failed: {msg} {err:?}")
                                }
                            }

                            // We count failing as progress, because we did look
                            // at one file that we were supposed to look at.
                            // Without this, the final count may be lower than
                            // the total.
                            let mut tasks = mutex_ref.lock().unwrap();
                            tasks.increment_progress();
                            next_task = tasks.pop();
                        }
                        Err(fatal) => panic!("Error during thumbnail generation: {:?}", fatal),
                    }
                }

                Ok(())
            };

            let join_handle = scope
                .builder()
                .name(format!("Thumbnail generation thread {}", i))
                .spawn(drain)
                .expect("Failed to spawn OS thread.");
            threads.push(join_handle);
        }

        for join_handle in threads.drain(..) {
            // The first unwrap is on joining, that should not fail because we
            // set panic=abort. The ? then propagates any errors that might have
            // happened in the thread.
            join_handle.join()?;
        }
        Ok(())
    })
}
