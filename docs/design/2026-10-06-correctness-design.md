# TinyMon refinement - SP0 baseline + SP1 correctness: design

- Status: Draft for owner review
- Date: 2026-10-06
- Baseline: `main` at b1c482a plus the uncommitted `render.rs` emphasize() change. Release exe 240,640 B (235,520 B with `--no-default-features`), 59 tests (58 without default features), 6 clippy warnings, `cargo fmt --check` not clean.

## 1. Scope and non-goals

This spec covers the first two steps of the refinement roadmap (SP0 hygiene baseline, SP1 correctness, SP2 portable exe, SP3 UX polish, SP4 GitHub release):

- **SP0 hygiene baseline** (step 0): finish the pending `render.rs` change, pin LF line endings, adopt a rustfmt baseline, make clippy clean.
- **SP1 correctness** (steps 1-38): fix every confirmed or plausible finding of the code review in the app shell, the settings UI and config, the render/registry core and the samplers, as amended by the design review; then bring the docs up to date, and finally verify on the owner's machine as the last act of SP1.

Finding references such as `samplers[0]` or `settings-ui[12]` name the code review's numbered findings (area, index). Where the design review amended a design, this spec describes only the amended version.

Non-goals:

- SP2 (portable exe): hybrid static CRT, CI-built releases, builds without build-machine paths, dark-mode build gating.
- SP3 (UX polish): Common Controls v6, a compact per-core layout, the D3DKMT discrete-GPU temperature, icon artwork unification, a settings layout for small screens.
- SP4 (GitHub release): LICENSE files, repository URL check, publishing.
- Not planned at all: a LibreHardwareMonitor (LHM) web-server reader, worker threads, new crates, new config keys.
- Nothing is pushed or merged during SP1: at its end the local branch `sp1-correctness` stays unmerged and unpushed for the owner's review; merging into `main` is the owner's decision.

Section 11 lists every deferred item with its reason.

## 2. Goals and success criteria

Goals:

1. Every confirmed or plausible finding (52 in the four areas) is fixed, or deferred in Section 11 with a reason.
2. Every behavioural change lands test-first: a unit test (or, where nothing is pure, a debug-copy runtime check) that fails before the change and passes after it. The exceptions are C2.8 (show without activation), whose focus behaviour only the owner's manual check in the live session (8.4) can show, and the settings window's WM_DPICHANGED relayout (step 28), which reuses step 27's tested layout table and can only be checked by hand (8.5).
3. The release exe grows by about +19 KB (the accepted budget; likely +17 to +21 KB). If the running total passes +22 KB (263,168 B) at any commit, work stops for re-planning.
4. No regressions in drag-anywhere moving, the widget and tray context menus, the tray icon, the dark owner-drawn settings look, config compatibility, or the widget's saved position on a secondary monitor left of the primary (negative X, for example (-213,-16)).

Success criteria:

- All gates green after every commit (8.1). At the end: about 190 tests in the default build (2 of them ignored diagnostics) and about 176 with `--no-default-features`.
- Final release exe about 259,500 B (about 253 KiB; the owner's "about 259.5 KB"), and never above 263,168 B (240,640 + 22 x 1,024) at any commit.
- The owner's current 137-byte config parses and re-serializes byte for byte; no config key is added or removed.
- All debug-copy runtime checks (8.3) and the live session (8.4) pass, and the owner confirms the one-minute manual check.
- All commits sit on a local branch, are small and test-first, and carry no Co-Authored-By trailer; at the end of SP1 the branch is neither pushed nor merged.

Finding coverage (confirmed and plausible findings, finding -> item):

| Area | Coverage |
|---|---|
| app-shell (11) | [0] C2.4; [1] C2.1b; [2] C2.5, C2.2; [3] C2.6; [4] C2.7; [5] C2.8; [6] C2.9; [7] C2.10 (v6 part deferred); [8] C2.4, C3.7; [9] C2.4; [10] C3.1 |
| settings-ui (18) | [0] C4.1; [1] C3.1, C3.8; [2] C3.3; [3] C3.1; [4] C4.1; [5] C4.1 (silent-failure part deferred); [6] C3.2; [7] C3.4; [8] C4.1; [9] C2.5; [10] C3.5; [11] C3.5; [12] C3.6a; [13] C3.6b; [14] C3.1; [15] C3.7; [16] C3.7; [17] C3.7 |
| render-registry (11) | [0] C1.1; [1] C2.1b; [2] C1.2; [3] C2.1a; [4] C2.1a; [5] C2.3; [6] C2.1a; [7] C2.2 (grid deferred); [8] C3.6b; [9] C4.1; [10] docs pass |
| samplers (12) | [0] C1.1; [1] C1.2; [2] C4.4; [3] C4.4; [4] C4.4; [5] C1.6; [6] C2.1a, C2.1b; [7] C1.5; [8] C4.2, C4.3; [9] C1.3; [10] C1.4; [11] docs pass |

Info-level items: app-shell[11] C2.9; app-shell[12] C2.10 and docs pass; settings-ui[18] C2.10 (manifest, partly) and SP3 (v6); settings-ui[19] SP2; settings-ui[20] step 3, C3.1 (dead Esc branch), C3.3 and docs pass; render-registry[11] step 0a.

## 3. Owner decisions

| Topic | Decision | Consequence |
|---|---|---|
| Roadmap | SP0 hygiene, SP1 correctness, SP2 portable exe, SP3 UX polish, SP4 GitHub release; this spec is SP0 + SP1 only | Work that belongs to a later step is deferred, not half-done here |
| Temperature ("honest + GPU temp") | Hide fake or static ACPI zones (liveness rule); label real zones "ACPI", never "CPU Temp"; show "Temp n/a" when nothing real exists; MetricKey::DEFAULT = [Cpu, Ram, Gpu]; keep and harden the legacy LHM WMI reader (works only with LHM 0.9.4 or older, since 0.9.5 removed WMI); no LHM web-server reader; discrete-GPU temperature via D3DKMT adapter perf data is SP3 | The owner's widget shows "Temp n/a" because their config lists temp. build_rows is the single row-assembly point, so SP3 can add a GPU temperature source without reworking SP1 |
| D1 settings preview | Preview column 210 -> 270 logical px (client width 502 -> 562); the preview renders the widget 1:1 at real size; only very tall layouts are scaled down | The settings window is 60 px wider; its width still fits a 1366 px wide screen at 125% (the existing height problem on 768 px screens is unchanged, Section 11) |
| D2 memory label | Integrated (UMA) adapters: "GPU Mem"; discrete adapters: "VRAM"; with several adapters `<label> <name>`, label first (e.g. "VRAM RTX 3050", "GPU Mem UHD 620"); the settings checkbox name and the `vram` config token do not change | Truncation keeps the label visible; config compatibility is unaffected. `<name>` is gpu::short_name's output minus a leading "Intel " (short_name strips "AMD" and "NVIDIA" but keeps "Intel", and the GPU load row still shows it), so the owner's example holds literally and a cut label still names the model |
| D3 live verification | At the very end of SP1 (after the docs pass): exit the owner's running TinyMon, run the automated C2.6 and C2.7 checks with the HKCU Run and StartupApproved values backed up first and restored afterwards, rebuild target/release in the repository, start the new build through explorer.exe so it runs non-elevated. No synthetic keyboard or mouse input: the owner spends about a minute confirming by hand that the toggle hotkey no longer steals focus and that dragging and the right-click menu still work | The only time the owner's instance, autostart entry, config or release folder is touched; the owner's config and exe are backed up first, and a failed check is fixed in a new commit before the session is repeated (8.4) |
| D4 size | Accept about +19 KB (240,640 B -> about 259,500 B). Measure a release build in a scratch `--target-dir` after every commit; stop and re-plan if the running total passes +22 KB (263,168 B = 240,640 + 22 x 1,024) | Size is a per-commit gate |
| Process | All work on a local branch (`sp1-correctness`), never pushed in SP1; every commit small and test-first; commits carry no co-author trailer | History is reviewable commit by commit; at the end of SP1 the branch stays unmerged and unpushed for the owner's review |
| SP0 content | 0a finish emphasize(); 0b .gitattributes; 0c rustfmt.toml plus one formatting-only commit; 0d the six clippy fixes plus #[ignore] on the two GPU dump tests | SP1 starts from a formatted, lint-clean tree with a fast test suite |

## 4. Standing constraints

- Pure Rust + Win32. windows 0.62.2 stays the only runtime dependency; no crate is added (the build dependency winres is unchanged). New windows-crate features only where an API is gated (5.9).
- Small size matters (D4). Known size traps are avoided: std::time::Instant (+8 KB), Path::with_extension and Vec::sort_by on two items (about +3.5 KB each), Path equality and String::from_utf8_lossy (+1 KB each), HashMap grouping.
- No admin rights, services or drivers; TinyMon stays an ordinary user process (asInvoker).
- One UI thread; no worker threads in SP1. Blocking calls on it stay cheap (PDH, GetIfTable2) or bounded (the LHM WMI reader; a hung winmgmt is the documented exception, Section 11).
- Release builds use `panic = "abort"`: no panic paths in wnd_proc, the renderer or the samplers (divide-by-zero guards, no unwrap on OS results).
- Config compatibility: no new keys; every existing file parses as before or better; x/y keep their meaning of "where the widget lives", now formalised as the home position.
- The settings window stays dark-only and owner-drawn on comctl32 5.82; nothing in SP1 depends on v6.
- Minimum OS stays Windows 10 1703 (the DPI functions are static imports); the GPU Adapter Memory counters need 1709 (VRAM shows n/a before that).
- Files are UTF-8 without BOM with LF endings. New and rewritten comments use ASCII; existing non-ASCII comments are left alone unless their line is edited anyway. New UI symbols in string literals are written as `\u{...}` escapes.
- No generated-by or co-author attribution lines anywhere in the repository or its history.
- The owner's running instance, config and registry are never touched outside the live session; launched test copies are debug builds in their own folders (8.3).
- The repository's `target/release` holds the owner's autostarted exe and its portable config (`target/release/tinymon.cfg`, the owner's only copy): it is never cleaned or deleted (no `cargo clean`), and SP1's release builds go to a scratch `--target-dir` until the live session.
- Code is referenced by file and function name, never by line number (the SP0 formatting commit shifts lines).

## 5. Architecture changes overview

### 5.1 New and changed files

| File | Changes (items) |
|---|---|
| `src/history.rs` (new) | RowId, row_ids, History (C2.1a) |
| `src/placement.rs` (new) | fit_into, anchored, corner_xy, place, after_relayout, drag_end, work-area shims (C2.5, reused by C3.6a) |
| `tinymon.manifest` (new) | asInvoker, Windows 10 supportedOS, PerMonitorV2 (C2.10) |
| `.gitattributes`, `rustfmt.toml` (new) | SP0 |
| `src/main.rs` | Apply hand-off (C2.9), history and pending wiring (C2.1a, C1.5), placement, relayout and runtime resize (C2.5, C2.1b, C2.2), tray (C2.4), no-activate show (C2.8), single instance (C2.6), key routing and WM_HOTKEY forwarding (C3.1), hotkey status (C3.2) |
| `src/registry.rs` | Row.alert, Row::rate, Row::na, is_rate, PENDING, mask_pending, collect_rows, DEFAULT, sample_rows |
| `src/render.rs` | emphasize tests (0a), is_critical, BarStyle and inset sparkline geometry, history slices, rows_fitting, widget_size, '+N' |
| `src/gpu.rs`, `src/pdh.rs` | luid_key, busiest-engine load, adapter-wide VRAM with UMA and labels, n/a rows, short_name; try_sample |
| `src/net.rs`, `src/disk.rs`, `src/percpu.rs` | GetIfTable2 network; n/a rows; stable per-core rows |
| `src/battery.rs` | alert: false on the two battery_row literals (C2.3) |
| `src/temp.rs` | zone liveness, honest labels, build_rows, bounded LHM reader |
| `src/config.rs`, `src/settings.rs` | trailing comments, decoding, load order, atomic save; metric dedup |
| `src/settings_win.rs` | apply() chain, capture fidelity, DM_GETDEFID, focus, layout table, DPI, true-size preview, paint details, numeric inputs, notes |
| `src/hotkeys.rs` | parse_hotkey fix, conflict_message, should_register, to_warn, any_failed, vk_types_text, blocks_typing |
| `src/startup.rs` | path matching, quoting, StartupApproved |
| `src/dpi.rs`, `src/icon.rs`, `src/dark.rs`, `src/fps.rs`, `build.rs`, `Cargo.toml` | manifest doc and test; icon header and size test; WARN colour; comment; manifest embedding; features and temp comment |

### 5.2 Row model

```rust
pub struct Row {
    pub label: String,
    pub value: String,
    pub bar: Option<f32>,                // None = value-only row
    pub color: Option<(u8, u8, u8)>,     // label accent, stamped per metric
    pub bar_color: Option<(u8, u8, u8)>, // fixed bar colour (battery); never a sparkline
    pub alert: bool,                     // label may turn red at >= 90%
}
```

- `Row::bar(label, value, frac)`: load rows (CPU, RAM, GPU, VRAM/GPU Mem, per-core, temperature); alert = true.
- `Row::rate(label, value, frac)`: throughput rows (disk, net); alert = false.
- `Row::na(label)`: value "n/a", bar None, alert = false; used for every reading that cannot be taken.
- Value-only literals (uptime, battery) set alert = false; a missing field is a compile error.
- `render::is_critical(row) = row.alert && bar >= ALERT_AT` (0.9).
- `MetricKey::is_rate()`: exhaustive match, true for Cpu, PerCpu, Gpu, Disk and Net. is_rate means the value is a delta between two samples (so CPU and GPU load count); it is unrelated to `Row::rate`, which marks throughput rows that never raise the red alert.
- `registry::PENDING = "\u{2026}"` and `mask_pending(rows)`: value becomes PENDING, `Some(bar)` becomes `Some(0.0)`, value-only rows stay value-only.
- `registry::collect_rows(sources, pending) -> (Vec<Row>, Vec<MetricKey>)`: the one flatten used for painting, history and row counting; masks a source's slice when `pending && key.is_rate()`.

### 5.3 Row identity and sparkline history

- `history::RowId { metric: MetricKey, label: String, nth: u16 }`; `nth` counts earlier rows of the same tick with the same metric and label. `row_ids(keys, rows)`.
- `History::record(ids, rows)`: a row with `Some(bar)` appends (capped at HIST_LEN = 64); a row present with bar None keeps its series unchanged, so the sparkline pauses; rows with a bar_color are not recorded; ids absent from the tick are dropped. `get(id) -> &[f32]` (empty if unknown).
- Only WM_TIMER records, with `collect_rows(.., false)`, so masked values never reach history. Apply never clears history. A label change (for example "Temp n/a" to "ACPI Temp") starts a new series.
- `render_rows(.., history: &[&[f32]])` receives slices aligned with the rows; `bar_style` chooses None, Level or Spark (Spark needs at least 2 samples).

### 5.4 The Apply hand-off

```text
settings_win::apply(hwnd) -> bool                 (settings window, UI thread)
    effective hotkeys (C3.1), parse_clamped + write-back (C3.4), same_combo guard (C3.2)
    crate::queue_apply(main, new) -> bool         stash in thread_local PENDING_APPLY, post bare
                                                  WM_APP_APPLY; if the post fails, take the slot back
IDOK: if apply(hwnd) { DestroyWindow(hwnd) }      IDC_APPLY ignores the result

wnd_proc WM_APP_APPLY (wParam ignored):
    let Some(new) = take_pending_apply() else { return 0 }    // foreign or duplicate post: no-op
    if let Some(text) = apply_settings(hwnd, new) { show_warning(hwnd, &text) }  // no AppState ref
```

`apply_settings(hwnd, new) -> Option<String>` runs in this fixed order:

1. One short `&mut AppState` borrow: keep the current home (settings.pos); apply opacity; on an interval change KillTimer/SetTimer; on a font change recreate the font; if the metric order changed, rebuild and prime the sources, set first_tick_pending and re-arm TIMER_ID with first_tick_ms(interval) (this re-arm supersedes the interval one; history is not cleared). Hotkeys: `changed = [toggle differs, settings differs]`; if `should_register(changed, hk_status)`, unregister, register and store hk_status. Copy out the row count, both specs, `to_warn(result, changed)` and settings_hwnd. The borrow ends.
2. `sync_layout(hwnd, n, order_changed || font_changed)` (SetWindowPos outside any borrow).
3. Re-derive `state_ptr(hwnd)`, re-check it for null, and persist last, so a home moved by the resize is saved.
4. If `IsWindow(settings_hwnd)`, post WM_APP_HOTKEY_STATUS.
5. InvalidateRect.
6. Return `hotkeys::conflict_message(toggle, settings, &warn)`; the arm shows it with show_warning, which only calls MessageBoxW.

The borrow rule, introduced by C2.9 and checked in every later `main.rs` commit: no AppState reference may be live across any call that sends messages to the widget window. Such calls include SetWindowPos, ShowWindow, SetWindowLongPtrW, UpdateWindow, DestroyWindow, MessageBoxW, TrackPopupMenu and the `settings_win` entry points. Copy what is needed, end the borrow, make the call, then re-derive `state_ptr(hwnd)` and re-check it for null.

### 5.5 Message and timer ids

All WM_APP ids live in one `main.rs` block; `main::tests::wm_app_ids_are_distinct` asserts they are distinct.

| Id | Value | Sender -> receiver | Notes |
|---|---|---|---|
| WM_TRAY | WM_APP + 1 | shell -> widget | version-0 tray callback |
| WM_APP_APPLY | WM_APP + 2 | settings -> widget | wParam ignored; settings travel in PENDING_APPLY |
| WM_APP_SETTINGS_CLOSED | WM_APP + 3 | settings -> widget | clears settings_hwnd only when `!IsWindow(settings_hwnd)` |
| WM_APP_SURFACE | WM_APP + 4 | second launch -> running widget | cross-process, possibly between builds: frozen forever |
| WM_APP_HOTKEY_STATUS | WM_APP + 5 | widget -> settings | refresh the hotkey notes |
| TaskbarCreated | RegisterWindowMessageW | Explorer -> widget | allowed through UIPI with ChangeWindowMessageFilterEx |

Timers: TIMER_ID = 1 (sampling), TRAY_TIMER_ID = 2 (tray retry). WM_TIMER dispatches on wParam.

### 5.6 Startup order

This is the final order once step 36 has landed; earlier steps fill it in progressively. main():

1. `dpi::set_process_dpi_aware()` (fallback; the manifest declares PMv2).
2. `if !claim_single_instance() { return Ok(()) }`, before any file I/O.
3. `TASKBAR_CREATED = RegisterWindowMessageW("TaskbarCreated")`.
4. The tray icon at the tray's small-icon size; RegisterClassW (no class icon); `dark::init()`; `settings_win::register_class`.
5. `cfg_path = config::resolve_write_path()`, then `config::load(cfg_path.as_deref())`.
6. Settings, font, build_sources and the priming sample.
7. AppState with `history: History`, `first_tick_pending: true`, `hk_status`, `displaced`, `in_move`, `move_start`, `sized_rows`, `tray_dblclk`, `tray_tries` (no `hk_suspended`).
8. CreateWindowExW at a provisional rect (home, or (0,0) on first run, uncapped size at system DPI), opacity, `show_no_activate`, UpdateWindow, message loop.

WM_CREATE: store the state pointer; `SetTimer(TIMER_ID, first_tick_ms(interval))`; `hk_status = hotkeys::register(..)` (silent); DPI reconcile in its own scope; `place_widget` outside any borrow; `ChangeWindowMessageFilterEx(hwnd, TASKBAR_CREATED, MSGFLT_ALLOW)`; `add_tray_icon`, starting the retry timer on failure.

### 5.7 Tick and paint flow

```text
WM_TIMER: match wparam
  TRAY_TIMER_ID => retry add_tray_icon; KillTimer on success or after TRAY_MAX_TRIES
  TIMER_ID => {
    let n = { short borrow: sample every source;
              if take(first_tick_pending) && interval > 1000 { SetTimer(TIMER_ID, interval) }
              (rows, keys) = collect_rows(&sources, false); ids = row_ids(&keys, &rows);
              history.record(&ids, &rows); rows.len() }
    if sync_layout(hwnd, n, false) { persist }    // only when home changed
    InvalidateRect
  }
WM_PAINT: (rows, keys) = collect_rows(&sources, first_tick_pending); ids = row_ids(..);
          hist: Vec<&[f32]> = ids.map(history.get); render_rows(.., &hist)
```

### 5.8 AppState

- Changed: `history: History` (was `Vec<Vec<f32>>` keyed by flat index); `settings.pos` now means home.
- Added: `first_tick_pending: bool`, `hk_status: RegisterResult`, `displaced: bool`, `in_move: bool`, `move_start: (i32, i32)`, `sized_rows: usize`, `tray_dblclk: bool`, `tray_tries: u8`.
- Removed: the unused 32 px class icon (`icon`). Not added: `hk_suspended` (hotkey suspension was rejected in favour of WM_HOTKEY forwarding, C3.1).

### 5.9 Cargo features and manifest

- Added windows features: `Win32_NetworkManagement_IpHelper` and `Win32_NetworkManagement_Ndis` (C1.6: GetIfTable2 and MIB_IF_ROW2 are gated on both), `Win32_Security` (C2.6: CreateMutexW's SECURITY_ATTRIBUTES). They are compile-time only and change no size by themselves.
- Everything else is already enabled: ChangeWindowMessageFilterEx and RegisterWindowMessageW (WindowsAndMessaging), GetSystemMetricsForDpi and AdjustWindowRectExForDpi (HiDpi), the monitor APIs and StretchBlt (Gdi), RegGetValueW (Registry), QueryPerformanceCounter (Performance), GetTickCount64 (SystemInformation), the WBEM status codes (Wmi, temp feature).
- `tinymon.manifest` (C2.10) is embedded as RT_MANIFEST #1 by winres; it does not clash with the icon's RT_GROUP_ICON #1. It declares asInvoker, the Windows 10/11 supportedOS GUID and PerMonitorV2, and deliberately leaves out assemblyIdentity, activeCodePage and the Common Controls 6.0 dependency. SP2 extends this file instead of adding another.

## 6. Detailed design

### 6.0 SP0 hygiene baseline (step 0)

All work happens on the local branch `sp1-correctness`, created from `main` while the uncommitted `render.rs` change is still in the working tree. Four commits.

**0a - finish emphasize()** (render-registry[11]; comment part of docs-spec[22])

- Problem: the pending change replaced lighten() with `emphasize(c, light_bg)` (30% toward white on the dark theme, 30% toward black on the light one) and deepened the LIGHT track colours, but left "brighter/lighter" comments, two lines over 100 columns and no test.
- Change: rewrite the draw_sparkline doc ("crisp brighter top line"), its inline comment ("Crisp brighter line along the top edge"), and the TRACK_EDGE doc and draw_row bevel comment ("1px lighter top edge"; on LIGHT the edge is darker than the track) so they describe emphasize(); wrap the highlight-band `paint_rect` call in draw_row and the `CreatePen` call in draw_sparkline.
- Tests first: `render::tests::emphasize_dark_matches_old_lighten`: for all 256 values `emphasize(c, false) == (c + (255 - c) * 3 / 10)`, the old lighten(). `render::tests::emphasize_light_darkens_by_30_percent`: for all 256 values `emphasize(c, true) == c * 7 / 10` (200 -> 140, 255 -> 178, 0 -> 0).
- Size: 0 B; the dark theme renders byte-identically.

**0b - .gitattributes**

- Problem: `core.autocrlf=true` comes from the system Git config and the repository has no attributes, so a checkout, stash or reset can rewrite the LF sources as CRLF.
- Change: `.gitattributes` with `* text=auto eol=lf` and `*.ico binary`. Every tracked text file is already LF in the index; `git status` after the commit shows nothing else.

**0c - rustfmt baseline**

- Problem: `cargo fmt --check` fails in 13 files (111 hunks) with default settings; the code is hand-tuned close to `use_small_heuristics = "Max"`.
- Change: `rustfmt.toml` with `use_small_heuristics = "Max"` (everything else default, max_width 100) to minimise churn. First move the trailing `// '[' (VK_OEM_4)` comment in `hotkeys::tests::parse_allows_bare_key_rejects_unknown_or_keyless` onto its own line, so rustfmt does not drag the following standalone comment to column 83; then run `cargo fmt` once. This is one formatting-only commit (about 10 files and 49 hunks once 0a has wrapped the two long render.rs lines in rustfmt's own style).

**0d - clippy clean; diagnostic tests ignored**

- Change: the six default-lint warnings: `pdh.rs` `(size as usize + 7) / 8` becomes `.div_ceil(8)`; the render.rs alert predicate `map_or(false, ..)` becomes `is_some_and(..)`; the redundant `as usize` cast in main.rs WM_COMMAND goes; `iter().any(..)` becomes `contains(..)` in the icon.rs test; `#[allow(clippy::too_many_arguments)]` on render_rows and draw_row with signatures unchanged (settings_win::control already does this, and C2.1a, C2.2 and C3.6b re-plumb render_rows later). Mark `gpu::tests::dump_detected_gpus` and `dump_gpu_load` with `#[ignore = "diagnostic: cargo test -- --ignored --nocapture dump_"]` and update their doc comments: they assert nothing, and dump_gpu_load sleeps 1.1 s (the suite drops from about 1.3 s to about 0.2 s).
- Exit state of step 0: fmt and clippy (both feature sets) clean; 61 tests, 59 run and 2 ignored; exe still 240,640 B. From here on every release build goes to a scratch `--target-dir`.

### 6.1 C1 - Sampler data correctness

#### C1.1 VRAM shows adapter-wide usage (step 10)

- Findings: samplers[0], render-registry[0] (both high).
- Problem: the VRAM row reads `IDXGIAdapter3::QueryVideoMemoryInfo(LOCAL).CurrentUsage`, which Microsoft documents as the calling process's own usage; TinyMon creates no D3D device, so it is always about 0. On an integrated GPU the total is only the BIOS carve-out: an Intel UHD 730 shows 0/128M while it really uses about 0.7 GB of shared memory.
- Approach:
  - Usage comes from the system-wide PDH counters `\GPU Adapter Memory(*)\Dedicated Usage` and `\GPU Adapter Memory(*)\Shared Usage` (instantaneous byte counts, valid on the first collect; instances `luid_0x..._0x..._phys_N`), summed over all phys_N instances whose lowercased name starts with the adapter's luid_key. LUIDs match strictly: the Basic Render Driver has its own instance.
  - Integrated (UMA) or discrete is decided once in detect_gpus from two DXGI budget queries: UMA when the NON_LOCAL budget is 0 and the LOCAL budget exceeds DedicatedVideoMemory (or dedicated memory is 0); any other pair of budgets means discrete; without both budgets, UMA only if DedicatedVideoMemory is 0. A zeroed query therefore falls back to discrete, the safe reading. (Microsoft's memory-management guidance: "For UMA adapters non-local is always zero.")
  - Discrete: used = Dedicated Usage, total = DedicatedVideoMemory (Task Manager's "Dedicated GPU memory"). Integrated: used = Dedicated + Shared Usage, total = DedicatedVideoMemory + SharedSystemMemory (Task Manager's "GPU memory").
  - Labels (D2): one integrated adapter gives "GPU Mem", one discrete adapter "VRAM"; with several adapters each row is `<label> <name>`, label first, so DT_END_ELLIPSIS keeps the label (for example "VRAM RTX 3050", "GPU Mem UHD 620"). `<name>` is the adapter's short_name with a leading "Intel " removed: short_name strips "AMD" and "NVIDIA" but keeps "Intel" (it turns "Intel(R) UHD Graphics 620" into "Intel UHD 620", and the GPU load row keeps that), but in the 92-99 px label column "GPU Mem Intel UHD 620" would be cut to "GPU Mem Int...", which names no model. This follows the owner's example literally; a name that is only "Intel" is kept as is.
  - Unknown usage (counter missing before Windows 10 1709, failed collection, no matching instance) shows "n/a".
- Code:
  - `gpu.rs` GpuAdapter: drop the `adapter: Option<IDXGIAdapter3>` field and GpuAdapter::refresh; `vram_used` becomes `Option<u64>`; add `uma: bool`; luid_key comes from luid_key() (C1.7).
  - detect_gpus: query LOCAL and NON_LOCAL once; `uma = is_uma(..)`, `vram_total = vram_total(..)`, `vram_used = None`; delete the stale "integrated GPUs often report 0" comment.
  - VramGroup gains `dedicated` and `shared` PdhCounters; refresh() calls try_sample() once per tick on each and sets every adapter's `vram_used = vram_used(uma, adapter_bytes(ded, key), adapter_bytes(shared, key))`; rows() delegates to vram_rows() (C1.4), which applies the D2 labels (label first; the name part drops a leading "Intel " via strip_prefix).
  - New pure fns: `is_uma(local: Option<u64>, non_local: Option<u64>, dedicated: u64) -> bool`; `vram_total(uma, dedicated, shared_system)`; `vram_used(uma, dedicated, shared) -> Option<u64>` (discrete: dedicated; UMA: None only when both are None, else the sum); `adapter_bytes(samples, luid_key) -> Option<u64>` (None when nothing matches, not 0).
  - `vram_row(name, used: Option<u64>, total)` returns Row::na unless used is known and total > 0; fmt_vram loses its unreachable n/a branch. `registry::sample_rows` uses `vram_row("VRAM", Some(4 GiB), 12 GiB)`. The ignored dump_detected_gpus prints uma, usage and total through VramGroup.
- Tests first:
  - `gpu::tests::uma_needs_both_budget_signals`: (Some(7369 MiB), Some(0), 128 MiB) -> true; (Some(0), Some(0), 8 GiB) -> false; (Some(7 GiB), Some(8 GiB), 8 GiB) -> false; (None, None, 0) -> true; (None, None, 4 GiB) -> false.
  - `adapter_bytes_sums_one_adapters_instances`: phys_0 100 + phys_1 50 of one LUID -> Some(150); the other LUID's 7 is excluded; an unknown LUID -> None.
  - `discrete_vram_is_dedicated_memory`: vram_total(false, 16G, 16G) = 16G; vram_used(false, Some(10G), Some(300M)) = Some(10G); (false, None, Some(_)) = None.
  - `integrated_vram_is_all_gpu_memory`: vram_total(true, 128M, 8027M) = 8155M; vram_used(true, Some(0), Some(793M)) = Some(793M) and the row reads "0.8/8G"; (true, None, Some(5)) = Some(5); (true, None, None) = None.
  - `vram_unavailable_is_na`: vram_row("VRAM", None, 16G) is "n/a" with bar None.
  - `vram_rows_label_by_kind_then_name_when_several`: one UMA adapter -> ("GPU Mem", "0.8/8G"); one discrete -> ("VRAM", "2.0/16G"); UMA "Intel UHD 620" plus discrete "RTX 3050" with unknown usage -> ("GPU Mem UHD 620", "1.0/8G") and ("VRAM RTX 3050", "n/a"); a UMA adapter named just "Intel" next to a discrete one -> "GPU Mem Intel".
  - `vram_row_formats` (rewritten): Some(2G) of 16G -> "2.0/16G" with bar 0.125; Some(0) of 512M -> "0/512M".
- Size: +512 B (measured). Per tick two PDH collects of a few microseconds; the DXGI query moves from every tick to construction.
- Risks: VRAM shows n/a before Windows 10 1709. A live driver update can change an adapter's LUID; VRAM then shows n/a until a restart or a metric-set change (Apply rebuilds sources only when the metric order changes). An APU reporting a non-zero non-local budget is treated as discrete and shows its carve-out usage, which is still Task Manager's dedicated figure. The module, detect_gpus and VramGroup docs are rewritten in the same commit.

#### C1.2 GPU load is the busiest engine (step 9)

- Findings: samplers[1], render-registry[2].
- Problem: GpuLoadSampler::update takes the maximum over raw `\GPU Engine(*)` instances, each of which is one process on one engine, so processes sharing an engine are never added together; the row reads well below Task Manager (13% against 22% measured).
- Approach: pure `busiest_engine(samples, luid_key: Option<&str>) -> Option<f64>` skips values outside 0..=100.5 and, with a key, other LUIDs; groups instances by `engine_key(name)` (drop the `pid_..._` prefix up to `luid_` and everything from `_engtype_` on); sums each group in a small Vec (about 30 keys, no HashMap); clamps each sum to 100; returns the maximum, or None when nothing matched. Pure `adapter_load(samples, luid_key, single) -> f64` matches the adapter's LUID strictly and, only for a lone adapter that matched nothing, falls back to all instances (so Basic Render Driver load stays out of the hardware row while the adapter's own LUID matches; after a LUID change the lone-adapter fallback reads all engines, Basic Render Driver included); no match gives 0.0 (an adapter nobody uses has no instances). update(): `let Some(samples) = counter.try_sample().filter(|s| !s.is_empty()) else { every load = None }`: a live desktop always has engine instances, so an empty sample (which PDH_NO_DATA can also mean for a restricted token) shows n/a, not 0%.
- Code: `gpu.rs` busiest_engine, engine_key, adapter_load, rewritten update() and its doc; test helper `eng(pid, luid_low, eng, ty, v)`.
- Tests first:
  - `gpu::tests::engine_key_strips_pid_and_engtype`: `pid_10680_luid_0x00000000_0x000109f8_phys_0_eng_0_engtype_3d` -> `luid_0x00000000_0x000109f8_phys_0_eng_0`; also an empty engtype and "gdi render".
  - `load_sums_processes_sharing_an_engine`: 60 + 12 + 8 on eng_0 and 30 on eng_1 -> Some(80) (old code: 60).
  - `load_is_clamped_and_skips_garbage`: 70 + 40 -> 100; instances reading 1e9 and -5 are ignored.
  - `load_matches_adapter_by_luid`: two LUIDs at 10 and 90 stay separate; an unknown LUID -> None; an empty sample -> None.
  - `lone_adapter_uses_its_own_engines_then_falls_back`: iGPU 3D 20% plus Basic Render Driver 3D 70%, single -> 20; only another LUID at 35%, single -> 35; the same with single = false -> 0.
- Size: +512 B (measured). Per tick the same single PDH collect (about 0.3 ms for 450 instances) plus a string scan.
- Risks: readings rise to Task Manager's level, so the red label at 90% or more appears more often under real load. On multi-GPU systems a LUID change after a driver update reads 0% until restart.

#### C1.3 short_name keeps the brand (step 6)

- Findings: samplers[9].
- Problem: "AMD Radeon(TM) Graphics" (Ryzen APUs and Ryzen 7000+ desktop iGPUs) shortens to the bare word "Graphics".
- Approach: normalise whitespace right after junk removal; strip company words ("AMD ", "NVIDIA ") and remember the result as `branded`; strip brand words ("Radeon ", "GeForce "); remove "Graphics" noise as today; if exactly "Graphics" is left, use `branded` minus " Graphics", which is "Radeon". "Radeon Graphics" was rejected: at 105 px it overflows the 92-99 px label column and ellipsizes to the same prefix as the memory row. Every existing output is unchanged.
- Code: `gpu.rs` short_name: the single prefix loop becomes a company loop and a brand loop.
- Tests first: `gpu::tests::ryzen_igpu_keeps_brand`: "AMD Radeon(TM) Graphics" and "AMD Radeon Graphics" -> "Radeon" (old: "Graphics"). `apu_model_still_shortened`: "AMD Radeon(TM) Vega 8 Graphics" -> "Vega 8"; "AMD Radeon(TM) 780M Graphics" -> "780M"; the five existing short_name tests stay green.
- Size: +512 B (an alignment step; the code growth is smaller).
- Risks: minimal; odd spacing such as "AMD Radeon (TM) Graphics" now also normalises to "Radeon".

#### C1.4 Readings that cannot be taken show n/a (step 8)

- Findings: samplers[10].
- Problem: PdhCounter::sample returns an empty Vec both on failure and when there are no instances. Disk and net then show 0B/s forever, per-core shows no rows, GPU shows 0%, and with no hardware adapter (RDP, a VM with only the Basic Render Driver) the default-on GPU metric shows no rows at all.
- Approach:
  - `pdh.rs` `try_sample(&self) -> Option<Vec<(String, f64)>>`: None when the counter was never added, PdhCollectQueryData fails with anything but PDH_NO_DATA, the sizing call returns anything but PDH_MORE_DATA, 0 or PDH_NO_DATA, or the fill call fails; Some(empty) on PDH_NO_DATA. Its doc states that Some(empty) means "no instance reported" (no instances right now, a missing single instance, or restricted access), not proof of health; callers whose reading can never legitimately be empty map it to n/a. sample() becomes `try_sample().unwrap_or_default()`; the unused ok() is deleted.
  - Disk: read/write become `Option<f64>`; `total(Option<Vec>) -> Option<f64>` reads the single _Total instance (None for an empty sample); `disk_row(label, Option<f64>)`.
  - Net (PDH version until C1.6): `sum_real(Option<Vec>) -> Option<f64>`, where Some(empty) is a real idle Some(0.0); `net_row(label, Option<f64>)`.
  - disk_row and net_row build `match bps { Some(b) => Row::rate(..), None => Row::na(label) }`.
  - Per-core: `cores: Vec<(usize, Option<f64>)>`; pure `next_cores(prev, sample)` parses the numbered instances and, whenever no numbered core comes back (None, empty, or _Total only), keeps the known indices with None, so the row count stays stable; core_rows(): no core ever seen -> one "Cores n/a" row; a None value -> "C{i} n/a".
  - GPU: update() sets every load to None when try_sample is None; `gpu_row(name, None)` returns Row::na (no empty bar track); new free fns `load_rows(gpus, out)` and `vram_rows(gpus, out)` emit one "GPU n/a" / "VRAM n/a" row when the adapter list is empty.
- Code: `pdh.rs` (try_sample, PDH_MORE_DATA and PDH_NO_DATA imports, struct doc; its new test module goes at the end of the file for clippy's items_after_test_module), `registry.rs` (Row::na; sample_rows wraps disk and net values in Some), `disk.rs`, `net.rs`, `percpu.rs`, `gpu.rs`.
- Tests first:
  - `pdh::tests::missing_counter_is_a_failure_not_an_empty_sample`: `\No Such Object(*)\No Such Counter` -> try_sample() None; sample() empty.
  - `pdh::tests::healthy_counter_yields_a_sample`: `\Memory\Available Bytes` (instantaneous, so timing-independent) -> Some with exactly one instance.
  - `registry::tests::na_row_is_value_only`: Row::na("Disk R") has value "n/a", bar None and alert false.
  - `disk::tests::unavailable_is_na_not_zero` and `total_reads_the_single_instance`: disk_row("Disk W", None) is n/a with bar None; total(Some([_total 42])) = Some(42); total(Some([])) = None; total(None) = None; the existing formats test passes Some(..).
  - `net::tests::unavailable_is_na_not_zero`: net_row(None) is n/a; sum_real(None) = None; sum_real(Some([])) = Some(0.0); loopback still filtered.
  - `percpu::tests::parses_sorted_cores_without_total` ("10", "_total", "2" -> [(2, Some), (10, Some)]); `failed_sample_keeps_cores_as_na` (a None sample keeps 2 rows as n/a; Some([("_total", 5.0)]) also gives [(0, None), (1, None)]); `no_cores_shows_one_na_row`.
  - `gpu::tests::gpu_row_no_load` (rewritten: bar None) and `no_hardware_adapter_shows_na_rows`: load_rows(&[]) then vram_rows(&[]) -> [("GPU", "n/a", None), ("VRAM", "n/a", None)].
- Size: +512 B (measured).
- Risks: a one-off collection failure shows n/a for one tick instead of 0; n/a GPU rows lose their empty bar track and match the Temp and Battery n/a rows; sparklines pause on n/a ticks (C2.1a) instead of dipping to 0.

#### C1.5 Rate metrics show a pending marker until a full interval exists (step 14)

- Findings: samplers[7].
- Problem: build_sources and PdhCounter::new take the rate baseline, and main.rs samples every source immediately (at startup and on every metric Apply) to learn row counts. The first frame's CPU, per-core, GPU, disk and net values cover only construction time, are arbitrary (all cores read 100% in one run and 0% in another, 45-53 us after the baseline) and stay on screen for a full interval, up to 10 s; at 100% every per-core label is painted red.
- Approach: keep the priming sample (it gives instantaneous metrics their first value and variable-row metrics their row count) but never display a rate value from it.
  - `MetricKey::is_rate()` as an exhaustive match (no wildcard), so a new metric must be classified; `PENDING`; `mask_pending(rows)` (5.2).
  - `collect_rows(sources, pending)` masks rate sources while pending; WM_PAINT passes `st.first_tick_pending`, WM_TIMER passes false.
  - `AppState.first_tick_pending` is true at startup and set again whenever apply_settings rebuilds the sources.
  - Pure `first_tick_ms(interval) = interval.min(1000)`. WM_CREATE and a metric-changing Apply arm TIMER_ID with it (re-setting an existing timer id restarts its countdown, so the first real sample after Apply covers a full period); the first tick clears the flag and, if the capped period was used, re-arms at interval_ms. The marker shows for at most 1 s, and the first real value averages a full min(interval, 1 s) period.
  - After a metric Apply, rate rows of metrics that stay enabled show the marker over their real sparkline trend (history is kept, C2.1a); the masked bar only suppresses the bogus value and the bogus red alert. Only a fresh start shows an empty track.
- Code: `registry.rs` (is_rate, PENDING, mask_pending, the pending flag of collect_rows); `main.rs` (AppState field, WM_PAINT, WM_TIMER, WM_CREATE, apply_settings, first_tick_ms); rewrite the startup priming comment.
- Tests first:
  - `registry::tests::rate_metrics_are_flagged`: over MetricKey::ALL, is_rate is true exactly for Cpu, PerCpu, Gpu, Disk and Net.
  - `pending_mask_hides_values_until_first_full_interval`: cpu_row(73.0) -> value "\u{2026}", bar Some(0.0); Row::na("GPU") -> "\u{2026}" with bar None.
  - `pending_mask_clears_a_bogus_red_alert`: percpu_row(0, 100.0) masked -> is_critical false.
  - `collect_rows_masks_only_rate_metrics_while_pending`: build_sources with [PerCpu, Ram], primed with sample(); pending = true -> every core row "\u{2026}" with bar Some(0.0) and RAM real; pending = false -> nothing masked.
  - `main::tests::first_tick_is_capped_at_one_second`: 250 -> 250; 1000 -> 1000; 10000 -> 1000.
- Size: about +0.5 KB.
- Risks: the marker appears for up to 1 s at every start and after every metric change, also for rate metrics that did not change (Apply rebuilds all sources).

#### C1.6 Network counts hardware NICs from GetIfTable2 (step 11)

- Findings: samplers[5].
- Problem: net.rs sums every `\Network Interface(*)` instance except loopback, isatap and teredo, so correctness depends on which interfaces the PDH provider happens to publish (undocumented); any virtual adapter it lists (VPN/TAP/Wintun, virtual switch, NIC team) would count the same bytes twice. On the development machine the set happens to be only the physical NIC.
- Approach: read MIB_IF_ROW2 rows with GetIfTable2 (the documented source the PDH network counters are themselves filled from) and choose interfaces with a pure selector: hardware interfaces (InterfaceAndOperStatusFlags bit 0) that are not filter modules (bit 1); if there are none (for example a VM guest whose synthetic NIC does not report HardwareInterface), interfaces with ConnectorPresent (bit 2) that are not filters; if still none, None, which shows n/a instead of a silent 0. TAP and Wintun report no connector, so VPN adapters stay excluded in both passes. Rates come from 64-bit InOctets/OutOctets deltas tracked per LUID (an interface that appears or resets contributes 0, not a spike), over QueryPerformanceCounter time taken at the same point relative to the read in new() and sample().
- Code: `Cargo.toml` adds `Win32_NetworkManagement_IpHelper` and `Win32_NetworkManagement_Ndis`. `net.rs`: remove the PDH counters and is_pseudo; consts IF_HARDWARE = 1, IF_FILTER = 2, IF_CONNECTOR = 4; `type IfOctets = (u64, u64, u64)` (luid, in, out); `read_if_rows() -> Option<Vec<(u8, IfOctets)>>` (GetIfTable2, rows read through `addr_of!((*table).Table).cast::<MIB_IF_ROW2>()` for NumEntries rows without indexing past the declared `[T; 1]`, then FreeMibTable); pure `pick_nics(rows) -> Option<Vec<IfOctets>>`; pure `octet_delta(prev, cur) -> (u64, u64)` over LUIDs present in both, saturating per direction; pure `rate(bytes, secs) -> Option<f64>`; `now_secs()`; `NetSampler { prev: Option<(f64, Vec<IfOctets>)>, up_bps, down_bps: Option<f64> }`; `net_row(label, Option<f64>)` kept. `pdh.rs` module doc: network no longer uses PDH, VRAM does.
- Tests first:
  - `net::tests::falls_back_to_connector_nics_only_when_no_hardware_nic`: flags [0b101, 0b010, 0b100, 0b000] -> the 0b101 row; without it -> the 0b100 row; [0b110, 0b000] -> None (filters never count); [0b010, 0b000] -> None.
  - `delta_sums_interfaces_seen_twice`: prev [(1,100,50), (2,1000,10)], cur [(1,150,80), (2,1500,10)] -> (550, 30).
  - `delta_ignores_new_and_reset_interfaces`: prev [(1,100,50)], cur [(1,90,60), (3,5000,5000)] -> (0, 10).
  - `rate_needs_elapsed_time`: rate(1000, 0.5) = Some(2000.0); rate(5, 0.0) = None.
  - `formats` and `unavailable_is_na_not_zero` (kept from C1.4).
- Size: +512 B (measured). Per tick about 0.5 ms instead of two PDH collects of about 0.6 ms each.
- Risks: traffic that never reaches a physical NIC (host to WSL or Hyper-V internal switch, loopback) is no longer counted, as intended. VM guest NIC flags are unverified, hence the connector fallback and the n/a floor. With Azure accelerated networking (a synthetic NIC and a hardware VF sharing one MAC) any rule, PDH included, may count twice. Two more windows-crate modules compile (build time only). If the owner preferred PDH, this commit reverts on its own.

#### C1.7 luid_key helper (step 1)

- Findings: none (test hygiene); its #[ignore] half is part of SP0 0d.
- Problem: the PDH matching of C1.1 and C1.2 depends on the LUID key format, which is built inline in detect_gpus and never asserted.
- Approach and code: extract `fn luid_key(high: i32, low: u32) -> String` from detect_gpus (HighPart printed as unsigned hex).
- Tests first: `gpu::tests::luid_key_matches_pdh_instance_names`: luid_key(0, 0x109f8) = "luid_0x00000000_0x000109f8"; luid_key(-1, 0xdeadbeef) = "luid_0xffffffff_0xdeadbeef"; the key is a prefix of the lowercased "luid_0x00000000_0x000109F8_phys_0".
- Size: 0 B. Risks: none.

### 6.2 C2 - Widget shell

#### C2.1a Stable row identity and sparkline edge cases (step 13)

- Findings: render-registry[6], render-registry[3], render-registry[4], samplers[6] (history part).
- Problem: update_history keys history by flat row index, rebuilds every series whenever the total row count changes and pushes 0.0 for bar-less rows. After every rebuild each bar row has one sample, and draw_sparkline turns n == 1 into an empty track for a full interval; n/a ticks draw false dips to zero; and y_at maps 0..1 onto th + 1 rows, so the 1-px top line of an idle metric lands one pixel below the track (and a steep last segment one pixel right of it).
- Approach:
  - New pure module `history.rs` (5.3). History survives Apply.
  - `registry::collect_rows(sources)` is the single flatten that tags every row with its source's key; this step creates it, C1.5 adds the pending flag.
  - `render.rs`: render_rows takes `history: &[&[f32]]`; nothing else in its signature changes. Pure `bar_style(row, samples) -> BarStyle { None, Level, Spark }`: None without a bar; Spark only with at least 2 samples and no bar_color; otherwise Level (0-1 samples draw the solid level bar). Pure inset geometry: `spark_line(track, hist)` puts x on [left, right - 1] and y = bottom - 1 - round((th - 1) * v), dividing by `(n - 1).max(1)` so n == 1 is a flat line and never divides by zero (release builds abort on panic), plus a horizontal stub to (right, y_last) so the last column is painted while GDI's excluded endpoint stays outside; `spark_area(track, line)` closes the polygon on (right, bottom) and (left, bottom), which GDI excludes. draw_sparkline loses its n == 1 and n >= 2 branches.
  - `settings_win::paint_preview` passes its canned series as slices (one line).
- Code: `src/history.rs` (new; HIST_LEN moves here); `registry.rs` collect_rows; `main.rs` (`mod history`, AppState.history, delete update_history, WM_TIMER and WM_PAINT through collect_rows and row_ids, no history.clear() in apply_settings); `render.rs`; `settings_win.rs`; stale comments (AppState.history doc, render_rows' history doc, the draw_sparkline doc and its "skipped for a single sample").
- Tests first:
  - `history::tests::row_count_change_in_one_metric_keeps_other_histories`: [Cpu "CPU" 0.1, Temp "A" 0.5, Temp "B" 0.6] then [Cpu "CPU" 0.2, Temp "A" 0.5] -> CPU [0.1, 0.2], A [0.5, 0.5], B empty.
  - `bar_less_rows_record_nothing`: Temp "Temp" with bar None leaves get() empty; a later Temp "ACPI Temp" 0.48 gives exactly [0.48] (no 0.0 dip).
  - `na_tick_pauses_series_without_dropping_it`: [0.2, 0.3], then a None tick, then 0.4 -> [0.2, 0.3, 0.4].
  - `duplicate_labels_keep_separate_series`: two Temp "Temperature" rows (0.3, 0.7) recorded twice -> nth 0 and 1, histories [0.3, 0.3] and [0.7, 0.7].
  - `reorder_keeps_histories`: [Cpu, Ram] then [Ram, Cpu] -> both series have 2 samples.
  - `history_is_capped_at_hist_len`: 70 records -> length 64, the first element is the 7th recorded value.
  - `fixed_color_rows_are_not_recorded`: battery_row(present) recorded three times -> get() is empty.
  - `registry::tests::collect_rows_tags_each_row_with_its_metric`: [Uptime { ms }, Ram { info }] -> labels ["Up", "RAM"], keys [Uptime, Ram].
  - `render::tests::bar_style_needs_two_samples_for_a_sparkline`: cpu_row(30.0) with 0 or 1 samples -> Level; with 2 -> Spark; battery_row(present) with 64 -> Level; uptime_row -> None.
  - `spark_line_stays_inside_track`: track (8,50)-(192,58), hist [0.0, 1.0, 0.5] -> (8,57), (99,50), (191,53) plus the stub (192,53).
  - `spark_line_single_sample_stays_inside`: one sample, no panic, every point within x 8..=192 and y 50..=57.
  - `spark_area_closes_on_track_bottom`: the area ends with (192,58), (8,58).
- Size: about +1 KB.
- Risks: the render_rows signature change touches settings_win (one line). Rows are matched by linear string compares (about 40 rows per tick and paint, negligible). A row missing for a single tick (one invalid per-core instance) restarts its sparkline.

#### C2.1b Resize when the row count changes at runtime (step 18)

- Findings: app-shell[1], render-registry[1], samplers[6] (resize part), docs-spec[24].
- Problem: the height is recomputed only at startup, on Apply and on a DPI change; WM_TIMER never resizes, although Temp switches between LHM rows, zone rows and one n/a row (and zones appear at runtime after C4.2), so rows are clipped or a blank band remains.
- Approach: pure `layout_dirty(sized, n, in_move, force) = (force || n != sized) && !in_move`. `sync_layout(hwnd, n, force) -> bool` calls `relayout(hwnd, None, anchor: true, track_home: true)` (C2.5) when dirty and returns whether home changed. relayout and place_widget store `sized_rows`, the total row count (also above the C2.2 cap). WM_TIMER calls it after its borrow ends and persists only when home changed; apply_settings calls it with `force = order_changed || font_changed`. During a drag it is skipped and the first tick after the drag catches up.
- Code: `main.rs` layout_dirty, sync_layout, the WM_TIMER and apply_settings call sites; fix the WM_TIMER comment ("once per second", although the interval is configurable).
- Tests first: `main::tests::layout_dirty_on_count_change_but_not_mid_drag`: (5, 6, false, false) -> true; (5, 5, false, false) -> false; (5, 6, true, false) -> false; (5, 5, false, true) -> true.
- Size: about +0.15 KB.
- Risks: a flapping source now makes the widget resize back and forth instead of clipping (visible and acceptable). The runtime trigger cannot be produced on the development machine (no LHM WMI namespace, one constant thermal zone, all per-core instances valid at priming), so the shared path is verified through Apply and re-checked after C4.2 (8.3).

#### C2.2 Cap the height to the work area and show '+N' (step 19)

- Findings: render-registry[7]; app-shell[2] (height clamp part).
- Problem: window_height grows linearly and nothing caps it: per-core CPU on 16 or more threads (20 rows at 150% = 1154 px; 36 rows at 100% = 1332 px) runs past the screen, and snap-to-bottom pushes the header above the top.
- Approach: show only whole rows that fit and say how many are hidden. Pure `rows_fitting(scale, font_px, max_h, n)` (the largest k <= n with window_height(k) <= max_h), `widget_size(scale, font_px, n, max_h) -> (w, h, shown)` and `overflow_label(total, shown) -> Option<String>`. place_widget and relayout size with the target monitor's work-area height. render_rows computes `shown = rows_fitting(.., h, rows.len())`, draws `rows[..shown]`, sizes the uniform value column over `rows[..shown]` only (a wide value in a hidden row must not narrow the visible labels) and right-aligns the '+N' label in the header band in header_fg. Hidden rows are the last in the user's order, so moving important metrics above Per-core keeps them visible. The settings preview renders at natural size (C3.6b) and never shows '+N': the cap depends on the widget's monitor, which the preview does not know.
- Code: `render.rs` (three pure fns, render_rows; window_height and render_rows docs); `main.rs` place_widget and relayout use widget_size.
- Tests first:
  - `render::tests::rows_fitting_inverts_window_height`: for n in 0..40 at Scale(96)/13 and Scale(144)/16, rows_fitting(window_height(n), 40) == n, and for n >= 1 rows_fitting(window_height(n) - 1, 40) == n - 1.
  - `widget_size_caps_height_to_work_area`: (Scale(144), 13, 20 rows, 1020) -> (300, 989, 17), the 1080p at 150% case; (Scale(96), 13, 5, 1080) -> (200, 216, 5); shown <= n, and h == window_height(shown) when capped.
  - `overflow_label_counts_hidden_rows`: (20, 17) -> Some("+3"); (5, 5) -> None.
- Size: about +0.4 KB.
- Risks: hidden rows are signalled only by the count; the compact per-core layout is SP3.

#### C2.3 The red alert is for load rows only (step 7)

- Findings: render-registry[5].
- Problem: draw_row turns a label red when bar_color is None and the bar is at least 0.9, so Disk and Net (throughput over soft ceilings of 100 MiB/s and 12.5 MB/s) raise an alarm on routine transfers.
- Approach: the alert becomes a property of the row (5.2): Row::bar sets it, the new Row::rate clears it (disk_row, net_row), the literals (uptime, two battery rows, temp n/a) set false; `render::is_critical(row)` with `ALERT_AT = 0.9`. Bars keep load_color; only the label alarm changes. The battery exemption is now simply battery's alert = false.
- Code: `registry.rs` (Row, Row::bar, Row::rate, uptime_row, Row doc); `disk.rs`, `net.rs`; `battery.rs`, `temp.rs` literals; `render.rs` (ALERT_AT, is_critical, draw_row; the "Battery (bar_color set) is exempt" comment). This step lands before C1.4 (so Row::na is born with alert false) and before C2.1a reworks draw_row; its test vectors use the row-builder signatures of the day and are updated when C1.4 and C1.1 change them.
- Tests first:
  - `render::tests::throughput_rows_never_alert`: disk_row("Disk R", 500 MiB/s) and net_row("Net\u{2193}", 50e6) are not critical (both bars are 1.0 with bar_color None; today's predicate says true).
  - `load_rows_alert_from_ninety_percent`: cpu_row(90.0), ram_row at 95%, gpu_row("GPU", Some(97.0)), vram_row at 95%, percpu_row(0, 99.0) and (temp builds) temp_row("CPU", 92.0) are critical; cpu_row(89.0) is not.
  - `battery_and_value_rows_never_alert`: battery_row(present, 100%) and uptime_row(..) are not critical.
- Size: about 0 B.
- Risks: none; a future FPS row (the fps.rs sketch) must use Row::rate.

#### C2.4 Tray: Explorer restarts, failed adds, double-clicks, crisp icon (step 21)

- Findings: app-shell[0], app-shell[9], app-shell[8].
- Problem: the tray icon is added once in WM_CREATE, NIM_ADD's result is ignored and TaskbarCreated is not handled, so after an Explorer restart a hidden or locked widget has no way back. Version-0 callbacks deliver DOWN, UP, DBLCLK, UP for a double-click and every UP toggles, so the widget flashes and ends where it started. The tray always gets a 16 px glyph (blurry above 100%), while the 32 px icon sits on a tool-window class that never shows it.
- Approach:
  - main() stores `TASKBAR_CREATED` (static AtomicU32) from RegisterWindowMessageW. WM_CREATE calls `ChangeWindowMessageFilterEx(hwnd, TASKBAR_CREATED, MSGFLT_ALLOW, None)` so the broadcast also reaches an elevated instance (UIPI silently drops registered messages from lower integrity; harmless at medium); debug builds assert that the call succeeded. A guard arm `m if m != 0 && m == TASKBAR_CREATED` re-adds the icon.
  - `add_tray_icon(hwnd) -> bool`: NIM_ADD, falling back to NIM_MODIFY (which succeeds if a timed-out ADD actually landed), so it is idempotent (C2.6 relies on that). On failure a TRAY_TIMER_ID timer retries every TRAY_RETRY_MS = 2000 up to TRAY_MAX_TRIES = 10; `tray_tries` is reset whenever the retry (re)starts. WM_TIMER dispatches on wParam (5.7).
  - Pure `tray_left_click(swallow: &mut bool, mouse_msg) -> bool`: WM_LBUTTONDOWN clears the flag, WM_LBUTTONDBLCLK sets it, WM_LBUTTONUP toggles unless the flag is set (then clears it). A double-click toggles once, and an UP lost after a double-click cannot eat the next click. The flag lives in AppState.tray_dblclk; the borrow ends before toggle_visibility. WM_RBUTTONUP is unchanged.
  - The tray icon is drawn at `GetSystemMetricsForDpi(SM_CXSMICON, tray_dpi)`, where tray_dpi is `GetDpiForWindow(FindWindowW("Shell_TrayWnd", null))` with `dpi::system_dpi()` as fallback (GetSystemMetrics is documented as unsuitable for per-monitor-aware threads). Windows 10 re-broadcasts TaskbarCreated when the primary display's DPI changes, so the TaskbarCreated arm rebuilds icon_sm at the current tray DPI before re-adding it and then destroys the old icon (the shell keeps its own copy).
  - Because this step rewrites icon creation in main(), it also stops creating the unused 32 px class icon and fixes icon.rs's stale "no .ico asset" header. Unifying the runtime glyph with icon.ico's artwork is SP3.
- Code: `main.rs` (TASKBAR_CREATED, TRAY_TIMER_ID, TRAY_RETRY_MS, TRAY_MAX_TRIES, add_tray_icon, tray_dpi, tray_left_click, WM_TIMER match, guard arm, WM_TRAY, AppState fields, class icon removal, comments in main() and WM_CREATE); `icon.rs` header.
- Tests first:
  - `main::tests::tray_double_click_toggles_once`: [DOWN, UP, DBLCLK, UP] -> exactly one toggle (today: two).
  - `tray_separate_clicks_each_toggle`: [DOWN, UP, DOWN, UP] -> two.
  - `tray_lost_up_after_double_click_does_not_eat_next_click`: [DOWN, UP, DBLCLK, DOWN, UP] -> two.
  - `icon::tests::glyph_paints_at_small_icon_sizes`: paint_icon at 20, 24 and 32 px keeps a transparent corner, an opaque background and the green bar.
- Size: about +0.75 KB.
- Risks: while the shell hangs, each retry can block the UI thread for Shell_NotifyIconW's internal timeout; the 10-try cap bounds it.

#### C2.5 On-screen placement with a remembered home (steps 15-17)

- Findings: app-shell[2], settings-ui[9].
- Problem: settings.pos goes into CreateWindowExW unchecked, so a position on a missing monitor opens the widget off-screen; resizes keep the top-left fixed with no work-area clamp; nothing handles display or work-area changes; the first-run default uses SM_CXSCREEN and an unscaled margin. persist() saves whatever GetWindowRect returns, so any automatic correction would overwrite the user's spot for good, for example when the left monitor is asleep or undetected at logon.
- Approach: settings.pos becomes the widget's home, the spot the user chose. It changes only on a drag that really moved the window, on Move to corner, or when an unclamped anchored resize moves the widget while it is at home. Every change of home is persisted; `persist(st)` writes the settings as they are and never reads GetWindowRect.
  - Step 15, `placement.rs` (pure except two shims): `fit_into(x, y, w, h, wa) -> (x, y)`, the least move that puts the rect inside wa, the top/left edge winning when the rect is larger; `anchored(old, w, h, wa) -> ((x, y), clamped)` keeps the right edge when old's centre is in wa's right half and the bottom edge when it is in the lower half (a corner-snapped widget grows away from its corner), then fits and reports whether fitting moved it; `corner_xy(corner, w, h, wa, margin)` (today's snap math, fitted; corner 0..3 = TL, TR, BL, BR as ID_SNAP_*); `place(home, w, h, wa) -> ((x, y), displaced)`; `after_relayout(home, spot, clamped, displaced, in_move, track_home) -> (home, displaced)`: a clamped result marks the widget displaced and keeps home, and a new spot becomes home only when `track_home && !clamped && !displaced && !in_move`; `drag_end(start, end) -> Option<(i32, i32)>` adopts only a real move. Shims shared with C3.6a: `work_area_near(&RECT)` (MonitorFromRect NEAREST + GetMonitorInfoW rcWork, falling back to the SM_CXSCREEN x SM_CYSCREEN rect) and `primary_work_area()` (MonitorFromPoint((0,0), MONITOR_DEFAULTTOPRIMARY)).
  - Step 16, home semantics: main() creates the window at a provisional rect; `place_widget(hwnd)` corrects it in WM_CREATE before the first show and runs again on WM_DISPLAYCHANGE, on WM_SETTINGCHANGE with wParam SPI_SETWORKAREA (taskbar moved, resized, auto-hide toggled, re-docked after a monitor change) and from C2.6's surface. It sizes with widget_size (C2.2), places with place(), stores displaced and sized_rows, and does one SetWindowPos outside any borrow; if that crosses monitors, the synchronous WM_DPICHANGED updates dpi, font and size. With no saved home (first run) it computes `corner_xy(top-right)` on the primary work area with `Scale(dpi).px(MARGIN)` and adopts that spot as home (persisted), so x/y are written after the first run as today. WM_ENTERSIZEMOVE sets in_move and records move_start; WM_EXITSIZEMOVE clears in_move and, through drag_end, adopts the new top-left as home (clearing displaced) only if the window actually moved, so clicking a displaced widget (for example to focus it for Esc) does not make the temporary spot permanent. snap_to_corner uses corner_xy on the current monitor's work area and sets home.
  - Step 17, resizes: `relayout(hwnd, from: Option<RECT>, anchor, track_home) -> bool` replaces apply_window_size and the inline WM_DPICHANGED sizing: anchored and fitted unless a drag is in progress, home decided by after_relayout, sized_rows stored, SetWindowPos outside the borrow, returns whether home changed. WM_DPICHANGED updates dpi and font, then calls relayout with `Some(suggested)` when lParam is non-null (None otherwise), `anchor = !in_move` and `track_home = false`, so a system relocation never becomes home. apply_settings resizes, then persists last (5.4).
  - While displaced, home is kept: when the missing monitor returns, the next WM_DISPLAYCHANGE puts the widget back.
- Code: `src/placement.rs` (new); `main.rs` (`mod placement`, AppState displaced, in_move, move_start, sized_rows; place_widget, relayout; new arms WM_ENTERSIZEMOVE, WM_DISPLAYCHANGE, WM_SETTINGCHANGE; rewritten WM_EXITSIZEMOVE, WM_DPICHANGED, snap_to_corner and persist; apply_window_size removed; stale persist, WM_EXITSIZEMOVE and WM_DPICHANGED comments).
- Tests first (placement::tests):
  - `fit_keeps_owner_spot_on_left_monitor`: fit_into(-213, -16, 200, 216, (-1920,-39,0,1041)) == (-213, -16).
  - `fit_pulls_orphaned_spot_onto_primary`: the same rect into (0,0,1920,1080) -> (0, 0).
  - `fit_far_off_spot_lands_in_nearest_corner`: (5000, 5000, 200, 216) -> (1720, 864).
  - `fit_oversized_pins_top_left`: (100, 100, 200, 2000) -> (100, 0).
  - `anchored_bottom_right_widget_grows_up_and_left`: old (1696,840)-(1896,1056), new 215 x 252 in (0,0,1920,1080) -> (1681, 804), not clamped (SWP_NOMOVE today would push the bottom to 1092).
  - `anchored_right_half_keeps_right_edge`: old (-213,-16)-(-13,200), new 215 x 216 on the left monitor -> (-228, -16); the right edge stays at -13.
  - `corner_xy_matches_snap_math`: TR, 200 x 216, margin 24 in (0,0,1920,1080) -> (1696, 24); BR -> (1696, 840).
  - `place_reports_displacement`: home (-213,-16) with both monitors -> ((-213,-16), false); with only the primary -> ((0,0), true).
  - `clamped_resize_keeps_home`: after_relayout(home Some((-213,-16)), spot (-228,-16), clamped true, displaced false, in_move false, track_home true) == (Some((-213,-16)), true): home kept, widget marked displaced.
  - `unclamped_resize_moves_home`: the same with clamped false == (Some((-228,-16)), false); with in_move true, track_home false or displaced true instead, home stays (-213,-16).
  - `click_without_move_keeps_home`: drag_end(p, p) == None for p = (-213,-16); drag_end(p, q) == Some(q) for q = (-180,40).
- Size: about +1.4 KB over three commits (+0.4, +0.6, +0.4).
- Risks: x/y now mean home, so every future move path must set settings.pos explicitly. A widget parked partly off-screen is pulled inside at the next start, resize or display change, while its home is kept until the user drags it. A widget straddling two monitors resolves to the one with the larger overlap. Visible change: a larger font grows a right-anchored widget to the left (its right edge stays put) instead of across onto the next monitor.

#### C2.6 Single instance (step 36)

- Findings: app-shell[3].
- Problem: a second launch adds a second widget and tray icon, its hotkeys silently fail to register, and both processes overwrite tinymon.cfg (the last writer wins).
- Approach: `claim_single_instance()` runs first in main()'s unsafe block, before any file I/O: `CreateMutexW(None, false, "Local\TinyMon.Instance")` (the wrapper reads GetLastError only on failure, so ERROR_ALREADY_EXISTS survives a successful call). Pure `instance_action(created_ok, last_error, debug) -> bool` (true = keep running): a debug build always runs (so `cargo run` and debug test copies work next to an installed release); a successful create with ERROR_ALREADY_EXISTS exits after `FindWindowW(CLASS_NAME, null)` (it finds hidden windows too), `PostMessageW(WM_APP_SURFACE)` and CloseHandle; a failed create (for example ERROR_ACCESS_DENIED against an elevated first instance) runs. Otherwise the handle is held for the process lifetime. In the running instance WM_APP_SURFACE calls `surface(hwnd)`: visible = true, place_widget (back home or on-screen), show_no_activate (C2.8), add_tray_icon (C2.4, idempotent).
- Code: `Cargo.toml` adds `Win32_Security`; `main.rs` claim_single_instance, instance_action, surface, WM_APP_SURFACE = WM_APP + 4 and its arm; wm_app_ids_are_distinct gains +4.
- Tests first: `main::tests::instance_action_exits_only_for_an_existing_instance`: (true, ERROR_ALREADY_EXISTS, false) -> exit; (true, 0, false) -> run; (true, ERROR_ALREADY_EXISTS, true) -> run; (false, ERROR_ACCESS_DENIED, false) -> run.
- Size: about +0.4 KB.
- Risks: portable copies in different folders can no longer run at the same time (intended); relaunching from another copy surfaces the first one. A medium-integrity launch next to an elevated instance runs as a second instance (it could not post across UIPI anyway). Of two launches milliseconds apart, the second exits. Landed late so no earlier harness can surface the owner's widget; verified in the live session.

#### C2.7 Start with Windows (step 37)

- Findings: app-shell[4].
- Problem: is_enabled() only checks that a "TinyMon" Run value exists, so a stale path, another copy's path or an entry disabled in Task Manager all show as checked, and toggling then deletes the entry instead of repairing it; set_enabled writes the path unquoted.
- Approach: pure helpers in `startup.rs`: `run_command(exe)` wraps the path in double quotes; `launches(value, exe)` trims the value, takes the text inside a leading quote pair (or the whole value, the legacy unquoted form) and compares case-insensitively; `approved_disabled(data)` reads the low bit of the first byte of `Explorer\StartupApproved\Run\TinyMon` (Task Manager writes 02... for enabled and 03... for disabled; the format is undocumented, so the doc comment calls it a heuristic pinned to the observed forms). `is_enabled()` is true when the Run value (RegGetValueW, RRF_RT_REG_SZ) launches current_exe() and StartupApproved (RRF_RT_REG_BINARY) is not disabled. `set_enabled(true)` writes run_command(exe) as REG_SZ and deletes the StartupApproved value (absent means enabled; ERROR_FILE_NOT_FOUND is fine); `set_enabled(false)` deletes the Run value as today. The menu keeps calling `set_enabled(!is_enabled())`, which now repairs a stale or foreign entry.
- Code: `startup.rs` (STARTUP_APPROVED, run_command, launches, approved_disabled, a RegGetValueW helper with a fixed 1024-unit buffer for the string and 16 bytes for the binary, is_enabled, set_enabled, exe_path_bytes built from run_command, is_enabled doc). main.rs is unchanged.
- Tests first:
  - `startup::tests::run_command_quotes_the_exe_path`: `C:\Program Files\TinyMon\tinymon.exe` -> the same path in double quotes.
  - `launches_accepts_quoted_and_legacy_values_case_insensitively`: an unquoted `D:\TinyMon\target\release\tinymon.exe` and its quoted form both launch `d:\tinymon\TARGET\release\tinymon.exe`.
  - `launches_rejects_stale_or_other_copy`: `D:\old\tinymon.exe` and `"C:\a\tinymon.exe" --flag` against `C:\b\tinymon.exe` -> false.
  - `approved_disabled_reads_task_manager_flag`: [0x02, 0, ...] -> false; [0x03, 0, ...] -> true; [] -> false.
- Size: about +0.5 KB.
- Risks: the StartupApproved format is undocumented; the worst case is a wrong checkmark, and a click re-enables and clears the flag. A debug build's menu shows unchecked (the Run value points at the release exe) and clicking it repoints autostart at the debug exe, so harnesses never send ID_STARTUP outside the live session. TinyMon cannot detect a removed or quarantined exe beyond the path check.

#### C2.8 Show the widget without activating it (step 20)

- Findings: app-shell[5].
- Problem: toggle_visibility and the startup show use ShowWindow(SW_SHOW), which activates the widget; a toggle-hotkey press gives TinyMon foreground rights, so it steals keyboard focus from the app the user is in, and a fullscreen-exclusive game typically minimises.
- Approach: `show_no_activate(hwnd) = SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW)`: shown without activation and raised to the top of the topmost band (SW_SHOWNA alone could leave it behind another always-on-top window). Used by toggle_visibility's show branch (flip and copy `visible` under a short borrow, call outside it), the startup show in main() (UpdateWindow kept) and C2.6's surface. Window styles are untouched, so HTCAPTION dragging, the window and tray menus (both call SetForegroundWindow before TrackPopupMenu) and click-to-activate for Esc-to-hide keep working. WS_EX_NOACTIVATE was rejected because it would break Esc-to-hide.
- Code: `main.rs` show_no_activate, toggle_visibility, main(); drop the SW_SHOW import; fix the toggle_visibility doc ("tray-only").
- Tests first: none (pure Win32). The focus behaviour is confirmed by the owner in the live session (8.4).
- Size: about 0 B.
- Risks: the widget also no longer takes focus at startup, which is intended.

#### C2.9 Apply hand-off without a pointer in wParam (step 12)

- Findings: app-shell[6], app-shell[11].
- Problem: settings_win::apply posts a `Box<Settings>` pointer in wParam, and WM_APP_APPLY `Box::from_raw`s any non-zero wParam, which any process on the desktop can post to the fixed window class. WM_APP_APPLY also keeps `&mut AppState` alive across the modal hotkey-conflict MessageBoxW; its nested loop re-enters wnd_proc (WM_TIMER creates a second `&mut`, and Exit from the tray frees AppState before the arm's persist runs): undefined behaviour and a use-after-free.
- Approach: the hand-off of 5.4. A same-thread `thread_local PENDING_APPLY: RefCell<Option<Settings>>` replaces the pointer (sound because the settings window lives on the UI thread); a foreign or duplicate WM_APP_APPLY finds the slot empty and does nothing. apply_settings does every state change under short borrows in the fixed order and returns the warning text, which the arm shows afterwards with no AppState reference. This step introduces the borrow rule; later steps fill in the parts of apply_settings that belong to them, and here the existing resize is already split so SetWindowPos runs after the borrow ends. WM_APP_SETTINGS_CLOSED clears settings_hwnd only when `!IsWindow(settings_hwnd)`, so a foreign post cannot open a second settings window sharing the thread_local METRIC_STATE.
- Code: `main.rs` (PENDING_APPLY, stash_apply, `queue_apply(main, new) -> bool` that takes the slot back if PostMessageW fails, take_pending_apply, apply_settings, show_warning replacing warn_hotkey_conflicts, the WM_APP_APPLY and WM_APP_SETTINGS_CLOSED arms, the WM_APP_APPLY doc); `hotkeys.rs` pure `conflict_message(toggle, settings, &res) -> Option<String>` (wording unchanged); `settings_win.rs` apply() calls crate::queue_apply (the WM_APP_APPLY import goes; module doc and apply() doc updated).
- Tests first: `main::tests::pending_apply_is_handed_over_once`: after stash_apply(Settings::defaults()), take_pending_apply() is Some and a second call is None. `hotkeys::tests::conflict_message_lists_only_failed_hotkeys`: no failures -> None; toggle_failed with Ctrl+Alt+M -> text containing "Show / Hide: Ctrl+Alt+M" and not "Open settings"; a failed flag with a None spec -> None.
- Size: about 0 to +0.2 KB.
- Risks: two queued Applies: the first message takes the latest settings and the second is a no-op, as intended.

#### C2.10 Minimal application manifest (step 2)

- Findings: app-shell[7], settings-ui[18] (partly), app-shell[12] (comment part).
- Problem: build.rs embeds no RT_MANIFEST, so the exe declares no supportedOS, no DPI awareness (PMv2 comes only from the runtime call) and no execution level.
- Approach: a checked-in `tinymon.manifest` (about 20 lines, about 900 bytes) with requestedExecutionLevel asInvoker, the Windows 10/11 supportedOS GUID `{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}`, dpiAware `true/pm` and dpiAwareness `PerMonitorV2, PerMonitor`, embedded by `res.set_manifest_file("tinymon.manifest")` (a file, because winres' inline form mangles XML lines). Left out on purpose: assemblyIdentity (a malformed one is a startup-fatal side-by-side error and its version would drift from Cargo.toml), activeCodePage (no ANSI APIs; needs 1903) and the Common Controls 6.0 dependency (SP3). `dpi::set_process_dpi_aware` stays as the fallback for a build without resources; its doc says so and states the real minimum (Windows 10 1703, because the DPI functions are static imports).
- Code: `tinymon.manifest` (new, repository root); `build.rs` (rerun-if-changed, set_manifest_file, header comment); `dpi.rs` (doc, test using FindResourceW, LoadResource, LockResource and SizeofResource from the already-enabled LibraryLoader feature).
- Tests first: `dpi::tests::exe_embeds_minimal_manifest`: when RT_VERSION #1 is absent (a build without the resource compiler) the test returns early; otherwise FindResourceW(None, #1, RT_MANIFEST) succeeds on the test binary and the text contains "PerMonitorV2", the Windows 10 GUID and "asInvoker" but not "Microsoft.Windows.Common-Controls", and RT_GROUP_ICON #1 exists (guarding C3.7's caption icon).
- Size: +1,024 B (measured).
- Risks: a manifest typo makes the exe refuse to start (CreateProcess fails with error 14001); the test and a debug-copy launch catch it. The test binary also becomes PMv2-aware (harmless).

### 6.3 C3 - Settings window and hotkeys

#### C3.1 Hotkey capture fidelity (steps 23-24)

- Findings: settings-ui[1], settings-ui[14], settings-ui[3], app-shell[10]; the dead Esc fallback of settings-ui[20]; docs-spec[11] (fixed in code by part b).
- Problem: msctls_hotkey32 cannot represent MOD_WIN and apply() rebuilds both specs from the controls, so a config Win+T becomes a bare T (captured system-wide) on the next OK. HOTKEYF_EXT is dropped, so a saved Left redisplays as "Num 4". A lone modifier keydown (the Shift of Shift+Tab, the Alt of Alt+Tab) restarts the control's capture and wipes the field. A combo TinyMon has registered itself is consumed by RegisterHotKey before the field sees it, so it cannot be moved from one action to the other.
- Approach:
  - (a) EXT, step 23: pure `word_to_spec(word)` (vk = low byte, None if 0; modifiers via hotkeyf_to_mods, which ignores EXT) and `spec_to_word(spec, ext)`; pure `needs_ext(vk, vsc_ex)` lists 0x21-0x28, 0x2D, 0x2E and 0x90 explicitly (the VK-to-scan mapping returns numpad scan codes for them) and adds any E0-prefixed key from `MapVirtualKeyW(vk, MAPVK_VK_TO_VSC_EX)`, excluding E1 (Pause). read_hotkey uses word_to_spec; set_hotkey_control sends `spec_to_word(spec, needs_ext(..))`.
  - (b) Win, step 23: pure `effective_hotkey(saved, field)` returns the saved spec when it has MOD_WIN and the field equals it minus Win (the user did not touch the field), else the field. apply() uses it for both fields.
  - (c) Routing, step 24: pure `field_key_route(in_field, msg, vk) -> KeyRoute { Dialog, Field, Drop }`. While a hotkey field has focus the loop drops WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN and WM_SYSKEYUP for Shift, Ctrl, Alt (including left/right) and LWin/RWin, and drops Alt+Enter (WM_SYSKEYDOWN VK_RETURN, which also clears the field). The control reads modifiers through GetKeyState when the real key arrives, and GetMessage updates the key state even for a dropped message, so Ctrl+Alt+M and Shift+[ still capture. Tab, Enter and Esc keydowns go to IsDialogMessageW. While the settings window is open the loop runs `match field_key_route(focused_is_hotkey(), msg.message, vk) { Drop => continue, Field => {}, Dialog => if IsDialogMessageW(sh, &msg) { continue } }` before TranslateMessage and DispatchMessageW. The manual Esc branch is dead code (IsDialogMessageW already maps Esc to IDCANCEL) and is deleted with its imports.
  - (d) Owned combos, step 24, WM_HOTKEY forwarding: when RegisterHotKey consumes a combo the system posts WM_HOTKEY with `lParam = MAKELONG(mods, vk)`. The WM_HOTKEY arm copies settings_hwnd out of AppState, drops the borrow and calls `settings_win::capture_fired_hotkey(sh, lparam) -> bool`: if GetFocus() is IDC_HK_TOGGLE or IDC_HK_SETTINGS of that window, it shows `hotkey_from_wm_hotkey(lparam)` in the field (EXT derived), sends `WM_COMMAND(id, EN_CHANGE)` so the C3.8 notes refresh (HKM_SETHOTKEY sends no EN_CHANGE), and returns true; the arm then returns without toggling or opening anything. Registration never changes, so TinyMon's hotkeys are never dead. Suspending registration while a field has focus was rejected: re-registration would wait for the next loop message (up to interval_ms, 10 s), during which the owner's bare "]" and "[" would type into other apps.
  - AppState.hk_status (the last RegisterResult) is stored in WM_CREATE (still without a popup) and in apply_settings; RegisterResult gains Clone, Copy, PartialEq, Eq and Debug.
- Code: `settings_win.rs` (word_to_spec, spec_to_word, needs_ext, effective_hotkey, is_modifier_vk, KeyRoute, field_key_route, hotkey_from_wm_hotkey, capture_fired_hotkey; read_hotkey, set_hotkey_control, apply()); `main.rs` (message loop, WM_HOTKEY, hk_status; GetAncestor, GA_ROOT, VK_TAB, VK_RETURN, VK_ESCAPE imports removed); `hotkeys.rs` derives; stale comments (the loop's "a modifier is required" and Esc comments, the read_hotkey doc).
- Tests first:
  - `settings_win::tests::word_to_spec_drops_ext_and_incomplete_captures`: 0x0825 -> {0, 0x25}; 0x0E24 -> {Ctrl|Alt, 0x24}; 0x0100 (lone Shift) -> None; 0 -> None.
  - `spec_to_word_sets_ext_on_request`: ({Ctrl|Alt, 0x24}, true) -> 0x0E24; ({0, 0xDB}, false) -> 0x00DB.
  - `navigation_and_e0_keys_need_ext`: true for (0x25, 0x004B), (0x24, 0x0047), (0x21, 0x0049), (0x2D, 0x0052), (0x2E, 0x0053), (0x90, 0x0045), (0x6F, 0xE035), (0xAD, 0xE020), (0x5D, 0xE05D); false for (0x41, 0x001E), (0xDB, 0x001A), (0x13, 0xE11D) Pause, (0x70, 0x003B), (0x64, 0x004B) numpad 4.
  - `win_combo_survives_an_untouched_field`: saved Win+T with field T -> Win+T; Win+Shift+T with Shift+T -> Win+Shift+T; Win+T with Ctrl+T -> Ctrl+T; Win+T with None -> None; Ctrl+T with T -> T.
  - `modifiers_never_reach_a_focused_hotkey_field`: in a field, every key message for 0x10-0x12, 0xA0-0xA5, 0x5B and 0x5C -> Drop; WM_SYSKEYDOWN VK_RETURN -> Drop; Tab, Enter and Esc WM_KEYDOWN -> Dialog; "M" WM_KEYDOWN or WM_SYSKEYDOWN -> Field; Tab WM_KEYUP -> Field; mouse messages and WM_CHAR -> Field; not in a field -> Dialog.
  - `hotkey_from_wm_hotkey_unpacks_mods_and_vk`: MAKELONG(3, 0x87) -> Ctrl+Alt+F24; MOD_NOREPEAT never leaks into mods.
  - `hotkey_word_round_trips_through_control_format`: rewritten on the new helpers.
- Size: about +0.5 KB over three commits.
- Risks: while modifiers are held the field no longer shows a partial "Ctrl + Alt +"; it jumps to the finished combo. This relies on the control reading GetKeyState (behaviour identical on comctl32 5.82 and 6.0). A Win combo from the config still cannot be captured in the UI (C3.8 explains it). A future key that must reach the dialog has to be added to field_key_route.

#### C3.2 Conflict reporting (step 32)

- Findings: settings-ui[6].
- Problem: apply() never compares the two specs, so a self-conflict is reported as "already in use by another app"; startup results are discarded and WM_APP_APPLY re-registers only when a spec changed, so a combo that failed at startup is never retried and nothing tells the user.
- Approach:
  - apply() returns bool. After computing the effective specs it refuses equal ones (pure `same_combo(a, b) = a.is_some() && a == b`): it refreshes the notes, focuses IDC_HK_SETTINGS and returns false. IDOK destroys the window only on true, so OK stays open and Apply does nothing until the second field changes. (A hand-edited config with the same combo for both actions therefore blocks OK until fixed; the docs pass says so.)
  - apply_settings re-registers when pure `hotkeys::should_register(changed, hk_status)` is true: a spec changed or any previous registration failed (the retry). The modal warning covers only combos changed in this Apply (pure `to_warn(res, changed)`); an unchanged combo that still fails is shown only inline.
  - After its borrow ends, apply_settings posts WM_APP_HOTKEY_STATUS (WM_APP + 5) to the settings window if it exists; its arm calls refresh_hotkey_notes, which reads `crate::hotkey_status(main)` (a Copy; default when there is no state). hk_status changes only in WM_CREATE and apply_settings.
  - New notes on the C3.8 infrastructure: SameAsShowHide ("Same as Show / Hide", second field only) and InUse ("In use by another app": the saved combo failed and the field still holds it). Precedence: Same, InUse, BlocksTyping, PlusWin.
- Code: `settings_win.rs` (apply -> bool, same_combo, IDOK and IDC_APPLY arms, WM_APP_HOTKEY_STATUS arm, the two HotkeyNote variants); `main.rs` (WM_APP_HOTKEY_STATUS, hotkey_status accessor, apply_settings); `hotkeys.rs` (RegisterResult::any_failed, should_register, to_warn).
- Tests first:
  - `settings_win::tests::same_combo_for_both_actions_is_rejected`: (Some(x), Some(x)) -> true; (None, None), (Some(x), None) and (Some(x), Some(y)) -> false.
  - `hotkeys::tests::failed_startup_hotkey_is_retried_on_apply`: should_register([false, false], {toggle_failed}) -> true; ([false, false], default) -> false; ([true, false], default) -> true.
  - `warn_only_about_changed_combos`: to_warn({true, true}, [false, true]) == {false, true}; to_warn({true, false}, [true, true]) == {true, false}.
  - `register_result_any_failed`: default -> false; either flag -> true.
  - `settings_win::tests::notes_rank_conflict_then_registration_then_typing`: fields [Ctrl+Alt+M, Ctrl+Alt+M] -> [None, SameAsShowHide]; field "]" with saved "]" and failed -> InUse (it outranks BlocksTyping); field Ctrl+Alt+M with saved "]" and failed -> None (the status belongs to the old combo); both None with both failed -> [None, None].
  - `main::tests::wm_app_ids_are_distinct`: covers +1, +2, +3 and +5 here; C2.6 adds +4.
- Size: about +0.35 KB.
- Risks: OK can now be refused; the amber note and the focus jump explain why. The modal text still says "another app", which after C2.6 can only be wrong across integrity levels.

#### C3.3 Enter activates the focused push button (step 22)

- Findings: settings-ui[2]; the mnemonics sentence of settings-ui[20].
- Problem: every push button is BS_OWNERDRAW and answers WM_GETDLGCODE with plain DLGC_BUTTON, and settings_proc does not answer DM_GETDEFID, so IsDialogMessageW turns Enter on a focused Cancel, Reset, Clear, Apply or up/down button into WM_COMMAND(IDOK): the changes the user meant to discard are applied, saved and the window closes.
- Approach: settings_proc answers DM_GETDEFID with `MAKELONG(enter_target(focused id), DC_HASDEFID)` (focused id via GetFocus, IsChild, GetDlgCtrlID). Pure `is_push_button(id)` covers OK, Cancel, Apply, Reset, both Clear buttons and the up/down ranges (sized by MetricKey::ALL.len()); pure `enter_target(id)` returns a push button's own id and IDOK for anything else (checkboxes, edits, hotkey fields, trackbar). The dialog manager never sends DM_SETDEFID or BM_SETSTYLE during Tab navigation, so BS_OWNERDRAW stays intact.
- Code: `settings_win.rs` (DM_GETDEFID arm, is_push_button, enter_target); comment sweep of the module doc ("Enter = OK", and the mnemonics no label has) and of the build_controls BS_DEFPUSHBUTTON note.
- Tests first: `settings_win::tests::enter_presses_the_focused_push_button_else_ok`: identity for IDCANCEL, IDC_APPLY, IDC_RESET, both Clear ids, IDC_MOVEUP_BASE + 3, IDC_MOVEDN_BASE + (n - 1) and IDOK; IDOK for IDC_METRIC_BASE, IDC_FONT, IDC_INTERVAL, IDC_HK_TOGGLE, IDC_OPACITY, 0 and IDC_MOVEUP_BASE + n (out of range).
- Size: about +0.15 KB.
- Risks: standard Win32 behaviour; Enter on an up/down button moves the item (and focus follows it, C3.5).

#### C3.4 Interval and font inputs (step 30)

- Findings: settings-ui[7].
- Problem: read_edit_u32 keeps the old value on any parse failure (pasted text, numbers above u32::MAX), the font text is cast with `as i32` so 3000000000 wraps negative and clamps to 9, and the clamped value is never written back (the field says 100 while the widget runs at 250).
- Approach: pure `parse_clamped(text, lo, hi, fallback)`: trimmed digits only, else the fallback (the current value); saturating accumulation; then clamp. apply() uses it for the interval (MIN_INTERVAL_MS..MAX_INTERVAL_MS) and the font (MIN_FONT_PX..MAX_FONT_PX, cast after clamping) and writes both effective values back into the edits. paint_preview uses the same function with `current_settings(main).font_px` as fallback, matching apply(). EM_LIMITTEXT is 5 for the interval and 2 for the font. The labels state the ranges as string literals, `"Update interval (250\u{2013}10000 ms)"` and `"Font size (9\u{2013}16 px)"`, 172 logical px wide with SS_ENDELLIPSIS (local const 0x4000, like SS_RIGHT), so an overflow truncates instead of wrapping onto a clipped second line; the edits start at x = 196.
- Code: `settings_win.rs` (delete read_edit_u32; read_edit_text, parse_clamped, the two label consts, apply(), paint_preview, the label and edit Places, EM_LIMITTEXT).
- Tests first: `settings_win::tests::parse_clamped_saturates_and_clamps`: ("100", 250, 10000, 1000) -> 250; "99999999999" -> 10000 (it used to keep the old value); ("3000000000", 9, 16, 13) -> 16 (it used to wrap to 9); " 14 " -> 14; "", "12a" and "-5" -> the fallback. `range_labels_match_the_clamp_constants`: both literals equal the text formatted from MIN/MAX_INTERVAL_MS and MIN/MAX_FONT_PX (format! only in the test).
- Size: about +0.4 to +0.6 KB.
- Risks: the write-back moves the caret to the start and fires EN_CHANGE on the font edit, which re-renders the preview (harmless).

#### C3.5 Focus restore and reorder focus (step 25)

- Findings: settings-ui[10], settings-ui[11].
- Problem: on every activation DefWindowProc focuses the frame and WM_SETFOCUS always jumps to the first metric checkbox, so typing is lost and Space toggles a metric. The up/down buttons belong to row positions: focus stays on a button that now acts on the neighbour, so pressing it again or double-clicking swaps the pair straight back.
- Approach: `thread_local LAST_FOCUS`: on WM_ACTIVATE with WA_INACTIVE save `GetDlgCtrlID(GetFocus())` if it is a child, then call DefWindowProcW; WM_SETFOCUS focuses `focus_target(LAST_FOCUS)` (the saved id, else IDC_METRIC_BASE); WM_DESTROY clears it. The two up/down arms merge into one handler using pure `reorder_step(len, row, up, double) -> Option<(from, to)>`: swap, re-render, refresh the preview, then SetFocus on the same-direction button at row `to`, so repeated Space or Enter keeps moving the item. BN_DOUBLECLICKED continues with the item the first click already moved, so a double-click moves it two rows.
- Code: `settings_win.rs` (LAST_FOCUS, WM_ACTIVATE arm, WM_SETFOCUS, WM_DESTROY, focus_target, reorder_step, the merged WM_COMMAND arm, the WM_SETFOCUS comment).
- Tests first: `settings_win::tests::reorder_step_follows_the_item`: (10, 3, up) -> (3, 2); (10, 0, up) -> None; (10, 9, down) -> None; (10, 3, down) -> (3, 4); double-click: (10, 3, up) -> (2, 1); (10, 1, up) -> None; (10, 3, down) -> (4, 5); (10, 8, down) -> None. `focus_target_falls_back_to_first_metric`: 0 -> IDC_METRIC_BASE; IDC_FONT -> IDC_FONT.
- Size: about +0.27 KB.
- Risks: mouse users see the focus ring jump with the item. Slow repeated clicks on the same arrow still act on whatever item now sits in that row (inherent to per-row buttons; the docs pass describes it).

#### C3.6a Layout table, work-area fit and DPI changes (steps 27-28)

- Findings: settings-ui[12].
- Problem: SCALE, the fonts and every control rect are computed once and settings_proc has no WM_DPICHANGED arm, so moving the window to a monitor with another scale leaves the controls at the old size while the preview rescales; open() never checks the work area (at 1366 x 768 and 125% the client alone fills it).
- Approach:
  - Step 27: pure `layout(n)` and `control_places(n) -> Vec<Place { id, x, y, w, h }>` (logical units; the row count is passed in, callers use MetricKey::ALL.len()) become the single layout source for every child. The four static labels that share id 0 get IDC_LBL_INTERVAL, IDC_LBL_FONT, IDC_LBL_HK_TOGGLE and IDC_LBL_HK_SETTINGS (1022-1025). build_controls creates each control through `child(class, text, style, id)`, which looks up its Place; a const list of created ids keeps today's creation (tab) order. Static labels and notes become 18 px tall (16 px clips descenders at 150% and 175%). `frame_size(dpi)` via AdjustWindowRectExForDpi replaces the GetClientRect/GetWindowRect difference in open(), and open() fits the window into `placement::work_area_near(rect)` with `placement::fit_into`.
  - Step 28, a separate commit that can be dropped: WM_DPICHANGED sets SCALE from LOWORD(wParam) (0 means 96), creates the new fonts, calls `place_controls(hwnd)` (scaled SetWindowPos plus WM_SETFONT for every Place), then frees the old fonts (make_fonts is split for this); it moves to the suggested rect's left/top with frame_size(dpi) when lParam is non-null and uses SWP_NOMOVE otherwise; InvalidateRect.
- Code: `settings_win.rs` (ids 1022-1025, Place, layout(n), control_places(n), child(), place_controls, frame_size, open(), WM_DPICHANGED arm, make_fonts split; stale SCALE and UI_FONT docs and the "Grow the window so the client area" comment).
- Tests first:
  - `settings_win::tests::layout_ids_unique_nonzero_and_inside_client`: for n in [9, 10] (both feature sets in one run), distinct non-zero ids and every rect inside 0..CLIENT_W_TOTAL x 0..client_h.
  - `layout_controls_do_not_overlap`: all Place rects are pairwise disjoint (guards the C3.4 widths and C3.8 notes).
  - `card_controls_sit_inside_their_cards`: each control lies inside its card (x 10..270, below the title band, above the card bottom) for Metrics, Appearance and Hotkeys.
  - `every_created_control_has_a_place`: each id in the creation list has exactly one Place.
  - `settings_window_fits_the_left_monitor`: fit_into(-100, 900, 562, 605, (-1920,-39,0,1041)) == (-562, 436).
- Size: about +0.8 KB (step 27) and +0.6 KB (step 28).
- Risks: the DPI path cannot be exercised on the development machine (both monitors at 100%). On screens of 768 px or less at 125% or more the window is still taller than the work area: it is top-aligned and Tab, Enter and Esc still work; a real fit needs a layout change (SP3).

#### C3.6b True-size live preview (step 29)

- Findings: settings-ui[13], render-registry[8].
- Problem: paint_preview renders at the pane's size (about 188 x 528 logical) instead of win_w x window_height and caps the height, so with all metrics the last row is cut off, at font 16 the 246 px widget is squeezed into 188 px, and changing the font never changes the preview's width.
- Approach (D1): PREVIEW_W 210 -> 270 (CLIENT_W_TOTAL 502 -> 562; preview Place (302, 26, 250, client_h - 44), whose 248 px pane holds win_w at font 16 at every scale). paint_preview renders into a natural-size bitmap of `win_w(scale, font) x window_height(scale, rows, font)` with no height cap, keeping C2.1a's history slices. Pure `fit_preview(nat_w, nat_h, pane_w, pane_h) -> (x, y, w, h)` centres it 1:1 when it fits and otherwise scales it down uniformly (i64 cross-multiplication). Only when scaling, a second bitmap receives StretchBlt with SetStretchBltMode(HALFTONE) and SetBrushOrgEx(0, 0); AlphaBlend then applies the opacity 1:1 (AlphaBlend's own stretching would force COLORONCOLOR and jagged text). Without a height cap the preview never shows '+N'.
- Code: `settings_win.rs` (PREVIEW_W, the preview Place, fit_preview, paint_preview; imports StretchBlt, SetStretchBltMode, SetBrushOrgEx, HALFTONE and render::win_w); the paint_preview comment and the render.rs header note about the preview.
- Tests first: `settings_win::tests::preview_pane_fits_the_widest_widget_at_any_dpi`: for dpi 96, 120, 144, 168 and 192, `Scale(dpi).px(w) - 2 >= win_w(Scale(dpi), MAX_FONT_PX)`, where w is the preview Place's width from `control_places(n)` (PREVIEW_W - 20: 190 before, 250 after), so the test fails while PREVIEW_W is 210 (188 < 246 at 96 dpi). `fit_preview_centers_or_scales_uniformly`: (200, 221 in 248 x 528) -> (24, 153, 200, 221); (246, 624) -> (20, 0, 208, 528); (300, 100) -> (0, 223, 248, 82); (0, 10) -> (0, 0, 0, 0).
- Size: about +0.7 KB.
- Risks: the window is 60 logical px wider (562 x 574 client). Tall layouts (12 or more rows at large fonts) are shown slightly scaled, with HALFTONE softness; the second bitmap is allocated only then.

#### C3.7 Paint details: trackbar fill, dark caption, cursor and icon (step 26)

- Findings: settings-ui[15], settings-ui[16], settings-ui[17]; the caption-icon part of app-shell[8].
- Problem: TBM_SETPOS and keyboard, page or channel-click jumps invalidate only the old and new thumb rectangles, so the custom-drawn accent fill between them goes stale (Reset from 100 to 220 is the visible case). The window is created WS_VISIBLE and the dark caption attribute is set afterwards, which Windows 10 does not repaint, so the caption opens white. Both window classes have no cursor or icon, so the I-beam sticks over the card background and the preview, and the caption shows the default icon.
- Approach: InvalidateRect(tb, None, FALSE) after TBM_SETPOS in reset_to_defaults and in the WM_HSCROLL handler (required, because jumps go stale too). The settings window is created without WS_VISIBLE; open() sizes and fits it (C3.6a), calls dark::apply_window, then ShowWindow(SW_SHOW). `hCursor = LoadCursorW(None, IDC_ARROW)` for TinyMonSettings and TinyMonPreview; the settings class gets `hIcon = LoadIconW(hinstance, MAKEINTRESOURCE(1))`, the embedded multi-size icon.ico (the C2.10 test guards resource id 1). The runtime glyph and icon.ico share colours; unifying their artwork is SP3.
- Code: `settings_win.rs` (register_class, open, WM_HSCROLL, reset_to_defaults; remove the wrong "Done after sizing so the frame repaints once" comment).
- Tests first: none (Win32 only); checks in 8.3.
- Size: about +0.03 KB.
- Risks: the creation order changes (WM_CREATE still runs first; activation then goes through WM_SETFOCUS, which C3.5 handles). If the icon resource is missing, LoadIconW fails and the caption shows the default icon, as today.

#### C3.8 Inline notes for typing keys and Win combos (step 31)

- Findings: owner context (the owner binds bare "]" to Show / Hide and bare "[" to Open settings); the display side of settings-ui[1].
- Problem: RegisterHotKey consumes a bound key system-wide; a bare key that types a character (or that key with only Shift) can no longer be typed in any app, and nothing in the UI says so. A Win combo from the config shows without its Win modifier.
- Approach: pure `hotkeys::vk_types_text(vk)`: Backspace 0x08, Tab 0x09, Enter 0x0D, Space, 0-9, A-Z, numpad 0x60-0x6F, OEM 0xBA-0xC2, 0xDB-0xDF and 0xE2. Pure `blocks_typing(spec)`: no Ctrl, Alt or Win and vk_types_text. Each hotkey field gets a right-aligned STATIC note on its label row: IDC_HK_TOGGLE_NOTE (1026) at (102, k + 20, 154 x 18) and IDC_HK_SETTINGS_NOTE (1027) at (102, k + 66, 154 x 18); the two labels shrink from 160 to 78 px, so the layout does not grow. Notes are created with SS_RIGHT | SS_ENDELLIPSIS (local consts; the windows crate exposes them only under a disabled feature). Pure `hotkey_notes(fields, saved, failed) -> [Option<HotkeyNote>; 2]` is evaluated on the effective specs (C3.1); this step adds BlocksTyping ("Blocks typing this key") and PlusWin ("+ Win (set in config)"), C3.2 adds two more. refresh_hotkey_notes runs at the end of build_controls, on EN_CHANGE from either field, after both Clear buttons, in reset_to_defaults and from the C3.1 forwarder. WM_CTLCOLORSTATIC paints warnings in a new amber `dark::WARN = rgb(0xE8, 0xB0, 0x4A)` (7.5:1 on PANEL_BG) and the Win note in TEXT_MUTED; the note kind is stored in the static's GWLP_USERDATA. Bare keys stay allowed.
- Code: `hotkeys.rs` (vk_types_text, blocks_typing); `dark.rs` (WARN); `settings_win.rs` (ids 1026 and 1027, SS_RIGHT and SS_ENDELLIPSIS consts, the note Places, HotkeyNote with text() and is_warning(), hotkey_notes, refresh_hotkey_notes, EN_CHANGE handling, Clear and Reset calls, WM_CTLCOLORSTATIC).
- Tests first:
  - `hotkeys::tests::typing_keys`: true for 0x08, 0x09, 0x0D, 0x41, 0x5A, 0x30, 0x39, 0x20, 0xDB, 0xDD, 0xBA, 0xC0, 0x60, 0x6F, 0xE2; false for 0x70, 0x87 (F24), 0x25, 0x24, 0x2E, 0x2D, 0x13, 0xAD, 0xB3, 0x5D, 0x90.
  - `bare_or_shift_typing_keys_block_typing`: "]" bare -> true; Shift+"[" -> true; Ctrl+"]", Alt+T, Win+T, bare F8 and bare Left -> false.
  - `settings_win::tests::owner_brackets_and_win_get_notes`: bare 0xDD and 0xDB (saved, no failures) -> [BlocksTyping, BlocksTyping]; [Win+T] -> PlusWin; [Ctrl+Alt+M, F8] -> [None, None]; [Shift+"[", F8] -> [BlocksTyping, None].
- Size: about +1.2 KB (including the note infrastructure C3.2 builds on).
- Risks: AltGr (Ctrl+Alt) chords that type on some layouts are not flagged, nor is a typing key outside the static table on an exotic layout (both keyboard layouts on the development machine agree with the table for "[" and "]"). The note wording is easy to change. The owner's Settings window shows two permanent amber notes.

#### Add-on: parse_hotkey rejects a second key (step 3)

- Findings: settings-ui[20] (code part).
- Problem: parse_hotkey's doc says exactly one key is required, but "Ctrl+A+B" silently binds Ctrl+B.
- Approach: return None when a second key token appears.
- Code: `hotkeys.rs` parse_hotkey and its doc.
- Tests first: `hotkeys::tests::parse_rejects_two_keys`: "ctrl+a+b" and "a+b" -> None; "ctrl+alt+m" and "0xDB" unchanged.
- Size: 0 B.
- Risks: a hand-edited "Ctrl+A+B", which used to bind Ctrl+B, now leaves that action unbound.

### 6.4 C4 - Config robustness and temperature honesty

#### C4.1 Config file robustness (steps 3-5)

- Findings: settings-ui[0] (medium), settings-ui[4], settings-ui[5], settings-ui[8], render-registry[9]; the code side of docs-spec[2] and docs-spec[18].
- Problem: parse() keeps everything after "=", so the README's own example with trailing comments silently drops GPU, the position, the hotkey and the preferences, and the next persist writes the degraded settings back. load() trusts the newest of the exe-folder and %APPDATA% files, so another install's file can win, and read_to_string fails on UTF-16 or ANSI files, which then load as defaults and get overwritten. write() truncates in place, so a crash can leave an empty or torn file that wins next time. Duplicate metric tokens build duplicate samplers and make the last metric unreachable in Settings.
- Approach:
  - Step 3 (C4.1a): `strip_comment(val)` keeps the value up to the first "#", trimmed (no value TinyMon reads can contain "#": numbers, booleans, metric tokens, hotkeys as format_hotkey writes them). `Settings::from_config` keeps the first occurrence of each metric key; it is the single gate for load, Apply and Reset.
  - Step 4 (C4.1b): load reads raw bytes and `decode()` turns them into text and never fails: a BOM-led UTF-16 LE or BE file through String::from_utf16_lossy; anything else as an ASCII view with a leading UTF-8 BOM dropped and every non-ASCII byte mapped to a space, so NBSP-padded values that parse today still parse after trimming. main() resolves the write path first and calls `load(cfg_path.as_deref())`. Pure `is_portable(write_path, exe_dir)` (the write path is the exe folder's file) selects the order; pure `load_order(portable, found)` keeps the exe folder's file first for a portable install even when %APPDATA% is newer (%APPDATA% only seeds a first run, because another install may have written it), and otherwise keeps newest-first with one compare-and-swap; `load_first(paths)` parses the first file that can actually be read and falls through on a missing or unreadable one. A CFG_NAME const replaces the repeated literal; paths compare through as_os_str.
  - Step 5 (C4.1c): `write() -> io::Result<()>` stages `tinymon.cfg.tmp` opened with `OpenOptions` write, create, truncate and `share_mode(0)` (a second writer cannot interleave bytes), writes, calls sync_all, then `fs::rename` over the target. If the rename fails, the temp file is deleted and the text is written in place as today, so a holder without FILE_SHARE_DELETE does not lose the save; if that fails too, the rename's error is returned. `tmp_path()` appends ".tmp" to the OsString. persist() still ignores the result (Section 11). The comment states how std's rename really behaves: MoveFileExW(REPLACE_EXISTING) first; on ERROR_ACCESS_DENIED a POSIX-semantics FileRenameInfoEx rename (NTFS, Windows 10 1607+), which can replace a file another process holds open with FILE_SHARE_DELETE; it fails when the holder did not share delete, on FAT/exFAT with any open holder, and on a read-only target.
- Code: `config.rs` (parse, strip_comment, decode, load(write_path), is_portable, load_order, load_first, write, tmp_path, CFG_NAME, module and function docs, a `scratch(name)` test helper; file_write_read_roundtrip calls `.expect("write")`); `settings.rs` from_config; `main.rs` (resolve before load; `let _ = config::write(..)` in persist).
- Tests first:
  - `config::tests::parse_strips_inline_comments`: "x=100   # window position", "metrics=cpu,ram,gpu   # which, in order", "hotkey_toggle=Ctrl+Alt+M  # optional; e.g. Ctrl+Shift+F1" and "opacity=200#tight" -> x 100, [cpu, ram, gpu], "Ctrl+Alt+M", 200.
  - `parse_readme_example_block`: the README "Keys" block copied verbatim parses to x/y 100/24, [cpu, ram, gpu], 220/1000/13, Ctrl+Alt+M and Ctrl+Alt+S.
  - `owner_config_still_parses_identically`: the owner's 137-byte file (x=-213, y=-16, metrics=cpu,ram,gpu,temp,uptime, opacity=188, interval_ms=1000, font_px=13, hotkey_toggle=0xDD, hotkey_settings=0xDB, light=false) parses unchanged and serialize(parse(text)) == text byte for byte.
  - `settings::tests::metrics_dedup_keeps_first_occurrence`: [cpu, ram, cpu, gpu, ram] -> [Cpu, Ram, Gpu].
  - `decode_utf16le_bom`: bytes FF FE followed by UTF-16LE "opacity=200\r\nx=5\r\n" give opacity 200 and x 5 through parse(&decode(..)) (today load() returns defaults for this file). `decode_utf16be_bom`: FE FF followed by UTF-16BE "opacity=201" gives 201. `decode_utf8_with_bom_still_parses`: EF BB BF followed by "opacity=199" gives 199.
  - `decode_invalid_utf8_is_lossy_not_fatal` (bytes 0xE9 0xE1 0xE2 in a comment line between metrics=cpu and opacity=150); `decode_keeps_unicode_whitespace_tolerance` (`metrics=cpu,\xC2\xA0ram`, `x=1\xC2\xA0`, `y=2` -> [cpu, ram], x 1, y 2).
  - `is_portable_matches_only_the_exe_folder_file`: (d/tinymon.cfg, d) -> true; (appdata/tinymon.cfg, d) -> false; (None, d) -> false.
  - `load_order_portable_prefers_own_file_even_if_older`: load_order(true, [(exe, t0), (appdata, t0 + 1 day)]) == [exe, appdata] (today's newest-wins rule picks appdata).
  - `load_order_appdata_mode_prefers_newer`: load_order(false, ..) gives [exe, appdata] when the exe-folder file is newer and [appdata, exe] when the %APPDATA% file is newer.
  - `load_first_skips_missing_and_uses_next`: load_first([missing, a file holding "opacity=77"]) gives opacity Some(77); load_first([missing]) == Config::default().
  - `write_replaces_file_instead_of_truncating_in_place` (a reader opened before write(x=2) still reads the old "x=1", the path now parses to x 2, no .tmp remains); `write_leaves_read_only_target_untouched` (Err, content still "x=1", temp removed); `write_refuses_a_temp_another_writer_holds` (a held tinymon.cfg.tmp -> Err, file still "x=1"); `write_falls_back_in_place_when_rename_is_denied` (a holder opened without FILE_SHARE_DELETE -> Ok, content x=2, no .tmp).
- Size: step 3 0 B; step 4 about +1 KB; step 5 about +1 to +1.5 KB.
- Risks: the rename replaces a symlinked or hard-linked cfg with a regular file and resets its attributes (hidden and system bits are dropped) and any explicit ACL to the folder's inherited one. Each save costs one flush (about 10 ms measured with real-time antivirus scanning). A crash between create and rename can leave a stray tinymon.cfg.tmp that is never read. "#" can never be part of a value (a future colour key would need another rule). A portable install no longer adopts a newer %APPDATA% file; the flip-back case after a read-only period is accepted. Without C2.6, two instances could still lose each other's updates, but never interleave bytes.

#### C4.2 Temperature honesty: live ACPI zones only (step 33)

- Findings: samplers[8] (raised to medium); docs-spec[7] and docs-spec[21] (comments).
- Problem: TempSource treats any plausible PDH "Thermal Zone Information" reading as a CPU temperature and calls a lone zone "CPU Temp". On many desktops, the development machine included, the only zone is a firmware stub reporting a constant 301 K at any load (verified across 4-80% CPU while Windows kept re-reading the zone), so the widget shows "CPU Temp 28 C" forever.
- Approach (owner decision): show a zone only after its reading has moved.
  - A `Zone { name, first, now, live }` is tracked per PDH instance across ticks; `observe_zones` records the first plausible reading (1-150 C, shared `plausible()`) and sets live once a reading differs by at least 0.5 K (the counter reports whole kelvin); live never resets. Proven zone names are remembered for the process lifetime in the UI-thread `thread_local PROVEN_ZONES`, so the source rebuild on a metric Apply does not hide a real sensor again.
  - Labels say what the reading is: "ACPI Temp" when the board has one zone, otherwise `ACPI <NAME>` from the 4-character ACPI name (`_tz.thrm` -> "ACPI THRM", taken with str::get so an odd name cannot panic); a zone is never labelled "CPU Temp".
  - `build_rows(lhm, zones, out)` is the single assembly point: LHM rows if there are any, else the live zones, else `Row::na("Temp")`, keeping today's policy that an enabled metric stays visible. A future discrete-GPU temperature source (SP3, D3DKMT) plugs in here, for example as GPU rows when LHM gave none.
  - Prep commit, first in this step: extract today's rows() fallback into build_rows unchanged, with a characterization test, and wire an identity `pick_lhm` into the LHM path, so the C4.2 and C4.4 tests fail on behaviour rather than on compilation.
  - Also: rewrite the Cargo.toml temp-feature comment in plain ASCII ("temp - the Temperature metric (LibreHardwareMonitor <= 0.9.4 WMI sensors, else a live ACPI thermal zone via PDH, else n/a). On by default; --no-default-features removes the metric and all COM/WMI code."), checking with git diff that only that comment changed; update the fps.rs comment ("feature-gated like the temp metric, which is on by default"); `registry::sample_rows`' canned temperature row becomes `temp_row("Temp", 55.0)`.
- Code: `temp.rs` (Zone, observe_zones, live_zone_rows, zone_label, build_rows, plausible, PROVEN_ZONES; TempSource.native becomes zones; module and struct docs rewritten); `Cargo.toml`; `fps.rs`; `registry.rs`.
- Tests first:
  - `temp::tests::build_rows_characterizes_todays_fallback` (prep commit; replaced by the tests below): one plausible zone -> "CPU Temp"; nothing -> ("Temp", "n/a").
  - `constant_stub_zone_is_never_shown`: 600 ticks of (_tz.tz00, 301 K) -> no zone rows; build_rows yields exactly ("Temp", "n/a").
  - `zone_is_shown_once_it_moves_and_stays_shown`: 318 K, 318 K -> nothing; after 319 K and then 318 K -> one "ACPI Temp" row with the latest value (44.85 C).
  - `several_zones_get_stable_firmware_names`: tz00 constant at 301 K and thrm moving 320 -> 322 K -> exactly one row, "ACPI THRM".
  - `implausible_zone_readings_are_ignored`: 0 K and 1000 K neither create a zone nor count as movement.
  - `proven_zone_is_shown_at_once_after_a_rebuild`: with proven = ["_tz.thrm"], a fresh zone list shows thrm on its first reading.
  - `lhm_rows_take_precedence_over_zones`: build_rows([("CPU Temp", 55)], live zones) emits only the LHM row.
- Size: about +1.5 KB.
- Risks: a real sensor that has not moved a whole kelvin since start (idle machine, coarse EC steps) reads "Temp n/a" until it first moves; a zone Windows never re-polls stays hidden; a stub that alternates between two constants or changes once on resume counts as live; the proof restarts at every launch. Labels change from "CPU Temp" / "Temp N" to "ACPI Temp" / `ACPI <NAME>`. Multi-zone laptops gain rows at runtime, which C2.1a and C2.1b (landed earlier) handle. The owner's widget changes from "CPU Temp 28 C" to "Temp n/a" because their config lists temp.

#### C4.3 Temperature is opt-in (step 34)

- Findings: samplers[8] (default part); makes render-registry[10] and docs-spec[1]'s README line true again.
- Problem: DEFAULT includes Temp on the assumption that the ACPI fallback is a CPU temperature; after C4.2 most desktops would show a permanent "Temp n/a" on a fresh install.
- Approach: `MetricKey::DEFAULT = [Cpu, Ram, Gpu]` as one const in both feature sets. Configs that list temp keep it; only configs without a `metrics=` key and "Reset to defaults" change.
- Code: `registry.rs` (single DEFAULT const and its doc); `settings.rs` tests.
- Tests first: `settings::tests::defaults_reproduce_cpu_ram_gpu` (rewritten: [Cpu, Ram, Gpu] in both builds; fails today in the default build) and `explicit_temp_in_config_is_kept` (temp builds: [cpu, temp] -> [Cpu, Temp]).
- Size: 0 B.
- Risks: "Reset to defaults" now unticks Temperature; a hand-written config without `metrics=` loses its Temp row.

#### C4.4 Legacy LHM reader: bounded, reconnecting, CPU and GPU only (step 35)

- Findings: samplers[2], samplers[3], samplers[4]; docs-spec[8].
- Problem: the legacy reader (WMI namespace ROOT\LibreHardwareMonitor, published only by LHM 0.9.4 and older) calls `IEnumWbemClassObject::Next(WBEM_INFINITE)` on the UI thread, connects with no connect flag and only once per source, and turns every temperature sensor into a row (about 15 on a 6-core Intel CPU, including "Distance to TjMax" headroom drawn as a temperature).
- Approach: keep the reader, but make it unable to freeze the UI and keep it to a few rows.
  - Bounded read: `query_temps(svc, deadline, out) -> Option<()>`; ExecQuery stays semi-synchronous; each Next fetches a batch of 16 objects with the milliseconds left until the deadline (LHM_BUDGET_MS = 500). Pure `next_step(hr)`: WBEM_S_NO_ERROR -> More, WBEM_S_FALSE -> Done, WBEM_S_TIMEDOUT or any error -> Fail. Pure `ms_left(deadline, now)` is None once expired, never 0. The deadline starts after the connect step (GetTickCount64 read again), so a slow reconnect does not use up the query budget.
  - Reconnect with back-off: TempSource starts unconnected; sample() connects when the pure `Retry` clock is due; a failed connect or a failed or timed-out query drops the IWbemServices and backs off LHM_RETRY_MS = 30 s. ConnectServer gets WBEM_FLAG_CONNECT_USE_MAX_WAIT (bounded at 2 minutes). CoInitializeEx(COINIT_MULTITHREADED) runs once in new(); if it returns RPC_E_CHANGED_MODE (the thread is already STA, where a blocking call would pump messages and re-enter wnd_proc), WMI is never used (`Retry::never()`).
  - Filtering: private `pick_lhm` maps (name, C) pairs through a 5-entry table: Intel "CPU Package", AMD Zen 2+ "Core (Tctl/Tdie)", Zen 1 "Core (Tdie)" (its Tctl carries an offset) and AMD family 10h-16h "CPU Cores" become "CPU Temp"; "GPU Core" becomes "GPU Temp"; values must pass plausible(). If nothing matches, the row falls back to the ACPI zones or "Temp n/a". The names come from the LHM 0.9.4 sources. pick_lhm needs only (name, value) pairs, so it does not tie the design to WMI.
- Code: `temp.rs` (LHM_BUDGET_MS, LHM_RETRY_MS, LHM_BATCH; fields lhm, lhm_retry, lhm_rows; sample_lhm; connect with MAX_WAIT; query_temps; Retry, Step, next_step, ms_left, LHM_SHOWN, pick_lhm; imports GetTickCount64, HRESULT and the WBEM codes; WBEM_INFINITE removed). No new features.
- Tests first:
  - `temp::tests::next_step_classifies_wbem_codes`: 0 -> More; 1 (WBEM_S_FALSE) -> Done; 0x40004 (WBEM_S_TIMEDOUT) -> Fail; 0x80041003 -> Fail.
  - `ms_left_counts_down_and_expires`: (1500, 1000) -> Some(500); (1000, 1000) -> None; (1000, 2000) -> None.
  - `retry_is_due_at_start_then_backs_off`: the default is due at 0; after fail(1000) it is not due at 30,999 and due at 31,000; Retry::never() is never due.
  - `lhm_intel_cpu_becomes_one_package_row`: an LHM 0.9.4 Intel set (six core sensors, six "Distance to TjMax", Core Max, Core Average, CPU Package 48, an NVMe "Temperature", Super I/O "System" and "CPU") -> exactly [("CPU Temp", 48)]; with the prep commit's identity pick_lhm it first fails with 18 rows (one per listed sensor).
  - `lhm_amd_uses_the_die_temperature`: Zen 2 [CCD1 (Tdie) 61, Core (Tctl/Tdie) 63] -> [("CPU Temp", 63)]; Zen 1 [Core (Tctl) 70, Core (Tdie) 50] -> [("CPU Temp", 50)].
  - `lhm_gpu_core_only`: [GPU Hot Spot 71, GPU Core 60, GPU Memory Junction 80] -> [("GPU Temp", 60)].
  - `lhm_drops_tjmax_distance_and_implausible_values`: [CPU Core #1 Distance to TjMax 60, CPU Package NaN, GPU Core 0] -> empty.
- Size: about +1 KB.
- Risks: a healthy provider slower than 500 ms never shows rows (an honest fallback). While connected, every tick still makes one cross-process WMI call on the UI thread; a stale namespace left by an old LHM install costs about 1 ms per tick. If winmgmt itself hangs, ExecQuery is unbounded and ConnectServer bounded only at 2 minutes; the first connect still happens on the UI thread during the startup priming sample (deferred, Section 11). The whitelist misses unusual CPUs (they fall back). Two GPUs give two rows both labelled "GPU Temp" (kept apart by RowId nth). Rows now change at runtime ("Temp n/a" -> "CPU Temp" + "GPU Temp"), which the runtime resize handles.

## 7. Implementation order

Each step is one or more small commits on `sp1-correctness`; every commit passes the gates of 8.1. Size and Total are in KiB (1,024 B): estimates of code-and-data growth (measured where noted); the file size moves in 512-byte steps, so one commit can measure 512 B above or below its estimate. The running total is what is checked against +22 KB (263,168 B).

| Step | Items | Depends on / notes | Size (KiB) | Total (KiB) |
|---|---|---|---|---|
| 0 | SP0: 0a finish emphasize() with tests; 0b .gitattributes; 0c rustfmt.toml + one formatting-only commit; 0d six clippy fixes + #[ignore] on the two GPU dump tests | Create the branch first; four commits | 0 | 0 |
| 1 | C1.7 luid_key(high, low) + test | No dependencies; C1.1/C1.2 tests use luid_key (the #[ignore] speed-up already landed in 0d) | 0 | 0 |
| 2 | C2.10 manifest + resource test (also asserts RT_GROUP_ICON #1) | Build-only; first so every later runtime check runs under the final manifest | +1.0 (measured) | 1.0 |
| 3 | C4.1a trailing comments + metric dedup; add-on: parse_hotkey rejects a second key | Two pure commits | 0 | 1.0 |
| 4 | C4.1b decode, load(write_path), is_portable, load_order, load_first; main() resolves the write path before loading | 3 | +1.0 | 2.0 |
| 5 | C4.1c atomic write (share_mode(0) temp, sync_all, rename, in-place fallback), io::Result ignored by persist, rename comment | 4 | +1.5 | 3.5 |
| 6 | C1.3 short_name falls back to "Radeon" | Independent | +0.5 (measured) | 4.0 |
| 7 | C2.3 Row.alert, Row::rate, is_critical, ALERT_AT | Before 8 (Row::na born with alert false) and 13 (draw_row rework) | 0 | 4.0 |
| 8 | C1.4 try_sample, Row::na, n/a rows for disk, net, per-core and GPU, load_rows/vram_rows, stable next_cores, timing-independent PDH test | 7; foundation for 9, 10, 11 and C4.2's n/a row | +0.5 (measured) | 4.5 |
| 9 | C1.2 busiest_engine, adapter_load (strict LUID, lone-adapter fallback), empty sample shows n/a | 1, 8 | +0.5 (measured) | 5.0 |
| 10 | C1.1 VRAM from GPU Adapter Memory, UMA rule (both budgets), totals by kind, D2 labels | 1, 8 | +0.5 (measured) | 5.5 |
| 11 | C1.6 Net from GetIfTable2 (hardware, then connector-present, then n/a), per-LUID deltas, QPC timing; IpHelper and Ndis features | 8; reverts on its own | +0.5 (measured) | 6.0 |
| 12 | C2.9 PENDING_APPLY, queue_apply (takes the slot back on failure), apply_settings, show_warning, conflict_message, guarded WM_APP_SETTINGS_CLOSED | Before every later Apply-path edit (14, 16-19, 24, 32); introduces the borrow rule | +0.1 | 6.1 |
| 13 | C2.1a history.rs, collect_rows, render_rows(&[&[f32]]), bar_style, inset spark_line/spark_area (n == 1 guarded), preview line, no history.clear() | 7, 12; prerequisite for 14, 18, 29, 33, 35 | +1.0 | 7.1 |
| 14 | C1.5 is_rate (exhaustive), PENDING, mask_pending, collect_rows pending flag, first_tick_pending, TIMER_ID re-arm after a metric Apply, first_tick_ms | 12, 13 | +0.5 | 7.6 |
| 15 | C2.5a placement.rs: fit_into, anchored (reports clamping), corner_xy, place, after_relayout, drag_end, work-area shims; owner-spot tests | Pure, tested before wiring; C3.6a reuses it | +0.4 | 8.0 |
| 16 | C2.5b home semantics: provisional rect, place_widget on WM_CREATE, WM_DISPLAYCHANGE and SPI_SETWORKAREA, first-run default on the primary work area, persist without GetWindowRect, drag end adopts only a real move, snap sets home | 12, 15; verify with debug copies at x=-213 and x=-5000 | +0.6 | 8.6 |
| 17 | C2.5c relayout (anchored and fitted, clamped never adopted) replaces apply_window_size and the inline WM_DPICHANGED sizing (lParam null-checked); apply_settings resizes, then persists last | 16 | +0.4 | 9.0 |
| 18 | C2.1b layout_dirty, sync_layout from WM_TIMER and apply_settings; sized_rows; WM_TIMER persists only when home changed | 13, 17 | +0.15 | 9.2 |
| 19 | C2.2 rows_fitting, widget_size, '+N'; work-area height passed by place_widget and relayout; value column over shown rows | 13, 17 | +0.4 | 9.6 |
| 20 | C2.8 show_no_activate for the toggle and the startup show | Reused by 36; focus check waits for the live session | 0 | 9.6 |
| 21 | C2.4 TaskbarCreated (UIPI filter), NIM_ADD/NIM_MODIFY, bounded retry with reset count, WM_TIMER dispatch on wParam, tray_left_click, tray-DPI icon rebuilt on TaskbarCreated, no class icon, icon.rs header | 14 (WM_TIMER); makes add_tray_icon idempotent for 36 | +0.75 | 10.3 |
| 22 | C3.3 DM_GETDEFID with the focused push button, else IDOK; Enter and mnemonics comment sweep | Later manual checks rely on the new Enter behaviour | +0.15 | 10.5 |
| 23 | C3.1a word_to_spec, spec_to_word, needs_ext (0x90 tested); C3.1b effective_hotkey | Two pure commits | +0.3 | 10.8 |
| 24 | C3.1c field_key_route (drops lone modifiers and Alt+Enter), dead Esc branch removed, WM_HOTKEY forwarding into a focused hotkey field, hk_status in WM_CREATE, RegisterResult derives | 12, 23; isolated because it changes the message loop | +0.2 | 11.0 |
| 25 | C3.5 LAST_FOCUS restore, reorder_step, focus follows the item, double-click moves two rows | Independent | +0.3 | 11.3 |
| 26 | C3.7 hidden create then size, dark, show; trackbar invalidation in WM_HSCROLL and Reset; arrow cursor on both classes; icon resource 1 on the settings class | Restructures open() for 27 | +0.03 | 11.3 |
| 27 | C3.6a-1 layout(n)/control_places(n), label ids 1022-1025, 18 px statics, frame_size, fit with placement::fit_into | 15, 26; single layout source for 29-31 | +0.8 | 12.1 |
| 28 | C3.6a-2 settings WM_DPICHANGED relayout (lParam null-checked; new fonts before old ones are freed) | 27; separate so it can be dropped; manual-only verification | +0.6 | 12.7 |
| 29 | C3.6b true-size preview at PREVIEW_W 270 (D1), HALFTONE scaling, history slices kept, no '+N' | 13, 27 | +0.7 | 13.4 |
| 30 | C3.4 parse_clamped, write-back, EM_LIMITTEXT, literal range labels 172 px with SS_ENDELLIPSIS, preview falls back to the current font | 27, 29 | +0.5 | 13.9 |
| 31 | C3.8 vk_types_text (with Backspace, Tab, Enter), blocks_typing, notes 1026/1027 (SS_RIGHT, SS_ENDELLIPSIS), dark::WARN | 23, 27 | +1.2 | 15.1 |
| 32 | C3.2 same_combo guard (apply returns bool), should_register, to_warn, any_failed, WM_APP_HOTKEY_STATUS = WM_APP + 5 posted after the borrow, Same/InUse notes, WM_APP id test | 12, 24, 31 | +0.35 | 15.5 |
| 33 | C4.2 prep (build_rows extracted unchanged + characterization test, identity pick_lhm); C4.2 zone liveness, PROVEN_ZONES, ACPI labels, Row::na("Temp"), ASCII Cargo.toml comment, fps.rs comment, sample_rows "Temp" | 13, 18 (zones can appear at runtime); re-run the C2.1b check afterwards | +1.5 | 17.0 |
| 34 | C4.3 MetricKey::DEFAULT = [Cpu, Ram, Gpu] | 33 | 0 | 17.0 |
| 35 | C4.4 bounded Next with the deadline after connect, 30 s back-off, MAX_WAIT, CoInitializeEx once (skip WMI on RPC_E_CHANGED_MODE), private pick_lhm whitelist | 33 | +1.0 | 18.0 |
| 36 | C2.6 named mutex Local\TinyMon.Instance, instance_action, WM_APP_SURFACE = WM_APP + 4 calls surface(), Win32_Security, guard skipped in debug builds | 16, 20, 21; late so no earlier harness can surface the owner's widget | +0.4 | 18.4 |
| 37 | C2.7 Run value compared with the current exe (written quoted, old unquoted form accepted), StartupApproved "disabled" cleared on enable | Last because its check edits the real Run and StartupApproved values | +0.5 | 18.9 |
| 38 | Docs pass (README and comment-only commits; no code changes), then the live session (8.4) as the last act of SP1: C2.6, C2.7 and C2.8 checks, rebuild target/release, restart through explorer.exe | Everything; behaviour is final after step 37, so the docs describe it. If a live-session check fails, the fix commit lands (test-first, all gates) and the live session is repeated | 0 | about 19 |

C1's steps add up to 3.0 KiB, but the design review measured C1 with its amendments at +3,584 B; C3's per-step estimates add up to about 5.1 KiB against +4,608 B measured for its full set; merging collect_rows into one flatten and sharing placement::fit_into save about 0.5 KiB. The cluster measurements put the final total at about +18.4 KiB (about 259,500 B), inside the table's "about 19".

The docs pass (step 38, before the live session) covers the README and the remaining comment drift: the metrics table (VRAM and GPU Mem semantics, Battery, Disk read/write, Net up/down from physical NICs), the default metrics and the valid tokens (vram, battery, temp in the default build), the config section (trailing comments allowed, comments not kept on save, the load rule, accepted encodings, atomic save, the light key, x/y as home), the Settings section (single reorderable list, Enter presses the focused button, OK refused while both hotkeys share a combo, the amber notes, Win combos config-only, bare keys captured system-wide, a focused field captures TinyMon's own combos, per-row arrows), the temperature section (ACPI liveness and labels, "Temp n/a", LHM 0.9.4 or older only and picked up within 30 s, the "Remote Web Server" step removed (docs-spec[19]), the stale size note), requirements (Windows 10 1703; VRAM needs 1709), the menu items, the widget visuals (sparklines, red label for load rows only, '+N'), single instance, sizes, the project structure and the test description; plus the remaining SAFETY-comment gaps (docs-spec[26]).

## 8. Verification strategy

### 8.1 Gates on every commit

- `cargo fmt --check`.
- `cargo clippy --all-targets` with zero warnings, default and `--no-default-features`.
- `cargo test --locked`, default and `--no-default-features`. The commit's new test is run before the change (it fails for the predicted reason) and after it.
- `cargo build --release --locked --target-dir <scratch>`; record the exe size against the running total in Section 7; stop and re-plan above +22 KB (263,168 B).
- `git diff` review: only the intended lines change; files stay UTF-8 without BOM with LF endings.

### 8.2 Unit tests

The risky logic is moved into pure, tested functions: placement and home rules, row identity and history, sparkline geometry, row builders and alerts, VRAM, GPU load and network math, PDH failure mapping, config decoding, load order and atomic writes, temperature liveness and LHM filtering, hotkey routing, capture and notes, settings layout and preview fitting. Expected totals: about 190 tests in the default build (2 ignored diagnostics, run with `cargo test -- --ignored --nocapture dump_`) and about 176 with `--no-default-features`. Filesystem tests use fresh folders under %TEMP%.

### 8.3 Launched test copies (debug builds only)

Rules:

- Every launched copy is a debug build (`target/debug/tinymon.exe`) copied into its own scratch folder with its own `tinymon.cfg` (written as UTF-8 without BOM), with `$env:APPDATA` pointed at a scratch folder before Start-Process; it runs in portable mode and never reads or writes the owner's config. Debug builds skip the single-instance guard, so they run next to the owner's instance without surfacing it.
- Release builds are only measured, never launched (outside the live session): after C2.6 a release copy would find the owner's widget, surface it and exit, measuring nothing; and the owner's autostarted exe locks target/release (os error 5, which an antivirus false positive on a fresh build can also cause), hence the scratch `--target-dir`.
- Test hotkeys never use the owner's bindings; most checks use F24-based combos, which no keyboard produces, or keys absent on US keyboards. Outside the live session, harnesses never send ID_STARTUP (2), which would rewrite the owner's Run value.
- Windows are found with EnumWindows, GetWindowThreadProcessId and the class name. PowerShell P/Invoke passes `[NullString]::Value` for a null string (`$null` becomes an empty string and FindWindowW then matches nothing).
- Pixels are read with PrintWindow(PW_RENDERFULLCONTENT), which returns the layered widget's pre-alpha pixels. Settings controls are driven by posted messages; focus is moved with AttachThreadInput and SetFocus and read with GetGUIThreadInfo.
- A copy is ended with Kill on its own PID (WM_CLOSE only hides it); a ghost tray icon may remain until hovered. A panic in wnd_proc aborts a debug copy, which the harness notices.
- Copies started from an elevated shell run elevated; paths that depend on UIPI (TaskbarCreated from Explorer) are only realistic for a copy started through explorer.exe.

Per-item checks. Rows marked (guard) are regression guards expected to pass before and after; every other row fails on the code before its step.

| Item | Setup | Expected |
|---|---|---|
| C1.1 | vram enabled | Row "GPU Mem 0.7/8G" (or similar) close to Task Manager's "GPU memory"; the old build shows 0/128M. The ignored dump test prints uma and usage |
| C1.2 | gpu enabled, video playing | The GPU row tracks Task Manager and a PowerShell busiest-engine grouping of `\GPU Engine(*)` instead of reading low |
| C1.5 | metrics=cpu,percpu,gpu,vram,disk,net,ram; interval_ms=3000 | Capture at about 0.5 s: rate rows show the marker on empty tracks, RAM and GPU Mem real; at about 1.6 s numbers everywhere (first tick capped at 1 s) |
| C1.6 (guard) | net enabled, during a download | Net down equals the physical NIC's rate; `Get-NetAdapter -IncludeHidden \| ? HardwareInterface` lists what is counted |
| C2.1a | metrics=cpu,ram,gpu,uptime (no temp), interval_ms=1000; Settings, tick Disk, Apply | After 2+ ticks every pixel of the row y = track.bottom (the first row below each bar track, e.g. y = 64 for the CPU track (8,56)-(192,64) at 96 dpi), x 8..=191, is the background #202020 (old: the idle line lands there); polling PrintWindow every 100 ms for 2.5 s after Apply, the CPU track is never entirely #404040 (old: the empty-track frame after the wipe); height = 36 + 36 x rows |
| C2.1b (guard) | the same Apply | Height follows the row count and stays on-screen; repeated after step 33 |
| C2.4 | tray | Shell_NotifyIconGetRect succeeds; after a scripted NIM_DELETE and a posted TaskbarCreated the icon is back within 300 ms (old: gone). WM_TRAY sequences 0x201, 0x202, 0x203, 0x202 flip visibility once and 0x201, 0x202, 0x203, 0x201, 0x202 twice. A debug assertion confirms ChangeWindowMessageFilterEx succeeded (the copy stays alive) |
| C2.5 (guard) | cfg x=-213, y=-16 | Rect (-213,-16,-13,200) on the development machine's left monitor |
| C2.5 | cfg x=-5000, y=-5000 | Rect fully inside a work area; after Exit (WM_COMMAND 3) the cfg still says -5000/-5000 |
| C2.5 | Move to corner bottom-right (WM_COMMAND 9), then the Disk Apply | bottom = rcWork.bottom - 24 and right = rcWork.right - 24 (old: grows past the bottom); a posted WM_DISPLAYCHANGE with a valid home leaves the rect unchanged |
| C2.8 (guard) | posted WM_HOTKEY(HK_TOGGLE) twice | Hide and show work and the widget stays topmost (the focus behaviour itself is the owner's check) |
| C2.9 | post WM_APP_APPLY with wParam 0xDEADBEEF | Process alive after 500 ms (old: access violation) |
| C2.9 | script registers Ctrl+Alt+F11; set it in the Show / Hide field; Apply; while the "hotkey in use" box is up, post WM_COMMAND 3 | Exit code 0 within 2 s; the cfg holds hotkey_toggle=Ctrl+Alt+F11 |
| C2.10 (start and pixel parts: guard) | scratch release build (not run) | RT_MANIFEST read through LoadLibraryEx(LOAD_LIBRARY_AS_DATAFILE); a debug copy starts; the settings window renders pixel-identical before and after (comctl32 stays 5.82) |
| C3.1 | cfg x=40, y=40; Settings via WM_COMMAND 4 | Win+F24 survives Apply; Ctrl+Alt+0x25 reads back 0x0E25 (old 0x0625); posted Shift down/up to a field holding 0x0687 leaves 0x0687 (old 0); with field 1014 focused, a posted WM_HOTKEY(1, MAKELONG(3, 0x87)) puts 0x0687 into the field and leaves the widget's visibility unchanged; Esc on the font edit still closes Settings |
| C3.2 | script holds Ctrl+Alt+F24; cfg hotkey_toggle=Ctrl+Alt+F24 | Note 1026 "In use by another app"; after the script releases it and Apply, the note is empty and the script's RegisterHotKey now fails with 1409; both fields set to 0x0687 plus IDOK: the window stays, note 1027 "Same as Show / Hide", cfg unchanged |
| C3.3 | TBM_SETPOS 100, then Enter on the focused Reset button | Window still open and TBM_GETPOS = 220 (old: closes and saves) |
| C3.4 | WM_SETTEXT "100" and "3000000000", Apply | Fields read 250 and 16; cfg interval_ms=250, font_px=16 |
| C3.5 | focus the font edit, WM_ACTIVATE inactive then active; BM_CLICK on 1205, then a double-click on 1205 | Focus back on 1012 (old: 1100); the item moves to row 4 with focus on 1204, then to row 3 with focus on 1203 |
| C3.6b | metrics=cpu,ram,gpu,temp,uptime, font 13; then all 10 metrics at font 16 | Preview pixel (10, 183) is #202020 (old: header separator from x=8); 14 bar bands down a mid-width column (old: 12) |
| C3.7 | cfg opacity=100, then Reset | The channel at 55% is ACCENT (old: TRACK); caption luminance below 0x80 300 ms after opening (with title-bar accent colour off); class cursor and icon non-zero |
| C3.8 | cfg hotkey_settings=0xDF; then hotkey_toggle=Win+F24 | Note 1027 "Blocks typing this key" with USERDATA 1; note 1026 "+ Win (set in config)"; Clear (1016) empties note 1027 |
| C4.1 | README "Keys" block as cfg | Widget at 100/24; the script's Ctrl+Alt+M registration fails with 1409 |
| C4.1 | PowerShell 5.1 Out-File "opacity=120" (UTF-16) | Layered alpha 120 (old: 220, then overwritten with defaults) |
| C4.1 | reader holding the cfg with ReadWrite+Delete sharing, then WM_COMMAND 10 (theme) | File has light=true; the held handle reads the old bytes; no .tmp. With ReadWrite sharing only, the save lands in place |
| C4.1 | older exe-folder cfg opacity=150, newer redirected %APPDATA% cfg opacity=90 | Alpha 150; after Exit the %APPDATA% file is unchanged |
| C4.2 | metrics=cpu,temp | "Temp n/a" (old: "CPU Temp 28 C"); height 108 px at 100% |
| C4.3 | no cfg | Height 144 px (3 rows; old: 180 px) |

### 8.4 Live session (D3)

The last act of SP1, after the docs pass (step 38), at a time the owner chooses:

1. Preconditions: every commit is in (code and docs), all gates are green, the final release size is measured, and the owner has read Section 9.
2. Record the two TinyMon registry values and nothing else: `reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v TinyMon` (REG_SZ) and `reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" /v TinyMon` (REG_BINARY), noting each value's type and data, or that it is absent. The keys are never exported or imported, so other apps' entries under Run and StartupApproved\Run are never reset to a snapshot.
3. Exit the owner's running TinyMon (WM_COMMAND 3 to its window, the same as tray Exit); it saves its own config as usual.
4. Back up the repository's `target/release/tinymon.cfg` (the owner's portable config next to the autostarted exe, and its only copy) and `target/release/tinymon.exe` into a scratch folder. target/release is never cleaned or deleted.
5. C2.6: one copy of the final scratch-target release build in its own folder with its own cfg. Launch it (A) and hide it with WM_CLOSE. Launch the same exe again (B): within 2 s B exits with code 0, one tinymon process remains, A is visible again and the foreground window is unchanged. Kill A, then Start-Process the exe twice back-to-back: one process remains after 2 s. With that release instance still running, start a debug copy from its own folder: both run. Kill all copies.
6. C2.7: the same release copy, running from its own folder. Set Run\TinyMon to `C:\nowhere\tinymon.exe` and post WM_COMMAND 2: the Run value becomes the copy's path in quotes (old: deleted). Set StartupApproved\Run\TinyMon to 03 00 00 00 ... and post WM_COMMAND 2 again: the StartupApproved value is gone and Run is rewritten quoted. Read the checkmark by posting WM_TRAY with WM_RBUTTONUP, sending MN_GETHMENU to the #32768 menu window and testing `GetMenuState(hmenu, 2, MF_BYCOMMAND) & MF_CHECKED`; then kill the copy.
7. Restore both values: `reg add "<key>" /v TinyMon /t REG_SZ /d "<recorded data>" /f` for Run and `/t REG_BINARY /d <recorded hex>` for StartupApproved\Run, or `reg delete "<key>" /v TinyMon /f` for a value that was absent; query both again and compare them with the record.
8. Rebuild target/release in the repository (`cargo build --release --locked`).
9. Start the new build with `explorer.exe "<repository>\target\release\tinymon.exe"`, so it runs at medium integrity instead of inheriting an elevated shell. Confirm one tinymon process at medium integrity, at its home on the left monitor, and that `target/release/tinymon.cfg` still equals the backup byte for byte.
10. The owner's check, about one minute and by hand (no synthetic input is injected at any point): with an editor focused, press the toggle hotkey twice; the widget hides and shows and the editor keeps the keyboard focus. Drag the widget and release it: it moves and stays. Right-click the widget and the tray icon: both menus open, and Start with Windows is checked (the restored unquoted value names this exe).

Failure path: if a check fails or the new build misbehaves (including an antivirus quarantine of the fresh exe, which surfaces as os error 5), the two registry values are still restored (item 7) and the owner's TinyMon is restarted through explorer.exe, from the backed-up exe and cfg if the new build is at fault. Only then is the bug fixed in a new test-first commit that passes every gate, and the session is repeated from item 1.

End state: SP1 ends with the owner's new build running from target/release and `sp1-correctness` as a local branch, neither merged into `main` nor pushed; merging is the owner's decision after review.

### 8.5 Manual-only checks

These ship on unit tests and code review; each is worth doing once when the hardware or software is at hand:

- WM_DPICHANGED for the widget and the settings window: it cannot be synthesised cross-process and both monitors of the development machine run at 100%. Set one monitor to 125% and drag across: fonts, sizes and the settings layout follow (OK at client (130, 670), 65 x 33 at 120 dpi).
- Monitor removal or undocking, and the widget returning home when the monitor comes back (C2.5).
- LibreHardwareMonitor 0.9.4 or older end to end (C4.4; needs admin and is not installed): start it after TinyMon (rows appear within 30 s); suspend LHM.exe (the widget keeps repainting, at most one 0.5 s stall per 30 s, and Temp falls back); resume it (rows return within 30 s).
- NIC flags inside VM guests (Hyper-V netvsc, vmxnet3, VirtIO) for C1.6.
- Windows 10 1703-1809: VRAM n/a before 1709; the undocumented uxtheme ordinals.
- A real Explorer restart with a de-elevated instance, and the tray icon's sharpness above 100% (C2.4).
- The height cap with per-core CPU on 16+ threads at 150% (C2.2); a discrete GPU's VRAM row and a Ryzen iGPU's name on real hardware.

## 9. User-visible changes

Widget and data:

- GPU % matches Task Manager (the busiest engine, summed across processes); the red label at 90% or more appears more often under real load.
- The memory row shows adapter-wide usage: discrete cards "VRAM" (dedicated usage of dedicated memory), integrated GPUs "GPU Mem" (dedicated plus shared, for example about 0.8/8G instead of 0/128M); with several adapters "VRAM RTX 3050", "GPU Mem UHD 620". The Settings checkbox still says VRAM.
- A Ryzen iGPU is named "Radeon" instead of "Graphics".
- Network counts physical adapters only: a VPN or virtual switch no longer doubles the rate; n/a when no adapter qualifies.
- Readings that cannot be taken show "n/a" instead of 0 or missing rows.
- CPU, per-core, GPU, disk and net show a pending marker for up to 1 s at startup and after a metric change, instead of a bogus first value (which could paint every core red).
- Temperature: static firmware zones are hidden, real zones read "ACPI Temp", or "ACPI" plus the zone's firmware name (for example "ACPI THRM"), and "Temp n/a" appears when nothing real exists. The owner's widget shows "Temp n/a" until Temperature is unticked. Fresh installs and "Reset to defaults" no longer enable Temperature.
- With LHM 0.9.4 or older: only "CPU Temp" and "GPU Temp", picked up within 30 s of LHM starting, and LHM can no longer freeze the widget.
- Sparklines survive metric changes and Apply, no longer flash an empty track, pause on n/a ticks instead of dipping to zero, and the idle baseline stays inside the bar.
- Disk and Net labels no longer turn red.
- The widget resizes itself when the row count changes, is capped to its monitor's work area, and shows "+N" when rows are hidden.
- Placement: an off-screen position is corrected without losing the saved spot, and the widget returns there when the monitor comes back; taskbar and resolution changes are followed; the first-run default respects the work area; a growing widget keeps its corner anchored, so a larger font grows the owner's right-anchored widget to the left.
- Showing the widget (toggle hotkey, tray, startup) no longer steals focus.
- Starting TinyMon again brings the running widget back instead of opening a second one.
- Tray: the icon survives Explorer restarts, a double-click toggles once, and the icon is sharp at any scaling.
- Start with Windows: the checkmark reflects whether this exe really starts (path, Task Manager's disabled flag); clicking it repairs a stale entry; the path is written quoted.

Settings window:

- 60 px wider (562 px client) with a true-size preview; very tall layouts are scaled to fit the pane.
- Enter presses the focused button (OK everywhere else).
- Win combos from the config survive Apply; arrow and navigation keys display correctly; Shift or Alt alone no longer wipe a field; a combo TinyMon owns can be captured; the field no longer shows a partial "Ctrl + Alt +" while keys are held.
- The same combo for both actions is refused with "Same as Show / Hide"; a failed hotkey is retried on every Apply and shown as "In use by another app".
- Amber "Blocks typing this key" notes for bare typing keys (the owner's "[" and "]" get permanent notes) and "+ Win (set in config)" for Win combos.
- Interval and font inputs clamp, show the applied value and state their ranges.
- Focus returns to the last control on reactivation; reordering keeps focus on the moved item and a double-click moves it two rows.
- The window fits the screen's work area and re-lays out on DPI changes; the dark caption shows from the first frame; arrow cursor and TinyMon icon in the caption; the opacity slider's fill is correct after Reset and jumps.

Config and build:

- Trailing "#" comments are allowed; UTF-16 and ANSI files are read; a portable copy reads its own file first; saves are atomic; duplicate metric tokens are ignored. Comments are still not kept when TinyMon rewrites the file.
- A hand-edited hotkey with two keys, such as "Ctrl+A+B", which used to bind Ctrl+B, now leaves that action unbound.
- The exe declares Windows 10, PerMonitorV2 and asInvoker in a manifest (no visual change).

## 10. Risks and mitigations

| Risk | Mitigation |
|---|---|
| main.rs is the merge hotspot: about a dozen items edit AppState, main(), WM_CREATE, WM_TIMER, WM_PAINT, WM_APP_APPLY and WM_HOTKEY; a live AppState reference across a call that sends messages to the widget is undefined behaviour | C2.9 lands first and sets the borrow rule (5.4); apply_settings has one fixed order; every later main.rs commit is reviewed against the rule and run as a debug copy, where a panic in wnd_proc aborts the process |
| C2.5 changes what x/y mean and when they are saved; a bug could move the owner's widget away from (-213,-16) or overwrite that home while the left monitor is asleep at logon | Pure placement logic tested first with the owner's coordinates; three separate commits for easy bisecting; a clamped or displaced position is never adopted or persisted; debug-copy checks at x=-213 and x=-5000; the owner is told that a larger font now grows the widget to the left |
| Verification gaps: WM_DPICHANGED, monitor removal, LHM 0.9.4, VM NIC flags, Windows 1703-1809, the C2.1b runtime trigger | The logic behind each is pure and unit-tested; the manual checklist (8.5); the connector fallback and n/a floor for network; behaviour documented in the docs pass |
| Test logistics could touch the owner's setup: the autostarted exe locks target/release; elevated copies behave differently under UIPI; after C2.6 release copies surface the owner's widget; C2.7 and C2.8 checks touch the real registry and input | Release builds only in a scratch target dir and never launched; launched copies are debug builds in their own folders with redirected %APPDATA%; `[NullString]::Value` in scripts; the intrusive checks happen only in the live session, with the two registry values recorded and restored, the owner's cfg and exe backed up, no synthetic input, a restart through explorer.exe and a defined failure path (8.4) |
| Visible behaviour changes surprise the owner (GPU % higher, "Temp n/a", the pending marker, no focus steal, leftward growth, amber notes, no partial combo display, a wider Settings window) | Section 9 is given to the owner before the live session; each change traces to a finding and an owner decision |
| Size grows about 8% and C2's share is mostly estimated | Measured after every commit with a hard stop at +22 KB (263,168 B); if re-planning is needed, the separate C3.6a-2 commit (about 0.6 KB) is the first candidate to drop |
| Heuristics and undocumented behaviour: the StartupApproved low bit, the hotkey control reading GetKeyState, the meaning of PDH_NO_DATA, the ACPI liveness rule (a steady real sensor stays hidden until it moves 1 K), the LHM name whitelist | Each is pinned by tests on the forms observed on real systems, and each failure degrades to an honest fallback (a wrong checkmark that a click repairs, n/a, "Temp n/a") |
| Encoding or stray-change accidents (the non-ASCII dash in the Cargo.toml comment, BOMs, CRLF) | .gitattributes from step 0; files written as UTF-8 without BOM; the Cargo.toml comment rewritten in ASCII; every diff reviewed |
| After a metric Apply, surviving rate rows show the pending marker over their real trend for up to 1 s | Accepted: the trend is real data and the masked bar suppresses the bogus red alert |

## 11. Deferred items

| Item | Finding refs | Target | Reason |
|---|---|---|---|
| Common Controls v6 (themed edit and hotkey borders via DarkMode_CFD, trackbar custom-draw re-check, restyled MessageBox) | app-shell[7] (v6 part), settings-ui[18] | SP3 | Needs a visual pass of the dark owner-drawn window; every SP1 change behaves the same under 5.82 and 6.0. ICC_STANDARD_CLASSES stays (InitCommonControlsEx returns FALSE under 5.82, harmlessly) |
| Compact or multi-column per-core layout | render-registry[7] (residual) | SP3 | A UX design; the C2.2 cap and '+N' are the correctness floor |
| Dark-mode build gating (uxtheme ordinals only on build 17763+, DWM attribute 19 on 1809-1909) | settings-ui[19] | SP2 | Portability pass: a version read (about 0.2 KB) with a tested pure gate, verified on a 1703 or 1809 VM |
| Reporting failed saves | settings-ui[5] (residual) | Not planned (YAGNI) | persist() ignores write()'s io::Result; revisit with a tray balloon only if lost settings are reported |
| First LHM connect on the UI thread during the startup priming sample | samplers[2] (residual) | Deferred | A worker thread is out of proportion for a path that only works with LHM 0.9.4 or older; ConnectServer is bounded at 2 minutes; ExecQuery has no bound if the WMI service itself hangs; documented |
| Static CRT (hybrid, measured +21.5 KB; the exe imports VCRUNTIME140.dll today) and username-free builds (panic-location strings embed the build machine's user-profile path; `--remap-path-prefix` or CI builds) | build notes | SP2 | The portable-exe pass |
| README and doc drift | render-registry[10], samplers[11], the remaining docs-spec findings (those not mapped to an item or another row) | Docs pass at the end of SP1 (step 38, before the live session) | Docs must describe the final behaviour |
| Icon artwork unification (runtime glyph versus icon.ico; loading resource 1 for the tray) | app-shell[8] (residual) | SP3 | Cosmetic; tied to the v6 and resource decisions |
| Discrete-GPU temperature via D3DKMT adapter perf data | owner decision | SP3 | build_rows is the plug-in point |
| LHM web-server reader | owner decision | Not planned | The legacy WMI reader is kept and hardened only |
| Settings layout for screens of 768 px or less at 125%+ | settings-ui[12] (residual) | SP3 | Needs a layout change (for example the Hotkeys card beside the preview) |
| A neutral bar colour for throughput rows | render-registry[5] follow-up | SP3 | A UX decision; SP1 removes only the false alarm |
| Re-detecting adapters after a live driver update changes a LUID | C1.1/C1.2 risk | Not planned | Rare; a restart or metric change recovers |
| Reusing unchanged sources across Apply | C1.5 risk | Not planned | Rebuild-all is simpler; the marker lasts at most 1 s |
| Partial modifier display in hotkey fields (subclassing) | C3.1 risk | Not planned | About three times the code for a cosmetic gain |
| Numbered "GPU 1 Temp" / "GPU 2 Temp" labels for LHM | C4.4 risk | Not planned | About 0.5 KB more for an LHM 0.9.4-only path |
| BOM-less UTF-16 config files | C4.1 option | Not planned | Rare; would need NUL-byte skipping |
| Reusable per-tick row and string buffers | docs-spec[25] | Not planned | Negligible cost |
| LICENSE files; repository URL check | docs-spec[27], docs-spec[28] | SP4 | Release work |

## 12. Amendments made during implementation

Sections 1-11 above stay as the owner approved them. This section records where the implementation departs from them: the owner's decisions taken during the work, the decisions made while the work was planned, the hardening and review-fix commits, and every place where the text above no longer matches the code. Where the two disagree, this section and the code win. All dates are 2026.

### 12.1 Owner decisions

| Date | Decision | What it changes |
|---|---|---|
| 10-07 | C1.5's `MetricKey::is_rate()` is named `is_delta()` ("the value is the delta between two samples"), so that it cannot be confused with `Row::rate`; the semantics are unchanged | 5.2, C1.5 and its tests read `is_delta` (`delta_metrics_are_flagged`, `collect_rows_masks_only_delta_metrics_while_pending`); landed with step 14 (b293514) |
| 10-08 | A hardening task before step 12: tests that pin behaviour Phase A's code relied on but no test fixed, and three small code changes | Five commits, 6e37ecb to 1954e99 (12.3); C1.1's UMA rule changes (12.4) |
| 10-08 | A test-only follow-up to step 14 makes the pending-mask test independent of the per-core priming sample, which made it fail now and then | c5a0401 and its review fix a94eaaa (12.3) |
| 10-08 | The widget's home must not move from one launch to the next: the first tick after startup puts the widget at home instead of resizing it from a corner. With it, the other fixes of the Phase B review: the marker stays short across an interval-only Apply, a partial per-core sample keeps the known cores as n/a, and two borrow sites are scoped | e0bd7ae, 3aeba5c, 6a8fbc2 and 315b6a7 (12.3); 5.4, 5.7, 5.8, C1.4, C1.5, C2.1a and C2.1b change (12.4) |
| 10-09 | Step 28 (C3.6a-2, the settings window's WM_DPICHANGED relayout), which Section 10 names as the first commit to drop, is kept | 64c7160; its check stays manual (8.5) |
| 10-09 | In Phase C, a task whose tree matches the plan's verified reference tree does not repeat its debug-copy runtime check (8.3): the reference run stands for it. A commit whose tree differs runs its checks | The runtime evidence of steps 22-31 is the plan's reference runs |
| 10-09 | The legacy LibreHardwareMonitor (LHM) WMI reader is removed instead of hardened, as the first Phase D commit, before step 33. LHM 0.9.5 and later publish no WMI namespace, and the owner does not run LHM. The Temperature metric keeps the PDH ACPI thermal-zone reader (and later, in SP3, a D3DKMT discrete-GPU source). The `temp` feature stays on by default but no longer enables `Win32_System_Com`, `Win32_System_Ole`, `Win32_System_Variant` or `Win32_System_Wmi` (measured: -3,072 B) | Replaces C4.4 and step 35; C4.2 loses its LHM parts (12.4) |
| 10-09 | The live session (8.4, decision D3) happens at the very end of SP1, and only when the owner says go. The C2.6 and C2.7 runtime checks wait for it. It adds one optional manual step: the owner types a real `Ctrl+Alt+<key>` combo into a Settings hotkey field and sees it captured | 8.4 items 5, 6 and 10 |

### 12.2 Decisions made while Phase D was planned (10-09)

- Step 33 (C4.2) is one commit without the prep commit: Phase D's first commit adds the test `zone_rows_keep_their_labels`, which pins the old zone rows in the place of `build_rows_characterizes_todays_fallback`, and without the LHM reader there is no `pick_lhm`. `build_rows(zones, out)` takes only the zones (SP3's GPU source plugs in there), and `lhm_rows_take_precedence_over_zones` is dropped. Added tests: `zone_labels_name_the_acpi_zone`, `registry::tests::preview_temp_row_claims_no_cpu_sensor` and `battery::tests::no_battery_row_is_value_only`. The `Cargo.toml` comment reads: "temp - the Temperature metric (live ACPI thermal zones via PDH, else n/a). On by default; --no-default-features removes the metric."
- Follow-up items folded into Phase D: the battery's no-battery row is `Row::na("Bat")` (with step 33); `WM_DESTROY` clears `GWLP_USERDATA` before it reclaims the `AppState` box, so no site keeps an AppState handle across a call the borrow rule names, and an `apply_settings` comment says that a first tick still due "stays capped" (both with step 36).
- Step 36 (C2.6): `wm_app_ids_are_distinct` also pins `WM_APP_SURFACE == WM_APP + 4`, the value another build may post; `surface` starts the tray retry when its add fails, as `WM_CREATE` does.
- Step 37 (C2.7): `launches` ignores the case of ASCII letters only (a byte loop with `u8::eq_ignore_ascii_case`, about 0.4 KiB smaller than `str::eq_ignore_ascii_case`); a non-ASCII letter in another case reads as off, which a click repairs. An opening quote without a closing one takes the rest of the value as the path. The registry is written with `RegSetKeyValueW` and `RegDeleteKeyValueW` and read with `RegGetValueW`.
- The runtime checks of C2.6 and C2.7 run only in the live session: a debug build skips the single-instance guard, a release copy is never launched outside the live session (and one could find the owner's widget), and the C2.7 check writes the real Run and StartupApproved values.
- The C2.1b check that Section 7 asks to repeat after step 33 is not repeated: the development machine's only thermal zone is a constant stub, so the temperature rows never change at runtime there.
- Step 38 is three commits: the code comments, the README, and this section.

### 12.3 Hardening and review-fix commits

| Date | Commit | Subject | Spec text it changes |
|---|---|---|---|
| 10-08 | 6e37ecb | test: pin config and hotkey edge cases the suite left open | none (tests only) |
| 10-08 | 7184172 | test: pin sampler and row edge cases the suite left open | none (tests only) |
| 10-08 | 5c46ca2 | gpu: show n/a for integrated GPU memory when shared usage is unknown | C1.1 |
| 10-08 | 00d5df6 | gpu: drop short_name's second whitespace squash | C1.3 |
| 10-08 | 1954e99 | render: expect too_many_arguments on render_rows and draw_row | 6.0, 0d |
| 10-08 | c5a0401 | test: make the pending-mask test independent of per-core priming | C1.5 (tests) |
| 10-08 | a94eaaa | test: cover a two-row delta metric in the pending-mask test | C1.5 (tests) |
| 10-08 | e0bd7ae | main: put the widget at home on the first tick after startup | 5.7, 5.8, C2.1b |
| 10-08 | 3aeba5c | main: keep the pending marker short across an interval-only Apply | 5.4, C1.5 |
| 10-08 | 6a8fbc2 | percpu: keep known cores as n/a when a sample lacks some of them | C1.4, C2.1a |
| 10-08 | 315b6a7 | main: scope the AppState borrows in toggle_lock and open_settings | none (5.4 applied to the letter) |
| 10-09 | 2f1b35d | settings_win: create each hotkey note before its label | C3.8 |
| 10-09 | 2de6d66 | settings_win: scale control edges, not sizes | C3.6a, C3.6b (tests) |
| 10-09 | fc97306 | settings_win: end overflowing hotkey labels in an ellipsis | C3.8 |
| 10-09 | b285053 | settings_win: pin the hotkey note code mapping | none (tests only) |
| 10-09 | fdfaaf6 | dpi: decode WM_DPICHANGED's DPI in one place | 5.1 |

Phase D, planned on 10-09, adds eight commits after fdfaaf6, in this order: "temp: remove the LibreHardwareMonitor WMI reader" (the owner's amendment, in the place of step 35), "temp: show an ACPI thermal zone only once its reading moves" (step 33), "registry: leave Temperature out of the default metrics" (step 34), "main: let a second launch surface the running widget and exit" (step 36), "startup: match the Run value to this exe and write it quoted" (step 37), and step 38's "docs: bring stale code comments up to date", "docs: rewrite the README for the finished correctness pass" and "docs: record the amendments made during implementation" (this section).

### 12.4 Where the text above no longer matches the code

- Section 2 and 8.2: SP1 ends with 225 tests and 2 ignored diagnostics (213 and 2 without default features), not about 190 and 176; the release exe ends at 258,048 B (255,488 B without default features), +17,408 B (17.0 KiB) over the baseline, below the "about 259,500 B" of Section 2 and 5,120 B below the 263,168 B stop.
- Section 3, the temperature decision: the LHM WMI reader is removed, not kept and hardened (12.1); the rest of the decision stands.
- Section 4: the UI thread makes no WMI call any more, so the "bounded (the LHM WMI reader; a hung winmgmt ...)" exception is void.
- 5.1: `src/temp.rs` has zone liveness, honest labels and `build_rows`, and no LHM reader; `src/dpi.rs` also has `dpi_from_wparam`, the one WM_DPICHANGED decode for the widget and the settings window (fdfaaf6); `src/battery.rs` builds its no-battery row with `Row::na("Bat")`; `Cargo.toml`'s `temp` feature enables no windows feature.
- 5.2 and C1.5: `MetricKey::is_delta` for `is_rate` (12.1).
- 5.4, step 1: an interval change re-arms TIMER_ID with `timer_period(interval, first_tick_pending)`, which is `first_tick_ms(interval)` while the first tick after a build of the sources is still due and the interval otherwise (3aeba5c).
- 5.7: the first TIMER_ID tick after startup catches a changed row count up with `place_widget` (the widget goes home, home is kept), and only later ticks call `sync_layout` (`tick_layout`, e0bd7ae).
- 5.8: AppState also has `startup_layout_pending` (e0bd7ae).
- 5.9: no WBEM status codes are used; the `temp` feature enables no windows feature.
- 6.0, 0d: `render_rows` and `draw_row` carry `#[expect(clippy::too_many_arguments)]` instead of `#[allow]` (1954e99), and after 0d the suite reported 0.00 s rather than about 0.2 s.
- C1.1: for an integrated (UMA) adapter `vram_used` is None when the shared usage is unknown; an unknown dedicated part still counts as 0, and discrete adapters are unchanged (5c46ca2). So `vram_used(true, Some(x), None)` is None, where the text above says "None only when both are None".
- C1.3: "Every existing output is unchanged" holds except for names with unusual whitespace, which now normalise; `short_name` normalises whitespace once (00d5df6).
- C1.4 and C2.1a's risks: once cores are known, a per-core sample yields the union of the known indices and the returned ones, so a core missing from one sample shows "C{i} n/a" and its sparkline pauses, instead of the list being replaced and the series restarting (6a8fbc2).
- C1.5's tests: `collect_rows_masks_only_delta_metrics_while_pending` builds its sources by hand (CPU, RAM, and a per-core and a disk sampler that have not sampled) instead of a primed `build_sources([PerCpu, Ram])` (c5a0401, a94eaaa). The marker also stays at most a second across an interval-only Apply (3aeba5c).
- C2.1b: see 5.7: the first tick after startup places the widget at home; rows that arrive on a later tick still resize it from a corner with home following.
- C2.4's risks: a failing try can block the UI thread for up to two Shell_NotifyIconW timeouts, NIM_ADD and then NIM_MODIFY.
- C2.6: as designed, with the additions of 12.2.
- C2.7: as designed, with the choices of 12.2.
- C3.6a: `Place::scaled` scales a control's edges and takes its size between them, so controls that touch in logical units also touch in device pixels (2de6d66); the test list gains `scaled_controls_do_not_overlap_at_any_dpi`.
- C3.6b: its test `preview_pane_fits_the_widest_widget_at_any_dpi` reads the pane's device width from `Place::scaled`, not `Scale(dpi).px(w) - 2` (2de6d66). Its risks say that layouts of 12 or more rows at large fonts are scaled; at 96 dpi the preview is scaled from 13 rows at fonts 15-16, 14 at fonts 13-14, 15 at fonts 11-12 and 16 at fonts 9-10.
- C3.8: the two hotkey labels are created with SS_ENDELLIPSIS (fc97306), and each note is created before its label, so a screen reader names each hotkey field after its label (2f1b35d).
- C4.2: without the LHM parts (12.2): no prep commit, no `pick_lhm`, `build_rows(zones, out)`, no `lhm_rows_take_precedence_over_zones`, and the `Cargo.toml` comment of 12.2.
- C4.4 and Section 7, step 35: replaced by the removal of the reader, done after step 32 and before step 33 (12.1). Section 7's totals change with it: about -3.0 KiB instead of +1.0 KiB.
- 8.3, the C3.8 row: Win+F24 cannot be registered on the development machine (RegisterHotKey fails with 1409), so a check that needs a free Win combo uses Win+Shift+F24.
- 8.4: as written, with the optional step of 12.1 in item 10. The Phase D plan gives the session's commands.
- 8.5: the LibreHardwareMonitor item is void. Since 2de6d66, controls that touch in logical units also touch at 125%, 175% and 225%, which no check at 96 dpi can show; the manual DPI check is where to look at the hotkey labels and notes and the opacity readout against the controls below them.
- Section 9: the LHM bullet is void: TinyMon reads no LHM sensors.
- Section 10: the "LHM name whitelist" in the heuristics row is void.
- Section 11: the rows "First LHM connect on the UI thread during the startup priming sample" and "Numbered GPU 1 Temp / GPU 2 Temp labels for LHM" are void; "LHM web-server reader" stays not planned, now because TinyMon reads LHM in no way.

### 12.5 The branch when the code work ends

- Commits after this document's own commit (a5c4f69): 19 on 10-07 (steps 0 to 11), 21 on 10-08 (the hardening task's five, steps 12 to 14, the two step-14 follow-ups, steps 15 to 21 and the four review fixes), 17 on 10-09 (steps 22 to 32, step 23 as two commits, the review fix 2f1b35d and the four fixes 2de6d66 to fdfaaf6), and Phase D's eight: 65 in all, none with a co-author trailer.
- Tests: 225 passed and 2 ignored (213 and 2 with `--no-default-features`).
- Release exe: 258,048 B (255,488 B with `--no-default-features`).
- Still to come: the live session (8.4). The branch `sp1-correctness` stays local, neither merged into `main` nor pushed; merging is the owner's decision.
