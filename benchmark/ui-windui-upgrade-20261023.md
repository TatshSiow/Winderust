# Wind UI 0.28 upgrade and attempted background comparison

Linked raw evidence is archived locally under `benchmark-results/archive-20261028/` and excluded from Git and release artifacts; replay requires that separate archive.

Winderust was upgraded from the former Wind UI 0.24 development revision to
0.28.0. Both release builds and the automated Rust checks passed. **The attempted
background comparison produced zero accepted performance trials.** It supplies
no new CPU-cycle or RAM ranking against the previous build or Iced.

The [portable evidence bundle](../benchmark-results/archive-20261028/benchmark/ui-windui-upgrade-results-20261023/README.md)
preserves both failed attempts and the separate readiness diagnostics. Its
verifier recomputes the failure/provenance checks without launching an app:

```console
python -B benchmark-results/archive-20261028/benchmark/ui-windui-upgrade-results-20261023/verify.py
```

Expected output includes `verified_evidence: true`, `comparison_available: false`
and `accepted_trials: 0`. Evidence verification does not mean that a benchmark
completed.

## Implementation

| Build | Wind UI version | Exact upstream revision | Frozen fixture SHA-256 |
| --- | --- | --- | --- |
| Previous | 0.24.0 development revision | `36b139c66abca0c8c94dec7ed08fdf8ffb2de1d2` | `35721b77ad06a86b61a6137c68747e593b9950fb455998ce88eba19d0c07f8e1` |
| Upgrade | 0.28.0 | `d5c105067d7a31f0f14e388f1fe6394f1a043988` | `69bcd174f2668f8396d68e5395b67083d3527064607672fe243a4192145a5dc9` |

`Cargo.toml` and `Cargo.lock` pin the upgrade. The software renderer and SVG
feature are retained; other locked dependency entries are unchanged. Integration
changes adapt the explicit vertical scroll axis and tooltip text API. The
resident application passes its resolved animation preference to the framework
at startup, preserving On, Off and Follow system. Wind UI upstream source was
not modified or vendored.

