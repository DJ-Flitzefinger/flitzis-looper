# Measured PCM cache and resident-loop acceptance (C3)

This Windows measurement uses 200 productive occupied pads, real LoaderController restore/poll, native command drain and source ACK, effective saved-loop readiness, sealed complete-source leases and real off-thread retirement. All 24 matrix runs, four unchanged 600-second save runs, two lifecycle runs and six actual native pool runs passed independent packet/runtime gates. One fresh process was measured per condition. These are individual observations, with no statistical confidence or universal speed/memory claim.

The 120-second mono PCM16 sources are 48 kHz, with the same saved 42.0..42.5-second loop. Shared-path uses one original/digest; duplicate-path uses 200 distinct originals with one digest; unique uses 200 originals/digests. Finite storage loads the admitted loop window; full uses the current code's complete PCM and then the same effective loop. Artifact-cold starts without a compatible committed cache. Fresh warm restores the actual successful cold-saved project. OS page cache, scheduling and file-generation/copy effects remain uncontrolled. Source-copy work outside the native process is separately bound by the runner and excluded from native startup counters.

## Startup and readiness

End-to-end is isolated measured probe startup, including the module-import/hash identity instrumentation before embedded Python/engine/pool setup, through all 200 actual effective loops and controller completion. It is not a timing of uninstrumented app/device startup. Load is the narrower controller/load interval. Final EXE/runtime-DLL hashing follows phase snapshots and is excluded from load/render intervals. ACK and loop columns are the last observed timestamps relative to load start. CPU is process kernel+user seconds; CPU/wall is one-core equivalents and can exceed one. Read/write are Win32 logical process transfers, not physical disk traffic. Workers/queue/admitted are observed productive job peaks.

| Condition | End-to-end s | Load s | Last ACK s | Last loop s | CPU s | CPU/wall | Read MiB | Write MiB | Workers/queue/admitted |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| debug-shared_path-finite-cold | 587.751 | 587.215 | 587.079 | 587.081 | 737.844 | 1.255 | 17754.636 | 2263.260 | 2/32/34 |
| debug-shared_path-finite-warm | 580.783 | 580.265 | 580.133 | 580.265 | 731.641 | 1.260 | 17677.961 | 2197.340 | 2/32/34 |
| debug-shared_path-full-cold | 588.911 | 588.408 | 588.271 | 588.272 | 734.578 | 1.247 | 17718.197 | 2263.260 | 2/32/34 |
| debug-shared_path-full-warm | 581.609 | 581.104 | 580.970 | 580.971 | 726.016 | 1.248 | 17685.285 | 2197.339 | 2/32/34 |
| debug-duplicate_path-finite-cold | 595.912 | 595.379 | 595.239 | 595.240 | 871.125 | 1.462 | 17754.636 | 2263.260 | 2/32/34 |
| debug-duplicate_path-finite-warm | 582.808 | 582.314 | 582.172 | 582.173 | 855.375 | 1.468 | 17677.961 | 2197.340 | 2/32/34 |
| debug-duplicate_path-full-cold | 598.819 | 598.303 | 598.163 | 598.164 | 877.797 | 1.466 | 17718.198 | 2263.260 | 2/32/34 |
| debug-duplicate_path-full-warm | 584.261 | 583.740 | 583.600 | 583.602 | 858.359 | 1.469 | 17685.285 | 2197.340 | 2/32/34 |
| debug-unique-finite-cold | 1489.144 | 1488.632 | 1488.473 | 1488.632 | 3469.234 | 2.330 | 33069.476 | 15381.415 | 2/32/34 |
| debug-unique-finite-warm | 420.733 | 420.127 | 419.985 | 419.987 | 847.016 | 2.013 | 17725.253 | 2197.337 | 2/32/34 |
| debug-unique-full-cold | 1650.698 | 1650.135 | 1649.991 | 1649.992 | 3847.500 | 2.331 | 33069.508 | 15381.416 | 2/32/34 |
| debug-unique-full-warm | 468.692 | 468.083 | 467.942 | 467.944 | 947.219 | 2.021 | 26477.694 | 2197.337 | 2/32/34 |
| release-shared_path-finite-cold | 38.369 | 37.804 | 37.540 | 37.804 | 58.422 | 1.523 | 17722.719 | 2263.256 | 2/32/34 |
| release-shared_path-finite-warm | 28.528 | 28.051 | 27.791 | 28.051 | 47.469 | 1.664 | 17646.039 | 2197.336 | 2/32/34 |
| release-shared_path-full-cold | 35.379 | 34.900 | 34.642 | 34.900 | 54.703 | 1.546 | 17686.275 | 2263.256 | 2/32/34 |
| release-shared_path-full-warm | 28.192 | 27.712 | 27.451 | 27.712 | 46.812 | 1.660 | 17653.367 | 2197.335 | 2/32/34 |
| release-duplicate_path-finite-cold | 38.094 | 37.614 | 37.350 | 37.614 | 63.625 | 1.670 | 17722.730 | 2263.257 | 2/32/34 |
| release-duplicate_path-finite-warm | 28.649 | 28.170 | 27.895 | 28.170 | 54.359 | 1.897 | 17646.043 | 2197.336 | 2/32/34 |
| release-duplicate_path-full-cold | 36.711 | 36.209 | 36.038 | 36.209 | 62.625 | 1.706 | 17686.291 | 2263.256 | 2/32/34 |
| release-duplicate_path-full-warm | 28.632 | 28.149 | 28.010 | 28.011 | 53.672 | 1.875 | 17653.362 | 2197.336 | 2/32/34 |
| release-unique-finite-cold | 1017.895 | 1017.408 | 1017.268 | 1017.270 | 2342.781 | 2.302 | 33037.596 | 15381.411 | 2/32/34 |
| release-unique-finite-warm | 33.352 | 32.764 | 32.630 | 32.764 | 81.234 | 2.436 | 17693.368 | 2197.335 | 2/32/34 |
| release-unique-full-cold | 1203.175 | 1202.643 | 1202.496 | 1202.498 | 2761.375 | 2.295 | 33037.593 | 15381.412 | 2/32/34 |
| release-unique-full-warm | 35.469 | 34.862 | 34.717 | 34.719 | 90.109 | 2.540 | 26445.828 | 2197.334 | 2/32/34 |

