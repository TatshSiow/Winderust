param(
    [ValidateRange(5, 300)]
    [int]$Seconds = 15,
    [ValidateRange(1, 10)]
    [int]$SampleIntervalSeconds = 1,
    [string]$OutputPath = ''
)

$ErrorActionPreference = 'Stop'
$sampleCount = [Math]::Max(2, [Math]::Ceiling($Seconds / $SampleIntervalSeconds))
$processorCount = [Environment]::ProcessorCount
$probe = [Diagnostics.Process]::GetCurrentProcess()
$probeCpuBefore = $probe.TotalProcessorTime.TotalMilliseconds
$startedAt = [DateTimeOffset]::UtcNow

$samples = Get-Counter -Counter @(
    '\Processor Information(_Total)\% Processor Utility',
    '\Processor Information(*)\% Processor Utility',
    '\GPU Engine(*)\Utilization Percentage'
) -SampleInterval $SampleIntervalSeconds -MaxSamples $sampleCount

$rows = foreach ($sample in $samples) {
    $cpu = @($sample.CounterSamples | Where-Object Path -Like '*processor information*')
    $cpuTotal = @($cpu | Where-Object InstanceName -EQ '_total' | Select-Object -First 1).CookedValue
    $cpuCores = @($cpu | Where-Object InstanceName -NE '_total' | ForEach-Object CookedValue)

    $engineTotals = @{}
    foreach ($counter in $sample.CounterSamples | Where-Object Path -Like '*gpu engine*') {
        $engine = $counter.InstanceName -replace '^pid_\d+_', ''
        $engineTotals[$engine] = [double]($engineTotals[$engine] + $counter.CookedValue)
    }
    $gpu = @($engineTotals.Values | ForEach-Object { [Math]::Min(100.0, $_) } | Sort-Object -Descending | Select-Object -First 1)

    $total = if ($cpuTotal.Count) { [double]$cpuTotal[0] } else { 0.0 }
    $core = if ($cpuCores.Count) { [double]($cpuCores | Measure-Object -Maximum).Maximum } else { 0.0 }
    $gpuBusy = if ($gpu.Count) { [double]$gpu[0] } else { 0.0 }
    $cpuBusy = $total -ge 85.0 -or $core -ge 90.0
    $gpuBusyEnough = $gpuBusy -ge 75.0
    $state = if ($total -ge 85.0 -and $gpuBusyEnough) {
        'Mixed'
    } elseif ($gpuBusyEnough) {
        'GPU Bound'
    } elseif ($cpuBusy) {
        'CPU Bound'
    } else {
        'Headroom'
    }

    [pscustomobject]@{
        timestamp = $sample.Timestamp.ToUniversalTime().ToString('O')
        cpu_total_percent = [Math]::Round($total, 2)
        cpu_busiest_processor_percent = [Math]::Round($core, 2)
        gpu_busiest_engine_percent = [Math]::Round($gpuBusy, 2)
        state = $state
    }
}

$probe.Refresh()
$elapsed = ([DateTimeOffset]::UtcNow - $startedAt).TotalMilliseconds
$probeCpuMs = [Math]::Max(0.0, $probe.TotalProcessorTime.TotalMilliseconds - $probeCpuBefore)
$stateGroups = @($rows | Group-Object state | Sort-Object Count -Descending)
$report = [pscustomobject]@{
    recorded_at = [DateTimeOffset]::UtcNow.ToString('O')
    duration_seconds = [Math]::Round($elapsed / 1000.0, 2)
    sample_interval_seconds = $SampleIntervalSeconds
    samples = $rows.Count
    dominant_state = if ($stateGroups.Count) { $stateGroups[0].Name } else { 'Unknown' }
    dominant_state_percent = if ($rows.Count) { [Math]::Round(($stateGroups[0].Count / $rows.Count) * 100.0, 1) } else { 0.0 }
    probe_cpu_percent = if ($elapsed -gt 0) { [Math]::Round(($probeCpuMs / ($elapsed * $processorCount)) * 100.0, 4) } else { 0.0 }
    observations = @($rows)
}

if ($OutputPath) {
    $report | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $OutputPath -Encoding utf8
}
$report
