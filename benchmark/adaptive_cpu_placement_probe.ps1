param(
    [Parameter(Mandatory)] [int]$ProcessId,
    [Parameter(Mandatory)] [string]$PresentMonPath,
    [ValidateRange(10, 120)] [int]$Seconds = 30,
    [ValidateRange(1, 16)] [int]$BackgroundWorkers = 4,
    [ValidateRange(0, 60)] [int]$CooldownSeconds = 10,
    [string]$OutputPath = ''
)

$ErrorActionPreference = 'Stop'
$target = [Diagnostics.Process]::GetProcessById($ProcessId)
$originalAffinity = [int64]$target.ProcessorAffinity
$processorCount = [Environment]::ProcessorCount
if ($processorCount -gt 62) {
    throw 'This screening probe supports at most 62 logical processors.'
}
$reservedCount = [Math]::Max(2, [Math]::Ceiling($processorCount * 0.25))
$gameCount = $processorCount - $reservedCount
$fullMask = (1L -shl $processorCount) - 1
$gameMask = (1L -shl $gameCount) - 1
$backgroundMask = $fullMask -bxor $gameMask
$cscript = Join-Path $env:SystemRoot 'System32\cscript.exe'
$workerScript = Join-Path ([IO.Path]::GetTempPath()) "winderust-placement-worker-$PID.js"
$captureDir = Join-Path ([IO.Path]::GetTempPath()) "winderust-placement-$PID"
[IO.Directory]::CreateDirectory($captureDir) | Out-Null
[IO.File]::WriteAllText($workerScript, @'
var deadline = Date.now() + parseInt(WScript.Arguments(0), 10) * 1000;
var value = 0;
while (Date.now() < deadline) {
    for (var i = 1; i <= 100000; i++) value += Math.sqrt(i);
}
'@, [Text.UTF8Encoding]::new($false))

function Get-Percentile([double[]]$Values, [double]$Percentile) {
    if ($Values.Count -eq 0) { return $null }
    $sorted = [double[]]$Values.Clone()
    [Array]::Sort($sorted)
    return $sorted[[int][Math]::Floor(($sorted.Count - 1) * $Percentile)]
}

function Get-Average([double[]]$Values) {
    if ($Values.Count -eq 0) { return $null }
    return ($Values | Measure-Object -Average).Average
}

function Start-Workers([int64]$AffinityMask) {
    $workers = @()
    for ($index = 0; $index -lt $BackgroundWorkers; $index++) {
        $worker = Start-Process -FilePath $cscript -ArgumentList @(
            '//B', '//NoLogo', $workerScript, ($Seconds + 10)
        ) -WindowStyle Hidden -PassThru
        Start-Sleep -Milliseconds 100
        $worker.ProcessorAffinity = [IntPtr]$AffinityMask
        $workers += $worker
    }
    return $workers
}

function Stop-Workers([object[]]$Workers) {
    foreach ($worker in $Workers) {
        try {
            if (-not $worker.HasExited) { $worker.Kill() }
        } catch {}
        $worker.Dispose()
    }
}

function Invoke-Phase([string]$Mode, [int]$Index) {
    $isZones = $Mode -eq 'zones'
    $target.Refresh()
    $target.ProcessorAffinity = [IntPtr]$(if ($isZones) { $gameMask } else { $fullMask })
    $workers = Start-Workers $(if ($isZones) { $backgroundMask } else { $fullMask })
    $csv = Join-Path $captureDir "phase-$Index-$Mode.csv"
    try {
        Start-Sleep -Seconds 2
        $workerCpuBefore = ($workers | ForEach-Object { $_.TotalProcessorTime.TotalMilliseconds } | Measure-Object -Sum).Sum
        & $PresentMonPath --process_id $ProcessId --delay 2 --timed $Seconds `
            --terminate_after_timed --output_file $csv --v2_metrics --exclude_dropped --no_console_stats |
            Out-Host
        if ($LASTEXITCODE -ne 0) { throw "PresentMon failed with exit code $LASTEXITCODE." }
        $workerCpuAfter = ($workers | ForEach-Object { $_.Refresh(); $_.TotalProcessorTime.TotalMilliseconds } | Measure-Object -Sum).Sum
        $rows = @(Import-Csv -LiteralPath $csv)
        if ($rows.Count -lt 60) { throw "Only $($rows.Count) frames were captured." }
        $frame = [double[]]@($rows.FrameTime)
        $cpuBusy = [double[]]@($rows.CPUBusy)
        $gpuTime = [double[]]@($rows.GPUTime | Where-Object { $_ -ne 'NA' })
        $displayLatency = [double[]]@($rows.DisplayLatency | Where-Object { $_ -ne 'NA' })
        $animationError = [double[]]@($rows.AnimationError | Where-Object { $_ -ne 'NA' })
        [pscustomobject]@{
            phase = $Index
            mode = $Mode
            frames = $rows.Count
            frame_avg_ms = [Math]::Round((Get-Average $frame), 4)
            frame_median_ms = [Math]::Round((Get-Percentile $frame 0.50), 4)
            frame_p95_ms = [Math]::Round((Get-Percentile $frame 0.95), 4)
            frame_p99_ms = [Math]::Round((Get-Percentile $frame 0.99), 4)
            presented_fps = [Math]::Round(1000.0 / (Get-Average $frame), 3)
            cpu_busy_avg_ms = [Math]::Round((Get-Average $cpuBusy), 4)
            gpu_time_avg_ms = [Math]::Round((Get-Average $gpuTime), 4)
            display_latency_p95_ms = [Math]::Round((Get-Percentile $displayLatency 0.95), 4)
            animation_error_p95_ms = [Math]::Round((Get-Percentile $animationError 0.95), 4)
            background_cpu_seconds = [Math]::Round(($workerCpuAfter - $workerCpuBefore) / 1000.0, 4)
            game_affinity_mask = ('0x{0:X}' -f [int64]$target.ProcessorAffinity)
            background_affinity_mask = ('0x{0:X}' -f $(if ($isZones) { $backgroundMask } else { $fullMask }))
        }
    } finally {
        Stop-Workers $workers
        $target.ProcessorAffinity = [IntPtr]$originalAffinity
    }
}

$runs = @()
try {
    $order = @('shared', 'zones', 'zones', 'shared', 'shared', 'zones')
    for ($index = 0; $index -lt $order.Count; $index++) {
        Write-Host "Starting phase $($index + 1)/$($order.Count): $($order[$index])"
        $runs += Invoke-Phase -Mode $order[$index] -Index ($index + 1)
        $runs[-1] | Format-List | Out-Host
        if ($index -lt ($order.Count - 1) -and $CooldownSeconds -gt 0) {
            Start-Sleep -Seconds $CooldownSeconds
        }
    }
} finally {
    try { $target.ProcessorAffinity = [IntPtr]$originalAffinity } catch {}
    Remove-Item -LiteralPath $workerScript -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $captureDir -Recurse -Force -ErrorAction SilentlyContinue
}

$summary = [pscustomobject]@{
    captured_at = (Get-Date).ToString('o')
    process_id = $ProcessId
    process_name = $target.ProcessName
    logical_processors = $processorCount
    original_affinity_mask = ('0x{0:X}' -f $originalAffinity)
    shared_mask = ('0x{0:X}' -f $fullMask)
    zone_game_mask = ('0x{0:X}' -f $gameMask)
    zone_background_mask = ('0x{0:X}' -f $backgroundMask)
    seconds_per_phase = $Seconds
    background_workers = $BackgroundWorkers
    runs = $runs
}
if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    $OutputPath = Join-Path $PSScriptRoot "results\adaptive-cpu-placement-overwatch-screening-$(Get-Date -Format yyyyMMdd-HHmmss).json"
}
$summary | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $OutputPath -Encoding utf8
Write-Host "Saved $OutputPath"
