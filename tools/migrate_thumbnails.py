#!/usr/bin/env python3

# Musium -- Music playback daemon with web-based library browser
# Copyright 2023 Ruud van Asseldonk
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# A copy of the License has been included in the root of the repository.

"""
migrate_thumbnails.py -- Import external thumbnails into the database.

Previously Musium stored thumbnails in the file system. Recently it started
storing thumbnails in the database instead. (Shortly after a5e953bc76ad2eaa).
To avoid having to regenerate all thumbnails -- which would be costly -- we can
import them using this script.

USAGE

  tools/migrate_thumbnails.py <database> <covers-dir>

  <database>     Path to the Musium sqlite3 database.
  <covers-dir>   Path to the covers directory that stores the thumbnails.
"""

import os
import os.path
import sqlite3
import sys

from typing import List, Tuple


def main(db_path: str, covers_path: str) -> None:
    insert_params: List[Tuple[int, bytes]] = []

    for fname in os.listdir(covers_path):
        if not fname.endswith(".jpg"):
            continue

        album_id_hex = os.path.splitext(fname)[0]
        album_id = int.from_bytes(
            bytes.fromhex(album_id_hex),
            byteorder="big",
            signed=True,
        )

        full_fname = os.path.join(covers_path, fname)
        with open(full_fname, "rb") as f:
            data = f.read()
            insert_params.append((album_id, data))

    with sqlite3.connect(db_path) as connection:
        connection.executemany(
            "insert into thumbnails (album_id, data) values (?, ?);",
            insert_params,
        )
        connection.commit()
        print(f"Inserted {len(insert_params)} thumbnails.")


if __name__ == "__main__":
    if len(sys.argv) == 3:
        main(sys.argv[1], sys.argv[2])

    else:
        print(__doc__)
        sys.exit(1)
