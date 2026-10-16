# Build the TinyMon release exe the way releases are built, then check it.
#
#   powershell -ExecutionPolicy Bypass -File scripts\build-release.ps1 ...
#
#   scripts\build-release.ps1                      build + check (target\release)
#   scripts\build-release.ps1 -TargetDir D:\t      build into another target dir
#   scripts\build-release.ps1 -NoDefaultFeatures   the build without Temperature
#   scripts\build-release.ps1 -CheckOnly x.exe     check an existing exe only
#   scripts\build-release.ps1 -InfoFile b.txt      also write the report to a file
#
# The build hides the cargo home (whose path holds the user name) behind
# /cargo with --remap-path-prefix; build.rs adds everything else (resources,
# the hybrid C runtime, /Brepro). The checks fail the script when the exe
# imports the Visual C++ runtime, holds a user or build path, lacks its icon,
# manifest or version resource, carries another version than Cargo.toml, or
# is larger than -MaxSize. The report names the tools (rustc, the MSVC
# toolset, the Windows SDK) that a rebuild needs to give the same bytes.
# Exit code 0: built and every check passed.
param(
    [string]$CheckOnly,
    [string]$TargetDir,
    [switch]$NoDefaultFeatures,
    [int]$MaxSize = 286720,
    [string]$InfoFile
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$report = New-Object System.Collections.Generic.List[string]
$msvc = $null
$linker = $null

# Paths from the command line, made absolute against the current location
# (cargo runs in the repository, .NET in the process's start folder).
function Get-FullPath([string]$p) { $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($p) }
if ($CheckOnly) { $CheckOnly = Get-FullPath $CheckOnly }
if ($TargetDir) { $TargetDir = Get-FullPath $TargetDir }
if ($InfoFile) { $InfoFile = Get-FullPath $InfoFile }

# Print a report line and keep it for -InfoFile.
function Say([string]$line) {
    Write-Host $line
    $report.Add($line)
}

# The resource compiler the build uses: RC if set, else rc.exe on PATH, else
# the newest one in the Windows 10 SDK (the order build.rs follows). The
# build gets it through RC, so the SDK reported is the one used.
function Select-Rc {
    if ($env:RC) {
        Say 'sdk:      the RC variable names the resource compiler'
        return
    }
    $onPath = Get-Command rc.exe -ErrorAction SilentlyContinue
    if ($onPath) {
        $rc = $onPath.Source
    } else {
        $root = $null
        foreach ($key in 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows Kits\Installed Roots',
                         'HKLM:\SOFTWARE\Microsoft\Windows Kits\Installed Roots') {
            if (-not $root) { $root = (Get-ItemProperty -LiteralPath $key -ErrorAction SilentlyContinue).KitsRoot10 }
        }
        if (-not $root) { $root = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10' }
        $dir = Get-ChildItem -LiteralPath (Join-Path $root 'bin') -Directory -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match '^\d+(\.\d+)+$' -and (Test-Path -LiteralPath (Join-Path $_.FullName 'x64\rc.exe')) } |
            Sort-Object { [version]$_.Name } | Select-Object -Last 1
        if (-not $dir) { throw 'rc.exe not found: install a Windows 10/11 SDK or set RC' }
        $rc = Join-Path $dir.FullName 'x64\rc.exe'
    }
    $env:RC = $rc
    $sdk = if ($rc -match '\\bin\\(\d+(\.\d+)+)\\') { $Matches[1] } else { 'unknown' }
    Say "sdk:      $sdk"
}

# The MSVC toolset: a developer prompt's, else the newest Visual Studio's (vswhere).
function Get-Msvc {
    if ($env:VCToolsVersion) { return $env:VCToolsVersion.TrimEnd('\') }
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path -LiteralPath $vswhere)) { return $null }
    $vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $vs) { return $null }
    $txt = Join-Path $vs 'VC\Auxiliary\Build\Microsoft.VCToolsVersion.default.txt'
    if (Test-Path -LiteralPath $txt) { return (Get-Content -LiteralPath $txt -TotalCount 1).Trim() }
    $null
}

