# SP3: UX polish (design)

Status: approved by the owner on 2026-10-16. Branch `sp3-polish`, from `main` at 9fed6fe. Release exe at the start: 280,576 B. Hard stop: **286,720 B**.

## 1. GPU temperature (approach A: part of the Temperature metric)

- **Source.** Windows' D3DKMT adapter performance data (`D3DKMTQueryAdapterInfo`, `KMTQAITYPE_ADAPTERPERFDATA`, Windows 10 1803 / WDDM 2.4 and later). This is the value Task Manager shows as "GPU temperature". It needs no admin rights and no extra software.
  - The adapter is opened by its DXGI LUID (`D3DKMTOpenAdapterFromLuid`) and closed after each query.
  - The windows crate feature `Wdk_Graphics_Direct3D` provides the bindings at compile time.
- **Values.** `Temperature` is in tenths of a degree Celsius.
  - 0 means the driver does not report one; an integrated GPU usually reports 0.
  - Readings outside 1-150 C are ignored, as for the ACPI zones.
- **Rows.** The Temperature metric shows the GPU rows first, then the live ACPI zones. "Temp n/a" appears only when neither has a row.
  - A GPU gets a row once it has reported a plausible temperature.
  - The label is "GPU" when one GPU reports, and "GPU <short name>" when several do.
  - The row looks like the ACPI row: the value in degrees C, a bar that fills at 100 C, and a red label from 90 C.
- **Missing readings.** A GPU or ACPI zone that has been shown, but has no reading in a sample, shows `n/a` for that tick instead of disappearing. The row count therefore stays stable, and only that row's sparkline pauses.
- **On the development PC.** The Intel UHD 730 reports 0, so no GPU row appears there. A live test checks that the query runs and returns "none".

## 2. Robustness fixes (from the follow-up inbox, sections C, H and I)

1. **CPU.** The deltas use checked subtraction. If any counter steps back, the tick is skipped: the last value is kept and the new baseline is stored. A failed `GetSystemTimes` gives `n/a` instead of 0%.
2. **Network.** When a counter steps back (a reset or a momentary 0), that tick and the next one show 0. The spike that the whole counter would otherwise produce as one delta never appears.
3. **GPU adapter set.** Adapters are re-detected when DXGI reports a changed adapter set (`IDXGIFactory1::IsCurrent` is false, after a driver update or an eGPU plug). The GPU-engine PDH query is opened only when there is a hardware adapter.
4. **ACPI zones.**
   - The label uses the zone's own ACPI name, the text after the last `.` or `\` (`\_tz.tz0` reads "ACPI TZ0").
   - Duplicate instance names in one sample count once, so they cannot prove a stub live.
5. **Placement.**
   - `surface()`, a display change and a work-area change leave the widget alone while a drag is in progress.
   - `placement::anchored` uses saturating arithmetic, so an odd suggested rect cannot wrap.
6. **GPU names.**
   - `short_name` drops a trailing parenthesised part ("(16GB)"), a trailing "GPU" word, and "with Max-Q Design" / "Laptop GPU". So "Intel(R) Arc(TM) 140V GPU (16GB)" becomes "Intel Arc 140V", and "NVIDIA GeForce GTX 1650 with Max-Q Design" becomes "GTX 1650".
   - Several adapters with the same short name are numbered ("GPU 1", "GPU 2").
7. **Start with Windows.** A legacy unquoted Run value that contains a space now reads as off, so one click rewrites it quoted. An unquoted path with spaces can launch a different program.
8. **Settings.** The metric list's toggle and move operations check the list's own length.
9. **Single instance.** A debug build closes its mutex handle, so a debug copy no longer holds the single-instance mutex for a later release launch.

## 3. Comment and documentation cleanup

These are the open wording items of the follow-up inbox sections H and I. They are comments and README text only, with no behaviour change:
- the fps.rs "on by default" line;
- the registry DEFAULT doc;
- the "single-instance" naming of the one settings window;
- the HANDLE no-Drop note;
- the startup.rs docs;
- the settings_proc doc;
- the VramGroup and vram_used docs;
- the WM_TIMER and timer_period docs;
- "home" instead of "window position" in config.rs and the README;
- README: the opacity and interval out-of-range wording, the place of the hotkey note, and "one TinyMon per logon session".

## 4. Process and acceptance

- Work happens on `sp3-polish`, test-first.
- The gates run after every commit: fmt, clippy `-D warnings` and tests in both feature sets, and the release script.
- Every commit's release exe stays at or below 286,720 B, and `scripts/build-release.ps1` passes.
- An Agent reviewer checks the branch before the merge. The merge happens with the owner's OK.
- After the merge, the owner's TinyMon is rebuilt with the release script and restarted with the safe swap.

## 5. As built (2026-10-16)

- Release exe: 286,208 B (281,088 B without default features), +5,632 B over 280,576 B and 512 B under the stop. GPU temperature costs 2,560 B, the adapter re-detection 512 B, the network fix 512 B, and the GPU name rules 2,048 B (mostly 512-byte section steps). The other fixes cost nothing measurable.
- Tests: 250 passed with 2 ignored, and 231 with 2 without default features.
- Deviation from 2.6: a trailing "GPU" word is dropped only before a parenthesised part ("140V GPU (16GB)"). This way an unknown name such as "Some Custom GPU" keeps it, as an existing test requires.
- `place_widget` itself skips while a drag is in progress, which covers all of its callers.
- `scripts/build-release.ps1`'s default size stop is 286,720 B.
