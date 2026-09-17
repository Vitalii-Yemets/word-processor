//! Pages drawn here, held against pages Word drew.
//!
//! # A score, not a pass
//!
//! Two renderings of the same page are never identical. Fonts are hinted,
//! edges are softened, a letter lands a third of a pixel to the left: a
//! comparison that demanded equality would fail on every document forever and
//! tell nobody anything. What is wanted is a number that goes up — one that
//! says a page is nine tenths right today and was eight tenths right last
//! month, and which page is the worst.
//!
//! So each page is reduced to ink: how dark each pixel is, white being none.
//! Two pages are then compared by how much of their ink falls in the same
//! places — the ink they agree on over the ink either of them has. Identical
//! pages score one. A blank page against a written one scores nothing. A page
//! whose every letter is a pixel to the left scores badly, which is why the
//! same comparison is made a second time in squares a few pixels across: the
//! first number says how exactly the drawing lands, the second whether the
//! right things are in the right places at all.
//!
//! # Where Word's pages come from
//!
//! From whoever runs this. They cannot be committed for the same reason the
//! documents cannot — see [`crate::corpus`] — so they live beside them, in
//! `corpus/reference`, in a folder named after the document: the images for
//! `corpus/letters/report.docx` go in `corpus/reference/letters/report/`.
//! Any picture format this program reads will do, and they are paired with
//! the pages in the order their names sort by number, so `page-1.png`,
//! `page-01.png` and `1.png` all work.
//!
//! The pages are drawn at whatever resolution the first reference image is,
//! worked back from how wide it is: comparing a page drawn at ninety-six dots
//! to the inch with one exported at two hundred would be measuring the
//! resampling and not the rendering.
//!
//! # What is tracked
//!
//! Every run appends a line to `corpus/fidelity.log` — when, which commit,
//! how many documents and pages, and the two scores — and prints the change
//! since the run before. That file is the point of the exercise. A number
//! that is not written down is a number nobody can see improve.

use std::path::{Path, PathBuf};

use wp_layout::{FontLibrary, LayoutEngine, Renderer};
use wp_raster::{Canvas, Color};

/// The folder inside the corpus where Word's own pages are kept.
pub const REFERENCE: &str = "reference";

/// And the file the scores are written to, run after run.
pub const HISTORY: &str = "fidelity.log";

/// The resolution a document is first laid out at, only so that how wide a
/// page comes out can be measured and the real resolution worked back from
/// the reference image.
const PROBE_DPI: f32 = 96.0;

/// How many pixels across the squares of the tolerant comparison are.
const CELL: usize = 4;

/// How dark every pixel of a page is, white being none.
#[derive(Clone, Debug)]
pub struct Ink {
    pub width: usize,
    pub height: usize,
    values: Vec<f32>,
}

impl Ink {
    /// A page with nothing on it.
    #[must_use]
    pub fn blank(width: usize, height: usize) -> Self {
        Self { width, height, values: vec![0.0; width * height] }
    }

    /// Ink from four-bytes-a-pixel colour, laid over white.
    ///
    /// Over white because that is what paper is: a reference image saved with
    /// an alpha channel and one saved without are the same page, and reading
    /// the transparent one as black would make them opposites.
    #[must_use]
    pub fn from_pixels(width: usize, height: usize, rgba: &[u8]) -> Self {
        let mut values = vec![0.0f32; width * height];
        for (value, pixel) in values.iter_mut().zip(rgba.chunks_exact(4)) {
            let alpha = f32::from(pixel[3]) / 255.0;
            let over_white = |channel: u8| f32::from(channel) * alpha + 255.0 * (1.0 - alpha);
            // The eye's own weighting, so that yellow text counts for less
            // than black text, as it looks.
            let light = 0.299 * over_white(pixel[0])
                + 0.587 * over_white(pixel[1])
                + 0.114 * over_white(pixel[2]);
            *value = (1.0 - light / 255.0).clamp(0.0, 1.0);
        }
        Self { width, height, values }
    }

