param(
    [switch]$SettingsOnly,
    [switch]$EnableDynamicResourceZones,
    [switch]$EnableCpuPressureRestraint,
    [int]$Passes = 4,
    [int]$Rounds = 5,
    [int]$Iterations = 1000000,
    [int]$WorkerSeconds = 180,
    [int]$CooldownSeconds = 30,
    [ValidateRange(5, 600)]
    [int]$WarmupSeconds = 100,
    [ValidateSet('CpuLoop', 'IoLoop', 'MessageLoop')]
    [string]$ForegroundScenario = 'CpuLoop',
    [string]$WinderustExePath = '.\target\release\winderust.exe',
    [switch]$DisableBackgroundProcessorLimit,
    [ValidateRange(1, 100)]
    [int]$ProcessRestraintThresholdPercent = 10,
    [ValidateRange(1, 32)]
    [int]$MaximumRestrainedApps = 4,
    [ValidateRange(1, 100)]
    [int]$ForegroundOrSystemCpuThresholdPercent = 75,
    [ValidateRange(0, 100)]
    [int]$BackgroundPressureAcBoostPolicy = 80,
    [ValidateSet('disabled', 'enabled', 'aggressive', 'efficient_enabled', 'efficient_aggressive', 'aggressive_at_guaranteed', 'efficient_aggressive_at_guaranteed')]
    [string]$BackgroundPressureAcBoostMode = 'efficient_aggressive',
    [ValidateRange(-100, 100)]
    [double]$MinimumPowerSavingPercent = -2.0,
    [switch]$SkipPower,
    [string]$OutputPath = ''
)

