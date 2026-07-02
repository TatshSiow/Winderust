param(
    [int]$Passes = 3,
    [int]$Rounds = 5,
    [int]$Iterations = 1000000,
    [int]$WorkerSeconds = 45,
    [ValidateSet('CpuLoop', 'TaskManagerLaunch', 'WinderustLaunch')]
    [string]$ForegroundScenario = 'CpuLoop',
    [string]$WinderustExePath = ''
)

$ErrorActionPreference = 'Stop'

$powerShellPath = Join-Path $PSHOME 'powershell.exe'
$logicalProcessors = [Environment]::ProcessorCount
$workerCount = [Math]::Min([Math]::Max($logicalProcessors, 4), 12)
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

if (-not ('AutoBalanceBenchmarkNative' -as [type])) {
    Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class AutoBalanceBenchmarkNative
{
    [StructLayout(LayoutKind.Sequential)]
    public struct MEMORY_PRIORITY_INFORMATION
    {
        public UInt32 MemoryPriority;
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool SetProcessInformation(IntPtr hProcess, Int32 processInformationClass, ref MEMORY_PRIORITY_INFORMATION processInformation, UInt32 processInformationSize);

    [DllImport("ntdll.dll")]
    public static extern Int32 NtSetInformationProcess(IntPtr processHandle, UInt32 processInformationClass, ref UInt32 processInformation, UInt32 processInformationLength);

    [DllImport("gdi32.dll")]
    public static extern Int32 D3DKMTSetProcessSchedulingPriorityClass(IntPtr processHandle, Int32 priority);
}
"@
}

$processMemoryPriorityClass = 0
$processIoPriorityClass = 33
$statusInvalidParameter = -1073741811
$memoryPriorityRaw = @{
    VeryLow = 1
    Low = 2
    Medium = 3
    BelowNormal = 4
    Normal = 5
}
$ioPriorityRaw = @{
    VeryLow = 0
    Low = 1
    Normal = 2
    High = 3
    Critical = 4
}
$gpuPriorityRaw = @{
    Idle = 0
    BelowNormal = 1
    Normal = 2
    AboveNormal = 3
    High = 4
    Realtime = 5
}

function Get-CpuName {
    try {
        return ((Get-CimInstance Win32_Processor | Select-Object -First 1 -ExpandProperty Name).Trim())
    } catch {
        return 'unknown'
    }
}

function New-AssistControls {
    param(
        [string]$ForegroundPriorityBoost = 'Default',
        [string]$BackgroundPriorityBoost = 'Default',
        [string]$ThreadPriority = 'Default',
        [string]$MemoryPriority = 'Default',
        [string]$IoPriority = 'Default',
        [string]$GpuPriority = 'Default'
    )

    [pscustomobject][ordered]@{
        foreground_priority_boost = $ForegroundPriorityBoost
        background_priority_boost = $BackgroundPriorityBoost
        thread_priority = $ThreadPriority
        memory_priority = $MemoryPriority
        io_priority = $IoPriority
        gpu_priority = $GpuPriority
    }
}

function New-AssistStatus {
    [pscustomobject][ordered]@{
        worker_processes = 0
        foreground_priority_boost_applied = 0
        background_priority_boost_applied = 0
        thread_priority_threads_applied = 0
        memory_priority_processes_applied = 0
        io_priority_processes_applied = 0
        gpu_priority_processes_applied = 0
        gpu_priority_unavailable = 0
        failed_actions = 0
    }
}

function Set-ProcessPriorityBoostSetting {
    param([Diagnostics.Process]$Process, [string]$Setting)
    if ([string]::IsNullOrWhiteSpace($Setting) -or $Setting -eq 'Default') {
        return $false
    }
    $Process.PriorityBoostEnabled = ($Setting -eq 'Enabled')
    return $true
}

function Apply-WorkerAssistControls {
    param(
        [Diagnostics.Process]$Process,
        [pscustomobject]$AssistControls,
        [pscustomobject]$AssistStatus
    )

    $AssistStatus.worker_processes += 1
    if ($AssistControls.background_priority_boost -ne 'Default') {
        try {
            if (Set-ProcessPriorityBoostSetting -Process $Process -Setting $AssistControls.background_priority_boost) {
                $AssistStatus.background_priority_boost_applied += 1
            }
        } catch {
            $AssistStatus.failed_actions += 1
        }
    }
    if ($AssistControls.thread_priority -ne 'Default') {
        try {
            $Process.Refresh()
            foreach ($thread in $Process.Threads) {
                try {
                    $thread.PriorityLevel = $AssistControls.thread_priority
                    $AssistStatus.thread_priority_threads_applied += 1
                } catch {
                    $AssistStatus.failed_actions += 1
                }
            }
        } catch {
            $AssistStatus.failed_actions += 1
        }
    }
    if ($memoryPriorityRaw.ContainsKey($AssistControls.memory_priority)) {
        try {
            $info = New-Object 'AutoBalanceBenchmarkNative+MEMORY_PRIORITY_INFORMATION'
            $info.MemoryPriority = [uint32]$memoryPriorityRaw[$AssistControls.memory_priority]
            if ([AutoBalanceBenchmarkNative]::SetProcessInformation($Process.Handle, $processMemoryPriorityClass, [ref]$info, [uint32]4)) {
                $AssistStatus.memory_priority_processes_applied += 1
            } else {
                $AssistStatus.failed_actions += 1
            }
        } catch {
            $AssistStatus.failed_actions += 1
        }
    }
    if ($ioPriorityRaw.ContainsKey($AssistControls.io_priority)) {
        try {
            $raw = [uint32]$ioPriorityRaw[$AssistControls.io_priority]
            $status = [AutoBalanceBenchmarkNative]::NtSetInformationProcess($Process.Handle, [uint32]$processIoPriorityClass, [ref]$raw, [uint32]4)
            if ($status -ge 0) {
                $AssistStatus.io_priority_processes_applied += 1
            } else {
                $AssistStatus.failed_actions += 1
            }
        } catch {
            $AssistStatus.failed_actions += 1
        }
    }
    if ($gpuPriorityRaw.ContainsKey($AssistControls.gpu_priority)) {
        try {
            $raw = [int]$gpuPriorityRaw[$AssistControls.gpu_priority]
            $status = [AutoBalanceBenchmarkNative]::D3DKMTSetProcessSchedulingPriorityClass($Process.Handle, $raw)
            if ($status -ge 0) {
                $AssistStatus.gpu_priority_processes_applied += 1
            } elseif ($status -eq $statusInvalidParameter) {
                $AssistStatus.gpu_priority_unavailable += 1
            } else {
                $AssistStatus.failed_actions += 1
            }
        } catch {
            $AssistStatus.failed_actions += 1
        }
    }
}

function Close-TaskManagerWindows {
    param([int[]]$ExistingProcessIds)
    foreach ($process in @(Get-Process -Name Taskmgr -ErrorAction SilentlyContinue)) {
        try {
            $process.Refresh()
            if ($process.MainWindowHandle -eq 0) {
                continue
            }
            [void]$process.CloseMainWindow()
            if ($ExistingProcessIds -notcontains $process.Id -and -not $process.WaitForExit(1500)) {
                $process.Kill()
                [void]$process.WaitForExit(2000)
            }
        } catch {
        }
    }
    $deadline = [DateTime]::UtcNow.AddSeconds(3)
    while ([DateTime]::UtcNow -lt $deadline) {
        $visible = @(Get-Process -Name Taskmgr -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 })
        if ($visible.Count -eq 0) {
            break
        }
        Start-Sleep -Milliseconds 100
    }
}

