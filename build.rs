//! Build script: embed the application icon, Windows version-info (file details)
//! and the application manifest (`tinymon.manifest`) into the `.exe`. Runs only
//! at build time and adds nothing to the runtime dependency set. Best-effort: if
//! the SDK resource compiler is unavailable the build still succeeds (the binary
//! then ships without these resources; `dpi::set_process_dpi_aware` still makes
//! the process Per-Monitor-V2 aware at runtime).

fn main() {
    // Rebuild if the icon or the manifest changes.
    println!("cargo:rerun-if-changed=icon.ico");
    println!("cargo:rerun-if-changed=tinymon.manifest");
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("icon.ico");
        res.set("CompanyName", "mthwSec");
        res.set("ProductName", "TinyMon");
        res.set("FileDescription", "Tiny always-on-top Windows resource monitor");
        res.set("LegalCopyright", "Copyright (C) 2026 mthwSec");
        res.set("OriginalFilename", "tinymon.exe");
        res.set("Comments", "https://github.com/mthwSec/TinyMon");
        // FileVersion / ProductVersion are taken from CARGO_PKG_VERSION by winres.
        // RT_MANIFEST #1: asInvoker, the Windows 10/11 supportedOS GUID and
        // Per-Monitor-V2 DPI awareness. Left out on purpose: assemblyIdentity (a
        // malformed one stops the exe from starting, and its version would drift
        // from Cargo.toml), activeCodePage (no ANSI APIs are used) and the Common
        // Controls 6.0 dependency (the settings window is owner-drawn for
        // comctl32 5.82). A file, because winres' inline form mangles XML lines.
        res.set_manifest_file("tinymon.manifest");
        if let Err(e) = res.compile() {
            // Don't fail the build over branding — just flag it.
            println!("cargo:warning=icon/version-info/manifest embedding skipped: {e}");
        }
    }
}
