param(
    [int]$Passes = 3,
    [int]$Rounds = 5,
    [int]$Iterations = 1000000,
    [int]$WorkerSeconds = 45,
    [int]$CooldownSeconds = 10,
    [string]$WinderustExePath = '.\target\release\winderust.exe',
    [switch]$SkipPower,
    [string]$OutputPath = ''
)

$ErrorActionPreference = 'Stop'
$benchmarkScript = Join-Path $PSScriptRoot 'workload_engine_benchmark.ps1'
$env:WINDERUST_BENCHMARK_IMPORT_ONLY = '1'
try {
    . $benchmarkScript `
        -Passes $Passes `
        -Rounds $Rounds `
        -Iterations $Iterations `
        -WorkerSeconds $WorkerSeconds `
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
check_interval_ms = 250

[adaptive_engine]
enabled = true
processor_policy_enabled = true

[adaptive_engine.processor_policy_values]
core_parking_min = 25
performance_min = 5
performance_max = 95
boost_policy = 60
boost_mode = "efficient_enabled"

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

function Write-IsolatedSettings {
    [IO.Directory]::CreateDirectory($configDir) | Out-Null
    Copy-Item -LiteralPath $sourceExePath -Destination $exePath
    [IO.File]::WriteAllText($configPath, $settingsToml, [Text.UTF8Encoding]::new($false))
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
        powercfg /delete $Runtime.plan_guid 2>$null
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

function Run-StockCase {
    powercfg /setactive $balancedGuid | Out-Null
    Run-Case `
        -Name 'stock_balanced' `
        -Model 'Windows Balanced with no Winderust runtime.' `
        -ForegroundPriority 'Normal' `
        -Priorities (New-Priorities -DefaultPriority 'Normal' -RestrainedCount 0 -RestrainedPriority 'Normal') `
        -AffinitySelectedCount 0 `
        -AffinityMask 0 `
        -AssistControls (New-AssistControls)
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
            -AssistControls (New-AssistControls)
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
    Remove-Item -LiteralPath $configDir -Recurse -Force -ErrorAction SilentlyContinue
}

$comparisons = @($runs | ForEach-Object { $_.comparison_vs_stock })
$stockRows = @($runs | ForEach-Object { $_.stock })
$adaptiveRows = @($runs | ForEach-Object { $_.adaptive })
$repeatWins = @($comparisons | Where-Object {
    $_.median_improvement_percent_vs_off -ge 3.0 -and $_.p95_improvement_percent_vs_off -ge 3.0
}).Count

$report = [pscustomobject]@{
    note = 'Real release-binary A/B. Stock is Windows Balanced; Adaptive runs the isolated Winderust automation loop.'
    cpu_name = Get-CpuName
    logical_processors = $logicalProcessors
    worker_count = $workerCount
    passes = $Passes
    rounds = $Rounds
    foreground_iterations_per_round = $Iterations
    cooldown_seconds = $CooldownSeconds
    original_power_scheme = $originalGuid
    stock_power_scheme = $balancedGuid
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
            median_improvement_percent = Get-AverageProperty -Items $comparisons -Name 'median_improvement_percent_vs_off'
            p95_improvement_percent = Get-AverageProperty -Items $comparisons -Name 'p95_improvement_percent_vs_off'
            background_suppression_percent = Get-AverageProperty -Items $comparisons -Name 'background_suppression_percent_vs_off'
            package_power_saving_percent = Get-AverageProperty -Items $comparisons -Name 'package_power_saving_percent_vs_off'
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
