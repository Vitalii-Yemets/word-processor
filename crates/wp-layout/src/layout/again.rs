//! Laying the body out again from where it changed.
//!
//! # What a keystroke used to cost
//!
//! What each paragraph measures to is kept with it — see `Measured` in
//! [`crate::layout`] — so a keystroke does not shape the paragraphs it did
//! not touch. It still *placed* every one of them: walked every page of the
//! document deciding where each line goes, which on a thousand pages was a
//! quarter of a second a letter. Word does not do that. It lays out again
//! from the paragraph that changed and stops the moment the pagination
//! lands where it landed before, because from there on nothing can differ.
//!
//! # How it is done here
//!
//! Every pass over the body leaves a trail of [`Checkpoint`]s, one at the
//! start of each block where the placement could be taken up again, holding
//! everything the placement carries at that moment: where the text has got
//! to down the page and which column it is in, how much of the page is
//! drawn, the list counters, how many drawings float so far, the outline
//! depth. The blocks that were laid out are kept too, in [`Remembered`].
//!
//! The next pass is given the pages back. It compares the new body with the
//! old one, finds the run of blocks that changed, and takes up from the last
//! checkpoint before it, with the pages cut back to that moment. It places
//! blocks until it reaches a checkpoint past the change that looks exactly
//! as it looked last time — same place on the page, same counters, same
//! drawings beside the text — and then it stops: the rest of that page and
//! every page after are the old ones, moved over untouched.
//!
//! # Why that is the same as laying it all out
//!
//! A block's placement depends on the state at its start, on the block, and
//! on the conditions of the layout — the resolution, the colours, what the
//! fields answer, the footnote room. So if the state at a block is the same,
//! and the block and everything after it are the same, and the conditions
//! are the same, the placement is the same. The conditions are compared
//! whole, in [`Conditions`], and any difference lays the body out from the
//! start. A paragraph with contextual spacing asks whether its neighbours
//! share its style, so the run that changed is widened by one block each
//! way. Paragraphs that ask to stay with the next are placed as a run, and
//! no checkpoint is taken inside one.
//!
//! What a page holds is not compared, because it cannot be cheaply. The
//! pages given back are trusted to be the ones this engine gave out last
//! time, which is what the stamp on each page says; anything else is laid
//! out afresh. And what a page holds is worked out from more than the
//! paragraphs on it: a style edited or a picture replaced changes how a
//! paragraph looks without changing the paragraph. Every such change writes
//! a part of the package, and the package counts its writes, so the count
//! is one of the conditions.
//!
//! `tests/incremental.rs` holds all of this to the one thing that matters:
//! whatever the engine has been through, its pages are the pages a new
//! engine gives.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use wp_docx::model::{Block, Body};
use wp_docx::numbering::ListCounters;
use wp_docx::sections::Start;
use wp_raster::Color;

use super::{Float, Frame, LayoutEngine, Page, PageMetrics, Stretch};

/// How much of a page is drawn: the length of each of its lists.
///
/// Enough to cut a page back to a moment, and enough to tell whether two
/// moments look alike — which they must for the rest of an old page to be
/// put after the start of a new one, because a line names its glyphs by
/// where they sit in the page's list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Extent {
    glyphs: usize,
    images: usize,
    shapes: usize,
    decorations: usize,
    cells: usize,
    paths: usize,
    inks: usize,
    lines: usize,
    turned: usize,
}

impl Extent {
    pub(crate) fn of(page: &Page) -> Self {
        Self {
            glyphs: page.glyphs.len(),
            images: page.images.len(),
            shapes: page.shapes.len(),
            decorations: page.decorations.len(),
            cells: page.cells.len(),
            paths: page.paths.len(),
            inks: page.inks.len(),
            lines: page.lines.len(),
            turned: page.turned.len(),
        }
    }

    /// Cuts the page back to this much.
    fn cut(self, page: &mut Page) {
        page.glyphs.truncate(self.glyphs);
        page.images.truncate(self.images);
        page.shapes.truncate(self.shapes);
        page.decorations.truncate(self.decorations);
        page.cells.truncate(self.cells);
        page.paths.truncate(self.paths);
        page.inks.truncate(self.inks);
        page.lines.truncate(self.lines);
        page.turned.truncate(self.turned);
    }

