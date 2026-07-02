# Auto Balance Benchmark Guide

This guide is for the next agent run. It documents the synthetic benchmark used
while tuning Auto Balance presets.

## What This Measures

The default benchmark measures foreground CPU work completion time while
temporary background CPU workers compete for scheduler time. Lower milliseconds
are better.

The optional `TaskManagerLaunch` foreground scenario measures Task Manager
launch-to-window time under the same generated background load. Close any
visible Task Manager window before running it. If Windows keeps a hidden
resident `Taskmgr` process alive, the script measures reopen-to-window time.

The optional `WinderustLaunch` foreground scenario measures launching this app
under the same generated background load. It refuses to run while Winderust is
already open, then closes only the instance it started for each sample. This is
a better normal-user proxy than Task Manager when checking app startup behavior.

It is not a full Winderust automation benchmark. It does not launch the app or
exercise the real automation loop. It models the preset scheduler effects with:

- process priority,
- foreground process priority,
- foreground and background dynamic priority boost,
- background thread priority,
- background memory priority,
- background I/O priority,
- background GPU priority attempts,
- target count,
- and hard processor affinity for the CPU-share approximation.

PowerShell hard affinity is stricter than Winderust Soft CPU Sets, so affinity
results should be treated as directional, not exact.

The foreground loop is still CPU-bound. I/O, memory, and GPU controls are applied
to the generated background workers where Windows accepts them, but the workload
does not include a dedicated disk, memory-pressure, or GPU-rendering phase.

## Hardware Scope

Do not treat one local benchmark as universal. Record the CPU model, logical
processor count, Windows power mode, and whether the machine has Intel-style
P-cores plus E-cores or an all-P-core layout such as most AMD desktop CPUs.

Auto Balance runtime masking is topology-aware:

- Hybrid CPUs: background affinity candidates prefer E-cores, then choose the
  least-loaded allowed E-cores when load data is available.
- All-P-core CPUs: background affinity candidates use all logical processors
  and choose the least-loaded allowed cores when load data is available.
- Automatic CPU-share floors are intentionally different: hybrid systems can be
  more assertive because E-cores give background work a separate lane, while
  all-P-core systems keep a higher floor to avoid over-restricting background
  work on shared performance cores.

Benchmark matrix for preset changes:

| Hardware class | Required check |
| --- | --- |
| Intel hybrid P-core + E-core | Verify foreground median, p95, jitter, and that Responsive does not steal P-core time from the focused app. |
| AMD or other all-P-core CPU | Verify foreground median and p95 improve without collapsing background retained capacity. |
| Low-core CPU, 4 to 8 logical processors | Verify Responsive still leaves at least one background lane and does not produce unstable tails. |

If only one hardware class is available, document that limitation and avoid
changing global preset constants unless the result is clearly supported by code
reasoning and topology-specific unit tests.

## Current Preset Model

Keep this in sync with `auto_balance_preset_values` in `src/app.rs`.

| Preset | Benchmark model |
| --- | --- |
| Off | 12 background workers at `Normal`; foreground benchmark process at `Normal`. |
| Gentle | All background workers at `Idle`; first 12 targets affinity-limited to 60% logical processors; foreground at `AboveNormal`; extra I/O, memory, thread, priority-boost, and GPU assists disabled. |
| Balance | All background workers at `Idle`; first 12 targets affinity-limited to 50% logical processors; foreground at `AboveNormal`; foreground priority boost enabled; background priority boost disabled; background threads `BelowNormal`; background memory `Low`; background I/O `Low`; background GPU `BelowNormal` when available. |
| Responsive | All background workers at `Idle`; first 12 targets affinity-limited to 16% logical processors; foreground at `AboveNormal`; foreground priority boost enabled; background priority boost disabled; background threads `BelowNormal`; background memory `VeryLow`; background I/O `VeryLow`; background GPU `BelowNormal` when available. |

For launch foreground scenarios, preset cases intentionally use launch grace:
foreground launch priority is raised to `AboveNormal`, while background
restraints are deferred until after the app-start window.

## Before Running

Run from the repository root:

```powershell
rtk cargo check
rtk cargo test auto_balance
```

For cleaner benchmark results:

- Plug in AC power.
- Close browsers, game launchers, update tools, and other background work.
- Avoid moving windows or using the machine during the run.
- Close any visible Task Manager window before using
  `-ForegroundScenario TaskManagerLaunch`.
