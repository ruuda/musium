# Performance

## Disk IO

Should files be read from multiple threads, even when the disk is the
bottleneck? By having multiple concurrent reads, the operating system might be
able to optimize the disk access pattern, and schedule reads more efficiently
for higher throughput. Let’s measure.

| Disk Cache | Threads | Time (seconds)  |
| ---------- | ------- | --------------- |
| Cold       |      64 |   106.632476927 |
| Cold       |      64 |   106.155341479 |
| Cold       |      64 |   104.968957864 |
| Warm       |      64 |     0.065452067 |
| Warm       |      64 |     0.065966143 |
| Warm       |      64 |     0.067338459 |
| Cold       |       6 |   109.032390370 |
| Cold       |       6 |   108.156613210 |
| Cold       |       6 |   110.175107966 |
| Warm       |       6 |     0.056552910 |
| Warm       |       6 |     0.051793717 |
| Warm       |       6 |     0.057326269 |
| Warm       |       6 |     0.056153033 |
| Cold       |       1 |   131.265989187 |
| Cold       |       1 |   130.512200200 |
| Cold       |       1 |   130.496186066 |
| Warm       |       1 |     0.145899503 |
| Warm       |       1 |     0.140550669 |
| Warm       |       1 |     0.140376767 |
| Warm       |       1 |     0.146533344 |

This is for roughly 11500 files. Program output was redirected to /dev/null.
Single-threaded taken from commit `4a5982ceb94b6a3dc575abce3c47e148dd28aa9f`.
Multi-threaded taken from commit `cc06a48af7c8ea8b8b647443db1a26f77374f9e4`.

Conclusion: multithreaded ingestion is advantageous, both when indexing from the
disk, as well as when indexing from memory. There must be a CPU-bound part as
well then. (On my system, for my workload, that is.) The next question then, is
how much threads to use. 64 threads probably already takes all of the parallel
gains, and the returns diminish quickly. It could be worth optimizing the number
of threads for running time with a warm disk cache, and that would likely also
perform almost optimally for a cold cache.

Some more results after reducing the thread queue size and doing non-blocking
pushes, to keep the queue sizes more even:

| Disk Cache | Threads | Queue Size | Time (seconds)  |
| ---------- | ------- | ---------- | --------------- |
| Cold       |     128 |         16 |   105.591609927 |
| Cold       |     512 |          0 |    97.509055644 |
| Cold       |     512 |          0 |    96.345510293 |
| Cold       |     128 |          1 |    94.403741744 |
| Cold       |     128 |          0 |    85.897972147 |
| Cold       |      64 |          0 |    82.595254011 |
| Cold       |      64 |          0 |    83.793832797 |
| Cold       |      48 |          0 |    80.877349368 |
| Cold       |      32 |          0 |    80.913407455 |
| Cold       |      24 |          0 |    82.893433723 |
| Cold       |      16 |          0 |    83.807142608 |
| Cold       |      16 |          0 |    83.967152892 |
| Warm       |     128 |         16 |     0.075636796 |
| Warm       |     128 |          1 |     0.072041480 |
| Warm       |     128 |          0 |     0.075571860 |

And without queues or channels, protecting the directory iterator with a mutex
instead:

| Disk Cache | Threads | Time (seconds)  |
| ---------- | ------- | --------------- |
| Cold       |      48 |    83.731602753 |
| Cold       |      48 |    83.806947689 |
| Cold       |      24 |    81.919455988 |
| Cold       |      24 |    80.765494864 |
| Cold       |      12 |    82.537088779 |
| Cold       |      12 |    83.135829488 |
| Warm       |      48 |     0.056744610 |
| Warm       |      24 |     0.059594100 |
| Warm       |      24 |     0.054264233 |
| Warm       |      12 |     0.056491306 |
| Warm       |      12 |     0.056685518 |

## Precollect

At commit `c6c611be9179d939dc5646dc43ab8bdf5ddc2962`, with 24 threads. First
collecting discovered paths into a vec, and constructing the index by iterating
over the paths in the vec. Is this the right thing to do, or should we put the
paths iterator in a mutex directly? Measurement setup:

    echo 3 | sudo tee /proc/sys/vm/drop_caches
    perf stat target/release/mindec ~/music

Note that the server was disabled to terminate the program after indexing. Also,
these results are not comparable to the previous numbers, as the library has
grown, and more data is processed. Furthermore, I did not redirect stdout to
`/dev/null` in this case, but for a cold disk cache that does not make so much
of a difference anyway.

| Precollect         | Time (seconds)  |
| ------------------ | --------------- |
| Vec precollect 1   |    91.870704962 |
| Vec precollect 1   |    90.106878818 |
| Vec precollect 1   |    90.031705480 |
| Vec precollect 2   |    86.926306901 |
| Vec precollect 2   |    86.876997701 |
| Vec precollect 2   |    89.131675265 |
| Iter, double alloc |    93.370680604 |
| Iter, double alloc |    93.180283609 |
| Iter, double alloc |    93.259494622 |
| Iter, single alloc |    94.026253229 |
| Iter, single alloc |    94.147137607 |
| Iter, single alloc |    94.352803977 |