## PCM ownership and process memory

All memory columns use MiB. Retained PCM deduplicates actual Arc backings in the sampled sample-cache/native-bank/voice owners at readiness. It is distinct from process RAM. The PCM checkpoint is the largest declared observation in a single decode/map/load operation, not an aggregate simultaneous transient-PCM peak or admission guarantee. It is the maximum across the whole case's observed operations, not a startup-only checkpoint. That aggregate is unmeasured. OS peak WS and peak process commit are cumulative process-lifetime maxima through the indicated point, including setup. Whole-case includes later rendering, exceptions, analysis and shutdown; the final runtime-artifact hash read follows that snapshot. Sampled private peaks are polling lower bounds. The bounded observer thread is included. Private after shutdown may retain allocator arenas and service state; releasing a strong owner does not promise zero process RAM.

| Condition | Backings | Retained PCM | Per-op checkpoint | Ready peak WS | Ready peak commit | Ready sampled private | Case peak WS | Case peak commit | After private |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| debug-shared_path-finite-cold | 200 | 36.621 | 65.918 | 348.074 | 347.355 | 347.336 | 384.598 | 1117.891 | 833.742 |
| debug-shared_path-finite-warm | 200 | 36.621 | 43.945 | 282.008 | 280.051 | 280.051 | 715.586 | 1453.246 | 834.793 |
| debug-shared_path-full-cold | 1 | 43.945 | 65.918 | 348.402 | 347.570 | 347.566 | 392.312 | 1125.602 | 841.402 |
| debug-shared_path-full-warm | 1 | 43.945 | 43.945 | 325.246 | 324.445 | 324.406 | 724.406 | 1459.387 | 843.359 |
| debug-duplicate_path-finite-cold | 200 | 36.621 | 65.918 | 349.488 | 348.656 | 348.645 | 385.297 | 1118.562 | 837.297 |
| debug-duplicate_path-finite-warm | 200 | 36.621 | 43.945 | 281.957 | 280.113 | 280.113 | 716.633 | 1457.000 | 835.145 |
| debug-duplicate_path-full-cold | 1 | 43.945 | 65.918 | 348.652 | 347.832 | 347.781 | 392.211 | 1125.355 | 841.469 |
| debug-duplicate_path-full-warm | 1 | 43.945 | 43.945 | 325.320 | 324.492 | 324.469 | 722.406 | 1459.398 | 841.984 |
| debug-unique-finite-cold | 200 | 36.621 | 65.918 | 450.828 | 449.363 | 449.328 | 450.828 | 1118.715 | 835.453 |
| debug-unique-finite-warm | 200 | 36.621 | 43.945 | 282.375 | 280.609 | 280.609 | 716.355 | 1453.477 | 837.121 |
| debug-unique-full-cold | 200 | 8789.062 | 65.918 | 9104.699 | 9120.602 | 9120.566 | 9104.699 | 9845.207 | 9560.211 |
| debug-unique-full-warm | 200 | 8789.062 | 43.945 | 9077.996 | 9093.957 | 9093.910 | 9425.094 | 10182.965 | 9562.539 |
| release-shared_path-finite-cold | 200 | 36.621 | 65.918 | 342.898 | 346.000 | 345.996 | 378.102 | 1116.156 | 833.223 |
| release-shared_path-finite-warm | 200 | 36.621 | 43.945 | 275.625 | 277.852 | 277.852 | 712.762 | 1452.840 | 834.633 |
| release-shared_path-full-cold | 1 | 43.945 | 65.918 | 342.848 | 345.738 | 345.730 | 385.176 | 1123.258 | 838.840 |
| release-shared_path-full-warm | 1 | 43.945 | 43.945 | 319.668 | 323.184 | 323.172 | 736.273 | 1475.777 | 837.836 |
| release-duplicate_path-finite-cold | 200 | 36.621 | 65.918 | 342.121 | 345.070 | 345.051 | 380.484 | 1159.375 | 834.238 |
| release-duplicate_path-finite-warm | 200 | 36.621 | 43.945 | 275.555 | 277.773 | 277.773 | 715.680 | 1455.859 | 834.371 |
| release-duplicate_path-full-cold | 1 | 43.945 | 65.918 | 342.492 | 345.633 | 345.590 | 419.297 | 1123.629 | 835.898 |
| release-duplicate_path-full-warm | 1 | 43.945 | 43.945 | 319.805 | 323.785 | 323.754 | 734.137 | 1474.055 | 838.188 |
| release-unique-finite-cold | 200 | 36.621 | 65.918 | 439.781 | 456.496 | 456.434 | 439.781 | 1116.488 | 832.762 |
| release-unique-finite-warm | 200 | 36.621 | 43.945 | 275.945 | 278.156 | 278.156 | 727.426 | 1466.734 | 834.883 |
| release-unique-full-cold | 200 | 8789.062 | 65.918 | 9098.379 | 9118.410 | 9118.367 | 9098.379 | 9843.926 | 9557.684 |
| release-unique-full-warm | 200 | 8789.062 | 43.945 | 9072.328 | 9092.355 | 9048.281 | 9436.656 | 10194.113 | 9559.316 |

