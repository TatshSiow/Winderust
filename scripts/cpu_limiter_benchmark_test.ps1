$ErrorActionPreference = 'Stop'

$scriptPath = Join-Path $PSScriptRoot 'cpu_limiter_benchmark.ps1'
$env:WINDERUST_CPU_LIMITER_BENCHMARK_IMPORT_ONLY = '1'
try {
    if (Test-Path -LiteralPath $scriptPath) {
        . $scriptPath
    }
} finally {
    Remove-Item Env:WINDERUST_CPU_LIMITER_BENCHMARK_IMPORT_ONLY -ErrorAction SilentlyContinue
}

if ($null -eq (Get-Command New-CpuLimiterMeasurement -ErrorAction SilentlyContinue)) {
    throw 'New-CpuLimiterMeasurement is not available.'
}
if ($null -eq (Get-Command Test-ProcessInAnyJob -ErrorAction SilentlyContinue)) {
    throw 'Test-ProcessInAnyJob is not available.'
}
if ($null -eq (Get-Command Test-IsAdministrator -ErrorAction SilentlyContinue)) {
    throw 'Test-IsAdministrator is not available.'
}

$isAdministrator = Test-IsAdministrator
if ($isAdministrator -isnot [bool]) {
    throw "Expected a Boolean administrator result, got $($isAdministrator.GetType().FullName)."
}

$currentProcess = [Diagnostics.Process]::GetProcessById($PID)
$inJob = Test-ProcessInAnyJob -Process $currentProcess
if ($inJob -isnot [bool]) {
    throw "Expected a Boolean Job membership result, got $($inJob.GetType().FullName)."
}

$measurement = New-CpuLimiterMeasurement `
    -WorkerCpuBeforeMs 250 `
    -WorkerCpuAfterMs 350 `
    -WinderustCpuBeforeMs 10 `
    -WinderustCpuAfterMs 30 `
    -ElapsedMilliseconds 10000 `
    -LogicalProcessors 20

$expected = [ordered]@{
    worker_cpu_ms = 100.0
    worker_one_core_percent = 1.0
    worker_task_manager_percent = 0.05
    winderust_cpu_ms = 20.0
    winderust_one_core_percent = 0.2
    winderust_task_manager_percent = 0.01
}
foreach ($entry in $expected.GetEnumerator()) {
    if ([Math]::Abs([double]$measurement.($entry.Key) - [double]$entry.Value) -gt 0.0001) {
        throw "Expected $($entry.Key)=$($entry.Value), got $($measurement.($entry.Key))."
    }
}

$invalidElapsedRejected = $false
try {
    New-CpuLimiterMeasurement `
        -WorkerCpuBeforeMs 0 `
        -WorkerCpuAfterMs 1 `
        -WinderustCpuBeforeMs 0 `
        -WinderustCpuAfterMs 1 `
        -ElapsedMilliseconds 0 `
        -LogicalProcessors 20 | Out-Null
} catch {
    $invalidElapsedRejected = $true
}
if (-not $invalidElapsedRejected) {
    throw 'Zero elapsed time must be rejected.'
}

'CPU Limiter benchmark checks passed.'