    /// Ink from a page this program drew.
    #[must_use]
    pub fn from_canvas(canvas: &Canvas) -> Self {
        Self::from_pixels(canvas.pixel_width(), canvas.pixel_height(), canvas.pixels())
    }

    /// Ink from a picture somebody else drew.
    #[must_use]
    pub fn from_image(image: &wp_image::Image) -> Self {
        Self::from_pixels(image.width, image.height, &image.pixels)
    }

    /// How much ink there is in all.
    #[must_use]
    pub fn sum(&self) -> f64 {
        self.values.iter().map(|value| f64::from(*value)).sum()
    }

    /// The same page at another size, every destination pixel the average of
    /// the source pixels it covers.
    ///
    /// Averaged rather than sampled because ink must not be lost: taking the
    /// nearest pixel of a page shrunk by half throws away half the letters
    /// and would score a perfect rendering at a half.
    #[must_use]
    pub fn resampled(&self, width: usize, height: usize) -> Self {
        if width == self.width && height == self.height {
            return self.clone();
        }
        if width == 0 || height == 0 || self.width == 0 || self.height == 0 {
            return Self::blank(width, height);
        }

        let mut values = vec![0.0f32; width * height];
        #[allow(clippy::cast_precision_loss)]
        let (across, down) = (self.width as f32 / width as f32, self.height as f32 / height as f32);
        for y in 0..height {
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            let top = (y as f32 * down) as usize;
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            let bottom = (((y + 1) as f32 * down).ceil() as usize).clamp(top + 1, self.height);
            for x in 0..width {
                #[allow(
                    clippy::cast_precision_loss,
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss
                )]
                let left = (x as f32 * across) as usize;
                #[allow(
                    clippy::cast_precision_loss,
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss
                )]
                let right = (((x + 1) as f32 * across).ceil() as usize).clamp(left + 1, self.width);
                let mut total = 0.0f32;
                let mut counted = 0usize;
                for row in top..bottom {
                    for column in left..right {
                        total += self.values[row * self.width + column];
                        counted += 1;
                    }
                }
                #[allow(clippy::cast_precision_loss)]
                if counted > 0 {
                    values[y * width + x] = total / counted as f32;
                }
            }
        }
        Self { width, height, values }
    }

    /// The same page measured in small squares instead of in pixels.
    ///
    /// This is the tolerant comparison, and it is a coarser question rather
    /// than a softer one. Softening does not forgive a letter that sits a
    /// pixel low: the disagreement moves to the softened edges and the ratio
    /// comes out where it started, which is what the numbers said when it was
    /// tried. Asking instead how much ink falls in each little square of the
    /// page does forgive it, because a letter a pixel out is still in the
    /// same square — and a letter in the wrong place, or missing, is not.
    /// The squares are then spread a little into their neighbours, because a
    /// letter that falls on the line between two of them would otherwise be
    /// counted wrong in both.
    #[must_use]
    pub fn coarse(&self) -> Self {
        self.resampled((self.width / CELL).max(1), (self.height / CELL).max(1)).smoothed()
    }

    /// Every square averaged with the eight around it.
    ///
    /// Cheap, because by the time this is reached a page is a sixteenth of
    /// the pixels it was.
    #[must_use]
    fn smoothed(&self) -> Self {
        let mut values = vec![0.0f32; self.width * self.height];
        for y in 0..self.height {
            for x in 0..self.width {
                let mut total = 0.0f32;
                let mut counted = 0usize;
                for row in y.saturating_sub(1)..(y + 2).min(self.height) {
                    for column in x.saturating_sub(1)..(x + 2).min(self.width) {
                        total += self.values[row * self.width + column];
                        counted += 1;
                    }
                }
                #[allow(clippy::cast_precision_loss)]
                if counted > 0 {
                    values[y * self.width + x] = total / counted as f32;
                }
            }
        }
        Self { width: self.width, height: self.height, values }
    }

    /// How much of two pages' ink falls in the same places.
    ///
    /// Compared at the smaller of the two sizes, so that neither page is ever
    /// blown up: enlarging one to meet the other invents detail it does not
    /// have and then marks it wrong for not matching detail the other does.
    #[must_use]
    pub fn against(&self, other: &Self) -> Agreement {
        let (width, height) = (self.width.min(other.width), self.height.min(other.height));
        let (mine, theirs) = (self.resampled(width, height), other.resampled(width, height));
        let mut agreed = 0.0f64;
        let mut total = 0.0f64;
        for (mine, theirs) in mine.values.iter().zip(&theirs.values) {
            agreed += f64::from(mine.min(*theirs));
            total += f64::from(mine.max(*theirs));
        }
        Agreement { agreed, total }
    }
}

