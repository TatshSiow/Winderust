# Background maintenance cycle diagnosis — 2026-10-20

Linked raw evidence is archived locally under `benchmark-results/archive-20261028/` and excluded from Git and release artifacts; replay requires that separate archive.

The Wind UI port performs application maintenance every five seconds after its
settings window has been destroyed. The preserved Iced implementation removes
its ordinary UI tick when hidden and healthy. A controlled Wind UI diagnostic
confirmed that this application maintenance is a substantial source of idle CPU
cycles: suppressing its callback reduced cycles by a median **81.1% across three
pairs** (range **72.3–85.4%**).

This identifies an application integration cost. It does not establish an
unavoidable framework cost, a safe production optimization, or an energy saving.
Production code, the production executable, portable settings, and the previous
benchmark evidence were not changed for this investigation.

| Background mode | Cycles/second, median | Trial range | Private resident RAM, median |
| --- | ---: | ---: | ---: |
| Ordinary maintenance | 644,987 | 585,676–807,623 | 6.180 MiB |
| Maintenance callback skipped after readiness | 110,622 | 94,232–223,667 | 6.109 MiB |

Values include the application and its recovery helper. Median paired savings
were **550,754 cycles/second**, with a range of **475,054–583,956**. The paired
percentage uses each pair's own baseline; it is not calculated from the two
independent group medians. Resident RAM ranges overlapped, so these runs do not
establish a meaningful memory improvement. Skipped-mode CPU time rounded to zero
in the Windows time counter while its cycle counter remained positive.

The source paths explain what the diagnostic suppresses:

- [App interval and message handling](../src/ui/app.rs): the unconditional
  five-second application interval at line 243 sends `Message::Tick`, even with
  no settings window. `native_tick` at line 1566 queries visibility and the UI
  HWND; both native queries enumerate the thread's windows.
- [Tray maintenance](../src/ui/app.rs): `probe_tray` at line 1632 periodically
  spawns a short-lived thread that checks native tray registration.
  `sync_tray` at line 1291 constructs localized menu/status data on ticks.
  `set_menu_state` caches that data; this is not a tray-icon repaint every tick.
- [Native resident shell](../src/platform/windows/resident_shell.rs): tray
  probing uses `Shell_NotifyIconGetRect`; visibility/HWND queries use
  `EnumThreadWindows`. The ordinary message path also performs power-source
  reads, settings comparisons, status checks, and signal bookkeeping.
- The preserved Iced source, archived under `source/v3/iced/`, has
  `ui_tick_interval` in `src/ui/app.rs` at line 2541. It returns `None` when
  hidden and healthy; pending recovery/retry can retain a five-second interval.
  Its ordinary background wake channel waits for actual events.
- The pinned Wind UI framework
  (`36b139c66abca0c8c94dec7ed08fdf8ffb2de1d2`) blocks in `GetMessageW`
  with no live or animating UI windows and releases its high-resolution timer
  request. The settings window's 250 ms timer ends when that HWND is destroyed.
  These source checks do not support a continuous background rendering loop as
  the cause of the observed difference.

The experiment used a source clone extracted from the frozen V3 Wind UI archive.
Only `src/ui/app.rs` and `src/ui/smoke/resources.rs` differ. Both modes use the
**same release executable**, register the **same native five-second interval**,
and complete the same startup. The diagnostic flag
`--idle-diagnostic-skip-maintenance` returns before application Tick handling
only after the resource fixture has confirmed background readiness. Native timer
delivery and framework dispatch therefore remain in both modes. The comparison
measures the aggregate callback path, including work it starts on other threads;
it does not isolate the cost of a timer, tray query, allocation, or function.

Both modes also use one identical sleeping thread that sends a single completion
wake 78 seconds after readiness. Each capture warms up for 15 seconds, measures
for 60 seconds at 1 Hz, and finishes before that deadline. The sequence was
full/skipped, skipped/full, full/skipped. All six accepted captures confirmed
automation disabled, in-memory settings, absent UI HWND, matching mode flags,
exit code zero, successful message-loop completion, and no runtime or cleanup
errors. No builds or extra command launches occurred during timed collection.
Sampler cost is recorded separately without subtraction. GPU, energy, and
expensive optional thread counters were disabled.

The short pilots are excluded. The initial sandbox attempt could not enumerate
the desktop; the next pilot needed the native UI window-class filter to exclude
the hidden resident host. An initial diagnostic that removed interval
registration entirely also stalled startup before readiness: the pre-loop queued
wake did not open the UI without the interval's first dispatch. That failed
fixture received a cooperative `WM_QUIT` request to its path-verified main thread;
it was not force-terminated. The revised same-startup diagnostic passed both
pilot modes before the six captures. Failed attempts and both diagnostic
revisions are retained under the evidence directory.

The earlier [scheduler comparison](ui-scheduler-resource-comparison-20261020.md)
used dedicated worker processes and scheduler-fixture bookkeeping, unlike this
targeted diagnostic. Do not pool their absolute cycle or memory values, subtract
this result from the earlier framework gap, or compare the skipped mode directly
with the earlier Iced median. Live desktop events remain uncontrolled; three
pairs are a small sample. CPU cycles are OS process-cycle accounting, not retired
instructions, physical wakeups, or watts. Individual maintenance operations and
shared Windows appearance-event hooks have not been isolated by this test.

For a production improvement, review event-driven visibility and tray/status
updates, cache unchanged menu data, and avoid creating a fresh thread for each
healthy tray probe. Preserve tray-loss detection, bounded retry, UI access,
process restoration, and shutdown. Suppressing the entire callback is a
diagnostic technique, not a production patch. Enabled scheduler work remains a
separate shared-backend optimization target.

[Summary and validation](../benchmark-results/archive-20261028/benchmark/ui-idle-maintenance-results-20261020/summary.json),
[trial data](../benchmark-results/archive-20261028/benchmark/ui-idle-maintenance-results-20261020/trials.csv), and
[paired differences](../benchmark-results/archive-20261028/benchmark/ui-idle-maintenance-results-20261020/pairs.csv) are retained
beside this report. Full raw data, patched source, build records, binary, failed
pilots, analysis script, and archive are under
`target/idle-cycle-diagnosis-20261020/`. Reproduce collection with:

```powershell
python -B scripts/measure_ui_resources.py --config target/idle-cycle-diagnosis-20261020/config.json --out target/idle-cycle-diagnosis-20261020/capture-reproduction --warmup 15 --duration 60 --interval 1 --trials 3 --startup-timeout 30 --no-gpu --memory-ex2
```

The diagnostic binary SHA-256 is
`6676a29becbd678d8ed7e7729e79ce13577ae5b12a08a8a226d0751c8f0d4733`.
The unchanged collector SHA-256 is
`98e473b2858c28e5a20f03dc71980a733f766916ee5ce7ad582e7182f9905946`.
The original V3 evidence ZIP SHA-256 is
`fc6457819e1e64b700970a43df87ca15c666ee845ae81a3d049df61c63ad941f`.