The Windows `run_message_loop` function is identical between the
[previous revision](https://github.com/huanfeng/wind-ui-rust/blob/36b139c66abca0c8c94dec7ed08fdf8ffb2de1d2/src/platform/win32/mod.rs#L1821-L1935)
and the
[0.28 revision](https://github.com/huanfeng/wind-ui-rust/blob/d5c105067d7a31f0f14e388f1fe6394f1a043988/src/platform/win32/mod.rs#L1836-L1950).
Its inactive-animation path still blocks on native messages. The
[presence relayout fix](https://github.com/huanfeng/wind-ui-rust/commit/b57769752645d03ac0dbc53bf57d98b9097d19dd)
included in 0.28 stops an exited presence node from repeatedly requesting layout;
that feature postdates the previous pinned revision. This is not evidence of a
lower cold-background cycle floor for Winderust.

Both frozen fixtures declare the same Rust compiler, release profile and
`render-smoke,runtime-benchmark` features. The source manifests record necessary
integration changes and test-only changes; this is an integration upgrade,
without a reproducible-build or isolated-framework claim. The new fixture is
128,512 bytes larger on disk (12,776,448 versus 12,647,936 bytes). Executable file
size is not resident RAM. The normal production release was also built separately
without the diagnostic features.

## Attempted measurement

The frozen design specified four fresh processes in old/new/new/old order:
two fixed AB/BA pairs, each with 180 seconds settling and 180 seconds sampled at
1 Hz. It included the main process and its actual recovery helper, with sampler
cost reported separately. Automation was disabled; readiness required the
settings window to be destroyed through the production resident lifecycle.
The 363-second lifetime was relative to readiness, encompassing the previously
observed activity near five minutes of process age.

The first attempt failed before readiness because `EnumWindows` returned FALSE
with last error zero and no callbacks on the available desktop. A separate native
probe showed that a sampler-owned hidden top-level `STATIC` window allowed
successful enumeration. The collector now requires TRUE and an exact callback
for that owned window on every enumeration. It validates ownership and hidden,
disabled state, never shows or activates the control, filters it from target
windows, and destroys it on its creating thread. Its overhead belongs to the
sampler. FALSE/error-zero remains a failure, never proof of an empty target.

A fresh, separately frozen cohort used that collector and the same binaries.
It stopped when the first old-build fixture timed out waiting for its native
background state. Its completion file reports 20,360.6479 ms total runtime,
`ready_to_cleanup_ms: null`, loop success, and the native-state timeout. No
resource samples were accepted in either attempt, and neither was overwritten
or resumed. No per-trial anchor coverage was returned by the failed fixture.

Separate short readiness diagnostics then launched both frozen builds with a
1,000 ms ready-relative lifetime. Both exited with code zero, produced no
readiness file and recorded the same 20-second native-state timeout. Those
diagnostics are not replacement performance trials. The collector correctly
rejects them despite their zero process exit codes.

Readiness requires a positive window layout observed by the window callback,
successful tray health permitting hide, and confirmed native-window destruction.
The artifacts do not distinguish a missing callback/layout from unavailable tray
readiness. They do not establish a Wind UI regression or the cause of the timeout.

The second attempt's prelaunch host gate passed its physical-memory and true
commit limits. Host CPU averaged 32.83% of the machine during that gate, under the
previously selected observe-only CPU policy. This is shared-host context, not
application CPU. Both recorded power-plan postflights were unchanged. No energy,
GPU, interaction, enabled-scheduler or Iced measurement was collected here.

## Validation and remaining limits

- `cargo test --locked`: 885 passed, 18 ignored, zero failures. Two helper tests
  now wait for explicit test-child readiness before exercising the unchanged
  production shutdown deadline; the former failures also reproduced with the
  old test binary.
- Strict Clippy passed for default targets and for
  `render-smoke,runtime-benchmark`. Formatting, legacy-name and architecture
  ownership checks passed. Both feature and normal production release builds
  completed.
- All 53 resource-collector tests passed, including real hidden-anchor lifecycle
  and native process-counter checks. The counter test holds its owned child
  alive explicitly instead of racing a fixed sleep.
- The render matrix saved 118 frames. All decoded with expected dimensions and
  nonempty content. Initially 116 cases completed cleanly; two reported recovery
  helper shutdown timeouts after saving their frames. Both affected cases passed
  targeted rechecks after compilation finished. The original failures remain
  recorded; the initial matrix was not wholly successful.
- Both old and upgraded visible lifecycle checks stalled before the first
  scripted navigation click. Interactive navigation, close/reopen, tray clicks
  and Explorer-restart behavior are unverified in this session. The user chose
  to continue with this limitation reported.

The earlier
[current-load Wind UI/Iced comparison](ui-settled-idle-current-load-20261023.md)
retains its own versions, timings and limits. Its observations cannot be relabeled
as 0.28 results or pooled with this attempted cohort. No claim that the upgrade
matches or surpasses Iced on idle cycles is supported by the new evidence.

## Newer upstream releases during validation

A final live GitHub API check found
[0.29.0](https://github.com/huanfeng/wind-ui-rust/releases/tag/v0.29.0), published
2026-10-23 at 11:24:43 UTC, and
[0.30.0](https://github.com/huanfeng/wind-ui-rust/releases/tag/v0.30.0), published
at 13:23:26 UTC. The latter peels to
`d9792f0bf8f5cc7efead985dd07696476461b135`.
**0.28 is the validated target of this upgrade, not the latest upstream release.**

The [0.28-to-0.30 changes](https://github.com/huanfeng/wind-ui-rust/compare/v0.28.0...v0.30.0)
include a broad theme/sizing revision, source API changes and additional input
widgets. The Windows platform file remains the identical Git blob
`88a21b0ecddd7db0d1a08c2e8f91b8a3386a89d9`, including its message loop. This
review identifies no new native idle-wait fix that explains or resolves the
current readiness failures. Upgrading to 0.30 needs separate integration and
render/lifecycle validation; none of the 0.28 build or render results validate it.
