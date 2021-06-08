# Musium

Musium is an album-centered music player. It is designed to run as a daemon on
an always-on device connected to speakers. Playback can be controlled from
anywhere on the local network through a webinterface.

*Vaporware warning: while Musium is usable, it is missing essential features,
such as the ability to pause playback.*

## Features

 * Respects album artist and original release date metadata.
 * Designed to scale to hundreds of thousands of tracks.
 * User interface responds quickly, and indexing is fast.
 * Optimized to run in resource-constrained environments, such as a Raspberry Pi.
 * Responsive design, supports both mobile and desktop.
 * Logarithmic volume control and loudness normalization.
 * Last.fm and Listenbrainz scrobbling.

## Limitations

 * Musium is not a tagger, it expects your files to be tagged correctly already.
 * Supports only flac, with no intention to support other audio formats.
 * Requires Linux, with no intention to become cross-platform.
 * Uses raw Alsa, with no intention to support PulseAudio.

## Getting started

Follow the [building](building.md) chapter to build from source. Then write a
configuration file to `musium.conf`:

    listen = 0.0.0.0:8233
    library_path = /home/user/music
    covers_path = /home/user/.cache/musium/covers
    audio_device = HDA Intel PCH

Generate cover art thumbnails (requires Imagemagick and Guetzli):

    mkdir -p /home/user/.cache/musium/covers
    target/release/musium cache musium.conf

Start the server:

    target/release/musium serve musium.conf

You can now open the library browser at http://localhost:8233. See the
[webinterface chapter](webinterface.md) for how to use it.

Musium expects files to be tagged in a particular way, see the
[tagging chapter](tagging.md) for more information.