Note that I did upgrade Walkdir when switching from vector precollect to the
iterator-based version, so the comparison may be unfair. The data collected
before the switch is labelled “Vec precollect 1”, the version after upgrading to
Walkdir 2.1.4 is labelled “Vec precollect 2”. Furtherore, Walkdir 2.1.4 requires
copying the path (labelled “double alloc”). I made a small change to the crate
to be able to avoid the copy and extra allocation (labelled “single alloc”).

Counterintuitively, copying the path returned by the iterator is faster than not
copying it. It might have something to do with ordering; spending more time in
the iterator lock is actually a good thing? Or maybe I should collect more data,
and this is just a statistical fluctuation. Just storing the paths is definitely
faster if the copy is avoided:

    copy    <- c(0.023223363, 0.022365082, 0.022318216, 0.022584837,
                 0.020660742, 0.023839308, 0.022084252, 0.021812114,
                 0.022180668, 0.019982074, 0.020979151, 0.023186709,
                 0.024758619, 0.022889618, 0.024148854, 0.024708654)
    noncopy <- c(0.022403112, 0.021863389, 0.019650964, 0.020984869,
                 0.021901483, 0.021376926, 0.021668108, 0.021504715,
                 0.023730031, 0.021861766, 0.021060567, 0.021986531,
                 0.022680138, 0.019719019, 0.020053399, 0.021137137)
    t.test(copy, noncopy)

    #     Welch Two Sample t-test
    #
    # data:  copy and noncopy
    # t = 2.6055, df = 28.297, p-value = 0.01447
    # alternative hypothesis: true difference in means is not equal to 0
    # 95 percent confidence interval:
    #  0.000242829 0.002024684
    # sample estimates:
    #  mean of x  mean of y
    # 0.02260764 0.02147388

So it is preferable to read many paths at once before processing them, perhaps
due to better branch prediction. The gains are so big that the extra allocations
and reallocations for storing the pathbuf pointers in a vec are totally worth
it. It might be even better then to alternate beween scanning paths and
processing them, to reduce peak memory usage, but let’s not worry about that at
this point.

## Fadvise

Command:

    echo 3 | sudo tee /proc/sys/vm/drop_caches
    perf stat target/release/mindec cache /pool/music /pool/volatile/covers dummy

Measurements were performed with disks spinning. If the disks needed to spin up
first, I restarted the measurement as soon as the disk was spinning.

Baseline, commit `bcb01aac03b72c6250823d44d2b4dd71887e387c`:

| Disk Cache | Tracks | Wall time (seconds) | User time (seconds) | Sys time (seconds |
| ---------- | ------ | ------------------- | ------------------- | ----------------- |
| Cold       |  15931 |       142.662233261 |         3.283129000 |       8.579975000 |
| Cold       |  15931 |       147.348811539 |         3.236058000 |       8.641414000 |
| Cold       |  15931 |       145.916103563 |         3.376106000 |       8.547039000 |
| Warm       |  15931 |         0.346267741 |         0.987189000 |       0.427480000 |
| Warm       |  15931 |         0.369951824 |         0.886352000 |       0.523628000 |
| Warm       |  15931 |         0.372806305 |         0.929290000 |       0.480558000 |

Open files first, read later, commit `0f2d00be7ef2009fe19af79ae02ac29d11c766cf`:

| Disk Cache | Tracks | Wall time (seconds) | User time (seconds) | Sys time (seconds |
| ---------- | ------ | ------------------- | ------------------- | ----------------- |
| Cold       |  15931 |       200.334320084 |         4.513103000 |      10.766578000 |
| Warm       |  15931 |         0.835945466 |         2.131593000 |       2.420703000 |

Use “frontier” read pattern, commit `64371ff0aa834add77185531bae7160cfd6134ad`:

| Disk Cache | Tracks | Wall time (seconds) | User time (seconds) | Sys time (seconds |
| ---------- | ------ | ------------------- | ------------------- | ----------------- |
| Cold       |  15931 |       148.444013742 |         4.524398000 |      10.423234000 |
| Cold       |  15931 |       147.144940804 |         4.670934000 |      10.321421000 |
| Warm       |  15931 |         1.134759625 |         2.831797000 |       4.271261000 |
| Warm       |  15931 |         1.204304762 |         3.183732000 |       4.562911000 |

After changing the IO queue size (and also tuning internal queue a bit), this
could be brought down to 93 seconds, which suggests the win is really more in IO
patterns, and for the warm case, simpler is probably better.


    $ cat /sys/block/sd{b,c,d}/queue/nr_requests
    4
    4
    4
    $ echo 2048 | sudo tee /sys/block/sd{b,c,d}/queue/nr_requests
    2048
