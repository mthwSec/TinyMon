//! FPS (frames per second) — **deferred by design; intentionally not implemented.**
//!
//! This is a documentation-only module: it contains no executable code, so it
//! adds nothing to the binary. It exists to (1) record *why* a real FPS metric
//! isn't in TinyMon and (2) show *exactly* how one would plug into the metric
//! registry, so adding it later is a localized change rather than a refactor.
//!
//! # Why FPS is hard (and not in the build)
//!
//! Windows exposes **no** vendor-neutral, user-mode API for a *specific
//! application's* render frame rate. Every realistic approach breaks at least
//! one of TinyMon's defining promises — a tiny, portable, **no-admin**,
//! AV-clean, single `.exe`:
//!
//! | Approach | How it works | Why it's not the default |
//! |---|---|---|
//! | **PresentMon / ETW** | Consume `Microsoft-Windows-DxgKrnl` *present* events from an ETW trace session (the technique RTSS / CapFrameX / Intel PresentMon use). | Starting an ETW trace requires **Administrator** or membership in the *Performance Log Users* group. ~hundreds of lines of consumer code; untestable without a GPU workload. |
//! | **Bundle `PresentMon.exe`** | Spawn Intel's PresentMon as a child process and parse its CSV/stdout. | Adds **several MB** (~20× TinyMon's size) and ships a *second* binary — breaks "tiny single exe". Still needs ETW privileges. |
//! | **Overlay injection** | Inject a DLL and hook `IDXGISwapChain::Present` in the target process (Fraps / RTSS style). | **Anti-cheat and AV flag this as malware.** Fragile, per-API (D3D9/11/12/GL/VK), and antithetical to a "clean little exe". |
//! | **DWM composition rate** | `DwmGetCompositionTimingInfo` — no admin, tiny. | Reports the **desktop compositor's** present cadence (≈ your monitor's refresh, e.g. 60/144), **not** a game's render FPS. Low value; would need an honest label so it isn't mistaken for game FPS. |
//!
//! # Decision
//!
//! Not built. If pursued later, the recommended path is **PresentMon/ETW behind
//! a `fps` Cargo feature** (feature-gated like the `temp` metric, which is on
//! by default), documented to require elevation, and **never** injection. The
//! no-admin `DwmGetCompositionTimingInfo` path is only worthwhile if labelled
//! plainly as a *compositor* rate (e.g. `DWM 144`), which is a different metric
//! than FPS.
//!
//! # How it would plug in (integration sketch)
//!
//! The registry in [`crate::registry`] is built so a new metric is an additive,
//! local change — the same five edits the optional `temp` metric makes. Nothing
//! else in the app needs to change.
//!
//! ```ignore
//! // 1) registry.rs — reserve the key (feature-gated, like Temp)
//! pub enum MetricKey { /* … */ #[cfg(feature = "fps")] Fps }
//! //    + as_str => "fps", from_str "fps" => Fps, label => "FPS", and ALL += 1
//!
//! // 2) registry.rs — the live source variant
//! pub enum MetricSource { /* … */ #[cfg(feature = "fps")] Fps(crate::fps::FpsSource) }
//! //    + key()/sample()/rows()/build_sources() arms, each #[cfg(feature = "fps")]
//!
//! // 3) fps.rs — the source itself (this file), behind #[cfg(feature = "fps")]
//! pub struct FpsSource {
//!     // e.g. an ETW trace-session handle + a ring buffer of present timestamps
//! }
//! impl FpsSource {
//!     pub fn new() -> Self { /* start the ETW session (requires privilege) */ }
//!     pub fn sample(&mut self) { /* drain present events; compute frames/sec */ }
//!     pub fn rows(&self, out: &mut Vec<crate::registry::Row>) {
//!         // bar scaled to a sensible ceiling (e.g. 144 Hz); honest label.
//!         // Row::rate, not Row::bar: a high frame rate must not turn the label red.
//!         out.push(crate::registry::Row::rate("FPS", format!("{:.0}", self.fps),
//!                                             (self.fps / 144.0).min(1.0)));
//!     }
//! }
//! ```
//!
//! Because disabled metrics are never constructed by `build_sources`, an FPS
//! source added this way would also honor the resource-off invariant: no ETW
//! session is opened unless the user enables FPS.
