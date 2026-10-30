# Idle daemon checkup - 27 October 2026

Linked raw evidence is archived locally under `benchmark-results/archive-20261028/` and excluded from Git and release artifacts; replay requires that separate archive.

The new fixed Wind-Iced-Iced-Wind run completed: **four accepted formal idle captures**, two excluded pilots, and before/after no-app baselines. The independent offline replay also accepted the four captures. This is additional evidence, separate from the [earlier failed daemon cohorts](ui-framework-followup-20261027.md).

**Iced's zero-window configuration used fewer measured cycles and slightly less private memory in both pairs. Neither framework has a demonstrated application power advantage.** This does not establish a universal framework winner: Iced lacked a persistent tray host, host load varied, and most captured cycles came from a brief housekeeping event rather than continuous idle work.

| Formal pair | Wind UI cycles/s | Iced cycles/s | Wind private commit, MiB | Iced private commit, MiB | Wind private resident, MiB | Iced private resident, MiB |
|---|---:|---:|---:|---:|---:|---:|
| 1: Wind then Iced | 76,045 | 20,920 | 12.359 | 11.863 | 6.168 | 5.617 |
| 2: Iced then Wind | 29,537 | 27,380 | 12.328 | 11.836 | 6.180 | 5.629 |

Rates include the application and every observed recovery helper; memory is the median sum of their private counters. Each capture lasted 120 seconds after a 60-second warmup. These are descriptive paired observations, not confidence intervals or independent per-second experimental repetitions. No trial was retried, discarded, or replaced.

Wind's cycle count exceeded Iced by 263.5% in pair 1 and 7.88% in pair 2. **The large first difference is not a reliable expected saving:** Wind trial 1 ran at 93-98% machine memory load, while the other trials had lower pressure. Its recovery helper also consumed more cycles. Both configurations' private commitment differed by about 0.49-0.50 MiB, with private resident memory differing by about 0.55 MiB. The old retained-window Iced memory result therefore cannot be generalized to a daemon that never creates the settings window.

The raw intervals explain why an average alone is misleading. In Iced trials 2 and 3 and Wind trial 4, each main process and helper recorded cycles in exactly one interval, around 74 seconds into sampling, or 134 seconds after launch. Threads and handles fell at the same event, along with a small amount of private memory. Other sampled intervals had zero cycle-counter increments. Each of these three captures then had about 46 seconds of zero observed increments through the end of sampling. Wind trial 1 included this event and additional sparse activity. This is evidence of occasional housekeeping during the measured process ages; it is **not** evidence of continuous 20,000-76,000 cycles/s polling. No new stack trace identifies the event's owner or proves an application or framework defect. It does not cover activity near five minutes of process age.

Additional observed metrics:

- Initial and full-capture median handle counts were 505 for Wind and 477 for Iced; thread counts were 21 and 32. After housekeeping they fell to 501/473 handles and 15/26 threads, respectively. More dormant threads do not by themselves imply higher CPU usage.
- Total working sets were about 42.46-42.47 MiB for Wind and 34.75-35.73 MiB for Iced. These sums include shared pages and must not be treated as unique system memory consumption.
- Raw process I/O counter deltas were zero during each capture. Page-fault deltas were small; Windows' counter includes faults that do not require disk access.
- Only Wind trial 1 accumulated a nonzero CPU-time increment, 31.25 ms across application and helper. The other trial totals were zero at that counter's resolution despite nonzero cycle counts. Zero recorded CPU time does not mean zero work.

| Capture | CPU-package power, W | Machine CPU load, % |
|---|---:|---:|
| No app, before | 22.289 | 22.00 |
| Wind 1 | 18.755 | 19.81 |
| Iced 2 | 16.715 | 18.00 |
| Iced 3 | 20.857 | 20.00 |
| Wind 4 | 14.071 | 13.07 |
| No app, after | 17.709 | 19.36 |

The meter passed continuity and counter-status checks, but this was the user-authorized comparison under current host load. Pair power differences reversed direction (+2.041 W and -6.786 W for Wind minus Iced), while no-app baselines drifted by -4.580 W. **Do not attribute those wattages to either app, subtract the baselines to manufacture app watts, or calculate battery life from them.** They cover the whole CPU package, including unrelated work and the sampler, not GPU, wall, or per-application energy.

The closing baseline retains a reused internal `before` label and `baseline-before.csv` filename inside its separate `baseline-after/` directory. Its after-position comes from the frozen runner order, distinct start/completion records, timestamps, and distinct raw hash. The original record is preserved; it was not silently relabelled.

The builds remain Wind UI 0.28 and Iced 0.14, using the same 112 engine source files. Wind retains its tray host; Iced uses the daemon experiment without a persistent tray host. Both explicitly reported automation disabled and no runtime worker. There were **zero external scheduler workers**. The dedicated target executable was a required fixture declaration only, not a running target. This run isolates idle application/recovery overhead and does not measure enabled schedulers or claim complete product parity.

