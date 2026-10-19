<#
.SYNOPSIS
  Packs PerMark for the Microsoft Store: builds the Store flavor and writes an unsigned PerMark_<version>_x64.msix.

.DESCRIPTION
  The Store flavor is the same program built with `--no-default-features --features store` and the overlay
  `app/src-tauri/tauri.msstore.conf.json` (no updater, no registry writer; the agent setup lines name the `permark` alias). This script:

    1. builds it, and the shell extension next to it (with -Build; otherwise it packs what is already built),
    2. lays out a package folder: permark.exe, permark_shell.dll (the right-click menu of folders, app/src-tauri/crates/shell-ext),
       the logos (app/src-tauri/msstore/Assets) and AppxManifest.xml, which it writes from the template
       app/src-tauri/msstore/AppxManifest.xml with the version filled in,
    3. runs MakeAppx.exe from the Windows SDK, which checks the manifest against the package schema and packs the folder.

  The package is not signed: the Store signs what is uploaded to it, and an unsigned package cannot be installed by hand.
  Nothing is installed or registered by this script.

  The package version is the app's version (tauri.conf.json) and a fourth part, 0, which the Store reserves: 0.4.0 becomes
  0.4.0.0. The app's version must be three plain numbers (no -beta suffix); the Store does not take one.

  Everything the script needs is in the checkout, so it also runs on a checkout of any commit that has it (a release tag
  from before the packaging existed needs the packaging files copied in first; docs/msstore.md says how).

.PARAMETER Build
  Build first: the Store flavor, `tauri build --no-bundle --features store --config <overlay> -- --no-default-features`
  (needs the frontend dependencies, npm ci, Rust and the usual Tauri prerequisites), and the shell extension,
  `cargo build --profile release-shell -p permark-shell` (a profile that unwinds on a panic).

.PARAMETER ExePath
  The built program. Default: app\src-tauri\target\release\permark.exe.

.PARAMETER DllPath
  The built shell extension. Default: app\src-tauri\target\release-shell\permark_shell.dll (workspace target).

.PARAMETER OutDir
  Where the package folder and the .msix go. Default: app\src-tauri\target\msix.

.PARAMETER Version
  The app's version, three parts. Default: the one in tauri.conf.json.

.PARAMETER MakeAppx
  The path of MakeAppx.exe. Default: the newest x64 one in the Windows SDK (C:\Program Files (x86)\Windows Kits\10\bin).

.PARAMETER Sideload
  Writes a package for trying on a computer without the Store (it is never what is uploaded): it has an identity of its own
  (EdelmarSchneider.PerMarkSideload, publisher CN=PerMark Sideload, the alias permark-sideload), so it installs beside
  the Store version and can be removed without touching it, and a test certificate whose subject is that publisher can sign
  it. The file is named PerMark_<version>_x64_sideload.msix. docs/msstore.md has the steps.

.EXAMPLE
  .\scripts\build-msix.ps1 -Build

.EXAMPLE
  .\scripts\build-msix.ps1 -Build -Sideload -Version 0.5.1

.OUTPUTS
  The path of the .msix.
