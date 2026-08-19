# Intel Core 5 210H CPU Scheduler Redesign and Context-Aware Power A/B

Date: 2026-08-18

## Verdict

The context-aware power route is the best measured tradeoff. It keeps Focus and Launch for
app launches and genuinely heavy Focus App demand, while background-dominant
pressure uses the editable Background Pressure Profile values. With Limit Background Processors disabled,
it favors responsiveness and retains 92.07% of background throughput. Enabling
Limit Background Processors with the tuned four-app limit reaches 25.98% package-power
saving and retains 93.38% of background throughput at the cost of some
foreground responsiveness.

| Metric vs paired Stock | Targeted-only | Previous hybrid | Context-aware | Context-aware + Limit Background Processors |
| --- | ---: | ---: | ---: | ---: |
| Runtime activation | 4/4 | 4/4 | 4/4 | 4/4 |
| Foreground median improvement | 16.73% | **43.42%** | 32.60% | 24.62% |
| Foreground P95 improvement | 22.05% | **43.80%** | 38.48% | 29.88% |
| Background throughput retained | **99.22%** | 91.85% | 92.07% | 93.38% |
| Package-power saving | 2.97% | 13.85% | 15.68% | **25.98%** |
| 20% package-power gate | FAIL | FAIL | FAIL | **PASS** |

Keep the context-aware route and the default Background Pressure A/C value of
80% Efficient Aggressive. The lower 60% Efficient Enabled candidate missed both
the selected 25% responsiveness target and the 20% power target. The new A/C and
Battery Background Pressure Profile and Focus and Launch Profile values remain editable so
future hardware can be calibrated without another runtime rewrite.

## Controlled broad-only versus hybrid comparison

Both sides use the same context-aware release binary, benchmark settings,
workload, and counterbalanced 4-pass harness. Disabled Limit Background Processors
represents the old broad priority/EcoQoS policy; enabling it adds targeted
hot-process CPU restraint. The follow-up tuning changes only the maximum
restrained-app count from six to four.

| Metric vs paired Stock | Old broad-only | Hybrid, six apps | Hybrid, four apps | Four-app change vs six |
| --- | ---: | ---: | ---: | ---: |
| Median latency improvement | **32.60%** | 25.75% | 24.62% | -1.13 percentage points |
| P95 latency improvement | **38.48%** | 24.58% | 29.88% | +5.30 percentage points |
| Background throughput retained | 92.07% | 90.00% | **93.38%** | +3.38 percentage points |
| Package-power saving | 15.68% | 20.48% | **25.98%** | +5.50 percentage points |
| 20% package-power gate | FAIL | PASS | **PASS** | Target retained |

Four restrained apps is the cleaner default: compared with six, it improves
P95 responsiveness, background retention, package power, and pass-to-pass power
stability while giving up only 1.13 percentage points of median improvement.
Limit Background Processors remains a power-first tradeoff versus the broad-only route.
The Power Save and Balanced presets now enable Limit Background Processors with this four-app limit,
so Balanced + Low Impact reproduces the validated configuration.

## Strong-gate confirmation

A 30-second-warmup context-aware screen passed the 20% gate, but the longer
100-second-warmup run did not reproduce it:

| Metric | Previous hybrid screen | Context-aware screen | Context-aware authoritative |
| --- | ---: | ---: | ---: |
| Median improvement | 34.07% | 28.85% | **32.60%** |
| P95 improvement | 34.07% | 28.85% | **38.48%** |
| Background throughput retained | 94.08% | **95.90%** | 92.07% |
| Package-power saving | 13.62% | **20.88%** | 15.68% |
| 20% power gate | FAIL | PASS | **FAIL** |

Every authoritative Adaptive pass observed the pressure control, and the
active plan reported the intended Background Pressure Profile values: A/C parking 40%, minimum
15%, maximum 100%, boost policy 80%, and Efficient Aggressive boost. The missing
power percentage is therefore not an application bug where the engine failed
to use the editable profile.

## Tuning screens

| Variant | Median gain | P95 gain | Background retained | Power saving | Decision |
| --- | ---: | ---: | ---: | ---: | --- |
| Targeted-only, 10% threshold | 27.30% | 29.72% | 97.25% | 2.15% | Superseded by hybrid |
| Targeted-only, 8% threshold | 12.65% | 10.32% | 96.55% | 2.42% | Reject: weaker latency |
| Targeted-only with Limit Background Processors | 19.62% | 21.50% | 74.60% | 6.18% | Reject: background floor failed |
| Hybrid broad policy + targeted layer | **29.80%** | **36.83%** | 91.72% | **24.72%** | Screen only; not reproduced |
| Context-aware hybrid, Background 80% | 32.60% | 38.48% | **92.07%** | **15.68%** | Keep: best authoritative tradeoff |
| Context-aware hybrid + Limit Background Processors, six apps | 25.75% | 24.58% | 90.00% | 20.48% | Superseded by four-app tuning |
| Context-aware hybrid + Limit Background Processors, four apps | 24.62% | 29.88% | **93.38%** | **25.98%** | Keep as tuned power-first default |
| Context-aware hybrid, Background 60% screen | 24.25% | 24.25% | 91.30% | 18.48% | Reject: missed both selected gates |

