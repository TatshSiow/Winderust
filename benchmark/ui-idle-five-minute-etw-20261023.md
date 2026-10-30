# Five-minute background CPU investigation

Linked raw evidence is archived locally under `benchmark-results/archive-20261028/` and excluded from Git and release artifacts; replay requires that separate archive.

Completed on **2026-10-23**. This investigation identified Windows threadpool
timer activity and Sophos security-module execution around five minutes of
process age. It did **not** identify an evidence-backed Winderust production
change that would reduce hidden-idle CPU further. Production code, the pinned
Wind UI dependency, and the normal release executable remain unchanged.

The [evidence bundle](../benchmark-results/archive-20261028/benchmark/ui-idle-five-minute-etw-results-20261023/README.md) preserves
the original failed CPU capture, the separately declared forensic analysis,
filtered target events and stacks, source, and a portable offline verifier.

## What the trace shows

The predeclared `[285,315)` second window contains 115 target events: five
thread starts, 77 context switches, 28 ready-thread events and five CPU samples.
One sample is ISR/nonprocess activity and is excluded from application execution
interpretation. The other four occur in target-thread context, including one
that captures ETW stack-walking instrumentation. They contain these observations:

| Main-process age, seconds | Scope | Executing sample evidence |
| ---: | --- | --- |
| 300.124549 | Main | Loader/initialization stack containing `SophosED.dll` frames |
| 300.336005 | Main | ETW stack-walk/thread-creation frames above `ntdll!NtReleaseWorkerFactoryWorker`, `TppPrepareDirectParams`, `TppWorkerThread` |
| 300.354788 | Recovery helper | Kernel/file-system stack containing `SophosED.sys`, NTFS and filter-manager frames |
| 300.482156 | Main | Instruction pointer and leaf frame in `SophosED.dll+0x19a8ed`, above Windows threadpool timer-expiration frames |

Three main-process thread starts occur at ages 300.124202, 300.124774 and
300.335982 seconds; two helper starts occur at main-process ages 300.353970 and
300.354677 seconds. Captured target-owned creator stacks include Windows
threadpool timer-expiration paths. Initial external creators remain redacted.
Creator and waker stacks are kept separate from executing CPU samples.

An exact-PDB, cache-only lookup resolved 130 of 171 selected frame-symbol
requests; 41 were unavailable in the cache. No private Sophos function names
were recovered. None of the four normal sample stacks contains an identified
Winderust or Wind UI frame. This establishes security-module involvement in
this trace's event cluster; it does not establish the entire cluster's cause,
total CPU cost, or the absence of all application work between samples.

The installed `SophosED.dll` reports "Sophos Endpoint Defense In-Process
Protection", version 2026.2.0.1, with a valid Authenticode signature from Sophos
Limited. Its PE CodeView GUID/age and image size match the two target module
records. This corroborates module identity; ETW did not hash the loaded image
bytes. The bundle preserves the identity and file-metadata observations.

The `[346,351)` second window has no target events. It represents a
counterfactual final-sampling boundary: the failed collector never reached that
boundary. The `[351,356)` cleanup window contains 1,184 target events.

## What failed, and what remains usable

The original collector failed on its first readiness-file read with
`PermissionError`. It accepted **zero CPU trials** and produced no process-counter
CSV. Its fixture nevertheless ran to its normal deadline and reported successful
cleanup. The wrapper then timed out after 120 seconds waiting for WPR to save;
the recorder later logged a successful save. These original failures are
preserved, rather than relabeled as a successful benchmark.

Fresh conversion of the saved 3,855,613,952-byte ETL reported zero lost events and
no truncation. The separate forensic selection matched one complete main
generation to the published PID, exact image and trial arguments, its collector
parent generation, and one correctly identified recovery-helper child. Both
targets exited with code zero. The main ETW lifetime was 352,557.2069 ms, within
324.3557 ms of the fixture's reported process-run duration and inside the
declared two-second tolerance. The helper lifetime was 352,180.404 ms.

The filtered export contains 6,259 events: 3,360 context switches, 2,188
ready-thread events, 657 sampled-profile events and 54 lifecycle events.
Independent review checked the exported identity, lifetime and count arithmetic.
No independent native process-creation FILETIME evidence survived the failed
collector; ETW-derived identities are explicitly labeled as such.

The full TraceEvent export contains 590 context/stack-owner associations across
318 events whose timestamps follow the associated thread's recorded end, by at
most 21.0128 ms. These are preserved and are not attributed as target CPU
execution. No such late associations occur in the five-minute window; all five
samples there fall inside their exact exported thread-generation lifetimes.

There is also a disclosed provenance deviation. The frozen forensic protocol
named a separate `postflight.json` ETL hash that was never completed. The
unchanged protocol is accompanied by an addendum: `cleanup-state.json` supplies
file size, recorder-off, process-exit and power-plan observations, while the
fresh analyzer's streaming hash is the **sole ETL fingerprint authority**.
Independent ETL hash agreement is unavailable. The public bundle excludes the
systemwide ETL and private ETLX, so its offline verifier cannot independently
reconvert the trace or prove export completeness or reported event loss.

## Implication for further optimization

This trace supports investigating process-lifetime Windows/security activity,
but provides no demonstrated Winderust polling loop to remove. Disabling
security protections or weakening crash recovery is not a measured application
optimization, and neither was changed. The existing source audit found no
application-owned 300-second idle timer.

[`QueryProcessCycleTime`](https://learn.microsoft.com/en-us/windows/win32/api/realtimeapiset/nf-realtimeapiset-queryprocesscycletime)
sums user-mode and kernel-mode cycles across a process's threads. From that
definition, security code executing on those threads can contribute to the
process counter; the counter alone does not isolate UI-framework cost.

The [previous successful matched comparison](ui-settled-idle-current-load-20261023.md)
still stands: Wind used less private memory in every pair; CPU-cycle results
split two pairs each way and were dominated by brief activity near five minutes.
This new trace cannot assign that earlier measured burst to Sophos, because no
cycle series confirms its recurrence and the failed observer did not perform
the previous run's one-second polling. It adds no framework ranking, cycle-rate
reduction, energy measurement or battery-life claim. Automation was disabled;
enabled scheduler cost needs its own matched workload measurement.

## Validation and cleanup

- All 160 frozen build-provenance source entries still match their recorded
  sizes and hashes. Fixture and normal-release executable hashes are unchanged.
- Formatting, Clippy with warnings denied, legacy-name and architecture ownership
  checks passed. The initial parallel Rust suite failed two helper-exit tests;
  both passed in isolation, and the complete serialized suite passed **885 tests,
  with 18 ignored**. No recovery timeout was changed.
- `git diff --check` passed and `graphify update .` completed.
- WPR is not recording, no Winderust fixture remains, and the original Windows
  Balanced power plan remains active. No unrelated application or security
  setting was changed.

Windows documents the recorder operations in its
[WPR command-line reference](https://learn.microsoft.com/en-us/windows-hardware/test/wpt/wpr-command-line-options)
and the distinction between sampled, scheduling and lifecycle stack collection
in the [stack reference](https://learn.microsoft.com/en-us/windows-hardware/test/wpt/stack-wpa).
