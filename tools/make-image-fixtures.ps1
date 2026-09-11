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

# --- Bitmaps ----------------------------------------------------------------
#
# GDI+ writes bitmaps, and reads a good many forms it does not write. The ones
# it writes are saved through it; the rest are assembled here, byte by byte,
# and read back through GDI+ so that what the manifest says is still an outside
# decoder's reading of the file and never this project's.


function U16([int]$value) { return ,[System.BitConverter]::GetBytes([uint16]$value) }
function U32([int]$value) { return ,[System.BitConverter]::GetBytes([uint32]$value) }
function I32([int]$value) { return ,[System.BitConverter]::GetBytes([int32]$value) }

# Writes the bytes to a file, reads them back through GDI+, and records what
# GDI+ says is at each point.
function Save-Raw($name, $bytes, $points) {
    $path = Join-Path $fixtures $name
    [System.IO.File]::WriteAllBytes($path, $bytes)

    $bitmap = New-Object System.Drawing.Bitmap $path
    foreach ($point in $points) {
        $x = $point[0]; $y = $point[1]
        $c = $bitmap.GetPixel($x, $y)
        $manifest.Add("$name $($bitmap.Width) $($bitmap.Height) $x $y $($c.R) $($c.G) $($c.B) $($c.A) 0")
    }
    $bitmap.Dispose()
    Write-Output ("  {0}  {1} bytes" -f $name, $bytes.Length)
}

# The file header and a forty-byte information header, as one array.
function Headers($width, $height, $depth, $compression, $paletteBytes, $extra) {
    $list = New-Object System.Collections.Generic.List[byte]
    $headerSize = 40 + $extra.Length
    $offset = 14 + $headerSize + $paletteBytes

    $list.AddRange([byte[]]@(0x42, 0x4D))
    $list.AddRange((U32 0))
    $list.AddRange((U32 0))
    $list.AddRange((U32 $offset))

    $list.AddRange((U32 $headerSize))
    $list.AddRange((I32 $width))
    $list.AddRange((I32 $height))
    $list.AddRange((U16 1))
    $list.AddRange((U16 $depth))
    $list.AddRange((U32 $compression))
    $list.AddRange((U32 0))
    $list.AddRange((I32 2835))
    $list.AddRange((I32 2835))
    $list.AddRange((U32 0))
    $list.AddRange((U32 0))
    if ($extra.Length -gt 0) { $list.AddRange([byte[]]$extra) }
    return ,$list
}

# --- The forms GDI+ writes itself -------------------------------------------

$corners = @(@(1, 1), @(2, 2), @(3, 0), @(0, 3))

$colours = @(
    [System.Drawing.Color]::FromArgb(255, 255, 0, 0),
    [System.Drawing.Color]::FromArgb(255, 0, 255, 0),
    [System.Drawing.Color]::FromArgb(255, 0, 0, 255),
    [System.Drawing.Color]::FromArgb(255, 250, 200, 50)
)

