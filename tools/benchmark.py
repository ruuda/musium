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

    tools/benchmark.py record <n_iters> <outfile> <command...>

Example:

    tools/benchmark.py record 50 before.tsv target/release/musium cache musium.conf
"""

import statistics
import subprocess
import sys

from typing import List, NamedTuple


class Stat(NamedTuple):
    cycles: str
    instructions: str
    seconds_elapsed: str
    seconds_user: str
    seconds_sys: str


def perf_stat(command: List[str]) -> None:
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


def main(n_iters: int, outfile: str, command: List[str]) -> None:
    stats: List[Stat] = []
    for i in range(n_iters):
        print(f'\r[{i+1} / {n_iters}] Running {command}', end='', flush=True)
        stats.append(perf_stat(command))

    with open(outfile, 'w', encoding='utf-8') as f:
        f.write('cycles\tinstructions\tseconds_elapsed\tseconds_user\tseconds_sys\n')
        for s in stats:
            f.write('\t'.join(s) + '\n')

    print(f'\nResults written to {outfile}.')
    print('Median instructions:', statistics.median(int(s.instructions) for s in stats))
    print('Median seconds:     ', statistics.median(float(s.seconds_elapsed) for s in stats))


if __name__ == '__main__':
    if len(sys.argv) < 5:
        print(__doc__)
        sys.exit(1)
    else:
        cmd = sys.argv[1]
        if cmd == 'record':
            main(int(sys.argv[2]), sys.argv[3], sys.argv[4:])
        else:
            print('Invalid command:', cmd)
            sys.exit(1)
