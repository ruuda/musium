# Mindec

Music player daemon with web-based library browser.

[![Build Status][ci-img]][ci]

Mindec is a music player that can be controlled through a web-based library
browser. It indexes a collection of flac files, and exposes playback controls to
the local network. It is intended to run on a low-power always-on device, such
as a Raspberry Pi, that has audio out connected to speakers. Playback on those
speakers can then be controlled from any device on the local network.

Mindec is designed to run fast in resource-constrained environments and scale to
hundreds of thousands of tracks.

## Overview

Mindec consists of a few components:

 * A library that can index a collection of flac files, to support operations
   such as listing all tracks in an album, and to accelerate search. The index
   is not backed by a general-purpose database, it is a domain-specific data
   structure.

 * A player that can decode and play back a queue of tracks through
   [Claxon][claxon] and [Alsa][alsa-rs].

 * An http server that exposes the index as json, and endpoints to control
   the player. It also serves the static content for the library browser.

 * A web-based library browser.

Mindec is not:

 * A tagger. Mindec expects properly tagged flac files. Mindec is picky and
   will complain about inconsistent or missing tags, but it will not fix them
   for you.

 * A database. Mindec treats the music library as read-only, and does not store
   mutable data such as playcounts itself.

[claxon]:  https://github.com/ruuda/claxon
[alsa-rs]: https://github.com/diwic/alsa-rs

## Compiling

The library browser is written in [Purescript][purescript]. There is a basic
makefile that calls `purs` and `psc-package`:

    make -C app
    stat app/output/app.js

The server will serve `app.js` and other static files alongside the API.

The server is written in [Rust][rust] and builds with Cargo:

    cargo build --release
    mkdir /tmp/cover-thumbs
    target/release/mindec cache ~/music /tmp/cover-thumbs
    target/release/mindec serve ~/music /tmp/cover-thumbs $ALSA_CARD_NAME

## Alternatives

 * [MPD with a web client](https://musicpd.org/clients/#web-clients)
 * [Mopidy with Iris](https://mopidy.com/ext/iris/)

## License

Mindec is licensed under the [Apache 2.0][apache2] license. It may be used in
free software as well as closed-source applications, both for commercial and
non-commercial use under the conditions given in the license. If you want to
use Mindec in your GPLv2-licensed software, you can add an [exception][except]
to your copyright notice. Please do not open an issue if you disagree with the
choice of license.

[ci-img]:     https://travis-ci.org/ruuda/mindec.svg?branch=master
[ci]:         https://travis-ci.org/ruuda/mindec
[rust]:       https://rust-lang.org
[purescript]: http://www.purescript.org/
[apache2]:    https://www.apache.org/licenses/LICENSE-2.0
[except]:     https://www.gnu.org/licenses/gpl-faq.html#GPLIncompatibleLibs
