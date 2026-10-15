# SP2: a truly portable exe (design)

Status: approved by the owner on 2026-10-15. Branch `sp2-portable`, starting from `main` at 01ab8c1.

## 1. Goals

1. No Visual C++ redistributable. The release exe imports no `VCRUNTIME140.dll`. The UCRT (`api-ms-win-crt-*`) ships with Windows 10 and stays dynamic.
2. Nothing personal in the exe: no user name, no absolute build path, no machine name.
3. Reproducible builds. The same source with the same rustc, MSVC toolset and Windows SDK gives the same bytes, whatever the user name, checkout path, `CARGO_HOME`, target dir or `TEMP`.
4. Release builds made by GitHub Actions. SP2 writes the workflows and validates them locally; their first run is part of SP4, which publishes the repo.

Also in SP2: the license files, and the OS-build guard for the undocumented dark-mode calls.

Not in SP2:
- publishing the `--no-default-features` exe;
- `/DEBUG:NONE` (the `.pdb` is kept privately and is never published);
- code signing (releases ship unsigned at first);
- the minor audit items in the follow-up inbox, section I.

## 2. Measurements behind the design (2026-10-13, main 01ab8c1, rustc 1.96.0, MSVC 14.51.36231, SDK 10.0.26100)

- The exe holds exactly one user-identifying string: the windows crate's registry path, `C:\Users\<user>\.cargo\registry\src\...\Gdi\mod.rs`. It is the panic location of the crate's slice-taking GDI wrappers. `--remap-path-prefix=<CARGO_HOME>=/cargo` removes it at 0 B. Cargo's `trim-paths` is unstable in 1.96.
- Build-to-build differences come from three sources:
  - the link timestamp, which appears in 4 places;
  - a random CodeView GUID;
  - winres 0.1.12 writing the VERSIONINFO strings from a HashMap, so in random order.
- With `/Brepro`, a fixed-order `.rc` and the remap, two builds from different directories, users and `CARGO_HOME`s are bit-identical. This holds in both feature sets and with the hybrid CRT.
- C runtime variants:

  | Variant | Size |
  |---|---|
  | dynamic | 258,560 B |
  | hybrid: static vcruntime/libcmt, dynamic UCRT | 280,064 B (+21,504) |
  | full static | 360,448 B |

  The reverse hybrid gives LNK4098 on every link. `no_std` is not realistic for this codebase.
- Changing the toolset from MSVC 14.51 to 14.41 changed 37,178 bytes. Changing the SDK from 26100 to 19041 changed 424 bytes. So byte identity needs the same toolset and SDK.
- `target\release\tinymon.pdb` holds the user name even with the remap. It is never published.

## 3. Design

### 3.1 Build rules live in `build.rs` (approach A)

`build.rs` owns every build rule that does not depend on the machine:

- **Resources.**
  - A checked-in `tinymon.rc` lists the icon, the manifest (`RT_MANIFEST` 1) and a VERSIONINFO block whose string values are written in a fixed order.
  - `build.rs` writes `OUT_DIR/version.h` from `CARGO_PKG_VERSION_*`, finds `rc.exe`, compiles the `.rc` to a `.res` and passes it to the linker.
  - It looks for `rc.exe` in this order: the `RC` environment variable; then `rc.exe` on `PATH`; then the Windows 10 Kits root (registry `KitsRoot10`), taking the highest `bin\<version>\x64\rc.exe`.
  - The winres build dependency goes, and with it 5 crates leave `Cargo.lock`, which then holds 16 packages. No dependency is added.
- **Hybrid CRT.** For the MSVC target only, `cargo:rustc-link-arg-bins` passes `/NODEFAULTLIB:msvcrt.lib /DEFAULTLIB:libcmt.lib /NODEFAULTLIB:libucrt.lib /DEFAULTLIB:ucrt.lib`. The measured exe is identical to the documented `+crt-static` recipe's. Putting it in `build.rs` means an environment `RUSTFLAGS` cannot silently drop it. It covers the unit-test exe too.
- **`/Brepro`.** For the MSVC target only, `/Brepro` is passed the same way. The PE timestamp then becomes a content hash.
- **Toolchain pin.** `rust-toolchain.toml` pins `channel = "1.96.0"` with the `rustfmt` and `clippy` components and `profile = "minimal"`.

Rejected alternatives:
- `.cargo/config.toml` with `+crt-static`. The release build sets an environment `RUSTFLAGS` (or `CARGO_ENCODED_RUSTFLAGS`) for the remap, and that replaces config `rustflags`, so `VCRUNTIME140` would silently come back.
- Doing everything in a script. A plain `cargo build --release` would then differ from the release exe.

### 3.2 Privacy and reproducibility: `scripts/build-release.ps1`

This script is the one way to make a release exe on a PC, and CI runs the same checks.

