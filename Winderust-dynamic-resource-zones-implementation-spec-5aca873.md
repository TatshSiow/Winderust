# Winderust — Separate Dynamic Resource Zones from Limit Background Processors

**Document type:** Agent-ready implementation specification  
**Repository:** TatshSiow/Winderust  
**Target branch:** `dev`  
**Inspected baseline:** `5aca8733b4d5f4cdbe6fe40362fede2673397f74`  
**Prepared:** September 22, 2026 — Asia/Taipei  
**Implementation status:** Specification only. No repository files were changed and no implementation tests were run while preparing this document.

> **Objective:** Make background-only processor limiting and foreground/background zoning independently understandable and independently operable, while retaining one CPU-allocation coordinator, the existing recovery machinery, and unrelated Adaptive Engine behavior. Turn zoning off by default in Speed. Do not make a cosmetic UI move while leaving the original runtime dependency intact.

This document defines the requested implementation, not another whole-repository audit. Statements labeled **Current** describe the inspected baseline. **Required** statements define the proposed behavior. New internal names and test names below are implementation targets, not claims that those symbols already exist.

## 1. Implementation decisions

| Decision | Required outcome |
| --- | --- |
| UI placement | CPU Pressure Restraint, Limit Background Processors, and Dynamic Resource Zones are sibling groups under Adaptive Engine → CPU Behaviour. No new top-level page or tab. |
| Enablement | Each policy has its own switch. Zoning must work when background limiting and CPU Pressure Restraint are both disabled. All remain subordinate to the existing application and Adaptive Engine master switches. |
| Setting semantics | Background processor percentage, selection, and custom mask belong to background limiting. Zoning owns a separate share, selection, and custom mask. Toggling either policy never reinterprets the other policy's values. |
| Speed default | Dynamic Resource Zones off. Preserve the other Speed values, including background limiting. |
| Other built-ins | Preserve their existing enabled/disabled intent during this scoped change. Performance remains an explicit follow-up throughput-validation case, not an assumed equivalent CPU-Z result. |
| Automatic zoning activation | Require fresh, eligible foreground and competing background work. Foreground utilization alone must not narrow the foreground set. |
| Both policies enabled | Active zoning takes precedence within Adaptive allocation. When zoning is ineligible, background-only limiting may run under its own rules. Do not compose two conflicting requests for the same target. |
| Native ownership | Keep `ControlOwner::AdaptiveEngine` and the existing CPU-allocation coordinator. Do not add a second native writer, restoration owner, watchdog, or worker. |
| Explicit user policies | Preserve CPU Sets (Soft) > Processor Affinity (Hard) > Adaptive Engine precedence. Do not overwrite explicit rules to complete a zone. |
| Persistence | Preserve existing settings files on load errors. Do not add automatic schema migrations, legacy aliases, or silently infer a custom zone from an unrelated field. |
| Claims and marketing | Zoning is experimental placement/isolation, not a universal speed improvement or a guarantee of exclusively reserved physical cores. |

## 2. Evidence and motivation

### 2.1 User-reported CPU-Z measurements

The user ran CPU-Z using System Default and the unchanged Speed preset:

| Run | Default single-thread | Default multi-thread | Speed single-thread | Speed multi-thread |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 674.1 | 5009.1 | 695.8 | 4907.7 |
| 2 | 684.6 | 5025.5 | 698.9 | 4854.6 |
| 3 | 680.3 | 5001.3 | 691.8 | 4876.4 |
| 4 | 672.8 | 5027.5 | 694.6 | 4904.2 |
| 5 | 683.1 | 4991.2 | 694.6 | 4817.7 |
| **Mean** | **678.98** | **5010.92** | **695.14** | **4872.12** |
| **Speed change** |  |  | **+2.38%** | **−2.77%** |

The user subsequently reported that disabling Dynamic Resource Zones identified it as the main contributor to the multi-thread loss. Raw zones-off results, CPU model, CPU-Z version, run ordering, and effective worker assignments were not supplied. Treat this as **user-reproduced feature-level evidence on one system**, not a measured universal effect or proof that every lost point came exclusively from foreground masking.

No CPU-Z executable-name special case is authorized. The correction must apply to ordinary foreground workloads generally.

### 2.2 Current implementation

At the inspected baseline:

- Speed enables `dynamic_resource_zones_enabled` and `limit_background_processors_enabled`, uses `processor_limit_percent = 75`, and triggers pressure handling at a 35% foreground-or-system threshold. [S1]
- The zoning switch is nested under Limit Background Processors. The same percentage field is displayed as a foreground-zone share with zoning enabled and a background processor limit with zoning disabled. [S2]
- `dynamic_resource_zones_apply` depends on `cpu_allocation_applies`, which depends on the background-limiting switch. The manager's early exit and runtime-required predicate also omit zoning as an independent producer. [S3][S4][S5]
- Foreground zone targets are created before background candidates are selected. A busy foreground task can therefore trigger a smaller foreground allocation without useful competing background targets. [S3]
- Both modes submit discovered targets through one `CpuAllocationManager` and the existing coordinator. The manager's owner-level update releases claims missing from its submitted target list. Two independent updates with disjoint lists would release one another's claims. [S6][S7]
- Mask-to-CPU-Set conversion currently uses processor group 0 and indices below 64. This change must not pretend that a `u64` represents every processor on a multi-group system. [S8]

### 2.3 Windows mechanism limits

Process-default CPU Sets apply to threads without thread-selected assignments; restrictive affinity can take precedence. Complementary process assignments are not equivalent to operating-system-wide core reservation. Do not clear thread-selected sets or widen hard affinity to force the policy to work. [W1]

An empty process-default CPU Set list means no process-default assignment; it is not interchangeable with an explicit list of every currently enumerated processor. Restoration must preserve that distinction. [W2][W3]

