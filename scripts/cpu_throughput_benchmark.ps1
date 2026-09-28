#Requires -Version 5.1
param(
    [Parameter(Mandatory = $true)][string]$Label,
    [ValidateRange(1, 100)][int]$Rounds = 5,
    [ValidateRange(1, 120)][int]$Seconds = 10,
    [ValidateRange(1, 60)][int]$WarmupSeconds = 5,
    [ValidateRange(0, 120)][int]$CooldownSeconds = 10,
    [ValidateRange(1, 64)][int]$Threads = [Environment]::ProcessorCount,
    [string]$OutputPath = '',
    [switch]$NoPrompt,
    [switch]$KeepWindowOpen,
    [UInt64]$SingleThreadAffinityMask = 0,
    [int[]]$MonitorProcessIds = @(),
    [switch]$Telemetry,
    [switch]$SelfTest
)

$ErrorActionPreference = 'Stop'
if ($PSEdition -ne 'Desktop') { throw 'Run with Windows PowerShell 5.1 (powershell.exe), not pwsh.' }
if (-not [Environment]::Is64BitProcess) { throw 'Use 64-bit Windows PowerShell.' }
if ([Environment]::ProcessorCount -gt 64) { throw 'This harness does not support multiple processor groups.' }
if (!$OutputPath) { $OutputPath = Join-Path $PWD ('cpu-throughput-{0}.csv' -f (Get-Date -Format 'yyyyMMdd-HHmmss')) }
if (Test-Path -LiteralPath $OutputPath) { throw "Output already exists: $OutputPath" }

