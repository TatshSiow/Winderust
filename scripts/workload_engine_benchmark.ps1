param(
    [int]$Passes = 3,
    [int]$Rounds = 5,
    [int]$Iterations = 1000000,
    [int]$ScoreIterations = 3000000,
    [int]$ScoreDataKb = 512,
    [int]$ScoreRounds = 2,
    [int]$WorkerSeconds = 45,
    [ValidateSet('CpuLoop', 'IoLoop', 'MessageLoop', 'WinderustLaunch')]
    [string]$ForegroundScenario = 'CpuLoop',
    [int]$IoOperations = 2000,
    [int]$MessageLoopTicks = 200,
    [int]$MessageLoopIntervalMilliseconds = 16,
    [string]$WinderustExePath = '',
    [string]$PowerCounterPath = '',
    [switch]$SkipScoreBenchmark,
    [switch]$SkipPower
)

$ErrorActionPreference = 'Stop'

$powerShellPath = Join-Path $PSHOME 'powershell.exe'
$logicalProcessors = [Environment]::ProcessorCount
$workerCount = [Math]::Min([Math]::Max($logicalProcessors, 4), 12)
$lowImpactTargetCount = $workerCount
$lowImpactAllPCorePercent = 0.65
$foregroundFirstAllPCorePercent = 0.50
$maxForegroundCorePercent = 0.06
$lowImpactCoreCount = 0
$foregroundFirstCoreCount = 0
$maxForegroundCoreCount = [Math]::Max(1, [Math]::Ceiling($logicalProcessors * $maxForegroundCorePercent))

function New-Mask([int]$Count) {
    $mask = 0L
    for ($core = 0; $core -lt [Math]::Min($Count, 62); $core++) {
        $mask = $mask -bor (1L -shl $core)
    }
    return $mask
}

function Test-LikelyHybridTopology {
    try {
        $cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
        if ($cpu.Manufacturer -notlike '*Intel*') {
            return $false
        }
        $fullSmtLogicalCount = [int]$cpu.NumberOfCores * 2
        return [int]$cpu.NumberOfLogicalProcessors -ne $fullSmtLogicalCount
    } catch {
        return $false
    }
}

$hasHybridTopology = Test-LikelyHybridTopology
if (-not $hasHybridTopology) {
    $lowImpactCoreCount = [Math]::Max(1, [Math]::Ceiling($logicalProcessors * $lowImpactAllPCorePercent))
    $foregroundFirstCoreCount = [Math]::Max(1, [Math]::Ceiling($logicalProcessors * $foregroundFirstAllPCorePercent))
}
$lowImpactMask = New-Mask $lowImpactCoreCount
$foregroundFirstMask = New-Mask $foregroundFirstCoreCount
$maxForegroundMask = New-Mask $maxForegroundCoreCount
$script:resolvedPowerCounterPath = ''
$script:powerWattsScale = 1.0
$processorSubgroupGuid = '54533251-82be-4824-96c1-47b60b740d00'
$processorPolicySettings = @(
    [pscustomobject]@{ Name = 'core_parking_min'; Guid = '0cc5b647-c1df-4637-891a-dec35c318583'; Saver = 0; Balanced = 25; Performance = 100; Speed = 100 },
    [pscustomobject]@{ Name = 'performance_min'; Guid = '893dee8e-2bef-41e0-89c6-b55d0929964c'; Saver = 5; Balanced = 5; Performance = 25; Speed = 25 },
    [pscustomobject]@{ Name = 'performance_max'; Guid = 'bc5038f7-23e0-4960-96da-33abaf5935ec'; Saver = 45; Balanced = 95; Performance = 100; Speed = 100 },
    [pscustomobject]@{ Name = 'boost_policy'; Guid = '45bcc044-d885-43e2-8605-ee0ec6e96b59'; Saver = 0; Balanced = 60; Performance = 85; Speed = 100 },
    [pscustomobject]@{ Name = 'boost_mode'; Guid = 'be337238-0d82-4146-a960-4f3749d470c7'; Saver = 0; Balanced = 3; Performance = 4; Speed = 2 }
)

function Get-CounterPaths {
    param([string]$CounterSetName)
    try {
        return @(Get-Counter -ListSet $CounterSetName | Select-Object -ExpandProperty PathsWithInstances)
    } catch {
        return @()
    }
}

function Resolve-PowerCounterPath {
    if (-not [string]::IsNullOrWhiteSpace($PowerCounterPath)) {
        [void](Get-Counter -Counter $PowerCounterPath -MaxSamples 1)
        return $PowerCounterPath
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
    return ''
}

function Initialize-PowerCounter {
    if ($SkipPower) {
        return
    }
    try {
        $script:resolvedPowerCounterPath = Resolve-PowerCounterPath
        if ($script:resolvedPowerCounterPath -match '\\Energy Meter\(') {
            $script:powerWattsScale = 0.001
        }
    } catch {
        Write-Warning "Package power counter unavailable: $($_.Exception.Message)"
        $script:resolvedPowerCounterPath = ''
    }
}

function Add-PowerSample {
    param([System.Collections.Generic.List[double]]$Samples)
    if ([string]::IsNullOrWhiteSpace($script:resolvedPowerCounterPath)) {
        return
    }
    try {
        $sample = Get-Counter -Counter $script:resolvedPowerCounterPath -MaxSamples 1
        $value = [double]$sample.CounterSamples[0].CookedValue * $script:powerWattsScale
        if (-not [double]::IsNaN($value) -and -not [double]::IsInfinity($value) -and $value -ge 0.0) {
            $Samples.Add($value)
        }
    } catch {
    }
}

function Add-PowerSummary {
    param([pscustomobject]$Summary, [double[]]$Samples)
    if ($Samples.Length -eq 0) {
        $Summary | Add-Member -NotePropertyName package_power_samples -NotePropertyValue 0
        $Summary | Add-Member -NotePropertyName package_power_avg_w -NotePropertyValue $null
        $Summary | Add-Member -NotePropertyName package_power_median_w -NotePropertyValue $null
        $Summary | Add-Member -NotePropertyName package_power_p95_w -NotePropertyValue $null
        return
    }
    $sorted = [double[]]$Samples.Clone()
    [Array]::Sort($sorted)
    $avg = Get-Average $Samples
    $Summary | Add-Member -NotePropertyName package_power_samples -NotePropertyValue $Samples.Length
    $Summary | Add-Member -NotePropertyName package_power_avg_w -NotePropertyValue ([Math]::Round($avg, 3))
    $Summary | Add-Member -NotePropertyName package_power_median_w -NotePropertyValue ([Math]::Round($sorted[[int][Math]::Floor(($sorted.Length - 1) * 0.50)], 3))
    $Summary | Add-Member -NotePropertyName package_power_p95_w -NotePropertyValue ([Math]::Round($sorted[[int][Math]::Floor(($sorted.Length - 1) * 0.95)], 3))
}

function Get-ActiveSchemeGuid {
    $output = powercfg /getactivescheme
    $text = ($output -join ' ')
    if ($text -match '([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})') {
        return $matches[1]
    }
    throw 'Could not determine the active power scheme.'
}

function Get-PowerSettingIndexes {
    param([string]$SchemeGuid, [string]$SettingGuid)
    $output = powercfg /qh $SchemeGuid $processorSubgroupGuid $SettingGuid
    $text = $output -join "`n"
    if ($text -notmatch 'Current AC Power Setting Index:\s*0x([0-9a-fA-F]+)') {
        throw "Could not read AC processor setting $SettingGuid."
    }
    $ac = [Convert]::ToUInt32($matches[1], 16)
    if ($text -notmatch 'Current DC Power Setting Index:\s*0x([0-9a-fA-F]+)') {
        throw "Could not read DC processor setting $SettingGuid."
    }
    $dc = [Convert]::ToUInt32($matches[1], 16)
    [pscustomobject]@{ ac = $ac; dc = $dc }
}

function Set-PowerSettingIndexes {
    param([string]$SchemeGuid, [string]$SettingGuid, [uint32]$Ac, [uint32]$Dc)
    powercfg /setacvalueindex $SchemeGuid $processorSubgroupGuid $SettingGuid $Ac | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to set AC processor setting $SettingGuid."
    }
    powercfg /setdcvalueindex $SchemeGuid $processorSubgroupGuid $SettingGuid $Dc | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to set DC processor setting $SettingGuid."
    }
}

function Get-ProcessorPolicySnapshot {
    $scheme = Get-ActiveSchemeGuid
    $values = @{}
    foreach ($setting in $processorPolicySettings) {
        $values[$setting.Name] = Get-PowerSettingIndexes -SchemeGuid $scheme -SettingGuid $setting.Guid
    }
    [pscustomobject]@{ scheme = $scheme; values = $values }
}

function Restore-ProcessorPolicySnapshot {
    param($Snapshot)
    if ($null -eq $Snapshot) {
        return
    }
    foreach ($setting in $processorPolicySettings) {
        $value = $Snapshot.values[$setting.Name]
        Set-PowerSettingIndexes -SchemeGuid $Snapshot.scheme -SettingGuid $setting.Guid -Ac $value.ac -Dc $value.dc
    }
    powercfg /setactive $Snapshot.scheme | Out-Null
}

