# Generates basic placeholder icons for Llama Switcher into src-tauri/icons.
# Replace later with real artwork via:  npm run tauri icon path\to\icon.png
#
# Usage:  powershell -ExecutionPolicy Bypass -File scripts/generate-icons.ps1

Add-Type -AssemblyName System.Drawing

$ErrorActionPreference = "Stop"
$iconsDir = Join-Path $PSScriptRoot "..\src-tauri\icons"
New-Item -ItemType Directory -Force -Path $iconsDir | Out-Null

function New-IconBitmap([int]$size) {
    $bmp = New-Object System.Drawing.Bitmap($size, $size)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = "AntiAlias"
    $g.Clear([System.Drawing.Color]::FromArgb(0, 0, 0, 0))

    # Rounded background.
    $bg = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 58, 90, 144))
    $pad = [int]($size * 0.06)
    $g.FillEllipse($bg, $pad, $pad, $size - 2 * $pad, $size - 2 * $pad)

    # A simple "L" glyph.
    $fontSize = [single]($size * 0.5)
    $font = New-Object System.Drawing.Font("Segoe UI", $fontSize, [System.Drawing.FontStyle]::Bold, [System.Drawing.GraphicsUnit]::Pixel)
    $fg = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::White)
    $fmt = New-Object System.Drawing.StringFormat
    $fmt.Alignment = "Center"
    $fmt.LineAlignment = "Center"
    $rect = New-Object System.Drawing.RectangleF(0, 0, $size, $size)
    $g.DrawString("L", $font, $fg, $rect, $fmt)

    $g.Dispose()
    return $bmp
}

# PNG sizes Tauri references.
$sizes = @{ "32x32.png" = 32; "128x128.png" = 128; "128x128@2x.png" = 256; "icon.png" = 512 }
foreach ($name in $sizes.Keys) {
    $bmp = New-IconBitmap $sizes[$name]
    $bmp.Save((Join-Path $iconsDir $name), [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
}

# ICO from a 256x256 bitmap.
$icoBmp = New-IconBitmap 256
$hicon = $icoBmp.GetHicon()
$icon = [System.Drawing.Icon]::FromHandle($hicon)
$icoPath = Join-Path $iconsDir "icon.ico"
$fs = [System.IO.File]::Create($icoPath)
$icon.Save($fs)
$fs.Close()
$icoBmp.Dispose()

Write-Host "Generated icons in $iconsDir"