## Actual integrity work and cache footprint

Byte counters below are summed actual assignment work, including repeated validation of shared content. Cache bytes count independently audited distinct on-disk manifest/decoder/playback files. A warm lease still validates complete source/decoder/playback identity before reading a finite resident slice. An unavailable worker CPU field stays unavailable.

| Condition | Cold/warm leases | Copy MiB | Snapshot verify MiB | Original verify MiB | Decoder verify MiB | Playback verify MiB | Selected read MiB | Manifest verify MiB | Assignment copy MiB | Original copy MiB | Distinct cache MiB | Warm worker CPU s |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| debug-shared_path-finite-cold | 1/199 | 2197.274 | 2197.274 | 0.000 | 4416.504 | 8833.008 | 36.438 | 0.478 | 0.000 | 0.000 | 65.920 | 573.562 |
| debug-shared_path-finite-warm | 0/200 | 2197.274 | 2197.274 | 0.000 | 4394.531 | 8789.062 | 36.621 | 0.475 | 0.000 | 0.000 | 65.920 | 579.047 |
| debug-shared_path-full-cold | 1/199 | 2197.274 | 2197.274 | 0.000 | 4416.504 | 8833.008 | 0.000 | 0.478 | 0.000 | 0.000 | 65.920 | 574.875 |
| debug-shared_path-full-warm | 0/200 | 2197.274 | 2197.274 | 0.000 | 4394.531 | 8789.062 | 43.945 | 0.475 | 0.000 | 0.000 | 65.920 | 579.375 |
| debug-duplicate_path-finite-cold | 1/199 | 2197.274 | 2197.274 | 0.000 | 4416.504 | 8833.008 | 36.438 | 0.478 | 0.000 | 0.000 | 65.920 | 578.078 |
| debug-duplicate_path-finite-warm | 0/200 | 2197.274 | 2197.274 | 0.000 | 4394.531 | 8789.062 | 36.621 | 0.475 | 0.000 | 0.000 | 65.920 | 580.734 |
| debug-duplicate_path-full-cold | 1/199 | 2197.274 | 2197.274 | 0.000 | 4416.504 | 8833.008 | 0.000 | 0.478 | 0.000 | 0.000 | 65.920 | 582.219 |
| debug-duplicate_path-full-warm | 0/200 | 2197.274 | 2197.274 | 0.000 | 4394.531 | 8789.062 | 43.945 | 0.475 | 0.000 | 0.000 | 65.920 | 582.266 |
| debug-unique-finite-cold | 200/0 | 2197.274 | 2197.274 | 0.000 | 8789.062 | 17578.125 | 0.000 | 47.793 | 0.000 | 0.000 | 13184.069 | 23.812 |
| debug-unique-finite-warm | 0/200 | 2197.274 | 2197.274 | 0.000 | 4394.531 | 8789.062 | 36.621 | 47.769 | 0.000 | 0.000 | 13184.069 | 622.641 |
| debug-unique-full-cold | 200/0 | 2197.274 | 2197.274 | 0.000 | 8789.062 | 17578.125 | 0.000 | 47.828 | 0.000 | 0.000 | 13184.069 | 23.781 |
| debug-unique-full-warm | 0/200 | 2197.274 | 2197.274 | 0.000 | 4394.531 | 8789.062 | 8789.062 | 47.769 | 0.000 | 0.000 | 13184.069 | 702.781 |
| release-shared_path-finite-cold | 1/199 | 2197.274 | 2197.274 | 0.000 | 4416.504 | 8833.008 | 36.438 | 0.478 | 0.000 | 0.000 | 65.920 | 14.516 |
| release-shared_path-finite-warm | 0/200 | 2197.274 | 2197.274 | 0.000 | 4394.531 | 8789.062 | 36.621 | 0.475 | 0.000 | 0.000 | 65.920 | 14.203 |
| release-shared_path-full-cold | 1/199 | 2197.274 | 2197.274 | 0.000 | 4416.504 | 8833.008 | 0.000 | 0.478 | 0.000 | 0.000 | 65.920 | 14.156 |
| release-shared_path-full-warm | 0/200 | 2197.274 | 2197.274 | 0.000 | 4394.531 | 8789.062 | 43.945 | 0.475 | 0.000 | 0.000 | 65.920 | 14.312 |
| release-duplicate_path-finite-cold | 1/199 | 2197.274 | 2197.274 | 0.000 | 4416.504 | 8833.008 | 36.438 | 0.478 | 0.000 | 0.000 | 65.920 | 14.234 |
| release-duplicate_path-finite-warm | 0/200 | 2197.274 | 2197.274 | 0.000 | 4394.531 | 8789.062 | 36.621 | 0.475 | 0.000 | 0.000 | 65.920 | 14.234 |
| release-duplicate_path-full-cold | 1/199 | 2197.274 | 2197.274 | 0.000 | 4416.504 | 8833.008 | 0.000 | 0.478 | 0.000 | 0.000 | 65.920 | 14.234 |
| release-duplicate_path-full-warm | 0/200 | 2197.274 | 2197.274 | 0.000 | 4394.531 | 8789.062 | 43.945 | 0.475 | 0.000 | 0.000 | 65.920 | 14.406 |
| release-unique-finite-cold | 200/0 | 2197.274 | 2197.274 | 0.000 | 8789.062 | 17578.125 | 0.000 | 47.774 | 0.000 | 0.000 | 13184.069 | 22.797 |
| release-unique-finite-warm | 0/200 | 2197.274 | 2197.274 | 0.000 | 4394.531 | 8789.062 | 36.621 | 47.769 | 0.000 | 0.000 | 13184.069 | 36.094 |
| release-unique-full-cold | 200/0 | 2197.274 | 2197.274 | 0.000 | 8789.062 | 17578.125 | 0.000 | 47.771 | 0.000 | 0.000 | 13184.069 | 22.359 |
| release-unique-full-warm | 0/200 | 2197.274 | 2197.274 | 0.000 | 4394.531 | 8789.062 | 8789.062 | 47.769 | 0.000 | 0.000 | 13184.069 | 42.609 |