## 3. Scope and preservation boundaries

### Required changes

Implement sibling controls, independent configuration and runtime enablement, deterministic allocation-mode resolution, competition-gated zoning, Speed's revised default, explicit status, regression tests, and aligned documentation/locales.

### Keep unchanged unless directly required by this specification

Preserve processor-power values, boost modes, priority values, priority preservation rules, Efficiency Mode settings, I/O/GPU/memory policy, CPU Limiter, App Suspension, standalone CPU allocation rules, startup registration, tray behavior, and diagnostic transport.

Preserve exact process identities, protected-process exclusions, cross-session acquisition rules, suppressed-target handling, controller baselines, pending-release retry state, external-change protection, and watchdog recovery. Existing user restrictions and other owners' claims are not disposable cleanup state. [S9][S10]

### Out of scope

No scheduler replacement, new daemon, automatic application profiler, per-thread affinity controller, topology-wide redesign, new crate, generic policy framework, CPU-Z-specific optimization, dependency purge, or promise of increased scores on every machine.

Do not re-open previously repaired UI, palette, diagnostics, or recovery issues as part of this implementation. `VERIFY-001` remains passed based on the user's earlier Windows test; it was not rerun for this document.

## 4. Settings contract

### 4.1 Minimal additive split

Keep these existing fields in `AdaptiveEngineProcessSettings`:

| Existing field | Meaning after separation |
| --- | --- |
| `cpu_pressure_restraint_enabled` | Enable the existing pressure-driven priority/efficiency policy. |
| `limit_background_processors_enabled` | Enable background-only processor limiting. |
| `dynamic_resource_zones_enabled` | Independently enable zoning. It must no longer depend on the limiter's enable flag. |
| `processor_limit_percent` | **Background-only** share for percentage-based background selection. Keep this serialized name to avoid an unnecessary rename; document its stable meaning. |
| `background_processor_selection` | Background limiter's own selection strategy. |
| `specific_processors` | Background limiter's own custom processor selection. |
| `cpu_allocation_method` | Background limiter's soft/hard method. Zoning does not read or change it. |

Add a dedicated `dynamic_resource_zone_settings` block with the following typed fields:

| New block field | Type | Default | Meaning |
| --- | --- | --- | --- |
| `foreground_share_percent` | `u8` | `75` | Share used to derive the complementary background selection for least-used zone strategies. Valid range: 1–99. |
| `background_processor_selection` | Existing `BackgroundProcessorSelection` enum | `LeastUsed` | Zoning's own background-zone selection strategy. |
| `specific_processors` | `Vec<u8>` | Empty | Zoning's own explicit background-zone indices for the Custom strategy. |

Use a dedicated `DynamicResourceZoneSettings` type. Do not make runtime zoning fall back to the background limiter's percentage or masks.

At the serialized input boundary, retain whether the new block was absent, using an optional wire field or equivalent validated deserialization. For validated, current settings, both editors and runtime must see a concrete zone configuration. Do not scatter missing-block handling through the policy loop.

### 4.2 Representative configuration subtree

This is the complete proposed allocation-related subtree for **new Speed settings**, not a replacement for the application's full TOML file. All unrelated sections remain present and unchanged.

```toml
[adaptive_engine_process]
limit_background_processors_enabled = true
dynamic_resource_zones_enabled = false
cpu_allocation_method = "cpu_sets_soft"
background_processor_selection = "least_used"
processor_limit_percent = 75
specific_processors = []

[adaptive_engine_process.dynamic_resource_zone_settings]
foreground_share_percent = 75
background_processor_selection = "least_used"
specific_processors = []
```

The new block does not contain a second enable Boolean. The existing `dynamic_resource_zones_enabled` remains the single source of enablement.

### 4.3 Existing configurations: no silent reinterpretation

The repository explicitly prohibits unsolicited migrations and legacy aliases. Apply these input rules instead. [S9]

| Incoming configuration | Required behavior |
| --- | --- |
| Zoning disabled; new block absent | Accept the existing background-limiter settings unchanged. Initialize the new, inactive zone configuration with its documented defaults in memory. Do not rewrite the file on load. |
| Zoning enabled; new block absent | Reject with an actionable settings error. The old foreground share and selection must not be silently inferred or silently changed. Preserve the original file and existing failed-load persistence guard. |
| New block present | Validate its own values independently of the limiter settings. |
| Invalid block or unusable explicitly selected mask | Reject the invalid input or show an inactive/error state before allocation; never substitute all processors or another policy's mask. |

The error should explain that explicit zone settings are required and that the user can either disable zoning or provide the new block. Documentation may explain how to **manually** copy the previous zone share/selection into the new block. Do not add an automatic converter under this task.

This validation must reach root settings, the Battery profile, and every saved Adaptive Engine preset. A hidden incompatible preset is not safe to defer until application.

This is a deliberate, limited pre-release configuration cutover: previously enabled zones lacking explicit independent settings require user correction. It is preferable to silently changing a custom partition. Ordinary disabled-zone inputs keep their existing limiter meaning.

Validate before installation into `SettingsEditor`, after explicit preset edits, and before producing runtime allocation intents. Keep tests that parse the configuration directly consistent with the same contract; do not validate only one load call site.

### 4.4 Shared detection versus independent policy settings

Retain the existing common sampling/reaction interval and existing pressure/recovery inputs. Sharing observations and timing is intentional; borrowing another feature's enable switch is not.

The existing foreground-or-system pressure threshold, background-app demand threshold, candidate cap, and relevant recovery timings can remain common Adaptive Engine detection settings for this change. Move or label their UI presentation as shared when multiple policies read them. Do not create extra polling loops or duplicate configuration values solely to make the cards look separate.

