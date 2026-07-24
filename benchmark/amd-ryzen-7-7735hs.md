# AMD Ryzen 7 7735HS

Previous README result from an Adaptive Engine preset CPU-loop validation with
16 logical processors and RAPL package-power sampling.

| Case | Median latency vs Off | P95 latency vs Off | Foreground throughput vs Off | Background latency vs Off | Package power vs Off | Repeat passes won |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Off | 228.67 ms (baseline) | 239.84 ms (baseline) | 4,540,008 iter/s (baseline) | 1.00x (baseline) | 63.90 W (baseline) | baseline |
| Powersave | 377.30 ms (-70.6%) | 378.55 ms (-65.1%) | 2,643,422 iter/s (-41.8%) | 1.21x (+20.9%) | 10.97 W (-82.8%) | 0/3 |
| Balanced | 181.46 ms (+24.2%) | 187.33 ms (+24.7%) | 5,423,882 iter/s (+19.5%) | 1.20x (+20.3%) | 21.16 W (-66.9%) | 3/3 |
| Performance | 123.99 ms (+47.1%) | 125.15 ms (+50.4%) | 8,033,565 iter/s (+76.9%) | 1.50x (+50.2%) | 57.91 W (-9.4%) | 3/3 |
| Speed | 119.07 ms (+44.7%) | 119.35 ms (+47.6%) | 8,410,122 iter/s (+85.2%) | 12.05x (+1,105.4%) | 22.65 W (-64.5%) | 3/3 |

Score component ratios from the same run, all vs paired `Off`:

| Case | Int arithmetic | Double arithmetic | Float batch | GZip | Deflate | SHA-256 | AES-CBC | L2 scan | Memory copy |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Off | 3,026.5 Mops (100.0%) | 863.5 Mops (100.0%) | 5,515.4 Mops (100.0%) | 59.2 MB/s (100.0%) | 48.9 MB/s (100.0%) | 1,749.3 MB/s (100.0%) | 836.8 MB/s (100.0%) | 2,245.0 MB/s (100.0%) | 8,517.6 MB/s (100.0%) |
| Powersave | 1,097.8 Mops (36.4%) | 287.8 Mops (33.2%) | 1,889.7 Mops (34.3%) | 25.2 MB/s (41.5%) | 22.5 MB/s (45.7%) | 637.4 MB/s (38.8%) | 532.8 MB/s (130.4%) | 1,337.8 MB/s (49.3%) | 766.4 MB/s (50.6%) |
| Balanced | 2,333.6 Mops (84.0%) | 606.5 Mops (70.8%) | 3,982.0 Mops (72.1%) | 50.9 MB/s (94.1%) | 44.3 MB/s (94.3%) | 1,335.2 MB/s (75.5%) | 751.8 MB/s (102.2%) | 2,646.3 MB/s (144.5%) | 12,455.8 MB/s (96.0%) |
| Performance | 2,956.3 Mops (94.4%) | 867.6 Mops (100.0%) | 5,363.7 Mops (97.5%) | 77.7 MB/s (132.9%) | 60.9 MB/s (119.2%) | 1,759.0 MB/s (98.5%) | 877.9 MB/s (115.9%) | 4,172.6 MB/s (198.6%) | 15,197.6 MB/s (117.0%) |
| Speed | 3,505.4 Mops (111.7%) | 962.7 Mops (111.6%) | 5,975.3 Mops (108.1%) | 81.4 MB/s (143.0%) | 66.0 MB/s (142.0%) | 2,015.6 MB/s (113.9%) | 1,106.4 MB/s (138.3%) | 4,346.6 MB/s (220.5%) | 16,687.0 MB/s (122.8%) |

Command:

```powershell
.\scripts\workload_engine_benchmark.ps1 -Passes 3 -Rounds 5 -Iterations 1000000
```

Results are local direction only. The script's controls and additional scenarios
are documented in [the benchmark guide](../docs/adaptive-engine-benchmark.md).
