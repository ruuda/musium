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

import gc
import json
import random
import socket
import sys
import time

AlbumId = str


def load_albums(host: str, port: int) -> List[AlbumId]:
    conn = HTTPConnection(host, port)
    conn.request("GET", "/api/albums")
    albums = json.load(conn.getresponse())
    conn.close()
    return [album["id"] for album in albums]


def measure_get_all_seconds(albums: List[AlbumId], host: str, port: int) -> float:
    """
    Benchmark how long it takes to request every individual album, in a random
    order. We send all requests at once, pipelined, directly to a socket to
    avoid Python overhead, and then we read until the end.

    Returns the time in seconds.
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

        # Force a GC now, so it doesn't bother us in the middle of the
        # measurement, should it be needed.
        gc.collect()
        gc.disable()

        t0_sec = time.monotonic()
        sock.sendall(b"".join(requests))
        while True:
            data = sock.recv(8192)
            # We only get no data when the socket is closed, which should be
            # after the last request.
            if len(data) == 0:
                break

        t1_sec = time.monotonic()
        gc.enable()

    return t1_sec - t0_sec


def record() -> None:
    host = "localhost"
    port = 8233
    albums = load_albums(host, port)

    # Perform a few warmup rounds. Maybe the Python socket module takes some
    # time to be imported when we use it for the first time, maybe the parts of
    # the server binary that handle this endpoint need to be paged in from disk
    # ... I don't know the exact reason, but the first measurement was always
    # an outlier. To be sure, let's do a few more rounds of warmup before the
    # start.
    for _ in range(5):
        _ = measure_get_all_seconds(albums, host, port)

    for _ in range(1000):
        duration_sec = measure_get_all_seconds(albums, host, port)
        print(f"{duration_sec:.6f}")


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
    gs = gridspec.GridSpec(3, 2)

    xs_ys = np.concatenate((xs, ys))
    p005, p995 = np.quantile(xs_ys, [0.005, 0.995])

    ax = fig.add_subplot(gs[2, 1])
    z = 0.5 * (np.median(xs) + np.median(ys))
    ax.axhline(z, color="black", alpha=0.4, linewidth=1.0)
    # Position the points sequentially. We don't devote half the space to each
    # data set, but instead space proportional to the size, so you can visually
    # see when there is a mismatch in the size of the data.
    ax.scatter(np.arange(0, len(xs)), xs, s=1.3)
    ax.scatter(np.arange(len(xs), len(xs) + len(ys)), ys, s=1.3)
    # Unset the axis labels, it's just the index, this is not meaningful aside
    # from the order.
    ax.set_xticks([])
    ax.set_xlabel("iteration")
    ax.set_ylabel("duration (seconds)")

    cmap = plt.get_cmap("tab10")
    hist_bins = np.linspace(p005, p995, 25)
    x_axis = None
    for i in range(2):
        zs_now = [xs, ys][i]
        zs_alt = [xs, ys][1 - i]
        ax = fig.add_subplot(gs[i, 0], sharex=x_axis)
        ax.hist(zs_now, bins=hist_bins, color=cmap(i))
        ax.hist(
            zs_alt,
            bins=hist_bins,
            edgecolor=cmap(1 - i),
            linewidth=1.0,
            histtype="step",
        )
        # We don't need labels on the y-axis, whether it is frequency or count,
        # what matters is the shape of the distribution.
        ax.axes.get_yaxis().set_visible(False)
        x_axis = ax

    ax = fig.add_subplot(gs[2, 0], sharex=x_axis)
    print(np.mean(xs))
    mean_a, std_a = np.mean(xs), np.std(xs)
    mean_b, std_b = np.mean(ys), np.std(ys)
    bar_a = ax.barh(2.0, mean_a, xerr=std_a)
    bar_b = ax.barh(1.0, mean_b, xerr=std_b)
    ax.set_xlim(p005, p995)
    # We don't need labels on the bars, the entire plot is color-coded.
    ax.axes.get_yaxis().set_visible(False)
    ax.set_xlabel("duration (seconds, mean ± stddev)")
    ax.legend([bar_a, bar_b], ["A", "B"])

    ax = fig.add_subplot(gs[0, 1])
    ax.text(
        0.5, 0.66, "B duration as percentage of A:", ha="center", va="center", size=10
    )
    rel_mean = mean_b / mean_a
    rel_std = rel_mean * np.sqrt((std_a / mean_a) ** 2 + (std_b / mean_b) ** 2)
    ax.text(
        0.5, 0.33, f"{rel_mean:.2%} ± {rel_std:.2%}", ha="center", va="center", size=15
    )
    ax.set_axis_off()

    ax = fig.add_subplot(gs[1, 1])
    utest = stats.mannwhitneyu(xs, ys)
    ax.text(
        0.5,
        0.65,
        "H0: Samples A and B are drawn\nfrom the same distribution.",
        ha="center",
        va="center",
        size=10,
    )
    ax.text(
        0.5, 0.25, f"p-value: {utest.pvalue:.2g}", ha="center", va="center", size=10
    )
    ax.set_axis_off()

    fig.suptitle(f"A = {f1} vs. B = {f2}")
    plt.show()


if __name__ == "__main__":
    if sys.argv[1] == "record":
        record()

    if sys.argv[1] == "report":
        report(sys.argv[2], sys.argv[3])