The short hybrid screen's 24.72% power result did not reproduce in either the
100-second authoritative run (13.85%) or the later strong-gate run (13.62%). It
must not be used as the accepted power result.

## Why Focus and Launch Profile was not weakened

Focus and Launch keeps the existing 100% Aggressive profile. Only
background-dominant pressure moves to Background Pressure. An earlier five-pass hardware
test already found that weakening Focus and Launch regressed median latency by 7.0% and P95
latency by 10.1%, so the new design does not hide a global compromise behind the
power target.

PowerSave behavior should instead be judged with a separate low-demand and
background-residency workload. Requiring a foreground-contended Focus and Launch workload
to save 20% conflates two different product goals.

## Invalid old-engine comparison removed

The first draft ranked an `old` executable against the redesign. That
executable came from a source snapshot that differed across the wider runtime
and architecture, not only CPU Scheduler. Its benchmark configuration also
used renamed settings that the old schema ignored. Its 27.05% power figure was
therefore not an isolated algorithm comparison and has been removed from the
report and result set.

## Methodology

| Item | Value |
| --- | --- |
| System | ASUS Vivobook 16 V3607VU |
| OS | Windows 11 Home, build 26200 |
| CPU | Intel Core 5 210H, 8 cores / 12 logical processors |
| Memory | 15.61 GB DDR5-5600 |
| Generated load | 11 hidden `cscript.exe` CPU workers |
| Foreground workload | 5 rounds x 1,000,000 CPU-loop iterations |
| Authoritative passes | 4, balanced Stock-first/Adaptive-first order |
| Authoritative warmup | 100 seconds per case |
| Stock | Windows Balanced with Winderust stopped |
| Adaptive | Isolated portable Winderust, Balanced + Low Impact |

Workers are created through WMI rather than as children of the foreground
benchmark shell. The harness clears inherited Power Throttling, excludes the
exact benchmark host path from CPU Scheduler control, requires every worker
to survive, and fails unless a real worker priority change is observed.

Results are local to this machine, Windows state, workload, and AC/thermal
conditions. Percentages are paired against each pass's adjacent Stock case;
absolute watts from different sessions are not directly comparable.

| Artifact | SHA-256 |
| --- | --- |
| Hybrid engine binary | `CCD9906055ADF881B1F44D7342DAC50F412A8BB1EB46D77448FC8F27037EBF6D` |
| Context-aware engine binary | `4528338E143D775A65BA9EA4E6BE2AD5BE165B97E5C2E71FD2A1620A68E4566A` |
| Targeted-only engine binary | `A3703DE7D8317535E98B0A191E88F008BC2EDD7F7A36FFAD54FB97A766BF37AB` |

## Raw results

- [Hybrid authoritative CPU run](results/hybrid-engine-isolated-cpu-authoritative-20260818.json)
- [Hybrid direct-control strong-gate run](results/hybrid-engine-equivalent-controls-screening-20260818.json)
- [Context-aware authoritative CPU run](results/hybrid-engine-context-power-authoritative-20260818.json)
- [Context-aware Limit Background Processors authoritative CPU run](results/hybrid-engine-context-power-processor-restraint-authoritative-20260818.json)
- [Four-app Limit Background Processors screening run](results/hybrid-engine-restraint-max4-screening-20260818.json)
- [Four-app Limit Background Processors authoritative CPU run](results/hybrid-engine-restraint-max4-authoritative-20260818.json)
- [Context-aware initial screening run](results/hybrid-engine-context-power-screening-20260818.json)
- [Context-aware 60% Background Pressure screen](results/hybrid-engine-background-60-screening-20260818.json)
- [Hybrid initial screening run](results/hybrid-engine-isolated-cpu-screening-20260818.json)
- [Targeted-only authoritative CPU run](results/new-engine-isolated-cpu-authoritative-20260818.json)
- [Targeted-only screening run](results/new-engine-isolated-cpu-screening-20260818.json)
- [Targeted-only 8% threshold screen](results/new-engine-threshold-8-isolated-cpu-screening-20260818.json)
- [Targeted-only processor-restraint screen](results/new-engine-processor-restraint-isolated-cpu-screening-20260818.json)
