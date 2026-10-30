// Disposable benchmark target. Compile with the .NET Framework csc /target:winexe.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Threading;

internal static class SchedulerWorker
{
    private static string Escape(string value)
    {
        return value.Replace("\\", "\\\\").Replace("\"", "\\\"")
            .Replace("\r", "\\r").Replace("\n", "\\n");
    }

    private static void WriteAtomic(string path, string value)
    {
        string temporary = path + ".tmp";
        File.WriteAllText(temporary, value, new UTF8Encoding(false));
        for (int attempt = 0; ; attempt++)
        {
            try
            {
                if (File.Exists(path))
                    File.Replace(temporary, path, null);
                else
                    File.Move(temporary, path);
                return;
            }
            catch (IOException error)
            {
                if (attempt >= 10 ||
                    (error.HResult != unchecked((int)0x80070020) &&
                     error.HResult != unchecked((int)0x80070021)))
                    throw;
            }
            catch (UnauthorizedAccessException error)
            {
                if (attempt >= 10 || error.HResult != unchecked((int)0x80070005))
                    throw;
            }
            // Brief reader locks can report sharing/lock or access denial; permanent failures rethrow.
            Thread.Sleep(20);
        }
    }

    private static int Main(string[] args)
    {
        try
        {
            var options = new Dictionary<string, string>(StringComparer.Ordinal);
            for (int index = 0; index < args.Length; index += 2)
            {
                if (index + 1 >= args.Length || options.ContainsKey(args[index]) ||
                    (args[index] != "--heartbeat" && args[index] != "--stop" &&
                     args[index] != "--mode" && args[index] != "--ready"))
                    return 2;
                options.Add(args[index], args[index + 1]);
            }
            if (!options.ContainsKey("--heartbeat") || !options.ContainsKey("--stop") ||
                !options.ContainsKey("--mode") ||
                (options["--mode"] != "idle" && options["--mode"] != "busy"))
                return 2;

            string heartbeat = Path.GetFullPath(options["--heartbeat"]);
            string stop = Path.GetFullPath(options["--stop"]);
            string ready = options.ContainsKey("--ready")
                ? Path.GetFullPath(options["--ready"]) : heartbeat + ".ready.json";
            Directory.CreateDirectory(Path.GetDirectoryName(heartbeat));
            Directory.CreateDirectory(Path.GetDirectoryName(ready));
            if (File.Exists(stop))
                return 2;

            var clock = Stopwatch.StartNew();
            WriteAtomic(heartbeat, "0");
            using (Process process = Process.GetCurrentProcess())
            {
                WriteAtomic(ready, "{\"pid\":" + process.Id +
                    ",\"creation_filetime\":" + process.StartTime.ToUniversalTime().ToFileTimeUtc() +
                    ",\"exe\":\"" + Escape(process.MainModule.FileName) +
                    "\",\"mode\":\"" + options["--mode"] + "\"}");
            }

            long nextHeartbeat = 1000;
            while (clock.ElapsedMilliseconds < 9000000)
            {
                if (File.Exists(stop))
                    return 0;
                if (clock.ElapsedMilliseconds >= nextHeartbeat)
                {
                    WriteAtomic(heartbeat, clock.ElapsedMilliseconds.ToString(
                        System.Globalization.CultureInfo.InvariantCulture));
                    nextHeartbeat = clock.ElapsedMilliseconds + 1000;
                }
                if (options["--mode"] == "idle")
                    Thread.Sleep(100);
                else
                    Thread.SpinWait(10000);
            }
            return 3;
        }
        catch (Exception error)
        {
            // winexe has no console; a sidecar records failures without a dialog.
            if (args.Length >= 2 && args[0] == "--heartbeat")
            {
                try { File.WriteAllText(args[1] + ".error.txt", error.ToString()); }
                catch (IOException) { }
                catch (UnauthorizedAccessException) { }
            }
            return 1;
        }
    }
}
