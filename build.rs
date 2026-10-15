//! Build script: compile `tinymon.rc` (the application icon, the Windows
//! version information and the application manifest `tinymon.manifest`) with
//! the Windows SDK resource compiler and link the result into the exe. The
//! version numbers come from Cargo.toml through `OUT_DIR/version.h`. Runs only
//! at build time and uses std only, so it adds nothing to the dependency set.
//! Without the resource compiler the build still succeeds, with a warning:
//! the exe then lacks these resources, which the test
//! `dpi::tests::exe_embeds_minimal_manifest` and `scripts/build-release.ps1`
//! report.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    for file in ["build.rs", "tinymon.rc", "icon.ico", "tinymon.manifest", "Cargo.toml"] {
        println!("cargo:rerun-if-changed={file}");
    }
    println!("cargo:rerun-if-env-changed=RC");
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    embed_resources();
}

/// Write `version.h`, compile `tinymon.rc` to `tinymon.res` in OUT_DIR and
/// hand the `.res` to the linker of the exe (and of its unit-test exe).
fn embed_resources() {
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set"));
    let var = |k: &str| env::var(k).expect("cargo sets the package version");
    let header = format!(
        "#define TM_VER_MAJOR {}\n#define TM_VER_MINOR {}\n#define TM_VER_PATCH {}\n\
         #define TM_VER_STR \"{}\"\n",
        var("CARGO_PKG_VERSION_MAJOR"),
        var("CARGO_PKG_VERSION_MINOR"),
        var("CARGO_PKG_VERSION_PATCH"),
        var("CARGO_PKG_VERSION"),
    );
    fs::write(out.join("version.h"), header).expect("write version.h");

    let Some(rc) = find_rc() else {
        println!(
            "cargo:warning=rc.exe not found (set RC or install the Windows SDK): \
             the exe is built without its icon, version information and manifest"
        );
        return;
    };
    let res = out.join("tinymon.res");
    let status = Command::new(&rc)
        .current_dir(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set"))
        .arg("/nologo")
        .arg("/I")
        .arg(&out)
        .arg("/fo")
        .arg(&res)
        .arg("tinymon.rc")
        .status()
        .unwrap_or_else(|e| panic!("cannot run {}: {e}", rc.display()));
    assert!(status.success(), "{} failed on tinymon.rc ({status})", rc.display());
    println!("cargo:rustc-link-arg-bins={}", res.display());
}

/// The resource compiler: the RC environment variable, else rc.exe on PATH (a
/// developer prompt), else the newest one in the Windows 10 SDK's bin folder
/// for this host.
fn find_rc() -> Option<PathBuf> {
    if let Some(rc) = env::var_os("RC") {
        return Some(PathBuf::from(rc));
    }
    if let Some(path) = env::var_os("PATH") {
        if let Some(rc) = env::split_paths(&path).map(|d| d.join("rc.exe")).find(|p| p.is_file()) {
            return Some(rc);
        }
    }
    let arch = match env::var("HOST").unwrap_or_default().split('-').next() {
        Some("aarch64") => "arm64",
        Some("i686") => "x86",
        _ => "x64",
    };
    let mut best: Option<(Vec<u32>, PathBuf)> = None;
    for entry in fs::read_dir(kits_root()?.join("bin")).ok()?.flatten() {
        // Version folders only ("10.0.26100.0"); "x64", "arm64" etc. do not parse.
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(version) = name.split('.').map(|p| p.parse().ok()).collect::<Option<Vec<u32>>>()
        else {
            continue;
        };
        let rc = entry.path().join(arch).join("rc.exe");
        if rc.is_file() && best.as_ref().is_none_or(|(v, _)| version > *v) {
            best = Some((version, rc));
        }
    }
    best.map(|(_, rc)| rc)
}

/// The Windows 10 SDK root (`KitsRoot10`), read with reg.exe so that the build
/// script needs no crate; the default install folder if the key is missing.
fn kits_root() -> Option<PathBuf> {
    let key = r"HKLM\SOFTWARE\Microsoft\Windows Kits\Installed Roots";
    let found = Command::new("reg")
        .args(["query", key, "/v", "KitsRoot10", "/reg:32"])
        .output()
        .ok()
        .and_then(|o| {
            let text = String::from_utf8_lossy(&o.stdout).into_owned();
            text.lines()
                .find(|l| l.contains("KitsRoot10"))
                .and_then(|l| l.split("REG_SZ").nth(1))
                .map(|p| PathBuf::from(p.trim()))
        });
    let default = PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10");
    found.filter(|p| p.is_dir()).or_else(|| default.is_dir().then_some(default))
}
