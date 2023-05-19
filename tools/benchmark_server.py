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
import sys

AlbumId = str


def load_albums(host: str, port: int) -> List[AlbumId]:
    conn = HTTPConnection(host, port)
    conn.request("GET", "/api/albums")
    albums = json.load(conn.getresponse())
    conn.close()
    return [album["id"] for album in albums]


def measure_get_all(albums: List[AlbumId], host: str, port: int) -> None:
    """
    Benchmark how long it takes to request every individual album, in a random
    order. We send all requests at once, pipelined, directly to a socket to
    avoid Python overhead, and then we read until the end.
    """
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
            # We only get no data when the socket is closed, which should be
            # after the last request.
            if len(data) == 0:
                break

        t1_sec = time.monotonic()
        print(f"{t1_sec - t0_sec:.6f}")


def record() -> None:
    host = "localhost"
    port = 8233
    albums = load_albums(host, port)
    for _ in range(1000):
        measure_get_all(albums, host, port)


def report(f1: str, f2: str) -> None:
    from matplotlib import gridspec, pyplot as plt
    from scipy import stats
    import numpy as np

    with open(f1, encoding="ascii") as f:
        xs = np.array([float(x) for x in f])

    with open(f2, encoding="ascii") as f:
        ys = np.array([float(y) for y in f])

    def summarize(zs: np.ndarray) -> None:
        m = np.mean(zs)
        sd = np.std(zs)
        print(f"{m:.6f} ± {sd:.6f} s")

    summarize(xs)
    summarize(ys)

    fig = plt.figure(tight_layout=True)
    gs = gridspec.GridSpec(2, 2)

    ax = fig.add_subplot(gs[0, 0])
    z = 0.5 * (np.median(xs) + np.median(ys))
    ax.axhline(z, color="black", alpha=0.2)
    ax.scatter(np.linspace(0, 1, len(xs)), xs, s=1.5)
    ax.scatter(np.linspace(1, 2, len(ys)), ys, s=1.5)

    cmap = plt.get_cmap("tab10")
    for i, zs in enumerate([xs, ys]):
        ax = fig.add_subplot(gs[1, i])
        ax.hist(zs, bins=50, color=cmap(i))

    plt.show()
    print(stats.mannwhitneyu(xs, ys))


if __name__ == "__main__":
    if sys.argv[1] == "record":
        record()

    if sys.argv[1] == "report":
        report(sys.argv[2], sys.argv[3])
