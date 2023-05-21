#!/usr/bin/env python3

# Musium -- Music playback daemon with web-based library browser
# Copyright 2023 Ruud van Asseldonk
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# A copy of the License has been included in the root of the repository.

"""
migrate_thumbnails.py -- Migrate album ids from 0.12.0 to 0.13.0.

This updates the database in-place.

USAGE

 1. Stop Musium 0.12.0.
 2. Make a back-up of your database file.
 3. Run 'tools/migrate_album_ids.py <db.sqlite3>'
 4. Start Musium 0.13.0.
"""

import sqlite3
import sys

from typing import Dict, List, Tuple


def parse_album_id_old(uuid: str) -> int:
    high = int(uuid[:8], base=16)
    low = int(uuid[28:], base=16)
    return (high << 32) | low


def parse_album_id_new(uuid: str) -> int:
    high = int(uuid[:8], base=16)
    low = int(uuid[31:], base=16)
    return (high << 20) | low


def i64_to_u64(x: int) -> int:
    return int.from_bytes(x.to_bytes(length=8, signed=True))


def u64_to_i64(x: int) -> int:
    return int.from_bytes(x.to_bytes(length=8), signed=True)


def main(db_path: str) -> None:
    with sqlite3.connect(db_path) as conn:
        cur = conn.cursor()

        cur.execute(
            """
            select value
            from tags
            where field_name = 'musicbrainz_albumid';
            """
        )
        mbrainz_album_ids = sorted({row[0] for row in cur.fetchall()})
        album_id_map = {
            parse_album_id_old(uuid): parse_album_id_new(uuid)
            for uuid in mbrainz_album_ids
        }

        track_id_map: Dict[int, Tuple[List[int], int, int]] = {
            id_old >> 12: ([], id_old, id_new)
            for id_old, id_new in album_id_map.items()
        }

        cur.execute("select track_id from track_loudness")
        for row in cur.fetchall():
            track_id = i64_to_u64(row[0])
            track_id_map[track_id >> 12][0].append(track_id)

        tuples_album_id: List[Tuple[int, int]] = []
        tuples_track_id: List[Tuple[int, int]] = []

        for track_ids, album_id_old, album_id_new in track_id_map.values():
            if False:
                # Debug print, change condition to enable.
                print(f"{album_id_old:x} -> {album_id_new:x} ({len(track_ids)} tracks)")
            tuples_album_id.append((u64_to_i64(album_id_new), u64_to_i64(album_id_old)))
            for track_id_old in track_ids:
                track_id_new = (track_id_old & 0x0FFF) | (album_id_new << 12)
                tuples_track_id.append(
                    (u64_to_i64(track_id_new), u64_to_i64(track_id_old))
                )

        def run_update(
            i: int, table: str, field: str, tuples: List[Tuple[int, int]]
        ) -> None:
            print(f"[{i}/6] Migrating table {table} ...")
            cur.executemany(
                f"update {table} set {field} = ? where {field} = ?;",
                tuples,
            )

        # We do the loudness tables first, if any id conflicts (which should be
        # rare enough for it not to happen, but it could happen in theory), that
        # will lead to a unique key violation.
        run_update(1, "album_loudness", "album_id", tuples_album_id)
        run_update(2, "track_loudness", "track_id", tuples_track_id)
        run_update(3, "listens", "album_id", tuples_album_id)
        run_update(4, "listens", "track_id", tuples_track_id)
        run_update(5, "thumbnails", "album_id", tuples_album_id)
        run_update(6, "waveforms", "track_id", tuples_track_id)
        conn.commit()
        cur.execute("vacuum")


if __name__ == "__main__":
    if len(sys.argv) == 2:
        main(sys.argv[1])

    else:
        print(__doc__)
        sys.exit(1)
