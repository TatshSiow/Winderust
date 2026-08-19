param(
    [ValidateRange(10, 600)]
    [int]$MeasurementSeconds = 30,
    [ValidateRange(100, 5000)]
    [int]$SampleIntervalMilliseconds = 500,
    [ValidateRange(3, 20)]
    [int]$ActionTrials = 5,
    [ValidateRange(1000, 30000)]
    [int]$ActionTimeoutMilliseconds = 15000,
    [string]$TargetDir = 'target\architecture-baseline',
    [string]$OutputPath = '',
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$diagnosticsEnvironmentVariable = 'WINDERUST_ARCHITECTURE_DIAGNOSTICS_PATH'
$repositoryRoot = Split-Path $PSScriptRoot -Parent
$resolvedTargetDir = Join-Path $repositoryRoot $TargetDir
$sourceExePath = Join-Path $resolvedTargetDir 'release\winderust.exe'
$logicalProcessors = [Environment]::ProcessorCount
$runDirectory = Join-Path ([IO.Path]::GetTempPath()) "winderust-architecture-baseline-$([guid]::NewGuid())"
$isolatedExePath = Join-Path $runDirectory 'winderust.exe'
$settingsPath = Join-Path $runDirectory 'settings.toml'
$targetExePath = Join-Path $env:SystemRoot 'System32\PING.EXE'
$activeWinderust = $null
$activeTarget = $null
$benchmarkHostProcess = $null
$originalBenchmarkHostPriority = $null

if (-not ('WinderustArchitectureBaseline.NativeWindow' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

namespace WinderustArchitectureBaseline {
    public static class NativeWindow {
        private delegate bool EnumWindowsProc(IntPtr window, IntPtr parameter);

        [DllImport("user32.dll")]
        private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr parameter);

        [DllImport("user32.dll")]
        private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

        [DllImport("user32.dll")]
        private static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);

        public static int PostClose(uint targetProcessId) {
            const uint WM_CLOSE = 0x0010;
            int posted = 0;
            EnumWindows(delegate(IntPtr window, IntPtr parameter) {
                uint processId;
                GetWindowThreadProcessId(window, out processId);
                if (processId == targetProcessId && PostMessage(window, WM_CLOSE, IntPtr.Zero, IntPtr.Zero)) {
                    posted++;
                }
                return true;
            }, IntPtr.Zero);
            return posted;
        }
    }
}
'@
}

function Get-Percentile {
    param([double[]]$Values, [double]$Percentile)
    if ($Values.Count -eq 0) {
        return $null
    }
    $sorted = @($Values | Sort-Object)
    $index = [Math]::Max(0, [Math]::Ceiling(($Percentile / 100.0) * $sorted.Count) - 1)
    [Math]::Round([double]$sorted[$index], 4)
}

function New-Summary {
    param([double[]]$Values)
    if ($Values.Count -eq 0) {
        return $null
    }
    [pscustomobject]@{
        minimum = [Math]::Round(($Values | Measure-Object -Minimum).Minimum, 4)
        median = Get-Percentile -Values $Values -Percentile 50
        p95 = Get-Percentile -Values $Values -Percentile 95
        maximum = [Math]::Round(($Values | Measure-Object -Maximum).Maximum, 4)
        mean = [Math]::Round(($Values | Measure-Object -Average).Average, 4)
    }
}