function Measure-TaskManagerLaunch {
    param([int]$Rounds)
    $samples = New-Object 'System.Collections.Generic.List[double]'
    $taskManagerPath = Join-Path $env:WINDIR 'System32\taskmgr.exe'
    for ($round = 0; $round -lt $Rounds; $round++) {
        $existing = @(Get-Process -Name Taskmgr -ErrorAction SilentlyContinue)
        if (@($existing | Where-Object { $_.MainWindowHandle -ne 0 }).Count -gt 0) {
            throw 'Close the visible Task Manager window before running -ForegroundScenario TaskManagerLaunch.'
        }
        $existingIds = @($existing | ForEach-Object { [int]$_.Id })
        [GC]::Collect()
        $sw = [Diagnostics.Stopwatch]::StartNew()
        try {
            [void](Start-Process -FilePath $taskManagerPath -PassThru)
            while ($sw.Elapsed.TotalMilliseconds -lt 10000) {
                Start-Sleep -Milliseconds 50
                $visible = @(Get-Process -Name Taskmgr -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 })
                if ($visible.Count -gt 0) {
                    break
                }
            }
            $sw.Stop()
            $samples.Add($sw.Elapsed.TotalMilliseconds)
        } finally {
            Close-TaskManagerWindows -ExistingProcessIds $existingIds
            Start-Sleep -Milliseconds 250
        }
    }
    return $samples.ToArray()
}

