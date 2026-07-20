param(
    [string]$CounterPath = '',
    [string[]]$Phases = @('Baseline', 'AdaptiveEngine'),
    [int]$MinPasses = 3,
    [int]$MaxPasses = 8,
    [int]$SampleSeconds = 30,
    [int]$SampleIntervalSeconds = 1,
    [int]$SettleSeconds = 5,
    [double]$StableCvPercent = 5.0,
    [string]$OutputDirectory = 'benchmark-results',
    [switch]$NoPrompt
)

$ErrorActionPreference = 'Stop'

function Get-CounterPaths {
    param([string]$CounterSetName)
    try {
        return @(Get-Counter -ListSet $CounterSetName | Select-Object -ExpandProperty PathsWithInstances)
    } catch {
        return @()
    }
}

function Resolve-PowerCounterPath {
    if (-not [string]::IsNullOrWhiteSpace($CounterPath)) {
        [void](Get-Counter -Counter $CounterPath -MaxSamples 1)
        return $CounterPath
    }

    $paths = @()
    $paths += Get-CounterPaths 'Energy Meter'
    $paths += Get-CounterPaths 'Power Meter'
    $powerPaths = @($paths | Where-Object { $_ -match '\\Power$' })

    foreach ($pattern in @(
        '\\Energy Meter\(RAPL_Package\d+_PKG\)\\Power$',
        '\\Energy Meter\(.*Package.*\)\\Power$',
        '\\Energy Meter\(Current Socket Power\)\\Power$',
        '\\Energy Meter\(Apu Power\)\\Power$',
        '\\Energy Meter\(_Total\)\\Power$',
        '\\Power Meter\(_Total\)\\Power$'
    )) {
        $match = @($powerPaths | Where-Object { $_ -match $pattern } | Select-Object -First 1)
        if ($match.Length -gt 0) {
            return $match[0]
        }
    }

    if ($powerPaths.Length -gt 0) {
        return $powerPaths[0]
    }

    throw 'No Energy Meter or Power Meter watt counter was found. Install/expose a CPU package power sensor or pass -CounterPath.'
}

function Get-PowerCounterScale {
    param([string]$ResolvedCounterPath)

    # ponytail: Windows Energy Meter exposes milliwatts on common RAPL providers; add vendor adapters only if this counter is missing.
    if ($ResolvedCounterPath -match '\\Energy Meter\(') {
        return 0.001
    }
    return 1.0
}

function Get-Average {
    param([double[]]$Values)
    if ($Values.Length -eq 0) {
        return 0.0
    }
    $sum = 0.0
    foreach ($value in $Values) {
        $sum += $value
    }
    return $sum / $Values.Length
}

function Get-SampleSummary {
    param([string]$Phase, [int]$Pass, [double[]]$Values)
    if ($Values.Length -eq 0) {
        throw "No valid watt samples were collected for $Phase."
    }

    $sorted = [double[]]$Values.Clone()
    [Array]::Sort($sorted)
    $avg = Get-Average $Values
    $median = $sorted[[int][Math]::Floor(($sorted.Length - 1) * 0.50)]
    $p95 = $sorted[[int][Math]::Floor(($sorted.Length - 1) * 0.95)]
    $variance = 0.0
    foreach ($value in $Values) {
        $variance += [Math]::Pow($value - $avg, 2)
    }
    $stddev = [Math]::Sqrt($variance / $Values.Length)
    $cv = if ($avg -gt 0.0) { ($stddev / $avg) * 100.0 } else { 0.0 }

    [pscustomobject][ordered]@{
        phase = $Phase
        pass = $Pass
        samples = $Values.Length
        avg_w = [Math]::Round($avg, 3)
        median_w = [Math]::Round($median, 3)
        p95_w = [Math]::Round($p95, 3)
        min_w = [Math]::Round($sorted[0], 3)
        max_w = [Math]::Round($sorted[$sorted.Length - 1], 3)
        stddev_w = [Math]::Round($stddev, 3)
        cv_percent = [Math]::Round($cv, 2)
    }
}

function Measure-PowerPhase {
    param([string]$Phase, [int]$Pass, [string]$ResolvedCounterPath, [double]$WattsScale)

    if (-not $NoPrompt) {
        [void](Read-Host "Set Winderust state for '$Phase', then press Enter")
    }
    if ($SettleSeconds -gt 0) {
        Start-Sleep -Seconds $SettleSeconds
    }

    $sampleCount = [Math]::Max(2, [Math]::Ceiling($SampleSeconds / [Math]::Max(1, $SampleIntervalSeconds)))
    $samples = Get-Counter -Counter $ResolvedCounterPath -SampleInterval $SampleIntervalSeconds -MaxSamples $sampleCount
    $values = @(
        $samples.CounterSamples |
            ForEach-Object { [double]$_.CookedValue * $WattsScale } |
            Where-Object { -not [double]::IsNaN($_) -and -not [double]::IsInfinity($_) -and $_ -ge 0.0 }
    )

    Get-SampleSummary -Phase $Phase -Pass $Pass -Values ([double[]]$values)
}