function Invoke-WithProcessorPolicy {
    param([ValidateSet('Saver', 'Balanced', 'Performance', 'Speed')] [string]$Preset, [scriptblock]$ScriptBlock)
    $snapshot = Get-ProcessorPolicySnapshot
    try {
        foreach ($setting in $processorPolicySettings) {
            $value = [uint32]$setting.$Preset
            Set-PowerSettingIndexes -SchemeGuid $snapshot.scheme -SettingGuid $setting.Guid -Ac $value -Dc $value
        }
        powercfg /setactive $snapshot.scheme | Out-Null
        & $ScriptBlock
    } finally {
        Restore-ProcessorPolicySnapshot -Snapshot $snapshot
    }
}

if (-not ('WorkloadEngineBenchmarkNative' -as [type])) {
    Add-Type -ReferencedAssemblies @('System.dll', 'System.Core.dll') -TypeDefinition @"
using System;
using System.Diagnostics;
using System.IO;
using System.IO.Compression;
using System.Runtime.InteropServices;
using System.Security.Cryptography;

public static class WorkloadEngineBenchmarkNative
{
    private static long sink;

    [StructLayout(LayoutKind.Sequential)]
    public struct MEMORY_PRIORITY_INFORMATION
    {
        public UInt32 MemoryPriority;
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool SetProcessInformation(IntPtr hProcess, Int32 processInformationClass, ref MEMORY_PRIORITY_INFORMATION processInformation, UInt32 processInformationSize);

    [DllImport("ntdll.dll")]
    public static extern Int32 NtSetInformationProcess(IntPtr processHandle, UInt32 processInformationClass, ref UInt32 processInformation, UInt32 processInformationLength);

    [DllImport("gdi32.dll")]
    public static extern Int32 D3DKMTSetProcessSchedulingPriorityClass(IntPtr processHandle, Int32 priority);

    public static bool IsVectorHardwareAccelerated()
    {
        return false;
    }

    public static int VectorFloatLanes()
    {
        return 1;
    }

    public static double IntArithmeticMops(int iterations)
    {
        iterations = Math.Max(1, iterations);
        long acc = 17;
        Stopwatch sw = Stopwatch.StartNew();
        for (int i = 1; i <= iterations; i++)
        {
            acc += (i * 1103515245L + 12345L) ^ (acc >> 7);
            acc ^= acc << 11;
        }
        sw.Stop();
        sink ^= acc;
        return Rate(iterations * 4L, sw, 1000000.0);
    }

    public static double DoubleArithmeticMops(int iterations)
    {
        iterations = Math.Max(1, iterations);
        double acc = 0.25;
        Stopwatch sw = Stopwatch.StartNew();
        for (int i = 1; i <= iterations; i++)
        {
            double value = (double)i;
            acc += Math.Sqrt(value) * 1.0000001;
            acc /= 1.0000003;
        }
        sw.Stop();
        sink ^= (long)acc;
        return Rate(iterations * 4L, sw, 1000000.0);
    }

    public static double SimdFloatMops(int iterations)
    {
        iterations = Math.Max(1, iterations);
        float a0 = 1.0f, a1 = 2.0f, a2 = 3.0f, a3 = 4.0f;
        float b0 = 5.0f, b1 = 6.0f, b2 = 7.0f, b3 = 8.0f;
        Stopwatch sw = Stopwatch.StartNew();
        for (int i = 0; i < iterations; i++)
        {
            a0 = (a0 + b0) * 1.00001f;
            a1 = (a1 + b1) * 1.00002f;
            a2 = (a2 + b2) * 0.99999f;
            a3 = (a3 + b3) * 0.99998f;
            b0 = (b0 + a3) * 0.99997f;
            b1 = (b1 + a2) * 1.00003f;
            b2 = (b2 + a1) * 1.00004f;
            b3 = (b3 + a0) * 0.99996f;
        }
        sw.Stop();
        sink ^= (long)(a0 + a1 + a2 + a3 + b0 + b1 + b2 + b3);
        return Rate((long)iterations * 16L, sw, 1000000.0);
    }

    public static double GZipRoundTripMbps(int kiloBytes, int rounds)
    {
        byte[] data = MakeData(kiloBytes);
        rounds = Math.Max(1, rounds);
        long bytes = 0;
        Stopwatch sw = Stopwatch.StartNew();
        for (int round = 0; round < rounds; round++)
        {
            byte[] compressed = Compress(data, true);
            byte[] decompressed = Decompress(compressed, true);
            bytes += data.Length * 2L;
            sink ^= decompressed[decompressed.Length - 1];
        }
        sw.Stop();
        return Rate(bytes, sw, 1024.0 * 1024.0);
    }

    public static double DeflateRoundTripMbps(int kiloBytes, int rounds)
    {
        byte[] data = MakeData(kiloBytes);
        rounds = Math.Max(1, rounds);
        long bytes = 0;
        Stopwatch sw = Stopwatch.StartNew();
        for (int round = 0; round < rounds; round++)
        {
            byte[] compressed = Compress(data, false);
            byte[] decompressed = Decompress(compressed, false);
            bytes += data.Length * 2L;
            sink ^= decompressed[decompressed.Length - 1];
        }
        sw.Stop();
        return Rate(bytes, sw, 1024.0 * 1024.0);
    }

    public static double Sha256Mbps(int kiloBytes, int rounds)
    {
        byte[] data = MakeData(kiloBytes);
        rounds = Math.Max(1, rounds);
        long bytes = 0;
        Stopwatch sw = Stopwatch.StartNew();
        using (SHA256 sha = SHA256.Create())
        {
            for (int round = 0; round < rounds; round++)
            {
                byte[] hash = sha.ComputeHash(data);
                bytes += data.Length;
                sink ^= hash[0];
            }
        }
        sw.Stop();
        return Rate(bytes, sw, 1024.0 * 1024.0);
    }

    public static double AesCbcRoundTripMbps(int kiloBytes, int rounds)
    {
        byte[] data = MakeData(kiloBytes);
        rounds = Math.Max(1, rounds);
        long bytes = 0;
        Stopwatch sw = Stopwatch.StartNew();
        using (Aes aes = Aes.Create())
        {
            aes.Mode = CipherMode.CBC;
            aes.Padding = PaddingMode.PKCS7;
            aes.KeySize = 128;
            aes.GenerateKey();
            aes.GenerateIV();
            for (int round = 0; round < rounds; round++)
            {
                byte[] encrypted;
                using (ICryptoTransform encryptor = aes.CreateEncryptor())
                {
                    encrypted = encryptor.TransformFinalBlock(data, 0, data.Length);
                }
                byte[] decrypted;
                using (ICryptoTransform decryptor = aes.CreateDecryptor())
                {
                    decrypted = decryptor.TransformFinalBlock(encrypted, 0, encrypted.Length);
                }
                bytes += data.Length * 2L;
                sink ^= decrypted[decrypted.Length - 1];
            }
        }
        sw.Stop();
        return Rate(bytes, sw, 1024.0 * 1024.0);
    }

    public static double MemoryScanMbps(int kiloBytes, int rounds)
    {
        byte[] data = MakeData(kiloBytes);
        rounds = Math.Max(1, rounds);
        long sum = 0;
        Stopwatch sw = Stopwatch.StartNew();
        for (int round = 0; round < rounds; round++)
        {
            for (int index = 0; index < data.Length; index++)
            {
                sum += data[index];
            }
        }
        sw.Stop();
        sink ^= sum;
        return Rate((long)data.Length * rounds, sw, 1024.0 * 1024.0);
    }

    public static double MemoryCopyMbps(int kiloBytes, int rounds)
    {
        byte[] source = MakeData(kiloBytes);
        byte[] target = new byte[source.Length];
        rounds = Math.Max(1, rounds);
        Stopwatch sw = Stopwatch.StartNew();
        for (int round = 0; round < rounds; round++)
        {
            Buffer.BlockCopy(source, 0, target, 0, source.Length);
            sink ^= target[round % target.Length];
        }
        sw.Stop();
        return Rate((long)source.Length * rounds, sw, 1024.0 * 1024.0);
    }

    private static double Rate(long work, Stopwatch sw, double divisor)
    {
        if (sw.Elapsed.TotalSeconds <= 0.0)
        {
            return 0.0;
        }
        return (work / sw.Elapsed.TotalSeconds) / divisor;
    }

    private static byte[] MakeData(int kiloBytes)
    {
        int size = Math.Max(1, kiloBytes) * 1024;
        byte[] data = new byte[size];
        uint state = 2166136261u;
        for (int index = 0; index < data.Length; index++)
        {
            state = (state ^ (uint)index) * 16777619u;
            data[index] = (byte)(state >> 24);
        }
        return data;
    }

    private static byte[] Compress(byte[] data, bool gzip)
    {
        using (MemoryStream output = new MemoryStream())
        {
            Stream stream = gzip
                ? (Stream)new GZipStream(output, CompressionLevel.Fastest, true)
                : (Stream)new DeflateStream(output, CompressionLevel.Fastest, true);
            using (stream)
            {
                stream.Write(data, 0, data.Length);
            }
            return output.ToArray();
        }
    }

    private static byte[] Decompress(byte[] data, bool gzip)
    {
        using (MemoryStream input = new MemoryStream(data))
        using (MemoryStream output = new MemoryStream())
        {
            Stream stream = gzip
                ? (Stream)new GZipStream(input, CompressionMode.Decompress)
                : (Stream)new DeflateStream(input, CompressionMode.Decompress);
            using (stream)
            {
                byte[] buffer = new byte[8192];
                int read;
                while ((read = stream.Read(buffer, 0, buffer.Length)) > 0)
                {
                    output.Write(buffer, 0, read);
                }
            }
            return output.ToArray();
        }
    }
}
"@
}

$processMemoryPriorityClass = 0
$processIoPriorityClass = 33
$statusInvalidParameter = -1073741811
$memoryPriorityRaw = @{
    VeryLow = 1
    Low = 2
    Medium = 3
    BelowNormal = 4
    Normal = 5
}
$ioPriorityRaw = @{
    VeryLow = 0
    Low = 1
    Normal = 2
    High = 3
    Critical = 4
}
$gpuPriorityRaw = @{
    Idle = 0
    BelowNormal = 1
    Normal = 2
    AboveNormal = 3
    High = 4
    Realtime = 5
}

function Get-CpuName {
    try {
        return ((Get-CimInstance Win32_Processor | Select-Object -First 1 -ExpandProperty Name).Trim())
    } catch {
        return 'unknown'
    }
}

$scoreBenchmarkFields = @(
    'int_arithmetic_mops',
    'double_arithmetic_mops',
    'float_batch_mops',
    'gzip_roundtrip_mbps',
    'deflate_roundtrip_mbps',
    'sha256_mbps',
    'aes_cbc_roundtrip_mbps',
    'l2_cache_scan_mbps',
    'memory_copy_mbps'
)

function Invoke-ScoreMetric {
    param(
        [scriptblock]$ScriptBlock,
        [System.Collections.Generic.List[double]]$PowerSamples
    )

    try {
        $value = & $ScriptBlock
        Add-PowerSample -Samples $PowerSamples
        if ($null -eq $value) {
            return $null
        }
        return [Math]::Round([double]$value, 2)
    } catch {
        return $null
    }
}

function Measure-ScoreBenchmarks {
    param([System.Collections.Generic.List[double]]$PowerSamples)

    if ($SkipScoreBenchmark) {
        return $null
    }

    $scoreIterations = [Math]::Max(1, $ScoreIterations)
    $scoreDataKbValue = [Math]::Max(64, $ScoreDataKb)
    $scoreRoundsValue = [Math]::Max(1, $ScoreRounds)
    $l2CacheKb = 256
    $memoryCopyKb = [Math]::Max(8192, $scoreDataKbValue * 16)

    $score = [ordered]@{
        score_iterations = $scoreIterations
        score_data_kb = $scoreDataKbValue
        score_rounds = $scoreRoundsValue
        l2_cache_proxy_kb = $l2CacheKb
        memory_copy_kb = $memoryCopyKb
        instruction_set_probe = 'managed_float_batch_no_intrinsics'
        simd_vector_available = [WorkloadEngineBenchmarkNative]::IsVectorHardwareAccelerated()
        simd_float_lanes = [WorkloadEngineBenchmarkNative]::VectorFloatLanes()
    }

    $score.int_arithmetic_mops = Invoke-ScoreMetric -PowerSamples $PowerSamples -ScriptBlock {
        [WorkloadEngineBenchmarkNative]::IntArithmeticMops($scoreIterations)
    }
    $score.double_arithmetic_mops = Invoke-ScoreMetric -PowerSamples $PowerSamples -ScriptBlock {
        [WorkloadEngineBenchmarkNative]::DoubleArithmeticMops($scoreIterations)
    }
    $score.float_batch_mops = Invoke-ScoreMetric -PowerSamples $PowerSamples -ScriptBlock {
        [WorkloadEngineBenchmarkNative]::SimdFloatMops([Math]::Max(1, [int]($scoreIterations / 2)))
    }
    $score.gzip_roundtrip_mbps = Invoke-ScoreMetric -PowerSamples $PowerSamples -ScriptBlock {
        [WorkloadEngineBenchmarkNative]::GZipRoundTripMbps($scoreDataKbValue, $scoreRoundsValue)
    }
    $score.deflate_roundtrip_mbps = Invoke-ScoreMetric -PowerSamples $PowerSamples -ScriptBlock {
        [WorkloadEngineBenchmarkNative]::DeflateRoundTripMbps($scoreDataKbValue, $scoreRoundsValue)
    }
    $score.sha256_mbps = Invoke-ScoreMetric -PowerSamples $PowerSamples -ScriptBlock {
        [WorkloadEngineBenchmarkNative]::Sha256Mbps($scoreDataKbValue, $scoreRoundsValue * 4)
    }
    $score.aes_cbc_roundtrip_mbps = Invoke-ScoreMetric -PowerSamples $PowerSamples -ScriptBlock {
        [WorkloadEngineBenchmarkNative]::AesCbcRoundTripMbps($scoreDataKbValue, $scoreRoundsValue)
    }
    $score.l2_cache_scan_mbps = Invoke-ScoreMetric -PowerSamples $PowerSamples -ScriptBlock {
        [WorkloadEngineBenchmarkNative]::MemoryScanMbps($l2CacheKb, $scoreRoundsValue * 128)
    }
    $score.memory_copy_mbps = Invoke-ScoreMetric -PowerSamples $PowerSamples -ScriptBlock {
        [WorkloadEngineBenchmarkNative]::MemoryCopyMbps($memoryCopyKb, $scoreRoundsValue * 16)
    }

    return [pscustomobject]$score
}

function Get-ScoreBenchmarkComparison {
    param($Off, $Case)

    if ($null -eq $Off.score_benchmark -or $null -eq $Case.score_benchmark) {
        return $null
    }

    $properties = [ordered]@{}
    foreach ($field in $scoreBenchmarkFields) {
        $offValue = $Off.score_benchmark.$field
        $caseValue = $Case.score_benchmark.$field
        if ($null -eq $offValue -or $null -eq $caseValue) {
            continue
        }
        $offDouble = [double]$offValue
        $caseDouble = [double]$caseValue
        if ($offDouble -le 0.0 -or $caseDouble -le 0.0) {
            continue
        }
        $ratio = $caseDouble / $offDouble
        $properties["${field}_vs_off_percent"] = [Math]::Round($ratio * 100.0, 1)
    }
    if ($properties.Count -eq 0) {
        return $null
    }

    return [pscustomobject]$properties
}

function New-AssistControls {
    param(
        [string]$ForegroundPriorityBoost = 'Default',
        [string]$ForegroundThreadPriority = 'Default',
        [string]$ForegroundIoPriority = 'Default',
        [string]$ForegroundGpuPriority = 'Default',
        [string]$BackgroundPriorityBoost = 'Default',
        [string]$ThreadPriority = 'Default',
        [string]$MemoryPriority = 'Default',
        [string]$IoPriority = 'Default',
        [string]$GpuPriority = 'Default'
    )

    [pscustomobject][ordered]@{
        foreground_priority_boost = $ForegroundPriorityBoost
        foreground_thread_priority = $ForegroundThreadPriority
        foreground_io_priority = $ForegroundIoPriority
        foreground_gpu_priority = $ForegroundGpuPriority
        background_priority_boost = $BackgroundPriorityBoost
        thread_priority = $ThreadPriority
        memory_priority = $MemoryPriority
        io_priority = $IoPriority
        gpu_priority = $GpuPriority
    }
}

function New-AssistStatus {
    [pscustomobject][ordered]@{
        worker_processes = 0
        foreground_priority_boost_applied = 0
        foreground_thread_priority_applied = 0
        foreground_io_priority_applied = 0
        foreground_gpu_priority_applied = 0
        foreground_gpu_priority_unavailable = 0
        background_priority_boost_applied = 0
        thread_priority_threads_applied = 0
        memory_priority_processes_applied = 0
        io_priority_processes_applied = 0
        gpu_priority_processes_applied = 0
        gpu_priority_unavailable = 0
        failed_actions = 0
    }
}

function Set-ProcessPriorityBoostSetting {
    param([Diagnostics.Process]$Process, [string]$Setting)
    if ([string]::IsNullOrWhiteSpace($Setting) -or $Setting -eq 'Default') {
        return $false
    }
    $Process.PriorityBoostEnabled = ($Setting -eq 'Enabled')
    return $true
}

function Set-CurrentThreadPrioritySetting {
    param([string]$Setting)
    if ([string]::IsNullOrWhiteSpace($Setting) -or $Setting -eq 'Default') {
        return $false
    }

    [Threading.Thread]::CurrentThread.Priority = $Setting
    return $true
}

function Set-ProcessIoPrioritySetting {
    param([Diagnostics.Process]$Process, [string]$Setting)
    if (-not $ioPriorityRaw.ContainsKey($Setting)) {
        return $false
    }

    $raw = [uint32]$ioPriorityRaw[$Setting]
    $status = [WorkloadEngineBenchmarkNative]::NtSetInformationProcess($Process.Handle, [uint32]$processIoPriorityClass, [ref]$raw, [uint32]4)
    if ($status -lt 0) {
        throw "NtSetInformationProcess failed with status $status"
    }
    return $true
}

function Set-ProcessGpuPrioritySetting {
    param([Diagnostics.Process]$Process, [string]$Setting)
    if (-not $gpuPriorityRaw.ContainsKey($Setting)) {
        return 'Skipped'
    }

    $raw = [int]$gpuPriorityRaw[$Setting]
    $status = [WorkloadEngineBenchmarkNative]::D3DKMTSetProcessSchedulingPriorityClass($Process.Handle, $raw)
    if ($status -ge 0) {
        return 'Applied'
    }
    if ($status -eq $statusInvalidParameter) {
        return 'Unavailable'
    }
    throw "D3DKMTSetProcessSchedulingPriorityClass failed with status $status"
}

function Apply-ForegroundAssistControls {
    param(
        [Diagnostics.Process]$Process,
        [pscustomobject]$AssistControls,
        [pscustomobject]$AssistStatus
    )

    try {
        if (Set-CurrentThreadPrioritySetting -Setting $AssistControls.foreground_thread_priority) {
            $AssistStatus.foreground_thread_priority_applied += 1
        }
    } catch {
        $AssistStatus.failed_actions += 1
    }
    try {
        if (Set-ProcessIoPrioritySetting -Process $Process -Setting $AssistControls.foreground_io_priority) {
            $AssistStatus.foreground_io_priority_applied += 1
        }
    } catch {
        $AssistStatus.failed_actions += 1
    }
    try {
        $gpuStatus = Set-ProcessGpuPrioritySetting -Process $Process -Setting $AssistControls.foreground_gpu_priority
        if ($gpuStatus -eq 'Applied') {
            $AssistStatus.foreground_gpu_priority_applied += 1
        } elseif ($gpuStatus -eq 'Unavailable') {
            $AssistStatus.foreground_gpu_priority_unavailable += 1
        }
    } catch {
        $AssistStatus.failed_actions += 1
    }
}

function Restore-ForegroundAssistControls {
    param(
        [Diagnostics.Process]$Process,
        [Threading.ThreadPriority]$OriginalThreadPriority,
        [pscustomobject]$AssistControls
    )

    try {
        [Threading.Thread]::CurrentThread.Priority = $OriginalThreadPriority
    } catch {
    }
    if ($ioPriorityRaw.ContainsKey($AssistControls.foreground_io_priority)) {
        try {
            [void](Set-ProcessIoPrioritySetting -Process $Process -Setting 'Normal')
        } catch {
        }
    }
    if ($gpuPriorityRaw.ContainsKey($AssistControls.foreground_gpu_priority)) {
        try {
            [void](Set-ProcessGpuPrioritySetting -Process $Process -Setting 'Normal')
        } catch {
        }
    }
}

function Apply-WorkerAssistControls {
    param(
        [Diagnostics.Process]$Process,
        [pscustomobject]$AssistControls,
        [pscustomobject]$AssistStatus
    )

    $AssistStatus.worker_processes += 1
    if ($AssistControls.background_priority_boost -ne 'Default') {
        try {
            if (Set-ProcessPriorityBoostSetting -Process $Process -Setting $AssistControls.background_priority_boost) {
                $AssistStatus.background_priority_boost_applied += 1
            }
        } catch {
            $AssistStatus.failed_actions += 1
        }
    }
    if ($AssistControls.thread_priority -ne 'Default') {
        try {
            $Process.Refresh()
            foreach ($thread in $Process.Threads) {
                try {
                    $thread.PriorityLevel = $AssistControls.thread_priority
                    $AssistStatus.thread_priority_threads_applied += 1
                } catch {
                    $AssistStatus.failed_actions += 1
                }
            }
        } catch {
            $AssistStatus.failed_actions += 1
        }
    }
    if ($memoryPriorityRaw.ContainsKey($AssistControls.memory_priority)) {
        try {
            $info = New-Object 'WorkloadEngineBenchmarkNative+MEMORY_PRIORITY_INFORMATION'
            $info.MemoryPriority = [uint32]$memoryPriorityRaw[$AssistControls.memory_priority]
            if ([WorkloadEngineBenchmarkNative]::SetProcessInformation($Process.Handle, $processMemoryPriorityClass, [ref]$info, [uint32]4)) {
                $AssistStatus.memory_priority_processes_applied += 1
            } else {
                $AssistStatus.failed_actions += 1
            }
        } catch {
            $AssistStatus.failed_actions += 1
        }
    }
    if ($ioPriorityRaw.ContainsKey($AssistControls.io_priority)) {
        try {
            if (Set-ProcessIoPrioritySetting -Process $Process -Setting $AssistControls.io_priority) {
                $AssistStatus.io_priority_processes_applied += 1
            }
        } catch {
            $AssistStatus.failed_actions += 1
        }
    }
    if ($gpuPriorityRaw.ContainsKey($AssistControls.gpu_priority)) {
        try {
            $gpuStatus = Set-ProcessGpuPrioritySetting -Process $Process -Setting $AssistControls.gpu_priority
            if ($gpuStatus -eq 'Applied') {
                $AssistStatus.gpu_priority_processes_applied += 1
            } elseif ($gpuStatus -eq 'Unavailable') {
                $AssistStatus.gpu_priority_unavailable += 1
            }
        } catch {
            $AssistStatus.failed_actions += 1
        }
    }
}

function Resolve-WinderustExePath {
    if (-not [string]::IsNullOrWhiteSpace($WinderustExePath)) {
        $resolved = Resolve-Path -LiteralPath $WinderustExePath -ErrorAction Stop
        return $resolved.Path
    }
    foreach ($candidate in @(
            (Join-Path $PSScriptRoot '..\target\release\winderust.exe'),
            (Join-Path $PSScriptRoot '..\target\debug\winderust.exe')
        )) {
        if (Test-Path -LiteralPath $candidate) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }
    throw 'Build Winderust first, or pass -WinderustExePath <path-to-winderust.exe>.'
}

function Stop-WinderustLaunchProcess {
    param([Diagnostics.Process]$Process)
    if ($null -eq $Process) {
        return
    }
    try {
        $Process.Refresh()
        if (-not $Process.HasExited) {
            [void]$Process.CloseMainWindow()
            if (-not $Process.WaitForExit(2000)) {
                $Process.Kill()
                [void]$Process.WaitForExit(3000)
            }
        }
    } catch {
    }
}

function Measure-WinderustLaunch {
    param([int]$Rounds, [string]$LaunchPriority, [System.Collections.Generic.List[double]]$PowerSamples)
    $samples = New-Object 'System.Collections.Generic.List[double]'
    $exePath = Resolve-WinderustExePath
    for ($round = 0; $round -lt $Rounds; $round++) {
        if (Get-Process -Name winderust -ErrorAction SilentlyContinue) {
            throw 'Close Winderust before running -ForegroundScenario WinderustLaunch.'
        }
        [GC]::Collect()
        $process = $null
        $inputIdle = $false
        $sw = [Diagnostics.Stopwatch]::StartNew()
        try {
            $process = Start-Process -FilePath $exePath -PassThru
            try {
                $process.PriorityClass = $LaunchPriority
            } catch {
            }
            while ($sw.Elapsed.TotalMilliseconds -lt 15000) {
                Start-Sleep -Milliseconds 50
                $process.Refresh()
                if ($process.HasExited) {
                    throw 'Winderust exited before it became ready.'
                }
                if ($process.MainWindowHandle -ne 0) {
                    break
                }
                if (-not $inputIdle) {
                    try {
                        $inputIdle = $process.WaitForInputIdle(1)
                    } catch {
                    }
                    if ($inputIdle) {
                        break
                    }
                }
            }
            $sw.Stop()
            $samples.Add($sw.Elapsed.TotalMilliseconds)
            Add-PowerSample -Samples $PowerSamples
        } finally {
            Stop-WinderustLaunchProcess -Process $process
            Start-Sleep -Milliseconds 500
        }
    }
    return $samples.ToArray()
}

function Measure-ForegroundWork {
    param([int]$Iterations, [int]$Rounds, [string]$LaunchPriority, [System.Collections.Generic.List[double]]$PowerSamples)
    if ($ForegroundScenario -eq 'WinderustLaunch') {
        return Measure-WinderustLaunch -Rounds $Rounds -LaunchPriority $LaunchPriority -PowerSamples $PowerSamples
    }
    if ($ForegroundScenario -eq 'IoLoop') {
        return Measure-ForegroundIoWork -Operations $IoOperations -Rounds $Rounds -PowerSamples $PowerSamples
    }
    if ($ForegroundScenario -eq 'MessageLoop') {
        return Measure-ForegroundMessageLoop -Ticks $MessageLoopTicks -IntervalMilliseconds $MessageLoopIntervalMilliseconds -Rounds $Rounds -PowerSamples $PowerSamples
    }
    $samples = New-Object 'System.Collections.Generic.List[double]'
    for ($round = 0; $round -lt $Rounds; $round++) {
        [GC]::Collect()
        $sw = [Diagnostics.Stopwatch]::StartNew()
        $acc = 0.0
        for ($i = 1; $i -le $Iterations; $i++) {
            $acc += [Math]::Sqrt($i)
        }
        $sw.Stop()
        $samples.Add($sw.Elapsed.TotalMilliseconds)
        Add-PowerSample -Samples $PowerSamples
        Start-Sleep -Milliseconds 150
    }
    return $samples.ToArray()
}

function Measure-ForegroundMessageLoop {
    param([int]$Ticks, [int]$IntervalMilliseconds, [int]$Rounds, [System.Collections.Generic.List[double]]$PowerSamples)
    if ($Ticks -lt 1) {
        throw '-MessageLoopTicks must be at least 1.'
    }
    if ($IntervalMilliseconds -lt 1) {
        throw '-MessageLoopIntervalMilliseconds must be at least 1.'
    }
    Add-Type -AssemblyName System.Windows.Forms
    Add-Type -AssemblyName System.Drawing

    $samples = New-Object 'System.Collections.Generic.List[double]'
    for ($round = 0; $round -lt $Rounds; $round++) {
        [GC]::Collect()
        $state = [pscustomobject]@{
            Count = 0
            LastMs = 0.0
            TotalDelayMs = 0.0
        }
        $form = New-Object System.Windows.Forms.Form
        $timer = New-Object System.Windows.Forms.Timer
        try {
            $form.ShowInTaskbar = $false
            $form.WindowState = [System.Windows.Forms.FormWindowState]::Minimized
            $form.Opacity = 0
            $form.Size = New-Object System.Drawing.Size(1, 1)
            $form.StartPosition = [System.Windows.Forms.FormStartPosition]::Manual
            $form.Location = New-Object System.Drawing.Point(-32000, -32000)
            $timer.Interval = $IntervalMilliseconds
            $sw = [Diagnostics.Stopwatch]::StartNew()
            $form.Add_Shown({
                $state.LastMs = $sw.Elapsed.TotalMilliseconds
                $timer.Start()
            })
            $timer.Add_Tick({
                $elapsed = $sw.Elapsed.TotalMilliseconds
                $delay = [Math]::Max(0.0, ($elapsed - $state.LastMs) - $IntervalMilliseconds)
                $state.Count += 1
                $state.TotalDelayMs += $delay
                $state.LastMs = $elapsed
                if ($state.Count -ge $Ticks) {
                    $timer.Stop()
                    $form.Close()
                }
            })
            [System.Windows.Forms.Application]::Run($form)
            $samples.Add($state.TotalDelayMs / [Math]::Max(1, $state.Count))
            Add-PowerSample -Samples $PowerSamples
        } finally {
            $timer.Dispose()
            $form.Dispose()
        }
        Start-Sleep -Milliseconds 150
    }
    return $samples.ToArray()
}

function Measure-ForegroundIoWork {
    param([int]$Operations, [int]$Rounds, [System.Collections.Generic.List[double]]$PowerSamples)
    $samples = New-Object 'System.Collections.Generic.List[double]'
    $buffer = New-Object byte[] 4096
    for ($index = 0; $index -lt $buffer.Length; $index++) {
        $buffer[$index] = [byte]($index % 251)
    }
    for ($round = 0; $round -lt $Rounds; $round++) {
        $path = Join-Path ([IO.Path]::GetTempPath()) ("winderust-workload-engine-bench-$PID-$round.bin")
        [GC]::Collect()
        $stream = $null
        $sw = [Diagnostics.Stopwatch]::StartNew()
        try {
            $stream = [IO.File]::Open($path, [IO.FileMode]::Create, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
            for ($operation = 0; $operation -lt $Operations; $operation++) {
                $stream.Write($buffer, 0, $buffer.Length)
            }
            $stream.Flush()
            $stream.Position = 0
            for ($operation = 0; $operation -lt $Operations; $operation++) {
                if ($stream.Read($buffer, 0, $buffer.Length) -le 0) {
                    break
                }
            }
            $sw.Stop()
            $samples.Add($sw.Elapsed.TotalMilliseconds)
            Add-PowerSample -Samples $PowerSamples
        } finally {
            if ($null -ne $stream) {
                $stream.Dispose()
            }
            Remove-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
        }
        Start-Sleep -Milliseconds 150
    }
    return $samples.ToArray()
}

function Start-CpuWorkers {
    param(
        [string[]]$Priorities,
        [int]$AffinitySelectedCount,
        [Int64]$AffinityMask,
        [int]$Seconds,
        [pscustomobject]$AssistControls,
        [pscustomobject]$AssistStatus
    )

    $code = @"
`$deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
`$acc = 0.0
while ([DateTime]::UtcNow -lt `$deadline) {
    for (`$i = 1; `$i -le 100000; `$i++) {
        `$acc += [Math]::Sqrt(`$i)
    }
}
"@
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($code))
    $processes = @()
    for ($worker = 0; $worker -lt $Priorities.Length; $worker++) {
        $process = Start-Process `
            -FilePath $powerShellPath `
            -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', $encoded) `
            -WindowStyle Hidden `
            -PassThru
        Start-Sleep -Milliseconds 80
        try {
            $process.PriorityClass = $Priorities[$worker]
        } catch {
        }
        if ($AffinitySelectedCount -gt 0 -and $worker -lt $AffinitySelectedCount -and $AffinityMask -gt 0) {
            try {
                $process.ProcessorAffinity = [IntPtr]$AffinityMask
            } catch {
            }
        }
        if ($null -ne $AssistControls -and $null -ne $AssistStatus) {
            Apply-WorkerAssistControls -Process $process -AssistControls $AssistControls -AssistStatus $AssistStatus
        }
        $processes += $process
    }
    return $processes
}

function Stop-CpuWorkers {
    param([object[]]$Processes)
    foreach ($process in $Processes) {
        if ($null -eq $process) {
            continue
        }
        try {
            $process.Refresh()
            if (-not $process.HasExited) {
                $process.Kill()
                [void]$process.WaitForExit(2000)
            }
        } catch {
        }
    }
}

function Get-WorkerCpuMilliseconds {
    param([object[]]$Processes)
    $total = 0.0
    foreach ($process in $Processes) {
        if ($null -eq $process) {
            continue
        }
        try {
            $process.Refresh()
            $total += $process.TotalProcessorTime.TotalMilliseconds
        } catch {
        }
    }
    return $total
}

function New-Priorities {
    param([string]$DefaultPriority, [int]$RestrainedCount, [string]$RestrainedPriority)
    $priorities = New-Object string[] $workerCount
    for ($worker = 0; $worker -lt $workerCount; $worker++) {
        if ($worker -lt $RestrainedCount) {
            $priorities[$worker] = $RestrainedPriority
        } else {
            $priorities[$worker] = $DefaultPriority
        }
    }
    return $priorities
}

function Test-ForegroundLaunchScenario {
    return $ForegroundScenario -eq 'WinderustLaunch'
}

function Run-LaunchGraceCase {
    param([string]$Name)
    return Run-Case `
        -Name $Name `
        -Model 'Launch grace: foreground launch boosted AboveNormal; background restraints deferred.' `
        -ForegroundPriority 'AboveNormal' `
        -Priorities (New-Priorities -DefaultPriority 'Normal' -RestrainedCount 0 -RestrainedPriority 'Normal') `
        -AffinitySelectedCount 0 `
        -AffinityMask 0 `
        -AssistControls (New-AssistControls)
}