Document that the background-app demand metric currently normalizes against one logical processor, whereas foreground/system pressure uses its existing aggregate metric. Keep units unchanged during this refactor; a separate units change would invalidate benchmark comparability. [S3][S4]

### 4.5 Presets and profiles

- Speed must set `dynamic_resource_zones_enabled = false`; its new inactive zone block retains the previous 75%/LeastUsed values for explicit opt-in.
- Performance retains its existing enabled zoning intent and receives an explicit zone block. Its competition-gating behavior changes as specified here, but do not claim measured Performance gains from the user's Speed result.
- Balanced and Power Save remain zoning-disabled.
- Do not change a preset's unrelated values, exclusions, application/Adaptive master switches, or the separate Background Efficiency feature.
- Capture, comparison, Apply, View Built-in, Edit Custom, Use Current, and preset serialization must include the zone block.
- Applying a preset copies both policies' settings independently. Disabling zoning in the live draft must not overwrite the dormant zone block or background settings.
- Keep policy values AC/Battery-specific and preset collections shared through the existing settings boundary. Do not classify zone policy as a global UI preference. [S1][S11]

## 5. UI contract

### 5.1 Layout

Keep the existing CPU Behaviour tab. Use the existing shared setting-group, switch, numeric editor, mask selector, scrolling, and validation controls.

```text
Adaptive Engine → CPU Behaviour

Shared detection and timing
    Existing pressure thresholds, reaction interval, candidate cap,
    and recovery controls that genuinely affect multiple policies

CPU Pressure Restraint
    Independent enable switch
    Existing policy-specific content

Limit Background Processors
    Independent enable switch
    Background processor selection
    Background processor limit, for least-used strategies
    Allocation method
    Background custom mask, for Custom

Dynamic Resource Zones (Experimental)
    Independent enable switch — off in Speed by default
    Throughput warning
    Zone background processor selection
    Foreground zone share, for least-used strategies
    Zone background custom mask, for Custom
    Current eligibility/status and assigned processor counts
```

Do not duplicate an editable shared field in both cards. A shared label or help text must state which policies use it. Turning CPU Pressure Restraint off must not make a shared field inaccessible while zoning still needs it.

### 5.2 Required UI behavior

- Give zoning its own expand/collapse state in both the live and preset editor. The current two-group state needs a third policy-group slot; do not reuse an existing index. [S2]
- The limiter's allocation-method selector no longer disappears when zoning is enabled. It remains its saved fallback method.
- The limiter's percentage label always means background processor limit. It must never become a foreground-zone label.
- The zone percentage label always refers to zoning. Keep its value when a fixed strategy temporarily hides it.
- Built-in preset previews remain read-only. Custom-preset edits stay in their own draft until saved/applied.
- Policy changes follow the current SettingsEditor/runtime publication boundary; a purely visual expansion does not alter configuration.
- Both enabled cards remain visibly enabled even when zoning supersedes background limiting. Show “Waiting while zoning is active” rather than silently toggling the limiter off.
- Save/Cancel, selected power profile, invalid numeric drafts, and native-dialog completion behavior remain unchanged.

### 5.3 Required explanatory text

Use equivalent text in both existing locales; preserve their translation conventions.

**Zoning help:** “During CPU pressure with eligible competing background work, place managed foreground and background workloads in complementary CPU Sets. This can reduce foreground multi-thread throughput.”

**Zoning limitation:** “CPU Sets control placement, not exclusive ownership of physical cores. Explicit process/thread restrictions and Windows scheduling still apply.”

**Background limiter help:** “Restrict eligible background workloads without assigning a foreground CPU set. Foreground access is not narrowed by this policy.”

**Share help:** “With Least used processors (All), this value determines the approximate foreground share. Other selection strategies can produce different total counts; inspect the displayed masks and logical-processor counts.”

Avoid “guaranteed reserved cores,” “always faster,” or a percentage-performance guarantee.

## 6. Runtime enablement and scheduling

### 6.1 Policy gates

Let `A` mean the application and Adaptive Engine master gates permit automation, `P` mean CPU Pressure Restraint enabled, `L` mean Limit Background Processors enabled, and `Z` mean Dynamic Resource Zones enabled.

```text
Adaptive process-policy work required = A AND (P OR L OR Z)
Priority/efficiency restraint allowed = A AND P
Background-only allocation allowed = A AND L
Zone allocation allowed = A AND Z
```

The real eligibility conditions still apply after these enable gates. An enabled setting is not a claim that allocation has succeeded.

Update all relevant dependencies, not only the final zoning Boolean:

| Area | Required change |
| --- | --- |
| `adaptive_engine_process_required()` | Include independent zoning. |
| Manager early disabled return | Do not return disabled when Z is the only enabled policy. |
| Pressure helper early returns | Evaluate pressure when zoning alone requires it. |
| Runtime worker lifetime | Consider all configured AC/Battery profiles for future work; run policy only for the active profile. |
| Observation requirements | Request process identity, foreground group, visible-window classification, CPU demand, and per-processor load when actually needed by the active producer. |
| Event invalidation | Settings, power-source, process appearance, and foreground changes must invalidate the relevant cached decision. |
| Priority-assist gates | Zoning-only must not turn on priority, I/O, GPU, memory, or Efficiency Mode mutations. |
| Cleanup | Pending allocation restoration remains schedulable even when all feature/master switches are off. |

Use the existing reaction interval and scheduler. When all producers are off and no managed/pending state remains, preserve worker dormancy. Do not repeatedly push a not-yet-due deadline forward on unrelated wakes. [S5][S10]

### 6.2 Separate the pressure signal from the policies it enables

Compute the shared observation-derived pressure signal once. Then derive policy-specific eligibility.

