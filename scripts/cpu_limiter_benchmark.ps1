param(
    [ValidateRange(1, 8)]
    [int]$Passes = 2,
    [ValidateRange(5, 120)]
    [int]$SampleSeconds = 20,
    [ValidateRange(2, 30)]
    [int]$WarmupSeconds = 4,
    [int[]]$Percentages = @(1, 25, 50),
    [string]$WinderustExePath = '.\target\release\winderust.exe',
    [string]$OutputPath = ''
)

$ErrorActionPreference = 'Stop'

function New-CpuLimiterMeasurement {
    param(
        [double]$WorkerCpuBeforeMs,
        [double]$WorkerCpuAfterMs,
        [double]$WinderustCpuBeforeMs,
        [double]$WinderustCpuAfterMs,
        [double]$ElapsedMilliseconds,
        [int]$LogicalProcessors
    )

    if ($ElapsedMilliseconds -le 0) {
        throw 'ElapsedMilliseconds must be greater than zero.'
    }
    if ($LogicalProcessors -le 0) {
        throw 'LogicalProcessors must be greater than zero.'
    }

    $workerCpuMs = [Math]::Max(0.0, $WorkerCpuAfterMs - $WorkerCpuBeforeMs)
    $winderustCpuMs = [Math]::Max(0.0, $WinderustCpuAfterMs - $WinderustCpuBeforeMs)
    $workerOneCorePercent = ($workerCpuMs / $ElapsedMilliseconds) * 100.0
    $winderustOneCorePercent = ($winderustCpuMs / $ElapsedMilliseconds) * 100.0

    [pscustomobject][ordered]@{
        worker_cpu_ms = [Math]::Round($workerCpuMs, 4)
        worker_one_core_percent = [Math]::Round($workerOneCorePercent, 4)
        worker_task_manager_percent = [Math]::Round($workerOneCorePercent / $LogicalProcessors, 4)
        winderust_cpu_ms = [Math]::Round($winderustCpuMs, 4)
        winderust_one_core_percent = [Math]::Round($winderustOneCorePercent, 4)
        winderust_task_manager_percent = [Math]::Round($winderustOneCorePercent / $LogicalProcessors, 4)
    }
}

if (-not ('CpuLimiterBenchmarkNative' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class CpuLimiterBenchmarkNative
{
    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool IsProcessInJob(
        IntPtr processHandle,
        IntPtr jobHandle,
        [MarshalAs(UnmanagedType.Bool)] out bool result);
}
'@
}

function Test-ProcessInAnyJob {
    param([Diagnostics.Process]$Process)

    $inJob = $false
    if (-not [CpuLimiterBenchmarkNative]::IsProcessInJob($Process.Handle, [IntPtr]::Zero, [ref]$inJob)) {
        $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
        throw "IsProcessInJob failed with error $errorCode."
    }
    return $inJob
}

function Test-IsAdministrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

if ($env:WINDERUST_CPU_LIMITER_BENCHMARK_IMPORT_ONLY -eq '1') {
    return
}

if (-not (Test-IsAdministrator)) {
    throw 'Run the CPU Limiter benchmark from an administrator PowerShell session.'
}

$percentages = @($Percentages | Sort-Object -Unique)
if ($percentages.Count -eq 0 -or @($percentages | Where-Object { $_ -lt 1 -or $_ -gt 100 }).Count -gt 0) {
    throw 'Percentages must contain unique values from 1 through 100.'
}
if (@(Get-Process -Name winderust -ErrorAction SilentlyContinue).Count -gt 0) {
    throw 'Close Winderust before running the CPU Limiter benchmark.'
}

$sourceExePath = (Resolve-Path -LiteralPath $WinderustExePath).Path
$logicalProcessors = [Environment]::ProcessorCount
$cscriptPath = Join-Path $env:SystemRoot 'System32\cscript.exe'
$configDir = Join-Path ([IO.Path]::GetTempPath()) "winderust-cpu-limiter-benchmark-$([guid]::NewGuid())"
$exePath = Join-Path $configDir 'winderust.exe'
$workerDir = Join-Path $configDir 'worker'
$workerExePath = Join-Path $workerDir 'cscript.exe'
$configPath = Join-Path $configDir 'settings.toml'
$workerScriptPath = Join-Path $configDir 'cpu-worker.js'
$startupRegistryPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$startupRegistryName = 'Winderust'

function Get-CpuName {
    try {
        return (Get-CimInstance Win32_Processor | Select-Object -First 1 -ExpandProperty Name).Trim()
    } catch {
        return $env:PROCESSOR_IDENTIFIER
    }
}

function Get-StartupRegistrationSnapshot {
    $present = $false
    $value = $null
    if (Test-Path -LiteralPath $startupRegistryPath) {
        $item = Get-ItemProperty -LiteralPath $startupRegistryPath
        $property = $item.PSObject.Properties[$startupRegistryName]
        if ($null -ne $property) {
            $present = $true
            $value = [string]$property.Value
        }
    }
    [pscustomobject]@{ present = $present; value = $value }
}

function Restore-StartupRegistration {
    param([pscustomobject]$Snapshot)

    if ($Snapshot.present) {
        if (-not (Test-Path -LiteralPath $startupRegistryPath)) {
            New-Item -Path $startupRegistryPath -Force | Out-Null
        }
        New-ItemProperty `
            -LiteralPath $startupRegistryPath `
            -Name $startupRegistryName `
            -Value $Snapshot.value `
            -PropertyType String `
            -Force | Out-Null
    } elseif (Test-Path -LiteralPath $startupRegistryPath) {
        Remove-ItemProperty `
            -LiteralPath $startupRegistryPath `
            -Name $startupRegistryName `
            -ErrorAction SilentlyContinue
    }
}

function New-IsolatedSettingsToml {
    param([bool]$LimiterEnabled, [int]$AllowedPercent)

    $escapedWorkerPath = $workerExePath.Replace('\', '\\')
    $enabled = $LimiterEnabled.ToString().ToLowerInvariant()
    $percent = $AllowedPercent.ToString([Globalization.CultureInfo]::InvariantCulture)
    $template = @'
adaptive_engine_presets = []

[general]
enabled = true
startup_with_windows = false
start_minimized = true
hide_to_tray = false
allow_cross_session_process_control = true
check_for_updates = false
check_interval_ms = 1000

[adaptive_engine]
enabled = false
processor_power_policy_enabled = false

[adaptive_engine.base_processor_policy]
core_parking_min = 100
performance_min = 5
performance_max = 100
boost_policy = 100
boost_mode = "aggressive"

[adaptive_engine.background_pressure_profile]
ac_policy = 100
ac_mode = "aggressive"
battery_policy = 100
battery_mode = "aggressive"

[adaptive_engine.focus_and_launch_profile]
ac_policy = 100
ac_mode = "aggressive"
battery_policy = 100
battery_mode = "aggressive"

[cpu_scheduler]
process_priority_enabled = false
background_efficiency_enabled = false
focus_process_background_efficiency_override_enabled = false
visible_window_background_efficiency_override_enabled = false
focus_process_background_efficiency_mode = false
visible_window_background_efficiency_mode = false
background_efficiency_mode = false
focus_process_priority = "normal"
visible_window_priority = "normal"
background_priority = "normal"
io_priority = { enabled = false }
thread_priority = { enabled = false }
dynamic_priority_boost = { enabled = false }
gpu_priority = { enabled = false }
memory_priority_enabled = false
focus_process_memory_priority = "default"
visible_window_memory_priority = "default"
background_memory_priority = "default"
cpu_pressure_restraint_enabled = false
limit_background_processors_enabled = false
dynamic_resource_zones_enabled = false
cpu_allocation_method = "cpu_sets_soft"
background_processor_selection = "least_used"
processor_limit_percent = 100
specific_processors = []
foreground_or_system_cpu_threshold_percent = 100
background_app_cpu_threshold_percent = 100
cpu_recovery_threshold_percent = 100
reaction_time_ms = 1000
cpu_restraint_time_seconds = 1
cpu_recovery_time_seconds = 1
maximum_restrained_apps = 1
custom_rules = []

[cpu_limiter]
enabled = __LIMITER_ENABLED__
focus_allowed_cpu_time_percent = __ALLOWED_PERCENT__
visible_window_allowed_cpu_time_percent = __ALLOWED_PERCENT__
background_allowed_cpu_time_percent = __ALLOWED_PERCENT__

[[cpu_limiter.rules]]
enabled = true
executable_path = "__WORKER_PATH__"
focus_mode = "enabled"
visible_window_mode = "enabled"
background_mode = "enabled"
focus_allowed_cpu_time_percent = __ALLOWED_PERCENT__
visible_window_allowed_cpu_time_percent = __ALLOWED_PERCENT__
background_allowed_cpu_time_percent = __ALLOWED_PERCENT__

[by_activity]
enabled = false
idle_timeout_seconds = 300
switch_to_performance_on_resume = false

[by_foreground]
enabled = false
rules = []

[by_time]
enabled = false
rules = []
'@
    return $template.Replace('__LIMITER_ENABLED__', $enabled).
        Replace('__WORKER_PATH__', $escapedWorkerPath).
        Replace('__ALLOWED_PERCENT__', $percent)
}

function Start-CpuWorker {
    param([int]$Seconds)

    $code = @'
var deadline = Date.now() + parseInt(WScript.Arguments(0), 10) * 1000;
var acc = 0;
while (Date.now() < deadline) {
    for (var i = 1; i <= 100000; i++) {
        acc += Math.sqrt(i);
    }
}
'@
    [IO.File]::WriteAllText($workerScriptPath, $code, [Text.UTF8Encoding]::new($false))
    $startup = New-CimInstance -ClassName Win32_ProcessStartup -ClientOnly -Property @{
        ShowWindow = [uint16]0
    }
    $commandLine = '"{0}" //B //NoLogo "{1}" {2}' -f $workerExePath, $workerScriptPath, $Seconds
    $created = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{
        CommandLine = $commandLine
        ProcessStartupInformation = $startup
    }
    if ($created.ReturnValue -ne 0) {
        throw "Failed to start detached CPU worker: Win32_Process.Create returned $($created.ReturnValue)."
    }
    return [Diagnostics.Process]::GetProcessById([int]$created.ProcessId)
}

function Stop-ProcessIfRunning {
    param([Diagnostics.Process]$Process)

    if ($null -eq $Process) {
        return
    }
    try {
        $Process.Refresh()
        if (-not $Process.HasExited) {
            Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
            [void]$Process.WaitForExit(3000)
        }
    } catch {
    }
}

function Stop-WinderustRuntime {
    param([Diagnostics.Process]$Process)

    if ($null -eq $Process) {
        return
    }
    try {
        $Process.Refresh()
        if (-not $Process.HasExited) {
            [void]$Process.CloseMainWindow()
            if (-not $Process.WaitForExit(8000)) {
                Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
                [void]$Process.WaitForExit(3000)
            }
        }
    } catch {
        Stop-ProcessIfRunning -Process $Process
    }
}

function Invoke-CpuLimiterCase {
    param([int]$AllowedPercent)

    $limiterEnabled = $AllowedPercent -gt 0
    $configuredPercent = if ($limiterEnabled) { $AllowedPercent } else { 50 }
    $settings = New-IsolatedSettingsToml `
        -LimiterEnabled $limiterEnabled `
        -AllowedPercent $configuredPercent
    [IO.File]::WriteAllText($configPath, $settings, [Text.UTF8Encoding]::new($false))

    $worker = $null
    $runtime = $null
    try {
        $worker = Start-CpuWorker -Seconds ($WarmupSeconds + $SampleSeconds + 30)
        Start-Sleep -Milliseconds 250
        $worker.Refresh()
        $workerInJobBefore = Test-ProcessInAnyJob -Process $worker

        $runtime = Start-Process -FilePath $exePath -PassThru -WindowStyle Hidden
        Start-Sleep -Seconds 2
        $runtime.Refresh()
        if ($runtime.HasExited) {
            throw 'Winderust exited during benchmark startup.'
        }

        Start-Sleep -Seconds $WarmupSeconds
        $worker.Refresh()
        $runtime.Refresh()
        if ($worker.HasExited) {
            throw 'CPU worker exited during benchmark warmup.'
        }
        $workerCpuBeforeMs = $worker.TotalProcessorTime.TotalMilliseconds
        $winderustCpuBeforeMs = $runtime.TotalProcessorTime.TotalMilliseconds
        $measurementWindow = [Diagnostics.Stopwatch]::StartNew()
        Start-Sleep -Seconds $SampleSeconds
        $measurementWindow.Stop()
        $worker.Refresh()
        $runtime.Refresh()
        $measurement = New-CpuLimiterMeasurement `
            -WorkerCpuBeforeMs $workerCpuBeforeMs `
            -WorkerCpuAfterMs $worker.TotalProcessorTime.TotalMilliseconds `
            -WinderustCpuBeforeMs $winderustCpuBeforeMs `
            -WinderustCpuAfterMs $runtime.TotalProcessorTime.TotalMilliseconds `
            -ElapsedMilliseconds $measurementWindow.Elapsed.TotalMilliseconds `
            -LogicalProcessors $logicalProcessors
        $workerInJobAfter = Test-ProcessInAnyJob -Process $worker

        [pscustomobject][ordered]@{
            case = if ($limiterEnabled) { "limit_$AllowedPercent" } else { 'off' }
            allowed_cpu_time_percent = if ($limiterEnabled) { $AllowedPercent } else { $null }
            worker_in_job_before = $workerInJobBefore
            worker_in_job_after = $workerInJobAfter
            backend_observation = if (-not $limiterEnabled) {
                'disabled'
            } elseif (-not $workerInJobBefore -and $workerInJobAfter) {
                'job_object_observed'
            } elseif ($workerInJobBefore) {
                'existing_job_hybrid_path'
            } else {
                'backend_not_observed'
            }
            measurement = $measurement
        }
    } finally {
        try {
            Stop-WinderustRuntime -Process $runtime
        } finally {
            Stop-ProcessIfRunning -Process $worker
        }
    }
}

function Remove-IsolatedConfig {
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
    $fullConfigDir = [IO.Path]::GetFullPath($configDir)
    $expectedPrefix = "$tempRoot\winderust-cpu-limiter-benchmark-"
    if (-not $fullConfigDir.StartsWith($expectedPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to remove unexpected benchmark directory: $fullConfigDir"
    }
    if (-not [IO.Directory]::Exists($fullConfigDir)) {
        return
    }
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        try {
            Remove-Item -LiteralPath $fullConfigDir -Recurse -Force -ErrorAction Stop
            return
        } catch {
            if ($attempt -eq 39) {
                throw
            }
            Start-Sleep -Milliseconds 250
        }
    }
}

function Get-AverageProperty {
    param([object[]]$Items, [string]$Name)

    $values = @($Items | ForEach-Object { [double]$_.$Name })
    return [Math]::Round(($values | Measure-Object -Average).Average, 4)
}

$startupSnapshot = Get-StartupRegistrationSnapshot
$runs = @()
[IO.Directory]::CreateDirectory($configDir) | Out-Null
[IO.Directory]::CreateDirectory($workerDir) | Out-Null
Copy-Item -LiteralPath $sourceExePath -Destination $exePath
Copy-Item -LiteralPath $cscriptPath -Destination $workerExePath
try {
    $forwardOrder = @(0) + $percentages
    for ($pass = 1; $pass -le $Passes; $pass++) {
        $order = if (($pass % 2) -eq 1) {
            $forwardOrder
        } else {
            @($forwardOrder[($forwardOrder.Count - 1)..0])
        }
        $caseRows = @()
        foreach ($allowedPercent in $order) {
            $caseRows += Invoke-CpuLimiterCase -AllowedPercent $allowedPercent
        }
        $off = @($caseRows | Where-Object { $_.case -eq 'off' })[0]
        foreach ($row in $caseRows) {
            $retainedPercent = if ($row.case -eq 'off') {
                100.0
            } else {
                [Math]::Round(
                    ($row.measurement.worker_one_core_percent / $off.measurement.worker_one_core_percent) * 100.0,
                    4
                )
            }
            $row | Add-Member -NotePropertyName retained_cpu_time_percent_vs_off -NotePropertyValue $retainedPercent
        }
        $runs += [pscustomobject][ordered]@{
            pass = $pass
            order = @($order | ForEach-Object { if ($_ -eq 0) { 'off' } else { "limit_$_" } })
            cases = $caseRows
        }
    }
} finally {
    Restore-StartupRegistration -Snapshot $startupSnapshot
    Remove-IsolatedConfig
}

$allRows = @($runs | ForEach-Object { $_.cases })
$summary = @(
    foreach ($allowedPercent in $forwardOrder) {
        $caseName = if ($allowedPercent -eq 0) { 'off' } else { "limit_$allowedPercent" }
        $rows = @($allRows | Where-Object { $_.case -eq $caseName })
        $retained = Get-AverageProperty -Items $rows -Name 'retained_cpu_time_percent_vs_off'
        $oneCorePercent = Get-AverageProperty -Items @($rows | ForEach-Object { $_.measurement }) -Name 'worker_one_core_percent'
        $errorPoints = if ($allowedPercent -eq 0) { 0.0 } else { [Math]::Round($oneCorePercent - $allowedPercent, 4) }
        $tolerance = if ($allowedPercent -eq 0) { $null } else { [Math]::Max(2.0, $allowedPercent * 0.15) }
        [pscustomobject][ordered]@{
            case = $caseName
            allowed_cpu_time_percent = if ($allowedPercent -eq 0) { $null } else { $allowedPercent }
            worker_one_core_percent = $oneCorePercent
            worker_task_manager_percent = Get-AverageProperty -Items @($rows | ForEach-Object { $_.measurement }) -Name 'worker_task_manager_percent'
            retained_cpu_time_percent_vs_off = $retained
            error_percentage_points = $errorPoints
            within_tolerance = if ($null -eq $tolerance) { $true } else { [Math]::Abs($errorPoints) -le $tolerance }
            winderust_task_manager_percent = Get-AverageProperty -Items @($rows | ForEach-Object { $_.measurement }) -Name 'winderust_task_manager_percent'
            backend_observations = @($rows | Select-Object -ExpandProperty backend_observation -Unique)
        }
    }
)

$limitedSummary = @($summary | Where-Object { $null -ne $_.allowed_cpu_time_percent })
$observedRetained = @($limitedSummary | Select-Object -ExpandProperty retained_cpu_time_percent_vs_off)
$monotonic = $true
for ($index = 1; $index -lt $observedRetained.Count; $index++) {
    if ($observedRetained[$index] -lt $observedRetained[$index - 1]) {
        $monotonic = $false
        break
    }
}

$report = [pscustomobject][ordered]@{
    note = 'Release-runtime CPU Limiter benchmark using one disposable single-thread CPU worker. Accuracy is measured against the configured absolute percentage of one logical core; Task Manager percentages are total-system estimates derived from process CPU time.'
    cpu_name = Get-CpuName
    logical_processors = $logicalProcessors
    passes = $Passes
    sample_seconds = $SampleSeconds
    warmup_seconds = $WarmupSeconds
    percentages = $percentages
    winderust_executable = $sourceExePath
    validation = [pscustomobject][ordered]@{
        passed = $summary[0].worker_one_core_percent -ge 80.0 -and $monotonic -and @($limitedSummary | Where-Object { -not $_.within_tolerance }).Count -eq 0
        off_worker_reached_80_percent_of_one_core = $summary[0].worker_one_core_percent -ge 80.0
        retained_cpu_time_is_monotonic = $monotonic
        all_limited_cases_within_tolerance = @($limitedSummary | Where-Object { -not $_.within_tolerance }).Count -eq 0
    }
    summary = $summary
    runs = $runs
}

$json = $report | ConvertTo-Json -Depth 8
if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    $timestamp = Get-Date -Format 'yyyyMMdd-HHmmss'
    $OutputPath = Join-Path (Split-Path $PSScriptRoot -Parent) "benchmark\results\cpu-limiter-$timestamp.json"
}
$outputDirectory = Split-Path $OutputPath -Parent
if (-not [string]::IsNullOrWhiteSpace($outputDirectory)) {
    [IO.Directory]::CreateDirectory($outputDirectory) | Out-Null
}
[IO.File]::WriteAllText($OutputPath, $json, [Text.UTF8Encoding]::new($false))
$json