    /// Whether the page holds at least this much.
    fn fits(self, page: &Page) -> bool {
        page.glyphs.len() >= self.glyphs
            && page.images.len() >= self.images
            && page.shapes.len() >= self.shapes
            && page.decorations.len() >= self.decorations
            && page.cells.len() >= self.cells
            && page.paths.len() >= self.paths
            && page.inks.len() >= self.inks
            && page.lines.len() >= self.lines
            && page.turned.len() >= self.turned
    }

    /// Takes everything beyond this much off the page, as a page of its own.
    ///
    /// The lines and turned spans in what comes off still count their glyphs
    /// from the start of the page they came off, which is what lets them go
    /// back onto a page drawn to the same extent — see [`Self::carry`].
    fn split(self, page: &mut Page) -> Page {
        Page {
            width: page.width,
            height: page.height,
            glyphs: page.glyphs.split_off(self.glyphs),
            images: page.images.split_off(self.images),
            shapes: page.shapes.split_off(self.shapes),
            decorations: page.decorations.split_off(self.decorations),
            cells: page.cells.split_off(self.cells),
            paths: page.paths.split_off(self.paths),
            inks: page.inks.split_off(self.inks),
            lines: page.lines.split_off(self.lines),
            turned: page.turned.split_off(self.turned),
            frame: Frame::default(),
            turned_glyphs: 0,
            stamp: 0,
        }
    }

    /// Moves what `from` holds beyond `at` onto the end of `to`, where
    /// `from` is a page split off at `base` and `to` is drawn to exactly
    /// `at`.
    fn carry(from: &mut Page, base: Self, at: Self, to: &mut Page) {
        debug_assert_eq!(Self::of(to), at, "the page is not drawn to the moment carried to");
        to.glyphs.extend(from.glyphs.drain(at.glyphs - base.glyphs..));
        to.images.extend(from.images.drain(at.images - base.images..));
        to.shapes.extend(from.shapes.drain(at.shapes - base.shapes..));
        to.decorations.extend(from.decorations.drain(at.decorations - base.decorations..));
        to.cells.extend(from.cells.drain(at.cells - base.cells..));
        to.paths.extend(from.paths.drain(at.paths - base.paths..));
        to.inks.extend(from.inks.drain(at.inks - base.inks..));
        to.lines.extend(from.lines.drain(at.lines - base.lines..));
        to.turned.extend(from.turned.drain(at.turned - base.turned..));
    }
}

/// Where the placement of the body could be taken up again: the start of a
/// block, with everything the placement carries at that moment.
#[derive(Clone, Debug)]
pub(crate) struct Checkpoint {
    /// Which block of the body begins here, and which paragraph it is.
    pub(crate) block: usize,
    pub(crate) index: usize,
    /// Which stretch of the body — which section — it is in.
    pub(crate) stretch: usize,
    /// Which page the text had got to, and how much of it was drawn.
    pub(crate) page: usize,
    pub(crate) held: Extent,
    /// Where the text had got to down the page, and which column it was in.
    pub(crate) y: f32,
    pub(crate) column: usize,
    pub(crate) counters: ListCounters,
    /// How many drawings had been found floating so far.
    pub(crate) floats: usize,
    pub(crate) outline_heading: u8,
}

/// Everything a body's placement depends on besides the body.
///
/// Compared whole: any difference means the pages from last time say
/// nothing about this time. Most of it is what the engine is set to; the
/// rest is what the last pass worked out and this one is given — the room
/// the footnotes take on each page, the pages the bookmarks fall on, the
/// numbers the notes carry — and the count of writes to the package, which
/// is how a style edited or a picture replaced is noticed.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Conditions {
    dpi: f32,
    automatic_color: Color,
    automatic_line: Color,
    show_markup: bool,
    show_formatting: bool,
    table_gridlines: bool,
    show_marks: bool,
    outline: Option<u8>,
    default_tab: i32,
    generation: u64,
    merge_record: Vec<(String, String)>,
    reserved: Vec<f32>,
    bookmark_pages: HashMap<String, usize>,
    note_numbers: HashMap<(bool, i32), usize>,
    metrics: PageMetrics,
    stretches: Vec<(PageMetrics, Start, usize)>,
}