- Close Winderust before using `-ForegroundScenario WinderustLaunch`.
- Run the benchmark at least twice if results are surprising.

The agent must request escalation for the benchmark command because it spawns
temporary CPU-load child processes, changes process priority, applies affinity,
and kills the workers during cleanup.

## Benchmark Command

Preferred repeat-loop command:

```powershell
.\scripts\auto_balance_benchmark.ps1 -Passes 3 -Rounds 5 -Iterations 1000000
```

Task Manager launch scenario:

```powershell
.\scripts\auto_balance_benchmark.ps1 -ForegroundScenario TaskManagerLaunch -Passes 3 -Rounds 3 -WorkerSeconds 20
```

Winderust launch scenario:

```powershell
.\scripts\auto_balance_benchmark.ps1 -ForegroundScenario WinderustLaunch -Passes 3 -Rounds 3 -WorkerSeconds 20
```

If the release binary is not built, either run `cargo build --release` first or
pass `-WinderustExePath <path-to-winderust.exe>`.

Trust a local tuning direction only when median and P95 both improve by at least
3% in at least two of three passes. If median improves but P95 or jitter gets
worse, the change is not validated; it probably only moved the average.

The script rotates case order between passes to reduce order and thermal bias.
Each preset is compared with its own adjacent Off run, and the pair order flips
between passes. The JSON includes `assist_controls`, `assist_status`, and
`assist_coverage` so a report can show which OS controls were applied and where
the synthetic workload is only directional. It still remains a local synthetic
benchmark, not proof of universal defaults.

Legacy inline command, kept only for agents that cannot execute repository
scripts. It is CPU-only and should not be used for the final insight report when
the repository script can run:

