# Changelog

Musium follows a “rolling release” or “nightly” development process. I compile
and run `master` shortly after I make changes. However, sometimes there are
notable changes, and especially if they require manual intervention, it is
useful to write them down in a changelog.

Musium only started tagging releases in February 2023. Versions prior to 0.11.0
have been tagged as such retroactively to mark notable milestones. Those tags
were not present at the time.

## Versioning policy

Musium versions are named `MAJOR.MINOR.PATCH`.

 * The major version number is purely cosmetic and represents the author’s
   sentiment about feature-completeness.
 * The minor version is bumped when new features are added and when user
   intervention is required to update (for example, a change in the config file,
   or a migration of the database).
 * The patch version is bumped for bugfixes and other small changes.

## Next

Not yet released.

 * **Breaking:** The `audio_device` configuration option now expects an
   <abbr>Alsa</abbr> <abbr>PCM</abbr> name, rather than the name of the device.
   This enables more control over how Musium outputs audio. See the [updated
   configuration docs](configuration.md#audio_device) for how to set this value.
 * The [high-pass filter](highpass.md) can now be adjusted at runtime, just like
   the volume. The existing [`high_pass_cutoff` setting](configuration.md#high_pass_cutoff)
   now controls the initial value at startup.
 * The initial volume at startup is now configurable with the new [`volume`
   setting](configuration.md#volume).

## 0.15.1

Released 2024-11-02.

 * Visual tweak: use tabular numbers for the track numbers in the <abbr>UI</abbr>.
 * Tweak the coefficients used for the discovery ranking.
 * Work around a regression in the Linux kernel that was introduced some time
   after 5.10.94, which prevents changing the hw params of an <abbr>ALSA</abbr>
   device. When switching playback between tracks that have a different sample
   rate, on Linux 5.10.94 this would work fine, but on later versions,
   `snd_pcm_hw_params` started returning error code 22 (invalid argument).
   We work around this by re-opening the audio device if the sample format
   changes, rather than trying to change the sample format of the existing
   device.

## 0.15.0

Released 2024-08-25.

 * **Breaking:** In `scrobble.py`, all commands are now prefixed by `lastfm` or
   `listenbrainz`. In particular, `scrobble.py scrobble` is now `scrobble.py
   lastfm scrobble`.
 * **Breaking:** Thumbnail generation now requires the `magick` command to be
   present, which requires ImageMagick 7. Compression now uses `cjpegli` from
   `libjxl` rather than Guetzli.
 * The scrobble script can now import listening history from Last.fm with the
   new `import` and `sync` commands, see [the importing chapter](lastfm-import.md).
   The imported history is not yet used for playcounts, but can already serve as
   a way to back up listens from Last.fm into a database under your control.
 * Internal preparations to match imported listens to library tracks.
 * Add support for normalizing the ring diacritic (as in å) in titles.
 * Tweak the discover sort mode, add a new _trending_ sort mode.

## 0.14.0

Released 2024-05-19.

 * **Breaking:** Musium now uses Rust 1.70 (up from 1.57) to build the server,
   and the Spago build tool to build the client. The Nix development environment
   makes both available.
 * Support storing track ratings. For now, in the webinterface you can only
   change the rating of the currently playing track.
 * Add <abbr>API</abbr> endpoint for shuffling the play queue. There is no
   button for this in the webinterface yet.
 * Add endpoints for clearing the play queue and dequeueing a track.
 * A few new exotic characters and diacritics are now normalized for the pursose
   of search. (E.g. a search for _dadi freyr_ will now match _Daði Freyr_.)
 * The SQLite <abbr>WAL</abbr> is now flushed after playback ends, to ensure
   that the database file is self-contained when the player is in an idle state.
   This makes it easier to back up the database.
 * There is a new playcount module that is used to power a new _discover_
   sorting mode. Discoveries are a mix of currently trending albums, and albums
   that were popular in the past but have few recent listens. The new `musium
   count` subcommand prints statistics for debugging.

## 0.13.0

Released 2023-05-21.

 * **Breaking:** Album ids are now 52 bits instead of 64 bit. This ensures that
   album ids are prefixes of track ids, which unlocks a few optimizations and
   simplifications. Unfortunately, this means that existing databases and
   listens are no longer valid. To migrate, use `tools/migrate_album_ids.py`.

## 0.12.0

Released 2023-05-06.

 * Sorting can now be controlled in the library browser. The sort options are
   _release date_ — the original release date, and _first seen_ — based on the
   age (mtime) or first listen of the files in the album (whichever is earlier).

## 0.11.0

Released 2023-04-10.

 * **Breaking:** Musium now supports multiple artists per album. However, to be
   able to support this, the database schema has changed significantly. To
   migrate, back up the old database, then migrate listens and import timestamps
   using `tools/migrate_0.11.0.py`.
 * **Breaking:** The `data_path` configuration option was renamed to `db_path`,
   Musium was not storing anything in the data path aside from the database
   anyway. Unlike `data_path`, `db_path` should include the file name.
 * **Breaking:** Cover art thumbnails are now stored in the database. The
   `covers_path` configuration option has been removed.
 * The Nix-based development environment that was using a Nix 2.3-compatible
   `default.nix` has been replaced with a flake that requires Nix 2.10 or later.
 * Database boilerplate is now generated by [Squiller][squiller].

[squiller]: https://docs.ruuda.nl/squiller/

## 0.10.1

Authored 2023-02-18, but tagged retroactively on 2023-02-26.

 * **Breaking:** Requires Rust 1.57.0 to build.
 * Make the webinterface more responsive.
 * Increase the priority of the playback thread to reduce the probability
   of a buffer underrun when the system is under load.
 * Update dependencies, small improvements and fixes.

## 0.10.0

Authored 2022-03-06, but tagged retroactively on 2023-02-26.

 * Add support for a pre-playback and post-idle command. This can be used to
   e.g. power speakers on and off. See also the included instructions for
   [controlling Ikea Trådfri wireless control outlets](tradfri.md).
 * Display a “waveform” of the track on the _now playing_ pane. The shape is
   technically not the sound wave, instead it represents loudness over time.
 * To power this new feature, compute loudness information during scanning, and
   store it in the database. Album and track loudness is also computed, although
   at this point this data is unused and Musium still relies on loudness
   metadata in tags.

Note: In November 2021 Linux 5.15 was released. This kernel contains a
regression in the <abbr>ALSA</abbr> subsystem that breaks Musium when switching
between different sample rates or bit depths. Linux 5.10.94 is the last version
without this regression.

## 0.9.0

Authored 2021-10-16, but tagged retroactively on 2023-02-26.

 * Add an _About_ pane on the webinterface that shows statistics about the
   library.
 * Add the ability to trigger a scan from the webinterface without having to
   restart the process.

## 0.8.0

Authored 2021-08-01, but tagged retroactively on 2023-02-26.

 * Add a high-pass filter to make playback of bass-heavy albums more pleasant
   when the room acoustics are suboptimal. (Or just for when you are afraid that
   turning up the volume would disturb the neighbors.)
 * Fix a rare hang during playback.

## 0.7.0

Authored 2021-06-19, but tagged retroactively on 2023-02-26.

 * Add queue size indicator on the queue tab in the webinterface.
 * Cache file metadata in the SQLite database to improve startup performance.
   Musium no longer scans the metadata of all files at startup, instead it loads
   metadata from the database, and a separate `scan` command updates the
   database.

## 0.6.0

Authored 2021-05-29, but tagged retroactively on 2023-02-26.

 * Add an “enqueue” button for enqueueing entire albums.
 * Reduce stutter in webinterface animations.
 * Open the _now playing_ page on first load if a track is playing.
 * Add keyboard shortcut for opening the search pane.
 * Make loudness normalization contextual. If multiple tracks from the same
   album play consecutively, we use the album loudness, to ensure that the album
   plays back exactly as it was mastered. When tracks from multiple albums are
   mixed in the play queue, use track loudness, to get more accurate loudness
   normalization.
 * Add support for notifying systemd of startup progress.
 * Move all <abbr>API</abbr> endpoints behind the `/api` path in the webserver,
   so <abbr>API</abbr> urls do not clash with app urls.

## 0.5.0

Authored 2021-04-03, but tagged retroactively on 2023-02-26.

 * List albums in reverse chronological order, instead of chronological.
 * Show a spinner in the webinterface when the track is buffering.
 * Enable opening the artist pane at first load if a track is playing.
 * Make more things clickable in the webinterface.
 * Fix display issues with non-square cover art.

## 0.4.0

Authored 2021-01-23, but tagged retroactively on 2023-02-26.

 * Split the library and album view.
 * Add navigation buttons in the webinterface.
 * Add support for submitting listens to ListenBrainz, in addition to Last.fm.
 * Handle an audio buffer underrun that could occur in some situations.

## 0.3.1

Authored 2020-11-27, but tagged retroactively on 2023-02-26.

 * Small tweaks to the _now playing_ and search results pages.
 * Keep all thumbnails in memory, so no disk access is required to serve most of
   the webinterface.
 * Internal improvements to search, to better enable matching external listening
   history from ListenBrainz. The script to import listening history is still a
   work in progress.

## 0.3.0

Authored 2020-10-04, but tagged retroactively on 2023-02-26.

 * Improvements to the webinterface, with multiple panes and animations.
 * Add the listens database, and a script to scrobble listens to Last.fm.
   Initially the script acted on the json play log, but the play log was quickly
   superseded by a SQLite database. At this point the database was not used for
   anything else, Musium would re-scan all files at startup.

## 0.2.0

Authored 2020-08-28, but tagged retroactively on 2023-02-26.

 * Add support for loudness normalization, through `bs17704_track_loudness` and
   `bs17704_album_loudness` tags. These tags can be written by an external
   utility, [`flacgain` in the BS1770 library][flacgain].
 * Volume control in the webinterface.

[flacgain]: https://github.com/ruuda/bs1770#tagging-flac-files

## 0.1.0

Authored 2020-08-22, but tagged retroactively on 2023-02-26.

Musium has been an evolving project. It started out as an attempt to index
metadata about a collection of flac files, and expose that (and the files
themselves) over http to enable Chromecast playback. Later it pivoted to being
a music player with a webinterface but local playback, essentially a music
playback server.

Until February 2023, Musium did not tag releases, I just ran `master` locally,
rebuilding and restarting it after every change. Essentially there was only the
“nightly” version. But now that I do want to start tracking changes, this
particular version makes sense as the initial release, as it marks the point
where I started using Musium personally.

Features included:

 * Fast indexing.
 * A webinterface to browse the library and start tracks.
 * Logging of listens to a file as <abbr>JSON</abbr>.
 * Advanced search.