# Compiled arithmetic keeps PowerShell interpretation and allocation out of the timed loop.
# Dedicated threads are shared by warm-up and measurement so reconciliation can see them.
if (-not ('CpuThroughputBench' -as [type])) {
    $compiler = New-Object System.CodeDom.Compiler.CompilerParameters
    $compiler.CompilerOptions = '/optimize+'
    [void]$compiler.ReferencedAssemblies.Add('System.dll')
    [void]$compiler.ReferencedAssemblies.Add('System.Windows.Forms.dll')
    [void]$compiler.ReferencedAssemblies.Add('System.Drawing.dll')
    Add-Type -CompilerParameters $compiler -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;
using System.Windows.Forms;
using System.Collections.Generic;
using System.Globalization;

public sealed class CpuThroughputResult {
    public double Seconds, BlocksPerSecond;
    public long Blocks;
    public bool FocusMaintained = true;
    public string Priority, WorkerPriorities;
    public string ProcessorMasks, TelemetryNotes;
    public double RuntimeCpuMilliseconds;
    public List<string> Telemetry = new List<string>();
}

public static class CpuThroughputBench {
    static Form Window;
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("kernel32.dll")] static extern uint GetCurrentProcessorNumber();
    public static void Pump() { Application.DoEvents(); }
    public static void Close() { if (Window != null) { Window.Dispose(); Window = null; } }
    public static void Wait(int seconds) {
        var clock = Stopwatch.StartNew();
        while (clock.Elapsed.TotalSeconds < seconds) {
            Pump();
            if (Window != null && Window.IsDisposed) throw new OperationCanceledException();
            Thread.Sleep(20);
        }
    }
    static ulong Block(ulong x) {
        unchecked {
            for (int i = 0; i < 65536; i++) {
                x ^= x << 13; x ^= x >> 7; x ^= x << 17;
            }
        }
        return x;
    }
    public static void Check() {
        if (Block(1) != 3937720911837777011UL) throw new Exception("Kernel verification failed.");
    }
    public static CpuThroughputResult Run(int count, int seconds, int warmup, string title, int[] monitorIds = null, bool telemetry = false, bool waitForFocus = true) {
        var result = new CpuThroughputResult();
        var counters = new List<PerformanceCounter>();
        var monitors = new List<Process>();
        try {
        if (monitorIds != null) foreach (int id in monitorIds) {
            var process = Process.GetProcessById(id);
            monitors.Add(process);
            var handle = process.Handle; // Retain this process instance for both CPU-time reads.
        }
        if (telemetry) foreach (string category in new [] { "Processor Information", "Energy Meter", "Thermal Zone Information" }) {
            try {
                foreach (string instance in new PerformanceCounterCategory(category).GetInstanceNames()) {
                    string[] names;
                    if (category == "Processor Information") {
                        if (instance == "_Total") names = new [] { "Actual Frequency", "% Processor Performance", "% Performance Limit", "Performance Limit Flags" };
                        else if (instance.StartsWith("0,") && instance != "0,_Total") names = new [] { "Actual Frequency" };
                        else continue;
                    } else if (category == "Energy Meter") {
                        if (!instance.EndsWith("_PKG")) continue;
                        names = new [] { "Power" };
                    } else names = new [] { "Temperature", "Throttle Reasons" };
                    foreach (string name in names) {
                        var counter = new PerformanceCounter(category, name, instance, true);
                        try { counter.NextValue(); counters.Add(counter); }
                        catch (Exception ex) { counter.Dispose(); result.TelemetryNotes += category + "/" + name + ": " + ex.Message + "; "; }
                    }
                }
            } catch (Exception ex) { result.TelemetryNotes += category + ": " + ex.Message + "; "; }
        }
        var workers = new Thread[count];
        var blocks = new long[count];
        var checksums = new ulong[count];
        var errors = new Exception[count];
        var priorities = new string[count];
        var processorMasks = new ulong[count];
        int phase = 0; // 0: warm up, 1: park, 2: measure, 3: stop
        using (var ready = new CountdownEvent(count))
        using (var start = new ManualResetEventSlim(false)) {
            if (Window == null || Window.IsDisposed) {
                Window = new Form();
                Window.Controls.Add(new Label { Dock = DockStyle.Fill, TextAlign = System.Drawing.ContentAlignment.MiddleCenter });
            }
            var window = Window;
            window.Text = title;
            window.Width = 580; window.Height = 160;
            var label = (Label)window.Controls[0];
            window.Show(); window.Activate();
            SetForegroundWindow(window.Handle);
            label.Text = "Activate this benchmark window to begin. Close to cancel.";
            var focusWait = Stopwatch.StartNew();
            while (waitForFocus && GetForegroundWindow() != window.Handle) {
                Application.DoEvents();
                if (window.IsDisposed) throw new OperationCanceledException();
                if (focusWait.Elapsed.TotalSeconds > 60) throw new Exception("Benchmark window was not focused within 60 seconds.");
                Thread.Sleep(20);
            }
            for (int i = 0; i < count; i++) {
                int index = i;
                workers[i] = new Thread(() => {
                    try {
                        ulong x = (ulong)index + 1;
                        while (Volatile.Read(ref phase) == 0) x = Block(x);
                        ready.Signal();
                        start.Wait();
                        long n = 0;
                        ulong mask = 0;
                        while (Volatile.Read(ref phase) == 2) {
                            x = Block(x); n++;
                            if ((n & 127) == 0) mask |= 1UL << (int)GetCurrentProcessorNumber();
                        }
                        processorMasks[index] = mask;
                        blocks[index] = n; checksums[index] = x;
                        priorities[index] = Thread.CurrentThread.Priority.ToString();
                    } catch (Exception ex) { errors[index] = ex; }
                });
                workers[i].IsBackground = true;
            }
            var clock = new Stopwatch();
            try {
                foreach (var worker in workers) worker.Start();
                label.Text = "Warm-up: keep this window focused. Close to cancel.";
                clock.Start();
                while (clock.Elapsed.TotalSeconds < warmup) {
                    Application.DoEvents();
                    if (window.IsDisposed) throw new OperationCanceledException();
                    Thread.Sleep(20);
                }
                Volatile.Write(ref phase, 1);
                while (!ready.IsSet) {
                    foreach (var error in errors) if (error != null) throw error;
                    Application.DoEvents();
                    if (window.IsDisposed) throw new OperationCanceledException();
                    Thread.Sleep(1);
                }
                label.Text = "Measuring: keep this window focused. Close to cancel.";
                Application.DoEvents();
                foreach (var counter in counters) counter.NextValue();
                double cpuBefore = 0;
                foreach (var process in monitors) cpuBefore += process.TotalProcessorTime.TotalMilliseconds;
                clock.Restart();
                Volatile.Write(ref phase, 2);
                start.Set();
                double nextTelemetry = 1;
                while (clock.Elapsed.TotalSeconds < seconds) {
                    Application.DoEvents();
                    if (window.IsDisposed) throw new OperationCanceledException();
                    if (GetForegroundWindow() != window.Handle) result.FocusMaintained = false;
                    if (clock.Elapsed.TotalSeconds >= nextTelemetry) {
                        foreach (var counter in counters) {
                            float value = counter.NextValue();
                            result.Telemetry.Add(String.Format(CultureInfo.InvariantCulture, "{0:F3}\t{1}\t{2}\t{3}\t{4:R}", clock.Elapsed.TotalSeconds, counter.CategoryName, counter.InstanceName, counter.CounterName, value));
                        }
                        nextTelemetry = clock.Elapsed.TotalSeconds + 1;
                    }
                    Thread.Sleep(10);
                }
                Volatile.Write(ref phase, 3);
                foreach (var worker in workers) worker.Join();
                clock.Stop();
                result.Seconds = clock.Elapsed.TotalSeconds;
                double cpuAfter = 0;
                foreach (var process in monitors) { process.Refresh(); cpuAfter += process.TotalProcessorTime.TotalMilliseconds; }
                result.RuntimeCpuMilliseconds = cpuAfter - cpuBefore;
                foreach (var error in errors) if (error != null) throw error;
                foreach (long n in blocks) result.Blocks += n;
                foreach (ulong x in checksums) if (x == 0) throw new Exception("Invalid checksum.");
                if (result.Blocks == 0) throw new Exception("No work completed.");
                result.BlocksPerSecond = result.Blocks / result.Seconds;
                result.WorkerPriorities = String.Join(";", priorities);
                result.ProcessorMasks = String.Join(";", Array.ConvertAll(processorMasks, mask => mask.ToString("X")));
                using (var process = Process.GetCurrentProcess()) result.Priority = process.PriorityClass.ToString();
            } finally {
                Volatile.Write(ref phase, 3); start.Set();
                foreach (var worker in workers) if (worker.IsAlive) worker.Join();
            }
        }
        return result;
        } finally {
            foreach (var counter in counters) counter.Dispose();
            foreach (var process in monitors) process.Dispose();
        }
    }
}
'@
}
[CpuThroughputBench]::Check()
if ($SelfTest) {
    foreach ($count in @(1, 2)) {
        $result = [CpuThroughputBench]::Run($count, 1, 1, 'Benchmark smoke test', @($PID), $false, $false)
        if ($result.Seconds -lt 1 -or $result.BlocksPerSecond -le 0) { throw 'Timing check failed.' }
        if ($result.RuntimeCpuMilliseconds -le 0 -or $result.ProcessorMasks -match '(^|;)0(;|$)') { throw 'CPU observation check failed.' }
    }
    $timer = New-Object System.Windows.Forms.Timer
    $timer.Interval = 500
    $timer.Add_Tick({ $timer.Stop(); [System.Windows.Forms.Application]::OpenForms[0].Close() })
    $cancelled = $false
    try {
        $timer.Start()
        [void][CpuThroughputBench]::Run(2, 1, 1, 'Cancellation check', @(), $false, $false)
    } catch {
        if ($_.Exception.InnerException -isnot [OperationCanceledException]) { throw }
        $cancelled = $true
    } finally { $timer.Dispose() }
    if (!$cancelled) { throw 'Cancellation check failed.' }
    [CpuThroughputBench]::Close()
    Write-Host 'Kernel, single-thread, multithread, and cancellation checks passed.'
    return
}