Both excluded pilots and all formal trials passed readiness, absent-settings-window observations, settings consistency, raw counter coverage, finished-state and coordinated cleanup checks. Each formal Iced trial received exactly one narrowly validated post-capture `WM_NULL` wakeup to complete deadline shutdown; it occurred outside measurement and remains a lifecycle limitation, not a shipping fix. No app was force-killed. The launcher ran without administrator privileges. The active power-plan GUID matched in query-only snapshots before and after; those snapshots are context, not continuous plan monitoring or proof of restoration after a mutation. A final process snapshot found no matching Winderust application, recovery helper, or worker candidate.

Fresh upstream source checks found [Wind UI 0.30.0](https://github.com/huanfeng/wind-ui-rust/releases/tag/v0.30.0) as the latest stable release. Across [0.28 to 0.30](https://github.com/huanfeng/wind-ui-rust/compare/v0.28.0...v0.30.0), the Windows event-loop/backend and tray modules retained identical Git blobs. The newer releases change widgets, focus, layout, themes and APIs; those changes may affect applications, but this check found no new core idle/tray change to credit with lower cycles. **Wind UI 0.30 performance was not measured.** Both Wind's [resident API](https://github.com/huanfeng/wind-ui-rust/blob/v0.30.0/src/app/mod.rs#L1214) and Iced's [daemon API](https://github.com/iced-rs/iced/blob/0.14.0/src/daemon.rs#L19) support keeping an application alive without a settings window. The architecture does not establish either framework's measured minimum.

A separate real Windows file-sharing check reproduced `WinError 5` when a reader held a JSON file open. The canonical bounded writer retry completed in 56.6-58.8 ms total after a temporary lock was released; a persistent lock failed in 244.9-252.6 ms while preserving the existing file and new temporary file. These requested behaviors passed in both execution contexts. An extra share-delete control had an incorrect expected-success assertion: replacement was denied there too, so its original combined test result remains failed. This shows reader contention can cause this failure mode; it does not establish the cause or owner of the earlier conflicting handle. Live benchmark observation used immutable marker existence instead of opening changing JSON files.

No production Rust, Wind UI source, dependency version, or saved application settings were changed for this checkup. The [prior accessibility and dependency findings](ui-framework-followup-20261027.md) remain separate: this performance run does not resolve semantic UI Automation support, keyboard/Narrator/IME coverage, or dependency maintenance issues. Prior enabled-engine CPU results still changed direction between pairs and are not pooled with this disabled run.

For Winderust, the defensible conclusion is narrower than "Wind is better in every measurement": a zero-window Iced daemon can be very small, and both implementations spent most sampled intervals without recorded process cycles. Wind's practical widget/tray benefits can still matter, but a claim that it inherently consumes less CPU or power is unsupported. The next decisive performance experiment would give Iced the equivalent tray/recovery lifecycle and cover longer process ages with enabled automation under controlled host conditions. This checkup does not require another framework migration or demonstrate an upstream idle-loop defect.

The [portable evidence ZIP](../benchmark-results/archive-20261028/benchmark/ui-idle-daemon-checkup-results-20261027.zip) contains raw CSVs/JSON, the frozen protocol and sources, excluded pilots, cleanup records, independent review, upstream/file-sharing evidence, and validation logs. Original executable paths are provenance declarations; offline replay does not need the binaries. The earlier evidence ZIP and failed cohorts remain unchanged.

Required project checks passed after the run: `git diff --check`, formatting, strict locked all-target Clippy, `cargo test --locked` (885 passed, 18 ignored), the legacy-name scan, architecture ownership checks, and `graphify update .`. The graph has 23,863 nodes and 60,540 edges; HTML generation was skipped by its size limit. Canonical measurement tests passed 111/111. An initial Python test invocation used the wrong working directory and failed imports; its log is preserved alongside the successful invocation from `scripts/`. The isolated idle helper and cohort validators also passed their 15 and 6 preparation tests.

The ZIP is 945,025 bytes and contains 212 manifest-listed payload files (5,136,713 bytes), plus the manifest itself. Archive SHA-256: `b0a850148c4adc093c4baeb8e9b4972c3f2f39626b71f2022f42387e60ceb32c`. Manifest SHA-256: `4772d8aca01386892aebed6cd7c54cf8c2c1eb8ca7f367c5b173a18a5da48735`.

After extraction, run `python -B export_results.py --verify .` for integrity. Replay the frozen analysis with `python -B idle-daemon-checkup-20261027/source/verify_idle.py --cohort idle-daemon-checkup-20261027 --output ../idle-replay.json`. The archive README gives the synthetic test commands. Keep replay output outside the extracted payload.

Fresh extraction passed exact manifest checks before and after replay. The frozen validator reproduced all four numerical results after rebasing only derived CSV paths; 6 cohort, 15 helper and 3 exporter integrity tests passed. An audited replay blocked access to the original workspace outside extraction and blocked native/network/subprocess operations. Its first guard configuration rejected Python's standard `ctypes` initialization; verification then preloaded that standard library before installing the guard and passed with no prohibited operations. The failed guard log and exact invocation are retained locally under `target/scheduler-benchmark/idle-daemon-export-20261027/`; the archive bytes were unchanged. The ordinary portable replay command above also passed independently of that guard.