Do not use `cpu_pressure_restraint_active` as a synonym for “some allocation is active.” That status feeds other Adaptive behavior. It must remain true only when the pressure-restraint policy itself is applicable.

Retain the existing startup/focus grace behavior where appropriate, but express allocation grace independently of whether Process Priority is enabled. Turning off priority tuning must not unexpectedly change zoning grace or enable a new mutation family.

## 7. Allocation-mode resolution

### 7.1 Decision table

| L | Z | Zone prerequisites satisfied | Desired Adaptive allocation mode |
| ---: | ---: | --- | --- |
| 0 | 0 | Any | None; release Adaptive allocation claims through the coordinator. |
| 1 | 0 | Any | BackgroundLimit, when its existing eligibility conditions pass. No foreground targets. |
| 0 | 1 | Yes | Zones. |
| 0 | 1 | No | None, with an explicit zoning waiting/unavailable reason. |
| 1 | 1 | Yes | Zones; background limiter waits without losing its configuration. |
| 1 | 1 | No | BackgroundLimit when independently eligible; otherwise None. |

“Zone prerequisites not satisfied” is not permission to apply an unknown or stale mask. Errors and unavailable observations remain distinct from confirmed absence of demand.

### 7.2 Real competition is a prerequisite

Before generating a foreground zone target, require all of the following:

1. A known, eligible foreground process group with validated identity metadata and no explicit rule that supersedes the proposed foreground allocation.
2. Fresh CPU observations showing the existing shared pressure condition, outside the applicable startup/focus grace.
3. At least one **current eligible background candidate**, outside the foreground group, whose measured demand meets the background threshold and which survives exclusions, protection checks, and higher-owner filtering.
4. A representable, usable zone selection with nonempty foreground and background masks.
5. Successful or verified unchanged background placement for at least one eligible background target in the current allocation generation before foreground narrowing is considered effective.

A high total or foreground CPU percentage, an idle background process, a remembered selected PID, a UI checkbox, or a candidate rejected by the coordinator is insufficient on its own.

Retain the existing foreground-descendant and visible-window classification rules; do not broaden candidate targeting to services or unrelated helper processes as a shortcut. Exact-process validation still occurs at the controller boundary.

### 7.3 Reclaim foreground capacity when competition disappears

When competing background targets exit, become ineligible, are all overridden, or cease to provide the required fresh demand, remove the zoning foreground intent at the next scheduled reconciliation. Do not keep the foreground partition merely because CPU-Z's foreground load remains high.

Existing minimum-hold/recovery timing may retain background-only restraint where that policy permits it. It must not, by itself, prove that a foreground restriction is still useful. Unavailable demand is not proof of fresh competition.

Disabling zoning, changing its configuration, switching profile, or losing the valid foreground group immediately invalidates the old zone decision. Use the existing cleanup/retry path; do not wait for a background minimum-hold timer before withdrawing the obsolete foreground intent.

## 8. Mask semantics and topology boundaries

### 8.1 Preserve the current selection strategies, but give them separate storage

To avoid an unrelated selection rewrite, use the existing background selection strategies separately for each policy.

Let `A` be the complete representable processor mask for the supported allocation domain. For zoning, choose the background mask `B` using the zone's own strategy and calculate `F = A & !B`.

For LeastUsed across all processors, with foreground share `p` and `N = popcount(A)`:

```text
background_percent = 100 - p
background_count = ceil(N × background_percent / 100)
foreground_count = N - background_count
```

Select the background indices deterministically using the existing load-aware ordering, then form the complement. Both counts must be nonzero. A representable domain of one logical processor cannot support disjoint zones and must produce an unavailable result, not an empty assignment.

For the existing P-core/E-core least-used strategies, apply the percentage to that strategy's candidate pool, as the current selection helper does; form the foreground complement in `A`. This means `p` is not necessarily the foreground percentage of the entire machine. Display the actual counts rather than advertising an exact global split.

For fixed P/E/no-SMT/custom background selections, use the explicit zone-owned background mask. The share field is inactive and hidden, but retained. Do not borrow the background limiter's custom indices. [S4]

### 8.2 Boundary rules

| Input or condition | Required handling |
| --- | --- |
| Zoning share 0 or 100 | Reject as invalid; the UI accepts 1–99. Use the enable switch to request no zoning. |
| Limiter share 100 | Preserve the existing no-narrowing behavior for percentage-based background limiting. It does not reserve a mandatory zone processor. |
| Empty mask / mask equal to A | No valid partition; do not submit an empty foreground or background assignment. |
| Missing requested P/E pool | Mark the requested strategy unavailable; do not silently switch to All. |
| Invalid custom indices | Reject unsupported indices before shifts or native calls. |
| Hybrid CPU or SMT | Count logical processors, not performance-equivalent cores. Do not claim physical-core isolation or proportional throughput. |
| Topology/domain cannot be represented safely | Decline zoning with an explicit reason. Do not truncate other processor groups into an apparently complete split. |

Retain the existing native API boundaries. Supporting arbitrary processor groups or physical-core isolation is not part of this task. If existing topology discovery cannot establish a complete safe domain, the conservative result is unavailable zoning, not an adapter rewrite hidden in this patch. [S8]

### 8.3 Cache and rebalance behavior

Retain the existing bounded rebalance cadence and deterministic tie-breaking. Invalidate a cached selection when the effective mode, profile, selector, share, custom mask, eligible topology, or relevant process identity changes.

A cached mask created for background-only 75% must never be reused as the 25% background zone merely because the previous mask has the same number of bits. Cache keys must reflect semantic inputs, not only cardinality.

Do not add periodic native writes for an unchanged effective allocation. Share the CPU-load sample across producers within the same runtime pass.

## 9. One coordinator, one Adaptive claim lifecycle

### 9.1 Composition boundary

Use this flow:

```text
Shared observations and exact candidate metadata
                   |
        independent policy eligibility
          /                      \
BackgroundLimit intent       Zones intent
          \                      /
           deterministic mode resolution
                   |
       one Adaptive target lifecycle
                   |
       existing CpuAllocationCoordinator
                   |
      existing Windows adapter + recovery
```

Keep both policies under `ControlOwner::AdaptiveEngine`. Separate policy-specific role/status metadata does not require a new recovery owner.

The final intended target set contains at most one Adaptive allocation request per exact process key. Resolve mode and role before submission; do not intersect two competing masks as an accidental arbitration strategy.

### 9.2 Do not call the whole target-replacement API twice

**Current:** `update_discovered_targets()` calls `apply_targets()`, which releases owner claims missing from that input before applying the new list. [S6]

Therefore, do not submit the background list and foreground list as two independent owner updates. The second update would treat the first list as stale. Likewise, two `CpuAllocationManager` instances using the same owner are not an acceptable shortcut.

Extend the existing typed facade narrowly if per-target outcomes or phased application are required. Keep one generation-aware lifecycle for the combined Adaptive intent. Internal ordered operations may have multiple steps, but not two competing “replace all this owner's targets” producers.

### 9.3 Ordered application and partial failures

Native mutations across processes are not a global atomic transaction. Preserve per-process transactions and make multi-target outcomes explicit.

- Prepare one coherent decision generation with exact foreground/background roles.
- Establish eligible background zone requests before new foreground narrowing. `Shadowed`, `NoUsableTarget`, access failure, suppression, and exit do not count as established zone coverage.
- Count `Unchanged` only when it represents a verified effective request matching the intended current generation, not merely a submitted Boolean or an unrelated aggregate managed count.
- If no background target is successfully placed, do not narrow foreground targets. Withdraw a now-obsolete foreground intent through the coordinator.
- If foreground application is partial or fails, report Degraded, withdraw the unsuccessful zone plan, and resolve to the independently eligible background fallback or no allocation. Do not leave a foreground restriction behind under an “inactive” label.
- Preserve unresolved release/compensation state for existing bounded retry. A diagnostic failure must not remove cleanup responsibility.
- Do not hide failures by repeatedly switching to the other mode on every tick. Use the existing failure/backoff machinery and invalidate it only for a relevant new identity/configuration/eligibility generation.

A narrow result-bearing facade can expose exact-key outcomes needed by the policy. It must not expose controller baseline ownership to the UI or duplicate native state in a feature manager.

### 9.4 Handoffs and restoration

Preserve external precedence and the original captured baseline throughout BackgroundLimit ↔ Zones, AC ↔ Battery, and foreground-group changes. Replacing an Adaptive request must not recapture an already constrained mask as the original baseline.

When zoning is disabled, remove only the now-unneeded Adaptive role intents. If a background limiter or higher-priority owner remains applicable, reconcile to that owner. Do not call controller shutdown or a blanket all-owner release to disable one subfeature.

If current native state no longer matches the expected owned state, retain the existing external-change protection. Restoration is not an opportunity to force a new all-processors assignment. An originally empty default CPU Set list must be restored as empty; an originally explicit list must be restored exactly. [S7][S10][W2][W3]

## 10. Status and observability

Add compact policy status to the existing Adaptive snapshot/status segment rather than a parallel runtime status owner.

| State | Meaning |
| --- | --- |
| Disabled | Zoning's own switch or a master gate is off. |
| Waiting | Enabled, but fresh competition, foreground identity, grace completion, or pressure prerequisites are absent. Include a concise reason. |
| Applying | A coherent generation is being reconciled; foreground narrowing is not yet fully verified. |
| Active | Current generation has verified applicable foreground and competing background placement. |
| Overridden | Explicit allocation rules prevent the requested zoning. No forced override. |
| Degraded | Application or release is incomplete; show the retained error and pending cleanup. |
| Unavailable | Unsupported domain, invalid pool/mask, or required observation/API capability is unavailable. |

Publish the effective mode, actual foreground/background target counts, masks or selected logical-processor counts, and a concise reason. Distinguish configured share from realized counts.

Use the existing Action Log category with policy-specific descriptions. Record meaningful mode transitions, changed allocations, and failures; do not emit a duplicate success line every 500 ms. Preserve existing log modes, bounded history, summary throttling, and asynchronous diagnostics.

## 11. File-level implementation map

| File or area | Required work |
| --- | --- |
| `src/config/settings.rs` | Add independent zone parameter type/block, defaults, validation, and profile/preset representation. Keep existing background fields semantically stable. |
| `src/config/storage.rs` | Apply the configuration validation contract through load/import; test absent blocks and preserve original files on errors. |
| `src/application/settings.rs` | Confirm new policy values participate in runtime projection, draft equality, Save/Cancel, preset handling, and AC/Battery behavior. Avoid unnecessary changes to persistence ownership. |
| `src/ui/adaptive_engine.rs` | Add sibling zoning group, independent collapse state, separate field bindings, precise labels, and status. Preserve live versus preset drafts. |
| `src/ui/adaptive_presets.rs` | Set new Speed zoning default off; explicitly initialize every built-in's zone block; include the block in capture/comparison/apply without clearing exclusions. |
| `src/features/winderust_features/adaptive_engine_process.rs` | Separate gates, candidate evaluation, allocation-mode resolution, foreground eligibility, and one combined target lifecycle. |
| `src/features/winderust_features/adaptive_engine_process/policy.rs` | Pure mode/competition/mask eligibility helpers; avoid hiding native writes here. |
| `src/features/winderust_features/adaptive_engine_process/tests.rs` | Unit and manager-level coverage of independent policies, competition, handoffs, and retained behavior. |
| `src/features/cpu_control/cpu_allocation.rs` | Only the narrow per-target outcome/ordering extension needed for safe phased zoning. Preserve shared-manager behavior for other owners. |
| `src/control/cpu_allocation.rs` | Reuse existing ownership/recovery. Add focused handoff/failure tests; do not introduce another coordinator or owner. |
| `src/backend/automation/requirements.rs` | Include zoning in work/observation requirements; preserve disabled-feature cleanup and dormant runtime behavior. |
| `src/backend/automation/runner.rs` and `automation.rs` | Feed shared observations, maintain independent status, and reconcile after producer decisions under the existing scheduler. |
| `locales/en.yml`, `locales/zh-TW.yml` | New or corrected labels/help/status with matching semantics and valid placeholders. |
| `scripts/adaptive_runtime_benchmark.ps1`, benchmark fixtures | Emit explicit current zone settings and validate the real payload; retain the existing zones-off scenario. Add a distinct opt-in zoning scenario rather than changing historical baseline meaning. |
| `docs/adaptive-engine-implementation.md`, runtime/design guidance | Explain three independent policies, shared detection, Speed default, precedence, configuration cutover, and throughput trade-off. |
| `.agents/memory/30-reference-library.md` | Update only if a feature-defining native contract changes; prefer existing APIs. |