$square = New-Object System.Drawing.Bitmap 4, 4, ([System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
for ($y = 0; $y -lt 4; $y++) {
    for ($x = 0; $x -lt 4; $x++) {
        $square.SetPixel($x, $y, $colours[($x + $y) % 4])
    }
}
$path = Join-Path $fixtures 'square24.bmp'
$square.Save($path, [System.Drawing.Imaging.ImageFormat]::Bmp)
$check = New-Object System.Drawing.Bitmap $path
foreach ($point in $corners) {
    $c = $check.GetPixel($point[0], $point[1])
    $manifest.Add("square24.bmp $($check.Width) $($check.Height) $($point[0]) $($point[1]) $($c.R) $($c.G) $($c.B) $($c.A) 0")
}
$check.Dispose()
Write-Output ("  square24.bmp  {0} bytes" -f (Get-Item $path).Length)

# --- The forms it does not ---------------------------------------------------

# Twenty-four bits with the oldest header, which says only the size and depth.
$core = New-Object System.Collections.Generic.List[byte]
$core.AddRange([byte[]]@(0x42, 0x4D))
$core.AddRange((U32 0))
$core.AddRange((U32 0))
$core.AddRange((U32 26))
$core.AddRange((U32 12))
$core.AddRange((U16 4))
$core.AddRange((U16 4))
$core.AddRange((U16 1))
$core.AddRange((U16 24))
for ($y = 3; $y -ge 0; $y--) {
    for ($x = 0; $x -lt 4; $x++) {
        $c = $colours[($x + $y) % 4]
        $core.AddRange([byte[]]@($c.B, $c.G, $c.R))
    }
}
Save-Raw 'core24.bmp' $core.ToArray() $corners

# One bit to the pixel: two colours and a chequer.
$palette = New-Object System.Collections.Generic.List[byte]
$palette.AddRange([byte[]]@(0x20, 0x40, 0x60, 0))
$palette.AddRange([byte[]]@(0xF0, 0xE0, 0xD0, 0))
$one = Headers 8 4 1 0 8 @()
$one.AddRange($palette)
for ($y = 3; $y -ge 0; $y--) {
    $byte = 0
    for ($x = 0; $x -lt 8; $x++) {
        if ((($x + $y) % 2) -eq 1) { $byte = $byte -bor (1 -shl (7 - $x)) }
    }
    $one.AddRange([byte[]]@($byte, 0, 0, 0))
}
Save-Raw 'bits1.bmp' $one.ToArray() @(@(0, 0), @(1, 0), @(4, 2), @(7, 3))

# Four bits to the pixel, with a palette of four.
$palette4 = New-Object System.Collections.Generic.List[byte]
foreach ($c in $colours) { $palette4.AddRange([byte[]]@($c.B, $c.G, $c.R, 0)) }
for ($i = 4; $i -lt 16; $i++) { $palette4.AddRange([byte[]]@(0, 0, 0, 0)) }
$four = Headers 4 4 4 0 64 @()
$four.AddRange($palette4)
for ($y = 3; $y -ge 0; $y--) {
    $row = New-Object System.Collections.Generic.List[byte]
    for ($x = 0; $x -lt 4; $x += 2) {
        $high = ($x + $y) % 4
        $low = ($x + 1 + $y) % 4
        $row.Add([byte](($high -shl 4) -bor $low))
    }
    while ($row.Count % 4 -ne 0) { $row.Add(0) }
    $four.AddRange($row)
}
Save-Raw 'bits4.bmp' $four.ToArray() $corners

# Runs of one colour, eight bits to the pixel: four rows of one colour each.
$palette8 = New-Object System.Collections.Generic.List[byte]
foreach ($c in $colours) { $palette8.AddRange([byte[]]@($c.B, $c.G, $c.R, 0)) }
for ($i = 4; $i -lt 256; $i++) { $palette8.AddRange([byte[]]@(0, 0, 0, 0)) }
$runs = Headers 4 4 8 1 1024 @()
$runs.AddRange($palette8)
for ($y = 3; $y -ge 0; $y--) {
    $runs.AddRange([byte[]]@(4, $y))
    $runs.AddRange([byte[]]@(0, 0))
}
$runs.AddRange([byte[]]@(0, 1))
Save-Raw 'rle8.bmp' $runs.ToArray() $corners

# The same, four bits to the pixel.
$runs4 = Headers 4 4 4 2 64 @()
$runs4.AddRange($palette4)
for ($y = 3; $y -ge 0; $y--) {
    $colour = [byte](($y -shl 4) -bor $y)
    $runs4.AddRange([byte[]]@(4, $colour))
    $runs4.AddRange([byte[]]@(0, 0))
}
$runs4.AddRange([byte[]]@(0, 1))
Save-Raw 'rle4.bmp' $runs4.ToArray() $corners

# Sixteen bits with masks the file states: five, six and five.
$masks = New-Object System.Collections.Generic.List[byte]
$masks.AddRange((U32 0xF800))
$masks.AddRange((U32 0x07E0))
$masks.AddRange((U32 0x001F))
$field = Headers 4 4 16 3 12 @()
$field.AddRange($masks)
for ($y = 3; $y -ge 0; $y--) {
    $row = New-Object System.Collections.Generic.List[byte]
    for ($x = 0; $x -lt 4; $x++) {
        $c = $colours[($x + $y) % 4]
        $value = ((([int]$c.R -shr 3) -shl 11) -bor (([int]$c.G -shr 2) -shl 5) -bor ([int]$c.B -shr 3))
        $row.AddRange((U16 $value))
    }
    $field.AddRange($row)
}
Save-Raw 'rgb565.bmp' $field.ToArray() $corners

# The rows the way they are read, which a negative height says.
$down = Headers 4 -4 24 0 0 @()
for ($y = 0; $y -lt 4; $y++) {
    for ($x = 0; $x -lt 4; $x++) {
        $c = $colours[($x + $y) % 4]
        $down.AddRange([byte[]]@($c.B, $c.G, $c.R))
    }
}
Save-Raw 'topdown.bmp' $down.ToArray() $corners

Set-Content -Path (Join-Path $fixtures 'manifest.txt') -Value $manifest -Encoding ascii
Write-Output "wrote $($manifest.Count) sample points to $fixtures"
Get-ChildItem $fixtures | ForEach-Object { Write-Output ("  {0}  {1} bytes" -f $_.Name, $_.Length) }