function Get-Average {
    param([double[]]$Values)
    $sum = 0.0
    foreach ($value in $Values) {
        $sum += $value
    }
    return $sum / $Values.Length
}

function Summarize-Samples {
    param([string]$Name, [double[]]$Samples, [string]$Model)
    $sorted = [double[]]$Samples.Clone()
    [Array]::Sort($sorted)
    $avg = Get-Average $Samples
    $variance = 0.0
    foreach ($sample in $Samples) {
        $variance += [Math]::Pow($sample - $avg, 2)
    }
    $variance = $variance / $Samples.Length
    $medianIndex = [int][Math]::Floor(($Samples.Length - 1) * 0.50)
    $p95Index = [int][Math]::Floor(($Samples.Length - 1) * 0.95)
    $median = $sorted[$medianIndex]
    $p95 = $sorted[$p95Index]
    $summary = [ordered]@{
        name = $Name
        model = $Model
        foreground_scenario = $ForegroundScenario
        samples_ms = [double[]]$Samples
        avg_ms = [Math]::Round($avg, 2)
        median_ms = [Math]::Round($median, 2)
        p95_ms = [Math]::Round($p95, 2)
        min_ms = [Math]::Round($sorted[0], 2)
        max_ms = [Math]::Round($sorted[$sorted.Length - 1], 2)
        stddev_ms = [Math]::Round([Math]::Sqrt($variance), 2)
        range_ms = [Math]::Round($sorted[$sorted.Length - 1] - $sorted[0], 2)
        p95_minus_median_ms = [Math]::Round($p95 - $median, 2)
    }
    if ($ForegroundScenario -eq 'CpuLoop') {
        $summary.iterations_per_sec = [Math]::Round(($Iterations / ($avg / 1000.0)), 0)
    } elseif ($ForegroundScenario -eq 'IoLoop') {
        $summary.foreground_iops = [Math]::Round((($IoOperations * 2) / ($avg / 1000.0)), 0)
    } elseif ($ForegroundScenario -eq 'MessageLoop') {
        $summary.message_loop_avg_delay_ms = [Math]::Round($avg, 2)
        $summary.message_loop_p95_delay_ms = [Math]::Round($p95, 2)
        $summary.message_loop_ticks = $MessageLoopTicks
        $summary.message_loop_interval_ms = $MessageLoopIntervalMilliseconds
    } else {
        $summary.launches_per_sec = [Math]::Round((1000.0 / $avg), 2)
    }
    return [pscustomobject]$summary
}

