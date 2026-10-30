# Background idle ETW investigation — 2026-10-22

Linked raw evidence is archived locally under `benchmark-results/archive-20261028/` and excluded from Git and release artifacts; replay requires that separate archive.

The remaining background activity includes Windows RPC and threadpool lifecycle
work. This capture does not establish a Wind UI renderer defect or explain the
entire previous Wind/Iced cycle difference. A separate, source-backed candidate
removes unused automation event registrations while retaining appearance
updates; its benefit requires an uninstrumented comparison.

## Capture and validation

One frozen Wind fixture used the same disabled-automation, cold Home-to-background
scenario as the [thread diagnostic](ui-idle-thread-diagnosis-20261022.md). The
software-rendered settings HWND was absent at readiness, settings stayed in
memory, updates and startup registration were disabled, and one real recovery
helper remained active. No application or framework source changed during this
capture. The fixture SHA-256 was
`e5e825397dd4417e292f39a2b300c1079dae458a4f10fea1c74cf26093cb51d6`.

WPR started before process creation and stopped after normal application/helper
exit. The custom profile enabled ProcessThread, Loader, CpuConfig, CSwitch,
ReadyThread and SampledProfile, with thread-create/delete, switch, ready and CPU
stacks. File mode used 64 buffers of 1 MiB; that is a buffer budget, not a file
size limit. The trace spans 204.724 seconds, the fixture requested 198 seconds
after readiness, and process counters cover 180.019 seconds after 15 seconds of
warmup. These are distinct horizons.

The trace contains 11,899,381 events, zero reported lost events, and no
truncation. Exact application/helper generations match native creation/exit
timestamps within 0.4 ms. Filtered output includes 4,601 events: 2,236 context
switches, 2,114 ready events, 205 sampled events and 46 thread lifecycle events.
All 205 samples are whole-lifetime observations, including startup and shutdown.
External creator/waker identities and stacks are redacted from the exports.

PowerShell returned a null collector ExitCode property, so the original wrapper
failed and remains recorded as failed. Independent ETW and raw-data audits
confirm one collector generation with native exit status 0, plus application
and helper exit status 0, clean fixture cleanup and the full requested lifetime.
The raw CSV summaries reconcile independently. The Balanced power-plan GUID
remained unchanged and the named tracing session stopped successfully. Native
HWND absence was checked at readiness and at each 1 Hz process sample; there was
no continuous guard between samples.

## What the stacks show

Near process age 30 seconds, a captured creator stack appears independently in
both the application and helper:

```text
ntdll!TppWorkerThread
  ntdll!TppTimerpExecuteCallback
    rpcrt4!PerformGarbageCollection
      rpcrt4!RpcpConvertToLongRunning
        ntdll!TpCallbackIndependent
          ntdll!NtSetInformationWorkerFactory
```

This is RPC garbage-collection/threadpool expansion context. The first new
worker's external creator is redacted; the displayed stack belongs to that
worker creating another worker. It does not assign every cycle in that interval
to RPC or identify which component initially caused RPC initialization.

Seven target threads exit around process age 134 seconds. Their termination
stacks include `TppWorkerThread`, `RtlExitUserThread` and `NtTerminateThread`.
That confirms Windows threadpool thread termination on those events. It does
not prove a particular documented timeout setting or justify changing a shared
Windows pool.

Five CPU samples lie in the approximate process-age 15–195-second background
window. One is explicitly flagged ISR/non-process; its interrupted thread stack
must not be counted as an application callback sample. The other four belong to
one thread under `user32!GetMessageW`. Three sampled instruction pointers resolve
to kernel unwind/ETW stack-walk work, and the fourth is in scheduling/wait
switching. Some stacks include WinEvent processing, but this trace samples its
own stack-capture overhead and cannot quantify normal hook callback cost.

Matching public PDB GUID/age and module offsets resolved 130 of 140 selected
addresses. Eight private Sophos offsets and two unavailable UCPD offsets remain
unresolved. No local Winderust PDB matched this frozen executable, so its Rust
frames remain module offsets. Security-module presence is not evidence of
causation. No security setting was changed.

## Optimization implication

Source inspection found that appearance-only observation still registered both
desktop-wide automation hooks and all power/session subscriptions, even though
disabled-automation callbacks were later rejected. The frozen Iced source has
the same behavior and the same Windows-accent preference. This is an application
registration candidate shared by both integrations, not a Wind-only defect.

The candidate retains the top-level window for Windows appearance broadcasts.
When a watcher is required and either power profile needs an automation worker,
it preserves all existing registrations, startup failure fallback and cleanup.
Mode changes publish the watcher inactive before recreating it, then publish
successful readiness. Its savings and any comparison with Iced must come from a fresh
uninstrumented comparison, not this trace.

## Evidence and limits

Curated evidence is in [the ETW export bundle](../benchmark-results/archive-20261028/benchmark/ui-idle-etw-results-20261022/README.md).
The full 1.10 GB ETL, private 2.22 GB ETLX and full filtered event stream stay
under `target/idle-etw-20261022/`; raw files include system-wide activity and are
not publication artifacts. Existing source/binary provenance snapshots and the
failed original wrapper are preserved.

There is one diagnostic run on a shared desktop. ETW adds overhead; sampled
stacks, cycle counters and energy are different measurements. Readiness was not
synchronized with the ETW clock, so the 15–195-second selection is explicitly
approximate. No Iced trace, watts, retired-instruction count or new framework
ranking follows from this investigation.

The profile follows Microsoft's [SystemProvider](https://learn.microsoft.com/en-us/windows-hardware/test/wpt/systemprovider)
and [stack schema](https://learn.microsoft.com/en-us/windows-hardware/test/wpt/stack-wpa).
Interpretation follows [CPU analysis](https://learn.microsoft.com/en-us/windows-hardware/test/wpt/cpu-analysis)
and the [TraceEvent guide](https://github.com/microsoft/perfview/blob/main/documentation/TraceEvent/TraceEventProgrammersGuide.md).
Windows documents [threadpool ownership and cleanup](https://learn.microsoft.com/en-us/windows/win32/procthread/thread-pools),
[out-of-context WinEvent delivery](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook),
and [appearance broadcasts](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-themechanged).
