# Metaindex

Mediaserver for flac files.

TODO: Add hipster build status badges.

Metaindex is:

 * Like `python -m http.server`, but for flac files specifically.
 * An indexer of music metadata to support efficient search.
 * An http server that exposes music metadata.
 * Designed to run fast in resource-constrained environments.

## Compiling

    cargo build --release
    target/release/metaindex ~/music

## Querying

List all of your albums, by original release date or by album artist:

    curl localhost:8233/albums | jq 'sort_by(.date)'
    curl localhost:8233/albums | jq 'sort_by(.sort_artist)'

List all album artists, ordered by sort name:

    curl localhost:8233/albums |
      jq 'map({artist, sort_artist})' |
      jq 'unique | sort_by(.sort_artist) | map(.artist)'