/// What the last pass over the document's body left behind.
#[derive(Debug)]
pub(crate) struct Remembered {
    conditions: Conditions,
    /// What each `SEQ` field showed, by the paragraph it sat in. Kept apart
    /// from the conditions because a paragraph added before a field moves
    /// its key without changing its answer — see [`same_sequences`].
    sequences: HashMap<(usize, usize), usize>,
    /// The body as it was laid out.
    blocks: Vec<Block>,
    /// Which stretch each block was laid out in.
    stretches_of: Vec<usize>,
    /// The paragraph each block begins at, and after the last block the
    /// total: one more entry than there are blocks.
    starts: Vec<usize>,
    /// One per block where placement could be taken up, in block order.
    checkpoints: Vec<Checkpoint>,
    /// What the body put on each page, before anything else was put there.
    pages: Vec<Extent>,
    page_sections: Vec<usize>,
    /// What the placement ended with, for the next pass to end with too when
    /// it stops early.
    floats: Vec<Float>,
    counters: ListCounters,
    outline_heading: u8,
    /// The stamp on every page of that pass.
    stamp: u64,
}

/// The run of blocks that changed between two bodies, in each one's
/// numbering, and how much the numbering after it moved.
#[derive(Clone, Copy, Debug)]
struct Change {
    /// The first block that changed, which is the same in both.
    from: usize,
    /// The first block after the run, in the old body and in the new.
    to_old: usize,
    to_new: usize,
    /// How many blocks and how many paragraphs the new body has more than
    /// the old — or fewer, when negative.
    blocks: isize,
    paragraphs: isize,
}

impl Change {
    /// The run that changed, or nothing when the bodies are the same.
    ///
    /// The longest common start and end are taken as unchanged, and what is
    /// between them is the change. A block is compared along with which
    /// stretch it is laid out in, so a section break that moved is a change
    /// to the blocks it moved across.
    fn between(
        old: &[Block],
        old_stretches: &[usize],
        new: &[Block],
        new_stretches: &[usize],
        old_starts: &[usize],
    ) -> Option<Self> {
        let same = |i: usize, j: usize| old[i] == new[j] && old_stretches[i] == new_stretches[j];
        let shortest = old.len().min(new.len());
        let mut from = 0;
        while from < shortest && same(from, from) {
            from += 1;
        }
        if from == old.len() && from == new.len() {
            return None;
        }
        let mut common = 0;
        while common < shortest - from && same(old.len() - 1 - common, new.len() - 1 - common) {
            common += 1;
        }
        let (mut to_old, mut to_new) = (old.len() - common, new.len() - common);

        // A paragraph with contextual spacing looks at the style of the one
        // before it and the one after, so those are placed again as well.
        from = from.saturating_sub(1);
        to_old = (to_old + 1).min(old.len());
        to_new = (to_new + 1).min(new.len());

        let old_paragraphs = old_starts[to_old] - old_starts[from];
        let new_paragraphs: usize = new[from..to_new].iter().map(Block::paragraph_count).sum();
        Some(Self {
            from,
            to_old,
            to_new,
            blocks: to_new as isize - to_old as isize,
            paragraphs: new_paragraphs as isize - old_paragraphs as isize,
        })
    }
}

/// What a pass is told about the last one, once the change is known.
#[derive(Clone, Debug)]
struct Plan {
    change: Change,
    /// Whether some stretch after each one begins on an even or an odd
    /// page, indexed by stretch: such a stretch takes a blank page or not
    /// according to the count before it, so a pass that has added or taken
    /// away an odd number of pages cannot stop before it.
    parity_matters: Vec<bool>,
}

/// The checkpoints a pass leaves, and where it stopped if it stopped early.
#[derive(Debug, Default)]
pub(crate) struct Trail {
    pub(crate) checkpoints: Vec<Checkpoint>,
    /// Which stretch is being placed, and which block of the body its first
    /// block is, so that a checkpoint can say which block it stands at.
    pub(crate) stretch: usize,
    pub(crate) offset: usize,
    /// The paper a body with no sections of its own is laid out on.
    pub(crate) metrics: PageMetrics,
    /// How many blocks the pass placed.
    pub(crate) placed: usize,
    plan: Option<Plan>,
    /// The old checkpoint at which this pass found itself where the last
    /// one was, and how many pages and floats the new pages have more than
    /// the old up to there.
    pub(crate) settled: Option<Settled>,
}