## Finite minus current full-storage baseline

All 12 like-for-like pairs are shown. Signed differences are finite minus full; positive values are increased costs. Percent columns use 100*(finite/full-1). The baseline shares current source, format and effective loop; it is not a historical pre-C2 binary. Single-run scheduling/page-cache noise prevents significance or universal superiority claims.

| Profile/topology/stage | Ready delta s | CPU delta s | Ready WS delta MiB | Ready commit delta MiB | PCM delta MiB | Read delta MiB | Write delta MiB | Ready change | WS change |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| debug-shared_path-cold | -1.160 | +3.266 | -0.328 | -0.215 | -7.324 | +36.438 | +0.000 | -0.2% | -0.1% |
| debug-shared_path-warm | -0.826 | +5.625 | -43.238 | -44.395 | -7.324 | -7.324 | +0.000 | -0.1% | -13.3% |
| debug-duplicate_path-cold | -2.907 | -6.672 | +0.836 | +0.824 | -7.324 | +36.438 | +0.000 | -0.5% | +0.2% |
| debug-duplicate_path-warm | -1.453 | -2.984 | -43.363 | -44.379 | -7.324 | -7.324 | +0.000 | -0.2% | -13.3% |
| debug-unique-cold | -161.554 | -378.266 | -8653.871 | -8671.238 | -8752.441 | -0.032 | -0.001 | -9.8% | -95.0% |
| debug-unique-warm | -47.959 | -100.203 | -8795.621 | -8813.348 | -8752.441 | -8752.440 | -0.000 | -10.2% | -96.9% |
| release-shared_path-cold | +2.990 | +3.719 | +0.051 | +0.262 | -7.324 | +36.444 | +0.000 | +8.5% | +0.0% |
| release-shared_path-warm | +0.336 | +0.656 | -44.043 | -45.332 | -7.324 | -7.328 | +0.000 | +1.2% | -13.8% |
| release-duplicate_path-cold | +1.383 | +1.000 | -0.371 | -0.562 | -7.324 | +36.439 | +0.000 | +3.8% | -0.1% |
| release-duplicate_path-warm | +0.018 | +0.688 | -44.250 | -46.012 | -7.324 | -7.320 | +0.000 | +0.1% | -13.8% |
| release-unique-cold | -185.280 | -418.594 | -8658.598 | -8661.914 | -8752.441 | +0.002 | -0.001 | -15.4% | -95.2% |
| release-unique-warm | -2.117 | -8.875 | -8796.383 | -8814.199 | -8752.441 | -8752.460 | +0.000 | -6.0% | -97.0% |

## Actual 96-handle pool

The separate productive 32-voice constructors create 64 warmed neutral handles and 32 source reserves, 96 total. Six fresh processes cover Debug/Release at 44.1/48/96 kHz. Memory deltas are whole-process current-counter changes around construction/warm-up, not allocator or lifetime peaks. Per-handle private is the whole-pool delta divided by 96. Rust allocation counts exclude native RubberBand/FFT allocations. Constructor CSV time excludes Cargo/helper setup.

| Profile | Rate Hz | Handles warmed+reserve | Construct/warm s | WS delta MiB | Private delta MiB | Private KiB/handle | Rust alloc count | Rust allocated MiB |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| debug | 44100 | 96 = 64 + 32 | 0.168 | +189.418 | +204.449 | +2180.792 | 1482 | 1.941 |
| debug | 48000 | 96 = 64 + 32 | 0.166 | +186.426 | +201.164 | +2145.750 | 1482 | 1.941 |
| debug | 96000 | 96 = 64 + 32 | 0.289 | +270.699 | +307.969 | +3285.000 | 1482 | 1.941 |
| release | 44100 | 96 = 64 + 32 | 0.169 | +189.129 | +204.203 | +2178.167 | 1482 | 1.941 |
| release | 48000 | 96 = 64 + 32 | 0.169 | +186.238 | +201.012 | +2144.125 | 1482 | 1.941 |
| release | 96000 | 96 = 64 + 32 | 0.300 | +270.746 | +307.871 | +3283.958 | 1482 | 1.941 |