#>
[CmdletBinding()]
param(
  [switch]$Build,
  [string]$ExePath,
  [string]$DllPath,
  [string]$OutDir,
  [string]$Version,
  [string]$MakeAppx,
  [switch]$Sideload
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Repo root = parent of scripts/; the program lives in app/.
$Root = Split-Path -Parent $PSScriptRoot
$App = Join-Path $Root 'app'
$Tauri = Join-Path $App 'src-tauri'
$Template = Join-Path $Tauri 'msstore\AppxManifest.xml'
$Assets = Join-Path $Tauri 'msstore\Assets'
$Overlay = 'src-tauri/tauri.msstore.conf.json' # relative to app/, where tauri build runs
if (-not $ExePath) { $ExePath = Join-Path $Tauri 'target\release\permark.exe' }
if (-not $DllPath) { $DllPath = Join-Path $Tauri 'target\release-shell\permark_shell.dll' }
if (-not $OutDir) { $OutDir = Join-Path $Tauri 'target\msix' }

function Find-MakeAppx {
  $bin = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
  if (-not (Test-Path $bin)) { return $null }
  Get-ChildItem $bin -Directory |
    Where-Object { $_.Name -match '^\d+(\.\d+){3}$' -and (Test-Path (Join-Path $_.FullName 'x64\makeappx.exe')) } |
    Sort-Object { [version]$_.Name } -Descending |
    Select-Object -First 1 |
    ForEach-Object { Join-Path $_.FullName 'x64\makeappx.exe' }
}

# ---- What is being packed -------------------------------------------------------------------------------------------
$config = Get-Content (Join-Path $Tauri 'tauri.conf.json') -Raw | ConvertFrom-Json
if (-not $Version) { $Version = $config.version }
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
  throw "The app version '$Version' is not three plain numbers (such as 0.4.0). The Store takes no suffix such as -beta."
}
$PackageVersion = "$Version.0"
$Description = $config.bundle.shortDescription
if (-not $Description) { throw 'bundle.shortDescription is missing from tauri.conf.json.' }

if (-not $MakeAppx) { $MakeAppx = Find-MakeAppx }
if (-not $MakeAppx -or -not (Test-Path $MakeAppx)) {
  throw 'MakeAppx.exe was not found. Install the Windows SDK (it is on the GitHub windows-latest runners), or pass -MakeAppx.'
}

# ---- Build ----------------------------------------------------------------------------------------------------------
if ($Build) {
  Write-Host "Building the Store flavor of PerMark $Version ..."
  Push-Location $App
  try {
    # The Tauri CLI itself (not `npm run tauri --` | npx: Windows PowerShell 5.1 and npm eat a bare `--`). Cargo flags the
    # CLI has no option for go after a quoted '--'.
    & (Join-Path $App 'node_modules\.bin\tauri.cmd') build --no-bundle --features store --config $Overlay '--' --no-default-features
    if ($LASTEXITCODE -ne 0) { throw "tauri build failed (exit code $LASTEXITCODE)." }

    # The DLL that File Explorer's helper process loads for the folder menu entries. Its profile unwinds on a panic (the
    # program's aborts), so its guard can turn a panic into an error code.
    Write-Host 'Building the shell extension ...'
    & cargo build --profile release-shell -p permark-shell --manifest-path (Join-Path $Tauri 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw "cargo build of the shell extension failed (exit code $LASTEXITCODE)." }
  }
  finally {
    Pop-Location
  }
}
if (-not (Test-Path $ExePath)) {
  throw "The program is not built: $ExePath was not found. Run this script with -Build, or build the Store flavor first."
}
if (-not (Test-Path $DllPath)) {
  throw "The shell extension is not built: $DllPath was not found. Run this script with -Build, or run cargo build --profile release-shell -p permark-shell in app\src-tauri first."
}

# ---- The package folder ---------------------------------------------------------------------------------------------
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
$Stage = Join-Path $OutDir 'package'
if (Test-Path $Stage) { Remove-Item $Stage -Recurse -Force }
New-Item -ItemType Directory -Force -Path (Join-Path $Stage 'Assets') | Out-Null

# The only file the program needs at run time. The interface is inside it, and the web view is the system's WebView2
# runtime (Windows 11, and Windows 10 kept up to date), so nothing else goes in the package but the DLL of the folder menu.
Copy-Item $ExePath (Join-Path $Stage 'permark.exe')
# Beside the program: the DLL finds the program from its own path, and the manifest names it by this name.
Copy-Item $DllPath (Join-Path $Stage 'permark_shell.dll')
Copy-Item (Join-Path $Assets '*.png') (Join-Path $Stage 'Assets')