Search all consumers of the old shared fields and zoning enablement before changing signatures. Include tests, custom presets, serialized examples, benchmark generators, status-rail/read models, and feature-gated render/smoke paths. Do not rely on default-branch search when implementing against a different checked-out commit.

## 12. Regression-test acceptance matrix

Suggested test names are specific acceptance targets; adapt existing test organization rather than creating a new framework.

### 12.1 Settings, presets, and UI

| ID | Test | Required assertion |
| --- | --- | --- |
| CFG-01 | Independent value round-trip | Different limiter/zone percentages, selectors, and custom masks survive serialization with no cross-over. |
| CFG-02 | Disabled zones, missing block | Existing background settings remain identical; new inactive defaults are available; file bytes remain unchanged on load. |
| CFG-03 | Enabled zones, missing block | Actionable load/import error; no file replacement, native mutation, or automatic persistence from fallback defaults. |
| CFG-04 | Nested validation | The same checks apply inside Battery settings and custom presets. |
| CFG-05 | Invalid share/mask | Reject 0, 100, out-of-domain indices, empty active custom partitions, and malformed fields without shifting out of range. |
| PRESET-01 | Speed default | Zoning false; all unrelated Speed fields match baseline; background limiting remains enabled. |
| PRESET-02 | Capture and Apply | Both independent policies round-trip; excluded paths and other feature settings remain unchanged. |
| UI-01 | Sibling visibility | Zoning visible/editable with the background limiter off; separate collapse state for live and preset views. |
| UI-02 | Stable label/value | Toggling zoning never changes the background percentage value or its meaning. |
| UI-03 | Read-only and Cancel | Built-ins cannot be edited; Cancel discards only the active draft; profile switching preserves intended values. |
| UI-04 | Existing repairs | Exit-time completion cleanup, invalid-draft guards, and unrelated UI-only message handling still pass. |

### 12.2 Policy and runtime

| ID | Test | Required assertion |
| --- | --- | --- |
| GATE-01 | All enable combinations | Test all eight P/L/Z combinations under both master states. Z-only produces allocation work, not priority/efficiency work. |
| GATE-02 | AC/Battery lifecycle | Start on an inactive profile, switch to one requiring only zones, and observe work without a settings revision change or busy loop. |
| GATE-03 | Pending cleanup | All policies off does not stop release retries while owned state remains. |
| POLICY-01 | Foreground-only saturation | Sustained foreground load, zero eligible background competition: no foreground zone claim and no foreground CPU Set write. |
| POLICY-02 | Idle background processes | Presence of idle processes does not satisfy competition. |
| POLICY-03 | Filtered competition | Excluded, protected, denied, exited, suppressed, or higher-owner background candidates do not justify foreground narrowing. |
| POLICY-04 | Real competition, Z only | Eligible foreground plus hot background can activate zones with L and P false. |
| POLICY-05 | Both policies enabled | One deterministic mode and at most one Adaptive request per exact key; inactive zoning uses only an independently valid fallback. |
| POLICY-06 | Lost competition | Withdraw foreground zoning on the next reconciliation even if foreground/system pressure remains high. |
| POLICY-07 | Unknown data | Missing samples are not zeros or proof of contention; no new foreground narrowing from stale evidence. |
| POLICY-08 | Foreground change/descendants | Exact current group only; old generation is released; PID reuse cannot inherit a claim. |
| MASK-01 | Rounding | Test 1, 2, 3, 8, 12, 16, and 24 logical-processor domains with boundary shares; never emit an empty effective assignment. |
| MASK-02 | Selection strategies | Validate least-used All/P/E and fixed/custom behavior separately, including realized count reporting. |
| MASK-03 | Unsupported domain | Decline incomplete/multi-group representation rather than silently truncating. |
| MASK-04 | Cache invalidation | Mode/profile/share/selector/topology changes cannot reuse a semantically stale mask. |
| SCHED-01 | Dormancy | No producers/no state means no extra allocation polling; unrelated wakes preserve pending deadlines. |

### 12.3 Coordinator and failure injection