function Get-ImprovementPercent {
    param([double]$OffValue, [double]$CaseValue)
    if ($OffValue -le 0.0) {
        return 0.0
    }
    return [Math]::Round((($OffValue - $CaseValue) / $OffValue) * 100.0, 1)
}

function Get-DeltaMilliseconds {
    param([double]$OffValue, [double]$CaseValue)
    return [Math]::Round($CaseValue - $OffValue, 2)
}

function Format-DeltaWithPercent {
    param([double]$DeltaMs, [double]$Percent)
    $deltaSign = if ($DeltaMs -gt 0.0) { '+' } else { '' }
    $percentSign = if ($Percent -gt 0.0) { '+' } else { '' }
    return ('{0}{1:N2} ms ({2}{3:N1}%)' -f $deltaSign, $DeltaMs, $percentSign, $Percent)
}

function Format-CaseWithBaseline {
    param([double]$CaseMs, [double]$OffMs, [double]$DeltaMs, [double]$Percent)
    $deltaSign = if ($DeltaMs -gt 0.0) { '+' } else { '' }
    $percentSign = if ($Percent -gt 0.0) { '+' } else { '' }
    return ('{0:N2} ms vs {1:N2} ms paired Off ({2}{3:N2} ms, {4}{5:N1}%)' -f $CaseMs, $OffMs, $deltaSign, $DeltaMs, $percentSign, $Percent)
}