$manifest = Get-Content $Template -Raw -Encoding UTF8
foreach ($token in '@VERSION@', '@DESCRIPTION@') {
  if (-not $manifest.Contains($token)) { throw "The manifest template has no $token." }
}
$manifest = $manifest.Replace('@VERSION@', $PackageVersion).Replace('@DESCRIPTION@', [System.Security.SecurityElement]::Escape($Description))

# A package to try on a computer without the Store: its own name, publisher and alias, so it lives beside the Store version.
# (The folder menu's class id is the same, so only one of the two should be installed once the Store version has the menu.)
$Suffix = ''
if ($Sideload) {
  $Suffix = '_sideload'
  foreach ($change in @(
      @('Name="EdelmarSchneider.TrailMark"', 'Name="EdelmarSchneider.PerMarkSideload"'),
      @('Publisher="CN=C8BB0009-0553-4E03-8EDF-1FC57AB0CCF7"', 'Publisher="CN=PerMark Sideload"'),
      @('<DisplayName>PerMark</DisplayName>', '<DisplayName>PerMark (sideload)</DisplayName>'),
      @('DisplayName="PerMark"', 'DisplayName="PerMark (sideload)"'),
      @('Alias="permark.exe"', 'Alias="permark-sideload.exe"'))) {
    if (-not $manifest.Contains($change[0])) { throw "The manifest has no $($change[0]), which -Sideload changes." }
    $manifest = $manifest.Replace($change[0], $change[1])
  }
}
$manifestPath = Join-Path $Stage 'AppxManifest.xml'
[System.IO.File]::WriteAllText($manifestPath, $manifest, (New-Object System.Text.UTF8Encoding($false)))

# The manifest is well-formed XML, and everything it points at is in the folder (MakeAppx checks the schema and the files
# too, with a less helpful message).
$null = [xml]$manifest
$referenced = @([regex]::Matches($manifest, 'Assets\\[^"<]+') | ForEach-Object { $_.Value })
$referenced += [regex]::Matches($manifest, 'Executable="([^"]+)"') | ForEach-Object { $_.Groups[1].Value }
$referenced += [regex]::Matches($manifest, '\sPath="([^"]+)"') | ForEach-Object { $_.Groups[1].Value }
if ($referenced.Count -eq 0) { throw 'The manifest names no files; the check for them is out of date.' }
foreach ($file in $referenced | Sort-Object -Unique) {
  if (-not (Test-Path (Join-Path $Stage $file))) { throw "The manifest names $file, which is not in the package folder." }
}

# ---- Pack -----------------------------------------------------------------------------------------------------------
$Package = Join-Path $OutDir "PerMark_${PackageVersion}_x64$Suffix.msix"
if (Test-Path $Package) { Remove-Item $Package -Force }
Write-Host "Packing $Package with $MakeAppx"
& $MakeAppx pack /d $Stage /p $Package /o /v
if ($LASTEXITCODE -ne 0) { throw "MakeAppx failed (exit code $LASTEXITCODE)." }

# ---- What came out --------------------------------------------------------------------------------------------------
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::OpenRead($Package)
try {
  Write-Host ''
  Write-Host 'Files in the package:'
  $archive.Entries | Sort-Object FullName | ForEach-Object {
    Write-Host ('  {0,12:N0}  {1}' -f $_.Length, [System.Uri]::UnescapeDataString($_.FullName))
  }
}
finally {
  $archive.Dispose()
}
$size = (Get-Item $Package).Length
Write-Host ''
Write-Host ('Package: {0}  ({1:N0} bytes, {2:N1} MiB), version {3}, not signed' -f $Package, $size, ($size / 1MB), $PackageVersion)
if ($env:GITHUB_OUTPUT) {
  "msix=$Package" | Out-File -FilePath $env:GITHUB_OUTPUT -Append -Encoding utf8
  "version=$PackageVersion" | Out-File -FilePath $env:GITHUB_OUTPUT -Append -Encoding utf8
}
$Package
