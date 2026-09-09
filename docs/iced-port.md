# Iced port

Winderust uses Iced 0.14 with only the Tiny Skia software renderer
to prioritize a lower memory footprint.
The local `iced_tiny_skia` patch combines heavily fragmented repaint regions to
avoid replaying the scene for hundreds of small regions during table updates.
See `vendor/iced_tiny_skia/README.winderust.md` for provenance and its regression test.
Production Cargo dependencies contain neither GPUI nor gpui-component.
The implementation lives in `src/ui/iced/`; `src/ui.rs` retains the canonical
page names and sections. The vendored GPUI directory is unchanged.

## Implemented

- All 34 navigation destinations, including section landing pages.
- Home charts and feature summaries; searchable, collapsible navigation and
  Lucide icons; custom window controls; system/light/dark appearance, accents,
  and English/Traditional Chinese localization.
- Settings save/cancel/import/export through the existing SettingsEditor,
  power-source profiles, validation of numeric drafts, update checks and About.
- The five Power Plan Control rule pages and Advanced Power Plan Tuning.
- All six Priority Control pages, Background Efficiency, CPU Limiter,
  CPU Sets (Soft), Processor Affinity (Hard), and Adaptive Engine presets.
- App Suspension, Memory Trim, Timer Resolution, and Win32 Priority Separation.
- Virtualized Process List with grouped rows, sorting, resource columns, icons,
  per-process details, saved policies, and typed one-shot commands.
- Action Log filters, pagination, clearing, and atomic CSV export to a chosen path.
- Animated navigation, page transitions, groups, removals, and virtualized process
  group expansion. Confirmed removal changes settings immediately; animation
  copies do not determine whether a change is saved.
- Existing native tray, minimized start, single-instance restoration, portable
  settings, runtime safety and shutdown restoration boundaries.

## Verification

The default build passes strict Clippy and 659 tests (14 intentionally ignored).
The optimized release build succeeds. The
architecture ownership script, locale-key scan, formatting and whitespace checks
also pass. Final command logs are under `target/iced-*.log`.

The explicit render harness captures every page in English/light at 1120x760 and
Traditional Chinese/dark at 900x620:

```powershell
cargo run --locked --features render-smoke -- --render-smoke
```

It uses an in-memory configuration with automation disabled, performs read-only
queries, writes 68 PNGs under `target/iced-smoke/`, then shuts down. The normal
release build does not include the harness. This is a rendering check, not a
simulation of physical mouse input or Chinese IME composition.

The Windows Computer Use helper is unavailable in this session. Physical mouse,
IME, monitor-DPI transitions and taskbar/tray interaction still need a Windows
manual pass. Full keyboard navigation and screen-reader accessibility remain
explicitly deferred by the user. Production RAM figures have not been measured
for this migrated build; earlier prototype measurements are not production data.

## Source cleanup

The retired `src/ui/app.rs` and `src/ui/app/` renderer is removed. The production
crate has no GPUI dependency or frontend switch. The existing `vendor/gpui/`
directory remains read-only as requested.