function ConvertTo-TomlString {
    param([string]$Value)
    $Value.Replace('\', '\\').Replace('"', '\"')
}

function Write-IsolatedSettings {
    param([bool]$EnableProcessPriority)

    $processPriority = ''
    if ($EnableProcessPriority) {
        $escapedTargetPath = ConvertTo-TomlString -Value $targetExePath
        $processPriority = @"

[process_priority]
enabled = true
foreground_detection_enabled = false
foreground_priority = "default"
visible_window_detection_enabled = true
visible_window_priority = "default"
background_priority = "default"
preserve_foreground_priority = true
preserve_visible_window_priority = true
preserve_background_priority = true
exclusions = [{ enabled = true, executable_path = "$escapedTargetPath", process_foreground_priority = "below_normal", process_background_priority = "below_normal" }]
"@
    }

    $settings = @"
[general]
enabled = true
startup_with_windows = false
start_minimized = false
hide_to_tray = false
check_for_updates = false
check_interval_ms = 1000

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
$processPriority
"@
    [IO.File]::WriteAllText($settingsPath, $settings, [Text.UTF8Encoding]::new($false))
}

function Start-IsolatedWinderust {
    param([string]$DiagnosticsPath)

    $previousValue = [Environment]::GetEnvironmentVariable(
        $diagnosticsEnvironmentVariable,
        [EnvironmentVariableTarget]::Process
    )
    try {
        [Environment]::SetEnvironmentVariable(
            $diagnosticsEnvironmentVariable,
            $DiagnosticsPath,
            [EnvironmentVariableTarget]::Process
        )
        $process = Start-Process -FilePath $isolatedExePath -PassThru -WindowStyle Hidden
    } finally {
        [Environment]::SetEnvironmentVariable(
            $diagnosticsEnvironmentVariable,
            $previousValue,
            [EnvironmentVariableTarget]::Process
        )
    }

    Start-Sleep -Seconds 3
    $process.Refresh()
    if ($process.HasExited) {
        throw 'The isolated Winderust process exited during startup.'
    }
    $process
}

function Get-IsolatedProcesses {
    $processName = [IO.Path]::GetFileNameWithoutExtension($isolatedExePath)
    @(
        Get-Process -Name $processName -ErrorAction SilentlyContinue |
            Where-Object {
                try {
                    $_.Path -and [IO.Path]::GetFullPath($_.Path) -ieq $isolatedExePath
                } catch {
                    $false
                }
            }
    )
}

function Measure-ProcessFootprint {
    $samples = @()
    $tracked = @(Get-IsolatedProcesses)
    if ($tracked.Count -lt 2) {
        throw "Expected the Winderust main process and crash helper, found $($tracked.Count)."
    }

    $previousCpuMilliseconds = ($tracked | ForEach-Object {
        $_.Refresh()
        $_.TotalProcessorTime.TotalMilliseconds
    } | Measure-Object -Sum).Sum
    $previousTimestamp = [Diagnostics.Stopwatch]::GetTimestamp()
    $sampleCount = [Math]::Ceiling(($MeasurementSeconds * 1000) / $SampleIntervalMilliseconds)

    for ($sample = 0; $sample -lt $sampleCount; $sample++) {
        Start-Sleep -Milliseconds $SampleIntervalMilliseconds
        $tracked = @(Get-IsolatedProcesses)
        if ($tracked.Count -lt 2) {
            throw 'A Winderust baseline process exited during measurement.'
        }
        foreach ($process in $tracked) {
            $process.Refresh()
        }

        $timestamp = [Diagnostics.Stopwatch]::GetTimestamp()
        $elapsedMilliseconds = (($timestamp - $previousTimestamp) * 1000.0) / [Diagnostics.Stopwatch]::Frequency
        $cpuMilliseconds = ($tracked | ForEach-Object {
            $_.TotalProcessorTime.TotalMilliseconds
        } | Measure-Object -Sum).Sum
        $cpuPercent = (($cpuMilliseconds - $previousCpuMilliseconds) / $elapsedMilliseconds) * (100.0 / $logicalProcessors)
        $samples += [pscustomobject]@{
            elapsed_ms = [Math]::Round((($sample + 1) * $SampleIntervalMilliseconds), 0)
            cpu_percent_total_capacity = [Math]::Round([Math]::Max(0.0, $cpuPercent), 4)
            working_set_mb = [Math]::Round((($tracked | Measure-Object WorkingSet64 -Sum).Sum / 1MB), 4)
            private_memory_mb = [Math]::Round((($tracked | Measure-Object PrivateMemorySize64 -Sum).Sum / 1MB), 4)
            thread_count = ($tracked | ForEach-Object { $_.Threads.Count } | Measure-Object -Sum).Sum
            handle_count = ($tracked | Measure-Object HandleCount -Sum).Sum
            process_count = $tracked.Count
        }
        $previousCpuMilliseconds = $cpuMilliseconds
        $previousTimestamp = $timestamp
    }

    [pscustomobject]@{
        sample_count = $samples.Count
        interval_ms = $SampleIntervalMilliseconds
        cpu_percent_total_capacity = New-Summary -Values @($samples.cpu_percent_total_capacity)
        working_set_mb = New-Summary -Values @($samples.working_set_mb)
        private_memory_mb = New-Summary -Values @($samples.private_memory_mb)
        thread_count = New-Summary -Values @($samples.thread_count)
        handle_count = New-Summary -Values @($samples.handle_count)
        samples = $samples
    }
}

function Start-TargetAndWaitForPriority {
    $target = Start-Process -FilePath $targetExePath -ArgumentList @('-t', '127.0.0.1') -PassThru -WindowStyle Hidden
    try {
        $normalization = [Diagnostics.Stopwatch]::StartNew()
        do {
            $target.PriorityClass = [Diagnostics.ProcessPriorityClass]::Normal
            $target.Refresh()
            if ($target.PriorityClass -eq [Diagnostics.ProcessPriorityClass]::Normal) {
                break
            }
            Start-Sleep -Milliseconds 10
        } while ($normalization.ElapsedMilliseconds -lt 500)
        if ($target.PriorityClass -ne [Diagnostics.ProcessPriorityClass]::Normal) {
            throw 'Could not normalize the architecture-baseline target to Normal priority.'
        }
    } catch {
        Stop-OwnedProcess -Process $target
        throw
    }
    $stopwatch = [Diagnostics.Stopwatch]::StartNew()
    while ($stopwatch.ElapsedMilliseconds -lt $ActionTimeoutMilliseconds) {
        Start-Sleep -Milliseconds 25
        $target.Refresh()
        if ($target.HasExited) {
            throw 'The architecture-baseline target exited before Winderust acted on it.'
        }
        if ($target.PriorityClass -eq [Diagnostics.ProcessPriorityClass]::BelowNormal) {
            return [pscustomobject]@{
                process = $target
                latency_ms = [double]$stopwatch.Elapsed.TotalMilliseconds
            }
        }
    }

    if (-not $target.HasExited) {
        $target.Kill()
        $target.WaitForExit()
    }
    $target.Dispose()
    throw "Winderust did not apply Process Priority within $ActionTimeoutMilliseconds ms."
}

function Stop-OwnedProcess {
    param([Diagnostics.Process]$Process)
    if ($null -eq $Process) {
        return
    }
    try {
        $Process.Refresh()
        if (-not $Process.HasExited) {
            $Process.Kill()
            $Process.WaitForExit()
        }
    } finally {
        $Process.Dispose()
    }
}

function Remove-RunDirectory {
    if (-not (Test-Path -LiteralPath $runDirectory -PathType Container)) {
        return
    }
    $resolvedRunDirectory = (Resolve-Path -LiteralPath $runDirectory).Path
    $temporaryRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
    if (-not $resolvedRunDirectory.StartsWith(
        "$temporaryRoot\winderust-architecture-baseline-",
        [StringComparison]::OrdinalIgnoreCase
    )) {
        throw "Refusing unexpected baseline cleanup target: $resolvedRunDirectory"
    }
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        try {
            Remove-Item -LiteralPath $resolvedRunDirectory -Recurse -Force -ErrorAction Stop
            return
        } catch {
            if ($attempt -eq 39) {
                throw
            }
            Start-Sleep -Milliseconds 250
        }
    }
}

