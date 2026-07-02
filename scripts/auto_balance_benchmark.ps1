param(
    [int]$Passes = 3,
    [int]$Rounds = 5,
    [int]$Iterations = 1000000,
    [int]$WorkerSeconds = 45
)

$ErrorActionPreference = 'Stop'

$powerShellPath = Join-Path $PSHOME 'powershell.exe'
$logicalProcessors = [Environment]::ProcessorCount
$workerCount = [Math]::Min([Math]::Max($logicalProcessors, 4), 12)
$gentleTargetCount = [Math]::Min(4, $workerCount)
$balanceCorePercent = 0.50
$responsiveCorePercent = 0.16
$balanceCoreCount = [Math]::Max(1, [Math]::Ceiling($logicalProcessors * $balanceCorePercent))
$responsiveCoreCount = [Math]::Max(1, [Math]::Ceiling($logicalProcessors * $responsiveCorePercent))

function New-Mask([int]$Count) {
    $mask = 0L
    for ($core = 0; $core -lt [Math]::Min($Count, 62); $core++) {
        $mask = $mask -bor (1L -shl $core)
    }
    return $mask
}

$balanceMask = New-Mask $balanceCoreCount
$responsiveMask = New-Mask $responsiveCoreCount

function Get-CpuName {
    try {
        return ((Get-CimInstance Win32_Processor | Select-Object -First 1 -ExpandProperty Name).Trim())
    } catch {
        return 'unknown'
    }
}

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

function Get-WorkerCpuMilliseconds {
    param([object[]]$Processes)
    $total = 0.0
    foreach ($process in $Processes) {
        if ($null -eq $process) {
            continue
        }
        try {
            $process.Refresh()
            $total += $process.TotalProcessorTime.TotalMilliseconds
        } catch {
        }
    }
    return $total
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

function Get-Average {
    param([double[]]$Values)
    $sum = 0.0
    foreach ($value in $Values) {
        $sum += $value
    }
    return $sum / $Values.Length
}

function Summarize-Samples {
    param([string]$Name, [double[]]$Samples, [string]$Model)
    $sorted = [double[]]$Samples.Clone()
    [Array]::Sort($sorted)
    $avg = Get-Average $Samples
    $variance = 0.0
    foreach ($sample in $Samples) {
        $variance += [Math]::Pow($sample - $avg, 2)
    }
    $variance = $variance / $Samples.Length
    $medianIndex = [int][Math]::Floor(($Samples.Length - 1) * 0.50)
    $p95Index = [int][Math]::Floor(($Samples.Length - 1) * 0.95)
    $median = $sorted[$medianIndex]
    $p95 = $sorted[$p95Index]
    return [pscustomobject]@{
        name = $Name
        model = $Model
        samples_ms = [double[]]$Samples
        avg_ms = [Math]::Round($avg, 2)
        median_ms = [Math]::Round($median, 2)
        p95_ms = [Math]::Round($p95, 2)
        min_ms = [Math]::Round($sorted[0], 2)
        max_ms = [Math]::Round($sorted[$sorted.Length - 1], 2)
        stddev_ms = [Math]::Round([Math]::Sqrt($variance), 2)
        range_ms = [Math]::Round($sorted[$sorted.Length - 1] - $sorted[0], 2)
        p95_minus_median_ms = [Math]::Round($p95 - $median, 2)
        iterations_per_sec = [Math]::Round(($Iterations / ($avg / 1000.0)), 0)
    }
}

function Get-ImprovementPercent {
    param([double]$OffValue, [double]$CaseValue)
    if ($OffValue -le 0.0) {
        return 0.0
    }
    return [Math]::Round((($OffValue - $CaseValue) / $OffValue) * 100.0, 1)
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
            -Seconds $WorkerSeconds
        Start-Sleep -Seconds 2
        $workerCpuBeforeMs = Get-WorkerCpuMilliseconds $processes
        $measurementWindow = [Diagnostics.Stopwatch]::StartNew()
        $samples = Measure-ForegroundWork -Iterations $Iterations -Rounds $Rounds
        $measurementWindow.Stop()
        $workerCpuAfterMs = Get-WorkerCpuMilliseconds $processes
        $summary = Summarize-Samples -Name $Name -Samples $samples -Model $Model
        $workerCpuDeltaMs = [Math]::Max(0.0, $workerCpuAfterMs - $workerCpuBeforeMs)
        $capacity = if ($measurementWindow.Elapsed.TotalMilliseconds -gt 0.0) {
            ($workerCpuDeltaMs / ($measurementWindow.Elapsed.TotalMilliseconds * $logicalProcessors)) * 100.0
        } else {
            0.0
        }
        $summary | Add-Member -NotePropertyName measurement_window_ms -NotePropertyValue ([Math]::Round($measurementWindow.Elapsed.TotalMilliseconds, 2))
        $summary | Add-Member -NotePropertyName background_cpu_ms -NotePropertyValue ([Math]::Round($workerCpuDeltaMs, 2))
        $summary | Add-Member -NotePropertyName background_capacity_percent -NotePropertyValue ([Math]::Round($capacity, 1))
        return $summary
    } finally {
        Stop-CpuWorkers -Processes $processes
        try {
            $currentProcess.PriorityClass = $originalPriority
        } catch {
        }
        Start-Sleep -Seconds 1
    }
}

