# Winderust staged-inventory CPU comparison — 28 October 2026

Raw evidence is archived locally and excluded from Git and release artifacts:
the [evidence ZIP](../benchmark-results/archive-20261028/benchmark/ui-tray-engine-staged-results-20261028.zip)
and [SHA-256 sidecar](../benchmark-results/archive-20261028/benchmark/ui-tray-engine-staged-results-20261028.zip.sha256).
Replay requires that separate archive.

Staging process inventory removes unnecessary creation-time queries, but this fresh matched run does **not establish a consistent Wind UI CPU advantage**. Iced used fewer cycles in the first pair; Wind used fewer in the second. Wind had lower private memory in both pairs, subject to unequal prewarm allocation history. No watts or energy were measured.

The independent raw-counter review matched all 60 numeric crosschecks and the fresh native actuation/restoration records.

Both builds contain the same optimization: Toolhelp PID/name metadata is collected once per reconciliation cycle, while identity reads are deferred until needed. Process-appearance detection needs only numeric PIDs. Process Priority can reject a definitely absent basename without opening every process. A distinct identity-attempt flag keeps “not requested” separate from failed identity reads; subsequent upgrades quarantine unknown or replaced generations. Exact live-handle creation time, path, access, protection and restoration checks remain. No scheduler interval, policy, native API or dependency was changed.

The separate read-only scan diagnostic retained all 40 rows from 20 alternating-order pairs. Raw metadata used **14.3905% fewer aggregate current-thread cycles** than identity-bearing inventory; raw was lower in 19 pairs and higher in one. Every scan contained 363 PID/name entries; identity queries succeeded for 266. This measures that scan boundary on one host, including its timing overhead and cache/order effects. Toolhelp still dominates. It is neither a predicted whole-app saving nor a framework/power comparison. Two earlier launch/instrumentation attempts yielded no usable retained CPU series: one had no retained exit/output, and one failed shell parsing before launch. Neither entered the diagnostic or framework comparison.

The fresh framework study used fixed **Wind / Iced / Iced / Wind** order, five minutes warmup followed by three minutes sampled at 1 Hz per formal trial, plus two excluded 3-second/10-second pilots. Each formal trial has 181 observations and 180 valid intervals: 724 observations and 720 intervals total. No trial was discarded or extended based on its CPU outcome. Both frameworks use software rendering, a persistent native tray host and Home-window history, with the settings window destroyed before sampling. Process Priority alone is enabled with a deliberately absent exact-path rule and a configured 1-second check interval; no external workers run during resource capture. Other schedulers, active-target resource cost and the incremental cost over engine-off were not measured.

The configured 1-second value does **not** mean this hidden profile ran once per second. `automation.rs` defines a 10-second hidden periodic floor and applies `automation_refresh_interval` to Process Priority; the requirements helper takes the maximum of that floor and the feature interval. Events can trigger extra passes. Ready/finished debug snapshots and thresholded high-cycle sampling intervals do not count scheduler passes.

The following values include the main process and its identified recovery helper. CPU percentage is relative to **one logical processor**; memory is the median of sampled combined values in MiB.

| Trial | Framework | Million cycles/s | CPU % of one core | Private commit MiB | Private resident MiB | Threads / handles p50 | Host CPU % |
|---|---|---:|---:|---:|---:|---:|---:|
| 1 | Wind UI | 6.500874 | 0.2691 | 20.2461 | 5.1250 | 22 / 515 | 14.23 |
| 2 | Iced | 5.866889 | 0.2170 | 20.5977 | 7.8320 | 32 / 557 | 17.84 |
| 3 | Iced | 9.527165 | 0.3385 | 20.4141 | 7.5195 | 33 / 557 | 15.89 |
| 4 | Wind UI | 8.930111 | 0.3038 | 18.6367 | 6.5273 | 22 / 515 | 11.82 |

Wind’s cycle rate was **10.81% higher** than Iced’s in pair 1 and **6.27% lower** in pair 2, using Iced as each percentage denominator. Summing counters and dividing by summed observed intervals within this cohort gives Wind 7.715496 million cycles/s and Iced 7.697071 million cycles/s: Wind is 0.2394% higher overall. That small aggregate obscures the reversed pair directions. Two pairs under changing host load do not establish equivalence, statistical significance or a reliable winner. Historical cohorts were neither pooled nor used as concurrent before/after controls, so this run cannot quantify whole-app improvement caused by staging.

Equivalently, Iced used 0.2388% fewer aggregate cycles relative to Wind. CPU-time totals were Wind 1.031250 seconds and Iced 1.000000 seconds across approximately 360.019 seconds of exposure each. Main-process work dominated; helper rates were only 3,311–11,961 cycles/s. During the capture interval, process Read/Write byte deltas were zero for all four trials. The additional observed counters were:

| Trial | Other I/O bytes | Other I/O operations | Page faults |
|---|---:|---:|---:|
| Wind 1 | 5,290 | 114 | 20,809 |
| Iced 1 | 5,264 | 108 | 18,866 |
| Iced 2 | 7,344 | 156 | 33,289 |
| Wind 2 | 5,004 | 108 | 29,197 |

