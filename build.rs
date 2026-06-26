//! Build script: embed the application icon + Windows version-info (file details)
//! into the `.exe`. Runs only at build time — adds nothing to the runtime
//! dependency set. Best-effort: if the SDK resource compiler is unavailable the
//! build still succeeds (the binary just ships without the icon/details).

fn main() {
    // Rebuild if the icon changes.
    println!("cargo:rerun-if-changed=icon.ico");
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
        if let Err(e) = res.compile() {
            // Don't fail the build over branding — just flag it.
            println!("cargo:warning=icon/version-info embedding skipped: {e}");
        }
    }
}