function Run-NamedCase {
    param([string]$Name)
    switch ($Name) {
        'off' {
            return Run-Case `
                -Name 'off' `
                -Model 'Background Normal; foreground Normal.' `
                -ForegroundPriority 'Normal' `
                -Priorities (New-Priorities -DefaultPriority 'Normal' -RestrainedCount 0 -RestrainedPriority 'Normal') `
                -AffinitySelectedCount 0 `
                -AffinityMask 0
        }
        'gentle' {
            return Run-Case `
                -Name 'gentle' `
                -Model '4 selected background workers Idle; foreground Normal.' `
                -ForegroundPriority 'Normal' `
                -Priorities (New-Priorities -DefaultPriority 'Normal' -RestrainedCount $gentleTargetCount -RestrainedPriority 'Idle') `
                -AffinitySelectedCount 0 `
                -AffinityMask 0
        }
        'balance' {
            return Run-Case `
                -Name 'balance' `
                -Model 'All background workers Idle; 50% affinity approximation; foreground AboveNormal.' `
                -ForegroundPriority 'AboveNormal' `
                -Priorities (New-Priorities -DefaultPriority 'Idle' -RestrainedCount $workerCount -RestrainedPriority 'Idle') `
                -AffinitySelectedCount ([Math]::Min(12, $workerCount)) `
                -AffinityMask $balanceMask
        }
        'responsive' {
            return Run-Case `
                -Name 'responsive' `
                -Model 'All background workers Idle; 16% affinity approximation; foreground AboveNormal.' `
                -ForegroundPriority 'AboveNormal' `
                -Priorities (New-Priorities -DefaultPriority 'Idle' -RestrainedCount $workerCount -RestrainedPriority 'Idle') `
                -AffinitySelectedCount ([Math]::Min(12, $workerCount)) `
                -AffinityMask $responsiveMask
        }
    }
}

function New-Comparison {
    param($Off, $Case)
    $backgroundRetained = if ($Off.background_capacity_percent -gt 0.0) {
        ($Case.background_capacity_percent / $Off.background_capacity_percent) * 100.0
    } else {
        0.0
    }
    [pscustomobject]@{
        name = $Case.name
        avg_improvement_percent_vs_off = Get-ImprovementPercent -OffValue $Off.avg_ms -CaseValue $Case.avg_ms
        median_improvement_percent_vs_off = Get-ImprovementPercent -OffValue $Off.median_ms -CaseValue $Case.median_ms
        p95_improvement_percent_vs_off = Get-ImprovementPercent -OffValue $Off.p95_ms -CaseValue $Case.p95_ms
        jitter_improvement_percent_vs_off = Get-ImprovementPercent -OffValue $Off.stddev_ms -CaseValue $Case.stddev_ms
        background_capacity_percent = $Case.background_capacity_percent
        background_retained_percent_vs_off = [Math]::Round($backgroundRetained, 1)
    }
}