1. It sets `CARGO_ENCODED_RUSTFLAGS` to `--remap-path-prefix=<CARGO_HOME>=/cargo`. `CARGO_HOME` falls back to `%USERPROFILE%\.cargo`.
2. It runs `cargo build --release --locked`. It accepts an optional target dir and feature switches.
3. It checks the built exe and fails on any violation:
   - no import of `VCRUNTIME*.dll` or any `MSVCP*.dll`;
   - no drive-letter path, no `\Users\`, no `runneradmin`, and no `.cargo\registry` outside `/cargo`, checked in ASCII and UTF-16;
   - an icon, `RT_MANIFEST` 1 and `RT_VERSION` are present, and the version resource's FileVersion matches `Cargo.toml`;
   - the size is at or below the stop.
4. It prints the SHA-256, the size and the tool versions: rustc, link.exe, VCToolsVersion and WindowsSDKVersion.

It has a `-CheckOnly <exe>` mode that runs only the checks. The PE parsing is done in PowerShell, so the checks need no `dumpbin`.

What is promised: the same source with the same rustc, MSVC toolset and SDK gives the same bytes. The GitHub build is the official one, and each release records its tool versions. A match between the owner's PC and CI is not promised.

### 3.3 GitHub Actions (written and checked now; first run in SP4)

- **`.github/workflows/ci.yml`** runs on push and pull request, on `windows-2025`. It runs `cargo fmt --check`, then `clippy -D warnings` and `test` in both feature sets. Swatinem/rust-cache saves the cache only on `main`.
- **`.github/workflows/release.yml`** runs on a `v*` tag, or on `workflow_dispatch` as a dry run that publishes nothing. Its jobs:
  1. A build job runs `scripts/build-release.ps1`.
  2. A reproducibility job builds again from a different checkout path and target dir, and publishing is refused unless the SHA-256 is equal.
  3. A packaging step makes a zip of the exe, `README.md` and the two license files, with fixed file times, plus `SHA256SUMS.txt`.
  4. Provenance is added with `actions/attest`, only when the repo is public.
  5. The release is created with `gh release create --verify-tag`.
- **Hygiene:**
  - `runs-on: windows-2025`;
  - every action pinned by its full commit SHA;
  - top-level `permissions: {}` with a grant per job;
  - `persist-credentials: false`;
  - no secrets;
  - the toolchain installed with plain `rustup` from `rust-toolchain.toml`.
- **Local validation:** `actionlint`, plus the release script, which mirrors the release job's build and checks. `act` is never used.

### 3.4 Dark-mode guard (`src/dark.rs`)

The undocumented uxtheme ordinals have changed meaning across Windows versions, so each is called only where its meaning is known:

| Ordinal | Called from build | As |
|---|---|---|
| 104, 133 | 17763 (1809) on | (as today) |
| 135 | 17763 to 18361 | `AllowDarkModeForApp(true)` |
| 135 | 18362 (1903) on | `SetPreferredAppMode` |

On older builds no uxtheme ordinal is called and Settings stays light. The documented `DwmSetWindowAttribute` call is unchanged.

- The real build number is read with `RtlGetVersion` from ntdll, which is not subject to version lies, and cached.
- A pure function maps the build number to the allowed calls, and unit tests pin it.

### 3.5 Licenses and docs

- Add `LICENSE-MIT` and `LICENSE-APACHE` (standard texts). The MIT notice reads "Copyright (c) 2026 mthwSec".
- Update the README:
  - no Visual C++ redistributable is needed;
  - the new sizes;
  - the test counts;
  - how to build a release (`scripts/build-release.ps1`) and how to verify one (checksums, rebuilding with the recorded tools);
  - the unsigned-exe SmartScreen note ("More info", then "Run anyway");
  - the license section.

## 4. Size

The baseline is 258,560 B. The expected size is about 280,064 B, plus a few hundred bytes for the guard. The hard stop is **282,624 B**: if any commit's release exe is larger, stop and re-plan.

## 5. Process and acceptance

- Every change is made test-first where code is involved.
- The gates run after every commit: fmt, clippy `-D warnings` and test in both feature sets, and a release build with its size measured.
- Commits carry no trailers. Work stays on `sp2-portable`, and it is merged only with the owner's OK.
- Acceptance:
  - `scripts/build-release.ps1` passes all its checks;
  - two release builds from different directories are bit-identical;
  - the exe imports no VCRUNTIME;
  - `actionlint` reports no errors;
  - the tests pass in both feature sets.
- At the end the owner's TinyMon is rebuilt with the release script and restarted with the safe swap:
  1. Exit it through its own Exit.
  2. Back up the cfg and the exe.
  3. Build.
  4. Start it with `explorer.exe`.
  5. Verify the result, including the registry values.
- Risk: Defender may flag a new binary layout as a false positive. That is observed, not predicted. Code signing is the only real fix, and it comes after SP4.

## 6. As built (2026-10-15)

- Commits on `sp2-portable`, after this document:
  1. the toolchain pin;
  2. the release script;
  3. `tinymon.rc` in place of winres;
  4. the hybrid CRT with `/Brepro`;
  5. the dark-mode guard;
  6. the license files;
  7. the tool versions in the release report;
  8. the workflows;
  9. the docs.
- Release exe: 280,576 B (278,016 B without default features). That is +22,016 B over 258,560 B, and 2,048 B under the stop. The hybrid CRT accounts for 21,504 B and the dark-mode guard for 512 B.
- Tests: 232 passed with 2 ignored, and 220 with 2 without default features. `Cargo.lock` holds 16 packages.
- Reproducibility: the release exe was built from three source paths: the repository, a second folder, and a deeper path containing spaces. All three gave the same SHA-256. `scripts/build-release.ps1` passes every check in both feature sets. Two packaging runs gave the same zip.
- Additions within the design:
  - `dpi::tests::exe_embeds_minimal_manifest` now requires the version resource instead of skipping, so a build without `rc.exe` fails the tests.
  - `scripts/build-release.ps1 -InfoFile` writes the report, which the release publishes as `BUILDINFO.txt` (rustc, MSVC toolset, SDK). It names the exe by its file name only, so no build path reaches it.
  - The release zip is written entry by entry: forward-slash paths, a fixed order, and the commit time on every entry.
- The workflows pass actionlint 1.7.12 with no error. Their first run comes with SP4.