Each matrix process independently observed 96 new/live native handles and restored its pre-run native live count on shutdown. OS HANDLE counts are different counters. The companion tables retain all 24 setup intervals and observed OS-handle peaks.

## Full-context exceptions and complete analysis

Each exception group below contains every matrix condition in that profile (12). Values are min..max of actual separate phases, not independent repetitions or one combined operation. The bound companion tables retain all 288 individual phase rows and resulting geometry/ACK details.

| Profile | Exception | Rows | Wall s | CPU s | Read MiB | Write MiB | Sampled private MiB |
| --- | --- | --- | --- | --- | --- | --- | --- |
| debug | active_outside_loop_tail_seek | 12 | 0.002..0.416 | 0.000..0.625 | 0.000..43.945 | 0.000..0.000 | 291.195..9054.562 |
| debug | finite_return | 12 | 0.003..0.010 | 0.000..0.000 | 0.000..0.000 | 0.000..0.000 | 291.195..9054.562 |
| debug | all | 12 | 0.392..0.419 | 0.391..0.656 | 43.945..43.945 | 0.000..0.000 | 372.141..9098.598 |
| debug | short_return | 12 | 0.002..0.008 | 0.000..0.000 | 0.000..0.000 | 0.000..0.000 | 328.109..9054.562 |
| debug | key_lock_full_context | 12 | 0.393..0.420 | 0.391..0.641 | 43.945..43.945 | 0.000..0.000 | 372.141..9098.598 |
| debug | key_lock_off | 12 | 0.003..0.003 | 0.000..0.000 | 0.000..0.000 | 0.000..0.000 | 328.109..9054.562 |
| debug | paused_outside_loop_intro_seek | 12 | 0.001..0.002 | 0.000..0.031 | 0.000..0.000 | 0.000..0.000 | 328.109..9054.562 |
| debug | stopped_seek_noop | 12 | 0.001..0.002 | 0.000..0.000 | 0.000..0.000 | 0.000..0.000 | 328.109..9054.562 |
| debug | editor_whole_source | 12 | 0.253..0.327 | 0.250..0.500 | 2.117..2.117 | 0.000..0.000 | 1073.422..9800.934 |
| debug | editor_distant_tail_raw | 12 | 0.000..0.000 | 0.000..0.000 | 0.000..0.000 | 0.000..0.000 | 1073.406..9800.934 |
| debug | explicit_return_to_finite_storage | 12 | 0.003..0.009 | 0.000..0.016 | 0.000..0.000 | 0.000..0.000 | 1073.816..9801.117 |
| debug | explicit_complete_materialization | 12 | 1.404..1.466 | 1.469..2.312 | 43.945..43.945 | 0.000..0.000 | 1117.852..9845.152 |
| release | active_outside_loop_tail_seek | 12 | 0.001..0.055 | 0.000..0.094 | 0.000..43.945 | 0.000..0.000 | 288.664..9053.836 |
| release | finite_return | 12 | 0.003..0.012 | 0.000..0.016 | 0.000..0.000 | 0.000..0.000 | 288.664..9053.832 |
| release | all | 12 | 0.021..0.059 | 0.016..0.109 | 43.945..43.945 | 0.000..0.000 | 369.961..9097.867 |
| release | short_return | 12 | 0.002..0.013 | 0.000..0.016 | 0.000..0.000 | 0.000..0.000 | 325.867..9053.836 |
| release | key_lock_full_context | 12 | 0.021..0.039 | 0.016..0.062 | 43.945..43.945 | 0.000..0.000 | 369.902..9097.867 |
| release | key_lock_off | 12 | 0.002..0.012 | 0.000..0.031 | 0.000..0.000 | 0.000..0.000 | 325.863..9053.836 |
| release | paused_outside_loop_intro_seek | 12 | 0.001..0.001 | 0.000..0.000 | 0.000..0.000 | 0.000..0.000 | 325.863..9053.832 |
| release | stopped_seek_noop | 12 | 0.001..0.002 | 0.000..0.016 | 0.000..0.000 | 0.000..0.000 | 325.867..9053.832 |
| release | editor_whole_source | 12 | 0.069..0.125 | 0.062..0.203 | 2.117..2.117 | 0.000..0.000 | 1071.074..9799.730 |
| release | editor_distant_tail_raw | 12 | 0.000..0.000 | 0.000..0.000 | 0.000..0.000 | 0.000..0.000 | 1071.062..9799.719 |
| release | explicit_return_to_finite_storage | 12 | 0.002..0.011 | 0.000..0.016 | 0.000..0.000 | 0.000..0.000 | 1071.258..9799.914 |
| release | explicit_complete_materialization | 12 | 0.047..0.065 | 0.047..0.109 | 43.945..43.945 | 0.000..0.000 | 1116.133..9843.953 |