function Summarize-Repeats {
    param([object[]]$Rows)
    $summaries = @()
    foreach ($group in ($Rows | Group-Object -Property phase)) {
        $medianValues = [double[]]@($group.Group | ForEach-Object { [double]$_.median_w })
        $avgMedian = Get-Average $medianValues
        $variance = 0.0
        foreach ($value in $medianValues) {
            $variance += [Math]::Pow($value - $avgMedian, 2)
        }
        $stddev = [Math]::Sqrt($variance / [Math]::Max(1, $medianValues.Length))
        $cv = if ($avgMedian -gt 0.0) { ($stddev / $avgMedian) * 100.0 } else { 0.0 }
        $summaries += [pscustomobject][ordered]@{
            phase = $group.Name
            passes = $medianValues.Length
            median_w_avg = [Math]::Round($avgMedian, 3)
            median_w_min = [Math]::Round(($medianValues | Measure-Object -Minimum).Minimum, 3)
            median_w_max = [Math]::Round(($medianValues | Measure-Object -Maximum).Maximum, 3)
            median_w_cv_percent = [Math]::Round($cv, 2)
            stable = ($medianValues.Length -ge $MinPasses -and $cv -le $StableCvPercent)
        }
    }
    return $summaries
}

function Add-BaselineDeltas {
    param([object[]]$Summaries)
    if ($Summaries.Length -lt 2) {
        return $Summaries
    }

    $baseline = $Summaries | Where-Object { $_.phase -eq $Phases[0] } | Select-Object -First 1
    if ($null -eq $baseline -or [double]$baseline.median_w_avg -le 0.0) {
        return $Summaries
    }

    foreach ($summary in $Summaries) {
        $delta = [double]$summary.median_w_avg - [double]$baseline.median_w_avg
        $saving = (([double]$baseline.median_w_avg - [double]$summary.median_w_avg) / [double]$baseline.median_w_avg) * 100.0
        $summary | Add-Member -Force -NotePropertyName median_delta_w_vs_baseline -NotePropertyValue ([Math]::Round($delta, 3))
        $summary | Add-Member -Force -NotePropertyName saving_percent_vs_baseline -NotePropertyValue ([Math]::Round($saving, 1))
    }
    return $Summaries
}

if ($MinPasses -lt 1 -or $MaxPasses -lt $MinPasses) {
    throw '-MaxPasses must be greater than or equal to -MinPasses.'
}
if ($SampleSeconds -lt 1 -or $SampleIntervalSeconds -lt 1) {
    throw '-SampleSeconds and -SampleIntervalSeconds must be at least 1.'
}
if ($Phases.Length -lt 1) {
    throw '-Phases must include at least one phase.'
}

$resolvedCounterPath = Resolve-PowerCounterPath
$wattsScale = Get-PowerCounterScale $resolvedCounterPath
$rows = @()
$finalSummary = @()
$stable = $false

Write-Host "Power counter: $resolvedCounterPath"
if ($wattsScale -ne 1.0) {
    Write-Host "Counter scale: raw value * $wattsScale = watts"
}
Write-Host "Stable target: phase median CV <= $StableCvPercent% after at least $MinPasses passes"

for ($pass = 1; $pass -le $MaxPasses; $pass++) {
    foreach ($phase in $Phases) {
        $row = Measure-PowerPhase -Phase $phase -Pass $pass -ResolvedCounterPath $resolvedCounterPath -WattsScale $wattsScale
        $rows += $row
        Write-Host ("{0} pass {1}: median {2:N3} W, avg {3:N3} W, CV {4:N2}%" -f $phase, $pass, $row.median_w, $row.avg_w, $row.cv_percent)
    }

    $finalSummary = @(Add-BaselineDeltas (Summarize-Repeats $rows))
    $stable = -not ($finalSummary | Where-Object { -not $_.stable })
    if ($stable) {
        break
    }
}

New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$timestamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$jsonPath = Join-Path $OutputDirectory "power-drain-$timestamp.json"
$csvPath = Join-Path $OutputDirectory "power-drain-$timestamp.csv"

$report = [ordered]@{
    counter_path = $resolvedCounterPath
    counter_watts_scale = $wattsScale
    sample_seconds = $SampleSeconds
    sample_interval_seconds = $SampleIntervalSeconds
    min_passes = $MinPasses
    max_passes = $MaxPasses
    stable_cv_percent = $StableCvPercent
    stable = $stable
    rows = $rows
    summary = $finalSummary
}

$report | ConvertTo-Json -Depth 6 | Set-Content -Path $jsonPath -Encoding UTF8
$rows | Export-Csv -Path $csvPath -NoTypeInformation -Encoding UTF8

Write-Host ''
Write-Host "Summary:"
$finalSummary | Format-Table -AutoSize
Write-Host ''
Write-Host "Wrote $jsonPath"
Write-Host "Wrote $csvPath"
