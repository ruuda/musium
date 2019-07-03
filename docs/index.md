# Mindec

Music metadata indexer and mediaserver.

Mindec is an <abbr>HTTP</abbr> server that exposes a collection of flac files
and their metadata to the local network, with an <abbr>API</abbr> to query the
library. Mindec is designed to scale to hundreds of thousands of tracks, and it
can run in resource-constrained environments such as a Raspberry Pi.

Built upon the server is a web-based library browser that can play back tracks
on a Chromecast.

## Getting started

Mindec needs to be built from source. See the [building](building.md) chapter
of the docs. It expects files to be tagged in a particular way, see the
[tagging](tagging.md) chapter for more information.