function Resolve-WinderustExePath {
    if (-not [string]::IsNullOrWhiteSpace($WinderustExePath)) {
        $resolved = Resolve-Path -LiteralPath $WinderustExePath -ErrorAction Stop
        return $resolved.Path
    }
    foreach ($candidate in @(
            (Join-Path $PSScriptRoot '..\target\release\winderust.exe'),
            (Join-Path $PSScriptRoot '..\target\debug\winderust.exe')
        )) {
        if (Test-Path -LiteralPath $candidate) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }
    throw 'Build Winderust first, or pass -WinderustExePath <path-to-winderust.exe>.'
}

function Stop-WinderustLaunchProcess {
    param([Diagnostics.Process]$Process)
    if ($null -eq $Process) {
        return
    }
    try {
        $Process.Refresh()
        if (-not $Process.HasExited) {
            [void]$Process.CloseMainWindow()
            if (-not $Process.WaitForExit(2000)) {
                $Process.Kill()
                [void]$Process.WaitForExit(3000)
            }
        }
    } catch {
    }
}

function Measure-WinderustLaunch {
    param([int]$Rounds, [string]$LaunchPriority)
    $samples = New-Object 'System.Collections.Generic.List[double]'
    $exePath = Resolve-WinderustExePath
    for ($round = 0; $round -lt $Rounds; $round++) {
        if (Get-Process -Name winderust -ErrorAction SilentlyContinue) {
            throw 'Close Winderust before running -ForegroundScenario WinderustLaunch.'
        }
        [GC]::Collect()
        $process = $null
        $inputIdle = $false
        $sw = [Diagnostics.Stopwatch]::StartNew()
        try {
            $process = Start-Process -FilePath $exePath -PassThru
            try {
                $process.PriorityClass = $LaunchPriority
            } catch {
            }
            while ($sw.Elapsed.TotalMilliseconds -lt 15000) {
                Start-Sleep -Milliseconds 50
                $process.Refresh()
                if ($process.HasExited) {
                    throw 'Winderust exited before it became ready.'
                }
                if ($process.MainWindowHandle -ne 0) {
                    break
                }
                if (-not $inputIdle) {
                    try {
                        $inputIdle = $process.WaitForInputIdle(1)
                    } catch {
                    }
                    if ($inputIdle) {
                        break
                    }
                }
            }
            $sw.Stop()
            $samples.Add($sw.Elapsed.TotalMilliseconds)
        } finally {
            Stop-WinderustLaunchProcess -Process $process
            Start-Sleep -Milliseconds 500
        }
    }
    return $samples.ToArray()
}