function Get-ForegroundLatencyBaselinePercent {
    param([double]$BaselineMs, [double]$CaseMs)
    if ($BaselineMs -le 0.0 -or $CaseMs -le 0.0) {
        return 0.0
    }
    return [Math]::Round(($BaselineMs / $CaseMs) * 100.0, 1)
}

function Get-AverageProperty {
    param($Items, [string]$Name)
    $values = @(
        $Items | ForEach-Object {
            $property = $_.PSObject.Properties[$Name]
            if ($null -ne $property) {
                [double]$property.Value
            }
        }
    )
    if ($values.Count -eq 0) {
        return $null
    }
    return [Math]::Round((Get-Average $values), 2)
}

function Run-Case {
    param(
        [string]$Name,
        [string]$Model,
        [string]$ForegroundPriority,
        [string[]]$Priorities,
        [int]$AffinitySelectedCount,
        [Int64]$AffinityMask,
        [pscustomobject]$AssistControls
    )

    if ($null -eq $AssistControls) {
        $AssistControls = New-AssistControls
    }
    $assistStatus = New-AssistStatus
    $currentProcess = [Diagnostics.Process]::GetCurrentProcess()
    $originalPriority = $currentProcess.PriorityClass
    $originalThreadPriority = [Threading.Thread]::CurrentThread.Priority
    $originalPriorityBoost = $null
    try {
        $originalPriorityBoost = $currentProcess.PriorityBoostEnabled
    } catch {
    }
    $processes = @()
    $powerSamples = New-Object 'System.Collections.Generic.List[double]'
    try {
        $currentProcess.PriorityClass = $ForegroundPriority
        try {
            if (Set-ProcessPriorityBoostSetting -Process $currentProcess -Setting $AssistControls.foreground_priority_boost) {
                $assistStatus.foreground_priority_boost_applied += 1
            }
        } catch {
            $assistStatus.failed_actions += 1
        }
        Apply-ForegroundAssistControls `
            -Process $currentProcess `
            -AssistControls $AssistControls `
            -AssistStatus $assistStatus
        $processes = Start-CpuWorkers `
            -Priorities $Priorities `
            -AffinitySelectedCount $AffinitySelectedCount `
            -AffinityMask $AffinityMask `
            -Seconds $WorkerSeconds `
            -AssistControls $AssistControls `
            -AssistStatus $assistStatus
        Start-Sleep -Seconds 2
        $workerCpuBeforeMs = Get-WorkerCpuMilliseconds $processes
        $measurementWindow = [Diagnostics.Stopwatch]::StartNew()
        $samples = Measure-ForegroundWork -Iterations $Iterations -Rounds $Rounds -LaunchPriority $ForegroundPriority -PowerSamples $powerSamples
        $scoreBenchmark = Measure-ScoreBenchmarks -PowerSamples $powerSamples
        $measurementWindow.Stop()
        $workerCpuAfterMs = Get-WorkerCpuMilliseconds $processes
        $summary = Summarize-Samples -Name $Name -Samples $samples -Model $Model
        Add-PowerSummary -Summary $summary -Samples ([double[]]$powerSamples.ToArray())
        $workerCpuDeltaMs = [Math]::Max(0.0, $workerCpuAfterMs - $workerCpuBeforeMs)
        $capacity = if ($measurementWindow.Elapsed.TotalMilliseconds -gt 0.0) {
            ($workerCpuDeltaMs / ($measurementWindow.Elapsed.TotalMilliseconds * $logicalProcessors)) * 100.0
        } else {
            0.0
        }
        $summary | Add-Member -NotePropertyName measurement_window_ms -NotePropertyValue ([Math]::Round($measurementWindow.Elapsed.TotalMilliseconds, 2))
        $summary | Add-Member -NotePropertyName background_cpu_ms -NotePropertyValue ([Math]::Round($workerCpuDeltaMs, 2))
        $summary | Add-Member -NotePropertyName background_throughput_percent -NotePropertyValue ([Math]::Round($capacity, 1))
        $summary | Add-Member -NotePropertyName score_benchmark -NotePropertyValue $scoreBenchmark
        $summary | Add-Member -NotePropertyName assist_controls -NotePropertyValue $AssistControls
        $summary | Add-Member -NotePropertyName assist_status -NotePropertyValue $assistStatus
        return $summary
    } finally {
        Stop-CpuWorkers -Processes $processes
        try {
            $currentProcess.PriorityClass = $originalPriority
        } catch {
        }
        if ($null -ne $originalPriorityBoost) {
            try {
                $currentProcess.PriorityBoostEnabled = $originalPriorityBoost
            } catch {
            }
        }
        Restore-ForegroundAssistControls `
            -Process $currentProcess `
            -OriginalThreadPriority $originalThreadPriority `
            -AssistControls $AssistControls
        Start-Sleep -Seconds 1
    }
}