function Stop-IsolatedWinderustCleanly {
    param([Diagnostics.Process]$Process)
    $Process.Refresh()
    if ($Process.HasExited) {
        throw 'Winderust exited before the clean-shutdown measurement.'
    }
    $posted = [WinderustArchitectureBaseline.NativeWindow]::PostClose([uint32]$Process.Id)
    if ($posted -eq 0) {
        [void]$Process.CloseMainWindow()
    }
    if (-not $Process.WaitForExit(15000)) {
        throw 'Winderust did not complete a clean shutdown within 15 seconds.'
    }
    $Process.Dispose()
}

function Read-Diagnostics {
    param([string]$Path)
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        if (Test-Path -LiteralPath $Path) {
            return Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
        }
        Start-Sleep -Milliseconds 250
    }
    throw "Winderust did not write architecture diagnostics to $Path."
}

function Run-IdleCase {
    $diagnosticsPath = Join-Path $runDirectory 'ui-idle-diagnostics.json'
    Write-IsolatedSettings -EnableProcessPriority $false
    $script:activeWinderust = Start-IsolatedWinderust -DiagnosticsPath $diagnosticsPath
    try {
        $footprint = Measure-ProcessFootprint
        Stop-IsolatedWinderustCleanly -Process $script:activeWinderust
        $script:activeWinderust = $null
        [pscustomobject]@{
            name = 'ui_idle'
            model = 'Release build with all automatic features disabled.'
            footprint = $footprint
            diagnostics = Read-Diagnostics -Path $diagnosticsPath
        }
    } finally {
        if ($null -ne $script:activeWinderust) {
            Stop-OwnedProcess -Process $script:activeWinderust
            $script:activeWinderust = $null
        }
    }
}