These counters are descriptive: Other I/O includes device/cached activity and page faults include soft faults. They do not identify physical disk traffic, hard faults or the cause of the CPU difference.

Wind’s private commit was lower by 0.3516 MiB and 1.7773 MiB in the two pairs; private resident RAM was lower by 2.7070 MiB and 0.9922 MiB. These are observed fixture differences. Iced alone captures and encodes a nonuniform Home PNG before readiness; Wind supplies native layout/visibility observations rather than equivalent first-paint proof. The screenshot payload is not deliberately retained for measurement, but allocation/cache history can persist after five minutes. No assumed buffer is subtracted and the small differences cannot be attributed solely to framework design. Private commit is not resident RAM; summed working sets may double-count shared pages.

All four trials passed the frozen lifecycle and identity gates. Native probes found each trial’s **own exact hidden tray HWND/PID/TID** alive throughout that capture, with successful `Shell_NotifyIconGetRect` before and after sampling and absent settings windows. There were 184 tray-owner checks per trial. The runtime worker was running at the measurement boundary without reported error; ordinary shutdown then completed with no cleanup errors, process exit 0, and the tray host/icon absent. No external wake, injected exit message or forced termination was used. This demonstrates the exercised background lifetime; it does not exercise tray-menu clicks, Explorer restart, accessibility or every UI function.

Separate capability trials launched one dedicated idle worker per framework, outside resource capture. Native priority changed **Normal (32) → BelowNormal (16384) → Normal (32)** on the same PID, creation time, executable and hash. Both apps, both recovery helpers and both workers exited 0; restoration and coordinated cleanup passed, with no live heartbeat reads. This checks one Process Priority action and restoration, not all scheduler behavior. The coordinator exited 0, no Winderust/worker processes remained in the post-run inventory, and Balanced was observed before and after.

Both fixtures use Wind UI 0.28.0 / Iced 0.14.0 with unchanged compiler/dependency pins, adapters and menu source proofs, and all 112 shared engine files byte-identical. The four-file overlay and full source fingerprints accompany the evidence. Production checks passed: formatting, strict Clippy, architecture/legacy scans, graph update and 890 tests, with 19 ignored. The isolated Iced fixture retains two inherited strict-lint failures: unused `TrayVisibilityWatcher` and its `start` function. Its recorded baseline passes with only `dead_code` allowed; this is distinct from the passing strict production checks.

The retained official-source review checked [Wind UI 0.31.0](https://github.com/huanfeng/wind-ui-rust/releases/tag/v0.31.0) and [the pinned-to-current comparison](https://github.com/huanfeng/wind-ui-rust/compare/d5c105067d7a31f0f14e388f1fe6394f1a043988...57391b6446eb027d1129d3c47519a159d937c7cb): all seven Win32 platform source blobs were unchanged. The [message-loop source](https://github.com/huanfeng/wind-ui-rust/blob/57391b6446eb027d1129d3c47519a159d937c7cb/src/platform/win32/mod.rs#L1852-L1947) already blocks in `GetMessageW` without active animation. This optimization changes shared engine observation work, not Wind UI’s event loop; an upstream upgrade alone supplies no demonstrated background saving.

The default production release build passed at **04:38:49 UTC**, after all captures and actuation. It used default features, without benchmark features, and was built without being launched. The resulting `target/release/winderust.exe` is 12,338,176 bytes, SHA256 `48e9f6ef6d5a1b7192fb844bd8c1de35edce42d5b090166d60449c63ec2c776b`. Its result/log are retained separately from the frozen benchmark build identities.

The host was Windows build 26200, Intel Core 5 210H with 12 logical processors and about 16 GiB physical RAM, on AC/Balanced. Memory-load ranges in trial order were **85–92%, 87–89%, 91–94% and 76–79%**: 209 of 724 observations were at least 90%, including 73 at least 93%. Machine CPU means were 14.2346%, 17.8356%, 15.8918% and 11.8155%. Such changing pressure/process mix can confound CPU and residency comparisons; it was not controlled away. The first Wind trial’s member intervals ranged from 0.6943 to 1.3072 seconds; all 180 intervals remained included and rates use observed elapsed time. Ready/finished debug scan counts are only boundary observations, not per-pass counts or proof of identical inventories.

The separate sampler consumed 1.26–1.94% of one core and is excluded from app totals; its second Wind exposure was higher. These are current-load measurements, not a quiet-host minimum. OS process cycles are execution accounting, not retired instructions, hardware wakeups or joules. GPU and context-switch collection were disabled. No energy meter, power baselines, wattage, battery-life or universal framework claim follows from this CPU-only study.

Evidence is under the bundle-relative `tray-engine-protocol-cpu-staged-20261028`, `tray-engine-actuation-cpu-staged-20261028`, `tray-parity-staged-20261028`, `tray-engine-launch-cpu-staged-20261028`, `staged-observations-20261028` and independent-review directories. The accompanying bundle contains frozen validators, raw observations, source/build identities, diagnostic and safety evidence, plus an exact file-inventory/SHA256 manifest. Its external `.sha256` sidecar identifies the ZIP. Integrity verification and guarded relocation-safe replay are separate checks; neither substitutes for the frozen benchmark’s acceptance gates.
