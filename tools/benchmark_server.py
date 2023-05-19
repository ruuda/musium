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
import socket

AlbumId = str


def load_albums(host: str, port: int) -> List[AlbumId]:
    conn = HTTPConnection(host, port)
    conn.request("GET", "/api/albums")
    albums = json.load(conn.getresponse())
    conn.close()
    return [album["id"] for album in albums]


def measure_get_all(albums: List[AlbumId], host: str, port: int) -> None:
    random.shuffle(albums)

    requests = []

    for i, album_id in enumerate(albums):
        is_last = i == len(albums) - 1
        conn_header = b"close" if is_last else b"keep-alive"

        requests.append(
            b"GET /api/album/" + album_id.encode("utf-8") + b" HTTP/1.1\r\n"
            b"Connection: " + conn_header + b"\r\n\r\n"
        )

    with socket.socket() as sock:
        sock.connect((host, port))

        t0_sec = time.monotonic()
        sock.sendall(b"".join(requests))
        while True:
            data = sock.recv(8192)
            if len(data) == 0:
                break

        t1_sec = time.monotonic()
        print(f"{t1_sec - t0_sec:.6f}")


def main() -> None:
    host = "localhost"
    port = 8233
    albums = load_albums(host, port)
    for _ in range(1000):
        measure_get_all(albums, host, port)


if __name__ == "__main__":
    main()
