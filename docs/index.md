# Musium

Musium is an album-centered music player. It is designed to run as a daemon on
an always-on device connected to speakers. Playback can be controlled from
anywhere on the local network through a webinterface.

*Vaporware warning: while Musium is usable, it is missing essential features,
such as the ability to pause playback.*

## Features

 * Respects album artist and original release date metadata.
 * Supports collaboration albums with multiple album artists.
 * Designed to scale to hundreds of thousands of tracks.
 * User interface responds quickly, and indexing is fast.
 * Optimized to run in resource-constrained environments, such as a Raspberry Pi.
 * Responsive design, supports both mobile and desktop.
 * Logarithmic volume control and loudness normalization.
 * Last.fm and ListenBrainz scrobbling.

## Limitations

 * Musium is not a tagger, it expects your files to be tagged correctly already.
 * Supports only flac, with no intention to support other audio formats.
 * Runs on Linux, with no intention to become cross-platform.
 * Uses raw <abbr>ALSA</abbr>, with no intention to support PulseAudio or
   PipeWire.

## Getting started

Follow the [building](building.md) chapter to build from source. Then write a
[configuration file](configuration.md) to `musium.conf`:

    listen = 0.0.0.0:8233
    library_path = /home/user/music
    db_path = /home/user/.config/musium.sqlite3
    audio_device = HDA Intel PCH

Index the library, compute loudness, and generate cover art thumbnails (requires
Imagemagick and Guetzli). Computing loudness and generating thumbnails can take
a long time, but you can already continue and start the server when
`musium scan` is past the indexing stage.

    target/release/musium scan musium.conf

Start the server:

    target/release/musium serve musium.conf

You can now open the library browser at <http://localhost:8233>. See the
[webinterface chapter](webinterface.md) for how to use it.

Musium expects files to be tagged in a particular way, see the
[tagging chapter](tagging.md) for more information.