function Get-Cargo {
    $c = Get-Command cargo -ErrorAction SilentlyContinue
    if ($c) { return $c.Source }
    $home2 = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
    $p = Join-Path $home2 'bin\cargo.exe'
    if (Test-Path -LiteralPath $p) { return $p }
    throw 'cargo not found (neither on PATH nor in the cargo home)'
}

# ---- PE reading ---------------------------------------------------------------

function U16([byte[]]$b, [int]$o) { [BitConverter]::ToUInt16($b, $o) }
function U32([byte[]]$b, [int]$o) { [BitConverter]::ToUInt32($b, $o) }

# The sections and data directories of a PE32+ image.
function Read-Pe([byte[]]$b) {
    if ((U16 $b 0) -ne 0x5A4D) { throw 'not an MZ image' }
    $pe = [int](U32 $b 0x3C)
    if ((U32 $b $pe) -ne 0x4550) { throw 'no PE signature' }
    $nsec = U16 $b ($pe + 6)
    $optSize = U16 $b ($pe + 20)
    $opt = $pe + 24
    if ((U16 $b $opt) -ne 0x20B) { throw 'not a PE32+ image' }
    $dirs = @()
    for ($i = 0; $i -lt 16; $i++) {
        $dirs += , @((U32 $b ($opt + 112 + 8 * $i)), (U32 $b ($opt + 116 + 8 * $i)))
    }
    $secs = @()
    $s0 = $opt + $optSize
    for ($i = 0; $i -lt $nsec; $i++) {
        $o = $s0 + 40 * $i
        $secs += [pscustomobject]@{
            Name = [Text.Encoding]::ASCII.GetString($b, $o, 8).TrimEnd([char]0)
            VSize = U32 $b ($o + 8); VA = U32 $b ($o + 12)
            RawSize = U32 $b ($o + 16); Raw = U32 $b ($o + 20)
        }
    }
    [pscustomobject]@{ Bytes = $b; Dirs = $dirs; Sections = $secs
        LinkerVersion = '{0}.{1}' -f $b[$opt + 2], $b[$opt + 3] }
}

# The file offset of an RVA.
function Get-Offset($pe, [uint32]$rva) {
    foreach ($s in $pe.Sections) {
        $span = [Math]::Max($s.VSize, $s.RawSize)
        if ($rva -ge $s.VA -and $rva -lt $s.VA + $span) { return [int]($rva - $s.VA + $s.Raw) }
    }
    throw ('RVA 0x{0:X} is in no section' -f $rva)
}

function Read-AsciiZ([byte[]]$b, [int]$o) {
    $e = $o
    while ($b[$e] -ne 0) { $e++ }
    [Text.Encoding]::ASCII.GetString($b, $o, $e - $o)
}

# The DLL names of the import and delay-import tables.
function Get-Imports($pe) {
    $b = $pe.Bytes
    $names = @()
    $imp = $pe.Dirs[1]
    if ($imp[0] -ne 0) {
        $o = Get-Offset $pe $imp[0]
        while ((U32 $b ($o + 12)) -ne 0) {
            $names += Read-AsciiZ $b (Get-Offset $pe (U32 $b ($o + 12)))
            $o += 20
        }
    }
    $dly = $pe.Dirs[13]
    if ($dly[0] -ne 0) {
        $o = Get-Offset $pe $dly[0]
        while ((U32 $b ($o + 4)) -ne 0) {
            $names += Read-AsciiZ $b (Get-Offset $pe (U32 $b ($o + 4)))
            $o += 32
        }
    }
    $names
}