/// Ink two pages agree on, over ink either of them has.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Agreement {
    pub agreed: f64,
    pub total: f64,
}

impl Agreement {
    /// Ink one page has and the other does not: a page missing altogether, or
    /// one drawn where the reference has none.
    #[must_use]
    pub fn none_of(ink: f64) -> Self {
        Self { agreed: 0.0, total: ink }
    }

    /// The score, where two blank pages agree perfectly.
    #[must_use]
    pub fn score(self) -> f32 {
        if self.total <= f64::EPSILON {
            return 1.0;
        }
        #[allow(clippy::cast_possible_truncation)]
        let score = (self.agreed / self.total) as f32;
        score.clamp(0.0, 1.0)
    }

    /// The score as a percentage, which is how it is read.
    #[must_use]
    pub fn percent(self) -> f32 {
        self.score() * 100.0
    }

    /// Adds another page's worth.
    pub fn add(&mut self, other: Self) {
        self.agreed += other.agreed;
        self.total += other.total;
    }
}

/// One page held against one reference image.
#[derive(Clone, Debug)]
pub struct PageScore {
    /// Which page, counting from one.
    pub number: usize,
    /// How exactly the drawing lands.
    pub exact: Agreement,
    /// And whether the right things are in the right places at all.
    pub tolerant: Agreement,
    /// Said when there is no pair: a page with no reference, or a reference
    /// with no page.
    pub note: Option<&'static str>,
}

/// One document's pages held against Word's.
#[derive(Clone, Debug)]
pub struct Score {
    /// How many pages this program laid the document out into.
    pub ours: usize,
    /// And how many reference images there are.
    pub theirs: usize,
    pub pages: Vec<PageScore>,
    pub exact: Agreement,
    pub tolerant: Agreement,
}

impl Score {
    /// The page that came out worst, which is where to look first.
    #[must_use]
    pub fn worst(&self) -> Option<&PageScore> {
        self.pages
            .iter()
            .min_by(|left, right| left.tolerant.score().total_cmp(&right.tolerant.score()))
    }
}

/// What came of one document.
#[derive(Clone, Debug)]
pub enum Judgement {
    /// It was compared.
    Scored(Box<Score>),
    /// Nobody has put Word's pages for it in the reference folder. Not a
    /// failure and not a zero: a document nobody has a reference for says
    /// nothing about the rendering, and scoring it at nothing would drag an
    /// honest average down to meaninglessness.
    NoReference,
    /// It would not open, or would not lay out, or a reference image would
    /// not decode.
    Failed(String),
}

/// One document and what came of it.
#[derive(Clone, Debug)]
pub struct Verdict {
    pub path: PathBuf,
    pub judgement: Judgement,
}

/// Where the reference images for a document belong.
#[must_use]
pub fn reference_folder(corpus: &Path, document: &Path) -> PathBuf {
    let relative = document.strip_prefix(corpus).unwrap_or(document);
    corpus.join(REFERENCE).join(relative.with_extension(""))
}

/// The reference images in a folder, in page order.
///
/// Ordered by the number in the name rather than by the name, so that page
/// ten comes after page nine and not after page one.
#[must_use]
pub fn reference_images(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut found: Vec<(u64, PathBuf)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| !name.starts_with('.'))
        })
        .map(|path| (number_in(&path), path))
        .collect();
    found.sort();
    found.into_iter().map(|(_, path)| path).collect()
}

