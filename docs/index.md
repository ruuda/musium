# Musium

Music playback daemon with a web-based library browser.

Musium is an album-centered music player designed to run on an always-on device
connected to speakers. Playback can be controlled from anywhere on the local
network through the webinterface.

## Features

 * Respects album artist and original release date metadata.
 * Designed to scale to hundreds of thousands of tracks.
 * User interface responds quickly, and indexing is fast.
 * Optimized to run in resource-constrained environments, such as a Raspberry Pi.
 * Responsive design, supports both mobile and desktop.

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

You can now open the library browser at http://localhost:8233.

Musium expects files to be tagged in a particular way, see the
[tagging](tagging.md) chapter for more information.