| ID | Scenario | Required assertion |
| --- | --- | --- |
| OWN-01 | One owner-level update | Background and foreground target lists do not release each other through separate whole-owner submissions. |
| OWN-02 | Soft/hard handoff | Background HardAffinity → Zones SoftCpuSets → background fallback preserves the original baseline and the existing switch-failure compensation. |
| OWN-03 | Explicit owner appears | Higher-priority rule wins; zoning cannot widen or erase it. |
| OWN-04 | Zones disabled | Remove foreground zone intent, preserve valid background/other-owner requests, and retain pending failures. |
| FAIL-01 | All background writes fail | No foreground narrowing; status is not Active. |
| FAIL-02 | Partial foreground failure | Degraded status; withdraw incomplete zone intent through the coordinator; cleanup survives retries. |
| FAIL-03 | Journal begin/commit/readback failure | Existing no-untracked-mutation and compensation guarantees remain intact. |
| FAIL-04 | Disable during pending handoff | Latest desired mode wins without baseline loss or accidental resurrection of old zone requests. |
| REC-01 | Clean shutdown/crash helper | Restore exact original CPU Sets/affinity for disposable targets; preserve pending-versus-committed recovery history. |
| REC-02 | External change | No blind overwrite during release; preserve the controller's current external-break behavior. |
| REC-03 | Empty versus explicit baseline | Restore each representation exactly, not as an explicit all-processors mask. |

Use pure policy tests, an injectable clock/observations, and existing fake controller adapters before live Windows integration. Source-text assertions alone do not establish runtime independence or recovery behavior.

## 13. CPU-Z and workload validation

### 13.1 Measurement configurations

| Configuration | Purpose |
| --- | --- |
| System Default / Adaptive disabled | Same-session performance reference after verified cleanup. |
| Original Speed with zones on | Reproduce the reported regression using the inspected baseline or a clearly recorded equivalent configuration. |
| Revised Speed with zones off | Verify the requested default policy and background-only behavior. |
| Revised Speed with zones explicitly on, no competing background workload | Verify that competition gating avoids narrowing foreground CPU Sets. |
| Revised Speed with zones explicitly on and controlled competing background workload | Characterize the opt-in isolation/throughput trade-off rather than assuming benefit. |

Run at least five measurements per compared condition, alternating or randomizing order. Keep the CPU-Z version, benchmark version/thread count, power source, foreground state, other Winderust features, and starting conditions comparable. Record CPU model and build SHA rather than inferring them from the scores.

Capture requested and observed process-default CPU Sets for the actual compute process, relevant explicit/thread restrictions where available, active mode, selected counts, CPU frequency/power/temperature when available, and Action Log transitions. Readback of process-default sets is not proof that every thread follows them. [W1][W3]

### 13.2 Acceptance

The hard functional gate is **no foreground CPU allocation acquired by background-only mode**, and **no foreground zoning when real competing background targets are absent**.

For the Speed throughput target, declare the allowed multi-thread regression budget before testing. A proposed starting budget is 1% in the same-session median against Default; this is an engineering acceptance target, not an established CPU-Z noise specification. If measurement uncertainty is comparable to the budget, collect additional controlled runs and report uncertainty instead of widening the budget after seeing results.

Report individual scores, means, medians, and spread. Do not claim the original +2.38% single-thread result is guaranteed to remain. Do not convert a logical-processor share into an expected score percentage. Preserve the opt-in zoning result even when it is slower.

The existing fixed-policy runtime benchmark explicitly disables zones. Keep that scenario and add named coverage for zoning; passing only the original scenario is insufficient to validate this change. [S12]

## 14. Implementation order and validation commands

1. **Baseline and consumer inventory.** Pin the implementation checkout, inspect relevant graph/callers, record existing preset values and passing tests, and leave unrelated files alone.
2. **Settings and UI separation.** Add zone-owned parameters and explicit input validation; implement sibling controls and Speed's off default; update all fixtures and presets together.
3. **Pure policy extraction.** Define independent gates, competition eligibility, mask resolution, fallback precedence, and cache invalidation with tests before wiring native effects.
4. **Runtime integration.** Update requirements/observations/lifetime, then feed one coherent Adaptive target lifecycle into the existing coordinator.
5. **Failure and handoff coverage.** Add result-bearing feedback only where required; test no-background-success, partial application, mode changes, and restoration.
6. **Documentation and validation.** Update both locales and current contracts; run static, test, render-feature, targeted Windows, and benchmark checks; report actual results.

Run these separately, or use a wrapper that stops on each nonzero exit status. These are instructions for the implementing agent, **not commands executed during specification preparation**.

```powershell
git diff --check
cargo fmt -- --check
cargo clippy --locked --all-targets -- -D warnings -D unsafe-op-in-unsafe-fn
cargo test --locked
cargo check --locked --features render-smoke
.\scripts\check_architecture_ownership.ps1
.\scripts\check_legacy_names.ps1
cargo test --locked adaptive_runtime_benchmark_generated_settings_are_valid
```

Follow the repository's Graphify requirements when its local graph/tooling is available; run `graphify update .` after code changes. If tooling or Windows integration is unavailable, state precisely which checks were not run. Do not report ignored tests as passed or benchmark preflight as a completed workload run. [S9]

Do not execute all destructive/ignored Windows tests blindly. Use the existing opt-in disposable-target procedures and their cleanup protections.

## 15. Definition of done

The change is complete only when:

- Zoning and background limiting are separate sibling controls with stable, independent values and no hidden enable dependency.
- New Speed defaults leave zoning off without altering unrelated Speed tuning.
- A foreground-only all-core workload receives no zone claim, even when it triggers CPU pressure.
- Zones-only and all AC/Battery lifecycle cases work through the existing worker and scheduler.
- One Adaptive allocation lifecycle prevents overlapping policy producers from releasing one another's targets.
- Per-target results distinguish desired, effective, overridden, and failed allocation; incomplete zoning cannot be reported as Active.
- Original CPU Sets/affinity and other owners survive toggles, failures, handoffs, shutdown, and tested recovery paths.
- New configuration data is validated at all relevant boundaries; incompatible enabled-zone input is reported without overwriting the original file.
- Applicable tests and exact implementation-head Windows CI pass; missing interactive/performance evidence is disclosed.
- The agent delivers a concise changed-file summary, exact commit/build identifier, test results, configuration-cutover note, and measured benchmark table. No unmeasured performance claims.

