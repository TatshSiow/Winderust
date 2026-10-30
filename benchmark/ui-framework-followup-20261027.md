# Wind UI and Iced: background application follow-up

Linked raw evidence is archived locally under `benchmark-results/archive-20261028/` and excluded from Git and release artifacts; replay requires that separate archive.

Measured and checked on 27 October 2026. This follow-up tests whether the earlier
Wind memory advantage survives an Iced application with no settings window,
checks actual Windows UI Automation output, and audits the locked dependencies.
It evaluates Winderust without assigning a migration-cost penalty to either UI.

**The daemon follow-up produced zero accepted formal trials.** The actual
accessibility and dependency checks completed. No universal framework winner
is established by the earlier measurements. Wind used less private memory in
the current retained-hidden Iced comparison; enabled-engine cycle results were
mixed and the CPU-package power differences were too small and confounded to
establish application energy savings.

## New background comparison

All four separately frozen attempts stopped during pilots. None reached the
formal eight-trial comparison or its bracketing baselines. Three standalone
Wind pilot captures were accepted across those attempts; no matched Iced daemon
pilot passed all gates. Ten-second pilot data are excluded from performance
conclusions. These failures do not rank framework CPU or energy efficiency.

| Attempt | Stop reason | Accepted formal trials |
| --- | --- | ---: |
| Original daemon | Iced deadline exit stalled; separate worker heartbeat replacement crash | 0 |
| Assisted | Conservative launch-based sampling guard rejected the Wind pilot | 0 |
| Larger lifetime margin | Iced post-capture wait returned before its strict wake threshold | 0 |
| Corrected wait | Summary replacement denied after collector launch; Iced native readiness failed on an unreadable worker heartbeat | 0 |

The earlier current-engine comparison remains the usable quantitative evidence:

| Condition / pair | Wind cycles/s | Iced cycles/s | Wind private commit MiB | Iced private commit MiB | Wind package W | Iced package W |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Disabled / 1 | 47,770 | 100,155 | 18.301 | 28.363 | 9.976896 | 9.985384 |
| Enabled unmatched rule / 1 | 36,877,102 | 31,728,762 | 18.301 | 28.980 | 10.013718 | 10.011167 |
| Enabled unmatched rule / 2 | 23,763,515 | 28,944,407 | 18.320 | 28.520 | 9.971807 | 9.976352 |

Those are six accepted captures from a separate, interrupted October 25 cohort:
Wind absent settings window versus Iced retained hidden window. Wind private
resident RAM was approximately 6.1-6.3 MiB versus Iced 11.2-16.0 MiB. Enabled
cycles reverse direction between pairs. Package differences of -8.49, +2.55 and
-4.55 mW do not isolate app power under the observed host load. The new daemon
attempts do not resolve the lifecycle asymmetry in those memory comparisons.
See the [current-engine report](ui-framework-current-power-20261025.md) for raw
counter definitions, scope, failures and the separate rejected continuation.

Both fixtures use software rendering, Rust 1.94.1, Thin LTO, one codegen unit,
the same settings and 112 byte-identical current engine Rust files. Wind uses
Wind UI 0.28.0; Iced uses stable 0.14.0. Their renderer/dependency configurations,
presentation code and executable code generation still differ.

The Wind executable is unchanged from the current-engine comparison. The new
Iced fixture opts into `iced::daemon`, publishes the existing hidden-engine
state, starts with no settings window and suppresses the historical tray retry
that depends on its settings HWND. Its engine and lockfile remain unchanged.
The adapter source and diff accompany the evidence.

**Tray functionality differs:** Wind retains its resident tray host; the Iced
daemon fixture has no persistent tray host. This tests an Iced resource floor,
not a fully equivalent shipping tray application. Any memory or cycle result
must retain that qualification. Neither result establishes the minimum possible
overhead of its framework.

The predeclared formal protocol is two AB/BA pairs per condition, eight captures
in total, with 60 seconds warmup and 120 seconds sampling at 1 Hz. Conditions are
automation disabled and Process Priority enabled with a nonexistent executable
rule. Four identical external idle workers accompany each capture. Native
identity/state/restoration and cooperative worker exits are required. The latter
condition measures process observation and rule evaluation, not active process
mutations or every scheduler. No-app 120-second baselines were planned to bracket
the cohort; these attempts stopped before those phases.

