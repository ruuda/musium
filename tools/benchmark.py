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
    tools/benchmark.py compare <before> <after>

Example:

    tools/benchmark.py record 50 before.tsv target/release/musium cache musium.conf
    tools/benchmark.py compare before.tsv after.tsv
"""

import subprocess
import sys

import numpy as np
import scipy.stats as stats

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


def record(n_iters: int, outfile: str, command: List[str]) -> None:
    stats: List[Stat] = []
    for i in range(n_iters):
        print(f'\r[{i+1} / {n_iters}] Running {command}', end='', flush=True)
        stats.append(perf_stat(command))

    with open(outfile, 'w', encoding='utf-8') as f:
        f.write('cycles\tinstructions\tseconds_elapsed\tseconds_user\tseconds_sys\n')
        for s in stats:
            f.write('\t'.join(s) + '\n')

    print(f'\nResults written to {outfile}.')


def load_tsv(fname: str) -> List[Stat]:
    with open(fname, 'r', encoding='utf-8') as f:
        lines = iter(f)
        # Read the first line and check that the header is as expected.
        # Cut off the trailing newline.
        header = Stat(*next(lines)[:-1].split('\t'))
        assert header == Stat(
            cycles='cycles',
            instructions='instructions',
            seconds_elapsed='seconds_elapsed',
            seconds_user='seconds_user',
            seconds_sys='seconds_sys',
        )

        return [Stat(*line[:-1].split('\t')) for line in lines]


def print_u_test(befores: np.array, afters: np.array) -> None:
    # Perform a two-sided Mann-Whitney U-test to test the following null
    # hypothesis: `befores` and `afters` are two samples drawn from the same
    # population. The alternative hypothesis, is that they are drawn from a
    # different distribution. The Mann-Whitney U-test is similar to
    # Student's t-test, but it is more robust to outliers, because it is based
    # on relative ranking, rather than mean and variance.
    test = stats.mannwhitneyu(befores, afters, alternative='two-sided')

    # The U-statistic is defined as the number of times that a value from
    # `afters` exceeds a value from `befores` (with 0.5 for ties), therefore
    # the maximal value is the product of the lengths. The quotient of the
    # U-statistic and its maximal value is also called the "effect size". A
    # value of 1 means that all `befores` were smaller than all `afters`, a
    # value of 0 means that all `afters` were smaller than all `befores`.
    max_statistic = float(len(befores) * len(afters))
    effect_size = 1.0 - test.statistic / max_statistic

    # The p-value is the probability of observing an effect size at least as
    # far from 0.5 as we did, under the assumption that the null hypothesis
    # is true.
    print('  Mann–Whitney U test')
    print('  null hypothesis: distributions before and after are equal')
    print(f'  effect size:    {effect_size:>10.7f}')
    print(f'  p-value:        {test.pvalue:>10.7f}')

    # Measurements are automated and fast; we can have much higher standards
    # than p=0.05 here.
    if test.pvalue < 0.0001:
        # If the effect is significant enough, also print the differences and
        # ratios of the means, together with an estimate of the error in those,
        # based on the MAD (median absolute deviation) and standard error
        # propagation formulas.
        med_before = np.median(befores)
        med_after = np.median(afters)
        mad_before = np.median(np.abs(befores - med_before))
        mad_after = np.median(np.abs(afters - med_after))

        diff = med_after - med_before
        # The squared error in A + B is the sum of the squared errors.
        diff_err = np.sqrt(np.square(mad_before) + np.square(mad_after))

        ratio = med_after / med_before
        # The relative squared error in A / B is the sum of the squared relative
        # errors.
        ratio_err = ratio * np.sqrt(
            np.square(mad_before / med_before) + np.square(mad_after / med_after)
        )

        print(f'  after - before: {diff:>10,.3f} ± {diff_err:.3f}')
        print(f'  after / before: {ratio:>10.3f} ± {ratio_err:.3f}')

    else:
        print('  insufficient evidence to reject the null hypothesis')


def compare(before_file: str, after_file: str) -> None:
    befores = load_tsv(before_file)
    afters = load_tsv(after_file)

    print('Instructions\n')
    print_u_test(
        np.array([float(s.instructions) for s in befores]),
        np.array([float(s.instructions) for s in afters]),
    )

    print('\nSeconds elapsed\n')
    print_u_test(
        np.array([float(s.seconds_elapsed) for s in befores]),
        np.array([float(s.seconds_elapsed) for s in afters]),
    )


if __name__ == '__main__':
    if len(sys.argv) < 4:
        print(__doc__)
        sys.exit(1)
    else:
        cmd = sys.argv[1]
        if cmd == 'record':
            record(int(sys.argv[2]), sys.argv[3], sys.argv[4:])

        elif cmd == 'compare':
            compare(sys.argv[2], sys.argv[3])

        else:
            print('Invalid command:', cmd)
            sys.exit(1)