/// The last run of digits in a file's name, which is its page number.
fn number_in(path: &Path) -> u64 {
    let name = path.file_stem().and_then(|name| name.to_str()).unwrap_or_default();
    let digits: String = name
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse().unwrap_or(u64::MAX)
}

/// Compares one document's pages with the reference images for it.
pub fn judge(library: &FontLibrary, corpus: &Path, document: &Path) -> Judgement {
    let folder = reference_folder(corpus, document);
    let references = reference_images(&folder);
    if references.is_empty() {
        return Judgement::NoReference;
    }

    let bytes = match std::fs::read(document) {
        Ok(bytes) => bytes,
        Err(error) => return Judgement::Failed(error.to_string()),
    };
    let opened = match crate::open_bytes(document, &bytes) {
        Ok(opened) => opened,
        Err(why) => return Judgement::Failed(why),
    };

    // Word's pages decide the resolution: the first one says how many pixels
    // a page of this document is supposed to be across, and the rest follow.
    let theirs: Vec<Ink> = {
        let mut inks = Vec::with_capacity(references.len());
        for path in &references {
            match std::fs::read(path).map_err(|error| error.to_string()).and_then(|bytes| {
                wp_image::decode(&bytes).map_err(|error| format!("{}: {error:?}", path.display()))
            }) {
                Ok(image) => inks.push(Ink::from_image(&image)),
                Err(why) => return Judgement::Failed(why),
            }
        }
        inks
    };

    let mut engine = LayoutEngine::new(library).with_dpi(PROBE_DPI);
    let probe = engine.layout_document(&opened);
    let Some(first) = probe.first() else {
        return Judgement::Failed("the document laid out into no pages at all".to_owned());
    };
    // Rounded to a whole number of dots to the inch, because a page is laid
    // out at the resolution it is drawn at and a fiftieth of a dot is enough
    // to move a word onto the next line. Whoever exported the reference chose
    // a round number; this finds it rather than a value a pixel of rounding
    // away from it.
    #[allow(clippy::cast_precision_loss)]
    let dpi = if first.width > 0.5 {
        (PROBE_DPI * theirs[0].width as f32 / first.width).round()
    } else {
        PROBE_DPI
    };
    engine.set_dpi(dpi);
    let pages = engine.layout_document(&opened);

    let mut renderer = Renderer::new(library);
    let mut score = Score {
        ours: pages.len(),
        theirs: theirs.len(),
        pages: Vec::new(),
        exact: Agreement::default(),
        tolerant: Agreement::default(),
    };
    for number in 0..pages.len().max(theirs.len()) {
        let page = match (pages.get(number), theirs.get(number)) {
            (Some(ours), Some(reference)) => {
                let drawn = Ink::from_canvas(&renderer.render(ours, Color::WHITE));
                PageScore {
                    number: number + 1,
                    exact: reference.against(&drawn),
                    tolerant: reference.coarse().against(&drawn.coarse()),
                    note: None,
                }
            }
            // A page with nothing to compare it with counts as ink nobody
            // agreed on, which is what a missing page costs.
            (Some(ours), None) => {
                let drawn = Ink::from_canvas(&renderer.render(ours, Color::WHITE));
                let missed = Agreement::none_of(drawn.sum());
                PageScore {
                    number: number + 1,
                    exact: missed,
                    tolerant: missed,
                    note: Some("no reference for this page"),
                }
            }
            (None, Some(reference)) => {
                let missed = Agreement::none_of(reference.sum());
                PageScore {
                    number: number + 1,
                    exact: missed,
                    tolerant: missed,
                    note: Some("this page was never laid out"),
                }
            }
            (None, None) => continue,
        };
        score.exact.add(page.exact);
        score.tolerant.add(page.tolerant);
        score.pages.push(page);
    }

    Judgement::Scored(Box::new(score))
}

