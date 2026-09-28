#Requires -Version 5.1
#Requires -RunAsAdministrator
param(
    [string]$WinderustExePath = '.\target\release\winderust.exe',
    [ValidateSet('Default', 'DefaultPCores', 'Speed', 'SpeedThreadDefault', 'SpeedThreadDefaultPowerOff', 'SpeedRestraint', 'SpeedRestraintThreadDefault')]
    [string[]]$Cases = @('Default', 'Speed', 'SpeedThreadDefault', 'SpeedThreadDefaultPowerOff'),
    [ValidateRange(1, 10)][int]$Passes = 4,
    [ValidateRange(1, 30)][int]$Seconds = 10,
    [ValidateRange(1, 30)][int]$WarmupSeconds = 5,
    [ValidateRange(0, 60)][int]$CooldownSeconds = 10,
    [ValidateRange(1, 64)][int]$Threads = [Environment]::ProcessorCount,
    [switch]$Telemetry,
    [string]$OutputDirectory = ''
)
$ErrorActionPreference = 'Stop'
if ($PSEdition -ne 'Desktop') { throw 'Use 64-bit Windows PowerShell 5.1.' }
if (!$OutputDirectory) { $OutputDirectory = Join-Path $PWD ('target\throughput-{0}' -f (Get-Date -Format 'yyyyMMdd-HHmmss')) }
$OutputDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Use a new output directory.' }
$output = [IO.Directory]::CreateDirectory($OutputDirectory).FullName
$source = (Resolve-Path -LiteralPath $WinderustExePath).Path
$hostDir = [IO.Directory]::CreateDirectory((Join-Path $output 'host')).FullName
$exe = Join-Path $hostDir 'winderust.exe'
Copy-Item -LiteralPath $source -Destination $exe
$workload = Join-Path $output 'workload.ps1'
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'cpu_throughput_benchmark.ps1') -Destination $workload
Copy-Item -LiteralPath $PSCommandPath -Destination (Join-Path $output 'controller.ps1')
$manifest = [ordered]@{
    Cases = $Cases; Passes = $Passes; Seconds = $Seconds; WarmupSeconds = $WarmupSeconds
    CooldownSeconds = $CooldownSeconds; Threads = $Threads
    Telemetry = $Telemetry.IsPresent
    BinarySha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
    SourceCommit = (git rev-parse HEAD); StartedUtc = [DateTime]::UtcNow.ToString('o')
}
$manifest | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $output 'manifest.json') -Encoding UTF8
$rows = @()
$baselineGuid = $null
Start-Transcript -LiteralPath (Join-Path $output 'transcript.txt') | Out-Null
try {
    for ($pass = 0; $pass -lt $Passes; $pass++) {
        # Rotate case positions; four passes balance a four-case matrix.
        for ($position = 0; $position -lt $Cases.Count; $position++) {
            $case = $Cases[($position + $pass) % $Cases.Count]
            $directory = [IO.Directory]::CreateDirectory((Join-Path $output ("{0}-{1}" -f ($pass + 1), $case))).FullName
            $process = $null
            try {
                $process = Start-Process -FilePath $exe -ArgumentList @('--runtime-benchmark', ('"{0}"' -f $directory), $case) -WindowStyle Hidden -PassThru
                $deadline = [DateTime]::UtcNow.AddSeconds(30)
                while (!(Test-Path -LiteralPath (Join-Path $directory 'ready.txt'))) {
                    if ($process.HasExited) {
                        $failure = Get-Content -LiteralPath (Join-Path $directory 'finished.txt') -ErrorAction SilentlyContinue
                        throw "Runtime failed: $failure"
                    }
                    if ([DateTime]::UtcNow -gt $deadline) { throw 'Runtime startup timed out. Build with --features runtime-benchmark and close other Winderust instances.' }
                    if ('CpuThroughputBench' -as [type]) { [CpuThroughputBench]::Pump() }
                    Start-Sleep -Milliseconds 100
                }
                $currentGuid = Get-Content -LiteralPath (Join-Path $directory 'ready.txt')
                if ($null -eq $baselineGuid) { $baselineGuid = $currentGuid }
                elseif ($currentGuid -ne $baselineGuid) { throw 'The baseline power plan changed between cases.' }
                $csv = Join-Path $directory 'scores.csv'
                [UInt64]$affinity = 0
                if ($case -eq 'DefaultPCores') {
                    foreach ($cpu in (Import-Csv -LiteralPath (Join-Path $directory 'topology.csv') | Where-Object kind -eq 'Performance')) {
                        $affinity = $affinity -bor ([UInt64]1 -shl [int]$cpu.index)
                    }
                    if ($affinity -eq 0) { throw 'No verified performance-core mask available.' }
                }
                $monitorIds = @(Get-Process winderust | Where-Object { $_.Path -eq $exe } | Select-Object -ExpandProperty Id)
                if ($monitorIds.Count -ne 2) { throw 'Expected exactly the runtime and its recovery helper.' }
                & $workload -Label $case -Rounds 1 -Seconds $Seconds -WarmupSeconds $WarmupSeconds -CooldownSeconds $CooldownSeconds -Threads $Threads -OutputPath $csv -NoPrompt -KeepWindowOpen -SingleThreadAffinityMask $affinity -MonitorProcessIds $monitorIds -Telemetry:$Telemetry
                $batch = @(Import-Csv -LiteralPath $csv)
                if ($batch.Count -ne 2 -or @($batch | Where-Object { $_.FocusMaintained -ne 'True' }).Count) {
                    throw 'Focus was lost; this batch is invalid. Keep the benchmark window active.'
                }
                foreach ($row in $batch) { $row | Add-Member -NotePropertyName Pass -NotePropertyValue ($pass + 1) }
            } finally {
                if ($null -ne $process) {
                    [IO.File]::WriteAllText((Join-Path $directory 'stop'), '')
                    $deadline = [DateTime]::UtcNow.AddSeconds(60)
                    while (!$process.HasExited -and [DateTime]::UtcNow -lt $deadline) {
                        if ('CpuThroughputBench' -as [type]) { [CpuThroughputBench]::Pump() }
                        Start-Sleep -Milliseconds 50
                    }
                    if (!$process.HasExited) { throw 'Runtime shutdown timed out. It was NOT killed; recovery must complete before another test.' }
                    $finished = Get-Content -LiteralPath (Join-Path $directory 'finished.txt')
                    if ($finished -ne 'ok') { throw "Restoration or runtime failed: $finished" }
                    $process.Dispose()
                }
            }
            # Accept results only after successful runtime and watchdog shutdown.
            $rows += $batch
            $rows | Export-Csv -LiteralPath (Join-Path $output 'scores.csv') -NoTypeInformation
        }
    }
    $rows | Group-Object Label,Mode | ForEach-Object {
        $values = @($_.Group | ForEach-Object { [double]::Parse($_.BlocksPerSecond, [Globalization.CultureInfo]::CurrentCulture) } | Sort-Object)
        [pscustomobject]@{ Case = $_.Name; Runs = $values.Count; Mean = ($values | Measure-Object -Average).Average; Min = $values[0]; Max = $values[-1] }
    } | Export-Csv -LiteralPath (Join-Path $output 'summary.csv') -NoTypeInformation
    Write-Host "Completed and restored: $output"
} finally {
    if ('CpuThroughputBench' -as [type]) { [CpuThroughputBench]::Close() }
    Stop-Transcript | Out-Null
}
