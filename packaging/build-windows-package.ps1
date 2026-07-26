#!/usr/bin/env pwsh
# Build a Windows package for ft991a-radio-control.
# Usage: pwsh ./packaging/build-windows-package.ps1
#
# Contract (radio-cat-rs ADR 0008 section 3, "the consuming-repo contract"):
# invoked via pwsh with NO arguments, after `cargo build --release` has
# already produced target/release/ft991a.exe (and, if wanted, pin-test.exe
# via `cargo build --release -p cat-transport-serial --bin pin-test` - see
# packaging/build-deb.sh's identical note on why that binary is built with
# an explicit -p flag rather than living in this repo's own workspace).
# Must produce one or more package files directly in the repo root -
# satisfied here with a single .zip, matching the shared release
# workflow's default `windows_package_glob: "*.zip"`.
#
# NOT executable in this sandbox (no pwsh/PowerShell available in this
# Linux dev environment) - written and reviewed by hand against ADR 0008's
# contract and this repo's own packaging/build-deb.sh precedent, not
# verified by an actual run. See this repo's final task report for the
# explicit callout.

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = (Resolve-Path (Join-Path $ScriptDir "..")).Path

# Version, read the same way packaging/build-deb.sh's `sed` extraction
# does, from the root Cargo.toml's `[workspace.package]` version line.
$CargoTomlPath = Join-Path $RepoRoot "Cargo.toml"
$VersionLine = Get-Content $CargoTomlPath | Where-Object { $_ -match '^version\s*=\s*"' } | Select-Object -First 1
if ($null -eq $VersionLine -or -not ($VersionLine -match '"([^"]+)"')) {
    throw "Could not determine version from $CargoTomlPath"
}
$Version = $Matches[1]

$Release = Join-Path $RepoRoot "target/release"
$PackageName = "ft991a-radio-control-$Version-windows-x86_64"
$StageDir = Join-Path $RepoRoot "target/windows-package/$PackageName"

Write-Host "==> Staging into $StageDir"
if (Test-Path $StageDir) {
    Remove-Item -Recurse -Force $StageDir
}
New-Item -ItemType Directory -Path $StageDir | Out-Null

# --- Main binary (required) ---
$MainExe = Join-Path $Release "ft991a.exe"
if (-not (Test-Path $MainExe)) {
    throw "Expected $MainExe to exist -- run 'cargo build --release' (or 'cargo build --release -p ft991a') first"
}
Copy-Item $MainExe (Join-Path $StageDir "ft991a.exe")

# --- pin-test.exe (optional -- not a workspace member of this repo; it
# lives in radio-cat-rs's cat-transport-serial crate, per ADR 0006 section
# 6, and is built via `cargo build --release -p cat-transport-serial --bin
# pin-test`, landing in the same target/release directory). Warn rather
# than fail if the caller didn't build it -- the main ft991a.exe is the
# only strictly required artifact for this contract. ---
$PinTestExe = Join-Path $Release "pin-test.exe"
if (Test-Path $PinTestExe) {
    Copy-Item $PinTestExe (Join-Path $StageDir "pin-test.exe")
} else {
    Write-Warning "pin-test.exe not found at $PinTestExe -- packaging without it. Run 'cargo build --release -p cat-transport-serial --bin pin-test' first to include it."
}

# --- Docs ---
Copy-Item (Join-Path $RepoRoot "README.md") (Join-Path $StageDir "README.md")
Copy-Item (Join-Path $RepoRoot "LICENSE.txt") (Join-Path $StageDir "LICENSE.txt")

# --- Zip, directly in the repo root (the contract's requirement) ---
$ZipPath = Join-Path $RepoRoot "$PackageName.zip"
if (Test-Path $ZipPath) {
    Remove-Item -Force $ZipPath
}

Write-Host "==> Compress-Archive -> $ZipPath"
Compress-Archive -Path (Join-Path $StageDir "*") -DestinationPath $ZipPath

Write-Host ""
Write-Host "Package built: $ZipPath"
Write-Host ""
Write-Host "Contents:"
Get-ChildItem $StageDir | ForEach-Object { Write-Host "  $($_.Name)" }