```powershell
$ErrorActionPreference = 'Stop'
$powerShellPath = Join-Path $PSHOME 'powershell.exe'
$logicalProcessors = [Environment]::ProcessorCount
$workerCount = [Math]::Min([Math]::Max($logicalProcessors, 4), 12)
$iterations = 1250000
$rounds = 7
$workerSeconds = 75
$gentleTargetCount = $workerCount
$gentleCorePercent = 0.60
$balanceCorePercent = 0.50
$responsiveCorePercent = 0.16
$gentleCoreCount = [Math]::Max(1, [Math]::Ceiling($logicalProcessors * $gentleCorePercent))
$balanceCoreCount = [Math]::Max(1, [Math]::Ceiling($logicalProcessors * $balanceCorePercent))
$responsiveCoreCount = [Math]::Max(1, [Math]::Ceiling($logicalProcessors * $responsiveCorePercent))

function New-Mask([int]$Count) {
    $mask = 0L
    for ($core = 0; $core -lt [Math]::Min($Count, 62); $core++) {
        $mask = $mask -bor (1L -shl $core)
    }
    return $mask
}
$gentleMask = New-Mask $gentleCoreCount
$balanceMask = New-Mask $balanceCoreCount
$responsiveMask = New-Mask $responsiveCoreCount

function Measure-ForegroundWork {
    param([int]$Iterations, [int]$Rounds)
    $samples = New-Object 'System.Collections.Generic.List[double]'
    for ($round = 0; $round -lt $Rounds; $round++) {
        [GC]::Collect()
        $sw = [Diagnostics.Stopwatch]::StartNew()
        $acc = 0.0
        for ($i = 1; $i -le $Iterations; $i++) {
            $acc += [Math]::Sqrt($i)
        }
        $sw.Stop()
        $samples.Add($sw.Elapsed.TotalMilliseconds)
        Start-Sleep -Milliseconds 150
    }
    return $samples.ToArray()
}

function Start-CpuWorkers {
    param(
        [string[]]$Priorities,
        [int]$AffinitySelectedCount,
        [Int64]$AffinityMask,
        [int]$Seconds
    )

    $code = @"
`$deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
`$acc = 0.0
while ([DateTime]::UtcNow -lt `$deadline) {
    for (`$i = 1; `$i -le 100000; `$i++) {
        `$acc += [Math]::Sqrt(`$i)
    }
}
"@
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($code))
    $processes = @()
    for ($worker = 0; $worker -lt $Priorities.Length; $worker++) {
        $process = Start-Process `
            -FilePath $powerShellPath `
            -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', $encoded) `
            -WindowStyle Hidden `
            -PassThru
        Start-Sleep -Milliseconds 80
        try {
            $process.PriorityClass = $Priorities[$worker]
        } catch {
        }
        if ($AffinitySelectedCount -gt 0 -and $worker -lt $AffinitySelectedCount -and $AffinityMask -gt 0) {
            try {
                $process.ProcessorAffinity = [IntPtr]$AffinityMask
            } catch {
            }
        }
        $processes += $process
    }
    return $processes
}

function Stop-CpuWorkers {
    param([object[]]$Processes)
    foreach ($process in $Processes) {
        if ($null -eq $process) {
            continue
        }
        try {
            $process.Refresh()
            if (-not $process.HasExited) {
                $process.Kill()
                [void]$process.WaitForExit(2000)
            }
        } catch {
        }
    }
}

function Summarize-Samples {
    param([string]$Name, [double[]]$Samples, [string]$Model)
    $sorted = [double[]]$Samples.Clone()
    [Array]::Sort($sorted)
    $sum = 0.0
    foreach ($sample in $Samples) {
        $sum += $sample
    }
    $avg = $sum / $Samples.Length
    $medianIndex = [int][Math]::Floor(($Samples.Length - 1) * 0.50)
    $p95Index = [int][Math]::Floor(($Samples.Length - 1) * 0.95)
    return [pscustomobject]@{
        name = $Name
        model = $Model
        samples_ms = [double[]]$Samples
        avg_ms = [Math]::Round($avg, 2)
        median_ms = [Math]::Round($sorted[$medianIndex], 2)
        p95_ms = [Math]::Round($sorted[$p95Index], 2)
        min_ms = [Math]::Round($sorted[0], 2)
        max_ms = [Math]::Round($sorted[$sorted.Length - 1], 2)
    }
}

function New-Priorities {
    param([string]$DefaultPriority, [int]$RestrainedCount, [string]$RestrainedPriority)
    $priorities = New-Object string[] $workerCount
    for ($worker = 0; $worker -lt $workerCount; $worker++) {
        if ($worker -lt $RestrainedCount) {
            $priorities[$worker] = $RestrainedPriority
        } else {
            $priorities[$worker] = $DefaultPriority
        }
    }
    return $priorities
}

function Run-Case {
    param(
        [string]$Name,
        [string]$Model,
        [string]$ForegroundPriority,
        [string[]]$Priorities,
        [int]$AffinitySelectedCount,
        [Int64]$AffinityMask
    )

    $currentProcess = [Diagnostics.Process]::GetCurrentProcess()
    $originalPriority = $currentProcess.PriorityClass
    $processes = @()
    try {
        $currentProcess.PriorityClass = $ForegroundPriority
        $processes = Start-CpuWorkers `
            -Priorities $Priorities `
            -AffinitySelectedCount $AffinitySelectedCount `
            -AffinityMask $AffinityMask `
            -Seconds $workerSeconds
        Start-Sleep -Seconds 2
        $samples = Measure-ForegroundWork -Iterations $iterations -Rounds $rounds
        return Summarize-Samples -Name $Name -Samples $samples -Model $Model
    } finally {
        Stop-CpuWorkers -Processes $processes
        try {
            $currentProcess.PriorityClass = $originalPriority
        } catch {
        }
        Start-Sleep -Seconds 1
    }
}

$normalPriorities = New-Priorities -DefaultPriority 'Normal' -RestrainedCount 0 -RestrainedPriority 'Normal'
$gentlePriorities = New-Priorities -DefaultPriority 'Normal' -RestrainedCount $gentleTargetCount -RestrainedPriority 'Idle'
$balancePriorities = New-Priorities -DefaultPriority 'Idle' -RestrainedCount $workerCount -RestrainedPriority 'Idle'
$responsivePriorities = New-Priorities -DefaultPriority 'Idle' -RestrainedCount $workerCount -RestrainedPriority 'Idle'

$currentProcess = [Diagnostics.Process]::GetCurrentProcess()
$originalPriority = $currentProcess.PriorityClass
try {
    $currentProcess.PriorityClass = 'Normal'
    $baseline = Summarize-Samples `
        -Name 'baseline_no_background_load' `
        -Samples (Measure-ForegroundWork -Iterations $iterations -Rounds $rounds) `
        -Model 'No generated background load.'
} finally {
    try {
        $currentProcess.PriorityClass = $originalPriority
    } catch {
    }
}

$off = Run-Case `
    -Name 'off' `
    -Model '12 background workers Normal; foreground Normal.' `
    -ForegroundPriority 'Normal' `
    -Priorities $normalPriorities `
    -AffinitySelectedCount 0 `
    -AffinityMask 0
$gentle = Run-Case `
    -Name 'gentle' `
    -Model 'Gentle: all background workers Idle; first 12 targets limited to 60% logical processors; foreground AboveNormal.' `
    -ForegroundPriority 'AboveNormal' `
    -Priorities $gentlePriorities `
    -AffinitySelectedCount ([Math]::Min(12, $workerCount)) `
    -AffinityMask $gentleMask
$balance = Run-Case `
    -Name 'balance' `
    -Model 'Balance: all background workers Idle; first 12 targets limited to 50% logical processors; foreground AboveNormal.' `
    -ForegroundPriority 'AboveNormal' `
    -Priorities $balancePriorities `
    -AffinitySelectedCount ([Math]::Min(12, $workerCount)) `
    -AffinityMask $balanceMask
$responsive = Run-Case `
    -Name 'responsive' `
    -Model 'Responsive: all background workers Idle; first 12 targets limited to 16% logical processors; foreground AboveNormal.' `
    -ForegroundPriority 'AboveNormal' `
    -Priorities $responsivePriorities `
    -AffinitySelectedCount ([Math]::Min(12, $workerCount)) `
    -AffinityMask $responsiveMask

$cases = @($gentle, $balance, $responsive)
$comparisons = foreach ($case in $cases) {
    [pscustomobject]@{
        name = $case.name
        avg_ms_delta_vs_off = [Math]::Round(($case.avg_ms - $off.avg_ms), 2)
        avg_improvement_percent_vs_off = [Math]::Round((($off.avg_ms - $case.avg_ms) / $off.avg_ms) * 100.0, 1)
        median_improvement_percent_vs_off = [Math]::Round((($off.median_ms - $case.median_ms) / $off.median_ms) * 100.0, 1)
        p95_improvement_percent_vs_off = [Math]::Round((($off.p95_ms - $case.p95_ms) / $off.p95_ms) * 100.0, 1)
    }
}

[pscustomobject]@{
    note = 'Synthetic scheduler benchmark. PowerShell affinity is stricter than Winderust Soft CPU Sets.'
    logical_processors = $logicalProcessors
    worker_count = $workerCount
    foreground_iterations_per_round = $iterations
    rounds = $rounds
    gentle_affinity_limited_processors = $gentleCoreCount
    balance_affinity_limited_processors = $balanceCoreCount
    responsive_affinity_limited_processors = $responsiveCoreCount
    baseline = $baseline
    off = $off
    presets = $cases
    comparisons_vs_off = $comparisons
} | ConvertTo-Json -Depth 6
```

## Interpreting Results

Use `avg_ms`, `median_ms`, and `p95_ms`. Lower is better.

- `Average foreground time (avg_ms)`: average milliseconds for the foreground CPU
  benchmark loop across all rounds.
- `Median foreground time (median_ms)`: middle round time after sorting the
  samples; useful when one round is an outlier.
- `P95 foreground time (p95_ms)`: near-worst round time. With 7 rounds this is
  effectively the second-slowest round, because the benchmark keeps the single
  worst round visible separately as `max_ms`.
- `Average vs Off`: percent change compared with the `Off` case. Positive means
  faster than Off; negative means slower than Off.
- `Foreground jitter (foreground_stddev_ms)`: standard deviation of foreground
  round times. Lower means the foreground work is more consistent.
- `Foreground range (foreground_range_ms)`: slowest round minus fastest round.
  Lower means fewer spikes.
- `P95 minus median (p95_minus_median_ms)`: tail-latency gap. Lower means the
  near-worst round is closer to normal behavior.
- `Foreground iterations/sec`: foreground work throughput derived from average
  time. Higher is better.
- `Background capacity percent`: approximate share of total logical CPU capacity
  consumed by the generated background workers during the foreground measurement.
  Lower usually means the preset is sacrificing more background throughput.
- `Background retained vs Off`: background capacity percent divided by the Off
  case. This helps show the foreground/background tradeoff.
- `Background tradeoff`: summary label from retained background capacity. `low`
  means at least 90% retained, `moderate` means 60% to 90%, and `high` means
  under 60%.

Prefer changes that improve median and p95 together. Ignore one-off wins where
average improves only because of a single outlier. If a preset is slower by less
than about 3%, treat it as neutral unless repeated runs show the same direction.

Previous reference run after the one-parameter optimization pass:

| Case | Average foreground time | Median foreground time | P95 foreground time | Average vs Off |
| --- | ---: | ---: | ---: | ---: |
| Off | 289.93 ms | 288.64 ms | 297.23 ms | baseline |
| Gentle | 261.99 ms | 263.03 ms | 268.07 ms | 9.6% faster |
| Balance | 250.00 ms | 248.33 ms | 257.24 ms | 13.8% faster |
| Responsive | 148.42 ms | 145.27 ms | 151.42 ms | 48.8% faster |

Richer reference run with stability and background-throughput metrics:

| Case | Average foreground time | P95 foreground time | Foreground jitter | P95 minus median | Foreground iterations/sec | Background retained vs Off |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Off | 844.28 ms | 892.48 ms | 59.31 ms | 71.09 ms | 1,480,548 | baseline |
| Gentle | 826.93 ms | 853.80 ms | 19.35 ms | 36.50 ms | 1,511,608 | 96.1% |
| Balance | 966.13 ms | 1292.23 ms | 218.17 ms | 434.27 ms | 1,293,828 | 87.7% |
| Responsive | 251.25 ms | 256.34 ms | 6.07 ms | 2.87 ms | 4,975,123 | 84.2% |

That richer run shows why a single compact score is risky: Responsive improved
both speed and stability, Gentle mostly improved stability, and Balance had a
large tail-latency outlier despite being good in the simpler run.

Previous paired methodology validation on Intel Core 5 210H, 12 logical processors:

| Case | Median improvement avg | P95 improvement avg | Agreement | Signal |
| --- | ---: | ---: | ---: | --- |
| Gentle | -0.7% | -2.2% | 0% | noisy |
| Balance | -14.1% | -16.2% | 0% | noisy |
| Responsive | 60.1% | 54.9% | 100% | strong |

Use this result to avoid over-tuning Gentle and Balance from this synthetic
loop. It validates the method for large scheduling changes, but priority-only
changes need longer runs, more hardware, or real app traces before changing
global defaults.

Previous paired validation after adding background-capacity measurement on the same CPU:

| Case | Median improvement avg | P95 improvement avg | Background retained avg | Signal | Tradeoff |
| --- | ---: | ---: | ---: | --- | --- |
| Gentle | 10.8% | 41.5% | 100.0% | usable | low |
| Balance | 10.2% | -62.7% | 117.2% | noisy | low |
| Responsive | 72.9% | 84.7% | 29.5% | strong | high |

This makes the tradeoff clearer: Responsive is the only strong foreground win,
but it deliberately gives up background throughput. Gentle may be useful for a
light touch, while Balance still needs real workload traces before more tuning.

Latest Winderust launch scenario on AMD Ryzen 7 7735HS after launch-grace tuning:

| Case | Median improvement avg | Median improvement min | P95 improvement avg | P95 improvement min | Background retained avg | Signal | Tradeoff |
| --- | ---: | ---: | ---: | ---: | ---: | --- | --- |
| Gentle | 3.8% | 0.0% | 3.8% | 0.0% | 99.8% | noisy | low |
| Balance | -4.1% | -11.3% | -4.1% | -11.3% | 99.7% | noisy | low |
| Responsive | -4.8% | -12.5% | -4.8% | -12.5% | 99.9% | noisy | low |

Launch grace keeps background throughput intact while the foreground app starts,
but this app-launch scenario still does not validate stronger Balance or
Responsive launch behavior. Treat the CPU-loop wins as scheduler headroom, not
guaranteed app-startup improvement.

## Known Limitations

- The benchmark applies I/O and memory priority to workers, but the foreground
  loop is CPU-bound and does not include dedicated disk or memory-pressure
  phases.
- GPU priority is attempted against generated workers; CPU-only workers may not
  have a GPU context, so `gpu_priority_unavailable` is expected on many systems.
- It does not test real foreground-app detection, Winderust exclusions, restore,
  cooldown, launch boost,
  or failure handling.
- Hard affinity may make CPU-share behavior look harsher than Winderust Soft CPU
  Sets.
- Thermal throttling and Windows background services can move results by several
  percent.

## After Changes

Run:

```powershell
rtk cargo check
rtk cargo test auto_balance
rtk cargo test responsiveness
rtk cargo test
rtk git diff --check
```

If `graphify-out/graph.json` exists, run:

```powershell
graphify update .
```

In the current workspace, `graphify-out` has been absent and `graphify update .`
has failed with:

```text
error: uv trampoline failed to canonicalize script path
```