ALL and Key Lock explicitly request complete extents. Active/paused nonresident seeks preserve the old effective voice until guarded native ACK; stopped seek is measured separately as a no-op. Whole-source editor buckets and distant raw tail samples are independently checked without changing playback residency. Explicit materialization verifies the complete PCM hash and reports the process while full PCM is held. A full resident backing can be intentionally shared; its remaining resident owner is not a cleanup failure. Later phase OS peaks remain cumulative.

Complete-source export, offline key completion, retirement, real native legacy-analysis terminal state and cancelled pre-start export are measured only in the 12 fresh-warm processes. Each profile has six conditions. Probe phase-progress output occurs inside these measurements and is included in their wall/CPU/logical-write counters. Export bytes/hash match the independent complete mono source. Synthetic key labels are not musical-quality acceptance. Cancellation proves absent output and released offline reservation; it does not measure active analyzer cancellation performance.

| Profile | Analysis phase | Rows | Wall s | CPU s | Read MiB | Write MiB | Sampled private MiB |
| --- | --- | --- | --- | --- | --- | --- | --- |
| debug | complete_export | 6 | 0.704..0.750 | 0.719..0.750 | 43.945..43.945 | 21.973..21.973 | 1073.918..9800.301 |
| debug | key_analysis | 6 | 23.582..24.305 | 24.266..25.203 | 21.973..21.973 | 0.000..0.000 | 1372.750..10099.027 |
| debug | retirement | 6 | 0.000..0.000 | 0.000..0.000 | 0.000..0.000 | 0.000..0.000 | 1039.379..9765.215 |
| debug | native_legacy_analysis | 6 | 24.016..24.616 | 27.453..28.344 | 43.945..43.945 | 0.000..0.000 | 1453.215..10182.945 |
| debug | cancelled_offline_export | 6 | 0.000..0.002 | 0.000..0.000 | 0.000..0.000 | 0.000..0.000 | 1037.602..9764.277 |
| release | complete_export | 6 | 0.028..0.030 | 0.016..0.047 | 43.945..43.945 | 21.973..21.973 | 1072.199..9799.918 |
| release | key_analysis | 6 | 0.830..0.959 | 0.859..1.062 | 21.973..21.973 | 0.000..0.000 | 1371.043..10098.453 |
| release | retirement | 6 | 0.000..0.000 | 0.000..0.000 | 0.000..0.000 | 0.000..0.000 | 1029.207..9765.738 |
| release | native_legacy_analysis | 6 | 0.855..0.898 | 1.016..1.125 | 43.945..43.945 | 0.000..0.000 | 1451.551..10192.820 |
| release | cancelled_offline_export | 6 | 0.000..0.004 | 0.000..0.016 | 0.000..0.000 | 0.000..0.000 | 1035.453..9764.211 |

## Unchanged actual 600-second accepted save

All four Debug/Release cold/fresh-warm cases use the unchanged private mono PCM24 source (28,800,000 full frames at 48 kHz) and frozen supported historical raw-QM envelope. The finite 42.0..42.5-second stereo resident is 192,000 bytes. The ordinary saved restore actually rejects the default 512 MiB budget. A cfg(test) 1 GiB diagnostic obtains current-ticket verification and actual native publication ACK; it grants neither persisted budget authority nor ordinary-default acceptance. Current request/source-generation/prepared/publication metadata are checked separately from immutable historical provenance. Real ProjectPersistence save/reload performs native content verification and atomic replacement. The saved original claim is durably acknowledged and the complete cache remains present after all runtime owners drop.

Rejected-actual-save rows include same-size corruption rejection **and corrected successful retry**. They cannot be labeled rejection-only timings. The previous file remains unchanged and dirty intent persists until correction. Process peaks at each point remain cumulative across earlier phases.