**Release interpretation:** Separating controls fixes predictability; disabling zones in Speed and requiring actual competition addresses the reported performance-policy problem. Neither change certifies that every other preset or workload is faster.

## 16. Source index

Repository sources below are pinned to the inspected baseline. Recheck the implementation checkout before editing; later changes may alter call sites without invalidating the stated product requirements.

| ID | Source | Used for |
| --- | --- | --- |
| S1 | [Adaptive presets][s1] | Current Speed/Performance values and preset capture/apply behavior. |
| S2 | [Adaptive editor][s2] | Existing tabs, nested controls, draft handling, and two-group expansion state. |
| S3 | [Adaptive process manager][s3] | Current gates, target ordering, foreground/background grouping, sampling, and allocation submission. |
| S4 | [Adaptive policy helpers][s4] | Pressure gates, complementary masks, selection strategies, and percentage semantics. |
| S5 | [Runtime requirements][s5] | Required-work predicates and shared reaction cadence. |
| S6 | [CPU allocation feature facade][s6] | Owner target replacement and apply/release outcomes. |
| S7 | [CPU allocation coordinator][s7] | Claims, baselines, handoffs, per-process transactions, and recovery. |
| S8 | [Windows CPU allocation adapter][s8] | Native queries/setters and group-0 mask conversion. |
| S9 | [AGENTS.md][s9] | No unsolicited migrations/aliases; safety and validation requirements. |
| S10 | [Runtime contracts][s10] | Single coordinator, precedence, settings/profile lifecycle, and restoration obligations. |
| S11 | [Settings model][s11] and [settings storage][s11b] | Current fields, strict Adaptive schema, profiles, deserialization, and atomic persistence. |
| S12 | [Adaptive runtime benchmark][s12] | Existing generated configuration and zones-off fixed-policy benchmark. |
| S13 | [Automation runner][s13] and [settings coordinator][s13b] | Composition, runtime projection, and publication boundaries. |
| S14 | [Architecture][s14] and [design specification][s14b] | Ownership and UI design boundaries. |
| W1 | [Microsoft: CPU Sets][w1] | Process/thread assignments, affinity precedence, and reservation limitations. |
| W2 | [Microsoft: SetProcessDefaultCpuSets][w2] | Process-default assignment and clearing semantics. |
| W3 | [Microsoft: GetProcessDefaultCpuSets][w3] | Readback and empty-assignment semantics. |

[s1]: https://github.com/TatshSiow/Winderust/blob/5aca8733b4d5f4cdbe6fe40362fede2673397f74/src/ui/adaptive_presets.rs
[s2]: https://github.com/TatshSiow/Winderust/blob/5aca8733b4d5f4cdbe6fe40362fede2673397f74/src/ui/adaptive_engine.rs
[s3]: https://github.com/TatshSiow/Winderust/blob/5aca8733b4d5f4cdbe6fe40362fede2673397f74/src/features/winderust_features/adaptive_engine_process.rs
[s4]: https://github.com/TatshSiow/Winderust/blob/5aca8733b4d5f4cdbe6fe40362fede2673397f74/src/features/winderust_features/adaptive_engine_process/policy.rs
[s5]: https://github.com/TatshSiow/Winderust/blob/5aca8733b4d5f4cdbe6fe40362fede2673397f74/src/backend/automation/requirements.rs
[s6]: https://github.com/TatshSiow/Winderust/blob/5aca8733b4d5f4cdbe6fe40362fede2673397f74/src/features/cpu_control/cpu_allocation.rs
[s7]: https://github.com/TatshSiow/Winderust/blob/5aca8733b4d5f4cdbe6fe40362fede2673397f74/src/control/cpu_allocation.rs
[s8]: https://github.com/TatshSiow/Winderust/blob/5aca8733b4d5f4cdbe6fe40362fede2673397f74/src/platform/windows/cpu_allocation.rs
[s9]: https://github.com/TatshSiow/Winderust/blob/5aca8733b4d5f4cdbe6fe40362fede2673397f74/AGENTS.md
[s10]: https://github.com/TatshSiow/Winderust/blob/5aca8733b4d5f4cdbe6fe40362fede2673397f74/.agents/memory/25-runtime-contracts.md
[s11]: https://github.com/TatshSiow/Winderust/blob/5aca8733b4d5f4cdbe6fe40362fede2673397f74/src/config/settings.rs
[s11b]: https://github.com/TatshSiow/Winderust/blob/5aca8733b4d5f4cdbe6fe40362fede2673397f74/src/config/storage.rs
[s12]: https://github.com/TatshSiow/Winderust/blob/5aca8733b4d5f4cdbe6fe40362fede2673397f74/scripts/adaptive_runtime_benchmark.ps1
[s13]: https://github.com/TatshSiow/Winderust/blob/5aca8733b4d5f4cdbe6fe40362fede2673397f74/src/backend/automation/runner.rs
[s13b]: https://github.com/TatshSiow/Winderust/blob/5aca8733b4d5f4cdbe6fe40362fede2673397f74/src/application/settings.rs
[s14]: https://github.com/TatshSiow/Winderust/blob/5aca8733b4d5f4cdbe6fe40362fede2673397f74/docs/architecture.md
[s14b]: https://github.com/TatshSiow/Winderust/blob/5aca8733b4d5f4cdbe6fe40362fede2673397f74/.agents/memory/15-design-spec.md
[w1]: https://learn.microsoft.com/en-us/windows/win32/procthread/cpu-sets
[w2]: https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setprocessdefaultcpusets
[w3]: https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocessdefaultcpusets
