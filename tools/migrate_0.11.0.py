#!/usr/bin/env python3

# Musium -- Music playback daemon with web-based library browser
# Copyright 2023 Ruud van Asseldonk
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# A copy of the License has been included in the root of the repository.

"""
migrate_0.11.0.py -- Migrate database from 0.10.1 to 0.11.0.

To migrate the database, back up the old database to a safe location, and ensure
that no file exists at the configured location. Then run "musium scan" with the
new version. This will create the database and populate it with files. Let the
loudness analysis and cover art scanning finish. (This is a bit unfortunate, but
letting this run for a few hours in the background is easier than sorting out
the migration.) Once the new scan is complete, run this script to migrate
listens and import times.

USAGE

  tools/migrate_0.11.0.py <old.sqlite3> <new.sqlite3>

  <old.sqlite3>   Path to the v0.10.1 Musium sqlite3 database.
  <new.sqlite3>   Path to the v0.11.1 Musium sqlite3 database.
"""

import sqlite3
import sys


def main(old_path: str, new_path: str) -> None:
    with sqlite3.connect(old_path) as conn_old:
        with sqlite3.connect(new_path) as conn_new:
            co = conn_old.cursor()
            cn = conn_new.cursor()

            # Migrate the import timestamps.
            co.execute("select imported_at, filename, mtime from file_metadata;")
            data = co.fetchall()
            cn.executemany(
                """
                update
                  files
                set
                  imported_at = ?
                where
                  filename = ? and mtime = ?;
                """,
                data,
            )
            conn_new.commit()

            # Migrate the listens.
            co.execute(
                """
                select
                    started_at
                  , completed_at
                  , queue_id
                  , track_id
                  , album_id
                  , album_artist_id
                  , track_title
                  , album_title
                  , track_artist
                  , album_artist
                  , duration_seconds
                  , track_number
                  , disc_number
                  , source
                  , scrobbled_at
                from
                  listens
                order by
                  id asc;
                """
            )
            data = co.fetchall()
            cn.executemany(
                """
                insert into listens
                  ( started_at
                  , completed_at
                  , queue_id
                  , track_id
                  , album_id
                  , album_artist_id
                  , track_title
                  , album_title
                  , track_artist
                  , album_artist
                  , duration_seconds
                  , track_number
                  , disc_number
                  , source
                  , scrobbled_at
                  )
                values
                  (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);
                """,
                data,
            )
            conn_new.commit()


if __name__ == "__main__":
    if len(sys.argv) == 3:
        main(sys.argv[1], sys.argv[2])

    else:
        print(__doc__)
        sys.exit(1)