| Condition | Actual phase | Wall s | CPU s | Read MiB | Write MiB | Sampled WS MiB | Sampled private MiB | Lifetime peak WS MiB | Lifetime peak commit MiB |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| debug-long-cold | load | 191.596 | 198.250 | 906.380 | 411.991 | 784.855 | 785.871 | 785.344 | 785.918 |
| debug-long-cold | ordinary_default_rejection | 1.120 | 1.203 | 82.398 | 0.000 | 237.004 | 235.535 | 785.344 | 785.918 |
| debug-long-cold | explicit_adoption | 22.946 | 23.625 | 384.523 | 0.000 | 869.207 | 869.195 | 870.234 | 869.227 |
| debug-long-cold | actual_project_save | 23.024 | 23.625 | 384.645 | 0.122 | 869.809 | 869.434 | 871.613 | 869.492 |
| debug-long-cold | actual_native_export | 23.047 | 23.859 | 384.523 | 0.000 | 871.961 | 870.527 | 872.973 | 870.543 |
| debug-long-cold | rejected_actual_save | 24.068 | 24.812 | 467.165 | 0.122 | 871.695 | 870.551 | 873.383 | 870.555 |
| debug-long-warm | load | 16.580 | 17.031 | 494.573 | 82.398 | 236.207 | 235.043 | 236.207 | 235.043 |
| debug-long-warm | ordinary_default_rejection | 1.114 | 1.188 | 82.398 | 0.000 | 236.648 | 235.309 | 236.648 | 235.328 |
| debug-long-warm | explicit_adoption | 22.845 | 23.594 | 384.523 | 0.000 | 869.797 | 869.008 | 870.004 | 869.031 |
| debug-long-warm | actual_project_save | 22.833 | 23.641 | 384.645 | 0.122 | 871.270 | 869.562 | 871.504 | 869.613 |
| debug-long-warm | actual_native_export | 22.913 | 23.797 | 384.523 | 0.000 | 870.621 | 870.562 | 872.711 | 870.594 |
| debug-long-warm | rejected_actual_save | 23.970 | 24.703 | 467.165 | 0.122 | 871.789 | 870.586 | 872.898 | 870.637 |
| release-long-cold | load | 171.666 | 177.516 | 906.380 | 411.991 | 774.660 | 784.566 | 780.680 | 784.609 |
| release-long-cold | ordinary_default_rejection | 0.045 | 0.047 | 82.398 | 0.000 | 231.840 | 234.223 | 780.680 | 784.609 |
| release-long-cold | explicit_adoption | 0.621 | 0.656 | 384.523 | 0.000 | 861.711 | 867.883 | 864.539 | 867.902 |
| release-long-cold | actual_project_save | 0.612 | 0.625 | 384.645 | 0.122 | 860.789 | 867.883 | 865.156 | 867.941 |
| release-long-cold | actual_native_export | 0.602 | 0.641 | 384.523 | 0.000 | 864.570 | 869.594 | 866.410 | 869.625 |
| release-long-cold | rejected_actual_save | 0.674 | 0.656 | 467.165 | 0.122 | 861.922 | 869.617 | 866.578 | 869.652 |
| release-long-warm | load | 0.500 | 0.531 | 494.573 | 82.398 | 230.645 | 234.059 | 230.645 | 234.059 |
| release-long-warm | ordinary_default_rejection | 0.047 | 0.047 | 82.398 | 0.000 | 230.906 | 234.059 | 230.906 | 234.070 |
| release-long-warm | explicit_adoption | 0.657 | 0.656 | 384.523 | 0.000 | 862.379 | 867.754 | 863.660 | 867.789 |
| release-long-warm | actual_project_save | 0.625 | 0.672 | 384.645 | 0.122 | 856.699 | 868.359 | 864.641 | 868.367 |
| release-long-warm | actual_native_export | 0.615 | 0.641 | 384.523 | 0.000 | 864.625 | 869.223 | 865.754 | 869.230 |
| release-long-warm | rejected_actual_save | 0.664 | 0.656 | 467.165 | 0.122 | 864.008 | 869.246 | 865.930 | 869.297 |

## Productive cancellation and last-reader cleanup

The two profile packets contain five actual scenarios. Nested cancel/unload/join and cleanup measurements are shown separately below; outer scenario metrics remain in the companion tables. Running cancellation observes worker-entry Started, with no claimed decoder-specific checkpoint. Queued unload preserves peer ACKs. Pending-ACK unload rejects the stale payload during drain. Shutdown closes admission with two active and 32 queued jobs, then joins to zero jobs. A held complete finite reader keeps the shared cache alive after both distinct assignments unload; release/join removes owned originals/cache while preserving the frozen external source. Real Windows FileID/exclusion checks and native-handle lifetime balance pass. Empty phase roots and allocator arenas may remain.

| Profile packet | Nested phase | Wall s | CPU s | Read MiB | Write MiB | Sampled WS MiB | Sampled private MiB | Lifetime peak WS MiB | Lifetime peak commit MiB |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| debug-lifecycle | running/cancel_after_actual_started | 0.009 | 0.016 | 0.000 | 0.000 | 234.512 | 235.039 | 234.512 | 235.059 |
| debug-lifecycle | running/actual_orphan_cleanup | 0.000 | 0.000 | 0.000 | 0.000 | 234.621 | 234.863 | 234.621 | 235.059 |
| debug-lifecycle | queued/actual_queued_unload | 0.003 | 0.000 | 0.000 | 0.000 | 240.043 | 239.141 | 349.031 | 349.023 |
| debug-lifecycle | queued/actual_peer_and_orphan_cleanup | 0.024 | 0.031 | 0.000 | 0.000 | 239.852 | 238.816 | 349.031 | 349.023 |
| debug-lifecycle | pending_ack/actual_pending_ack_unload | 0.002 | 0.000 | 0.000 | 0.000 | 239.324 | 238.441 | 349.090 | 349.023 |
| debug-lifecycle | pending_ack/actual_orphan_cleanup | 0.033 | 0.031 | 0.000 | 0.000 | 239.133 | 238.191 | 349.090 | 349.023 |
| debug-lifecycle | shutdown/actual_closed_admission_join | 0.004 | 0.000 | 0.000 | 0.000 | 240.270 | 239.266 | 349.895 | 349.137 |
| debug-lifecycle | shutdown/actual_orphan_cleanup | 0.050 | 0.016 | 0.000 | 0.000 | 239.965 | 238.895 | 349.895 | 349.137 |
| debug-lifecycle | shared_last_reader/actual_final_reader_cleanup | 0.012 | 0.000 | 0.000 | 0.000 | 239.684 | 238.957 | 349.895 | 349.137 |
| release-lifecycle | running/cancel_after_actual_started | 0.011 | 0.016 | 0.000 | 0.000 | 229.688 | 232.816 | 229.688 | 232.836 |
| release-lifecycle | running/actual_orphan_cleanup | 0.000 | 0.000 | 0.000 | 0.000 | 229.695 | 232.664 | 229.758 | 232.836 |
| release-lifecycle | queued/actual_queued_unload | 0.001 | 0.000 | 0.000 | 0.000 | 233.293 | 236.441 | 342.785 | 346.379 |
| release-lifecycle | queued/actual_peer_and_orphan_cleanup | 0.012 | 0.031 | 0.000 | 0.000 | 233.117 | 236.168 | 342.785 | 346.379 |
| release-lifecycle | pending_ack/actual_pending_ack_unload | 0.001 | 0.000 | 0.000 | 0.000 | 232.789 | 236.043 | 342.785 | 346.379 |
| release-lifecycle | pending_ack/actual_orphan_cleanup | 0.031 | 0.016 | 0.000 | 0.000 | 232.656 | 235.840 | 342.785 | 346.379 |
| release-lifecycle | shutdown/actual_closed_admission_join | 0.004 | 0.000 | 0.000 | 0.000 | 233.809 | 236.965 | 343.504 | 346.945 |
| release-lifecycle | shutdown/actual_orphan_cleanup | 0.040 | 0.047 | 0.000 | 0.000 | 233.582 | 236.680 | 343.504 | 346.945 |
| release-lifecycle | shared_last_reader/actual_final_reader_cleanup | 0.012 | 0.000 | 0.000 | 0.000 | 235.832 | 239.266 | 345.492 | 349.137 |