/// Compares every document in a corpus with the reference pages for it.
#[must_use]
pub fn run(corpus: &Path) -> Vec<Verdict> {
    let library = FontLibrary::scan_system();
    crate::corpus::documents(corpus)
        .into_iter()
        .map(|path| {
            let judgement = if library.is_empty() {
                Judgement::Failed("no usable fonts were found on this machine".to_owned())
            } else {
                judge(&library, corpus, &path)
            };
            Verdict { path, judgement }
        })
        .collect()
}

/// The scores over every document that had a reference.
#[must_use]
pub fn total(verdicts: &[Verdict]) -> (Agreement, Agreement, usize, usize) {
    let (mut exact, mut tolerant) = (Agreement::default(), Agreement::default());
    let (mut documents, mut pages) = (0usize, 0usize);
    for verdict in verdicts {
        if let Judgement::Scored(score) = &verdict.judgement {
            exact.add(score.exact);
            tolerant.add(score.tolerant);
            documents += 1;
            pages += score.pages.len();
        }
    }
    (exact, tolerant, documents, pages)
}

/// The line this run leaves behind in the history.
#[must_use]
pub fn history_line(verdicts: &[Verdict], stamp: &str, commit: &str) -> String {
    let (exact, tolerant, documents, pages) = total(verdicts);
    format!(
        "{stamp}  {commit:<8}  {}  {}  tolerant {:.1}%  exact {:.1}%",
        plural(documents, "document"),
        plural(pages, "page"),
        tolerant.percent(),
        exact.percent()
    )
}

/// The score a history line records, for saying what has changed since.
#[must_use]
pub fn score_in(line: &str) -> Option<f32> {
    let after = line.split("tolerant ").nth(1)?;
    after.trim_start().trim_end_matches('%').split('%').next()?.trim().parse().ok()
}

/// Which commit this is, read out of the repository rather than asked of a
/// program this container may not have.
#[must_use]
pub fn commit(root: &Path) -> String {
    let short = |id: &str| id.trim().chars().take(7).collect::<String>();
    let Ok(head) = std::fs::read_to_string(root.join(".git").join("HEAD")) else {
        return "unknown".to_owned();
    };
    let Some(reference) = head.trim().strip_prefix("ref: ") else {
        // Detached: HEAD is the commit itself.
        return short(&head);
    };
    if let Ok(id) = std::fs::read_to_string(root.join(".git").join(reference)) {
        return short(&id);
    }
    // A branch whose ref has been packed away has no file of its own.
    if let Ok(packed) = std::fs::read_to_string(root.join(".git").join("packed-refs")) {
        for line in packed.lines() {
            if let Some((id, name)) = line.split_once(' ') {
                if name.trim() == reference {
                    return short(id);
                }
            }
        }
    }
    "unknown".to_owned()
}

/// A count and the thing counted, in the right number.
fn plural(count: usize, thing: &str) -> String {
    format!("{count} {thing}{}", if count == 1 { "" } else { "s" })
}

/// What to say about one document.
fn said(verdict: &Verdict, corpus: &Path) -> (String, String) {
    let name =
        verdict.path.strip_prefix(corpus).unwrap_or(&verdict.path).to_string_lossy().into_owned();
    let detail = match &verdict.judgement {
        Judgement::NoReference => "no reference pages".to_owned(),
        Judgement::Failed(why) => format!("failed: {why}"),
        Judgement::Scored(score) => {
            let mut detail = format!(
                "{:.1}% tolerant, {:.1}% exact  {}, {} reference",
                score.tolerant.percent(),
                score.exact.percent(),
                plural(score.ours, "page"),
                score.theirs
            );
            if let Some(worst) = score.worst() {
                if score.pages.len() > 1 {
                    detail.push_str(&format!(
                        ", worst page {} at {:.1}%",
                        worst.number,
                        worst.tolerant.percent()
                    ));
                }
                if let Some(note) = worst.note {
                    detail.push_str(&format!(" ({note})"));
                }
            }
            detail
        }
    };
    (name, detail)
}

