param(
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
$benchmarkScript = Join-Path $PSScriptRoot 'workload_engine_benchmark.ps1'
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
$originalGuid = Get-ActiveSchemeGuid
$sourceExePath = (Resolve-Path -LiteralPath $WinderustExePath).Path
$configDir = Join-Path ([IO.Path]::GetTempPath()) "winderust-adaptive-benchmark-$([guid]::NewGuid())"
$exePath = Join-Path $configDir 'winderust.exe'
$configPath = Join-Path $configDir 'settings.toml'
$runtime = $null

$settingsToml = @'
[general]
enabled = true
startup_with_windows = false
start_minimized = true
hide_to_tray = false
check_interval_ms = 500

[adaptive_engine]
enabled = true
processor_policy_enabled = true

[adaptive_engine.processor_policy_values]
core_parking_min = 25
performance_min = 5
performance_max = 95
boost_policy = 60
boost_mode = "efficient_enabled"

[background_efficiency]
enabled = false

[workload_engine]
enabled = true
lower_background_apps = true
workload_engine_background_efficiency_enabled = true
workload_engine_background_priority = "below_normal"
workload_engine_visible_window_priority = "normal"
lower_background_io_priority_enabled = false
workload_engine_memory_priority_enabled = false
lower_background_auto_cpu_percent = true
workload_engine_enabled = true
workload_engine_affinity_escalation_enabled = false
workload_engine_total_threshold_percent = 75
workload_engine_threshold_percent = 10
workload_engine_restore_threshold_percent = 5
workload_engine_sustain_seconds = 3
workload_engine_minimum_restraint_seconds = 2
workload_engine_cooldown_seconds = 4
workload_engine_max_targeted_processes = 6
boost_foreground_app = true
foreground_boost = "auto"
workload_engine_exclusions = [{ enabled = true, executable_path = "__BENCHMARK_HOST_PATH__" }]

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
$benchmarkHostPath = [Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
$escapedBenchmarkHostPath = $benchmarkHostPath.Replace('\', '\\')
$settingsToml = $settingsToml.Replace(
    '__BENCHMARK_HOST_PATH__',
    $escapedBenchmarkHostPath
)

function Write-IsolatedSettings {
    [IO.Directory]::CreateDirectory($configDir) | Out-Null
    Copy-Item -LiteralPath $sourceExePath -Destination $exePath
    [IO.File]::WriteAllText($configPath, $settingsToml, [Text.UTF8Encoding]::new($false))
}

function Remove-IsolatedConfig {
    if (-not [IO.Directory]::Exists($configDir)) {
        return
    }
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        try {
            Remove-Item -LiteralPath $configDir -Recurse -Force -ErrorAction Stop
            return
        } catch {
            if ($attempt -eq 39) {
                throw
            }
            Start-Sleep -Milliseconds 250
        }
    }
}

function Start-AdaptiveRuntime {
    powercfg /setactive $balancedGuid | Out-Null
    $process = Start-Process -FilePath $exePath -PassThru -WindowStyle Hidden

    for ($attempt = 0; $attempt -lt 80; $attempt++) {
        Start-Sleep -Milliseconds 250
        $process.Refresh()
        if ($process.HasExited) {
            throw 'Winderust exited before Adaptive Engine activated.'
        }
        $planLine = powercfg /list | Where-Object { $_ -like '*Winderust Adaptive*' } | Select-Object -First 1
        if ($planLine) {
            return [pscustomobject]@{
                process = $process
                plan_guid = ([regex]::Match($planLine, '[0-9a-fA-F-]{36}')).Value
            }
        }
    }

    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    throw 'Adaptive Engine did not create its managed power plan within 20 seconds.'
}

function Stop-AdaptiveRuntime {
    param($Runtime)
    if ($null -eq $Runtime) {
        return
    }
    $process = $Runtime.process
    try {
        $process.Refresh()
        if (-not $process.HasExited) {
            [void]$process.CloseMainWindow()
            if (-not $process.WaitForExit(5000)) {
                Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
            }
        }
    } finally {
        powercfg /setactive $balancedGuid | Out-Null
        $managedPlanExists = powercfg /list | Where-Object { $_ -match [regex]::Escape($Runtime.plan_guid) }
        if ($managedPlanExists) {
            powercfg /delete $Runtime.plan_guid | Out-Null
        }
    }
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
}

function Run-StockCase {
    powercfg /setactive $balancedGuid | Out-Null
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
    return $result
}

function Run-AdaptiveCase {
    $script:runtime = Start-AdaptiveRuntime
    try {
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
        if (@($result.observed_worker_priorities | Where-Object { $_ -ne 'Normal' }).Count -eq 0) {
            throw "Invalid runtime benchmark: Workload Engine did not change any generated worker priority."
        }
        $result | Add-Member -NotePropertyName runtime_control_observed -NotePropertyValue $true
        $result | Add-Member -NotePropertyName adaptive_policy_after_load -NotePropertyValue (Read-ActiveProcessorPolicy)
        return $result
    } finally {
        Stop-AdaptiveRuntime -Runtime $script:runtime
        $script:runtime = $null
    }
}

if (@(Get-Process winderust -ErrorAction SilentlyContinue).Count -gt 0) {
    throw 'Close Winderust before running the runtime Adaptive Engine benchmark.'
}

Write-IsolatedSettings
Initialize-PowerCounter
$runs = @()
try {
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
} finally {
    Stop-AdaptiveRuntime -Runtime $runtime
    powercfg /setactive $originalGuid | Out-Null
    Remove-IsolatedConfig
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
$activationPasses = @($adaptiveRows | Where-Object { $_.runtime_control_observed }).Count
$powerGatePassed = $SkipPower -or ($null -ne $powerSaving -and $powerSaving -ge -2.0)
$validationPassed = $activationPasses -eq $Passes `
    -and $medianImprovement -ge 3.0 `
    -and $p95Improvement -ge 3.0 `
    -and $backgroundRetained -ge 85.0 `
    -and $powerGatePassed

$report = [pscustomobject]@{
    note = 'Counterbalanced release-binary A/B. Stock is Windows Balanced; Adaptive runs the isolated Winderust automation loop with the current Balanced and Low Impact presets.'
    cpu_name = Get-CpuName
    logical_processors = $logicalProcessors
    worker_count = $workerCount
    passes = $Passes
    rounds = $Rounds
    foreground_scenario = $ForegroundScenario
    foreground_iterations_per_round = $Iterations
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
        median_improvement_percent = $medianImprovement
        p95_improvement_percent = $p95Improvement
        background_throughput_retained_percent = $backgroundRetained
        package_power_saving_percent = $powerSaving
        gates = [pscustomobject]@{
            activation_all_passes = $activationPasses -eq $Passes
            median_at_least_3_percent = $medianImprovement -ge 3.0
            p95_at_least_3_percent = $p95Improvement -ge 3.0
            background_retained_at_least_85_percent = $backgroundRetained -ge 85.0
            package_power_regression_within_2_percent = $powerGatePassed
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
$json = $report | ConvertTo-Json -Depth 9
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
