# Performance

## Disk IO

Should files be read from multiple threads, even when the disk is the
bottleneck? By having multiple concurrent reads, the operating system might be
able to optimize the disk access pattern, and schedule reads more efficiently
for higher throughput. Let's measure.

Disk Cache  Threads  Time (seconds)
----------  -------  ---------------
Cold             64    106.632476927
Cold             64    106.155341479
Cold             64    104.968957864
Warm             64      0.065452067
Warm             64      0.065966143
Warm             64      0.067338459
Cold              6    109.032390370
Cold              6    108.156613210
Cold              6    110.175107966
Warm              6      0.056552910
Warm              6      0.051793717
Warm              6      0.057326269
Warm              6      0.056153033
Cold              1    131.265989187
Cold              1    130.512200200
Cold              1    130.496186066
Warm              1      0.145899503
Warm              1      0.140550669
Warm              1      0.140376767
Warm              1      0.146533344

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

Disk Cache  Threads  Queue Size  Time (seconds)
----------  -------  ----------  ---------------
Cold            128          16    105.591609927
Cold            512           0     97.509055644
Cold            512           0     96.345510293
Cold            128           1     94.403741744
Cold            128           0     85.897972147
Cold             64           0     82.595254011
Cold             64           0     83.793832797
Cold             48           0     80.877349368
Cold             32           0     80.913407455
Cold             24           0     82.893433723
Cold             16           0     83.807142608
Cold             16           0     83.967152892
Warm            128          16      0.075636796
Warm            128           1      0.072041480
Warm            128           0      0.075571860