function Run-ProcessPriorityCase {
    $diagnosticsPath = Join-Path $runDirectory 'process-priority-diagnostics.json'
    Write-IsolatedSettings -EnableProcessPriority $true
    $script:activeWinderust = Start-IsolatedWinderust -DiagnosticsPath $diagnosticsPath
    $latencies = @()
    try {
        for ($trial = 0; $trial -lt $ActionTrials; $trial++) {
            $result = Start-TargetAndWaitForPriority
            $latencies += $result.latency_ms
            Stop-OwnedProcess -Process $result.process
            Start-Sleep -Milliseconds 1000
        }

        $footprint = Measure-ProcessFootprint
        $retainedTarget = Start-TargetAndWaitForPriority
        $script:activeTarget = $retainedTarget.process
        Stop-IsolatedWinderustCleanly -Process $script:activeWinderust
        $script:activeWinderust = $null

        $restored = $false
        for ($attempt = 0; $attempt -lt 40; $attempt++) {
            Start-Sleep -Milliseconds 100
            $script:activeTarget.Refresh()
            if ($script:activeTarget.PriorityClass -eq [Diagnostics.ProcessPriorityClass]::Normal) {
                $restored = $true
                break
            }
        }

        [pscustomobject]@{
            name = 'process_priority_reconciliation'
            model = 'Release build with Process Priority and visible-window observation enabled, global values set to Default, and one exact-path Below Normal rule.'
            footprint = $footprint
            process_appearance_to_action_latency_ms = [pscustomobject]@{
                trials = $latencies.Count
                median = Get-Percentile -Values $latencies -Percentile 50
                p95 = Get-Percentile -Values $latencies -Percentile 95
                minimum = [Math]::Round(($latencies | Measure-Object -Minimum).Minimum, 4)
                maximum = [Math]::Round(($latencies | Measure-Object -Maximum).Maximum, 4)
                values = @($latencies | ForEach-Object { [Math]::Round($_, 4) })
            }
            clean_release_restored_normal_priority = $restored
            diagnostics = Read-Diagnostics -Path $diagnosticsPath
        }
    } catch {
        $failure = $_
        if ($null -ne $script:activeWinderust) {
            try {
                Stop-IsolatedWinderustCleanly -Process $script:activeWinderust
                $script:activeWinderust = $null
            } catch {
                # The original action failure remains authoritative; force cleanup runs below.
            }
        }
        if (Test-Path -LiteralPath $diagnosticsPath) {
            $diagnostics = Read-Diagnostics -Path $diagnosticsPath
            $details = $diagnostics | ConvertTo-Json -Depth 5 -Compress
            throw "$($failure.Exception.Message) Diagnostics: $details"
        }
        throw $failure
    } finally {
        if ($null -ne $script:activeWinderust) {
            Stop-OwnedProcess -Process $script:activeWinderust
            $script:activeWinderust = $null
        }
        if ($null -ne $script:activeTarget) {
            Stop-OwnedProcess -Process $script:activeTarget
            $script:activeTarget = $null
        }
    }
}

if (-not $SkipBuild) {
    & (Join-Path $PSScriptRoot 'build_release.ps1') `
        -TargetDir $resolvedTargetDir `
        -Features architecture-diagnostics
    if ($LASTEXITCODE -ne 0) {
        throw "Architecture diagnostics release build failed with exit code $LASTEXITCODE."
    }
}
if (-not (Test-Path -LiteralPath $sourceExePath -PathType Leaf)) {
    throw "Architecture diagnostics release executable not found at $sourceExePath."
}
if (@(Get-Process ping -ErrorAction SilentlyContinue).Count -gt 0) {
    throw 'Close existing ping.exe processes before running the architecture baseline.'
}

[IO.Directory]::CreateDirectory($runDirectory) | Out-Null
Copy-Item -LiteralPath $sourceExePath -Destination $isolatedExePath
$benchmarkHostProcess = [Diagnostics.Process]::GetCurrentProcess()
$benchmarkHostProcess.Refresh()
$originalBenchmarkHostPriority = $benchmarkHostProcess.PriorityClass
$benchmarkHostProcess.PriorityClass = [Diagnostics.ProcessPriorityClass]::Normal

