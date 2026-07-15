# Intel Core 5 210H

Run on 2026-07-15 with 12 logical processors and 12 workers. The CPU-loop suite
used 3 passes, 5 rounds, and 1,000,000 foreground iterations per round. Package
power came from the RAPL `Package0` counter.

| Case | Median latency vs Off | P95 latency vs Off | Foreground throughput vs Off | Background suppression vs Off | Package power vs Off | Repeat passes won |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Off | 305.75 ms (baseline) | 312.59 ms (baseline) | 3,819,836 iter/s (baseline) | 0.0% | 50.01 W (baseline) | baseline |
| Powersave | 541.63 ms (-147.7%) | 553.02 ms (-151.8%) | 2,204,262 iter/s (-42.3%) | 31.3% | 14.39 W (-71.2%) | 0/3 |
| Balanced | 247.99 ms (+24.8%) | 251.21 ms (+26.2%) | 4,632,913 iter/s (+21.3%) | 10.1% | 41.84 W (-16.3%) | 3/3 |
| Performance | 180.72 ms (+34.9%) | 183.62 ms (+35.1%) | 5,540,620 iter/s (+45.0%) | 1.0% | 45.84 W (-8.3%) | 3/3 |
| Speed | 150.66 ms (+45.9%) | 156.27 ms (+43.9%) | 6,545,259 iter/s (+71.3%) | 87.0% | 24.13 W (-51.7%) | 3/3 |

Score component ratios from the same run, all vs paired `Off`:

| Case | Int arithmetic | Double arithmetic | Float batch | GZip | Deflate | SHA-256 | AES-CBC | L2 scan | Memory copy |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Off | 2,504.4 Mops (100.0%) | 732.1 Mops (100.0%) | 3,481.5 Mops (100.0%) | 40.9 MB/s (100.0%) | 36.1 MB/s (100.0%) | 1,402.0 MB/s (100.0%) | 628.9 MB/s (100.0%) | 1,295.7 MB/s (100.0%) | 7,259.0 MB/s (100.0%) |
| Powersave | 1,191.4 Mops (41.9%) | 339.9 Mops (37.0%) | 1,660.4 Mops (39.0%) | 23.5 MB/s (49.2%) | 20.5 MB/s (50.9%) | 724.9 MB/s (41.6%) | 634.4 MB/s (59.0%) | 919.2 MB/s (56.8%) | 7,423.6 MB/s (81.1%) |
| Balanced | 2,590.5 Mops (118.1%) | 730.6 Mops (123.4%) | 3,594.3 Mops (116.0%) | 48.8 MB/s (128.2%) | 39.8 MB/s (120.9%) | 1,497.3 MB/s (133.0%) | 591.5 MB/s (131.8%) | 1,452.7 MB/s (130.2%) | 8,510.2 MB/s (142.0%) |
| Performance | 2,968.9 Mops (159.8%) | 824.5 Mops (165.2%) | 4,236.0 Mops (164.2%) | 57.1 MB/s (186.1%) | 47.5 MB/s (181.5%) | 1,818.4 MB/s (172.7%) | 755.4 MB/s (188.0%) | 1,682.5 MB/s (195.6%) | 9,148.8 MB/s (201.3%) |
| Speed | 2,966.2 Mops (166.9%) | 811.6 Mops (170.7%) | 4,360.8 Mops (175.7%) | 69.6 MB/s (221.9%) | 51.9 MB/s (182.6%) | 2,025.1 MB/s (190.9%) | 897.5 MB/s (214.3%) | 1,624.3 MB/s (184.3%) | 12,617.1 MB/s (237.0%) |

Latency and score ratios use each case's adjacent `Off` samples. Throughput
and package-power ratios compare the displayed aggregate rows. Speed produced
the best foreground result, but retained only 13.0% of background throughput.
Performance was the less disruptive high-performance result: it improved paired
median and p95 latency by about 35% while retaining 99.0% of background throughput.
The PowerShell runner omits adaptive affinity on hybrid CPUs and does not launch
the app automation loop, so this result does not exercise runtime custom-plan
tier switching or the Soft CPU Set topology floor.

Command:

```powershell
.\scripts\workload_engine_benchmark.ps1 -Passes 3 -Rounds 5 -Iterations 1000000
```

Results are local direction, not a cross-machine ranking. Thermal state, firmware,
Windows power policy, and other running software can change them.