function Run-Pass {
    param([int]$Pass)
    $presetOrders = @(
        @('gentle', 'balance', 'responsive'),
        @('responsive', 'balance', 'gentle'),
        @('balance', 'gentle', 'responsive')
    )
    $presetOrder = $presetOrders[($Pass - 1) % $presetOrders.Count]
    $currentProcess = [Diagnostics.Process]::GetCurrentProcess()
    $originalPriority = $currentProcess.PriorityClass
    try {
        $currentProcess.PriorityClass = 'Normal'
        $baseline = Summarize-Samples `
            -Name 'baseline_no_background_load' `
            -Samples (Measure-ForegroundWork -Iterations $Iterations -Rounds $Rounds) `
            -Model 'No generated background load.'
    } finally {
        try {
            $currentProcess.PriorityClass = $originalPriority
        } catch {
        }
    }

    $pairs = @()
    $cases = @()
    $comparisons = @()
    for ($index = 0; $index -lt $presetOrder.Count; $index++) {
        $preset = $presetOrder[$index]
        if ((($Pass + $index) % 2) -eq 0) {
            $order = @('off', $preset)
        } else {
            $order = @($preset, 'off')
        }
        $results = @{}
        foreach ($name in $order) {
            $results[$name] = Run-NamedCase $name
        }
        $case = $results[$preset]
        $comparison = New-Comparison -Off $results['off'] -Case $case
        $cases += $case
        $comparisons += $comparison
        $pairs += [pscustomobject]@{
            preset = $preset
            order = $order
            off = $results['off']
            case = $case
            comparison_vs_off = $comparison
        }
    }
    return [pscustomobject]@{
        pass = $Pass
        preset_order = $presetOrder
        baseline = $baseline
        pairs = $pairs
        presets = $cases
        comparisons_vs_off = $comparisons
    }
}

function Summarize-Method {
    param($Runs)
    foreach ($name in @('gentle', 'balance', 'responsive')) {
        $comparisons = @()
        foreach ($run in $Runs) {
            $comparisons += @($run.comparisons_vs_off | Where-Object { $_.name -eq $name })
        }
        $medianValues = @($comparisons | ForEach-Object { [double]$_.median_improvement_percent_vs_off })
        $p95Values = @($comparisons | ForEach-Object { [double]$_.p95_improvement_percent_vs_off })
        $backgroundRetainedValues = @($comparisons | ForEach-Object { [double]$_.background_retained_percent_vs_off })
        $medianAvg = Get-Average $medianValues
        $p95Avg = Get-Average $p95Values
        $backgroundRetainedAvg = Get-Average $backgroundRetainedValues
        $medianWins = @($medianValues | Where-Object { $_ -ge 3.0 }).Count
        $p95Wins = @($p95Values | Where-Object { $_ -ge 3.0 }).Count
        $agreement = [Math]::Round(([Math]::Min($medianWins, $p95Wins) / $comparisons.Count) * 100.0, 1)
        $tradeoff = if ($backgroundRetainedAvg -ge 90.0) {
            'low'
        } elseif ($backgroundRetainedAvg -ge 60.0) {
            'moderate'
        } else {
            'high'
        }
        $signal = if ($agreement -eq 100.0 -and $medianAvg -ge 5.0 -and $p95Avg -ge 5.0) {
            'strong'
        } elseif ($agreement -ge 66.0 -and $medianAvg -gt 0.0 -and $p95Avg -gt 0.0) {
            'usable'
        } else {
            'noisy'
        }
        [pscustomobject]@{
            name = $name
            passes = $comparisons.Count
            median_improvement_avg_percent = [Math]::Round($medianAvg, 1)
            median_improvement_min_percent = [Math]::Round(($medianValues | Measure-Object -Minimum).Minimum, 1)
            p95_improvement_avg_percent = [Math]::Round($p95Avg, 1)
            p95_improvement_min_percent = [Math]::Round(($p95Values | Measure-Object -Minimum).Minimum, 1)
            background_retained_avg_percent = [Math]::Round($backgroundRetainedAvg, 1)
            background_retained_min_percent = [Math]::Round(($backgroundRetainedValues | Measure-Object -Minimum).Minimum, 1)
            background_tradeoff = $tradeoff
            agreement_percent = $agreement
            signal = $signal
        }
    }
}

$runs = @()
for ($pass = 1; $pass -le $Passes; $pass++) {
    $runs += Run-Pass $pass
}

[pscustomobject]@{
    note = 'Synthetic local scheduler benchmark. Use this to validate tuning direction, not universal defaults.'
    cpu_name = Get-CpuName
    logical_processors = $logicalProcessors
    worker_count = $workerCount
    passes = $Passes
    rounds = $Rounds
    balance_affinity_limited_processors = $balanceCoreCount
    foreground_iterations_per_round = $Iterations
    responsive_affinity_limited_processors = $responsiveCoreCount
    methodology_gate = 'Trust a local tuning direction only when median and p95 both improve by at least 3% in at least two of three passes.'
    runs = $runs
    method_summary = @(Summarize-Method $runs)
} | ConvertTo-Json -Depth 8
