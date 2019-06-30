# Mindec

Music metadata indexer.

TODO: Add hipster build status badges.

Mindec is:

 * A library for indexing metadata of a collection of flac files.
 * An http mediaserver that exposes music metadata.
 * A web-based library browser.
 * Designed to run fast in resource-constrained environments
   and scale to hundreds of thousands of tracks.

## Overview

Mindec *the library* can be used to:

 * Power a music player.
 * Build music analytics tools (e.g. to aid analyzing Last.fm playcounts).

Mindec *the server* can be used to:

 * Expose local flac files to the network for e.g. Chromecast playback.
 * Export music metadata as json, for ad-hoc querying with `jq`.
 * Report statistics about your music library and find inconsistencies in tags.

Mindec *the webapp* can be used to:

 * Browse your music library.
 * Play music on Chromecast (not implemented yet).

Mindec is **not**:

 * A tagger. Mindec expects properly tagged flac files. Mindec is picky and
   will complain about inconsistent or missing tags, but it will not fix them
   for you.
 * A database. Mindec treats the music library as read-only, and does not store
   additional data such as playcounts itself.

## Compiling

The library and server are written in [Rust][rust] and build with Cargo:

    cargo build --release
    mkdir /tmp/cover-thumbs
    target/release/mindec cache ~/music /tmp/cover-thumbs
    target/release/mindec serve ~/music /tmp/cover-thumbs

The webapp is written in [Purescript][purescript]:

    cd app
    make
    stat output/app.js

The server will serve `app.js` and other static files alongside the API.

## Querying

List all of your albums, by original release date or by album artist:

    curl localhost:8233/albums | jq 'sort_by(.date)'
    curl localhost:8233/albums | jq 'sort_by(.sort_artist)'

List all album artists, ordered by sort name:

    curl localhost:8233/albums |
      jq 'map({artist, sort_artist})' |
      jq 'unique | sort_by(.sort_artist) | map(.artist)'

## License

Mindec is licensed under the [Apache 2.0][apache2] license. It may be used in
free software as well as closed-source applications, both for commercial and
non-commercial use under the conditions given in the license. If you want to
use Mindec in your GPLv2-licensed software, you can add an [exception][except]
to your copyright notice. Please do not open an issue if you disagree with the
choice of license.

[rust]:       https://rust-lang.org
[purescript]: http://www.purescript.org/
[apache2]:    https://www.apache.org/licenses/LICENSE-2.0
[except]:     https://www.gnu.org/licenses/gpl-faq.html#GPLIncompatibleLibs