# The resource tree: type id -> list of name ids, and the version resource bytes.
function Get-Resources($pe) {
    $b = $pe.Bytes
    $rsrc = $pe.Dirs[2]
    $types = @{}
    $version = $null
    if ($rsrc[0] -eq 0) { return [pscustomobject]@{ Types = $types; Version = $version } }
    $base = Get-Offset $pe $rsrc[0]
    function Entries([int]$dir) {
        $n = (U16 $b ($dir + 12)) + (U16 $b ($dir + 14))
        for ($i = 0; $i -lt $n; $i++) {
            $e = $dir + 16 + 8 * $i
            [pscustomobject]@{ Id = U32 $b $e; Off = U32 $b ($e + 4) }
        }
    }
    foreach ($t in (Entries $base)) {
        if ($t.Id -band 0x80000000) { continue }  # named types: none expected
        $ids = @()
        $sub = $base + ($t.Off -band 0x7FFFFFFF)
        foreach ($nm in (Entries $sub)) {
            $ids += $nm.Id
            if ($t.Id -eq 16 -and $null -eq $version) {
                $lang = (Entries ($base + ($nm.Off -band 0x7FFFFFFF)))[0]
                $de = $base + $lang.Off
                $o = Get-Offset $pe (U32 $b $de)
                $version = $b[$o..($o + (U32 $b ($de + 4)) - 1)]
            }
        }
        $types[[int]$t.Id] = $ids
    }
    [pscustomobject]@{ Types = $types; Version = $version }
}

# ---- checks -------------------------------------------------------------------

