<#
.SYNOPSIS
  Packs TrailMark for the Microsoft Store: builds the Store flavor and writes an unsigned TrailMark_<version>_x64.msix.

.DESCRIPTION
  The Store flavor is the same program built with the cargo feature `store` and the overlay `src-tauri/tauri.msstore.conf.json`
  (no self-update; the agent setup lines name the `trailmark` alias). This script:

    1. builds it (with -Build; otherwise it packs the program that is already built),
    2. lays out a package folder: trailmark.exe, the logos (src-tauri/msstore/Assets) and AppxManifest.xml, which it writes
       from the template src-tauri/msstore/AppxManifest.xml with the version filled in,
    3. runs MakeAppx.exe from the Windows SDK, which checks the manifest against the package schema and packs the folder.

  The package is not signed: the Store signs what is uploaded to it, and an unsigned package cannot be installed by hand.
  Nothing is installed or registered by this script.

  The package version is the app's version (tauri.conf.json) and a fourth part, 0, which the Store reserves: 0.4.0 becomes
  0.4.0.0. The app's version must be three plain numbers (no -beta suffix); the Store does not take one.

  Everything the script needs is in the checkout, so it also runs on a checkout of any commit that has it (a release tag
  from before the packaging existed needs the packaging files copied in first; docs/msstore.md says how).

.PARAMETER Build
  Build the Store flavor first: `npx tauri build --no-bundle --features store --config <overlay>`. Needs the
  frontend dependencies (npm ci), Rust and the usual Tauri prerequisites.

.PARAMETER ExePath
  The built program. Default: src-tauri\target\release\trailmark.exe.

.PARAMETER OutDir
  Where the package folder and the .msix go. Default: src-tauri\target\msix.

.PARAMETER Version
  The app's version, three parts. Default: the one in tauri.conf.json.

.PARAMETER MakeAppx
  The path of MakeAppx.exe. Default: the newest x64 one in the Windows SDK (C:\Program Files (x86)\Windows Kits\10\bin).

.EXAMPLE
  .\scripts\build-msix.ps1 -Build

.OUTPUTS
  The path of the .msix.
#>
[CmdletBinding()]
param(
  [switch]$Build,
  [string]$ExePath,
  [string]$OutDir,
  [string]$Version,
  [string]$MakeAppx
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$Root = Split-Path -Parent $PSScriptRoot
$Tauri = Join-Path $Root 'src-tauri'
$Template = Join-Path $Tauri 'msstore\AppxManifest.xml'
$Assets = Join-Path $Tauri 'msstore\Assets'
$Overlay = 'src-tauri/tauri.msstore.conf.json'
if (-not $ExePath) { $ExePath = Join-Path $Tauri 'target\release\trailmark.exe' }
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
  Write-Host "Building the Store flavor of TrailMark $Version ..."
  Push-Location $Root
  try {
    # npx rather than `npm run tauri --`: Windows PowerShell 5.1 swallows the `--` and npm then rejects the flags.
    & npx tauri build --no-bundle --features store --config $Overlay
    if ($LASTEXITCODE -ne 0) { throw "tauri build failed (exit code $LASTEXITCODE)." }
  }
  finally {
    Pop-Location
  }
}
if (-not (Test-Path $ExePath)) {
  throw "The program is not built: $ExePath was not found. Run this script with -Build, or build the Store flavor first."
}

# ---- The package folder ---------------------------------------------------------------------------------------------
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
$Stage = Join-Path $OutDir 'package'
if (Test-Path $Stage) { Remove-Item $Stage -Recurse -Force }
New-Item -ItemType Directory -Force -Path (Join-Path $Stage 'Assets') | Out-Null

# The only file the program needs at run time. The interface is inside it, and the web view is the system's WebView2
# runtime (Windows 11, and Windows 10 kept up to date), so nothing else goes in the package.
Copy-Item $ExePath (Join-Path $Stage 'trailmark.exe')
Copy-Item (Join-Path $Assets '*.png') (Join-Path $Stage 'Assets')

$manifest = Get-Content $Template -Raw -Encoding UTF8
foreach ($token in '@VERSION@', '@DESCRIPTION@') {
  if (-not $manifest.Contains($token)) { throw "The manifest template has no $token." }
}
$manifest = $manifest.Replace('@VERSION@', $PackageVersion).Replace('@DESCRIPTION@', [System.Security.SecurityElement]::Escape($Description))
$manifestPath = Join-Path $Stage 'AppxManifest.xml'
[System.IO.File]::WriteAllText($manifestPath, $manifest, (New-Object System.Text.UTF8Encoding($false)))

# The manifest is well-formed XML, and everything it points at is in the folder (MakeAppx checks the schema and the files
# too, with a less helpful message).
$null = [xml]$manifest
$referenced = @([regex]::Matches($manifest, 'Assets\\[^"<]+') | ForEach-Object { $_.Value })
$referenced += [regex]::Matches($manifest, 'Executable="([^"]+)"') | ForEach-Object { $_.Groups[1].Value }
if ($referenced.Count -eq 0) { throw 'The manifest names no files; the check for them is out of date.' }
foreach ($file in $referenced | Sort-Object -Unique) {
  if (-not (Test-Path (Join-Path $Stage $file))) { throw "The manifest names $file, which is not in the package folder." }
}

# ---- Pack -----------------------------------------------------------------------------------------------------------
$Package = Join-Path $OutDir "TrailMark_${PackageVersion}_x64.msix"
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
