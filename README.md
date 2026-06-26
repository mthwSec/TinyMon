# TinyMon

A tiny, always-on-top Windows resource monitor written in **pure Rust + Win32** —
no Electron, no webview, no runtime, no Python/C#. A single self-contained `.exe`
(~195 KB) with a near-zero footprint. Pick exactly what it shows, restyle it, and
drive it from global hotkeys — all from a native settings window.

```
┌──────────────────────┐
│ CPU              37%  │   label · value · load-colored bar
│ ████████░░░░░░░░░░░░  │   (green → yellow → red by load)
│ RAM       58% 9.3/16G │
│ ████████████░░░░░░░░  │
│ Intel UHD 730  2% 0/… │   one row per detected GPU
│ █░░░░░░░░░░░░░░░░░░░░  │
└──────────────────────┘
```

## Metrics (all individually toggleable)

| Metric | Shows | Source |
|---|---|---|
| **CPU** | total usage % | `GetSystemTimes` deltas |
| **RAM** | % used + used/total GB | `GlobalMemoryStatusEx` |
| **GPU** | one row per adapter (discrete **or** integrated, **any vendor**): VRAM used/total + live utilization % | DXGI + vendor-neutral PDH "GPU Engine" counters |
| **Per-core CPU** | one row per logical core | PDH `Processor(*)` |
| **Disk** | total throughput + busy % | PDH `PhysicalDisk(_Total)` |
| **Network** | total throughput | PDH `Network Interface(*)` (sums real NICs) |
| **Uptime** | time since boot | `GetTickCount64` |
| **Temperature** *(optional)* | CPU/GPU temps, one row per sensor | LibreHardwareMonitor via WMI — see [Temperature](#temperature-optional) |

Only **CPU, RAM, GPU** are on by default; the rest are opt-in from the settings
window. **Disabling a metric truly disables it** — a hidden metric is never
sampled and opens no performance-counter query, so it costs nothing.

## Features

- **Configurable** — a native settings window to choose **what's shown**, restyle
  it (opacity, update interval, font size), bind **global hotkeys**, and a **live
  preview** that mirrors the widget as you change things (plus **Reset to defaults**)
- **Readable at a glance** — per-metric **accent-colored labels**, a compact
  header, aligned value columns, and load-colored bars (green → red)
- **DPI-aware** (Per-Monitor-V2) — crisp and correctly sized on 4K / scaled
  displays, and rescales when dragged between monitors
- **Always-on-top, borderless, draggable** layered window — semi-transparent,
  no taskbar button, drag from anywhere
- **System tray icon** (a runtime-drawn TinyMon glyph) with a right-click menu
  (mirrored on the window itself): Show/Hide · **Settings…** · **Start with Windows** · Exit
- **Global hotkeys** (optional) — toggle visibility / open settings from anywhere
- **Portable** — remembers everything in a config file that travels with the
  `.exe`; runs from a USB stick with no install
- **Tiny** — size-optimized, double-buffered (flicker-free), minimal per-frame
  allocations; a single self-contained ~205 KB `.exe`

## Requirements (build)

- **Rust** (stable) with the **MSVC** toolchain (`x86_64-pc-windows-msvc`)
- **Visual Studio Build Tools** (the C++ "Desktop development" workload, which
  provides `link.exe`) and a **Windows 10/11 SDK**

`cargo` locates the MSVC linker automatically — no "Developer Command Prompt"
needed.

## Build

```sh
cargo build --release
```

The binary is produced at:

```
target\release\tinymon.exe
```

Just run it (double-click or from a shell). It's a self-contained executable —
copy it anywhere, including a USB stick.

### Optional: temperature support

Temperature readings are behind an off-by-default Cargo feature so the standard
build pulls in **no** COM/WMI bindings and stays tiny:

```sh
cargo build --release --features temp
```

This enables the **Temperature** metric (adds ~3.5 KB). At runtime it reads from
**LibreHardwareMonitor** — see [Temperature](#temperature-optional).

## Usage

| Action | How |
|---|---|
| Move the window | Click-drag anywhere on it |
| Open settings | Right-click → **Settings…** (or a hotkey you bind) |
| Show/hide | Left-click the tray icon (or menu → Show/Hide, or a hotkey) |
| Context menu | Right-click the tray icon **or** the window |
| Start with Windows | Menu → "Start with Windows" (checkmark = on) |
| Hide to tray | Close (Alt+F4) or press **Esc** — it does **not** quit |
| Quit | Menu → **Exit** (only this exits; the tray icon is removed) |

The window updates once per second by default (configurable, 250 ms – 10 s).

## Settings & customization

Right-click the window or tray icon → **Settings…** opens a native window with
three control sections plus a **live preview**:

- **Metrics** — checkboxes (two columns) to show/hide each metric. Toggling one
  off rebuilds the active set and drops the disabled sampler (closing its
  performance-counter query), so hidden metrics use **no resources**.
- **Appearance** — **opacity** (slider with a live readout), **update interval**
  (ms), and **font size** (px). Font size scales the whole widget — bigger font
  means taller rows *and* a proportionally wider window, so nothing clips.
- **Hotkeys** — capture fields for two **global hotkeys** (*toggle visibility*,
  *open settings*) that work from any app, each with a **Clear** button to remove
  the binding.
- **Live preview** — a mini-widget drawn with the *same* renderer as the real
  widget, updating as you toggle metrics / drag opacity / change the font. It
  uses representative values, so it opens no performance-counter queries.
- **Reset** — restores every control to the factory defaults (you still click
  Apply / OK to commit).

Full keyboard support: **Tab** between fields, **Space** toggles a checkbox,
**Enter** = OK, **Esc** = Cancel. Changes apply live (no restart) on **Apply**
or **OK**, and are saved to the portable config.

## Portable config (USB-safe)

Everything is saved to **`tinymon.cfg`** (a tiny `key=value` text file). The save
location is chosen by an actual *test write*, never guessed:

1. **Next to the `.exe`** if that folder is writable — portable mode, ideal for
   USB sticks.
2. Otherwise **`%APPDATA%\TinyMon\tinymon.cfg`** (e.g. when run from a read-only
   stick or `C:\Program Files`).

So the same `.exe` works copied to a USB drive and run on any PC.

It's plain text and safe to hand-edit (unknown keys are ignored, and a leading
UTF-8 BOM from editors like Notepad is tolerated). Keys:

```ini
x=100                     # window position (set automatically on move)
y=24
metrics=cpu,ram,gpu       # which metrics to show, in order
opacity=220               # 40–255
interval_ms=1000          # 250–10000
font_px=13                # 9–16
hotkey_toggle=Ctrl+Alt+M  # optional; e.g. Ctrl+Shift+F1
hotkey_settings=Ctrl+Alt+S
```

Valid `metrics` tokens: `cpu`, `ram`, `gpu`, `percpu`, `disk`, `net`, `uptime`
(and `temp` in a `--features temp` build).

## Start with Windows

The "Start with Windows" toggle writes a per-user registry value (no admin):

```
HKCU\Software\Microsoft\Windows\CurrentVersion\Run    TinyMon = <full exe path>
```

Toggling on writes it, off deletes it, and the current state is read on launch
to show the checkmark.

## GPU support — design note

GPU detection is intentionally **vendor-neutral**: it shows *any* GPU (discrete
or integrated — AMD, Intel, NVIDIA) using only DXGI (VRAM) and Windows PDH
performance counters (load). It deliberately does **not** depend on any vendor
SDK (e.g. AMD ADLX/ADL or NVIDIA NVML), so one binary behaves the same on a
discrete-GPU gaming PC and an integrated-GPU work PC. VRAM uses the DXGI *LOCAL*
memory segment; integrated GPUs report a small local pool (shown in MiB).

## Temperature (optional)

Windows exposes **no** vendor-neutral temperature API to a normal user-mode
process, so TinyMon reads temperatures from
[**LibreHardwareMonitor**](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor)
(LHM) — a free, open-source tool that publishes sensor data to a WMI namespace
(`ROOT\LibreHardwareMonitor`). This is vendor-neutral (any CPU/GPU LHM supports)
and needs no admin rights in TinyMon itself.

To use it:

1. Build with the feature: `cargo build --release --features temp`.
2. Install and run **LibreHardwareMonitor**, and enable its WMI/"Remote Web
   Server" sensor publishing (LHM exposes sensors to WMI when running).
3. Enable **Temperature** in TinyMon's settings.

When LHM isn't running, the metric shows a single **`Temp  n/a`** row rather than
disappearing — and the COM/WMI calls fail gracefully (no hang, no crash). The
default build doesn't include this code at all.

> Verified here: clean compile, graceful **`n/a`** + crash-free operation when LHM
> is absent, and the size delta (+3.5 KB). The *populated* readings path requires
> a machine running LHM to confirm end-to-end.

## FPS — why it's not included

A per-application **frame rate** is a common ask, but Windows offers no
vendor-neutral, **user-mode**, no-admin way to measure a *specific game's* render
FPS, and every real approach gives up one of TinyMon's core promises:

| Approach | Cost |
|---|---|
| **PresentMon / ETW** (how RTSS/CapFrameX work) | needs **Administrator** or *Performance Log Users* group to open an ETW trace |
| **Bundle PresentMon.exe** | **+several MB** (~20× the size) and a second binary; still needs ETW rights |
| **Overlay injection** (Fraps/RTSS-style) | **AV / anti-cheat flag it as malware**; fragile |
| **`DwmGetCompositionTimingInfo`** | no-admin & tiny, but it's the **desktop compositor's** rate (≈ monitor Hz), *not* a game's FPS |

So FPS is **deliberately deferred**. The architecture leaves a clean slot for it
(see [`src/fps.rs`](src/fps.rs) for the rationale and a concrete integration
sketch); if added later it would live behind an opt-in Cargo feature (like
`temp`) using PresentMon/ETW — documented to require elevation, never injection.

## Antivirus note (false positives)

TinyMon is a small, **unsigned**, freshly-compiled binary that reads system
performance counters and writes one registry Run value — a behavior profile that
some heuristic/ML antivirus engines flag as a **false positive** (e.g. Symantec
`Heur.AdvML.B`, or Microsoft Defender). It does nothing malicious: no network, no
file writes outside its own config. If your AV quarantines it (symptoms: build
fails with *"Access is denied (os error 5)"*, or the `.exe` shows 0 KB / goes
missing), add a **folder exclusion** for the project/output directory, or restore
it from quarantine. Code-signing the binary would avoid this for distribution.

## Project structure

```
src/
  main.rs         window, message loop, double-buffered paint, tray + menu,
                  live-apply of settings, hotkeys, DPI (WM_DPICHANGED), app icon
  render.rs       shared renderer (render_rows) + layout/colors + geometry math
                  (unit-tested) — used by BOTH the widget and the settings preview
  dpi.rs          Per-Monitor-V2 awareness + Scale (logical->device px, tested)
  icon.rs         runtime-drawn app/tray icon (pure pixel routine, unit-tested)
  registry.rs     metric registry: MetricKey / MetricSource / build_sources +
                  per-metric accents + sample_rows + pure formatters (unit-tested)
  settings.rs     runtime Settings (defaults + clamps) <-> on-disk Config
  settings_win.rs native settings window + live preview child + reset-to-defaults
  hotkeys.rs      global-hotkey parse/format + register/unregister (unit-tested)
  metrics.rs      CPU% / memory sampling + pure math (unit-tested)
  pdh.rs          shared PDH performance-counter helper (open/sample/Drop-close)
  gpu.rs          DXGI adapter enumeration + PDH GPU-load sampler
  percpu.rs       per-core CPU sampler (PDH Processor(*))
  disk.rs         disk throughput + busy% sampler (PDH PhysicalDisk)
  net.rs          network throughput sampler (PDH Network Interface)
  temp.rs         optional temperatures via LibreHardwareMonitor WMI (feature "temp")
  fps.rs          FPS design notes (deferred — documentation only, emits no metric)
  config.rs       portable tinymon.cfg load/save (unit-tested)
  startup.rs      HKCU Run-key "Start with Windows" toggle
```

## Implementation notes

- Pure Rust + the official [`windows`](https://crates.io/crates/windows) crate
  for Win32/DXGI/PDH/registry bindings; only the needed crate features are enabled.
- Release profile is size-optimized: `opt-level = "z"`, `lto = true`,
  `codegen-units = 1`, `panic = "abort"`, `strip = true`.
- Every `unsafe` block carries a comment explaining the Win32 call it makes.
- Pure logic (CPU% math, byte→GiB, load→color, config parsing, adapter-name
  cleanup) is covered by unit tests: `cargo test`.

## License

MIT OR Apache-2.0