impl Trail {
    /// A trail for a pass on the given paper.
    pub(crate) fn on(metrics: PageMetrics) -> Self {
        Self { metrics, ..Self::default() }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Settled {
    at: usize,
    pages: isize,
    floats: isize,
}

/// What a pass took up from the last one.
#[derive(Debug)]
pub(crate) struct Resumed {
    /// Which of the old checkpoints was taken up from, and where it stood.
    checkpoint: usize,
    pub(crate) block: usize,
    pub(crate) stretch: usize,
    page: usize,
    pub(crate) y: f32,
    pub(crate) column: usize,
    pub(crate) index: usize,
    /// The pages up to and including the one it stood on, cut back to it,
    /// and which section each belongs to.
    pub(crate) pages: Vec<Page>,
    pub(crate) belongs: Vec<usize>,
    /// The rest of that page beyond the checkpoint, and every page after,
    /// for when this pass stops early.
    rest: Page,
    base: Extent,
    tail: Vec<Page>,
}

/// How a pass begins.
pub(crate) enum Taking {
    /// From the first block, with nothing from last time.
    Afresh,
    /// Not at all: nothing changed, and these are the pages.
    Unchanged(Vec<Page>),
    /// From a checkpoint, with the state restored.
    From(Box<Resumed>),
}

/// Which stretch each block of a body is laid out in.
fn stretches_of(stretches: &[Stretch], blocks: usize) -> Vec<usize> {
    let mut of = vec![usize::MAX; blocks];
    for (which, stretch) in stretches.iter().enumerate() {
        for slot in of.iter_mut().take(stretch.blocks.end.min(blocks)).skip(stretch.blocks.start) {
            *slot = which;
        }
    }
    of
}

/// The paragraph each block begins at, and the total after the last.
fn starts_of(blocks: &[Block]) -> Vec<usize> {
    let mut starts = Vec::with_capacity(blocks.len() + 1);
    let mut count = 0;
    for block in blocks {
        starts.push(count);
        count += block.paragraph_count();
    }
    starts.push(count);
    starts
}

/// Whether the sequence numbers are the same, allowing for the paragraphs
/// that moved: a key before the change must be there unchanged, a key after
/// it must be there moved along, and the keys inside the change are placed
/// again anyway.
fn same_sequences(
    old: &HashMap<(usize, usize), usize>,
    new: &HashMap<(usize, usize), usize>,
    from: usize,
    to_old: usize,
    to_new: usize,
    moved: isize,
) -> bool {
    let outside_new =
        new.keys().filter(|(paragraph, _)| *paragraph < from || *paragraph >= to_new).count();
    let mut counted = 0;
    for (&(paragraph, nth), number) in old {
        if paragraph >= from && paragraph < to_old {
            continue;
        }
        counted += 1;
        let key = if paragraph < from {
            (paragraph, nth)
        } else {
            ((paragraph as isize + moved) as usize, nth)
        };
        if new.get(&key) != Some(number) {
            return false;
        }
    }
    counted == outside_new
}

/// Moves every paragraph number on the page along, for a page kept from
/// after a change that added or took away paragraphs in front of it.
fn renumber(page: &mut Page, moved: isize) {
    let shift = |index: usize| (index as isize + moved) as usize;
    for glyph in &mut page.glyphs {
        // A glyph that is not the document's text — the mark of a list item,
        // the figures on a chart — points at nothing, and stays pointing at
        // nothing. Nothing on a page after a change can point at the first
        // paragraph, so nothing else looks like this.
        let nowhere = glyph.source == wp_docx::TextPosition::default() && glyph.source_length == 0;
        if !nowhere {
            glyph.source.paragraph = shift(glyph.source.paragraph);
        }
    }
    for line in &mut page.lines {
        line.paragraph = shift(line.paragraph);
    }
    for cell in &mut page.cells {
        cell.at.paragraph = shift(cell.at.paragraph);
    }
    for image in &mut page.images {
        if let Some(at) = &mut image.at {
            at.paragraph = shift(at.paragraph);
        }
    }
    for shape in &mut page.shapes {
        if let Some(at) = &mut shape.at {
            at.paragraph = shift(at.paragraph);
        }
    }
}

/// Each engine stamps its pages with a number no other engine uses, so that
/// pages from another engine — or from an earlier pass of this one — are
/// never taken for the last pass's.
static ENGINES: AtomicU64 = AtomicU64::new(1);

pub(crate) fn engine_number() -> u64 {
    ENGINES.fetch_add(1, Ordering::Relaxed)
}

impl LayoutEngine<'_> {
    /// The stamp for the next pass's pages.
    fn next_stamp(&mut self) -> u64 {
        self.passes += 1;
        (self.engine << 32) | self.passes
    }

    /// Everything the placement depends on besides the body, as it is now.
    fn conditions(&self, metrics: PageMetrics, stretches: &[Stretch]) -> Conditions {
        Conditions {
            dpi: self.dpi,
            automatic_color: self.automatic_color,
            automatic_line: self.automatic_line,
            show_markup: self.show_markup,
            show_formatting: self.show_formatting,
            table_gridlines: self.table_gridlines,
            show_marks: self.show_marks,
            outline: self.outline,
            default_tab: self.default_tab,
            generation: self.generation,
            merge_record: self.merge_record.clone(),
            reserved: self.reserved.clone(),
            bookmark_pages: self.bookmark_pages.clone(),
            note_numbers: self.note_numbers.clone(),
            metrics,
            stretches: stretches
                .iter()
                .map(|stretch| (stretch.metrics, stretch.start, stretch.section))
                .collect(),
        }
    }

    /// Decides how the pass over the body begins, given the pages the last
    /// one gave, and puts the engine in the state to begin that way.
    pub(crate) fn take_up(
        &mut self,
        body: &Body,
        stretches: &[Stretch],
        mut previous: Vec<Page>,
        trail: &mut Trail,
    ) -> Taking {
        let metrics = trail.metrics;
        let Some(memo) = &self.remembered else { return Taking::Afresh };

        // The pages have to be the ones the last pass gave, every one of them
        // with at least what that pass put on it still there.
        let mine = previous.len() == memo.pages.len()
            && previous
                .iter()
                .zip(&memo.pages)
                .all(|(page, extent)| page.stamp == memo.stamp && extent.fits(page));
        if !mine || memo.conditions != self.conditions(metrics, stretches) {
            return Taking::Afresh;
        }

        let new_stretches = stretches_of(stretches, body.blocks.len());
        let change = Change::between(
            &memo.blocks,
            &memo.stretches_of,
            &body.blocks,
            &new_stretches,
            &memo.starts,
        );
        let Some(change) = change else {
            if memo.sequences != self.sequence_numbers {
                return Taking::Afresh;
            }
            // Nothing moved: the pages are what they were, with what was put
            // on them afterwards taken off again.
            for (page, extent) in previous.iter_mut().zip(&memo.pages) {
                extent.cut(page);
            }
            self.floats.clone_from(&memo.floats);
            self.counters.clone_from(&memo.counters);
            self.outline_heading = memo.outline_heading;
            self.page_sections.clone_from(&memo.page_sections);
            return Taking::Unchanged(previous);
        };
        let (from, to_old) = (memo.starts[change.from], memo.starts[change.to_old]);
        let to_new = (to_old as isize + change.paragraphs) as usize;
        if !same_sequences(
            &memo.sequences,
            &self.sequence_numbers,
            from,
            to_old,
            to_new,
            change.paragraphs,
        ) {
            return Taking::Afresh;
        }

        // The last checkpoint before the change, to take up from.
        let Some(at) = memo.checkpoints.iter().rposition(|point| point.block <= change.from) else {
            return Taking::Afresh;
        };
        let point = memo.checkpoints[at].clone();
        let floats = memo.floats[..point.floats].to_vec();
        let belongs = memo.page_sections[..=point.page].to_vec();
        let parity_matters = {
            let mut matters = vec![false; stretches.len()];
            let mut later = false;
            for (which, stretch) in stretches.iter().enumerate().rev() {
                matters[which] = later;
                later |= matches!(stretch.start, Start::EvenPage | Start::OddPage);
            }
            matters
        };

        // What was measured keeps its place against the paragraphs that
        // moved, so a paragraph added or taken away in front of a thousand
        // others does not have all of them measured again.
        if change.paragraphs != 0 && from < self.measured.len() {
            let to = to_old.min(self.measured.len());
            let count = ((to - from) as isize + change.paragraphs).max(0) as usize;
            self.measured.splice(from..to, std::iter::repeat_n(None, count));
        }

        // The pages before the checkpoint stay; the one it is on is cut back
        // to it, and what came off it and every page after are kept for
        // when this pass stops.
        for (page, extent) in previous.iter_mut().zip(&memo.pages) {
            extent.cut(page);
        }
        let tail = previous.split_off(point.page + 1);
        let rest = point.held.split(&mut previous[point.page]);

        self.floats = floats;
        self.counters.clone_from(&point.counters);
        self.outline_heading = point.outline_heading;
        trail.plan = Some(Plan { change, parity_matters });
        Taking::From(Box::new(Resumed {
            checkpoint: at,
            block: point.block,
            stretch: point.stretch,
            page: point.page,
            y: point.y,
            column: point.column,
            index: point.index,
            pages: previous,
            belongs,
            rest,
            base: point.held,
            tail,
        }))
    }

    /// Whether the placement has found itself where the last pass was, at
    /// this checkpoint: the old checkpoint it matches, if so.
    pub(crate) fn settles(&self, trail: &Trail, point: &Checkpoint) -> Option<Settled> {
        let plan = trail.plan.as_ref()?;
        if point.block < plan.change.to_new {
            return None;
        }
        let memo = self.remembered.as_ref()?;
        let old_block = (point.block as isize - plan.change.blocks) as usize;
        let at = memo.checkpoints.binary_search_by_key(&old_block, |old| old.block).ok()?;
        let old = &memo.checkpoints[at];

        let pages = point.page as isize - old.page as isize;
        let same = old.index as isize + plan.change.paragraphs == point.index as isize
            && old.stretch == point.stretch
            && old.y == point.y
            && old.column == point.column
            && old.held == point.held
            && old.counters == point.counters
            && old.outline_heading == point.outline_heading;
        if !same {
            return None;
        }
        // A pass that has changed the count of pages by an odd number cannot
        // stop before a section that begins on an even or an odd page.
        if pages % 2 != 0 && plan.parity_matters[point.stretch] {
            return None;
        }
        // The footnote room is the page's, and the pages after this one keep
        // theirs only if the room is the same at their new numbers.
        if pages != 0 {
            let room = |page: usize| self.reserved.get(page).copied().unwrap_or(0.0);
            let reaches = self.reserved.len().saturating_sub(old.page.min(point.page));
            if (0..reaches).any(|offset| room(old.page + offset) != room(point.page + offset)) {
                return None;
            }
        }
        // The drawings floating beside the text on this page narrow the lines
        // after them, so they have to be the same drawings in the same
        // places.
        let mine: Vec<Float> = self.floats[..point.floats]
            .iter()
            .filter(|float| float.page == point.page)
            .map(|float| Float { page: 0, ..*float })
            .collect();
        let theirs: Vec<Float> = memo.floats[..old.floats]
            .iter()
            .filter(|float| float.page == old.page)
            .map(|float| Float { page: 0, ..*float })
            .collect();
        if mine != theirs {
            return None;
        }
        Some(Settled { at, pages, floats: point.floats as isize - old.floats as isize })
    }

    /// Finishes a pass that stopped early: puts the old pages after the
    /// ones placed, and the engine in the state the old pass ended in.
    pub(crate) fn settle(
        &mut self,
        trail: &Trail,
        resumed: &mut Resumed,
        pages: &mut Vec<Page>,
        belongs: &mut Vec<usize>,
    ) {
        let (Some(settled), Some(plan), Some(memo)) =
            (trail.settled, &trail.plan, &self.remembered)
        else {
            return;
        };
        let old = &memo.checkpoints[settled.at];
        let moved = plan.change.paragraphs;

        let (source, base) = if old.page == resumed.page {
            (&mut resumed.rest, resumed.base)
        } else {
            (&mut resumed.tail[old.page - resumed.page - 1], Extent::default())
        };
        if moved != 0 {
            renumber(source, moved);
        }
        let Some(last) = pages.last_mut() else { return };
        Extent::carry(source, base, old.held, last);

        let after = old.page - resumed.page;
        for mut page in resumed.tail.drain(after..) {
            if moved != 0 {
                renumber(&mut page, moved);
            }
            pages.push(page);
        }
        belongs.extend_from_slice(&memo.page_sections[old.page + 1..]);

        let shift = |float: &Float| Float {
            page: (float.page as isize + settled.pages) as usize,
            ..*float
        };
        self.floats.extend(memo.floats[old.floats..].iter().map(shift));
        self.counters.clone_from(&memo.counters);
        self.outline_heading = memo.outline_heading;
    }

    /// Keeps what this pass leaves for the next one.
    pub(crate) fn remember(
        &mut self,
        trail: Trail,
        resumed: Option<Box<Resumed>>,
        pages: &mut [Page],
        belongs: Vec<usize>,
        body: &Body,
        stretches: &[Stretch],
    ) {
        let metrics = trail.metrics;
        let stamp = self.next_stamp();
        for page in pages.iter_mut() {
            page.stamp = stamp;
        }

        let mut memo = match (self.remembered.take(), resumed, &trail.plan) {
            (Some(mut memo), Some(resumed), Some(plan)) => {
                let change = plan.change;
                memo.blocks.splice(
                    change.from..change.to_old,
                    body.blocks[change.from..change.to_new].iter().cloned(),
                );
                let fresh = trail.checkpoints.len();
                match trail.settled {
                    Some(settled) => {
                        // The old checkpoints from the one taken up from to
                        // the one settled on are replaced by this pass's,
                        // whose last is the one settled on; those after are
                        // moved along.
                        let old_page = memo.checkpoints[settled.at].page;
                        memo.checkpoints.splice(resumed.checkpoint..=settled.at, trail.checkpoints);
                        for point in &mut memo.checkpoints[resumed.checkpoint + fresh..] {
                            point.block = (point.block as isize + change.blocks) as usize;
                            point.index = (point.index as isize + change.paragraphs) as usize;
                            point.page = (point.page as isize + settled.pages) as usize;
                            point.floats = (point.floats as isize + settled.floats) as usize;
                        }
                        let placed = pages.len() - (memo.pages.len() - old_page - 1);
                        memo.pages.splice(
                            resumed.page..=old_page,
                            pages[resumed.page..placed].iter().map(Extent::of),
                        );
                    }
                    None => {
                        memo.checkpoints.truncate(resumed.checkpoint);
                        memo.checkpoints.extend(trail.checkpoints);
                        memo.pages.truncate(resumed.page);
                        memo.pages.extend(pages[resumed.page..].iter().map(Extent::of));
                    }
                }
                memo
            }
            _ => Remembered {
                conditions: self.conditions(metrics, stretches),
                sequences: HashMap::new(),
                blocks: body.blocks.clone(),
                stretches_of: Vec::new(),
                starts: Vec::new(),
                checkpoints: trail.checkpoints,
                pages: pages.iter().map(Extent::of).collect(),
                page_sections: Vec::new(),
                floats: Vec::new(),
                counters: ListCounters::new(),
                outline_heading: 0,
                stamp,
            },
        };
        memo.conditions = self.conditions(metrics, stretches);
        memo.sequences.clone_from(&self.sequence_numbers);
        memo.stretches_of = stretches_of(stretches, body.blocks.len());
        memo.starts = starts_of(&body.blocks);
        memo.page_sections = belongs;
        memo.floats.clone_from(&self.floats);
        memo.counters.clone_from(&self.counters);
        memo.outline_heading = self.outline_heading;
        memo.stamp = stamp;
        debug_assert_eq!(memo.pages.len(), pages.len(), "the pages remembered are not the pages");
        debug_assert_eq!(memo.blocks, body.blocks, "the blocks remembered are not the body");
        self.remembered = Some(memo);
    }
}