function Test-Exe([string]$exe) {
    $exe = (Resolve-Path -LiteralPath $exe).Path
    $b = [IO.File]::ReadAllBytes($exe)
    $pe = Read-Pe $b
    $fail = @()

    $imports = Get-Imports $pe
    $crt = @($imports | Where-Object { $_ -match '^(vcruntime|msvcp|msvcr|concrt|vcomp|ucrtbased)' })
    if ($crt.Count) { $fail += 'imports the Visual C++ runtime: ' + ($crt -join ', ') }

    # Paths, in one-byte text and in UTF-16 at both byte parities. A drive path
    # needs three more path characters, so that random bytes cannot match. The
    # user name counts only between path separators ("User" is also a word in
    # std's messages), the machine name only as a whole word.
    $latin = [Text.Encoding]::GetEncoding(28591).GetString($b)
    $u16a = [Text.Encoding]::Unicode.GetString($b, 0, $b.Length - ($b.Length % 2))
    $u16b = [Text.Encoding]::Unicode.GetString($b, 1, ($b.Length - 1) - (($b.Length - 1) % 2))
    $patterns = [ordered]@{
        'a drive-letter path' = '(?<![A-Za-z0-9])[A-Za-z]:[\\/](?![\\/])[\x21-\x7E]{3}'
        'a \Users\ path' = '[\\/]Users[\\/]'
        'runneradmin' = 'runneradmin'
        'the cargo registry path' = '\.cargo[\\/]registry'
    }
    if ($env:USERNAME) {
        $patterns['the user name in a path'] = '(?<=[\\/])' + [regex]::Escape($env:USERNAME) + '(?=[\\/])'
    }
    if ($env:COMPUTERNAME -and $env:COMPUTERNAME.Length -ge 6) {
        $patterns['the machine name'] = '(?<![A-Za-z0-9-])' + [regex]::Escape($env:COMPUTERNAME) + '(?![A-Za-z0-9-])'
    }
    foreach ($k in $patterns.Keys) {
        foreach ($text in @($latin, $u16a, $u16b)) {
            $m = [regex]::Match($text, $patterns[$k], 'IgnoreCase')
            if ($m.Success) {
                $ctx = $text.Substring([Math]::Max(0, $m.Index - 8), [Math]::Min(72, $text.Length - [Math]::Max(0, $m.Index - 8)))
                $fail += ('holds {0}: "{1}"' -f $k, ($ctx -replace '[^\x20-\x7E]', '.'))
                break
            }
        }
    }

    $res = Get-Resources $pe
    foreach ($t in @(@(3, 'an icon image'), @(14, 'an icon group'), @(16, 'a version resource'), @(24, 'a manifest'))) {
        if (-not $res.Types.ContainsKey($t[0])) { $fail += 'lacks ' + $t[1] }
    }
    if ($res.Types.ContainsKey(24) -and -not ($res.Types[24] -contains 1)) { $fail += 'its manifest is not resource 1' }
    if ($res.Types.ContainsKey(14) -and -not ($res.Types[14] -contains 1)) { $fail += 'its icon group is not resource 1' }

    $want = (Select-String -LiteralPath (Join-Path $repo 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' |
        Select-Object -First 1).Matches[0].Groups[1].Value
    $fileVersion = $null
    if ($res.Version) {
        $vtext = [Text.Encoding]::Unicode.GetString([byte[]]$res.Version)
        $vm = [regex]::Match($vtext, 'FileVersion\x00+([^\x00]+)')
        if ($vm.Success) { $fileVersion = $vm.Groups[1].Value }
    }
    if ($fileVersion -ne $want) { $fail += "FileVersion is '$fileVersion', Cargo.toml says '$want'" }

    $size = $b.Length
    if ($size -gt $MaxSize) { $fail += "is $size bytes, over the $MaxSize-byte stop" }

    $sha = (Get-FileHash -Algorithm SHA256 -LiteralPath $exe).Hash.ToLower()
    Write-Host "path:     $exe"
    Say "exe:      $(Split-Path -Leaf $exe)"
    Say "size:     $size bytes"
    Say "sha256:   $sha"
    Say ("imports:  " + (($imports | Sort-Object -Unique) -join ', '))
    Say "version:  $fileVersion; linker $($pe.LinkerVersion)"
    $script:linker = $pe.LinkerVersion
    if ($fail.Count) {
        foreach ($f in $fail) { Say "FAIL: the exe $f" }
        return $false
    }
    Say 'PASS: every check passed'
    return $true
}

# ---- main ---------------------------------------------------------------------

function Write-Info {
    if ($InfoFile) { [IO.File]::WriteAllText($InfoFile, (($report -join "`n") + "`n")) }
}

if ($CheckOnly) {
    $ok = Test-Exe $CheckOnly
    Write-Info
    exit $(if ($ok) { 0 } else { 1 })
}

$cargo = Get-Cargo
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$td = if ($TargetDir) { $TargetDir } else { Join-Path $repo 'target' }
$args2 = @('build', '--release', '--locked', '--target-dir', $td, '--message-format=json-render-diagnostics')
if ($NoDefaultFeatures) { $args2 += '--no-default-features' }
if ($env:RUSTFLAGS) { Write-Host 'note: RUSTFLAGS is set; the release build ignores it' }
$savedFlags = $env:CARGO_ENCODED_RUSTFLAGS
$savedRc = $env:RC
$env:CARGO_ENCODED_RUSTFLAGS = "--remap-path-prefix=$cargoHome=/cargo"
$exe = $null
Push-Location $repo
try {
    Say ("rustc:    " + (& (Join-Path (Split-Path $cargo) 'rustc.exe') -V))
    $msvc = Get-Msvc
    if ($msvc) { Say "msvc:     $msvc" }
    Select-Rc
    # Diagnostics go to the console; stdout carries cargo's JSON messages,
    # whose compiler-artifact line names the exe that was built.
    $messages = & $cargo @args2
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" }
    foreach ($line in $messages) {
        if ($line -like '{"reason":"compiler-artifact"*') {
            $m = $line | ConvertFrom-Json
            if ($m.executable -and $m.target.name -eq 'tinymon') { $exe = $m.executable }
        }
    }
} finally {
    Pop-Location
    $env:CARGO_ENCODED_RUSTFLAGS = $savedFlags
    $env:RC = $savedRc
}
if (-not $exe) { throw 'cargo reported no tinymon executable' }
$ok = Test-Exe $exe
if ($msvc -and $linker -and -not $msvc.StartsWith("$linker.")) {
    Say "note: the exe's linker is $linker, not the reported toolset $msvc"
}
Write-Info
exit $(if ($ok) { 0 } else { 1 })
