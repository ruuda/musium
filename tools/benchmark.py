#!/usr/bin/env python3

# Musium -- Music playback daemon with web-based library browser
# Copyright 2021 Ruud van Asseldonk
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# A copy of the License has been included in the root of the repository.

"""
benchmark.py -- Benchmark Musium indexing performance.

Usage:

    tools/benchmark.py <outfile> <command...>

Example:

    tools/benchmark.py before.tsv target/release/musium cache musium.conf
"""

import sys
import subprocess

from typing import List, NamedTuple


class Stat(NamedTuple):
    cycles: str
    instructions: str
    seconds_elapsed: str
    seconds_user: str
    seconds_sys: str


def stat(command: List[str]) -> None:
    result = subprocess.run(
        [
            'perf',
            'stat',
            # Prevent thousand separators
            '--no-big-num',
            *command,
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        encoding='utf-8',
    )

    cycles = ''
    instructions = ''
    seconds_elapsed = ''
    seconds_user = ''
    seconds_sys = ''

    # Parse the 'perf stat' output. Perf does have a "pseudo-csv" output mode
    # with --field-separator, but it does not include the total time elapsed,
    # and it is not that much easier to parse anyway.
    for line in result.stderr.splitlines():
        parts = line.split(maxsplit=1)
        if len(parts) == 2:
            value, key = parts
            if key.startswith('cycles:u'):
                cycles = value
            elif key.startswith('instructions:u'):
                instructions = value
            elif key == 'seconds time elapsed':
                seconds_elapsed = value
            elif key == 'seconds user':
                seconds_user = value
            elif key == 'seconds sys':
                seconds_sys = value

    return Stat(cycles, instructions, seconds_elapsed, seconds_user, seconds_sys)


def main(outfile: str, command: List[str]) -> None:
    print(stat(command))


if __name__ == '__main__':
    if len(sys.argv) < 3:
        print(__doc__)
        sys.exit(1)
    else:
        main(sys.argv[1], sys.argv[2:])