$cpu = $env:PROCESSOR_IDENTIFIER
$scriptHash = (Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash
Write-Host 'Keep the benchmark window focused. Scores are blocks/second, NOT CPU-Z scores.'
if (!$NoPrompt) {
    Write-Host 'Apply and save the desired Winderust configuration first. This script changes no policies.'
    [void](Read-Host 'Press Enter when ready')
}
$rows = @()
try {
    for ($round = 1; $round -le $Rounds; $round++) {
        # Alternate phase order to avoid always giving MT the hotter starting condition.
        $modes = if ($round % 2) { @('ST', 'MT') } else { @('MT', 'ST') }
        foreach ($mode in $modes) {
            Write-Host "Cooldown ${CooldownSeconds}s; next: $Label round $round $mode"
            [CpuThroughputBench]::Wait($CooldownSeconds)
            $count = if ($mode -eq 'ST') { 1 } else { $Threads }
            $benchmarkProcess = [Diagnostics.Process]::GetCurrentProcess()
            $originalAffinity = $benchmarkProcess.ProcessorAffinity
            try {
                if ($mode -eq 'ST' -and $SingleThreadAffinityMask -ne 0) {
                    $mask = [IntPtr]([BitConverter]::ToInt64([BitConverter]::GetBytes($SingleThreadAffinityMask), 0))
                    $benchmarkProcess.ProcessorAffinity = $mask
                    if ($benchmarkProcess.ProcessorAffinity -ne $mask) { throw 'Affinity verification failed.' }
                }
                $result = [CpuThroughputBench]::Run($count, $Seconds, $WarmupSeconds, "$Label - $mode - round $round", $MonitorProcessIds, $Telemetry.IsPresent)
            } finally {
                try {
                    $benchmarkProcess.ProcessorAffinity = $originalAffinity
                    if ($benchmarkProcess.ProcessorAffinity -ne $originalAffinity) { throw 'Affinity restoration verification failed.' }
                } finally { $benchmarkProcess.Dispose() }
            }
            if ($Telemetry) {
                @("ElapsedSeconds`tCategory`tInstance`tCounter`tValue") + $result.Telemetry |
                    Set-Content -LiteralPath "$OutputPath.$round.$mode.telemetry.tsv" -Encoding UTF8
            }
            $row = [pscustomobject]@{
                Label = $Label; Round = $round; Mode = $mode; Threads = $count
                BlocksPerSecond = $result.BlocksPerSecond; ElapsedSeconds = $result.Seconds
                FocusMaintained = $result.FocusMaintained; PriorityAfter = $result.Priority
                WorkerPrioritiesAfter = $result.WorkerPriorities
                ProcessorMasks = $result.ProcessorMasks; SingleThreadAffinityMask = $SingleThreadAffinityMask
                RuntimeCpuMilliseconds = $result.RuntimeCpuMilliseconds
                RuntimeCpuPercentOfOneCore = 100 * $result.RuntimeCpuMilliseconds / ($result.Seconds * 1000)
                TelemetryNotes = $result.TelemetryNotes
                TimestampUtc = [DateTime]::UtcNow.ToString('o'); Cpu = $cpu
                LogicalProcessors = [Environment]::ProcessorCount; OS = [Environment]::OSVersion.VersionString
                Runtime = [Environment]::Version.ToString(); ScriptHash = $scriptHash
                WarmupSeconds = $WarmupSeconds; CooldownSeconds = $CooldownSeconds
            }
            $row | Export-Csv -LiteralPath $OutputPath -NoTypeInformation -Append
            $rows += $row
            Write-Host ('{0}: {1:N2} blocks/s; focus maintained: {2}' -f $mode, $result.BlocksPerSecond, $result.FocusMaintained)
        }
    }
    $rows | Group-Object Mode | ForEach-Object {
        $valid = @($_.Group | Where-Object FocusMaintained)
        [pscustomobject]@{ Mode = $_.Name; ValidRuns = $valid.Count; MeanBlocksPerSecond = ($valid | Measure-Object BlocksPerSecond -Average).Average }
    } | Format-Table
    Write-Host "Saved: $OutputPath"
} finally { if (!$KeepWindowOpen) { [CpuThroughputBench]::Close() } }
