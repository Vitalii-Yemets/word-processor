//! Pictures in the codings a PDF may carry them in, made by another
//! program and read back here.
//!
//! ImageMagick is in the build image, with OpenJPEG under it for JPEG 2000
//! and its own fax coder. It writes a picture made here in one of those
//! codings, the result is set on a PDF page of its own, and the picture
//! this reader takes out of the page is held against the one that went in
//! — exactly, where the coding loses nothing — and against ImageMagick's
//! own reading of the same file where it does.

use std::path::{Path, PathBuf};
use std::process::Command;

use wp_docx::model::{Block, RunContent};

/// A folder of its own, for tests that run at once.
fn folder(name: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let number = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let folder = std::env::temp_dir()
        .join(format!("wp-pdf-pictures-{}-{number}-{name}", std::process::id()));
    let _ = std::fs::create_dir_all(&folder);
    folder
}

fn convert(folder: &Path, arguments: &[&str]) {
    let output =
        Command::new("convert").current_dir(folder).args(arguments).output().unwrap_or_else(
            |error| {
                panic!("cannot run convert: {error}\nthe build image should install imagemagick")
            },
        );
    assert!(
        output.status.success(),
        "convert {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The picture that goes in: colour that changes smoothly across it, with
/// hard edges through it, at a size that is not a power of two.
fn source(width: usize, height: usize) -> wp_raster::Canvas {
    let mut canvas = wp_raster::Canvas::new(width, height);
    let mut pixels = vec![0u8; width * height * 4];
    for y in 0..height {
        for x in 0..width {
            let at = (y * width + x) * 4;
            let stripe = (x / 7 + y / 5) % 3 == 0;
            pixels[at] = (x * 255 / width) as u8;
            pixels[at + 1] = if stripe { 240 } else { (y * 255 / height) as u8 };
            pixels[at + 2] = ((x * y) % 256) as u8;
            pixels[at + 3] = 255;
        }
    }
    canvas.paste_rect(0, 0, width as i32, height as i32, &pixels);
    canvas
}

/// A PDF of one page showing one picture, its stream as given.
fn page_with(entries: &str, data: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut pdf = b"%PDF-1.5\n".to_vec();
    pdf.extend_from_slice(b"1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n");
    pdf.extend_from_slice(b"2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n");
    pdf.extend_from_slice(
        format!(
            "3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Resources << /XObject << /Im1 4 0 R >> >> /Contents 5 0 R >> endobj\n"
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(
        format!(
            "4 0 obj << /Type /XObject /Subtype /Image /Width {width} /Height {height} {entries} /Length {} >> stream\n",
            data.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(data);
    pdf.extend_from_slice(b"\nendstream endobj\n");
    let content = format!("q {width} 0 0 {height} 0 0 cm /Im1 Do Q");
    pdf.extend_from_slice(
        format!("5 0 obj << /Length {} >> stream\n{content}\nendstream endobj\n", content.len())
            .as_bytes(),
    );
    pdf.extend_from_slice(b"trailer << /Root 1 0 R >>\n%%EOF\n");
    pdf
}

/// The picture a PDF reads back to, as pixels.
fn picture_in(pdf: &[u8]) -> wp_image::Image {
    let document = wp_pdf::open(pdf).expect("opened");
    for block in &document.body().blocks {
        let Block::Paragraph(paragraph) = block else { continue };
        for run in &paragraph.runs {
            for content in &run.content {
                if let RunContent::Picture(picture) = content {
                    let bytes = document.embedded_part(&picture.relationship).expect("its part");
                    return wp_image::decode(bytes).expect("a picture that decodes");
                }
            }
        }
    }
    panic!("no picture in {:?}", document.body().blocks);
}

/// How far apart two pictures are: the largest difference in any colour,
/// and how many samples differ at all.
fn difference(one: &wp_image::Image, other: &wp_image::Image) -> (u8, usize) {
    assert_eq!((one.width, one.height), (other.width, other.height), "sizes");
    let mut most = 0;
    let mut count = 0;
    for (a, b) in one.pixels.chunks_exact(4).zip(other.pixels.chunks_exact(4)) {
        for channel in 0..3 {
            let d = a[channel].abs_diff(b[channel]);
            most = most.max(d);
            count += usize::from(d > 0);
        }
    }
    (most, count)
}

fn read_png(path: &Path) -> wp_image::Image {
    wp_image::decode(&std::fs::read(path).expect("written")).expect("a PNG")
}

/// Codes the source picture with ImageMagick's arguments, reads it back
/// here and there, and says how far apart the two readings are and how
/// far this one is from the source. The file is JP2 unless `format`
/// names another.
fn jpeg_2000(name: &str, arguments: &[&str], grey: bool) -> ((u8, usize), (u8, usize)) {
    jpeg_2000_as(name, "JP2", arguments, grey)
}

fn jpeg_2000_as(
    name: &str,
    format: &str,
    arguments: &[&str],
    grey: bool,
) -> ((u8, usize), (u8, usize)) {
    let folder = folder(name);
    let (width, height) = (97, 61);
    std::fs::write(folder.join("source.png"), wp_raster::encode_png(&source(width, height)))
        .unwrap();
    let mut all = vec!["source.png"];
    if grey {
        all.extend(["-colorspace", "Gray"]);
    }
    all.extend_from_slice(arguments);
    let coded_as = format!("{format}:coded.jp2");
    all.push(&coded_as);
    convert(&folder, &all);
    convert(
        &folder,
        &[&coded_as, "-colorspace", "sRGB", "-type", "TrueColor", "PNG24:reference.png"],
    );
    if grey {
        convert(
            &folder,
            &["source.png", "-colorspace", "Gray", "-type", "TrueColor", "PNG24:source-grey.png"],
        );
    }
    let coded = std::fs::read(folder.join("coded.jp2")).unwrap();
    let reference = read_png(&folder.join("reference.png"));
    let source = read_png(&folder.join(if grey { "source-grey.png" } else { "source.png" }));
    let _ = std::fs::remove_dir_all(&folder);
    let space = if grey { "/DeviceGray" } else { "/DeviceRGB" };
    let pdf = page_with(
        &format!("/ColorSpace {space} /BitsPerComponent 8 /Filter /JPXDecode"),
        &coded,
        width,
        height,
    );
    let read = picture_in(&pdf);
    (difference(&read, &reference), difference(&read, &source))
}

#[test]
fn a_lossless_jpeg_2000_picture_comes_back_exact() {
    let (against_reference, against_source) = jpeg_2000("lossless", &["-quality", "100"], false);
    assert_eq!(against_reference, (0, 0));
    assert_eq!(against_source, (0, 0));
}

#[test]
fn a_grey_jpeg_2000_picture_comes_back_exact() {
    let (against_reference, _) = jpeg_2000("grey", &["-quality", "100"], true);
    assert_eq!(against_reference, (0, 0));
}

#[test]
fn a_lossy_jpeg_2000_picture_reads_as_openjpeg_reads_it() {
    let (against_reference, against_source) = jpeg_2000("lossy", &["-quality", "40"], false);
    assert!(against_reference.0 <= 2, "{against_reference:?}");
    assert!(against_source.0 > 0, "not lossy after all");
}

#[test]
fn jpeg_2000_tiles_resolutions_and_orders_are_followed() {
    for order in ["LRCP", "RLCP", "RPCL", "PCRL", "CPRL"] {
        let define = format!("jp2:progression-order={order}");
        let (against_reference, _) = jpeg_2000(
            &format!("tiles-{order}"),
            &["-extract", "32x24", "-define", &define, "-define", "jp2:number-resolutions=3"],
            false,
        );
        assert_eq!(against_reference, (0, 0), "{order}");
    }
}

#[test]
fn jpeg_2000_layers_are_gathered() {
    let (against_reference, _) = jpeg_2000(
        "layers",
        &[
            "-define",
            "jp2:layer-number=3",
            "-define",
            "jp2:rate=40,10,4",
            "-define",
            "jp2:progression-order=PCRL",
        ],
        false,
    );
    assert!(against_reference.0 <= 2, "{against_reference:?}");
}

#[test]
fn a_bare_jpeg_2000_codestream_is_read() {
    let (against_reference, _) = jpeg_2000_as("codestream", "J2K", &["-quality", "100"], false);
    assert_eq!(against_reference, (0, 0));
}

/// The fields of the first picture in a TIFF, and its strips' bytes put
/// together: enough to lift a fax coding out of one.
fn tiff_strips(bytes: &[u8]) -> (std::collections::HashMap<u16, Vec<u32>>, Vec<u8>) {
    let little = bytes.starts_with(b"II");
    let u16_at = |at: usize| {
        let pair = [bytes[at], bytes[at + 1]];
        if little {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        }
    };
    let u32_at = |at: usize| {
        let four = [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]];
        if little {
            u32::from_le_bytes(four)
        } else {
            u32::from_be_bytes(four)
        }
    };
    let directory = u32_at(4) as usize;
    let mut fields = std::collections::HashMap::new();
    for entry in 0..usize::from(u16_at(directory)) {
        let at = directory + 2 + entry * 12;
        let (tag, kind, count) = (u16_at(at), u16_at(at + 2), u32_at(at + 4) as usize);
        let size = if kind == 3 { 2 } else { 4 };
        let start = if size * count <= 4 { at + 8 } else { u32_at(at + 8) as usize };
        let values: Vec<u32> =
            (0..count)
                .map(|i| {
                    if kind == 3 {
                        u32::from(u16_at(start + 2 * i))
                    } else {
                        u32_at(start + 4 * i)
                    }
                })
                .collect();
        fields.insert(tag, values);
    }
    let mut data = Vec::new();
    for (offset, length) in fields[&273].iter().zip(&fields[&279]) {
        data.extend_from_slice(&bytes[*offset as usize..(*offset + *length) as usize]);
    }
    (fields, data)
}

/// The source picture in black and white, coded by ImageMagick's TIFF
/// writer in a fax coding, set in a PDF here; and the black and white
/// picture ImageMagick means.
fn fax_page(name: &str, compression: &str) -> (Vec<u8>, wp_image::Image) {
    let folder = folder(name);
    let (width, height) = (97, 61);
    std::fs::write(folder.join("source.png"), wp_raster::encode_png(&source(width, height)))
        .unwrap();
    convert(&folder, &["source.png", "-monochrome", "-compress", compression, "coded.tif"]);
    convert(&folder, &["source.png", "-monochrome", "-type", "TrueColor", "PNG24:reference.png"]);
    let coded = std::fs::read(folder.join("coded.tif")).unwrap();
    let reference = read_png(&folder.join("reference.png"));
    let _ = std::fs::remove_dir_all(&folder);
    let (fields, data) = tiff_strips(&coded);
    assert_eq!(fields.get(&266).map_or(1, |v| v[0]), 1, "bits from the top of each byte");
    // A fax coding's runs start white; where the TIFF says a nought bit is
    // black, what it coded as white is black.
    let black_is_1 = fields.get(&262).map_or(0, |v| v[0]) == 1;
    let options = fields.get(&292).map_or(0, |v| v[0]);
    let parameters = match fields[&259][0] {
        4 => format!("/K -1 /Columns {width} /Rows {height} /BlackIs1 {black_is_1}"),
        3 => format!(
            "/K {} /EndOfLine true /EncodedByteAlign {} /Columns {width} /Rows {height} /BlackIs1 {black_is_1}",
            options & 1,
            options & 4 != 0
        ),
        other => panic!("compression {other}"),
    };
    let pdf = page_with(
        &format!(
            "/ColorSpace /DeviceGray /BitsPerComponent 1 /Filter /CCITTFaxDecode /DecodeParms << {parameters} >>"
        ),
        &data,
        width,
        height,
    );
    (pdf, reference)
}

#[test]
fn a_group_4_fax_picture_is_read() {
    let (pdf, reference) = fax_page("group4", "Group4");
    assert_eq!(difference(&picture_in(&pdf), &reference), (0, 0));
}

#[test]
fn a_group_3_fax_picture_is_read() {
    let (pdf, reference) = fax_page("group3", "Fax");
    assert_eq!(difference(&picture_in(&pdf), &reference), (0, 0));
}

// ---------------------------------------------------------------------------
// JBIG2. Nothing in the build image writes it, so the tests code their own
// with the standard's encoder — the arithmetic coder checked against the
// standard's example — and poppler, which LibreOffice's PDF import runs
// on, reads the same file as a second opinion: an encoder and a decoder
// written from one reading of the standard would agree with each other
// however wrong the reading was.

/// The MQ coder's states: probability, next after the more likely symbol,
/// next after the less likely, and whether that swaps them.
const STATES: [(u32, u8, u8, bool); 47] = [
    (0x5601, 1, 1, true),
    (0x3401, 2, 6, false),
    (0x1801, 3, 9, false),
    (0x0AC1, 4, 12, false),
    (0x0521, 5, 29, false),
    (0x0221, 38, 33, false),
    (0x5601, 7, 6, true),
    (0x5401, 8, 14, false),
    (0x4801, 9, 14, false),
    (0x3801, 10, 14, false),
    (0x3001, 11, 17, false),
    (0x2401, 12, 18, false),
    (0x1C01, 13, 20, false),
    (0x1601, 29, 21, false),
    (0x5601, 15, 14, true),
    (0x5401, 16, 14, false),
    (0x5101, 17, 15, false),
    (0x4801, 18, 16, false),
    (0x3801, 19, 17, false),
    (0x3401, 20, 18, false),
    (0x3001, 21, 19, false),
    (0x2801, 22, 19, false),
    (0x2401, 23, 20, false),
    (0x2201, 24, 21, false),
    (0x1C01, 25, 22, false),
    (0x1801, 26, 23, false),
    (0x1601, 27, 24, false),
    (0x1401, 28, 25, false),
    (0x1201, 29, 26, false),
    (0x1101, 30, 27, false),
    (0x0AC1, 31, 28, false),
    (0x09C1, 32, 29, false),
    (0x08A1, 33, 30, false),
    (0x0521, 34, 31, false),
    (0x0441, 35, 32, false),
    (0x02A1, 36, 33, false),
    (0x0221, 37, 34, false),
    (0x0141, 38, 35, false),
    (0x0111, 39, 36, false),
    (0x0085, 40, 37, false),
    (0x0049, 41, 38, false),
    (0x0025, 42, 39, false),
    (0x0015, 43, 40, false),
    (0x0009, 44, 41, false),
    (0x0005, 45, 42, false),
    (0x0001, 45, 43, false),
    (0x5601, 46, 46, false),
];

#[derive(Clone, Copy, Default)]
struct Cx {
    index: u8,
    more_likely: u8,
}

/// The MQ encoder, [T.88] E.2, the way a software encoder keeps its bytes.
struct Mq {
    a: u64,
    c: u64,
    ct: u32,
    b: u64,
    started: bool,
    out: Vec<u8>,
}

impl Mq {
    fn new() -> Self {
        Self { a: 0x8000, c: 0, ct: 12, b: 0, started: false, out: Vec::new() }
    }

    fn encode(&mut self, cx: &mut Cx, bit: u8) {
        let (qe, next_more, next_less, switch) = STATES[usize::from(cx.index)];
        let qe = u64::from(qe);
        self.a -= qe;
        if bit == cx.more_likely {
            if self.a & 0x8000 != 0 {
                self.c += qe;
                return;
            }
            if self.a < qe {
                self.a = qe;
            } else {
                self.c += qe;
            }
            cx.index = next_more;
        } else {
            if self.a < qe {
                self.c += qe;
            } else {
                self.a = qe;
            }
            if switch {
                cx.more_likely = 1 - cx.more_likely;
            }
            cx.index = next_less;
        }
        loop {
            self.a <<= 1;
            self.c <<= 1;
            self.ct -= 1;
            if self.ct == 0 {
                self.byte_out();
            }
            if self.a & 0x8000 != 0 {
                break;
            }
        }
    }

    fn emit(&mut self) {
        if self.started {
            self.out.push(self.b as u8);
        }
        self.started = true;
    }

    fn byte_out(&mut self) {
        let wide = if self.b == 0xFF {
            true
        } else if self.c < 0x800_0000 {
            false
        } else {
            self.b += 1;
            if self.b == 0xFF {
                self.c &= 0x7FF_FFFF;
                true
            } else {
                false
            }
        };
        self.emit();
        if wide {
            self.b = self.c >> 20;
            self.c &= 0xF_FFFF;
            self.ct = 7;
        } else {
            self.b = self.c >> 19;
            self.c &= 0x7_FFFF;
            self.ct = 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        let temp = self.c + self.a;
        self.c |= 0xFFFF;
        if self.c >= temp {
            self.c -= 0x8000;
        }
        self.c <<= self.ct;
        self.byte_out();
        self.c <<= self.ct;
        self.byte_out();
        self.emit();
        if self.b != 0xFF {
            self.out.push(0xFF);
        }
        self.out.push(0xAC);
        self.out
    }
}

#[test]
fn the_test_encoder_codes_the_standards_example() {
    // T.88 Annex H.2: 256 bits, one context, and what they code to.
    let bits = [
        0x00u8, 0x02, 0x00, 0x51, 0x00, 0x00, 0x00, 0xC0, 0x03, 0x52, 0x87, 0x2A, 0xAA, 0xAA, 0xAA,
        0xAA, 0x82, 0xC0, 0x20, 0x00, 0xFC, 0xD7, 0x9E, 0xF6, 0xBF, 0x7F, 0xED, 0x90, 0x4F, 0x46,
        0xA3, 0xBF,
    ];
    let coded = [
        0x84u8, 0xC7, 0x3B, 0xFC, 0xE1, 0xA1, 0x43, 0x04, 0x02, 0x20, 0x00, 0x00, 0x41, 0x0D, 0xBB,
        0x86, 0xF4, 0x31, 0x7F, 0xFF, 0x88, 0xFF, 0x37, 0x47, 0x1A, 0xDB, 0x6A, 0xDF, 0xFF, 0xAC,
    ];
    let mut mq = Mq::new();
    let mut cx = Cx::default();
    for byte in bits {
        for shift in (0..8).rev() {
            mq.encode(&mut cx, (byte >> shift) & 1);
        }
    }
    assert_eq!(mq.finish(), coded);
}

/// A black and white page for JBIG2, a byte a pixel.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Bilevel {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

impl Bilevel {
    fn get(&self, x: i64, y: i64) -> u8 {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            0
        } else {
            self.pixels[y as usize * self.width + x as usize]
        }
    }

    fn part(&self, x: usize, y: usize, width: usize, height: usize) -> Self {
        let mut pixels = Vec::with_capacity(width * height);
        for row in y..y + height {
            for column in x..x + width {
                pixels.push(self.get(column as i64, row as i64));
            }
        }
        Self { width, height, pixels }
    }

    /// As a PDF picture shows it: black where set.
    fn image(&self) -> wp_image::Image {
        let mut pixels = Vec::with_capacity(self.width * self.height * 4);
        for &pixel in &self.pixels {
            let value = if pixel == 1 { 0 } else { 255 };
            pixels.extend_from_slice(&[value, value, value, 255]);
        }
        wp_image::Image { width: self.width, height: self.height, pixels }
    }
}

/// Shapes with edges every way, and a band of rows all alike for typical
/// prediction to skip.
fn bilevel(width: usize, height: usize) -> Bilevel {
    let mut pixels = vec![0u8; width * height];
    for y in 0..height {
        for x in 0..width {
            let (xf, yf) = (x as f64, y as f64);
            let ring = ((xf - 30.0).powi(2) + (yf - 25.0).powi(2)).sqrt();
            let on = (15.0..19.0).contains(&ring)
                || (x + 2 * y) % 23 < 3
                || (60..80).contains(&x) && (35..50).contains(&y) && (x + y) % 2 == 0
                || (20..26).contains(&y) && (70..90).contains(&x);
            pixels[y * width + x] = u8::from(on);
        }
    }
    Bilevel { width, height, pixels }
}

/// A pixel's context in a generic region, its bits in the standard's order.
fn generic_context(page: &Bilevel, x: i64, y: i64, template: u8, at: &[(i64, i64); 4]) -> usize {
    let p = |dx: i64, dy: i64| usize::from(page.get(x + dx, y + dy));
    let a = |i: usize| p(at[i].0, at[i].1);
    match template {
        0 => {
            p(-1, 0)
                | p(-2, 0) << 1
                | p(-3, 0) << 2
                | p(-4, 0) << 3
                | a(0) << 4
                | p(2, -1) << 5
                | p(1, -1) << 6
                | p(0, -1) << 7
                | p(-1, -1) << 8
                | p(-2, -1) << 9
                | a(1) << 10
                | a(2) << 11
                | p(1, -2) << 12
                | p(0, -2) << 13
                | p(-1, -2) << 14
                | a(3) << 15
        }
        1 => {
            p(-1, 0)
                | p(-2, 0) << 1
                | p(-3, 0) << 2
                | a(0) << 3
                | p(2, -1) << 4
                | p(1, -1) << 5
                | p(0, -1) << 6
                | p(-1, -1) << 7
                | p(-2, -1) << 8
                | p(2, -2) << 9
                | p(1, -2) << 10
                | p(0, -2) << 11
                | p(-1, -2) << 12
        }
        2 => {
            p(-1, 0)
                | p(-2, 0) << 1
                | a(0) << 2
                | p(1, -1) << 3
                | p(0, -1) << 4
                | p(-1, -1) << 5
                | p(-2, -1) << 6
                | p(1, -2) << 7
                | p(0, -2) << 8
                | p(-1, -2) << 9
        }
        _ => {
            p(-1, 0)
                | p(-2, 0) << 1
                | p(-3, 0) << 2
                | p(-4, 0) << 3
                | a(0) << 4
                | p(1, -1) << 5
                | p(0, -1) << 6
                | p(-1, -1) << 7
                | p(-2, -1) << 8
                | p(-3, -1) << 9
        }
    }
}

/// A generic region coded with the MQ coder, [T.88] 6.2.5 the other way.
fn encode_generic(
    mq: &mut Mq,
    contexts: &mut [Cx],
    page: &Bilevel,
    template: u8,
    typical: bool,
    at: &[(i64, i64); 4],
) {
    let typical_context = [0x9B25, 0x0795, 0x00E5, 0x0195][usize::from(template)];
    let mut same_before = false;
    for y in 0..page.height as i64 {
        if typical {
            let same = (0..page.width as i64).all(|x| page.get(x, y) == page.get(x, y - 1));
            mq.encode(&mut contexts[typical_context], u8::from(same != same_before));
            same_before = same;
            if same {
                continue;
            }
        }
        for x in 0..page.width as i64 {
            let context = generic_context(page, x, y, template, at);
            mq.encode(&mut contexts[context], page.get(x, y));
        }
    }
}

fn default_at(template: u8) -> [(i64, i64); 4] {
    match template {
        0 => [(3, -1), (-3, -1), (2, -2), (-2, -2)],
        1 => [(3, -1), (0, 0), (0, 0), (0, 0)],
        _ => [(2, -1), (0, 0), (0, 0), (0, 0)],
    }
}

/// A segment: its number, kind, the segments it refers to, its page and
/// its data.
fn segment(number: u32, kind: u8, referred: &[u32], page: u8, data: &[u8]) -> Vec<u8> {
    let mut out = number.to_be_bytes().to_vec();
    out.push(kind);
    out.push((referred.len() as u8) << 5);
    out.extend(referred.iter().map(|&n| n as u8));
    out.push(page);
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(data);
    out
}

fn page_information(width: usize, height: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for value in [width as u32, height as u32, 0, 0] {
        out.extend_from_slice(&value.to_be_bytes());
    }
    out.extend_from_slice(&[0, 0, 0]);
    out
}

fn region_information(width: usize, height: usize, x: i64, y: i64, combine: u8) -> Vec<u8> {
    let mut out = Vec::new();
    for value in [width as u32, height as u32, x as u32, y as u32] {
        out.extend_from_slice(&value.to_be_bytes());
    }
    out.push(combine);
    out
}

fn at_bytes(template: u8, at: &[(i64, i64); 4]) -> Vec<u8> {
    let count = if template == 0 { 4 } else { 1 };
    at.iter().take(count).flat_map(|&(x, y)| [x as i8 as u8, y as i8 as u8]).collect()
}

/// A generic region segment's data.
fn generic_region(page: &Bilevel, template: u8, typical: bool, at: &[(i64, i64); 4]) -> Vec<u8> {
    let mut data = region_information(page.width, page.height, 0, 0, 0);
    data.push((template << 1) | if typical { 8 } else { 0 });
    data.extend(at_bytes(template, at));
    let mut mq = Mq::new();
    let mut contexts = vec![Cx::default(); 1 << 16];
    encode_generic(&mut mq, &mut contexts, page, template, typical, at);
    data.extend(mq.finish());
    data
}

/// A PDF of one JBIG2 picture: the page's segments, and those in the
/// globals stream if any.
fn jbig2_page(width: usize, height: usize, segments: &[u8], globals: Option<&[u8]>) -> Vec<u8> {
    let entries = "/ColorSpace /DeviceGray /BitsPerComponent 1 /Filter /JBIG2Decode";
    let mut pdf = page_with(
        &match globals {
            Some(_) => format!("{entries} /DecodeParms << /JBIG2Globals 6 0 R >>"),
            None => entries.to_owned(),
        },
        segments,
        width,
        height,
    );
    if let Some(globals) = globals {
        let at = pdf.len() - b"trailer << /Root 1 0 R >>\n%%EOF\n".len();
        let mut object = format!("6 0 obj << /Length {} >> stream\n", globals.len()).into_bytes();
        object.extend_from_slice(globals);
        object.extend_from_slice(b"\nendstream endobj\n");
        pdf.splice(at..at, object);
    }
    pdf
}

/// What poppler makes of a PDF's picture. LibreOffice's PDF import runs
/// poppler in a helper of its own, which writes what each page draws, a
/// line a drawing — for a picture, `drawImage` with its width, height,
/// whether it is masked, its format and its length — and the pictures
/// themselves, in the same order, to its error output.
fn poppler_reads(pdf: &[u8], name: &str) -> wp_image::Image {
    let folder = folder(&format!("poppler-{name}"));
    std::fs::write(folder.join("page.pdf"), pdf).unwrap();
    let output = Command::new("/usr/lib/libreoffice/program/xpdfimport")
        .arg(folder.join("page.pdf"))
        .stdin(std::process::Stdio::null())
        .output()
        .expect("LibreOffice's PDF import helper runs");
    let _ = std::fs::remove_dir_all(&folder);
    let out = output.stdout;
    let at = out
        .windows(10)
        .position(|w| w == b"drawImage ")
        .unwrap_or_else(|| panic!("no picture drawn: {}", String::from_utf8_lossy(&out)));
    let end = at + out[at..].iter().position(|&b| b == b'\n').expect("a line");
    let line = String::from_utf8_lossy(&out[at..end]).into_owned();
    let words: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(words.get(4), Some(&"PNG"), "{line}");
    let length: usize = words[5].parse().expect("a length");
    let binary = output.stderr;
    let start = binary
        .windows(8)
        .position(|w| w == b"\x89PNG\r\n\x1a\n")
        .unwrap_or_else(|| panic!("no PNG: {}", String::from_utf8_lossy(&binary)));
    wp_image::decode(&binary[start..start + length]).expect("a picture")
}

#[test]
fn a_jbig2_generic_region_is_read() {
    let page = bilevel(97, 61);
    for template in 0..4u8 {
        for typical in [false, true] {
            let mut at = default_at(template);
            if typical {
                // A moved pixel, to show the context follows it.
                at[0] = if template == 0 { (4, -1) } else { (-2, -2) };
            }
            let mut stream = segment(0, 48, &[], 1, &page_information(97, 61));
            stream.extend(segment(1, 38, &[], 1, &generic_region(&page, template, typical, &at)));
            stream.extend(segment(2, 49, &[], 1, &[]));
            let pdf = jbig2_page(97, 61, &stream, None);
            let read = picture_in(&pdf);
            assert_eq!(
                difference(&read, &page.image()),
                (0, 0),
                "template {template}, typical {typical}"
            );
            let poppler = poppler_reads(&pdf, &format!("generic-{template}-{typical}"));
            assert_eq!(
                difference(&poppler, &page.image()),
                (0, 0),
                "poppler, template {template}, typical {typical}"
            );
        }
    }
}

#[test]
fn a_jbig2_generic_region_coded_as_a_fax_is_read() {
    // The coding is libtiff's, lifted out of the Group 4 TIFF ImageMagick
    // writes.
    let page = bilevel(97, 61);
    let mut data = region_information(97, 61, 0, 0, 0);
    data.push(1);
    data.extend(group_4(&page, "mmr-region"));
    let mut stream = segment(0, 48, &[], 1, &page_information(97, 61));
    stream.extend(segment(1, 38, &[], 1, &data));
    stream.extend(segment(2, 49, &[], 1, &[]));
    let pdf = jbig2_page(97, 61, &stream, None);
    assert_eq!(difference(&picture_in(&pdf), &page.image()), (0, 0));
    assert_eq!(difference(&poppler_reads(&pdf, "mmr"), &page.image()), (0, 0), "poppler");
}

/// A bitmap in Group 4, as libtiff codes it.
fn group_4(bitmap: &Bilevel, name: &str) -> Vec<u8> {
    let folder = folder(name);
    let image = bitmap.image();
    let mut canvas = wp_raster::Canvas::new(image.width, image.height);
    canvas.paste_rect(0, 0, image.width as i32, image.height as i32, &image.pixels);
    std::fs::write(folder.join("bitmap.png"), wp_raster::encode_png(&canvas)).unwrap();
    convert(&folder, &["bitmap.png", "-monochrome", "-compress", "Group4", "coded.tif"]);
    let coded = std::fs::read(folder.join("coded.tif")).unwrap();
    let _ = std::fs::remove_dir_all(&folder);
    let (fields, data) = tiff_strips(&coded);
    assert_eq!(fields[&259][0], 4, "Group 4");
    assert_eq!(fields.get(&262).map_or(0, |v| v[0]), 0, "a nought bit white");
    data
}

fn bits_for(count: usize) -> u32 {
    let mut bits = 0;
    while (1usize << bits) < count {
        bits += 1;
    }
    bits
}

/// Numbers coded with the MQ coder, [T.88] A.2 the other way.
struct IntegerCoder {
    contexts: Vec<Cx>,
}

impl IntegerCoder {
    fn new() -> Self {
        Self { contexts: vec![Cx::default(); 512] }
    }

    fn encode(&mut self, mq: &mut Mq, value: Option<i64>) {
        let (sign, magnitude) = match value {
            None => (1u8, 0u64),
            Some(value) => (u8::from(value < 0), value.unsigned_abs()),
        };
        let mut previous = 1usize;
        let contexts = &mut self.contexts;
        let mut put = |mq: &mut Mq, bit: u8| {
            mq.encode(&mut contexts[previous], bit);
            let next = (previous << 1) | usize::from(bit);
            previous = if previous < 256 { next } else { (next & 511) | 256 };
        };
        put(mq, sign);
        let (prefix, bits, offset): (&[u8], u32, u64) = match magnitude {
            0..=3 => (&[0], 2, 0),
            4..=19 => (&[1, 0], 4, 4),
            20..=83 => (&[1, 1, 0], 6, 20),
            84..=339 => (&[1, 1, 1, 0], 8, 84),
            340..=4435 => (&[1, 1, 1, 1, 0], 12, 340),
            _ => (&[1, 1, 1, 1, 1], 32, 4436),
        };
        for &bit in prefix {
            put(mq, bit);
        }
        for shift in (0..bits).rev() {
            put(mq, (((magnitude - offset) >> shift) & 1) as u8);
        }
    }
}

/// Symbol numbers, so many bits each.
struct IdCoder {
    length: u32,
    contexts: Vec<Cx>,
}

impl IdCoder {
    fn new(length: u32) -> Self {
        Self { length, contexts: vec![Cx::default(); 1 << (length + 1)] }
    }

    fn encode(&mut self, mq: &mut Mq, id: usize) {
        let mut previous = 1usize;
        for shift in (0..self.length).rev() {
            let bit = ((id >> shift) & 1) as u8;
            mq.encode(&mut self.contexts[previous], bit);
            previous = (previous << 1) | usize::from(bit);
        }
    }
}

/// A pixel's context in a refinement, its bits in the standard's order.
fn refinement_context(
    target: &Bilevel,
    reference: &Bilevel,
    (x, y): (i64, i64),
    template: u8,
    at: [(i64, i64); 2],
    (dx, dy): (i64, i64),
) -> usize {
    let p = |a: i64, b: i64| usize::from(target.get(x + a, y + b));
    let r = |a: i64, b: i64| usize::from(reference.get(x - dx + a, y - dy + b));
    if template == 0 {
        p(-1, 0)
            | p(1, -1) << 1
            | p(0, -1) << 2
            | p(at[0].0, at[0].1) << 3
            | r(1, 1) << 4
            | r(0, 1) << 5
            | r(-1, 1) << 6
            | r(1, 0) << 7
            | r(0, 0) << 8
            | r(-1, 0) << 9
            | r(1, -1) << 10
            | r(0, -1) << 11
            | r(at[1].0, at[1].1) << 12
    } else {
        p(-1, 0)
            | p(1, -1) << 1
            | p(0, -1) << 2
            | p(-1, -1) << 3
            | r(1, 1) << 4
            | r(0, 1) << 5
            | r(1, 0) << 6
            | r(0, 0) << 7
            | r(-1, 0) << 8
            | r(0, -1) << 9
    }
}

/// A refinement coded with the MQ coder, [T.88] 6.3.5 the other way.
fn encode_refinement(
    mq: &mut Mq,
    contexts: &mut [Cx],
    target: &Bilevel,
    reference: &Bilevel,
    template: u8,
    offset: (i64, i64),
) {
    let at = [(-1, -1), (-1, -1)];
    for y in 0..target.height as i64 {
        for x in 0..target.width as i64 {
            let context = refinement_context(target, reference, (x, y), template, at, offset);
            mq.encode(&mut contexts[context], target.get(x, y));
        }
    }
}

/// Glyph-like shapes of a few heights, two of them alike in height.
fn glyphs() -> Vec<Bilevel> {
    let shape = |width: usize, height: usize, on: &dyn Fn(usize, usize) -> bool| {
        let mut pixels = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                pixels.push(u8::from(on(x, y)));
            }
        }
        Bilevel { width, height, pixels }
    };
    vec![
        shape(6, 8, &|x, y| x == 0 || x == 5 || y == 0 || y == 7),
        shape(3, 8, &|x, _| x == 1),
        shape(7, 7, &|x, y| {
            let (dx, dy) = (x as i64 - 3, y as i64 - 3);
            (dx * dx + dy * dy - 9).abs() <= 3
        }),
        shape(5, 9, &|x, y| x == (8 - y) / 2),
        shape(2, 2, &|_, _| true),
        shape(9, 3, &|_, y| y == 1),
    ]
}

/// A symbol dictionary's order: by height, and by width within a height.
fn dictionary_order(symbols: &[Bilevel]) -> Vec<Bilevel> {
    let mut ordered = symbols.to_vec();
    ordered.sort_by_key(|symbol| (symbol.height, symbol.width));
    ordered
}

/// A symbol dictionary of generic bitmaps coded with the MQ coder; the
/// symbols in the order given, which is by height.
fn arithmetic_dictionary(symbols: &[Bilevel], template: u8) -> Vec<u8> {
    let mut data = (u16::from(template) << 10).to_be_bytes().to_vec();
    let at = default_at(template);
    data.extend(at_bytes(template, &at));
    data.extend_from_slice(&(symbols.len() as u32).to_be_bytes());
    data.extend_from_slice(&(symbols.len() as u32).to_be_bytes());
    let mut mq = Mq::new();
    let (mut dh, mut dw, mut ex) = (IntegerCoder::new(), IntegerCoder::new(), IntegerCoder::new());
    let mut contexts = vec![Cx::default(); 1 << 16];
    let mut height = 0i64;
    let mut index = 0;
    while index < symbols.len() {
        let class = symbols[index].height;
        dh.encode(&mut mq, Some(class as i64 - height));
        height = class as i64;
        let mut width = 0i64;
        while index < symbols.len() && symbols[index].height == class {
            dw.encode(&mut mq, Some(symbols[index].width as i64 - width));
            width = symbols[index].width as i64;
            encode_generic(&mut mq, &mut contexts, &symbols[index], template, false, &at);
            index += 1;
        }
        dw.encode(&mut mq, None);
    }
    // None of the symbols it was given, and all of its own, offered.
    ex.encode(&mut mq, Some(0));
    ex.encode(&mut mq, Some(symbols.len() as i64));
    data.extend(mq.finish());
    data
}

/// One symbol drawn in a text region: which, where its top left goes, and
/// what it is refined into there, if it is.
#[derive(Clone)]
struct Instance {
    id: usize,
    x: i64,
    y: i64,
    refined: Option<Bilevel>,
}

/// What a text region codes, in order, before it is coded one way or the
/// other.
enum TextCode {
    Dt(i64),
    Fs(i64),
    Ds(Option<i64>),
    It(i64),
    Id(usize),
    Ri(i64),
    Refined { rdw: i64, rdh: i64, rdx: i64, rdy: i64, target: Bilevel, reference: usize },
}

/// How a text region lays its symbols out.
#[derive(Clone, Copy)]
struct Layout {
    log_strips: u32,
    corner: u8,
    transposed: bool,
    offset: i64,
    refine: bool,
    /// The first strip's offset, whatever the table allows.
    first_dt: i64,
}

fn text_codes(instances: &[Instance], symbols: &[Bilevel], layout: Layout) -> Vec<TextCode> {
    let strips = 1i64 << layout.log_strips;
    let right = matches!(layout.corner, 2 | 3);
    let bottom = matches!(layout.corner, 0 | 2);
    let mut placed: Vec<(i64, i64, i64, &Instance)> = instances
        .iter()
        .map(|instance| {
            let bitmap = instance.refined.as_ref().unwrap_or(&symbols[instance.id]);
            let (w, h) = (bitmap.width as i64, bitmap.height as i64);
            let across = if right { instance.x + w - 1 } else { instance.x };
            let down = if bottom { instance.y + h - 1 } else { instance.y };
            let (s, t) = if layout.transposed { (down, across) } else { (across, down) };
            (t.div_euclid(strips) * strips, s, t, instance)
        })
        .collect();
    placed.sort_by_key(|&(strip, s, ..)| (strip, s));
    let mut codes = vec![TextCode::Dt(layout.first_dt)];
    let mut strip_t = -layout.first_dt * strips;
    let (mut first_s, mut current_s) = (0i64, 0i64);
    let mut at = 0;
    while at < placed.len() {
        let strip = placed[at].0;
        codes.push(TextCode::Dt((strip - strip_t) / strips));
        strip_t = strip;
        let mut first = true;
        while at < placed.len() && placed[at].0 == strip {
            let (_, s, t, instance) = placed[at];
            let bitmap = instance.refined.as_ref().unwrap_or(&symbols[instance.id]);
            let (w, h) = (bitmap.width as i64, bitmap.height as i64);
            let before = match (layout.transposed, right, bottom) {
                (false, true, _) => w - 1,
                (true, _, true) => h - 1,
                _ => 0,
            };
            let after = match (layout.transposed, right, bottom) {
                (false, false, _) => w - 1,
                (true, _, false) => h - 1,
                _ => 0,
            };
            let base = s - before;
            if first {
                codes.push(TextCode::Fs(base - first_s));
                first_s = base;
                first = false;
            } else {
                codes.push(TextCode::Ds(Some(base - current_s - layout.offset)));
            }
            if strips > 1 {
                codes.push(TextCode::It(t - strip));
            }
            codes.push(TextCode::Id(instance.id));
            if layout.refine {
                match &instance.refined {
                    Some(target) => {
                        let reference = &symbols[instance.id];
                        codes.push(TextCode::Ri(1));
                        codes.push(TextCode::Refined {
                            rdw: target.width as i64 - reference.width as i64,
                            rdh: target.height as i64 - reference.height as i64,
                            rdx: 0,
                            rdy: 0,
                            target: target.clone(),
                            reference: instance.id,
                        });
                    }
                    None => codes.push(TextCode::Ri(0)),
                }
            }
            current_s = s + after;
            at += 1;
        }
        codes.push(TextCode::Ds(None));
    }
    codes
}

/// The integer coders of a text region.
struct TextCoders {
    dt: IntegerCoder,
    fs: IntegerCoder,
    ds: IntegerCoder,
    it: IntegerCoder,
    ri: IntegerCoder,
    rdw: IntegerCoder,
    rdh: IntegerCoder,
    rdx: IntegerCoder,
    rdy: IntegerCoder,
    id: IdCoder,
    refinement: Vec<Cx>,
}

impl TextCoders {
    fn new(id_length: u32) -> Self {
        Self {
            dt: IntegerCoder::new(),
            fs: IntegerCoder::new(),
            ds: IntegerCoder::new(),
            it: IntegerCoder::new(),
            ri: IntegerCoder::new(),
            rdw: IntegerCoder::new(),
            rdh: IntegerCoder::new(),
            rdx: IntegerCoder::new(),
            rdy: IntegerCoder::new(),
            id: IdCoder::new(id_length),
            refinement: vec![Cx::default(); 1 << 13],
        }
    }

    fn write(&mut self, mq: &mut Mq, codes: &[TextCode], symbols: &[Bilevel], template: u8) {
        for code in codes {
            match code {
                TextCode::Dt(v) => self.dt.encode(mq, Some(*v)),
                TextCode::Fs(v) => self.fs.encode(mq, Some(*v)),
                TextCode::Ds(v) => self.ds.encode(mq, *v),
                TextCode::It(v) => self.it.encode(mq, Some(*v)),
                TextCode::Id(id) => self.id.encode(mq, *id),
                TextCode::Ri(v) => self.ri.encode(mq, Some(*v)),
                TextCode::Refined { rdw, rdh, rdx, rdy, target, reference } => {
                    self.rdw.encode(mq, Some(*rdw));
                    self.rdh.encode(mq, Some(*rdh));
                    self.rdx.encode(mq, Some(*rdx));
                    self.rdy.encode(mq, Some(*rdy));
                    let offset = (rdw.div_euclid(2) + rdx, rdh.div_euclid(2) + rdy);
                    encode_refinement(
                        mq,
                        &mut self.refinement,
                        target,
                        &symbols[*reference],
                        template,
                        offset,
                    );
                }
            }
        }
    }
}

/// A text region's flags.
fn text_flags(layout: Layout, huffman: bool, template: u8) -> u16 {
    u16::from(huffman)
        | u16::from(layout.refine) << 1
        | (layout.log_strips as u16) << 2
        | u16::from(layout.corner) << 4
        | u16::from(layout.transposed) << 6
        | ((layout.offset & 0x1F) as u16) << 10
        | u16::from(template) << 15
}

/// A text region segment coded with the MQ coder.
fn arithmetic_text(
    (width, height): (usize, usize),
    instances: &[Instance],
    symbols: &[Bilevel],
    layout: Layout,
    template: u8,
) -> Vec<u8> {
    let mut data = region_information(width, height, 0, 0, 0);
    data.extend_from_slice(&text_flags(layout, false, template).to_be_bytes());
    if layout.refine && template == 0 {
        data.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
    }
    data.extend_from_slice(&(instances.len() as u32).to_be_bytes());
    let mut mq = Mq::new();
    let mut coders = TextCoders::new(bits_for(symbols.len()));
    coders.write(&mut mq, &text_codes(instances, symbols, layout), symbols, template);
    data.extend(mq.finish());
    data
}

/// The page a text region draws.
fn drawn(width: usize, height: usize, instances: &[Instance], symbols: &[Bilevel]) -> Bilevel {
    let mut page = Bilevel { width, height, pixels: vec![0; width * height] };
    for instance in instances {
        let bitmap = instance.refined.as_ref().unwrap_or(&symbols[instance.id]);
        for y in 0..bitmap.height as i64 {
            for x in 0..bitmap.width as i64 {
                let (px, py) = (instance.x + x, instance.y + y);
                if px >= 0 && py >= 0 && (px as usize) < width && (py as usize) < height {
                    page.pixels[py as usize * width + px as usize] |= bitmap.get(x, y);
                }
            }
        }
    }
    page
}

/// Lines of text: every glyph, across the page and down it.
fn lines_of(symbols: &[Bilevel]) -> Vec<Instance> {
    let mut instances = Vec::new();
    for line in 0..4i64 {
        let mut x = 2 + line * 3;
        for (index, id) in (0..symbols.len()).chain((0..symbols.len()).rev()).enumerate() {
            let baseline = 12 + line * 14;
            let symbol = &symbols[id];
            instances.push(Instance {
                id,
                x,
                y: baseline - symbol.height as i64 + (index as i64 % 2),
                refined: None,
            });
            x += symbol.width as i64 + 1 + (index as i64 % 3);
        }
    }
    instances
}

const TEXT_SIZE: (usize, usize) = (97, 61);

#[test]
fn jbig2_text_is_drawn_with_its_symbols() {
    let symbols = dictionary_order(&glyphs());
    let instances = lines_of(&symbols);
    let expected = drawn(TEXT_SIZE.0, TEXT_SIZE.1, &instances, &symbols);
    for corner in 0..4u8 {
        for transposed in [false, true] {
            let layout = Layout {
                log_strips: u32::from(corner % 3),
                corner,
                transposed,
                offset: if transposed { -2 } else { 1 },
                refine: false,
                first_dt: 0,
            };
            // The dictionary in the globals, as a PDF keeps the symbols
            // pages share.
            let globals =
                segment(1, 0, &[], 0, &arithmetic_dictionary(&symbols, u8::from(transposed) * 2));
            let mut stream = segment(2, 48, &[], 1, &page_information(TEXT_SIZE.0, TEXT_SIZE.1));
            stream.extend(segment(
                3,
                6,
                &[1],
                1,
                &arithmetic_text(TEXT_SIZE, &instances, &symbols, layout, 0),
            ));
            stream.extend(segment(4, 49, &[], 1, &[]));
            let pdf = jbig2_page(TEXT_SIZE.0, TEXT_SIZE.1, &stream, Some(&globals));
            let what = format!("corner {corner}, transposed {transposed}");
            assert_eq!(difference(&picture_in(&pdf), &expected.image()), (0, 0), "{what}");
            let poppler = poppler_reads(&pdf, &format!("text-{corner}-{transposed}"));
            assert_eq!(difference(&poppler, &expected.image()), (0, 0), "poppler, {what}");
        }
    }
}

/// A symbol a little changed, to be refined into.
fn changed(symbol: &Bilevel, grow: bool) -> Bilevel {
    let (width, height) =
        if grow { (symbol.width + 2, symbol.height + 1) } else { (symbol.width, symbol.height) };
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height as i64 {
        for x in 0..width as i64 {
            let source = symbol.get(x - i64::from(grow), y);
            pixels.push(if (x + y) % 5 == 0 { 1 - source } else { source });
        }
    }
    Bilevel { width, height, pixels }
}

#[test]
fn jbig2_symbols_are_refined_where_they_are_drawn() {
    let symbols = dictionary_order(&glyphs());
    let mut instances = lines_of(&symbols);
    for (index, instance) in instances.iter_mut().enumerate() {
        if index % 4 == 1 {
            instance.refined = Some(changed(&symbols[instance.id], index % 8 == 1));
        }
    }
    let expected = drawn(TEXT_SIZE.0, TEXT_SIZE.1, &instances, &symbols);
    for template in 0..2u8 {
        let layout = Layout {
            log_strips: 1,
            corner: 1,
            transposed: false,
            offset: 0,
            refine: true,
            first_dt: 0,
        };
        let mut stream = segment(0, 48, &[], 1, &page_information(TEXT_SIZE.0, TEXT_SIZE.1));
        stream.extend(segment(1, 0, &[], 1, &arithmetic_dictionary(&symbols, 1)));
        stream.extend(segment(
            2,
            6,
            &[1],
            1,
            &arithmetic_text(TEXT_SIZE, &instances, &symbols, layout, template),
        ));
        let pdf = jbig2_page(TEXT_SIZE.0, TEXT_SIZE.1, &stream, None);
        assert_eq!(difference(&picture_in(&pdf), &expected.image()), (0, 0), "template {template}");
        let poppler = poppler_reads(&pdf, &format!("refined-{template}"));
        assert_eq!(difference(&poppler, &expected.image()), (0, 0), "poppler, template {template}");
    }
}

/// A symbol dictionary whose symbols are made of another's: the first a
/// refinement of one of its symbols, the second two of them side by side.
fn aggregate_dictionary(input: &[Bilevel]) -> (Vec<u8>, Vec<Bilevel>) {
    let refined = changed(&input[1], false);
    let pair_parts = [(0usize, 0i64), (2usize, input[0].width as i64 + 1)];
    let pair_width = input[0].width + 1 + input[2].width;
    let pair_height = input[0].height.max(input[2].height);
    let pair_instances: Vec<Instance> =
        pair_parts.iter().map(|&(id, x)| Instance { id, x, y: 0, refined: None }).collect();
    let pair = drawn(pair_width, pair_height, &pair_instances, input);
    let mut new = vec![refined.clone(), pair.clone()];
    new.sort_by_key(|symbol| (symbol.height, symbol.width));
    // Aggregation, refinement template 1: no moveable pixels to send.
    let flags: u16 = 2 | 1 << 12;
    let mut data = flags.to_be_bytes().to_vec();
    data.extend(at_bytes(0, &default_at(0)));
    data.extend_from_slice(&2u32.to_be_bytes());
    data.extend_from_slice(&2u32.to_be_bytes());
    let mut mq = Mq::new();
    let (mut dh, mut dw, mut ex, mut ai) =
        (IntegerCoder::new(), IntegerCoder::new(), IntegerCoder::new(), IntegerCoder::new());
    let id_length = bits_for(input.len() + 2);
    let mut coders = TextCoders::new(id_length);
    let mut symbols_so_far = input.to_vec();
    let mut height = 0i64;
    for symbol in &new {
        dh.encode(&mut mq, Some(symbol.height as i64 - height));
        height = symbol.height as i64;
        dw.encode(&mut mq, Some(symbol.width as i64));
        if *symbol == refined {
            ai.encode(&mut mq, Some(1));
            coders.id.encode(&mut mq, 1);
            coders.rdx.encode(&mut mq, Some(0));
            coders.rdy.encode(&mut mq, Some(0));
            encode_refinement(&mut mq, &mut coders.refinement, symbol, &input[1], 1, (0, 0));
        } else {
            ai.encode(&mut mq, Some(2));
            let layout = Layout {
                log_strips: 0,
                corner: 1,
                transposed: false,
                offset: 0,
                refine: true,
                first_dt: 0,
            };
            let codes = text_codes(&pair_instances, &symbols_so_far, layout);
            coders.write(&mut mq, &codes, &symbols_so_far, 1);
        }
        symbols_so_far.push(symbol.clone());
        dw.encode(&mut mq, None);
    }
    ex.encode(&mut mq, Some(input.len() as i64));
    ex.encode(&mut mq, Some(2));
    data.extend(mq.finish());
    (data, new)
}

#[test]
fn jbig2_symbols_are_made_of_other_symbols() {
    let symbols = dictionary_order(&glyphs());
    let (aggregate, made) = aggregate_dictionary(&symbols);
    // The text draws with both dictionaries' symbols, the first's then the
    // second's.
    let all: Vec<Bilevel> = symbols.iter().chain(&made).cloned().collect();
    let instances: Vec<Instance> = (0..all.len())
        .map(|id| Instance { id, x: 3 + id as i64 * 11, y: 20, refined: None })
        .collect();
    let expected = drawn(TEXT_SIZE.0, TEXT_SIZE.1, &instances, &all);
    let layout = Layout {
        log_strips: 0,
        corner: 1,
        transposed: false,
        offset: 0,
        refine: false,
        first_dt: 0,
    };
    let mut stream = segment(0, 48, &[], 1, &page_information(TEXT_SIZE.0, TEXT_SIZE.1));
    stream.extend(segment(1, 0, &[], 1, &arithmetic_dictionary(&symbols, 0)));
    stream.extend(segment(2, 0, &[1], 1, &aggregate));
    stream.extend(segment(
        3,
        6,
        &[1, 2],
        1,
        &arithmetic_text(TEXT_SIZE, &instances, &all, layout, 0),
    ));
    let pdf = jbig2_page(TEXT_SIZE.0, TEXT_SIZE.1, &stream, None);
    assert_eq!(difference(&picture_in(&pdf), &expected.image()), (0, 0));
    assert_eq!(difference(&poppler_reads(&pdf, "aggregate"), &expected.image()), (0, 0), "poppler");
}

/// Bits written most significant first.
#[derive(Default)]
struct BitWriter {
    out: Vec<u8>,
    used: u32,
}

impl BitWriter {
    fn put(&mut self, value: u64, count: u32) {
        for shift in (0..count).rev() {
            if self.used % 8 == 0 {
                self.out.push(0);
            }
            if (value >> shift) & 1 == 1 {
                *self.out.last_mut().unwrap() |= 0x80 >> (self.used % 8);
            }
            self.used += 1;
        }
    }

    fn align(&mut self) {
        self.used = self.used.div_ceil(8) * 8;
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.align();
        self.out.extend_from_slice(bytes);
        self.used += bytes.len() as u32 * 8;
    }
}

/// A standard Huffman table as the test codes with it: its lines, then
/// its lower and upper ranges and its out-of-band line, if any.
struct HuffmanTable {
    lines: Vec<(u32, u32, i64)>,
    lower: Option<(u32, i64)>,
    upper: (u32, i64),
    out_of_band: Option<u32>,
    codes: Vec<u64>,
}

impl HuffmanTable {
    fn new(
        lines: &[(u32, u32, i64)],
        lower: Option<(u32, i64)>,
        upper: (u32, i64),
        out_of_band: Option<u32>,
    ) -> Self {
        let mut lengths: Vec<u32> = lines.iter().map(|line| line.0).collect();
        lengths.extend(lower.map(|l| l.0));
        lengths.push(upper.0);
        lengths.extend(out_of_band);
        let codes = canonical(&lengths);
        Self { lines: lines.to_vec(), lower, upper, out_of_band, codes }
    }

    fn put(&self, writer: &mut BitWriter, value: Option<i64>) {
        let n = self.lines.len();
        let lower_at = n;
        let upper_at = n + usize::from(self.lower.is_some());
        let Some(value) = value else {
            writer.put(self.codes[upper_at + 1], self.out_of_band.expect("an out-of-band line"));
            return;
        };
        for (index, &(prefix, range, low)) in self.lines.iter().enumerate() {
            if value >= low && value < low + (1i64 << range) {
                writer.put(self.codes[index], prefix);
                writer.put((value - low) as u64, range);
                return;
            }
        }
        match self.lower {
            Some((prefix, low)) if value <= low => {
                writer.put(self.codes[lower_at], prefix);
                writer.put((low - value) as u64, 32);
            }
            _ => {
                writer.put(self.codes[upper_at], self.upper.0);
                writer.put((value - self.upper.1) as u64, 32);
            }
        }
    }
}

/// Prefix codes from their lengths, [T.88] B.3.
fn canonical(lengths: &[u32]) -> Vec<u64> {
    let longest = lengths.iter().copied().max().unwrap_or(0) as usize;
    let mut counts = vec![0u64; longest + 1];
    for &length in lengths {
        counts[length as usize] += 1;
    }
    counts[0] = 0;
    let mut codes = vec![0u64; lengths.len()];
    let mut first = 0u64;
    for length in 1..=longest {
        first = (first + counts[length - 1]) << 1;
        let mut current = first;
        for (index, &line) in lengths.iter().enumerate() {
            if line as usize == length {
                codes[index] = current;
                current += 1;
            }
        }
    }
    codes
}

fn table_b1() -> HuffmanTable {
    HuffmanTable::new(&[(1, 4, 0), (2, 8, 16), (3, 16, 272)], None, (3, 65808), None)
}

fn table_b2() -> HuffmanTable {
    HuffmanTable::new(
        &[(1, 0, 0), (2, 0, 1), (3, 0, 2), (4, 3, 3), (5, 6, 11)],
        None,
        (6, 75),
        Some(6),
    )
}

fn table_b4() -> HuffmanTable {
    HuffmanTable::new(
        &[(1, 0, 1), (2, 0, 2), (3, 0, 3), (4, 3, 4), (5, 6, 12)],
        None,
        (5, 76),
        None,
    )
}

fn table_b6() -> HuffmanTable {
    HuffmanTable::new(
        &[
            (5, 10, -2048),
            (4, 9, -1024),
            (4, 8, -512),
            (4, 7, -256),
            (5, 6, -128),
            (5, 5, -64),
            (4, 5, -32),
            (2, 7, 0),
            (3, 7, 128),
            (3, 8, 256),
            (4, 9, 512),
            (4, 10, 1024),
        ],
        Some((6, -2049)),
        (6, 2048),
        None,
    )
}

fn table_b8() -> HuffmanTable {
    HuffmanTable::new(
        &[
            (8, 3, -15),
            (9, 1, -7),
            (8, 1, -5),
            (9, 0, -3),
            (7, 0, -2),
            (4, 0, -1),
            (2, 1, 0),
            (5, 0, 2),
            (6, 0, 3),
            (3, 4, 4),
            (6, 1, 20),
            (4, 4, 22),
            (4, 5, 38),
            (5, 6, 70),
            (5, 7, 134),
            (6, 7, 262),
            (7, 8, 390),
            (6, 10, 646),
        ],
        Some((9, -16)),
        (9, 1670),
        Some(2),
    )
}

fn table_b11() -> HuffmanTable {
    HuffmanTable::new(
        &[
            (1, 0, 1),
            (2, 1, 2),
            (4, 0, 4),
            (4, 1, 5),
            (5, 1, 7),
            (5, 2, 9),
            (6, 2, 13),
            (7, 2, 17),
            (7, 3, 21),
            (7, 4, 29),
            (7, 5, 45),
            (7, 6, 77),
        ],
        None,
        (7, 141),
        None,
    )
}

fn table_b15() -> HuffmanTable {
    HuffmanTable::new(
        &[
            (7, 4, -24),
            (6, 2, -8),
            (5, 1, -4),
            (4, 0, -2),
            (3, 0, -1),
            (1, 0, 0),
            (3, 0, 1),
            (4, 0, 2),
            (5, 1, 3),
            (6, 2, 5),
            (7, 4, 9),
        ],
        Some((7, -25)),
        (7, 25),
        None,
    )
}

/// A symbol dictionary coded with the standard Huffman tables, each
/// height class's symbols one bitmap side by side: stored as it is where
/// `stored` says so of the class's number, and as Group 4 where not.
fn huffman_dictionary(symbols: &[Bilevel], stored: impl Fn(usize) -> bool) -> Vec<u8> {
    let mut data = 1u16.to_be_bytes().to_vec();
    data.extend_from_slice(&(symbols.len() as u32).to_be_bytes());
    data.extend_from_slice(&(symbols.len() as u32).to_be_bytes());
    let (b1, b2, b4) = (table_b1(), table_b2(), table_b4());
    let mut writer = BitWriter::default();
    let mut height = 0i64;
    let mut index = 0;
    let mut class_number = 0;
    while index < symbols.len() {
        let class = symbols[index].height;
        b4.put(&mut writer, Some(class as i64 - height));
        height = class as i64;
        let mut width = 0i64;
        let first = index;
        while index < symbols.len() && symbols[index].height == class {
            b2.put(&mut writer, Some(symbols[index].width as i64 - width));
            width = symbols[index].width as i64;
            index += 1;
        }
        b2.put(&mut writer, None);
        let total: usize = symbols[first..index].iter().map(|s| s.width).sum();
        let mut collective =
            Bilevel { width: total, height: class, pixels: vec![0; total * class] };
        let mut x = 0;
        for symbol in &symbols[first..index] {
            for y in 0..class {
                for column in 0..symbol.width {
                    collective.pixels[y * total + x + column] = symbol.get(column as i64, y as i64);
                }
            }
            x += symbol.width;
        }
        if stored(class_number) {
            b1.put(&mut writer, Some(0));
            let row_bytes = total.div_ceil(8);
            let mut rows = vec![0u8; row_bytes * class];
            for y in 0..class {
                for x in 0..total {
                    if collective.pixels[y * total + x] == 1 {
                        rows[y * row_bytes + x / 8] |= 0x80 >> (x % 8);
                    }
                }
            }
            writer.bytes(&rows);
        } else {
            let coded = group_4(&collective, &format!("collective-{class_number}"));
            b1.put(&mut writer, Some(coded.len() as i64));
            writer.bytes(&coded);
        }
        class_number += 1;
    }
    b1.put(&mut writer, Some(0));
    b1.put(&mut writer, Some(symbols.len() as i64));
    data.extend(writer.out);
    data
}

/// A text region coded with the standard Huffman tables, its refinements
/// with the MQ coder: all of them standard, or the first symbol of each
/// strip coded with a table the file sends — a copy of B.6, the standard
/// one it would otherwise be.
fn huffman_text(
    (width, height): (usize, usize),
    instances: &[Instance],
    symbols: &[Bilevel],
    layout: Layout,
    own_table: bool,
) -> Vec<u8> {
    let mut data = region_information(width, height, 0, 0, 0);
    data.extend_from_slice(&text_flags(layout, true, 1).to_be_bytes());
    // B.15 for the refinements' numbers.
    let huffman_flags: u16 =
        (u16::from(own_table) * 3) | (1 << 6) | (1 << 8) | (1 << 10) | (1 << 12);
    data.extend_from_slice(&huffman_flags.to_be_bytes());
    data.extend_from_slice(&(instances.len() as u32).to_be_bytes());
    let mut writer = BitWriter::default();
    // Every symbol's code the same length: the code lengths' own codes
    // give that length the one code there is, a single nought.
    let length = bits_for(symbols.len()).max(1);
    for index in 0..35u32 {
        writer.put(u64::from(index == length), 4);
    }
    for _ in symbols {
        writer.put(0, 1);
    }
    writer.align();
    let (b1, b6, b8, b11, b15) = (table_b1(), table_b6(), table_b8(), table_b11(), table_b15());
    let mut refinement = vec![Cx::default(); 1 << 13];
    for code in text_codes(instances, symbols, layout) {
        match code {
            TextCode::Dt(v) => b11.put(&mut writer, Some(v)),
            TextCode::Fs(v) => b6.put(&mut writer, Some(v)),
            TextCode::Ds(v) => b8.put(&mut writer, v),
            TextCode::It(v) => writer.put(v as u64, layout.log_strips),
            TextCode::Id(id) => writer.put(id as u64, length),
            TextCode::Ri(v) => writer.put(v as u64, 1),
            TextCode::Refined { rdw, rdh, rdx, rdy, target, reference } => {
                for value in [rdw, rdh, rdx, rdy] {
                    b15.put(&mut writer, Some(value));
                }
                let mut mq = Mq::new();
                let offset = (rdw.div_euclid(2) + rdx, rdh.div_euclid(2) + rdy);
                encode_refinement(
                    &mut mq,
                    &mut refinement,
                    &target,
                    &symbols[reference],
                    1,
                    offset,
                );
                let coded = mq.finish();
                b1.put(&mut writer, Some(coded.len() as i64));
                writer.bytes(&coded);
            }
        }
    }
    data.extend(writer.out);
    data
}

/// Table B.6 as a table segment sends it: three bits for a prefix length,
/// four for a range length, from -2048 to 2048.
fn own_table_b6() -> Vec<u8> {
    let mut data = vec![(2 << 1) | (3 << 4)];
    data.extend_from_slice(&(-2048i32).to_be_bytes());
    data.extend_from_slice(&2048i32.to_be_bytes());
    let mut writer = BitWriter::default();
    for (prefix, range) in [
        (5, 10),
        (4, 9),
        (4, 8),
        (4, 7),
        (5, 6),
        (5, 5),
        (4, 5),
        (2, 7),
        (3, 7),
        (3, 8),
        (4, 9),
        (4, 10),
    ] {
        writer.put(prefix, 3);
        writer.put(range, 4);
    }
    // The lower range, then the upper.
    writer.put(6, 3);
    writer.put(6, 3);
    data.extend(writer.out);
    data
}

/// The page a Huffman-coded dictionary and text region draw, the text's
/// symbols refined here and there; and the PDF of it.
fn huffman_page(stored: impl Fn(usize) -> bool, own_table: bool) -> (Vec<u8>, Bilevel) {
    let symbols = dictionary_order(&glyphs());
    let mut instances = lines_of(&symbols);
    for (index, instance) in instances.iter_mut().enumerate() {
        if index % 5 == 2 {
            instance.refined = Some(changed(&symbols[instance.id], index % 10 == 2));
        }
    }
    let expected = drawn(TEXT_SIZE.0, TEXT_SIZE.1, &instances, &symbols);
    // Table B.11 has no nought, so the strips start one before the first.
    let layout = Layout {
        log_strips: 2,
        corner: 1,
        transposed: false,
        offset: 0,
        refine: true,
        first_dt: 1,
    };
    let mut stream = segment(0, 48, &[], 1, &page_information(TEXT_SIZE.0, TEXT_SIZE.1));
    stream.extend(segment(1, 0, &[], 1, &huffman_dictionary(&symbols, stored)));
    stream.extend(segment(2, 53, &[], 1, &own_table_b6()));
    let text = huffman_text(TEXT_SIZE, &instances, &symbols, layout, own_table);
    stream.extend(segment(3, 6, if own_table { &[1, 2] } else { &[1] }, 1, &text));
    (jbig2_page(TEXT_SIZE.0, TEXT_SIZE.1, &stream, None), expected)
}

#[test]
fn huffman_coded_jbig2_text_is_read() {
    let (pdf, expected) = huffman_page(|_| false, false);
    assert_eq!(difference(&picture_in(&pdf), &expected.image()), (0, 0));
    assert_eq!(difference(&poppler_reads(&pdf, "huffman"), &expected.image()), (0, 0), "poppler");
}

#[test]
fn a_jbig2_text_region_may_send_its_own_tables() {
    let (pdf, expected) = huffman_page(|_| false, true);
    assert_eq!(difference(&picture_in(&pdf), &expected.image()), (0, 0));
    assert_eq!(difference(&poppler_reads(&pdf, "own-table"), &expected.image()), (0, 0), "poppler");
}

#[test]
fn a_huffman_coded_height_class_may_be_stored_as_it_is() {
    // Poppler draws nothing from a dictionary with a height class stored
    // uncoded, the bytes of its rows straight after its size of nought;
    // jbig2dec reads such a dictionary to the page this reader does, the
    // same page as the dictionary coded any other way.
    let (pdf, expected) = huffman_page(|class| class % 2 == 0, false);
    assert_eq!(difference(&picture_in(&pdf), &expected.image()), (0, 0));
}

#[test]
fn a_jbig2_page_of_stripes_takes_up_a_region_kept_for_later() {
    // A page whose height its last stripe says; an intermediate generic
    // region, kept, and then a refinement of it put onto the page.
    let region = bilevel(60, 30);
    let refined = changed(&region, false);
    let (x, y) = (20usize, 17usize);
    let height = 50usize;
    let mut expected = Bilevel { width: 97, height, pixels: vec![0; 97 * height] };
    for row in 0..30 {
        for column in 0..60 {
            expected.pixels[(y + row) * 97 + x + column] = refined.get(column as i64, row as i64);
        }
    }
    let mut page = page_information(97, 0);
    page[4..8].copy_from_slice(&u32::MAX.to_be_bytes());
    page[17..19].copy_from_slice(&(0x8000u16 | 64).to_be_bytes());
    let mut generic = generic_region(&region, 1, false, &default_at(1));
    generic[8..16].copy_from_slice(&[0, 0, 0, x as u8, 0, 0, 0, y as u8]);
    let mut refinement = region_information(60, 30, x as i64, y as i64, 0);
    refinement.push(1);
    let mut mq = Mq::new();
    let mut contexts = vec![Cx::default(); 1 << 13];
    encode_refinement(&mut mq, &mut contexts, &refined, &region, 1, (0, 0));
    refinement.extend(mq.finish());
    let mut stream = segment(0, 48, &[], 1, &page);
    stream.extend(segment(1, 36, &[], 1, &generic));
    stream.extend(segment(2, 42, &[1], 1, &refinement));
    stream.extend(segment(3, 50, &[], 1, &(height as u32 - 1).to_be_bytes()));
    stream.extend(segment(4, 49, &[], 1, &[]));
    let pdf = jbig2_page(97, height, &stream, None);
    assert_eq!(difference(&picture_in(&pdf), &expected.image()), (0, 0));
    assert_eq!(difference(&poppler_reads(&pdf, "stripes"), &expected.image()), (0, 0), "poppler");
}

#[test]
fn a_jbig2_halftone_is_drawn_with_its_patterns() {
    // Four patterns of four by four, darker each; a grid of cells whose
    // values run across and down.
    let (size, count) = (4usize, 4usize);
    let patterns: Vec<Bilevel> = (0..count)
        .map(|level| Bilevel {
            width: size,
            height: size,
            pixels: (0..size * size).map(|i| u8::from((i * 7) % 16 < level * 5)).collect(),
        })
        .collect();
    let mut collective =
        Bilevel { width: size * count, height: size, pixels: vec![0; size * count * size] };
    for (index, pattern) in patterns.iter().enumerate() {
        for y in 0..size {
            for x in 0..size {
                collective.pixels[y * size * count + index * size + x] =
                    pattern.get(x as i64, y as i64);
            }
        }
    }
    let mut dictionary = vec![0u8, size as u8, size as u8];
    dictionary.extend_from_slice(&(count as u32 - 1).to_be_bytes());
    let mut mq = Mq::new();
    let mut contexts = vec![Cx::default(); 1 << 16];
    let at = [(-(size as i64), 0), (-3, -1), (2, -2), (-2, -2)];
    encode_generic(&mut mq, &mut contexts, &collective, 0, false, &at);
    dictionary.extend(mq.finish());

    let (grid_width, grid_height) = (24usize, 15usize);
    let value = |m: usize, n: usize| (m / 3 + n / 5) % count;
    let (width, height) = (grid_width * size, grid_height * size);
    let mut expected = Bilevel { width, height, pixels: vec![0; width * height] };
    for m in 0..grid_height {
        for n in 0..grid_width {
            let pattern = &patterns[value(m, n)];
            for y in 0..size {
                for x in 0..size {
                    expected.pixels[(m * size + y) * width + n * size + x] =
                        pattern.get(x as i64, y as i64);
                }
            }
        }
    }
    let mut region = region_information(width, height, 0, 0, 0);
    region.push(0);
    for word in [grid_width as u32, grid_height as u32, 0, 0] {
        region.extend_from_slice(&word.to_be_bytes());
    }
    region.extend_from_slice(&((size as u16) << 8).to_be_bytes());
    region.extend_from_slice(&0u16.to_be_bytes());
    // The grey values a plane at a time, most significant first, each
    // after it the difference from the one above.
    let planes = bits_for(count);
    let mut mq = Mq::new();
    let mut contexts = vec![Cx::default(); 1 << 16];
    let at = [(3, -1), (-3, -1), (2, -2), (-2, -2)];
    for plane in (0..planes).rev() {
        let bit = |m: usize, n: usize, p: u32| ((value(m, n) >> p) & 1) as u8;
        let mut pixels = Vec::with_capacity(grid_width * grid_height);
        for m in 0..grid_height {
            for n in 0..grid_width {
                let above = if plane + 1 < planes { bit(m, n, plane + 1) } else { 0 };
                pixels.push(bit(m, n, plane) ^ above);
            }
        }
        let bitmap = Bilevel { width: grid_width, height: grid_height, pixels };
        encode_generic(&mut mq, &mut contexts, &bitmap, 0, false, &at);
    }
    region.extend(mq.finish());
    let mut stream = segment(0, 48, &[], 1, &page_information(width, height));
    stream.extend(segment(1, 16, &[], 1, &dictionary));
    stream.extend(segment(2, 22, &[1], 1, &region));
    let pdf = jbig2_page(width, height, &stream, None);
    assert_eq!(difference(&picture_in(&pdf), &expected.image()), (0, 0));
    assert_eq!(difference(&poppler_reads(&pdf, "halftone"), &expected.image()), (0, 0), "poppler");
}

#[test]
fn a_jbig2_page_is_refined_where_it_stands() {
    // A generic region, then part of the page refined into something a
    // little different.
    let page = bilevel(97, 61);
    let (x, y, w, h) = (20usize, 10usize, 40usize, 30usize);
    let before = page.part(x, y, w, h);
    let after = changed(&before, false);
    let mut expected = page.clone();
    for row in 0..h {
        for column in 0..w {
            expected.pixels[(y + row) * 97 + x + column] = after.get(column as i64, row as i64);
        }
    }
    // Replacing what it refines, which the region says: its operator is
    // how it goes onto the page, as any region does.
    let mut refinement = region_information(w, h, x as i64, y as i64, 4);
    refinement.push(0);
    refinement.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
    let mut mq = Mq::new();
    let mut contexts = vec![Cx::default(); 1 << 13];
    encode_refinement(&mut mq, &mut contexts, &after, &before, 0, (0, 0));
    refinement.extend(mq.finish());
    let mut stream = segment(0, 48, &[], 1, &page_information(97, 61));
    stream.extend(segment(1, 38, &[], 1, &generic_region(&page, 0, true, &default_at(0))));
    stream.extend(segment(2, 42, &[], 1, &refinement));
    let pdf = jbig2_page(97, 61, &stream, None);
    assert_eq!(difference(&picture_in(&pdf), &expected.image()), (0, 0));
    assert_eq!(
        difference(&poppler_reads(&pdf, "page-refined"), &expected.image()),
        (0, 0),
        "poppler"
    );
}

/// A PDF of one page whose content is given.
fn page_of_content(content: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut pdf = b"%PDF-1.5\n".to_vec();
    pdf.extend_from_slice(b"1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n");
    pdf.extend_from_slice(b"2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n");
    pdf.extend_from_slice(
        format!("3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Contents 4 0 R >> endobj\n")
            .as_bytes(),
    );
    pdf.extend_from_slice(format!("4 0 obj << /Length {} >> stream\n", content.len()).as_bytes());
    pdf.extend_from_slice(content);
    pdf.extend_from_slice(b"\nendstream endobj\ntrailer << /Root 1 0 R >>\n%%EOF\n");
    pdf
}

#[test]
fn an_inline_picture_is_read() {
    let (width, height) = (9usize, 7usize);
    // Samples that spell "EI " here and there, which must not end the data.
    let mut samples = Vec::new();
    for index in 0..width * height {
        let pixel: [u8; 3] = if index % 4 == 1 { *b"EI " } else { [index as u8 * 3, 200, 17] };
        samples.extend_from_slice(&pixel);
    }
    let expected = wp_image::Image {
        width,
        height,
        pixels: samples.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
    };
    let mut content =
        format!("q {width} 0 0 {height} 0 0 cm BI /W {width} /H {height} /BPC 8 /CS /RGB ID ")
            .into_bytes();
    content.extend_from_slice(&samples);
    content.extend_from_slice(b"\nEI Q");
    let read = picture_in(&page_of_content(&content, width, height));
    assert_eq!(difference(&read, &expected), (0, 0));

    let hex: String = samples.iter().map(|b| format!("{b:02X}")).collect();
    let content = format!(
        "q {width} 0 0 {height} 0 0 cm BI /W {width} /H {height} /BPC 8 /CS /RGB /F /AHx ID {hex}> EI Q"
    );
    let read = picture_in(&page_of_content(content.as_bytes(), width, height));
    assert_eq!(difference(&read, &expected), (0, 0), "filtered");
}
