param(
    [int]$Passes = 3,
    [int]$Rounds = 5,
    [int]$Iterations = 1000000,
    [int]$WorkerSeconds = 45,
    [ValidateSet('CpuLoop', 'IoLoop', 'MessageLoop', 'WinderustLaunch')]
    [string]$ForegroundScenario = 'CpuLoop',
    [int]$IoOperations = 2000,
    [int]$MessageLoopTicks = 200,
    [int]$MessageLoopIntervalMilliseconds = 16,
    [string]$WinderustExePath = ''
)

$ErrorActionPreference = 'Stop'

$powerShellPath = Join-Path $PSHOME 'powershell.exe'
$logicalProcessors = [Environment]::ProcessorCount
$workerCount = [Math]::Min([Math]::Max($logicalProcessors, 4), 12)
$lowImpactTargetCount = $workerCount
$maxForegroundCorePercent = 0.10
$lowImpactCoreCount = 0
$foregroundFirstCoreCount = 0
$maxForegroundCoreCount = [Math]::Max(1, [Math]::Ceiling($logicalProcessors * $maxForegroundCorePercent))

function New-Mask([int]$Count) {
    $mask = 0L
    for ($core = 0; $core -lt [Math]::Min($Count, 62); $core++) {
        $mask = $mask -bor (1L -shl $core)
    }
    return $mask
}

$maxForegroundMask = New-Mask $maxForegroundCoreCount