try {
    $idleCase = Run-IdleCase
    $processPriorityCase = Run-ProcessPriorityCase
    $expectedAppliedChanges = $ActionTrials + 1
    $processPriorityDiagnostics = $processPriorityCase.diagnostics.process_priority
    $processPriorityFailures = $processPriorityDiagnostics.process_exit_failures `
        + $processPriorityDiagnostics.access_denied_failures `
        + $processPriorityDiagnostics.other_failures
    $cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
    $operatingSystem = Get-CimInstance Win32_OperatingSystem
    $report = [pscustomobject]@{
        schema_version = 1
        purpose = 'Phase 0 behavior-preserving architecture performance baseline.'
        recorded_at_utc = [DateTime]::UtcNow.ToString('o')
        winderust_version = $idleCase.diagnostics.winderust_version
        build = [pscustomobject]@{
            profile = 'release'
            cargo_feature = 'architecture-diagnostics'
            executable_sha256 = (Get-FileHash -LiteralPath $sourceExePath -Algorithm SHA256).Hash.ToLowerInvariant()
        }
        host = [pscustomobject]@{
            cpu_name = $cpu.Name.Trim()
            logical_processors = $logicalProcessors
            memory_gb = [Math]::Round($operatingSystem.TotalVisibleMemorySize / 1MB, 2)
            os_caption = $operatingSystem.Caption
            os_version = $operatingSystem.Version
        }
        methodology = [pscustomobject]@{
            measurement_seconds_per_case = $MeasurementSeconds
            sample_interval_ms = $SampleIntervalMilliseconds
            cpu_definition = 'Sum of Winderust main/helper TotalProcessorTime deltas divided by wall time and logical processor count.'
            memory_definition = 'Sum across the isolated Winderust main process and crash helper.'
            action_latency_definition = 'Wall time from normalizing a new owned hidden System32 PING.EXE process to Normal until its PriorityClass is observed as BelowNormal; the runner refuses pre-existing ping.exe processes.'
            clean_release_definition = 'The retained target returns to its original Normal priority after a graceful Winderust shutdown.'
        }
        cases = @($idleCase, $processPriorityCase)
        validation = [pscustomobject]@{
            passed = $idleCase.diagnostics.worker.reconciliation_passes -eq 0 `
                -and $processPriorityCase.diagnostics.worker.reconciliation_passes -gt 0 `
                -and $processPriorityCase.diagnostics.inventory.process_snapshot_scans -gt 0 `
                -and $processPriorityCase.diagnostics.inventory.visible_window_scans -gt 0 `
                -and $processPriorityCase.process_appearance_to_action_latency_ms.trials -eq $ActionTrials `
                -and $processPriorityCase.clean_release_restored_normal_priority `
                -and $processPriorityDiagnostics.applied_changes -eq $expectedAppliedChanges `
                -and $processPriorityFailures -eq 0
            idle_worker_dormant = $idleCase.diagnostics.worker.reconciliation_passes -eq 0
            active_worker_observed = $processPriorityCase.diagnostics.worker.reconciliation_passes -gt 0
            process_scans_observed = $processPriorityCase.diagnostics.inventory.process_snapshot_scans -gt 0
            visible_window_scans_observed = $processPriorityCase.diagnostics.inventory.visible_window_scans -gt 0
            all_action_trials_observed = $processPriorityCase.process_appearance_to_action_latency_ms.trials -eq $ActionTrials
            clean_release_restored = $processPriorityCase.clean_release_restored_normal_priority
            expected_process_priority_changes = $expectedAppliedChanges
            observed_process_priority_changes = $processPriorityDiagnostics.applied_changes
            process_priority_failures = $processPriorityFailures
        }
    }

    if ([string]::IsNullOrWhiteSpace($OutputPath)) {
        $date = Get-Date -Format 'yyyyMMdd'
        $OutputPath = Join-Path $repositoryRoot "benchmark\results\architecture-baseline-$date.json"
    } elseif (-not [IO.Path]::IsPathRooted($OutputPath)) {
        $OutputPath = Join-Path $repositoryRoot $OutputPath
    }
    [IO.Directory]::CreateDirectory((Split-Path $OutputPath -Parent)) | Out-Null
    $json = $report | ConvertTo-Json -Depth 12
    [IO.File]::WriteAllText($OutputPath, $json, [Text.UTF8Encoding]::new($false))
    $json

    if (-not $report.validation.passed) {
        throw "Architecture baseline validation failed. See $OutputPath."
    }
} finally {
    try {
        if ($null -ne $activeWinderust) {
            Stop-OwnedProcess -Process $activeWinderust
        }
        if ($null -ne $activeTarget) {
            Stop-OwnedProcess -Process $activeTarget
        }
        Remove-RunDirectory
    } finally {
        if ($null -ne $benchmarkHostProcess) {
            try {
                $benchmarkHostProcess.Refresh()
                $benchmarkHostProcess.PriorityClass = $originalBenchmarkHostPriority
            } finally {
                $benchmarkHostProcess.Dispose()
            }
        }
    }
}
