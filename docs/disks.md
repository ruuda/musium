# Disks

Musium is built to handle music libraries in the order of terabytes in size.
Spinning disks are still the most cost-effective storage medium at this scale,
but storing your music on a spinning disk has downsides:

 * Disks are slow, with high latency for reads, e.g. when starting a track.
 * Disks are noisy, you can hear them spin up, or hear them rattle when seeking.

Especially for a music player, these can be a nuisance.

Furthermore, disks can spin down to save power when unused. While this does
reduce audible noise, it also causes access latencies of dozens of seconds when
accessing the disk after a period of inactivity.

## Disk optimizations

To optimize for disks that aggressively try to spin down, Musium takes the
following actions:

 * Load all resources required to serve the webinterface into memory (aside from
   full-resolution cover art), to enable browsing the library without disk
   access.
 * Decode in bursts, and buffer about 10 minutes of audio in memory. When the
   play queue is full, this means that the disk only has to spin up about once
   every 10 minutes, and it can be silent in the meantime. The cost for 10
   minutes of 16-bit, 44.1 kHz stereo audio is about 105 MB of memory, which is
   still acceptable even on a Raspberry Pi.
 * Resume decoding well in time to allow for the disk to spin up before the
   buffer runs out.

## The play database

Musium keeps a record of which songs you played in a database. It writes to this
database every time a new track starts playing. When you keep this database on
a spinning disk, that undermines the above optimizations. Fortunately, the
database should not be terabytes in size, so it can easily be kept on
solid-state storage, for example on the <abbr>SD</abbr> card of a Raspberry Pi.