if (-not ('WorkloadEngineBenchmarkNative' -as [type])) {
    Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class WorkloadEngineBenchmarkNative
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
        [string]$ForegroundThreadPriority = 'Default',
        [string]$ForegroundIoPriority = 'Default',
        [string]$ForegroundGpuPriority = 'Default',
        [string]$BackgroundPriorityBoost = 'Default',
        [string]$ThreadPriority = 'Default',
        [string]$MemoryPriority = 'Default',
        [string]$IoPriority = 'Default',
        [string]$GpuPriority = 'Default'
    )

    [pscustomobject][ordered]@{
        foreground_priority_boost = $ForegroundPriorityBoost
        foreground_thread_priority = $ForegroundThreadPriority
        foreground_io_priority = $ForegroundIoPriority
        foreground_gpu_priority = $ForegroundGpuPriority
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
        foreground_thread_priority_applied = 0
        foreground_io_priority_applied = 0
        foreground_gpu_priority_applied = 0
        foreground_gpu_priority_unavailable = 0
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

function Set-CurrentThreadPrioritySetting {
    param([string]$Setting)
    if ([string]::IsNullOrWhiteSpace($Setting) -or $Setting -eq 'Default') {
        return $false
    }

    [Threading.Thread]::CurrentThread.Priority = $Setting
    return $true
}

function Set-ProcessIoPrioritySetting {
    param([Diagnostics.Process]$Process, [string]$Setting)
    if (-not $ioPriorityRaw.ContainsKey($Setting)) {
        return $false
    }

    $raw = [uint32]$ioPriorityRaw[$Setting]
    $status = [WorkloadEngineBenchmarkNative]::NtSetInformationProcess($Process.Handle, [uint32]$processIoPriorityClass, [ref]$raw, [uint32]4)
    if ($status -lt 0) {
        throw "NtSetInformationProcess failed with status $status"
    }
    return $true
}

function Set-ProcessGpuPrioritySetting {
    param([Diagnostics.Process]$Process, [string]$Setting)
    if (-not $gpuPriorityRaw.ContainsKey($Setting)) {
        return 'Skipped'
    }

    $raw = [int]$gpuPriorityRaw[$Setting]
    $status = [WorkloadEngineBenchmarkNative]::D3DKMTSetProcessSchedulingPriorityClass($Process.Handle, $raw)
    if ($status -ge 0) {
        return 'Applied'
    }
    if ($status -eq $statusInvalidParameter) {
        return 'Unavailable'
    }
    throw "D3DKMTSetProcessSchedulingPriorityClass failed with status $status"
}

function Apply-ForegroundAssistControls {
    param(
        [Diagnostics.Process]$Process,
        [pscustomobject]$AssistControls,
        [pscustomobject]$AssistStatus
    )

    try {
        if (Set-CurrentThreadPrioritySetting -Setting $AssistControls.foreground_thread_priority) {
            $AssistStatus.foreground_thread_priority_applied += 1
        }
    } catch {
        $AssistStatus.failed_actions += 1
    }
    try {
        if (Set-ProcessIoPrioritySetting -Process $Process -Setting $AssistControls.foreground_io_priority) {
            $AssistStatus.foreground_io_priority_applied += 1
        }
    } catch {
        $AssistStatus.failed_actions += 1
    }
    try {
        $gpuStatus = Set-ProcessGpuPrioritySetting -Process $Process -Setting $AssistControls.foreground_gpu_priority
        if ($gpuStatus -eq 'Applied') {
            $AssistStatus.foreground_gpu_priority_applied += 1
        } elseif ($gpuStatus -eq 'Unavailable') {
            $AssistStatus.foreground_gpu_priority_unavailable += 1
        }
    } catch {
        $AssistStatus.failed_actions += 1
    }
}

function Restore-ForegroundAssistControls {
    param(
        [Diagnostics.Process]$Process,
        [Threading.ThreadPriority]$OriginalThreadPriority,
        [pscustomobject]$AssistControls
    )

    try {
        [Threading.Thread]::CurrentThread.Priority = $OriginalThreadPriority
    } catch {
    }
    if ($ioPriorityRaw.ContainsKey($AssistControls.foreground_io_priority)) {
        try {
            [void](Set-ProcessIoPrioritySetting -Process $Process -Setting 'Normal')
        } catch {
        }
    }
    if ($gpuPriorityRaw.ContainsKey($AssistControls.foreground_gpu_priority)) {
        try {
            [void](Set-ProcessGpuPrioritySetting -Process $Process -Setting 'Normal')
        } catch {
        }
    }
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
            $info = New-Object 'WorkloadEngineBenchmarkNative+MEMORY_PRIORITY_INFORMATION'
            $info.MemoryPriority = [uint32]$memoryPriorityRaw[$AssistControls.memory_priority]
            if ([WorkloadEngineBenchmarkNative]::SetProcessInformation($Process.Handle, $processMemoryPriorityClass, [ref]$info, [uint32]4)) {
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
            if (Set-ProcessIoPrioritySetting -Process $Process -Setting $AssistControls.io_priority) {
                $AssistStatus.io_priority_processes_applied += 1
            }
        } catch {
            $AssistStatus.failed_actions += 1
        }
    }
    if ($gpuPriorityRaw.ContainsKey($AssistControls.gpu_priority)) {
        try {
            $gpuStatus = Set-ProcessGpuPrioritySetting -Process $Process -Setting $AssistControls.gpu_priority
            if ($gpuStatus -eq 'Applied') {
                $AssistStatus.gpu_priority_processes_applied += 1
            } elseif ($gpuStatus -eq 'Unavailable') {
                $AssistStatus.gpu_priority_unavailable += 1
            }
        } catch {
            $AssistStatus.failed_actions += 1
        }
    }
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
    if ($ForegroundScenario -eq 'WinderustLaunch') {
        return Measure-WinderustLaunch -Rounds $Rounds -LaunchPriority $LaunchPriority
    }
    if ($ForegroundScenario -eq 'IoLoop') {
        return Measure-ForegroundIoWork -Operations $IoOperations -Rounds $Rounds
    }
    if ($ForegroundScenario -eq 'MessageLoop') {
        return Measure-ForegroundMessageLoop -Ticks $MessageLoopTicks -IntervalMilliseconds $MessageLoopIntervalMilliseconds -Rounds $Rounds
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

function Measure-ForegroundMessageLoop {
    param([int]$Ticks, [int]$IntervalMilliseconds, [int]$Rounds)
    if ($Ticks -lt 1) {
        throw '-MessageLoopTicks must be at least 1.'
    }
    if ($IntervalMilliseconds -lt 1) {
        throw '-MessageLoopIntervalMilliseconds must be at least 1.'
    }
    Add-Type -AssemblyName System.Windows.Forms
    Add-Type -AssemblyName System.Drawing

    $samples = New-Object 'System.Collections.Generic.List[double]'
    for ($round = 0; $round -lt $Rounds; $round++) {
        [GC]::Collect()
        $state = [pscustomobject]@{
            Count = 0
            LastMs = 0.0
            TotalDelayMs = 0.0
        }
        $form = New-Object System.Windows.Forms.Form
        $timer = New-Object System.Windows.Forms.Timer
        try {
            $form.ShowInTaskbar = $false
            $form.WindowState = [System.Windows.Forms.FormWindowState]::Minimized
            $form.Opacity = 0
            $form.Size = New-Object System.Drawing.Size(1, 1)
            $form.StartPosition = [System.Windows.Forms.FormStartPosition]::Manual
            $form.Location = New-Object System.Drawing.Point(-32000, -32000)
            $timer.Interval = $IntervalMilliseconds
            $sw = [Diagnostics.Stopwatch]::StartNew()
            $form.Add_Shown({
                $state.LastMs = $sw.Elapsed.TotalMilliseconds
                $timer.Start()
            })
            $timer.Add_Tick({
                $elapsed = $sw.Elapsed.TotalMilliseconds
                $delay = [Math]::Max(0.0, ($elapsed - $state.LastMs) - $IntervalMilliseconds)
                $state.Count += 1
                $state.TotalDelayMs += $delay
                $state.LastMs = $elapsed
                if ($state.Count -ge $Ticks) {
                    $timer.Stop()
                    $form.Close()
                }
            })
            [System.Windows.Forms.Application]::Run($form)
            $samples.Add($state.TotalDelayMs / [Math]::Max(1, $state.Count))
        } finally {
            $timer.Dispose()
            $form.Dispose()
        }
        Start-Sleep -Milliseconds 150
    }
    return $samples.ToArray()
}

function Measure-ForegroundIoWork {
    param([int]$Operations, [int]$Rounds)
    $samples = New-Object 'System.Collections.Generic.List[double]'
    $buffer = New-Object byte[] 4096
    for ($index = 0; $index -lt $buffer.Length; $index++) {
        $buffer[$index] = [byte]($index % 251)
    }
    for ($round = 0; $round -lt $Rounds; $round++) {
        $path = Join-Path ([IO.Path]::GetTempPath()) ("winderust-workload-engine-bench-$PID-$round.bin")
        [GC]::Collect()
        $stream = $null
        $sw = [Diagnostics.Stopwatch]::StartNew()
        try {
            $stream = [IO.File]::Open($path, [IO.FileMode]::Create, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
            for ($operation = 0; $operation -lt $Operations; $operation++) {
                $stream.Write($buffer, 0, $buffer.Length)
            }
            $stream.Flush()
            $stream.Position = 0
            for ($operation = 0; $operation -lt $Operations; $operation++) {
                if ($stream.Read($buffer, 0, $buffer.Length) -le 0) {
                    break
                }
            }
            $sw.Stop()
            $samples.Add($sw.Elapsed.TotalMilliseconds)
        } finally {
            if ($null -ne $stream) {
                $stream.Dispose()
            }
            Remove-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
        }
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
    return $ForegroundScenario -eq 'WinderustLaunch'
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
    } elseif ($ForegroundScenario -eq 'IoLoop') {
        $summary.foreground_iops = [Math]::Round((($IoOperations * 2) / ($avg / 1000.0)), 0)
    } elseif ($ForegroundScenario -eq 'MessageLoop') {
        $summary.message_loop_avg_delay_ms = [Math]::Round($avg, 2)
        $summary.message_loop_p95_delay_ms = [Math]::Round($p95, 2)
        $summary.message_loop_ticks = $MessageLoopTicks
        $summary.message_loop_interval_ms = $MessageLoopIntervalMilliseconds
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

function Get-DeltaMilliseconds {
    param([double]$OffValue, [double]$CaseValue)
    return [Math]::Round($CaseValue - $OffValue, 2)
}

function Format-DeltaWithPercent {
    param([double]$DeltaMs, [double]$Percent)
    $deltaSign = if ($DeltaMs -gt 0.0) { '+' } else { '' }
    $percentSign = if ($Percent -gt 0.0) { '+' } else { '' }
    return ('{0}{1:N2} ms ({2}{3:N1}%)' -f $deltaSign, $DeltaMs, $percentSign, $Percent)
}

function Format-CaseWithBaseline {
    param([double]$CaseMs, [double]$OffMs, [double]$DeltaMs, [double]$Percent)
    $deltaSign = if ($DeltaMs -gt 0.0) { '+' } else { '' }
    $percentSign = if ($Percent -gt 0.0) { '+' } else { '' }
    return ('{0:N2} ms vs {1:N2} ms paired Off ({2}{3:N2} ms, {4}{5:N1}%)' -f $CaseMs, $OffMs, $deltaSign, $DeltaMs, $percentSign, $Percent)
}

function Get-ForegroundLatencyBaselinePercent {
    param([double]$BaselineMs, [double]$CaseMs)
    if ($BaselineMs -le 0.0 -or $CaseMs -le 0.0) {
        return 0.0
    }
    return [Math]::Round(($BaselineMs / $CaseMs) * 100.0, 1)
}

function Get-AverageProperty {
    param($Items, [string]$Name)
    $values = @(
        $Items | ForEach-Object {
            $property = $_.PSObject.Properties[$Name]
            if ($null -ne $property) {
                [double]$property.Value
            }
        }
    )
    if ($values.Count -eq 0) {
        return $null
    }
    return [Math]::Round((Get-Average $values), 2)
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
    $originalThreadPriority = [Threading.Thread]::CurrentThread.Priority
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
        Apply-ForegroundAssistControls `
            -Process $currentProcess `
            -AssistControls $AssistControls `
            -AssistStatus $assistStatus
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
        $summary | Add-Member -NotePropertyName background_throughput_percent -NotePropertyValue ([Math]::Round($capacity, 1))
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
        Restore-ForegroundAssistControls `
            -Process $currentProcess `
            -OriginalThreadPriority $originalThreadPriority `
            -AssistControls $AssistControls
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
        'low_impact' {
            if (Test-ForegroundLaunchScenario) {
                return Run-LaunchGraceCase -Name 'low_impact'
            }
            return Run-Case `
                -Name 'low_impact' `
                -Model 'Low Impact: all background workers Idle; adaptive CPU share; foreground Auto boost modeled as AboveNormal for this low foreground CPU synthetic case; background threads BelowNormal; priority boost disabled.' `
                -ForegroundPriority 'AboveNormal' `
                -Priorities (New-Priorities -DefaultPriority 'Idle' -RestrainedCount $lowImpactTargetCount -RestrainedPriority 'Idle') `
                -AffinitySelectedCount 0 `
                -AffinityMask 0 `
                -AssistControls (New-AssistControls -ForegroundPriorityBoost 'Enabled' -BackgroundPriorityBoost 'Disabled' -ThreadPriority 'BelowNormal')
        }
        'foreground_first' {
            if (Test-ForegroundLaunchScenario) {
                return Run-LaunchGraceCase -Name 'foreground_first'
            }
            return Run-Case `
                -Name 'foreground_first' `
                -Model 'Foreground First: all background workers Idle; adaptive CPU share; foreground Auto boost modeled as AboveNormal for this low foreground CPU synthetic case; background I/O VeryLow; memory VeryLow; threads BelowNormal; priority boost disabled; GPU BelowNormal when available.' `
                -ForegroundPriority 'AboveNormal' `
                -Priorities (New-Priorities -DefaultPriority 'Idle' -RestrainedCount $workerCount -RestrainedPriority 'Idle') `
                -AffinitySelectedCount 0 `
                -AffinityMask 0 `
                -AssistControls (New-AssistControls -ForegroundPriorityBoost 'Enabled' -BackgroundPriorityBoost 'Disabled' -ThreadPriority 'BelowNormal' -MemoryPriority 'VeryLow' -IoPriority 'VeryLow' -GpuPriority 'BelowNormal')
        }
        'max_foreground' {
            if (Test-ForegroundLaunchScenario) {
                return Run-LaunchGraceCase -Name 'max_foreground'
            }
            return Run-Case `
                -Name 'max_foreground' `
                -Model 'Max Foreground: all background workers Idle; 10% affinity approximation; foreground AboveNormal boost; foreground I/O High; foreground thread Highest; foreground GPU High when available; background I/O VeryLow; memory VeryLow; threads Idle; priority boost disabled; GPU Idle when available.' `
                -ForegroundPriority 'AboveNormal' `
                -Priorities (New-Priorities -DefaultPriority 'Idle' -RestrainedCount $workerCount -RestrainedPriority 'Idle') `
                -AffinitySelectedCount ([Math]::Min(12, $workerCount)) `
                -AffinityMask $maxForegroundMask `
                -AssistControls (New-AssistControls -ForegroundPriorityBoost 'Enabled' -ForegroundThreadPriority 'Highest' -ForegroundIoPriority 'High' -ForegroundGpuPriority 'High' -BackgroundPriorityBoost 'Disabled' -ThreadPriority 'Idle' -MemoryPriority 'VeryLow' -IoPriority 'VeryLow' -GpuPriority 'Idle')
        }
    }
}

function New-Comparison {
    param($Off, $Case)
    $backgroundRetained = if ($Off.background_throughput_percent -gt 0.0) {
        ($Case.background_throughput_percent / $Off.background_throughput_percent) * 100.0
    } else {
        0.0
    }
    $avgDelta = Get-DeltaMilliseconds -OffValue $Off.avg_ms -CaseValue $Case.avg_ms
    $medianDelta = Get-DeltaMilliseconds -OffValue $Off.median_ms -CaseValue $Case.median_ms
    $p95Delta = Get-DeltaMilliseconds -OffValue $Off.p95_ms -CaseValue $Case.p95_ms
    $jitterDelta = Get-DeltaMilliseconds -OffValue $Off.stddev_ms -CaseValue $Case.stddev_ms
    $avgPercent = Get-ImprovementPercent -OffValue $Off.avg_ms -CaseValue $Case.avg_ms
    $medianPercent = Get-ImprovementPercent -OffValue $Off.median_ms -CaseValue $Case.median_ms
    $p95Percent = Get-ImprovementPercent -OffValue $Off.p95_ms -CaseValue $Case.p95_ms
    $jitterPercent = Get-ImprovementPercent -OffValue $Off.stddev_ms -CaseValue $Case.stddev_ms
    [pscustomobject]@{
        name = $Case.name
        avg_vs_off = Format-CaseWithBaseline -CaseMs $Case.avg_ms -OffMs $Off.avg_ms -DeltaMs $avgDelta -Percent $avgPercent
        avg_off_ms = $Off.avg_ms
        avg_case_ms = $Case.avg_ms
        avg_delta_vs_off = Format-DeltaWithPercent -DeltaMs $avgDelta -Percent $avgPercent
        avg_delta_ms_vs_off = $avgDelta
        avg_improvement_percent_vs_off = $avgPercent
        median_vs_off = Format-CaseWithBaseline -CaseMs $Case.median_ms -OffMs $Off.median_ms -DeltaMs $medianDelta -Percent $medianPercent
        median_off_ms = $Off.median_ms
        median_case_ms = $Case.median_ms
        median_delta_vs_off = Format-DeltaWithPercent -DeltaMs $medianDelta -Percent $medianPercent
        median_delta_ms_vs_off = $medianDelta
        median_improvement_percent_vs_off = $medianPercent
        p95_vs_off = Format-CaseWithBaseline -CaseMs $Case.p95_ms -OffMs $Off.p95_ms -DeltaMs $p95Delta -Percent $p95Percent
        p95_off_ms = $Off.p95_ms
        p95_case_ms = $Case.p95_ms
        p95_delta_vs_off = Format-DeltaWithPercent -DeltaMs $p95Delta -Percent $p95Percent
        p95_delta_ms_vs_off = $p95Delta
        p95_improvement_percent_vs_off = $p95Percent
        jitter_vs_off = Format-CaseWithBaseline -CaseMs $Case.stddev_ms -OffMs $Off.stddev_ms -DeltaMs $jitterDelta -Percent $jitterPercent
        jitter_off_ms = $Off.stddev_ms
        jitter_case_ms = $Case.stddev_ms
        jitter_delta_vs_off = Format-DeltaWithPercent -DeltaMs $jitterDelta -Percent $jitterPercent
        jitter_delta_ms_vs_off = $jitterDelta
        jitter_improvement_percent_vs_off = $jitterPercent
        background_throughput_percent = $Case.background_throughput_percent
        background_throughput_retained_percent_vs_off = [Math]::Round($backgroundRetained, 1)
    }
}

function Run-Pass {
    param([int]$Pass)
    $presetOrders = @(
        @('low_impact', 'foreground_first', 'max_foreground'),
        @('max_foreground', 'foreground_first', 'low_impact'),
        @('foreground_first', 'max_foreground', 'low_impact')
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
    $baselineRows = @($Runs | ForEach-Object { $_.baseline })
    $baselineAvg = Get-AverageProperty -Items $baselineRows -Name 'avg_ms'
    $baselineMedian = Get-AverageProperty -Items $baselineRows -Name 'median_ms'
    $baselineP95 = Get-AverageProperty -Items $baselineRows -Name 'p95_ms'
    $offRows = @($Runs | ForEach-Object { $_.pairs } | ForEach-Object { $_.off })
    $offAvg = Get-AverageProperty -Items $offRows -Name 'avg_ms'
    $offMedian = Get-AverageProperty -Items $offRows -Name 'median_ms'
    $offP95 = Get-AverageProperty -Items $offRows -Name 'p95_ms'
    [pscustomobject]@{
        name = 'off'
        off_sample_count = $offRows.Count
        foreground_latency_avg_ms = $offAvg
        foreground_latency_median_avg_ms = $offMedian
        foreground_latency_p95_avg_ms = $offP95
        foreground_latency_baseline_percent = Get-ForegroundLatencyBaselinePercent -BaselineMs $baselineAvg -CaseMs $offAvg
        no_background_avg_ms = $baselineAvg
        no_background_median_ms = $baselineMedian
        no_background_p95_ms = $baselineP95
        foreground_iterations_per_sec_avg = Get-AverageProperty -Items $offRows -Name 'iterations_per_sec'
        foreground_iops_avg = Get-AverageProperty -Items $offRows -Name 'foreground_iops'
        message_loop_avg_delay_ms_avg = Get-AverageProperty -Items $offRows -Name 'message_loop_avg_delay_ms'
        message_loop_p95_delay_ms_avg = Get-AverageProperty -Items $offRows -Name 'message_loop_p95_delay_ms'
        background_throughput_retained_avg_percent = 100.0
        background_throughput_retained_min_percent = 100.0
        repeat_passes_won = 'baseline'
        repeat_pass_win_count = $null
        repeat_pass_count = $null
        repeat_pass_win_rate_percent = $null
    }
    foreach ($name in @('low_impact', 'foreground_first', 'max_foreground')) {
        $comparisons = @()
        $caseRows = @()
        foreach ($run in $Runs) {
            $comparisons += @($run.comparisons_vs_off | Where-Object { $_.name -eq $name })
            $caseRows += @($run.presets | Where-Object { $_.name -eq $name })
        }
        $medianValues = @($comparisons | ForEach-Object { [double]$_.median_improvement_percent_vs_off })
        $p95Values = @($comparisons | ForEach-Object { [double]$_.p95_improvement_percent_vs_off })
        $medianOffValues = @($comparisons | ForEach-Object { [double]$_.median_off_ms })
        $medianCaseValues = @($comparisons | ForEach-Object { [double]$_.median_case_ms })
        $p95OffValues = @($comparisons | ForEach-Object { [double]$_.p95_off_ms })
        $p95CaseValues = @($comparisons | ForEach-Object { [double]$_.p95_case_ms })
        $medianDeltaValues = @($comparisons | ForEach-Object { [double]$_.median_delta_ms_vs_off })
        $p95DeltaValues = @($comparisons | ForEach-Object { [double]$_.p95_delta_ms_vs_off })
        $backgroundRetainedValues = @($comparisons | ForEach-Object { [double]$_.background_throughput_retained_percent_vs_off })
        $medianAvg = Get-Average $medianValues
        $p95Avg = Get-Average $p95Values
        $medianOffAvg = Get-Average $medianOffValues
        $medianCaseAvg = Get-Average $medianCaseValues
        $p95OffAvg = Get-Average $p95OffValues
        $p95CaseAvg = Get-Average $p95CaseValues
        $medianDeltaAvg = Get-Average $medianDeltaValues
        $p95DeltaAvg = Get-Average $p95DeltaValues
        $backgroundRetainedAvg = Get-Average $backgroundRetainedValues
        $repeatWins = @(
            $comparisons | Where-Object {
                $_.median_improvement_percent_vs_off -ge 3.0 -and
                $_.p95_improvement_percent_vs_off -ge 3.0
            }
        ).Count
        $repeatWinRate = [Math]::Round(($repeatWins / $comparisons.Count) * 100.0, 1)
        [pscustomobject]@{
            name = $name
            passes = $comparisons.Count
            foreground_latency_avg_ms = Get-AverageProperty -Items $caseRows -Name 'avg_ms'
            foreground_latency_median_avg_ms = [Math]::Round($medianCaseAvg, 2)
            foreground_latency_p95_avg_ms = [Math]::Round($p95CaseAvg, 2)
            foreground_latency_baseline_percent = Get-ForegroundLatencyBaselinePercent -BaselineMs $baselineAvg -CaseMs (Get-AverageProperty -Items $caseRows -Name 'avg_ms')
            foreground_iterations_per_sec_avg = Get-AverageProperty -Items $caseRows -Name 'iterations_per_sec'
            foreground_iops_avg = Get-AverageProperty -Items $caseRows -Name 'foreground_iops'
            message_loop_avg_delay_ms_avg = Get-AverageProperty -Items $caseRows -Name 'message_loop_avg_delay_ms'
            message_loop_p95_delay_ms_avg = Get-AverageProperty -Items $caseRows -Name 'message_loop_p95_delay_ms'
            median_vs_off_avg = Format-CaseWithBaseline -CaseMs $medianCaseAvg -OffMs $medianOffAvg -DeltaMs $medianDeltaAvg -Percent $medianAvg
            median_off_avg_ms = [Math]::Round($medianOffAvg, 2)
            median_case_avg_ms = [Math]::Round($medianCaseAvg, 2)
            median_improvement_avg = Format-DeltaWithPercent -DeltaMs $medianDeltaAvg -Percent $medianAvg
            median_delta_avg_ms_vs_off = [Math]::Round($medianDeltaAvg, 2)
            median_improvement_avg_percent = [Math]::Round($medianAvg, 1)
            median_improvement_min_percent = [Math]::Round(($medianValues | Measure-Object -Minimum).Minimum, 1)
            p95_vs_off_avg = Format-CaseWithBaseline -CaseMs $p95CaseAvg -OffMs $p95OffAvg -DeltaMs $p95DeltaAvg -Percent $p95Avg
            p95_off_avg_ms = [Math]::Round($p95OffAvg, 2)
            p95_case_avg_ms = [Math]::Round($p95CaseAvg, 2)
            p95_improvement_avg = Format-DeltaWithPercent -DeltaMs $p95DeltaAvg -Percent $p95Avg
            p95_delta_avg_ms_vs_off = [Math]::Round($p95DeltaAvg, 2)
            p95_improvement_avg_percent = [Math]::Round($p95Avg, 1)
            p95_improvement_min_percent = [Math]::Round(($p95Values | Measure-Object -Minimum).Minimum, 1)
            background_throughput_retained_avg_percent = [Math]::Round($backgroundRetainedAvg, 1)
            background_throughput_retained_min_percent = [Math]::Round(($backgroundRetainedValues | Measure-Object -Minimum).Minimum, 1)
            repeat_passes_won = "$repeatWins/$($comparisons.Count)"
            repeat_pass_win_count = $repeatWins
            repeat_pass_count = $comparisons.Count
            repeat_pass_win_rate_percent = $repeatWinRate
        }
    }
}

$assistCoverage = [pscustomobject][ordered]@{
    process_priority = 'applied to generated background workers'
    foreground_process_priority = 'applied to the benchmark process'
    affinity = 'applied as hard affinity for Max Foreground; adaptive presets omit affinity in low foreground CPU synthetic cases'
    foreground_priority_boost = 'applied to the benchmark process when preset enables it'
    foreground_thread_priority = 'applied to the benchmark thread when preset enables it'
    foreground_io_priority = 'applied to the benchmark process when preset enables it; CPU loop has minimal I/O'
    foreground_gpu_priority = 'attempted on the benchmark process when preset enables it; CPU loop may report no GPU context'
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
    low_impact_affinity_limited_processors = $lowImpactCoreCount
    foreground_iterations_per_round = $Iterations
    foreground_io_operations_per_round = $IoOperations * 2
    foreground_message_loop_ticks_per_round = $MessageLoopTicks
    foreground_message_loop_interval_ms = $MessageLoopIntervalMilliseconds
    foreground_first_affinity_limited_processors = $foregroundFirstCoreCount
    max_foreground_affinity_limited_processors = $maxForegroundCoreCount
    assist_coverage = $assistCoverage
    methodology_gate = 'Trust a local tuning direction only when median and p95 both improve by at least 3% in at least two of three passes.'
    runs = $runs
    method_summary = @(Summarize-Method $runs)
} | ConvertTo-Json -Depth 8