$ErrorActionPreference = 'Stop'
if ($Passes -lt 4 -or ($Passes % 2) -ne 0) {
    throw 'Passes must be an even number of at least 4 so Stock-first and Adaptive-first orders are balanced.'
}
if ($WorkerSeconds -le ($WarmupSeconds + 30)) {
    throw 'WorkerSeconds must exceed WarmupSeconds by more than 30 seconds so workers survive measurement.'
}
$repoRoot = Split-Path $PSScriptRoot -Parent
$scratchRoot = [IO.Path]::GetFullPath((Join-Path $repoRoot 'target\adaptive-runtime-benchmark'))
$configDir = Join-Path $scratchRoot ([guid]::NewGuid().ToString())
$exePath = Join-Path $configDir 'winderust.exe'
$runtime = $null
$exportProcess = $null
$originalGuid = $null
$originalTemp = $env:TEMP
$originalTmp = $env:TMP
$benchmarkHostPath = [Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
$explicitOverrides = [ordered]@{}
$overrideParameters = [ordered]@{
    EnableDynamicResourceZones = 'dynamic_resource_zones_enabled'
    EnableCpuPressureRestraint = 'cpu_pressure_restraint_enabled'
    DisableBackgroundProcessorLimit = 'limit_background_processors_enabled'
    ProcessRestraintThresholdPercent = 'background_app_cpu_threshold_percent'
    MaximumRestrainedApps = 'maximum_restrained_apps'
    ForegroundOrSystemCpuThresholdPercent = 'foreground_or_system_cpu_threshold_percent'
    BackgroundPressureAcBoostPolicy = 'background_pressure_ac_boost_policy'
    BackgroundPressureAcBoostMode = 'background_pressure_ac_boost_mode'
}
foreach ($name in $overrideParameters.Keys) {
    if ($PSBoundParameters.ContainsKey($name)) {
        $value = $PSBoundParameters[$name]
        if ($value -is [System.Management.Automation.SwitchParameter]) { $value = $value.IsPresent }
        if ($name -eq 'DisableBackgroundProcessorLimit') { $value = !$value }
        $explicitOverrides[$overrideParameters[$name]] = $value
    }
}
$request = [ordered]@{ benchmark_host_path = $benchmarkHostPath }
foreach ($key in $explicitOverrides.Keys) { $request[$key] = $explicitOverrides[$key] }
$requestJson = $request | ConvertTo-Json

function Assert-IsolatedProcessesExited {
    try {
        $processes = @(Get-Process -Name ([IO.Path]::GetFileNameWithoutExtension($exePath)) -ErrorAction Stop)
    } catch {
        if ($_.FullyQualifiedErrorId -eq 'NoProcessFoundForGivenName,Microsoft.PowerShell.Commands.GetProcessCommand') { return }
        throw
    }
    try {
        foreach ($process in $processes) {
            try {
                # Retain this process identity while reading its path and checking exit.
                $null = $process.Handle
                if ($process.HasExited) { continue }
                $path = $process.Path
                $started = $process.StartTime.ToUniversalTime().ToString('o')
                if ($process.HasExited) { continue }
                if ([string]::IsNullOrWhiteSpace($path)) { throw 'The executable path is unavailable.' }
                $pathMatches = [IO.Path]::GetFullPath($path).Equals($exePath, [StringComparison]::OrdinalIgnoreCase)
            } catch {
                throw "Cannot verify whether process $($process.Id) is an isolated recovery process: $($_.Exception.Message)"
            }
            if ($pathMatches) {
                throw "Isolated runtime or recovery process $($process.Id), started $started, remains alive."
            }
        }
    } finally {
        foreach ($process in $processes) { $process.Dispose() }
    }
}

function Remove-IsolatedConfig {
    if (-not [IO.Directory]::Exists($configDir)) { return }
    foreach ($process in @($exportProcess, $script:runtime.process)) {
        if ($null -ne $process -and !$process.HasExited) {
            throw "Benchmark process is still running; recovery files remain at $configDir."
        }
    }
    $target = [IO.Path]::GetFullPath($configDir)
    $expectedRoot = $scratchRoot.TrimEnd('\') + '\'
    if (!$target.StartsWith($expectedRoot, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Refusing to remove a benchmark directory outside its scratch directory.'
    }
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        try {
            Remove-Item -LiteralPath $target -Recurse -Force -ErrorAction Stop
            return
        } catch {
            if ($attempt -eq 39) { throw }
            Start-Sleep -Milliseconds 250
        }
    }
}

if (!(Test-Path -LiteralPath $WinderustExePath -PathType Leaf)) {
    throw 'Build the benchmark binary first: cargo build --locked --release --features runtime-benchmark; then pass -WinderustExePath if needed.'
}
try {
    [IO.Directory]::CreateDirectory($configDir) | Out-Null
    Copy-Item -LiteralPath (Resolve-Path -LiteralPath $WinderustExePath).Path -Destination $exePath
    $binarySha256 = (Get-FileHash -LiteralPath $exePath -Algorithm SHA256).Hash
    [IO.File]::WriteAllText((Join-Path $configDir 'request.json'), $requestJson, [Text.UTF8Encoding]::new($false))
    $exportProcess = Start-Process -FilePath $exePath -ArgumentList @('--runtime-benchmark-settings', ('"{0}"' -f $configDir), 'Balanced') -WindowStyle Hidden -PassThru
    if (!$exportProcess.WaitForExit(30000)) {
        throw 'Settings export timed out. Build with --features runtime-benchmark and close the unexpected Winderust instance before retrying.'
    }
    $finishedPath = Join-Path $configDir 'finished.txt'
    if (!(Test-Path -LiteralPath $finishedPath)) {
        throw 'The binary did not export benchmark settings. Rebuild with --features runtime-benchmark.'
    }
    $finished = [IO.File]::ReadAllText($finishedPath).Trim()
    if ($finished -ne 'ok') { throw "Settings export failed: $finished" }
    $settingsToml = [IO.File]::ReadAllText((Join-Path $configDir 'settings.toml'))
    $settingsJson = [IO.File]::ReadAllText((Join-Path $configDir 'settings.json'))
    $effectiveSettings = $settingsJson | ConvertFrom-Json
    if ($SettingsOnly) {
        $settingsToml
        return
    }
} catch {
    Remove-IsolatedConfig
    throw
} finally {
    if ($SettingsOnly) { Remove-IsolatedConfig }
}

$failures = New-Object 'System.Collections.Generic.List[string]'
try {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    try {
        $principal = New-Object Security.Principal.WindowsPrincipal $identity
        if (!$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
            throw 'Run the runtime benchmark from an elevated PowerShell host. SettingsOnly does not require elevation.'
        }
    } finally {
        $identity.Dispose()
    }
    $env:TEMP = $configDir
    $env:TMP = $configDir
    $benchmarkScript = Join-Path $PSScriptRoot 'adaptive_engine_process_benchmark.ps1'
    $env:WINDERUST_BENCHMARK_IMPORT_ONLY = '1'
    try {
        . $benchmarkScript `
            -Passes $Passes `
            -Rounds $Rounds `
            -Iterations $Iterations `
            -WorkerSeconds $WorkerSeconds `
            -BackgroundWorkers ([Math]::Min([Math]::Ceiling([Environment]::ProcessorCount * 0.9), 24)) `
            -ForegroundScenario $ForegroundScenario `
            -WinderustExePath $WinderustExePath `
            -SkipPower:$SkipPower
    } finally {
        Remove-Item Env:WINDERUST_BENCHMARK_IMPORT_ONLY -ErrorAction SilentlyContinue
    }

    $balancedGuid = '381b4222-f694-41f0-9685-ff5bb260df2e'
    if (!('AdaptiveRuntimeCpuSets' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class AdaptiveRuntimeCpuSets {
    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetProcessDefaultCpuSets(IntPtr process, [Out] uint[] ids, uint count, out uint required);
    public static uint[] Read(IntPtr process) {
        uint count;
        if (!GetProcessDefaultCpuSets(process, null, 0, out count)) {
            int error = Marshal.GetLastWin32Error();
            if (error != 122) throw new Win32Exception(error);
        }
        if (count == 0) return new uint[0];
        uint[] ids = new uint[count];
        if (!GetProcessDefaultCpuSets(process, ids, count, out count)) {
            int error = Marshal.GetLastWin32Error();
            throw new Win32Exception(error);
        }
        if (count > ids.Length) throw new InvalidOperationException("CPU Set count exceeds the query buffer.");
        Array.Resize(ref ids, (int)count);
        return ids;
    }
}
'@
    }
    if (@([AdaptiveRuntimeCpuSets]::Read([Diagnostics.Process]::GetCurrentProcess().Handle)).Count) {
        throw 'The benchmark controller already has CPU Sets; use an unrestricted PowerShell host.'
    }
    $script:originalGetWorkerPriorities = (Get-Item Function:\Get-WorkerPriorities).ScriptBlock
    $script:workerCpuSetSamples = @{}
    function Get-WorkerPriorities {
        param([object[]]$Processes)
        # Sample the exact worker handles while Run-Case still owns the live processes.
        foreach ($process in $Processes) {
            try {
                $ids = @([AdaptiveRuntimeCpuSets]::Read($process.Handle))
                $observation = [pscustomobject]@{
                    pid = $process.Id
                    start_time_utc = $process.StartTime.ToUniversalTime().ToString('o')
                    cpu_set_ids = $ids
                    error = $null
                }
            } catch {
                $observation = [pscustomobject]@{ pid = $process.Id; start_time_utc = $null; cpu_set_ids = @(); error = $_.Exception.Message }
            }
            $key = '{0}:{1}:{2}:{3}' -f $observation.pid, $observation.start_time_utc, ($observation.cpu_set_ids -join ','), $observation.error
            $script:workerCpuSetSamples[$key] = $observation
        }
        & $script:originalGetWorkerPriorities -Processes $Processes
    }

    function Select-PowerScheme {
        param([string]$Guid)
        powercfg /setactive $Guid | Out-Null
        if ($LASTEXITCODE -ne 0 -or (Get-ActiveSchemeGuid) -ne $Guid) {
            throw "Failed to select power scheme $Guid."
        }
    }

    function Start-AdaptiveRuntime {
        Select-PowerScheme $balancedGuid
        $directory = [IO.Directory]::CreateDirectory((Join-Path $configDir ([guid]::NewGuid().ToString()))).FullName
        [IO.File]::WriteAllText((Join-Path $directory 'request.json'), $requestJson, [Text.UTF8Encoding]::new($false))
        $process = Start-Process -FilePath $exePath -ArgumentList @('--runtime-benchmark', ('"{0}"' -f $directory), 'Balanced') -WindowStyle Hidden -PassThru
        $script:runtime = [pscustomobject]@{ process = $process; directory = $directory; shutdown_attempted = $false }
        $deadline = [DateTime]::UtcNow.AddSeconds(30)
        while (!(Test-Path -LiteralPath (Join-Path $directory 'ready.txt'))) {
            $process.Refresh()
            if ($process.HasExited) {
                $failure = Get-Content -LiteralPath (Join-Path $directory 'finished.txt') -ErrorAction SilentlyContinue
                throw "Adaptive Engine runtime failed: $failure"
            }
            if ([DateTime]::UtcNow -gt $deadline) {
                throw 'Adaptive Engine startup timed out. Build with --features runtime-benchmark and close other Winderust instances.'
            }
            Start-Sleep -Milliseconds 100
        }
        if ([IO.File]::ReadAllText((Join-Path $directory 'ready.txt')).Trim() -ne $balancedGuid) {
            throw 'The runtime captured an unexpected baseline power scheme.'
        }
        if ([IO.File]::ReadAllText((Join-Path $directory 'settings.json')) -ne $settingsJson) {
            throw 'The production runtime settings differ from the exported Balanced settings.'
        }
        if ($effectiveSettings.adaptive_engine.processor_power_policy_enabled) {
            while ((Get-ActiveSchemeGuid) -eq $balancedGuid) {
                if ($process.HasExited -or [DateTime]::UtcNow -gt $deadline) {
                    throw 'Adaptive Engine did not activate its managed processor power policy.'
                }
                Start-Sleep -Milliseconds 100
            }
        }
        return $script:runtime
    }

    function Stop-AdaptiveRuntime {
        param($Runtime)
        if ($null -eq $Runtime) {
            return
        }
        $Runtime.shutdown_attempted = $true
        $process = $Runtime.process
        [IO.File]::WriteAllText((Join-Path $Runtime.directory 'stop'), '')
        $deadline = [DateTime]::UtcNow.AddSeconds(60)
        while (!$process.HasExited -and [DateTime]::UtcNow -lt $deadline) {
            Start-Sleep -Milliseconds 100
            $process.Refresh()
        }
        if (!$process.HasExited) {
            throw "Runtime shutdown timed out; it remains alive to finish recovery. Files: $($Runtime.directory)"
        }
        $finished = [IO.File]::ReadAllText((Join-Path $Runtime.directory 'finished.txt')).Trim()
        if ($finished -ne 'ok') { throw "Runtime shutdown or restoration failed: $finished" }
        if ((Get-ActiveSchemeGuid) -ne $balancedGuid) { throw 'The runtime did not restore Windows Balanced.' }
    }

    function Read-ActiveProcessorPolicy {
        $scheme = Get-ActiveSchemeGuid
        $values = [ordered]@{}
        foreach ($setting in $processorPolicySettings) {
            $index = Get-PowerSettingIndexes -SchemeGuid $scheme -SettingGuid $setting.Guid
            $values[$setting.Name] = [pscustomobject]@{ ac = $index.ac; dc = $index.dc }
        }
        [pscustomobject]@{ scheme = $scheme; values = [pscustomobject]$values }
    }

    function Assert-ValidCase {
        param([pscustomobject]$Result, [string]$Name)
        if ($Result.workers_alive_after_measurement -ne $workerCount) {
            throw ('Invalid {0} benchmark: only {1} of {2} workers survived measurement.' -f $Name, $Result.workers_alive_after_measurement, $workerCount)
        }
        if ($Result.foreground_priority_after_measurement -ne 'Normal') {
            throw ('Invalid {0} benchmark: foreground host priority changed to {1}.' -f $Name, $Result.foreground_priority_after_measurement)
        }
        $Result | Add-Member -NotePropertyName observed_worker_cpu_sets -NotePropertyValue @($script:workerCpuSetSamples.Values)
        if (@($Result.observed_worker_cpu_sets | Where-Object { $null -ne $_.error }).Count) {
            throw "Invalid $Name benchmark: a native worker CPU Set query failed."
        }
    }

    function Run-StockCase {
        Select-PowerScheme $balancedGuid
        $script:workerCpuSetSamples = @{}
        $result = Run-Case `
            -Name 'stock_balanced' `
            -Model 'Windows Balanced with no Winderust runtime.' `
            -ForegroundPriority 'Normal' `
            -Priorities (New-Priorities -DefaultPriority 'Normal' -RestrainedCount 0 -RestrainedPriority 'Normal') `
            -AffinitySelectedCount 0 `
            -AffinityMask 0 `
            -AssistControls (New-AssistControls) `
            -WarmupSeconds $WarmupSeconds
        Assert-ValidCase -Result $result -Name 'Stock'
        if (@($result.observed_worker_cpu_sets | Where-Object { $_.cpu_set_ids.Count -gt 0 }).Count) {
            throw 'Invalid Stock benchmark: generated workers already have CPU Sets.'
        }
        return $result
    }

    function Run-AdaptiveCase {
        try {
            $script:runtime = Start-AdaptiveRuntime
            $script:workerCpuSetSamples = @{}
            $result = Run-Case `
                -Name 'adaptive_runtime' `
                -Model 'Real Winderust Adaptive Engine runtime on a managed plan cloned from Windows Balanced.' `
                -ForegroundPriority 'Normal' `
                -Priorities (New-Priorities -DefaultPriority 'Normal' -RestrainedCount 0 -RestrainedPriority 'Normal') `
                -AffinitySelectedCount 0 `
                -AffinityMask 0 `
                -AssistControls (New-AssistControls) `
                -WarmupSeconds $WarmupSeconds
            Assert-ValidCase -Result $result -Name 'Adaptive'
            $cpuSetsObserved = @($result.observed_worker_cpu_sets | Where-Object { $_.cpu_set_ids.Count -gt 0 }).Count -gt 0
            $priorityObserved = @($result.observed_worker_priorities | Where-Object { $_ -ne 'Normal' }).Count -gt 0
            $processControlObserved = $cpuSetsObserved -or $priorityObserved -or $result.worker_efficiency_coverage_percent -gt 0
            if (($effectiveSettings.adaptive_engine_process.limit_background_processors_enabled -or $effectiveSettings.adaptive_engine_process.dynamic_resource_zones_enabled) -and !$cpuSetsObserved) {
                throw 'Invalid runtime benchmark: no generated worker received native CPU Set allocation.'
            }
            if ($effectiveSettings.adaptive_engine_process.cpu_pressure_restraint_enabled -and !$processControlObserved) {
                throw 'Invalid runtime benchmark: no native worker process control was observed.'
            }
            $result | Add-Member -NotePropertyName runtime_ready_observed -NotePropertyValue $true
            $result | Add-Member -NotePropertyName runtime_control_observed -NotePropertyValue $processControlObserved
            $result | Add-Member -NotePropertyName runtime_cpu_sets_observed -NotePropertyValue $cpuSetsObserved
            $result | Add-Member -NotePropertyName adaptive_policy_after_load -NotePropertyValue (Read-ActiveProcessorPolicy)
            return $result
        } catch {
            if (!$failures.Contains($_.Exception.Message)) { $failures.Add($_.Exception.Message) }
            throw
        } finally {
            Stop-AdaptiveRuntime -Runtime $script:runtime
            $script:runtime = $null
        }
    }

    if (@(Get-Process winderust -ErrorAction SilentlyContinue).Count -gt 0) {
        throw 'Close Winderust before running the runtime Adaptive Engine benchmark.'
    }

    $originalGuid = Get-ActiveSchemeGuid
    Initialize-PowerCounter
    $runs = @()
    for ($pass = 1; $pass -le $Passes; $pass++) {
        $order = if (($pass % 2) -eq 1) { @('stock', 'adaptive') } else { @('adaptive', 'stock') }
        $results = @{}
        foreach ($name in $order) {
            if ($CooldownSeconds -gt 0) {
                Start-Sleep -Seconds $CooldownSeconds
            }
            $results[$name] = if ($name -eq 'stock') { Run-StockCase } else { Run-AdaptiveCase }
        }
        $runs += [pscustomobject]@{
            pass = $pass
            order = $order
            stock = $results.stock
            adaptive = $results.adaptive
            comparison_vs_stock = New-Comparison -Off $results.stock -Case $results.adaptive
        }
    }
} catch {
    if (!$failures.Contains($_.Exception.Message)) { $failures.Add($_.Exception.Message) }
} finally {
    $env:TEMP = $originalTemp
    $env:TMP = $originalTmp
    $hostExited = $true
    if ($null -ne $runtime) {
        if (!$runtime.shutdown_attempted) {
            try { Stop-AdaptiveRuntime -Runtime $runtime }
            catch { if (!$failures.Contains($_.Exception.Message)) { $failures.Add($_.Exception.Message) } }
        }
        try { $hostExited = $runtime.process.HasExited }
        catch {
            $hostExited = $false
            $failures.Add($_.Exception.Message)
        }
    }
    if ($hostExited -and $null -ne $originalGuid -and ($null -ne $runtime -or $failures.Count -gt 0)) {
        try { Assert-IsolatedProcessesExited }
        catch {
            $hostExited = $false
            $failures.Add($_.Exception.Message)
        }
    }
    if (!$hostExited) { $failures.Add('Runtime shutdown remains unconfirmed.') }
    if ($hostExited -and $null -ne $originalGuid) {
        try { Select-PowerScheme $originalGuid }
        catch { $failures.Add($_.Exception.Message) }
    }
    if ($failures.Count -eq 0 -and $hostExited) {
        try { Remove-IsolatedConfig }
        catch { $failures.Add($_.Exception.Message) }
    }
    if ($failures.Count -gt 0) {
        throw (($failures -join '; ') + "; Benchmark files retained at $configDir.")
    }
}

$comparisons = @($runs | ForEach-Object { $_.comparison_vs_stock })
$stockRows = @($runs | ForEach-Object { $_.stock })
$adaptiveRows = @($runs | ForEach-Object { $_.adaptive })
$repeatWins = @($comparisons | Where-Object {
    $_.median_improvement_percent_vs_off -ge 3.0 -and $_.p95_improvement_percent_vs_off -ge 3.0
}).Count
$medianImprovement = Get-AverageProperty -Items $comparisons -Name 'median_improvement_percent_vs_off'
$p95Improvement = Get-AverageProperty -Items $comparisons -Name 'p95_improvement_percent_vs_off'
$backgroundRetained = Get-AverageProperty -Items $comparisons -Name 'background_throughput_retained_percent_vs_off'
$powerSaving = Get-AverageProperty -Items $comparisons -Name 'package_power_saving_percent_vs_off'
$activationPasses = @($adaptiveRows | Where-Object { $_.runtime_ready_observed }).Count
$processControlPasses = @($adaptiveRows | Where-Object { $_.runtime_control_observed }).Count
$powerGatePassed = $SkipPower -or ($null -ne $powerSaving -and $powerSaving -ge $MinimumPowerSavingPercent)
$validationPassed = $activationPasses -eq $Passes `
    -and $medianImprovement -ge 3.0 `
    -and $p95Improvement -ge 3.0 `
    -and $backgroundRetained -ge 85.0 `
    -and $powerGatePassed

$report = [pscustomobject]@{
    note = 'Counterbalanced release-binary A/B. Stock is Windows Balanced; Adaptive uses the production runtime with the built-in Balanced preset and only the listed explicit setting overrides. Runtime readiness and native worker process control are reported separately.'
    cpu_name = Get-CpuName
    logical_processors = $logicalProcessors
    worker_count = $workerCount
    passes = $Passes
    rounds = $Rounds
    foreground_scenario = $ForegroundScenario
    foreground_iterations_per_round = $Iterations
    preset = 'Balanced'
    effective_settings = $effectiveSettings
    effective_settings_toml = $settingsToml
    binary_sha256 = $binarySha256
    explicit_setting_overrides = $explicitOverrides
    background_pressure_ac_boost_policy = $effectiveSettings.adaptive_engine.background_pressure_profile.ac_policy
    background_pressure_ac_boost_mode = $effectiveSettings.adaptive_engine.background_pressure_profile.ac_mode
    cpu_pressure_restraint_enabled = $effectiveSettings.adaptive_engine_process.cpu_pressure_restraint_enabled
    dynamic_resource_zones_enabled = $effectiveSettings.adaptive_engine_process.dynamic_resource_zones_enabled
    zone_foreground_share_percent = $effectiveSettings.adaptive_engine_process.dynamic_resource_zone_settings.foreground_share_percent
    limit_background_processors_enabled = $effectiveSettings.adaptive_engine_process.limit_background_processors_enabled
    processor_limit_percent = $effectiveSettings.adaptive_engine_process.processor_limit_percent
    process_restraint_threshold_percent = $effectiveSettings.adaptive_engine_process.background_app_cpu_threshold_percent
    maximum_restrained_apps = $effectiveSettings.adaptive_engine_process.maximum_restrained_apps
    foreground_or_system_cpu_threshold_percent = $effectiveSettings.adaptive_engine_process.foreground_or_system_cpu_threshold_percent
    benchmark_foreground_host = $benchmarkHostPath
    warmup_seconds_per_case = $WarmupSeconds
    cooldown_seconds = $CooldownSeconds
    original_power_scheme = $originalGuid
    stock_power_scheme = $balancedGuid
    validation = [pscustomobject]@{
        passed = $validationPassed
        stock_first_passes = @($runs | Where-Object { $_.order[0] -eq 'stock' }).Count
        adaptive_first_passes = @($runs | Where-Object { $_.order[0] -eq 'adaptive' }).Count
        activation_passes = $activationPasses
        native_process_control_passes = $processControlPasses
        median_improvement_percent = $medianImprovement
        p95_improvement_percent = $p95Improvement
        background_throughput_retained_percent = $backgroundRetained
        package_power_saving_percent = $powerSaving
        minimum_package_power_saving_percent = $MinimumPowerSavingPercent
        gates = [pscustomobject]@{
            activation_all_passes = $activationPasses -eq $Passes
            median_at_least_3_percent = $medianImprovement -ge 3.0
            p95_at_least_3_percent = $p95Improvement -ge 3.0
            background_retained_at_least_85_percent = $backgroundRetained -ge 85.0
            package_power_saving_at_least_minimum = $powerGatePassed
        }
    }
    summary = @(
        [pscustomobject]@{
            name = 'stock_balanced'
            median_ms = Get-AverageProperty -Items $stockRows -Name 'median_ms'
            p95_ms = Get-AverageProperty -Items $stockRows -Name 'p95_ms'
            foreground_iterations_per_sec = Get-AverageProperty -Items $stockRows -Name 'iterations_per_sec'
            package_power_median_w = Get-AverageProperty -Items $stockRows -Name 'package_power_median_w'
            background_suppression_percent = 0.0
            repeat_passes_won = 'baseline'
        },
        [pscustomobject]@{
            name = 'adaptive_runtime'
            median_ms = Get-AverageProperty -Items $adaptiveRows -Name 'median_ms'
            p95_ms = Get-AverageProperty -Items $adaptiveRows -Name 'p95_ms'
            foreground_iterations_per_sec = Get-AverageProperty -Items $adaptiveRows -Name 'iterations_per_sec'
            package_power_median_w = Get-AverageProperty -Items $adaptiveRows -Name 'package_power_median_w'
            median_improvement_percent = $medianImprovement
            p95_improvement_percent = $p95Improvement
            background_suppression_percent = Get-AverageProperty -Items $comparisons -Name 'background_suppression_percent_vs_off'
            package_power_saving_percent = $powerSaving
            repeat_passes_won = "$repeatWins/$Passes"
        }
    )
    runs = $runs
}
$json = $report | ConvertTo-Json -Depth 20
if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    $timestamp = Get-Date -Format 'yyyyMMdd-HHmmss'
    $OutputPath = Join-Path (Split-Path $PSScriptRoot -Parent) "benchmark\results\intel-core-5-210h-adaptive-runtime-$timestamp.json"
}
$outputDirectory = Split-Path $OutputPath -Parent
if (-not [string]::IsNullOrWhiteSpace($outputDirectory)) {
    [IO.Directory]::CreateDirectory($outputDirectory) | Out-Null
}
[IO.File]::WriteAllText($OutputPath, $json, [Text.UTF8Encoding]::new($false))
$json
