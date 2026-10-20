<#
.SYNOPSIS
  Regenerates the logos of the Microsoft Store package (app/src-tauri/msstore/Assets) from app/src-tauri/icons/icon.png.

.DESCRIPTION
  The logos are committed, so building the package needs nothing from this script. Run it only when the app's icon
  changes, then commit the result. Needs Windows (it draws with System.Drawing).

  Sizes (docs/msstore.md explains where Windows shows each one):
    Square44x44Logo      the app list, the taskbar, Alt+Tab: the icon edge to edge, in the sizes Windows asks for
                         (targetsize-N), with and without the plate Windows draws behind an icon (altform-unplated)
    Square150x150Logo    the Start tile
    Wide310x150Logo      the wide Start tile
    Square310x310Logo    the large Start tile
    StoreLogo            the package's own logo (50x50)
  The tiles show the icon at 80 percent of the tile, on the tile's background color (the manifest sets it to the icon's own
  dark, so the rounded corners of the icon do not show).
#>
[CmdletBinding()]
param(
  [string]$Source,
  [string]$OutDir
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

# Repo root = parent of scripts/; the program lives in app/.
$Root = Split-Path -Parent $PSScriptRoot
$Tauri = Join-Path $Root 'app\src-tauri'
if (-not $Source) { $Source = Join-Path $Tauri 'icons\icon.png' }
if (-not $OutDir) { $OutDir = Join-Path $Tauri 'msstore\Assets' }

function Save-Logo {
  param(
    [System.Drawing.Image]$Icon,
    [string]$Name,
    [int]$Width,
    [int]$Height,
    # How much of the shorter side of the canvas the icon takes.
    [double]$Fill = 1.0
  )
  $side = [int][math]::Round([math]::Min($Width, $Height) * $Fill)
  $left = [int][math]::Round(($Width - $side) / 2)
  $top = [int][math]::Round(($Height - $side) / 2)

  $canvas = New-Object System.Drawing.Bitmap($Width, $Height, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $canvas.SetResolution(96, 96)
  $graphics = [System.Drawing.Graphics]::FromImage($canvas)
  try {
    $graphics.Clear([System.Drawing.Color]::Transparent)
    $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $graphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    # Tiling the edge keeps the border of the icon from fading into the transparent canvas.
    $attributes = New-Object System.Drawing.Imaging.ImageAttributes
    $attributes.SetWrapMode([System.Drawing.Drawing2D.WrapMode]::TileFlipXY)
    $target = New-Object System.Drawing.Rectangle($left, $top, $side, $side)
    $graphics.DrawImage($Icon, $target, 0, 0, $Icon.Width, $Icon.Height, [System.Drawing.GraphicsUnit]::Pixel, $attributes)
    $canvas.Save((Join-Path $OutDir $Name), [System.Drawing.Imaging.ImageFormat]::Png)
  }
  finally {
    $graphics.Dispose()
    $canvas.Dispose()
  }
  Write-Host ("  {0,-52} {1}x{2}" -f $Name, $Width, $Height)
}

$Source = (Resolve-Path $Source).Path
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
Write-Host "Logos from $Source into $OutDir"

$icon = [System.Drawing.Image]::FromFile($Source)
try {
  Save-Logo $icon 'Square44x44Logo.png' 44 44
  foreach ($size in 16, 24, 32, 48, 256) {
    Save-Logo $icon "Square44x44Logo.targetsize-$size.png" $size $size
    Save-Logo $icon "Square44x44Logo.targetsize-${size}_altform-unplated.png" $size $size
  }
  Save-Logo $icon 'Square150x150Logo.png' 150 150 0.8
  Save-Logo $icon 'Square310x310Logo.png' 310 310 0.8
  Save-Logo $icon 'Wide310x150Logo.png' 310 150 0.8
  Save-Logo $icon 'StoreLogo.png' 50 50
}
finally {
  $icon.Dispose()
}
