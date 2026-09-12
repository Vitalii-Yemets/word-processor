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
function Save-Raw($name, $bytes, $points, $tolerance = 0) {
    $path = Join-Path $fixtures $name
    [System.IO.File]::WriteAllBytes($path, $bytes)

    $bitmap = New-Object System.Drawing.Bitmap $path
    foreach ($point in $points) {
        $x = $point[0]; $y = $point[1]
        $c = $bitmap.GetPixel($x, $y)
        $manifest.Add("$name $($bitmap.Width) $($bitmap.Height) $x $y $($c.R) $($c.G) $($c.B) $($c.A) $tolerance")
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

# --- GIFs -------------------------------------------------------------------
#
# GDI+ writes one kind of GIF and reads several. The one it writes is saved
# through it; the rest are assembled here and read back through it, the same way
# the bitmaps above are.
#
# The pixels of the assembled ones are written as a real LZW stream with a clear
# code every few pixels, which is what a program with no compressor writes: the
# table never fills, so the codes never grow past their first width and the
# stream can be produced without a compressor here either. A decoder cannot tell
# such a stream from a compressed one.

function Gif-Pixels($indices, $least) {
    $clear = 1 -shl $least
    $end = $clear + 1
    $width = $least + 1
    # How many codes may follow a clear before the table would grow. The first
    # of them adds nothing to the table, each of the rest adds one, and the
    # codes widen the moment the table reaches twice the palette.
    $run = $clear - 2

    $codes = New-Object System.Collections.Generic.List[int]
    $since = 0
    $codes.Add($clear)
    foreach ($index in $indices) {
        if ($since -ge $run) { $codes.Add($clear); $since = 0 }
        $codes.Add([int]$index)
        $since++
    }
    $codes.Add($end)

    $bytes = New-Object System.Collections.Generic.List[byte]
    $held = 0
    $count = 0
    foreach ($code in $codes) {
        $held = $held -bor ($code -shl $count)
        $count += $width
        while ($count -ge 8) {
            $bytes.Add([byte]($held -band 0xFF))
            $held = $held -shr 8
            $count -= 8
        }
    }
    if ($count -gt 0) { $bytes.Add([byte]($held -band 0xFF)) }

    # The stream is carried in sub-blocks of at most 255 bytes, ended by a zero.
    $out = New-Object System.Collections.Generic.List[byte]
    $out.Add([byte]$least)
    $at = 0
    while ($at -lt $bytes.Count) {
        $length = [Math]::Min(255, $bytes.Count - $at)
        $out.Add([byte]$length)
        for ($i = 0; $i -lt $length; $i++) { $out.Add($bytes[$at + $i]) }
        $at += $length
    }
    $out.Add(0)
    return ,$out
}

# The header, the screen descriptor and a global palette of four.
function Gif-Head($width, $height) {
    $out = New-Object System.Collections.Generic.List[byte]
    $out.AddRange([byte[]][System.Text.Encoding]::ASCII.GetBytes('GIF89a'))
    $out.AddRange((U16 $width))
    $out.AddRange((U16 $height))
    $out.Add([byte](0x80 -bor 0x01))
    $out.Add(0)
    $out.Add(0)
    foreach ($c in $colours) { $out.AddRange([byte[]]@($c.R, $c.G, $c.B)) }
    return ,$out
}

# One image block: where it goes, how big, and its pixels.
function Gif-Frame($left, $top, $width, $height, $indices, $interlaced) {
    $out = New-Object System.Collections.Generic.List[byte]
    $out.Add(0x2C)
    $out.AddRange((U16 $left))
    $out.AddRange((U16 $top))
    $out.AddRange((U16 $width))
    $out.AddRange((U16 $height))
    $out.Add([byte]$(if ($interlaced) { 0x40 } else { 0x00 }))
    $out.AddRange((Gif-Pixels $indices 2))
    return ,$out
}

# The four-by-four square, as palette indices read row by row.
$square16 = @()
for ($y = 0; $y -lt 4; $y++) {
    for ($x = 0; $x -lt 4; $x++) { $square16 += (($x + $y) % 4) }
}

# What GDI+ writes itself.
$path = Join-Path $fixtures 'square.gif'
$square.Save($path, [System.Drawing.Imaging.ImageFormat]::Gif)
$check = New-Object System.Drawing.Bitmap $path
foreach ($point in $corners) {
    $c = $check.GetPixel($point[0], $point[1])
    $manifest.Add("square.gif $($check.Width) $($check.Height) $($point[0]) $($point[1]) $($c.R) $($c.G) $($c.B) $($c.A) 0")
}
$check.Dispose()
Write-Output ("  square.gif  {0} bytes" -f (Get-Item $path).Length)

# The rows in the order interlacing puts them: for four rows, 0 and 2 in the
# first two passes and 1 and 3 in the last.
$woven = @()
foreach ($row in @(0, 2, 1, 3)) {
    for ($x = 0; $x -lt 4; $x++) { $woven += (($x + $row) % 4) }
}
$interlaced = Gif-Head 4 4
$interlaced.AddRange((Gif-Frame 0 0 4 4 $woven $true))
$interlaced.Add(0x3B)
Save-Raw 'interlaced.gif' $interlaced.ToArray() $corners

# A colour the file says is not to be drawn.
$clearOne = Gif-Head 4 4
$clearOne.AddRange([byte[]]@(0x21, 0xF9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00))
$clearOne.AddRange((Gif-Frame 0 0 4 4 $square16 $false))
$clearOne.Add(0x3B)
Save-Raw 'transparent.gif' $clearOne.ToArray() $corners

# Two frames. What is drawn is the first of them, which is what Word draws.
$moving = Gif-Head 4 4
$first = @(); foreach ($i in 0..15) { $first += 1 }
$second = @(); foreach ($i in 0..15) { $second += 2 }
$moving.AddRange([byte[]]@(0x21, 0xF9, 0x04, 0x00, 0x32, 0x00, 0x00, 0x00))
$moving.AddRange((Gif-Frame 0 0 4 4 $first $false))
$moving.AddRange([byte[]]@(0x21, 0xF9, 0x04, 0x00, 0x32, 0x00, 0x00, 0x00))
$moving.AddRange((Gif-Frame 0 0 4 4 $second $false))
$moving.Add(0x3B)
Save-Raw 'animated.gif' $moving.ToArray() $corners

# --- Progressive JPEG --------------------------------------------------------
#
# GDI+ writes baseline JPEG and nothing else, so this one is assembled here. It
# reads progressive perfectly well, which is what matters: the manifest is still
# its reading of the file and not this project's.
#
# The picture is one block of one component, and it is written in four scans —
# the first bits of the first coefficient, the first bits of the rest, one more
# bit of the first, and one more bit of the rest. That is every kind of scan a
# progressive picture is made of.

# A writer of entropy-coded bits: most significant first, and an 0xFF byte
# followed by the stuffed zero that keeps it from looking like a marker.
$script:jheld = 0
$script:jcount = 0
$script:jbytes = $null

function Jpeg-Start {
    $script:jheld = 0
    $script:jcount = 0
    $script:jbytes = New-Object System.Collections.Generic.List[byte]
}

function Jpeg-Put([int]$value, [int]$width) {
    for ($i = $width - 1; $i -ge 0; $i--) {
        $bit = ($value -shr $i) -band 1
        $script:jheld = (($script:jheld -shl 1) -bor $bit) -band 0xFF
        $script:jcount++
        if ($script:jcount -eq 8) {
            $byte = [byte]$script:jheld
            $script:jbytes.Add($byte)
            if ($byte -eq 0xFF) { $script:jbytes.Add(0) }
            $script:jheld = 0
            $script:jcount = 0
        }
    }
}

# The last byte is padded with ones, which is what the format says to pad with.
function Jpeg-End {
    while ($script:jcount -ne 0) { Jpeg-Put 1 1 }
    return ,$script:jbytes
}

# A marker segment: the marker, the length, and the body.
function Jpeg-Segment([int]$marker, $body) {
    $out = New-Object System.Collections.Generic.List[byte]
    $out.Add(0xFF)
    $out.Add([byte]$marker)
    $length = $body.Count + 2
    $out.Add([byte](($length -shr 8) -band 0xFF))
    $out.Add([byte]($length -band 0xFF))
    $out.AddRange($body)
    return ,$out
}

# A Huffman table of eight codes, all four bits long, standing for the symbols
# nought to seven. Canonical, so the code for a symbol is the symbol — and
# eight of them rather than sixteen because a table may not use the code that
# is all ones, which sixteen four-bit codes would.
function Jpeg-Table([int]$class, [int]$index) {
    $body = New-Object System.Collections.Generic.List[byte]
    $body.Add([byte](($class -shl 4) -bor $index))
    foreach ($length in 1..16) {
        $body.Add([byte]$(if ($length -eq 4) { 8 } else { 0 }))
    }
    foreach ($symbol in 0..7) { $body.Add([byte]$symbol) }
    return ,$body
}

$progressive = New-Object System.Collections.Generic.List[byte]
$progressive.AddRange([byte[]]@(0xFF, 0xD8))

# A quantisation table of ones, so the coefficients are the coefficients.
$quant = New-Object System.Collections.Generic.List[byte]
$quant.Add(0)
foreach ($i in 0..63) { $quant.Add(1) }
$progressive.AddRange((Jpeg-Segment 0xDB $quant))

# A progressive frame: eight by eight, one component, no subsampling.
$frame = New-Object System.Collections.Generic.List[byte]
$frame.Add(8)
$frame.AddRange([byte[]]@(0, 8))
$frame.AddRange([byte[]]@(0, 8))
$frame.Add(1)
$frame.AddRange([byte[]]@(1, 0x11, 0))
$progressive.AddRange((Jpeg-Segment 0xC2 $frame))

$progressive.AddRange((Jpeg-Segment 0xC4 (Jpeg-Table 0 0)))
$progressive.AddRange((Jpeg-Segment 0xC4 (Jpeg-Table 1 0)))

# The header of a scan: one component, and the band and bits it carries.
function Jpeg-ScanHead([int]$tables, [int]$start, [int]$end, [int]$high, [int]$low) {
    $head = New-Object System.Collections.Generic.List[byte]
    $head.Add(1)
    $head.AddRange([byte[]]@(1, $tables))
    $head.Add([byte]$start)
    $head.Add([byte]$end)
    $head.Add([byte](($high -shl 4) -bor $low))
    return ,$head
}

# The first coefficient, at half its precision: a difference of four, which is
# three bits wide, so the symbol is three and the bits are those of four.
$progressive.AddRange((Jpeg-Segment 0xDA (Jpeg-ScanHead 0x00 0 0 0 1)))
Jpeg-Start
Jpeg-Put 3 4
Jpeg-Put 4 3
$progressive.AddRange((Jpeg-End))

# The rest of the band, also at half precision: three at the first place, minus
# one at the second, and then the end of the block.
$progressive.AddRange((Jpeg-Segment 0xDA (Jpeg-ScanHead 0x00 1 63 0 1)))
Jpeg-Start
Jpeg-Put 2 4     # No run, two bits.
Jpeg-Put 3 2     # Which are three.
Jpeg-Put 1 4     # No run, one bit.
Jpeg-Put 0 1     # Which is minus one.
Jpeg-Put 0 4     # And the end of the block.
$progressive.AddRange((Jpeg-End))

# One more bit of the first coefficient.
$progressive.AddRange((Jpeg-Segment 0xDA (Jpeg-ScanHead 0x00 0 0 1 0)))
Jpeg-Start
Jpeg-Put 1 1
$progressive.AddRange((Jpeg-End))

# And one more bit of the rest: nothing new in the band, so the end of the
# block comes first and the two coefficients already there take their bits
# after it.
$progressive.AddRange((Jpeg-Segment 0xDA (Jpeg-ScanHead 0x00 1 63 1 0)))
Jpeg-Start
Jpeg-Put 0 4     # The end of the block, a run of one.
Jpeg-Put 1 1     # The first coefficient grows.
Jpeg-Put 0 1     # The second does not.
$progressive.AddRange((Jpeg-End))

$progressive.AddRange([byte[]]@(0xFF, 0xD9))

Save-Raw 'progressive.jpg' $progressive.ToArray() @(@(0, 0), @(3, 3), @(7, 7), @(5, 2)) 2

Set-Content -Path (Join-Path $fixtures 'manifest.txt') -Value $manifest -Encoding ascii
Write-Output "wrote $($manifest.Count) sample points to $fixtures"
Get-ChildItem $fixtures | ForEach-Object { Write-Output ("  {0}  {1} bytes" -f $_.Name, $_.Length) }