Main process plus observed recovery-helper cycles and private memory exclude the
external workers, collector and launcher. Package watts include all software on
the CPU package, including those observers. CPU time can round to zero while
process cycles remain nonzero. Cycles are OS-reported execution accounting,
not retired instructions or watts. See the
[Microsoft cycle-counter contract](https://learn.microsoft.com/en-us/windows/win32/api/realtimeapiset/nf-realtimeapiset-queryprocesscycletime).

The host runs under the current load selected by the user. There is no quiet-host
claim, outlier deletion, baseline subtraction, cycle-to-watt conversion,
per-application wattage, wall-power measurement or battery-life prediction.
Samples within one capture are correlated observations, not independent trials.
The sampler gap limit remains 2.5 seconds. Other software and temperature or
frequency variation can affect process cycles and package readings.

## Preserved failures and exit assistance

All attempts have separate immutable directories. Pilot captures are excluded
from formal comparisons, and none are pooled with the October 25 measurements.

- The original daemon pilot accepted Wind's disabled capture, then Iced failed
  to finish its deadline exit. One external worker separately crashed while
  replacing its heartbeat file. A later exact-identity `WM_NULL` diagnostic
  allowed the Iced process to finish normal runtime/helper cleanup. That proves
  wake-sensitive exit progress, without identifying an upstream defect.
- The first assisted pilot rejected Wind because the conservative process-launch
  cleanup bound did not leave enough startup margin. Its app finished normally;
  all four workers later passed native restoration checks and exited normally.
- The next pilot used a declared 48-second margin for both fixtures. Wind passed;
  the Iced helper refused to post before its observed-readiness time threshold.
  The installed Python 3.9 Windows implementation truncates process waits to
  integer milliseconds, a plausible early-return mechanism. The actual offset
  was not recorded. No message was posted by that helper.
- The separately declared corrected helper adds 20 ms to that post-capture wait.
  It retains the strict clock, exact process/thread identity, one-message maximum
  and full cleanup checks. A fractional-clock test models the truncation. Its
  cohort accepted Wind's first pilot, then the parent failed to replace its
  summary after launching the Iced collector. That collector failed native
  readiness because worker 2's heartbeat was unreadable (Errno 13), then timed
  out waiting for Iced's exit. No raw Iced sampling or formal trials passed.

The atomic summary replacement was denied with WinError 5. A live monitor was
reading that file without explicit delete-sharing; a reader or antivirus
conflict is plausible, but no handle trace identifies the owner. The failure
was after collector launch, not before it. A later harness-only fix retries
transient file-sharing errors within 250 ms and always waits for a launched
collector when progress persistence fails. That fix was not used in these
captures. No further cohort was run after it.

The final collector uses fixture lifetime `warmup + duration + 48 seconds` for
both apps. Sampling must end before **process launch plus that lifetime**,
a conservative lower bound on cleanup because the fixtures start their timers
after readiness. This prevents sampling an engine that already started cleanup.
Only after capture and the observed-readiness-plus-lifetime-plus-one-second
threshold may the Iced helper send one `WM_NULL`, if the same app is still alive.
That reference is not a measurement of the exact application timer activation.
No assistance occurs during warmup, sampling or the failure/finally path. Accepted
captures still require normal exit and complete native/runtime/helper cleanup.
An assisted result does not demonstrate unassisted production lifecycle behavior.

The first stalled Iced process later exited after one exact-identity diagnostic
wake and wrote successful runtime/helper completion. The assisted Wind guard
failure and the larger-margin Iced wait failure received separate native
restoration verification followed by cooperative worker exit. No agent forced
process termination or changed failed-capture acceptance.

For the last failed cohort, a later read-only audit found the original collector,
Iced app and four workers all absent, and the active plan matched original
Balanced. No stop/wake action was performed in that audit. Their exit codes and
full runtime/helper restoration were not recovered: Iced had no completed
`finished.json`. Absence and a matching plan are point-in-time observations,
not a successful cleanup or performance gate. A subsequent
[executable inventory](../benchmark-results/archive-20261028/benchmark/ui-framework-followup-final-inventory-20261027.json) at
13:23 Taipei time (05:23 UTC) enumerated 267 processes and found no Winderust or
benchmark-worker basename candidates, including same-executable recovery helpers,
with no query errors or inaccessible candidates. This later read-only audit is
an adjacent companion outside the ZIP manifest; it does not recover past cleanup
proof.

## Actual accessibility observations

Both visible settings fixtures were inspected through Windows UI Automation.
Each exposed exactly one generic `ControlType.Window` root named `Winderust` and
zero descendants in Raw, Control and Content views. Each exposed only Window
and Transform patterns; neither exposed semantic settings controls, labels or
actions. Both inspectors completed without errors or truncation, and both apps
exited normally. The initial Iced schema-mismatch attempt is preserved separately
and is not counted as an accessibility observation.

This is an accessibility gap in these builds. It does not establish that visual
Tab navigation is broken, or describe every framework version/configuration.
Narrator, physical keyboard use, IME and the full accessibility experience were
not tested. Built-in framework-painted controls do not automatically provide
Windows UI Automation semantics.

## Locked dependency audit

The actual locked configurations were scanned with official cargo-audit 0.22.2
and a fixed RustSec database revision, with raw reports, Windows feature trees
and an independent official registry-index check retained. The tool archive's
published SHA256 was verified. Production and frozen Wind manifests/locks agree;
the new Iced daemon retains the audited Iced lock.

| Finding | Wind configuration | Iced configuration | Interpretation |
| --- | --- | --- | --- |
| crossbeam-epoch 0.9.18, RUSTSEC-2026-0204 | Present in build/proc-macro path | Same | Shared maintenance item; no demonstrated application trigger |
| Unmaintained ttf-parser 0.25.1 | Inactive selected features | Active | Dependency maintenance difference |
| Unmaintained rustybuzz 0.20.1 | Absent affected version | Active | Dependency maintenance difference |
| lru 0.16.4 unsound notice | Absent affected version | Inactive optional wgpu path | Not demonstrated exposure in this software renderer |
| Tokio 1.52.3 unsound notice | Absent affected version | Active Tokio, affected Unix/net path unavailable | Not a demonstrated Windows exposure |
| Yanked clipboard-win 5.4.1 | None found | Active | Yank is a maintenance signal, not itself a vulnerability |

All 283 Wind and 383 Iced registry lock entries were matched to exact versions
and checksums using 388 distinct saved official index responses. Wind had zero
yanked entries and Iced one. Raw counts alone do not establish a security winner.
No dependencies were upgraded and no exploitability claim is made. Published
advisories do not cover every unsafe-code or native-platform defect.

The shared advisory is
[RUSTSEC-2026-0204](https://rustsec.org/advisories/RUSTSEC-2026-0204.html).
Detailed paths, other advisory links, tool/database provenance and offline replay
are in the evidence's security report. Its portable mode deliberately excludes
tool binaries and checks recorded tool metadata, while retaining raw report,
lockfile, feature-tree and registry-response integrity checks.

## Architecture and maintenance decision

| Winderust concern | Assessment |
| --- | --- |
| Background engine correctness | The same policy/controller engine and safety checks are shared. Neither GUI's state model makes a scheduler algorithm inherently cheaper. |
| UI programming model | Wind offers reactive signal bindings; Iced uses typed messages, updates, tasks and subscriptions. Both support separating pure application logic for unit tests. |
| Windows tray integration | The Wind app has working resident integration but depends on pinned private upstream identities in its native adapter. Iced's tested daemon still needs a persistent tray integration before product parity. |
| Current private memory | Earlier retained-hidden comparisons favor Wind; interpret the new daemon result separately with its tray limitation. |
| Enabled-engine cycles | Earlier current-engine pairs reverse direction; no consistent winner. The unmatched process-observation workload is shared engine work. |
| Energy | CPU-package counters under current host load do not isolate application power. A reliable framework power winner remains unproved. |
| Accessibility | Neither tested settings UI exposes semantic controls through UIA. Both need work if assistive technology support is a requirement. |
| Native lifecycle reliability | The new native reopen attempts did not pass external validation. Wind's tray guard refused a transition; Iced's request counts are not native proof. |
| Dependency maintenance | Wind's selected Windows configuration has fewer active notices; both share a build dependency advisory. Keep feature-path qualifications. |
| Latest releases | The tests pin Wind 0.28.0 and Iced 0.14.0. Wind 0.30.0 was checked as the latest release on October 27, but was not performance-tested here. |

The separate lifecycle attempts collected zero accepted native cycles because
desktop cursor access failed before sampling. The shared collector was then
fixed to query/save the cursor only when visible activation needs it. Stable
absent-window captures no longer request cursor access; visible foreground and
input checks remain strict. The failure does not prove that Windows was locked.

The official release references are
[Wind UI 0.30.0](https://github.com/huanfeng/wind-ui-rust/releases/tag/v0.30.0) and
[Iced 0.14.0](https://github.com/iced-rs/iced/releases/tag/0.14.0).
Iced explicitly supports a
[zero-window daemon](https://docs.rs/iced/0.14.0/iced/fn.daemon.html).

For a Windows-only settings UI with the current low-memory implementation,
**Wind UI is a reasonable Winderust choice**. That recommendation does not rely
on migration cost. Its measured application-memory advantage is useful, while
its private native integration and accessibility gap need explicit maintenance.

**The evidence does not establish that Wind UI provides lower scheduler CPU or
lower power than Iced.** Both can keep the engine independent of visible UI.
The Iced zero-window alternative remains unmeasured under a fully valid matched
protocol. A claim that Wind is inherently superior, or that Iced cannot match
its memory, exceeds these results.

The strongest next product checks are semantic accessibility, native tray and
reopen/error paths, and shared-engine process-observation profiling. Treat any
proposed scan/cache optimization as an experiment; preserve exact target
identity, protected-process filtering and restoration. No energy saving is
promised by source inspection alone.

## Verification and evidence

The [portable ZIP](../benchmark-results/archive-20261028/benchmark/ui-framework-followup-results-20261027.zip) includes separate
failed cohorts and frozen sources, daemon adapter provenance, worker correction,
exit/cleanup diagnostics, actual UIA observations, native lifecycle failures,
advisory/index evidence and harness-fix tests. It excludes executables, tool
archives/caches and personal tooling. Full original current-engine source is
retained in the adjacent October 25 evidence bundle; the new ZIP records the
three Iced adapter files, patch and matching-engine provenance.

Integrity verification and benchmark acceptance are distinct. The archive's
manifest verifier checks exact payload inventory, byte counts and SHA256s.
The ZIP contains 1,117 payload files (35,342,427 uncompressed bytes) and is
4,247,721 bytes. Its SHA256 is
`eb82c6d54443499d220ae5c1b368d69faf208afc4ed700e0eaf7adffffe9d973`.
Fresh extraction and nine offline replay commands completed with their expected
return codes, including all four cohort rejections and successful security
consistency replay. Thirty offline tests passed and extracted integrity was
unchanged after replay.

Frozen cohort validators must still reject all four incomplete attempts; a valid
archive does not make a failed benchmark successful. The security replay checks
its own raw findings, locked inputs, feature trees and registry responses.

Validation completed:

- Production `git diff --check`, `cargo fmt -- --check`, strict all-target Clippy,
  legacy-name scan and architecture ownership gates passed. `cargo test --locked`
  passed 885 tests, with 18 ignored.
- Canonical benchmark Python tests: 111 passed, including the three cursor-access
  regressions and four file-persistence/child-wait regressions.
- Final isolated helper: 11 tests passed; cohort replay: 13 tests passed;
  snapshot namespace: two tests and fixed-order configuration self-check passed.
- Actual UIA probe gates: five tests passed; native lifecycle sequence validator:
  five tests passed; portable audit verifier: four tests passed; archive integrity
  verifier: three tests passed. These tests are distinct from native observations.
- The isolated Iced daemon release build and two focused Rust tests passed.
  Its strict all-target Clippy reported two existing dead-code errors in the
  historical application's tray-visibility adapter; the separate run allowing
  dead code passed. This is separate from production's strict Clippy success.
- `graphify update .` passed: 23,861 nodes and 60,539 edges; HTML output was
  skipped by its configured graph-size limit.

The archive README provides file-only integrity, frozen cohort, security and
lifecycle replay commands. All four incomplete cohort replays are expected to
return `accepted: false`. Successful manifest/security checks do not override
those rejections.

Production Rust and UI dependency source were unchanged in this follow-up.
The new daemon adapter and worker changes are isolated benchmark fixtures.
Canonical Python collector changes defer cursor access until visible activation,
retry bounded transient summary-write conflicts and wait for an already launched
collector when progress persistence fails. The final two fixes were not measured.

Not checked: latest-Wind performance, full native tray/Explorer restart and
reopening parity, full assistive-technology/input coverage, all enabled scheduler
combinations, long-duration memory growth, retired instructions, hardware
wakeups, GPU/wall power and battery life. The daemon tray mismatch and current
host load are material limits on any framework-wide conclusion.
