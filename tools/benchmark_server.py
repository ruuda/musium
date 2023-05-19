#!/usr/bin/env python3

# Musium -- Music playback daemon with web-based library browser
# Copyright 2023 Ruud van Asseldonk
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# A copy of the License has been included in the root of the repository.

"""
benchmark_server.py -- Benchmark Musium API server performance.
"""

from http.client import HTTPConnection
from typing import List

import json
import random
import time

AlbumId = str

def load_albums(conn: HTTPConnection) -> List[AlbumId]:
    conn.request("GET", "/api/albums")
    albums = json.load(conn.getresponse())
    return [album["id"] for album in albums]


def measure_get_all(conn: HTTPConnection, albums: List[AlbumId]) -> List[float]:
    random.shuffle(albums)
    chunk_size = 10

    for i in range(0, len(albums), chunk_size):
        ids = albums[i:i + chunk_size]
        t0_sec = time.monotonic()

        for album_id in ids:
            conn.request("GET", f"/api/album/{album_id}")
            response = conn.getresponse()
            response.read()
            assert not response.closed

        t1_sec = time.monotonic()
        print(f"[{i:4}/{len(albums)}] {t1_sec - t0_sec:.3f}s")


if __name__ == "__main__":
    conn = HTTPConnection("localhost:8233")
    albums = load_albums(conn)
    measure_get_all(conn, albums)