/// The report, as lines to print.
#[must_use]
pub fn lines(corpus: &Path, verdicts: &[Verdict], before: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    let (exact, tolerant, documents, pages) = total(verdicts);

    if verdicts.is_empty() {
        out.push(format!("{} holds no documents.", corpus.display()));
        return out;
    }
    if documents == 0 {
        out.push(format!(
            "None of the {} in {} has reference pages.",
            plural(verdicts.len(), "document"),
            corpus.display()
        ));
        out.push(String::new());
        out.push(format!(
            "Put Word's own pages in {}/<document>/, as pictures named by page",
            corpus.join(REFERENCE).display()
        ));
        out.push("number: page-1.png, page-2.png and so on. Any picture format this".into());
        out.push("program reads will do.".into());
        return out;
    }

    let widest =
        verdicts.iter().map(|verdict| said(verdict, corpus).0.len()).max().unwrap_or(0).min(44);
    out.push(format!("{} — {}", corpus.display(), plural(verdicts.len(), "document")));
    out.push(String::new());
    for verdict in verdicts {
        let (name, detail) = said(verdict, corpus);
        out.push(format!("  {name:<widest$}  {detail}", widest = widest));
    }

    out.push(String::new());
    out.push(format!(
        "{}, {}: {:.1}% tolerant, {:.1}% exact",
        plural(documents, "document"),
        plural(pages, "page"),
        tolerant.percent(),
        exact.percent()
    ));
    if let Some(line) = before {
        if let Some(was) = score_in(line) {
            let moved = tolerant.percent() - was;
            let when = line.split_whitespace().next().unwrap_or("the run before");
            out.push(format!("since {when}: {moved:+.1} points, from {was:.1}%"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("wp-fidelity-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a directory to work in");
        directory
    }

    /// A page with a black band across it, a tenth of the page deep.
    ///
    /// Deep in proportion, so that the same page at two sizes really is the
    /// same page: a band of a fixed four pixels would be a tenth of a small
    /// page and a twentieth of a large one, and comparing those would be
    /// measuring the test and not the code.
    fn banded(width: usize, height: usize, top: usize) -> Ink {
        let mut ink = Ink::blank(width, height);
        for y in top..(top + height / 10).min(height) {
            for x in width / 15..width - width / 15 {
                ink.values[y * width + x] = 1.0;
            }
        }
        ink
    }

    #[test]
    fn a_page_against_itself_agrees_completely() {
        let page = banded(40, 40, 10);
        assert_eq!(page.against(&page).score(), 1.0);
        // And two blank pages are not a disagreement about nothing.
        let blank = Ink::blank(40, 40);
        assert_eq!(blank.against(&blank).score(), 1.0);
    }

    #[test]
    fn a_blank_page_where_there_should_be_writing_scores_nothing() {
        let page = banded(40, 40, 10);
        let blank = Ink::blank(40, 40);
        assert_eq!(page.against(&blank).score(), 0.0);
        assert_eq!(blank.against(&page).score(), 0.0);
    }

    #[test]
    fn a_page_a_pixel_out_is_nearly_right_rather_than_wrong() {
        // This is the whole reason there are two numbers. A rendering that is
        // a pixel low is a rendering that works; one that failed the
        // comparison for it would make the comparison useless.
        let page = banded(60, 60, 20);
        let moved = banded(60, 60, 21);

        let exact = page.against(&moved).score();
        let tolerant = page.coarse().against(&moved.coarse()).score();
        assert!(exact < 0.9, "a pixel of movement went unnoticed: {exact}");
        assert!(tolerant > exact, "the coarse look did not forgive it: {tolerant} vs {exact}");
        assert!(tolerant > 0.7, "a page a pixel out was called wrong: {tolerant}");
    }

    #[test]
    fn a_page_is_measured_against_the_reference_and_not_against_its_own_size() {
        // The same page drawn twice as big is the same page. If the score
        // depended on the size, every document would be measuring the
        // exporter's resolution rather than this program's drawing.
        let small = banded(30, 30, 10);
        let large = banded(60, 60, 20);
        let score = small.against(&large).score();
        assert!(score > 0.9, "the same page at two sizes scored {score}");
    }

    #[test]
    fn ink_is_read_over_white_however_the_picture_was_saved() {
        // A page saved with an alpha channel and one saved without are the
        // same page; reading the transparent one as black would make them
        // opposites and score a perfect rendering at nothing.
        let opaque = Ink::from_pixels(1, 1, &[255, 255, 255, 255]);
        let transparent = Ink::from_pixels(1, 1, &[0, 0, 0, 0]);
        assert!(opaque.sum() < 0.01);
        assert!(transparent.sum() < 0.01);

        let black = Ink::from_pixels(1, 1, &[0, 0, 0, 255]);
        assert!(black.sum() > 0.99);
    }

    #[test]
    fn the_images_are_paired_with_the_pages_by_number() {
        // Page ten comes after page nine, which sorting names does not do.
        let folder = scratch("order");
        for name in ["page-9.png", "page-10.png", "page-1.png", ".hidden.png"] {
            std::fs::write(folder.join(name), b"not really a picture").expect("writing it");
        }
        let found: Vec<String> = reference_images(&folder)
            .iter()
            .map(|path| path.file_name().unwrap_or_default().to_string_lossy().into_owned())
            .collect();
        assert_eq!(found, vec!["page-1.png", "page-9.png", "page-10.png"]);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_reference_folder_is_named_after_the_document() {
        let corpus = Path::new("corpus");
        assert_eq!(
            reference_folder(corpus, &corpus.join("letters").join("report.docx")),
            corpus.join(REFERENCE).join("letters").join("report")
        );
    }

    #[test]
    fn a_document_with_no_reference_is_not_a_zero() {
        // Everybody's corpus has documents nobody has exported pages for, and
        // scoring those at nothing would drag the average to meaninglessness
        // and hide the documents that really are drawn badly.
        let corpus = scratch("bare");
        std::fs::write(corpus.join("one.docx"), a_document()).expect("writing it");

        let verdicts = run(&corpus);
        assert_eq!(verdicts.len(), 1);
        assert!(matches!(verdicts[0].judgement, Judgement::NoReference));

        let (_, _, documents, pages) = total(&verdicts);
        assert_eq!((documents, pages), (0, 0), "a document with no reference was counted");
        let said = lines(&corpus, &verdicts, None).join("\n");
        assert!(said.contains("reference"), "{said}");
        let _ = std::fs::remove_dir_all(&corpus);
    }

    /// A document this program wrote, which is the only kind the suite has.
    fn a_document() -> Vec<u8> {
        wp_docx::Document::create(&crate::demonstration_body())
            .expect("the demonstration document")
            .save()
            .expect("saving it")
    }

    #[test]
    fn a_document_measured_against_its_own_pages_is_perfect() {
        // The end of the machinery: the walk, the pairing, the resolution
        // worked back from the reference, the rendering and the score. The
        // reference here is this program's own drawing, so the only thing
        // this cannot prove is the one thing no test can: what Word's pages
        // look like.
        let library = FontLibrary::scan_system();
        if library.is_empty() {
            return;
        }
        let corpus = scratch("perfect");
        let document = corpus.join("one.docx");
        std::fs::write(&document, a_document()).expect("writing it");

        let opened = crate::open_bytes(&document, &std::fs::read(&document).expect("reading it"))
            .expect("opening it");
        let mut engine = LayoutEngine::new(&library).with_dpi(150.0);
        let pages = engine.layout_document(&opened);
        assert!(!pages.is_empty(), "the demonstration document laid out into nothing");

        let folder = reference_folder(&corpus, &document);
        std::fs::create_dir_all(&folder).expect("the reference folder");
        let mut renderer = Renderer::new(&library);
        for (number, page) in pages.iter().enumerate() {
            let canvas = renderer.render(page, Color::WHITE);
            std::fs::write(
                folder.join(format!("page-{}.png", number + 1)),
                wp_raster::encode_png(&canvas),
            )
            .expect("writing a reference page");
        }

        let verdicts = run(&corpus);
        assert_eq!(verdicts.len(), 1);
        match &verdicts[0].judgement {
            Judgement::Scored(score) => {
                assert_eq!(score.ours, pages.len());
                assert_eq!(score.theirs, pages.len());
                assert!(
                    score.exact.score() > 0.999,
                    "a page against its own picture scored {:.3}",
                    score.exact.score()
                );
            }
            other => panic!("a document with pages was reported as {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&corpus);
    }

    #[test]
    fn a_page_that_was_never_laid_out_costs_what_it_holds() {
        let page = banded(20, 20, 5);
        let missed = Agreement::none_of(page.sum());
        assert_eq!(missed.score(), 0.0);

        // And it drags the document's score down in proportion, rather than
        // being left out of it.
        let mut whole = page.against(&page);
        assert_eq!(whole.score(), 1.0);
        whole.add(missed);
        assert!(whole.score() > 0.4 && whole.score() < 0.6, "{}", whole.score());
    }

    #[test]
    fn the_history_line_says_when_what_and_how_much() {
        let verdicts = vec![Verdict {
            path: PathBuf::from("one.docx"),
            judgement: Judgement::Scored(Box::new(Score {
                ours: 2,
                theirs: 2,
                pages: vec![
                    PageScore {
                        number: 1,
                        exact: Agreement { agreed: 8.0, total: 10.0 },
                        tolerant: Agreement { agreed: 9.0, total: 10.0 },
                        note: None,
                    },
                    PageScore {
                        number: 2,
                        exact: Agreement { agreed: 8.0, total: 10.0 },
                        tolerant: Agreement { agreed: 9.0, total: 10.0 },
                        note: None,
                    },
                ],
                exact: Agreement { agreed: 16.0, total: 20.0 },
                tolerant: Agreement { agreed: 18.0, total: 20.0 },
            })),
        }];

        let line = history_line(&verdicts, "2026-09-17T10:00:00Z", "abc1234");
        assert!(line.contains("1 document "), "{line}");
        assert!(line.contains("2 pages"), "{line}");
        assert!(line.contains("tolerant 90.0%"), "{line}");
        assert!(line.contains("exact 80.0%"), "{line}");
        assert_eq!(score_in(&line), Some(90.0));

        // And the report says which way it moved since the run before.
        let earlier = history_line(&verdicts, "2026-09-10T10:00:00Z", "abc1234")
            .replace("tolerant 90.0%", "tolerant 85.0%");
        let said = lines(Path::new("corpus"), &verdicts, Some(&earlier)).join("\n");
        assert!(said.contains("+5.0 points"), "{said}");
        assert!(said.contains("2026-09-10"), "{said}");
    }

    #[test]
    fn the_commit_is_read_out_of_the_repository_however_it_is_kept() {
        let root = scratch("git");
        let git = root.join(".git");
        std::fs::create_dir_all(git.join("refs").join("heads")).expect("a refs folder");

        // A branch with a ref of its own.
        std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").expect("writing HEAD");
        std::fs::write(git.join("refs").join("heads").join("main"), "0123456789abcdef\n")
            .expect("writing the ref");
        assert_eq!(commit(&root), "0123456");

        // A branch whose ref has been packed away has no file of its own.
        std::fs::remove_file(git.join("refs").join("heads").join("main")).expect("removing it");
        std::fs::write(git.join("packed-refs"), "# pack-refs\nfedcba9876543210 refs/heads/main\n")
            .expect("writing packed-refs");
        assert_eq!(commit(&root), "fedcba9");

        // And a repository that is not one says so rather than guessing.
        let bare = scratch("nogit");
        assert_eq!(commit(&bare), "unknown");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&bare);
    }
}