function Run-NamedCase {
    param([string]$Name)
    switch ($Name) {
        'off' {
            return Run-Case `
                -Name 'off' `
                -Model 'Background Normal; foreground Normal.' `
                -ForegroundPriority 'Normal' `
                -Priorities (New-Priorities -DefaultPriority 'Normal' -RestrainedCount 0 -RestrainedPriority 'Normal') `
                -AffinitySelectedCount 0 `
                -AffinityMask 0 `
                -AssistControls (New-AssistControls)
        }
        'powersave' {
            return Invoke-WithProcessorPolicy -Preset Saver -ScriptBlock {
                Run-Case `
                    -Name 'powersave' `
                    -Model 'Powersave: strict processor Saver policy plus Low Impact scheduling for maximum battery life.' `
                    -ForegroundPriority 'Normal' `
                    -Priorities (New-Priorities -DefaultPriority 'Idle' -RestrainedCount $lowImpactTargetCount -RestrainedPriority 'Idle') `
                    -AffinitySelectedCount ([Math]::Min(12, $workerCount)) `
                    -AffinityMask $lowImpactMask `
                    -AssistControls (New-AssistControls -BackgroundPriorityBoost 'Disabled' -ThreadPriority 'BelowNormal' -MemoryPriority 'Low' -IoPriority 'Low' -GpuPriority 'BelowNormal')
            }
        }
        'balanced' {
            if (Test-ForegroundLaunchScenario) {
                return Invoke-WithProcessorPolicy -Preset Balanced -ScriptBlock { Run-LaunchGraceCase -Name 'balanced' }
            }
            return Invoke-WithProcessorPolicy -Preset Balanced -ScriptBlock {
                Run-Case `
                    -Name 'balanced' `
                    -Model 'Balanced: processor Balanced policy plus Low Impact scheduling; all background workers Idle; adaptive CPU share; foreground Auto boost modeled as AboveNormal for this low foreground CPU synthetic case; background threads BelowNormal; priority boost disabled.' `
                    -ForegroundPriority 'AboveNormal' `
                    -Priorities (New-Priorities -DefaultPriority 'Idle' -RestrainedCount $lowImpactTargetCount -RestrainedPriority 'Idle') `
                    -AffinitySelectedCount ([Math]::Min(12, $workerCount)) `
                    -AffinityMask $lowImpactMask `
                    -AssistControls (New-AssistControls -ForegroundPriorityBoost 'Enabled' -BackgroundPriorityBoost 'Disabled' -ThreadPriority 'BelowNormal' -MemoryPriority 'Low' -IoPriority 'Low' -GpuPriority 'BelowNormal')
            }
        }
        'performance' {
            return Invoke-WithProcessorPolicy -Preset Performance -ScriptBlock {
                Run-Case `
                    -Name 'performance' `
                    -Model 'Performance: high processor policy (min 25, max 100, efficient aggressive boost) plus Foreground First CPU-pressure scheduling.' `
                    -ForegroundPriority 'Normal' `
                    -Priorities (New-Priorities -DefaultPriority 'Normal' -RestrainedCount $workerCount -RestrainedPriority 'Idle') `
                    -AffinitySelectedCount ([Math]::Min(12, $workerCount)) `
                    -AffinityMask $foregroundFirstMask `
                    -AssistControls (New-AssistControls -ForegroundPriorityBoost 'Enabled' -BackgroundPriorityBoost 'Disabled' -ThreadPriority 'BelowNormal' -MemoryPriority 'Low' -IoPriority 'VeryLow' -GpuPriority 'BelowNormal')
            }
        }
        'speed' {
            return Invoke-WithProcessorPolicy -Preset Speed -ScriptBlock {
                Run-Case `
                    -Name 'speed' `
                    -Model 'Speed: aggressive processor policy (parking 100, min 25, max 100, aggressive boost) plus Max Foreground CPU-pressure scheduling.' `
                    -ForegroundPriority 'AboveNormal' `
                    -Priorities (New-Priorities -DefaultPriority 'Normal' -RestrainedCount $workerCount -RestrainedPriority 'Idle') `
                    -AffinitySelectedCount ([Math]::Min(12, $workerCount)) `
                    -AffinityMask $maxForegroundMask `
                    -AssistControls (New-AssistControls -ForegroundPriorityBoost 'Enabled' -ForegroundThreadPriority 'Highest' -ForegroundIoPriority 'High' -ForegroundGpuPriority 'High' -BackgroundPriorityBoost 'Disabled' -ThreadPriority 'Idle' -MemoryPriority 'VeryLow' -IoPriority 'VeryLow' -GpuPriority 'Idle')
            }
        }
    }
}

function New-Comparison {
    param($Off, $Case)
    $backgroundRetained = if ($Off.background_throughput_percent -gt 0.0) {
        ($Case.background_throughput_percent / $Off.background_throughput_percent) * 100.0
    } else {
        0.0
    }
    $backgroundLatencyMultiplier = $null
    $backgroundLatencySlowdown = $null
    if ($Off.background_throughput_percent -gt 0.0 -and $Case.background_throughput_percent -gt 0.0) {
        $backgroundLatencyMultiplier = [Math]::Round(($Off.background_throughput_percent / $Case.background_throughput_percent), 2)
        $backgroundLatencySlowdown = [Math]::Round((($Off.background_throughput_percent / $Case.background_throughput_percent) - 1.0) * 100.0, 1)
    }
    $scoreComparison = Get-ScoreBenchmarkComparison -Off $Off -Case $Case
    $avgDelta = Get-DeltaMilliseconds -OffValue $Off.avg_ms -CaseValue $Case.avg_ms
    $medianDelta = Get-DeltaMilliseconds -OffValue $Off.median_ms -CaseValue $Case.median_ms
    $p95Delta = Get-DeltaMilliseconds -OffValue $Off.p95_ms -CaseValue $Case.p95_ms
    $jitterDelta = Get-DeltaMilliseconds -OffValue $Off.stddev_ms -CaseValue $Case.stddev_ms
    $avgPercent = Get-ImprovementPercent -OffValue $Off.avg_ms -CaseValue $Case.avg_ms
    $medianPercent = Get-ImprovementPercent -OffValue $Off.median_ms -CaseValue $Case.median_ms
    $p95Percent = Get-ImprovementPercent -OffValue $Off.p95_ms -CaseValue $Case.p95_ms
    $jitterPercent = Get-ImprovementPercent -OffValue $Off.stddev_ms -CaseValue $Case.stddev_ms
    $powerOff = $Off.package_power_median_w
    $powerCase = $Case.package_power_median_w
    $powerDelta = $null
    $powerSaving = $null
    if ($null -ne $powerOff -and [double]$powerOff -gt 0.0 -and $null -ne $powerCase) {
        $powerDelta = [Math]::Round(([double]$powerCase - [double]$powerOff), 3)
        $powerSaving = [Math]::Round((([double]$powerOff - [double]$powerCase) / [double]$powerOff) * 100.0, 1)
    }
    [pscustomobject]@{
        name = $Case.name
        avg_vs_off = Format-CaseWithBaseline -CaseMs $Case.avg_ms -OffMs $Off.avg_ms -DeltaMs $avgDelta -Percent $avgPercent
        avg_off_ms = $Off.avg_ms
        avg_case_ms = $Case.avg_ms
        avg_delta_vs_off = Format-DeltaWithPercent -DeltaMs $avgDelta -Percent $avgPercent
        avg_delta_ms_vs_off = $avgDelta
        avg_improvement_percent_vs_off = $avgPercent
        median_vs_off = Format-CaseWithBaseline -CaseMs $Case.median_ms -OffMs $Off.median_ms -DeltaMs $medianDelta -Percent $medianPercent
        median_off_ms = $Off.median_ms
        median_case_ms = $Case.median_ms
        median_delta_vs_off = Format-DeltaWithPercent -DeltaMs $medianDelta -Percent $medianPercent
        median_delta_ms_vs_off = $medianDelta
        median_improvement_percent_vs_off = $medianPercent
        p95_vs_off = Format-CaseWithBaseline -CaseMs $Case.p95_ms -OffMs $Off.p95_ms -DeltaMs $p95Delta -Percent $p95Percent
        p95_off_ms = $Off.p95_ms
        p95_case_ms = $Case.p95_ms
        p95_delta_vs_off = Format-DeltaWithPercent -DeltaMs $p95Delta -Percent $p95Percent
        p95_delta_ms_vs_off = $p95Delta
        p95_improvement_percent_vs_off = $p95Percent
        jitter_vs_off = Format-CaseWithBaseline -CaseMs $Case.stddev_ms -OffMs $Off.stddev_ms -DeltaMs $jitterDelta -Percent $jitterPercent
        jitter_off_ms = $Off.stddev_ms
        jitter_case_ms = $Case.stddev_ms
        jitter_delta_vs_off = Format-DeltaWithPercent -DeltaMs $jitterDelta -Percent $jitterPercent
        jitter_delta_ms_vs_off = $jitterDelta
        jitter_improvement_percent_vs_off = $jitterPercent
        background_throughput_percent = $Case.background_throughput_percent
        background_throughput_retained_percent_vs_off = [Math]::Round($backgroundRetained, 1)
        background_latency_multiplier_vs_off = $backgroundLatencyMultiplier
        background_latency_slowdown_percent_vs_off = $backgroundLatencySlowdown
        score_benchmark_vs_off = $scoreComparison
        package_power_off_median_w = $powerOff
        package_power_case_median_w = $powerCase
        package_power_delta_w_vs_off = $powerDelta
        package_power_saving_percent_vs_off = $powerSaving
    }
}

function Run-Pass {
    param([int]$Pass)
    $presetOrders = @(
        @('powersave', 'balanced', 'performance', 'speed'),
        @('speed', 'performance', 'balanced', 'powersave'),
        @('balanced', 'powersave', 'speed', 'performance'),
        @('performance', 'speed', 'powersave', 'balanced')
    )
    $presetOrder = $presetOrders[($Pass - 1) % $presetOrders.Count]
    $currentProcess = [Diagnostics.Process]::GetCurrentProcess()
    $originalPriority = $currentProcess.PriorityClass
    try {
        $currentProcess.PriorityClass = 'Normal'
        $baselinePowerSamples = New-Object 'System.Collections.Generic.List[double]'
        $baselineSamples = Measure-ForegroundWork -Iterations $Iterations -Rounds $Rounds -LaunchPriority 'Normal' -PowerSamples $baselinePowerSamples
        $baseline = Summarize-Samples `
            -Name 'baseline_no_background_load' `
            -Samples $baselineSamples `
            -Model 'No generated background load.'
        $baselineScore = Measure-ScoreBenchmarks -PowerSamples $baselinePowerSamples
        Add-PowerSummary -Summary $baseline -Samples ([double[]]$baselinePowerSamples.ToArray())
        $baseline | Add-Member -NotePropertyName score_benchmark -NotePropertyValue $baselineScore
    } finally {
        try {
            $currentProcess.PriorityClass = $originalPriority
        } catch {
        }
    }

    $pairs = @()
    $cases = @()
    $comparisons = @()
    for ($index = 0; $index -lt $presetOrder.Count; $index++) {
        $preset = $presetOrder[$index]
        if ((($Pass + $index) % 2) -eq 0) {
            $order = @('off', $preset)
        } else {
            $order = @($preset, 'off')
        }
        $results = @{}
        foreach ($name in $order) {
            $results[$name] = Run-NamedCase $name
        }
        $case = $results[$preset]
        $comparison = New-Comparison -Off $results['off'] -Case $case
        $cases += $case
        $comparisons += $comparison
        $pairs += [pscustomobject]@{
            preset = $preset
            order = $order
            off = $results['off']
            case = $case
            comparison_vs_off = $comparison
        }
    }
    return [pscustomobject]@{
        pass = $Pass
        preset_order = $presetOrder
        baseline = $baseline
        pairs = $pairs
        presets = $cases
        comparisons_vs_off = $comparisons
    }
}

function Summarize-Method {
    param($Runs)
    $baselineRows = @($Runs | ForEach-Object { $_.baseline })
    $baselineAvg = Get-AverageProperty -Items $baselineRows -Name 'avg_ms'
    $baselineMedian = Get-AverageProperty -Items $baselineRows -Name 'median_ms'
    $baselineP95 = Get-AverageProperty -Items $baselineRows -Name 'p95_ms'
    $offRows = @($Runs | ForEach-Object { $_.pairs } | ForEach-Object { $_.off })
    $offAvg = Get-AverageProperty -Items $offRows -Name 'avg_ms'
    $offMedian = Get-AverageProperty -Items $offRows -Name 'median_ms'
    $offP95 = Get-AverageProperty -Items $offRows -Name 'p95_ms'
    [pscustomobject]@{
        name = 'off'
        off_sample_count = $offRows.Count
        foreground_latency_avg_ms = $offAvg
        foreground_latency_median_avg_ms = $offMedian
        foreground_latency_p95_avg_ms = $offP95
        foreground_latency_baseline_percent = Get-ForegroundLatencyBaselinePercent -BaselineMs $baselineAvg -CaseMs $offAvg
        no_background_avg_ms = $baselineAvg
        no_background_median_ms = $baselineMedian
        no_background_p95_ms = $baselineP95
        foreground_iterations_per_sec_avg = Get-AverageProperty -Items $offRows -Name 'iterations_per_sec'
        foreground_iops_avg = Get-AverageProperty -Items $offRows -Name 'foreground_iops'
        message_loop_avg_delay_ms_avg = Get-AverageProperty -Items $offRows -Name 'message_loop_avg_delay_ms'
        message_loop_p95_delay_ms_avg = Get-AverageProperty -Items $offRows -Name 'message_loop_p95_delay_ms'
        package_power_median_w_avg = Get-AverageProperty -Items $offRows -Name 'package_power_median_w'
        package_power_saving_avg_percent_vs_off = 0.0
        background_throughput_retained_avg_percent = 100.0
        background_throughput_retained_min_percent = 100.0
        background_latency_multiplier_avg_vs_off = 1.0
        background_latency_slowdown_avg_percent_vs_off = 0.0
        repeat_passes_won = 'baseline'
        repeat_pass_win_count = $null
        repeat_pass_count = $null
        repeat_pass_win_rate_percent = $null
    }
    foreach ($name in @('powersave', 'balanced', 'performance', 'speed')) {
        $comparisons = @()
        $caseRows = @()
        foreach ($run in $Runs) {
            $comparisons += @($run.comparisons_vs_off | Where-Object { $_.name -eq $name })
            $caseRows += @($run.presets | Where-Object { $_.name -eq $name })
        }
        $medianValues = @($comparisons | ForEach-Object { [double]$_.median_improvement_percent_vs_off })
        $p95Values = @($comparisons | ForEach-Object { [double]$_.p95_improvement_percent_vs_off })
        $medianOffValues = @($comparisons | ForEach-Object { [double]$_.median_off_ms })
        $medianCaseValues = @($comparisons | ForEach-Object { [double]$_.median_case_ms })
        $p95OffValues = @($comparisons | ForEach-Object { [double]$_.p95_off_ms })
        $p95CaseValues = @($comparisons | ForEach-Object { [double]$_.p95_case_ms })
        $medianDeltaValues = @($comparisons | ForEach-Object { [double]$_.median_delta_ms_vs_off })
        $p95DeltaValues = @($comparisons | ForEach-Object { [double]$_.p95_delta_ms_vs_off })
        $backgroundRetainedValues = @($comparisons | ForEach-Object { [double]$_.background_throughput_retained_percent_vs_off })
        $backgroundLatencyMultiplierValues = @($comparisons | ForEach-Object {
            if ($null -ne $_.background_latency_multiplier_vs_off) {
                [double]$_.background_latency_multiplier_vs_off
            }
        })
        $backgroundLatencySlowdownValues = @($comparisons | ForEach-Object {
            if ($null -ne $_.background_latency_slowdown_percent_vs_off) {
                [double]$_.background_latency_slowdown_percent_vs_off
            }
        })
        $powerSavingValues = @($comparisons | ForEach-Object {
            if ($null -ne $_.package_power_saving_percent_vs_off) {
                [double]$_.package_power_saving_percent_vs_off
            }
        })
        $medianAvg = Get-Average $medianValues
        $p95Avg = Get-Average $p95Values
        $medianOffAvg = Get-Average $medianOffValues
        $medianCaseAvg = Get-Average $medianCaseValues
        $p95OffAvg = Get-Average $p95OffValues
        $p95CaseAvg = Get-Average $p95CaseValues
        $medianDeltaAvg = Get-Average $medianDeltaValues
        $p95DeltaAvg = Get-Average $p95DeltaValues
        $backgroundRetainedAvg = Get-Average $backgroundRetainedValues
        $repeatWins = @(
            $comparisons | Where-Object {
                $_.median_improvement_percent_vs_off -ge 3.0 -and
                $_.p95_improvement_percent_vs_off -ge 3.0
            }
        ).Count
        $repeatWinRate = [Math]::Round(($repeatWins / $comparisons.Count) * 100.0, 1)
        [pscustomobject]@{
            name = $name
            passes = $comparisons.Count
            foreground_latency_avg_ms = Get-AverageProperty -Items $caseRows -Name 'avg_ms'
            foreground_latency_median_avg_ms = [Math]::Round($medianCaseAvg, 2)
            foreground_latency_p95_avg_ms = [Math]::Round($p95CaseAvg, 2)
            foreground_latency_baseline_percent = Get-ForegroundLatencyBaselinePercent -BaselineMs $baselineAvg -CaseMs (Get-AverageProperty -Items $caseRows -Name 'avg_ms')
            foreground_iterations_per_sec_avg = Get-AverageProperty -Items $caseRows -Name 'iterations_per_sec'
            foreground_iops_avg = Get-AverageProperty -Items $caseRows -Name 'foreground_iops'
            message_loop_avg_delay_ms_avg = Get-AverageProperty -Items $caseRows -Name 'message_loop_avg_delay_ms'
            message_loop_p95_delay_ms_avg = Get-AverageProperty -Items $caseRows -Name 'message_loop_p95_delay_ms'
            package_power_median_w_avg = Get-AverageProperty -Items $caseRows -Name 'package_power_median_w'
            package_power_saving_avg_percent_vs_off = if ($powerSavingValues.Count -gt 0) { [Math]::Round((Get-Average $powerSavingValues), 1) } else { $null }
            median_vs_off_avg = Format-CaseWithBaseline -CaseMs $medianCaseAvg -OffMs $medianOffAvg -DeltaMs $medianDeltaAvg -Percent $medianAvg
            median_off_avg_ms = [Math]::Round($medianOffAvg, 2)
            median_case_avg_ms = [Math]::Round($medianCaseAvg, 2)
            median_improvement_avg = Format-DeltaWithPercent -DeltaMs $medianDeltaAvg -Percent $medianAvg
            median_delta_avg_ms_vs_off = [Math]::Round($medianDeltaAvg, 2)
            median_improvement_avg_percent = [Math]::Round($medianAvg, 1)
            median_improvement_min_percent = [Math]::Round(($medianValues | Measure-Object -Minimum).Minimum, 1)
            p95_vs_off_avg = Format-CaseWithBaseline -CaseMs $p95CaseAvg -OffMs $p95OffAvg -DeltaMs $p95DeltaAvg -Percent $p95Avg
            p95_off_avg_ms = [Math]::Round($p95OffAvg, 2)
            p95_case_avg_ms = [Math]::Round($p95CaseAvg, 2)
            p95_improvement_avg = Format-DeltaWithPercent -DeltaMs $p95DeltaAvg -Percent $p95Avg
            p95_delta_avg_ms_vs_off = [Math]::Round($p95DeltaAvg, 2)
            p95_improvement_avg_percent = [Math]::Round($p95Avg, 1)
            p95_improvement_min_percent = [Math]::Round(($p95Values | Measure-Object -Minimum).Minimum, 1)
            background_throughput_retained_avg_percent = [Math]::Round($backgroundRetainedAvg, 1)
            background_throughput_retained_min_percent = [Math]::Round(($backgroundRetainedValues | Measure-Object -Minimum).Minimum, 1)
            background_latency_multiplier_avg_vs_off = if ($backgroundLatencyMultiplierValues.Count -gt 0) { [Math]::Round((Get-Average $backgroundLatencyMultiplierValues), 2) } else { $null }
            background_latency_slowdown_avg_percent_vs_off = if ($backgroundLatencySlowdownValues.Count -gt 0) { [Math]::Round((Get-Average $backgroundLatencySlowdownValues), 1) } else { $null }
            repeat_passes_won = "$repeatWins/$($comparisons.Count)"
            repeat_pass_win_count = $repeatWins
            repeat_pass_count = $comparisons.Count
            repeat_pass_win_rate_percent = $repeatWinRate
        }
    }
}

$assistCoverage = [pscustomobject][ordered]@{
    process_priority = 'applied to generated background workers'
    foreground_process_priority = 'applied to the benchmark process'
    affinity = 'applied as hard affinity for Max Foreground, and for adaptive presets on standard/all-P CPUs to approximate runtime Soft CPU Sets'
    foreground_priority_boost = 'applied to the benchmark process when preset enables it'
    foreground_thread_priority = 'applied to the benchmark thread when preset enables it'
    foreground_io_priority = 'applied to the benchmark process when preset enables it; CPU loop has minimal I/O'
    foreground_gpu_priority = 'attempted on the benchmark process when preset enables it; CPU loop may report no GPU context'
    background_priority_boost = 'applied to generated background workers when preset enables it'
    thread_priority = 'applied to existing generated worker threads when preset enables it'
    memory_priority = 'applied to generated background workers when preset enables it'
    io_priority = 'applied to generated background workers when preset enables it; CPU loop has minimal I/O'
    gpu_priority = 'attempted on generated workers when preset enables it; CPU workers may report no GPU context'
    foreground_detection = 'modeled by treating the benchmark process as foreground; the app automation loop is not launched'
}

Initialize-PowerCounter

$runs = @()
for ($pass = 1; $pass -le $Passes; $pass++) {
    $runs += Run-Pass $pass
}

[pscustomobject]@{
    note = 'Synthetic local scheduler benchmark. Use this to validate tuning direction, not universal defaults.'
    cpu_name = Get-CpuName
    logical_processors = $logicalProcessors
    worker_count = $workerCount
    passes = $Passes
    rounds = $Rounds
    foreground_scenario = $ForegroundScenario
    score_benchmark_enabled = -not $SkipScoreBenchmark
    score_iterations = $ScoreIterations
    score_data_kb = $ScoreDataKb
    score_rounds = $ScoreRounds
    topology_class = if ($hasHybridTopology) { 'hybrid_or_asymmetric' } else { 'standard_all_p' }
    low_impact_affinity_limited_processors = $lowImpactCoreCount
    foreground_iterations_per_round = $Iterations
    foreground_io_operations_per_round = $IoOperations * 2
    foreground_message_loop_ticks_per_round = $MessageLoopTicks
    foreground_message_loop_interval_ms = $MessageLoopIntervalMilliseconds
    power_counter_path = $script:resolvedPowerCounterPath
    power_counter_watts_scale = $script:powerWattsScale
    foreground_first_affinity_limited_processors = $foregroundFirstCoreCount
    max_foreground_affinity_limited_processors = $maxForegroundCoreCount
    assist_coverage = $assistCoverage
    methodology_gate = 'Trust a local tuning direction only when median and p95 both improve by at least 3% in at least two of three passes.'
    runs = $runs
    method_summary = @(Summarize-Method $runs)
} | ConvertTo-Json -Depth 8
