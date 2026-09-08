# Produces the pictures the image decoders are tested against.
#
# Why: decoding a picture this project also encoded would prove nothing about
# interoperability, and there is no PNG or JPEG encoder here at all. These come
# out of GDI+, which is an outside implementation and the same one that produced
# a great many of the pictures found inside real documents.
#
# The manifest records what colour each picture is at particular points, read
# back through GDI+ rather than computed here, so the expected values are the
# encoder's own idea of what it wrote.
#
# Run on Windows: powershell -File tools\make-image-fixtures.ps1

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$root = Split-Path -Parent $PSScriptRoot
$fixtures = Join-Path $root 'crates\wp-image\tests\fixtures'
New-Item -ItemType Directory -Force -Path $fixtures | Out-Null

# Where each picture is sampled: away from the edges, where a lossy encoder is
# least accurate.
$points = @(@(4, 4), @(12, 9), @(20, 17), @(29, 25))
$manifest = New-Object System.Collections.Generic.List[string]

function Save-Fixture($bitmap, $name, $format, $tolerance, $encoder) {
    $path = Join-Path $fixtures $name
    if ($encoder) {
        $bitmap.Save($path, $encoder.Codec, $encoder.Parameters)
    } else {
        $bitmap.Save($path, $format)
    }

    foreach ($point in $points) {
        $x = $point[0]; $y = $point[1]
        if ($x -ge $bitmap.Width -or $y -ge $bitmap.Height) { continue }
        $c = $bitmap.GetPixel($x, $y)
        $manifest.Add("$name $($bitmap.Width) $($bitmap.Height) $x $y $($c.R) $($c.G) $($c.B) $($c.A) $tolerance")
    }
}

# A gradient: every channel varies, so a mistake in any one of them shows.
$gradient = New-Object System.Drawing.Bitmap 32, 32, ([System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
for ($y = 0; $y -lt 32; $y++) {
    for ($x = 0; $x -lt 32; $x++) {
        $gradient.SetPixel($x, $y, [System.Drawing.Color]::FromArgb(255, $x * 8, $y * 8, 128))
    }
}
Save-Fixture $gradient 'gradient.png' ([System.Drawing.Imaging.ImageFormat]::Png) 0 $null

# The same picture with transparency, which PNG carries and JPEG does not.
$alpha = New-Object System.Drawing.Bitmap 32, 32, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
for ($y = 0; $y -lt 32; $y++) {
    for ($x = 0; $x -lt 32; $x++) {
        $alpha.SetPixel($x, $y, [System.Drawing.Color]::FromArgb($y * 8, 200, $x * 8, 60))
    }
}
Save-Fixture $alpha 'alpha.png' ([System.Drawing.Imaging.ImageFormat]::Png) 0 $null

# A JPEG at high quality. Some loss is unavoidable, hence the tolerance.
$codec = [System.Drawing.Imaging.ImageCodecInfo]::GetImageEncoders() | Where-Object { $_.MimeType -eq 'image/jpeg' }
$parameters = New-Object System.Drawing.Imaging.EncoderParameters 1
$parameters.Param[0] = New-Object System.Drawing.Imaging.EncoderParameter ([System.Drawing.Imaging.Encoder]::Quality), 95
Save-Fixture $gradient 'gradient.jpg' $null 12 @{ Codec = $codec; Parameters = $parameters }

# Flat colour, where a lossy encoder should be very close indeed.
$flat = New-Object System.Drawing.Bitmap 32, 32, ([System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
$graphics = [System.Drawing.Graphics]::FromImage($flat)
$graphics.Clear([System.Drawing.Color]::FromArgb(255, 30, 144, 255))
$graphics.Dispose()
Save-Fixture $flat 'flat.jpg' $null 4 @{ Codec = $codec; Parameters = $parameters }
Save-Fixture $flat 'flat.png' ([System.Drawing.Imaging.ImageFormat]::Png) 0 $null

Set-Content -Path (Join-Path $fixtures 'manifest.txt') -Value $manifest -Encoding ascii
Write-Output "wrote $($manifest.Count) sample points to $fixtures"
Get-ChildItem $fixtures | ForEach-Object { Write-Output ("  {0}  {1} bytes" -f $_.Name, $_.Length) }