function Measure-ForegroundWork {
    param([int]$Iterations, [int]$Rounds, [string]$LaunchPriority)
    if ($ForegroundScenario -eq 'TaskManagerLaunch') {
        return Measure-TaskManagerLaunch -Rounds $Rounds
    }
    if ($ForegroundScenario -eq 'WinderustLaunch') {
        return Measure-WinderustLaunch -Rounds $Rounds -LaunchPriority $LaunchPriority
    }
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
        [int]$Seconds,
        [pscustomobject]$AssistControls,
        [pscustomobject]$AssistStatus
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
        if ($null -ne $AssistControls -and $null -ne $AssistStatus) {
            Apply-WorkerAssistControls -Process $process -AssistControls $AssistControls -AssistStatus $AssistStatus
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

function Test-ForegroundLaunchScenario {
    return $ForegroundScenario -ne 'CpuLoop'
}

function Run-LaunchGraceCase {
    param([string]$Name)
    return Run-Case `
        -Name $Name `
        -Model 'Launch grace: foreground launch boosted AboveNormal; background restraints deferred.' `
        -ForegroundPriority 'AboveNormal' `
        -Priorities (New-Priorities -DefaultPriority 'Normal' -RestrainedCount 0 -RestrainedPriority 'Normal') `
        -AffinitySelectedCount 0 `
        -AffinityMask 0 `
        -AssistControls (New-AssistControls)
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
    $summary = [ordered]@{
        name = $Name
        model = $Model
        foreground_scenario = $ForegroundScenario
        samples_ms = [double[]]$Samples
        avg_ms = [Math]::Round($avg, 2)
        median_ms = [Math]::Round($median, 2)
        p95_ms = [Math]::Round($p95, 2)
        min_ms = [Math]::Round($sorted[0], 2)
        max_ms = [Math]::Round($sorted[$sorted.Length - 1], 2)
        stddev_ms = [Math]::Round([Math]::Sqrt($variance), 2)
        range_ms = [Math]::Round($sorted[$sorted.Length - 1] - $sorted[0], 2)
        p95_minus_median_ms = [Math]::Round($p95 - $median, 2)
    }
    if ($ForegroundScenario -eq 'CpuLoop') {
        $summary.iterations_per_sec = [Math]::Round(($Iterations / ($avg / 1000.0)), 0)
    } else {
        $summary.launches_per_sec = [Math]::Round((1000.0 / $avg), 2)
    }
    return [pscustomobject]$summary
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
        [Int64]$AffinityMask,
        [pscustomobject]$AssistControls
    )

    if ($null -eq $AssistControls) {
        $AssistControls = New-AssistControls
    }
    $assistStatus = New-AssistStatus
    $currentProcess = [Diagnostics.Process]::GetCurrentProcess()
    $originalPriority = $currentProcess.PriorityClass
    $originalPriorityBoost = $null
    try {
        $originalPriorityBoost = $currentProcess.PriorityBoostEnabled
    } catch {
    }
    $processes = @()
    try {
        $currentProcess.PriorityClass = $ForegroundPriority
        try {
            if (Set-ProcessPriorityBoostSetting -Process $currentProcess -Setting $AssistControls.foreground_priority_boost) {
                $assistStatus.foreground_priority_boost_applied += 1
            }
        } catch {
            $assistStatus.failed_actions += 1
        }
        $processes = Start-CpuWorkers `
            -Priorities $Priorities `
            -AffinitySelectedCount $AffinitySelectedCount `
            -AffinityMask $AffinityMask `
            -Seconds $WorkerSeconds `
            -AssistControls $AssistControls `
            -AssistStatus $assistStatus
        Start-Sleep -Seconds 2
        $workerCpuBeforeMs = Get-WorkerCpuMilliseconds $processes
        $measurementWindow = [Diagnostics.Stopwatch]::StartNew()
        $samples = Measure-ForegroundWork -Iterations $Iterations -Rounds $Rounds -LaunchPriority $ForegroundPriority
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
        $summary | Add-Member -NotePropertyName assist_controls -NotePropertyValue $AssistControls
        $summary | Add-Member -NotePropertyName assist_status -NotePropertyValue $assistStatus
        return $summary
    } finally {
        Stop-CpuWorkers -Processes $processes
        try {
            $currentProcess.PriorityClass = $originalPriority
        } catch {
        }
        if ($null -ne $originalPriorityBoost) {
            try {
                $currentProcess.PriorityBoostEnabled = $originalPriorityBoost
            } catch {
            }
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
                -AffinityMask 0 `
                -AssistControls (New-AssistControls)
        }
        'gentle' {
            if (Test-ForegroundLaunchScenario) {
                return Run-LaunchGraceCase -Name 'gentle'
            }
            return Run-Case `
                -Name 'gentle' `
                -Model 'All background workers Idle; 60% affinity approximation; foreground AboveNormal.' `
                -ForegroundPriority 'AboveNormal' `
                -Priorities (New-Priorities -DefaultPriority 'Idle' -RestrainedCount $gentleTargetCount -RestrainedPriority 'Idle') `
                -AffinitySelectedCount ([Math]::Min(12, $workerCount)) `
                -AffinityMask $gentleMask `
                -AssistControls (New-AssistControls)
        }
        'balance' {
            if (Test-ForegroundLaunchScenario) {
                return Run-LaunchGraceCase -Name 'balance'
            }
            return Run-Case `
                -Name 'balance' `
                -Model 'All background workers Idle; 50% affinity approximation; foreground AboveNormal; background I/O Low; memory Low; threads BelowNormal; priority boost disabled; GPU BelowNormal when available.' `
                -ForegroundPriority 'AboveNormal' `
                -Priorities (New-Priorities -DefaultPriority 'Idle' -RestrainedCount $workerCount -RestrainedPriority 'Idle') `
                -AffinitySelectedCount ([Math]::Min(12, $workerCount)) `
                -AffinityMask $balanceMask `
                -AssistControls (New-AssistControls -ForegroundPriorityBoost 'Enabled' -BackgroundPriorityBoost 'Disabled' -ThreadPriority 'BelowNormal' -MemoryPriority 'Low' -IoPriority 'Low' -GpuPriority 'BelowNormal')
        }
        'responsive' {
            if (Test-ForegroundLaunchScenario) {
                return Run-LaunchGraceCase -Name 'responsive'
            }
            return Run-Case `
                -Name 'responsive' `
                -Model 'All background workers Idle; 16% affinity approximation; foreground AboveNormal; background I/O VeryLow; memory VeryLow; threads BelowNormal; priority boost disabled; GPU BelowNormal when available.' `
                -ForegroundPriority 'AboveNormal' `
                -Priorities (New-Priorities -DefaultPriority 'Idle' -RestrainedCount $workerCount -RestrainedPriority 'Idle') `
                -AffinitySelectedCount ([Math]::Min(12, $workerCount)) `
                -AffinityMask $responsiveMask `
                -AssistControls (New-AssistControls -ForegroundPriorityBoost 'Enabled' -BackgroundPriorityBoost 'Disabled' -ThreadPriority 'BelowNormal' -MemoryPriority 'VeryLow' -IoPriority 'VeryLow' -GpuPriority 'BelowNormal')
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
            -Samples (Measure-ForegroundWork -Iterations $Iterations -Rounds $Rounds -LaunchPriority 'Normal') `
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

$assistCoverage = [pscustomobject][ordered]@{
    process_priority = 'applied to generated background workers'
    foreground_process_priority = 'applied to the benchmark process'
    affinity = 'applied as hard affinity; stricter than Winderust Soft CPU Sets'
    foreground_priority_boost = 'applied to the benchmark process when preset enables it'
    background_priority_boost = 'applied to generated background workers when preset enables it'
    thread_priority = 'applied to existing generated worker threads when preset enables it'
    memory_priority = 'applied to generated background workers when preset enables it'
    io_priority = 'applied to generated background workers when preset enables it; CPU loop has minimal I/O'
    gpu_priority = 'attempted on generated workers when preset enables it; CPU workers may report no GPU context'
    foreground_detection = 'modeled by treating the benchmark process as foreground; the app automation loop is not launched'
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
    foreground_scenario = $ForegroundScenario
    gentle_affinity_limited_processors = $gentleCoreCount
    balance_affinity_limited_processors = $balanceCoreCount
    foreground_iterations_per_round = $Iterations
    responsive_affinity_limited_processors = $responsiveCoreCount
    assist_coverage = $assistCoverage
    methodology_gate = 'Trust a local tuning direction only when median and p95 both improve by at least 3% in at least two of three passes.'
    runs = $runs
    method_summary = @(Summarize-Method $runs)
} | ConvertTo-Json -Depth 8