## Dry renderer and separate numerical acceptance

All 120 dry render rows use 1/2/4/6/32 voices and 1,000 blocks of 512 output frames. Render-only time excludes reset/oracle work; mean block microseconds divide that interval by 1,000 blocks. A further 48,007 frames are compared with an independent phase-origin-zero integer/unity f32 source-sum oracle. Finite/full/cold/warm/profile hashes agree. This proof is separate from native-history, fractional-rate, Key Lock, stem and long-cycle timing authority.

| Profile | Voices | Rows | Render-only s | Mean block us | Max block us | Render CPU s |
| --- | --- | --- | --- | --- | --- | --- |
| debug | 1 | 12 | 0.324..0.343 | 324.130..343.374 | 384.300..1624.400 | 0.312..0.531 |
| debug | 2 | 12 | 0.641..0.680 | 640.521..680.151 | 695.500..874.100 | 0.656..1.016 |
| debug | 4 | 12 | 1.272..1.338 | 1272.162..1338.490 | 1371.000..1848.200 | 1.297..2.000 |
| debug | 6 | 12 | 1.905..2.012 | 1905.487..2011.818 | 1974.300..2866.000 | 1.922..3.125 |
| debug | 32 | 12 | 10.166..10.692 | 10165.549..10692.117 | 10655.400..16110.000 | 10.422..16.172 |
| release | 1 | 12 | 0.021..0.022 | 20.762..22.135 | 39.800..550.500 | 0.016..0.047 |
| release | 2 | 12 | 0.042..0.044 | 41.620..44.045 | 49.100..68.100 | 0.031..0.078 |
| release | 4 | 12 | 0.082..0.087 | 81.898..87.060 | 85.000..128.700 | 0.078..0.141 |
| release | 6 | 12 | 0.122..0.129 | 122.252..128.753 | 133.600..171.400 | 0.125..0.203 |
| release | 32 | 12 | 0.649..0.681 | 648.504..680.954 | 677.200..988.400 | 0.672..1.047 |

Separate final-tree numerical acceptance requires the current test source's 81 P/H/full-buffer cases and 54 actual 75/1,000-cycle checkpoints within one loaded frame, Debug and Release full suites, and all 29 final-tree checks. This measured-resource document is conditional on the independently bound terminal acceptance packet confirming those actual current passes. These numerical/native results do not establish hearing, CPAL/device deadlines, a six-pad live guarantee, Windows power-loss durability, or portable non-Windows immutable capture. Human/device acceptance and remaining pre-port work stay open. No app, stream, device, GUI or recorder was run for this hardware-free measurement.

## Evidence and freeze binding

Executing productive/probe source and runner/protocol bytes are frozen across measurements. Current dependency content is independently audited, with only six explicitly enumerated post-measurement documentation/task derivatives allowed. Writing measured result text after collection does not require circular reruns; changes to an executing dependency invalidate the affected measurement. Final-tree validation is a separate binding. Source/runtime hashes identify observed on-disk files, not a digest of mapped PE memory. Actual current EXE and loaded Python/RubberBand paths/bytes are bound to the executing process; shared installed PYD history is compared with the persistent requested-profile DLL. Exact normal-tool runtime paths may be external, while the independent workspace auditor does not reopen external DLLs.

Measured dependency projection SHA-256: `6a897b977ef71dd0e8c3028529b25df35079151b0f57c1672235617c6da44a85`. Aggregate of actual accepted run/config/log/measurement/source and independent measurement-audit file records: `a6f80a234bdae2b8182c6f8146ad7426eab81adcb766415bac99fafc5814e0ed`. Full extracted table content SHA-256: `e42642a917ff5d4fc717f4441bed125ff8b15d0e69543892fa7d855afa56b5eb`. Local private sources, large cache files and raw logs remain uncommitted. Independent audits bind their actual byte lengths/hashes. The scratch companion preserves every individual renderer, exception, analysis and setup row so these ranges do not hide omitted conditions.
