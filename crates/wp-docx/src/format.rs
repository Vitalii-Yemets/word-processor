//! Character and paragraph formatting applied to a stretch of text.
//!
//! # Why runs have to be split
//!
//! A run is the unit that carries character formatting, so making half a word
//! bold means the word has to become two runs. Everything here therefore starts
//! by ensuring a run boundary exists at each end of the range, and only then
//! sets the property — on whole runs, which is the only place the format can
//! record it.
//!
//! Splitting copies the run and keeps opposite halves, so both sides come out
//! with every property the original had: its font, its colour, its language,
//! and anything this program does not model. A run cut in half must not lose
//! what it was.
//!
//! # Why "off" is written rather than removed
//!
//! Turning bold off inside a heading does not mean "say nothing about bold" —
//! that would inherit the heading's bold straight back. It means `w:b w:val="0"`,
//! an explicit override. So a format is switched off by writing it off, which is
//! what Word does and what the format is designed for.

use wp_xml::tree::{Element, Node};

use crate::edit::{
    atomic_text, collect_text_pieces, element_at_path_mut, insert_ordered, name_with,
    paragraph_properties_of, preserve_space_if_needed, toggle, valued, PARAGRAPH_PROPERTY_ORDER,
    RUN_PROPERTY_ORDER,
};
use crate::model::{
    Alignment, LineRule, LineSpacing, NumberingReference, ParagraphBorders, ParagraphProperties,
    ResolvedRunProperties, RunProperties, TabStop, Underline, VerticalAlignment,
};
use crate::read::{self, W};
use crate::styles::Styles;

/// One on/off character property a button or a keystroke can change.
///
/// Colour, size and font are set through the same machinery underneath — they
/// are just not on/off, so they are not here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharacterFormat {
    Bold,
    Italic,
    Underline,
    Strikethrough,
    Superscript,
    Subscript,
}

impl CharacterFormat {
    /// Whether this format is on, once inheritance has been applied.
    #[must_use]
    pub fn is_on(self, resolved: &ResolvedRunProperties) -> bool {
        match self {
            Self::Bold => resolved.bold,
            Self::Italic => resolved.italic,
            Self::Underline => resolved.underline.is_visible(),
            Self::Strikethrough => resolved.strike,
            Self::Superscript => resolved.vertical_align == VerticalAlignment::Superscript,
            Self::Subscript => resolved.vertical_align == VerticalAlignment::Subscript,
        }
    }

    /// The authored properties that turn this format on or off.
    #[must_use]
    pub fn change(self, on: bool) -> RunProperties {
        let mut properties = RunProperties::default();
        match self {
            Self::Bold => properties.bold = Some(on),
            Self::Italic => properties.italic = Some(on),
            Self::Strikethrough => properties.strike = Some(on),
            Self::Underline => {
                properties.underline = Some(if on { Underline::Single } else { Underline::None });
            }
            // Turning one of these off means going back to the line, not
            // saying nothing: the run may be inheriting it from its style.
            Self::Superscript => {
                properties.vertical_align = Some(if on {
                    VerticalAlignment::Superscript
                } else {
                    VerticalAlignment::Baseline
                });
            }
            Self::Subscript => {
                properties.vertical_align = Some(if on {
                    VerticalAlignment::Subscript
                } else {
                    VerticalAlignment::Baseline
                });
            }
        }
        properties
    }

    /// Reads this format out of authored properties, if they mention it.
    #[must_use]
    pub fn read(self, properties: &RunProperties) -> Option<bool> {
        match self {
            Self::Bold => properties.bold,
            Self::Italic => properties.italic,
            Self::Strikethrough => properties.strike,
            Self::Underline => properties.underline.as_ref().map(Underline::is_visible),
            Self::Superscript => {
                properties.vertical_align.map(|value| value == VerticalAlignment::Superscript)
            }
            Self::Subscript => {
                properties.vertical_align.map(|value| value == VerticalAlignment::Subscript)
            }
        }
    }

    /// Takes this format back out of authored properties.
    pub fn clear(self, properties: &mut RunProperties) {
        match self {
            Self::Bold => properties.bold = None,
            Self::Italic => properties.italic = None,
            Self::Strikethrough => properties.strike = None,
            Self::Underline => properties.underline = None,
            Self::Superscript | Self::Subscript => properties.vertical_align = None,
        }
    }
}

/// Where one run sits within its paragraph's text.
struct RunSpan {
    /// Indices into `children` at each level, from the paragraph down.
    path: Vec<usize>,
    start: usize,
    length: usize,
}

/// Every run under a paragraph, in document order.
///
/// Walked exactly the way the text is assembled, so the offsets here and the
/// offsets a caret uses are the same numbers. Runs inside a hyperlink or an
/// insertion are included, because their text is part of what the paragraph
/// says.
fn collect_run_spans(paragraph: &Element) -> Vec<RunSpan> {
    let mut spans = Vec::new();
    let mut path = Vec::new();
    let mut offset = 0usize;
    walk_run_spans(paragraph, &mut path, &mut offset, &mut spans);
    spans
}

fn walk_run_spans(
    element: &Element,
    path: &mut Vec<usize>,
    offset: &mut usize,
    spans: &mut Vec<RunSpan>,
) {
    for (index, node) in element.children.iter().enumerate() {
        let Node::Element(child) = node else { continue };
        if child.namespace.as_deref() != Some(W) {
            continue;
        }
        // Deleted text is not part of what the document says.
        if child.local_name() == "del" {
            continue;
        }

        path.push(index);
        if child.local_name() == "r" {
            let length = text_length_within(child);
            spans.push(RunSpan { path: path.clone(), start: *offset, length });
            *offset += length;
        } else {
            walk_run_spans(child, path, offset, spans);
        }
        path.pop();
    }
}

/// How much of the paragraph's text lies inside an element.
fn text_length_within(element: &Element) -> usize {
    if element.is(Some(W), "t") {
        return element.text_content().len();
    }
    if let Some(text) = atomic_text(element) {
        return text.len();
    }
    collect_text_pieces(element).iter().map(|piece| piece.text.len()).sum()
}

/// How much text one child of a run contributes.
fn content_length(node: &Node) -> usize {
    match node.as_element() {
        Some(element) => text_length_within(element),
        None => 0,
    }
}

/// Which half of a split run is being kept.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Head,
    Tail,
}

/// Keeps one side of a run, dropping the content on the other.
///
/// The run's properties stay on both halves, which is the whole point: a bold
/// word cut in two must come out as two bold runs.
fn keep_side(run: &mut Element, offset: usize, side: Side) {
    let starts: Vec<usize> = {
        let mut running = 0usize;
        run.children
            .iter()
            .map(|node| {
                let start = running;
                running += content_length(node);
                start
            })
            .collect()
    };

    // From the back, so removing one child does not move the others.
    for index in (0..run.children.len()).rev() {
        let Some(element) = run.children[index].as_element() else { continue };
        if element.is(Some(W), "rPr") {
            continue;
        }

        let start = starts[index];
        let length = content_length(&run.children[index]);

        // Content that ends at or before the split, or begins at or after it,
        // belongs wholly to one side. A tab or a break counts as one character,
        // so it is never straddled: it goes to whichever side it starts on.
        if start + length <= offset && !(length == 0 && start >= offset) {
            if side == Side::Tail {
                run.children.remove(index);
            }
            continue;
        }
        if start >= offset {
            if side == Side::Head {
                run.children.remove(index);
            }
            continue;
        }

        // Straddling the split. Text can be cut; anything else cannot, so it
        // stays whole on the head rather than being duplicated or lost.
        let local = offset - start;
        let Some(element) = run.children[index].as_element_mut() else { continue };
        if !element.is(Some(W), "t") {
            if side == Side::Tail {
                run.children.remove(index);
            }
            continue;
        }

        let text = element.text_content();
        if !text.is_char_boundary(local) {
            continue;
        }
        let kept = if side == Side::Head { &text[..local] } else { &text[local..] };
        let kept = kept.to_owned();
        element.set_text(&kept);
        preserve_space_if_needed(element, &kept);
    }
}

/// Makes sure a run boundary exists at an offset in a paragraph.
///
/// Does nothing when the offset already falls between two runs, which is the
/// common case and the reason formatting the same range twice does not keep
/// multiplying runs.
/// Cuts whichever run spans an offset in two, so something can be put between
/// the halves.
pub(crate) fn split_runs_at_offset(paragraph: &mut Element, offset: usize) {
    split_runs_at(paragraph, offset);
}

fn split_runs_at(paragraph: &mut Element, offset: usize) {
    let target = collect_run_spans(paragraph)
        .into_iter()
        .find(|span| span.length > 0 && offset > span.start && offset < span.start + span.length);
    let Some(span) = target else { return };

    let local = offset - span.start;
    let Some(original) = element_at_path_mut(paragraph, &span.path) else { return };

    let mut tail = original.clone();
    keep_side(original, local, Side::Head);
    keep_side(&mut tail, local, Side::Tail);

    let Some((position, parent_path)) = span.path.split_last() else { return };
    let position = *position;
    let Some(parent) = element_at_path_mut(paragraph, parent_path) else { return };
    parent.insert_element(position + 1, tail);
}

/// Whether a run already says exactly what a change would say.
///
/// Judged on what the run itself records, not on what it inherits. A run that
/// looks bold because its paragraph style is bold still gets an explicit `w:b`
/// when the user asks for bold, so the text stays bold if the style is later
/// changed — which is what the user was asking for.
fn run_satisfies(run: &Element, change: &RunProperties) -> bool {
    let direct = run.child(Some(W), "rPr").map(read::read_run_properties).unwrap_or_default();

    let same = |wanted: bool, equal: bool| !wanted || equal;
    same(change.bold.is_some(), direct.bold == change.bold)
        && same(change.italic.is_some(), direct.italic == change.italic)
        && same(change.strike.is_some(), direct.strike == change.strike)
        && same(change.underline.is_some(), direct.underline == change.underline)
        && same(change.color.is_some(), direct.color == change.color)
        && same(
            change.size_half_points.is_some(),
            direct.size_half_points == change.size_half_points,
        )
        && same(change.font.is_some(), direct.font == change.font)
        && same(change.highlight.is_some(), direct.highlight == change.highlight)
        && same(change.vertical_align.is_some(), direct.vertical_align == change.vertical_align)
        && same(change.right_to_left.is_some(), direct.right_to_left == change.right_to_left)
        && same(change.language.is_some(), direct.language == change.language)
        && same(change.style.is_some(), direct.style == change.style)
        && same(
            change.effect.is_some(),
            change.effect.as_ref().is_none_or(|wanted| effect_matches(&direct, wanted)),
        )
        && same(change.double_strike.is_some(), direct.double_strike == change.double_strike)
        && same(change.caps.is_some(), direct.caps == change.caps)
        && same(change.small_caps.is_some(), direct.small_caps == change.small_caps)
        && same(change.hidden.is_some(), direct.hidden == change.hidden)
        && same(change.underline_color.is_some(), direct.underline_color == change.underline_color)
        // The value that means "normal" is stored by writing nothing, so a run
        // asked for a hundred per cent when it says nothing already has it.
        && same(change.scale.is_some(), normal_or_equal(direct.scale, change.scale, 100))
        && same(
            change.spacing_twentieths.is_some(),
            normal_or_equal(direct.spacing_twentieths, change.spacing_twentieths, 0),
        )
        && same(
            change.position_half_points.is_some(),
            normal_or_equal(direct.position_half_points, change.position_half_points, 0),
        )
        && same(
            change.kerning_half_points.is_some(),
            direct.kerning_half_points == change.kerning_half_points,
        )
        && same(change.open_type.is_some(), open_type_matches(&direct, change))
}

/// Whether a run already says what is being asked of it, counting the value
/// that means "normal" as the same as saying nothing at all.
fn normal_or_equal<T: Copy + PartialEq>(had: Option<T>, wanted: Option<T>, normal: T) -> bool {
    had.unwrap_or(normal) == wanted.unwrap_or(normal)
}

/// Whether a run already asks the font for what is being asked of it.
fn open_type_matches(direct: &RunProperties, change: &RunProperties) -> bool {
    let wanted = change.open_type.clone().unwrap_or_default();
    direct.open_type.clone().unwrap_or_default() == wanted
}

/// Whether a run already has the effect being asked for.
///
/// Taking the effect off a run that never had one changes nothing, so those two
/// count as the same run — otherwise every press of "no effect" would record an
/// undo step with nothing in it.
fn effect_matches(direct: &RunProperties, wanted: &crate::effects::TextEffect) -> bool {
    if wanted.effect == crate::effects::Effect::None {
        return direct.effect.as_ref().is_none_or(|had| had.effect == crate::effects::Effect::None);
    }
    direct.effect.as_ref() == Some(wanted)
}

/// Whether applying a change to a range would alter anything.
///
/// Two things depend on this. An undo step is only recorded when there is
/// something to take back, so pressing a formatting key on text that already
/// has it does nothing at all. And runs are only split when the split is
/// needed: typing one letter at a time with bold chosen would otherwise leave
/// one run per letter, because each letter would be cut out of the run before
/// it.
#[must_use]
pub(crate) fn range_needs_change(
    paragraph: &Element,
    start: usize,
    end: usize,
    change: &RunProperties,
) -> bool {
    if end <= start || change.is_empty() {
        return false;
    }
    collect_run_spans(paragraph).iter().any(|span| {
        if span.length == 0 || span.start + span.length <= start || span.start >= end {
            return false;
        }
        path_to_element(paragraph, &span.path).is_some_and(|run| !run_satisfies(run, change))
    })
}

/// Applies authored properties to every run covering a range of one paragraph.
///
/// Only the fields the change actually names are touched, so turning on bold
/// leaves the colour, the font and everything else exactly as it was.
pub(crate) fn apply_to_range(
    paragraph: &mut Element,
    start: usize,
    end: usize,
    change: &RunProperties,
    prefix: Option<&str>,
) -> bool {
    if !range_needs_change(paragraph, start, end, change) {
        return false;
    }

    split_runs_at(paragraph, start);
    split_runs_at(paragraph, end);

    let mut changed = false;
    for span in collect_run_spans(paragraph) {
        let inside = span.start >= start && span.start + span.length <= end;
        // An empty run is worth formatting only when it is genuinely inside the
        // range, not when it happens to sit on either edge of it.
        let meaningful = span.length > 0 || (span.start > start && span.start < end);
        if !inside || !meaningful {
            continue;
        }
        if let Some(run) = element_at_path_mut(paragraph, &span.path) {
            apply_to_run(run, change, prefix);
            changed = true;
        }
    }

    changed
}

/// Writes authored properties onto one run, replacing what it said before.
fn apply_to_run(run: &mut Element, change: &RunProperties, prefix: Option<&str>) {
    if run.child(Some(W), "rPr").is_none() {
        // Run properties must be the first child of the run.
        run.insert_element(0, Element::new(&name_with(prefix, "rPr"), Some(W)));
    }
    let properties = run.child_mut(Some(W), "rPr").expect("just ensured");
    write_run_properties(properties, change, prefix);
}

/// Writes authored properties into a `w:rPr`, wherever that `rPr` lives.
///
/// A run has one, and so does a style, and so does the document's own set of
/// defaults. Word's Set As Default writes into the last of those, and it must
/// write it exactly as a run would — a property spelt one way in one place and
/// another way elsewhere is a document that formats differently depending on
/// where the formatting came from.
pub(crate) fn write_run_properties(
    properties: &mut Element,
    change: &RunProperties,
    prefix: Option<&str>,
) {
    if let Some(state) = change.bold {
        // Latin and complex-script weight are separate properties; setting one
        // without the other leaves Arabic and Hebrew text unbolded.
        set_toggle(properties, "b", state, prefix);
        set_toggle(properties, "bCs", state, prefix);
    }
    if let Some(state) = change.italic {
        set_toggle(properties, "i", state, prefix);
        set_toggle(properties, "iCs", state, prefix);
    }
    if let Some(state) = change.strike {
        set_toggle(properties, "strike", state, prefix);
    }
    if let Some(state) = change.double_strike {
        set_toggle(properties, "dstrike", state, prefix);
    }
    if let Some(state) = change.caps {
        set_toggle(properties, "caps", state, prefix);
    }
    if let Some(state) = change.small_caps {
        set_toggle(properties, "smallCaps", state, prefix);
    }
    if let Some(state) = change.hidden {
        set_toggle(properties, "vanish", state, prefix);
    }
    if let Some(underline) = &change.underline {
        properties.remove_children_named(Some(W), "u");
        let mut line = valued(prefix, "u", underline.to_attribute());
        // The colour rides on the same element as the style, so it has to be
        // written here rather than in an arm of its own — and a change that
        // names a colour without naming a style would have nothing to ride on.
        if let Some(color) = &change.underline_color {
            line.set_namespaced_attribute(&name_with(prefix, "color"), W, color);
        }
        insert_ordered(properties, line, RUN_PROPERTY_ORDER);
    }
    // The four measured properties of the Advanced tab. Each is written as the
    // number the format wants, and the value that means "normal" is written by
    // taking the element away: a run that says 100 per cent and one that says
    // nothing are the same run.
    for (local, value, normal) in [
        ("w", change.scale.map(i64::from), i64::from(crate::typography::NORMAL_SCALE)),
        ("spacing", change.spacing_twentieths.map(i64::from), 0),
        ("position", change.position_half_points.map(i64::from), 0),
    ] {
        let Some(value) = value else { continue };
        properties.remove_children_named(Some(W), local);
        if value != normal {
            insert_ordered(
                properties,
                valued(prefix, local, &value.to_string()),
                RUN_PROPERTY_ORDER,
            );
        }
    }
    if let Some(kerning) = change.kerning_half_points {
        // Kerning is the exception: zero is not "nothing said", it is "never
        // kern", and a run that means it has to say so.
        properties.remove_children_named(Some(W), "kern");
        insert_ordered(
            properties,
            valued(prefix, "kern", &kerning.to_string()),
            RUN_PROPERTY_ORDER,
        );
    }
    if let Some(wanted) = &change.open_type {
        crate::typography::remove_open_type(properties);
        crate::typography::write_open_type(properties, wanted);
    }
    if let Some(color) = &change.color {
        properties.remove_children_named(Some(W), "color");
        insert_ordered(properties, valued(prefix, "color", color), RUN_PROPERTY_ORDER);
    }
    if let Some(half_points) = change.size_half_points {
        let size = half_points.to_string();
        for local in ["sz", "szCs"] {
            properties.remove_children_named(Some(W), local);
            insert_ordered(properties, valued(prefix, local, &size), RUN_PROPERTY_ORDER);
        }
    }
    if let Some(highlight) = &change.highlight {
        // The band behind the letters. Word stores it by name from a fixed
        // palette, and "none" is written by taking the element away rather than
        // by naming it — a run that says `none` and one that says nothing are
        // the same run, and only one of them is tidy.
        properties.remove_children_named(Some(W), "highlight");
        if highlight != "none" {
            insert_ordered(properties, valued(prefix, "highlight", highlight), RUN_PROPERTY_ORDER);
        }
    }
    if let Some(alignment) = change.vertical_align {
        properties.remove_children_named(Some(W), "vertAlign");
        if alignment != VerticalAlignment::Baseline {
            insert_ordered(
                properties,
                valued(prefix, "vertAlign", alignment.to_attribute()),
                RUN_PROPERTY_ORDER,
            );
        }
    }
    if let Some(state) = change.right_to_left {
        // The direction of the text and the fact that it is a complex script
        // are two properties, and Word writes both: without the second the
        // right-to-left run is laid out the right way and measured with the
        // wrong font.
        set_toggle(properties, "rtl", state, prefix);
        set_toggle(properties, "cs", state, prefix);
    }
    if let Some(style) = &change.style {
        properties.remove_children_named(Some(W), "rStyle");
        insert_ordered(properties, valued(prefix, "rStyle", style), RUN_PROPERTY_ORDER);
    }
    if let Some(language) = &change.language {
        // Both the Latin tag and the one for complex scripts, because a run
        // that says only the first is checked in the wrong language wherever
        // the text is not Latin.
        properties.remove_children_named(Some(W), "lang");
        let mut element = Element::new(&name_with(prefix, "lang"), Some(W));
        element.set_namespaced_attribute(&name_with(prefix, "val"), W, language);
        element.set_namespaced_attribute(&name_with(prefix, "bidi"), W, language);
        insert_ordered(properties, element, RUN_PROPERTY_ORDER);
    }
    if let Some(effect) = &change.effect {
        // The effects are in Microsoft's namespace rather than the standard
        // one, and Word writes them at the front of the properties. Taking one
        // off is saying "no effect", which writes no element at all.
        crate::effects::remove_effects(properties);
        if let Some(element) = crate::effects::effect_element(effect, crate::effects::W14_PREFIX) {
            properties.insert_element(0, element);
        }
    }
    if let Some(font) = &change.font {
        properties.remove_children_named(Some(W), "rFonts");
        let mut fonts = Element::new(&name_with(prefix, "rFonts"), Some(W));
        for attribute in ["ascii", "hAnsi", "cs", "eastAsia"] {
            fonts.set_namespaced_attribute(&name_with(prefix, attribute), W, font);
        }
        insert_ordered(properties, fonts, RUN_PROPERTY_ORDER);
    }
}

fn set_toggle(properties: &mut Element, local: &str, state: bool, prefix: Option<&str>) {
    properties.remove_children_named(Some(W), local);
    insert_ordered(properties, toggle(prefix, local, state), RUN_PROPERTY_ORDER);
}

// --- Reading what the formatting currently is --------------------------------

/// The style a paragraph element names, if it names one.
fn paragraph_style_of(paragraph: &Element) -> Option<String> {
    paragraph
        .child(Some(W), "pPr")
        .and_then(|properties| properties.child(Some(W), "pStyle"))
        .and_then(read::value)
        .map(str::to_owned)
}

/// The resolved formatting of every run holding text in a range.
///
/// Overlapping rather than wholly inside: the question being asked is what the
/// selected characters look like, and a run half inside the selection still
/// contributes some of them.
#[must_use]
pub(crate) fn resolved_in_range(
    paragraph: &Element,
    start: usize,
    end: usize,
    styles: &Styles,
) -> Vec<ResolvedRunProperties> {
    let paragraph_style = paragraph_style_of(paragraph);
    let mut out = Vec::new();

    for span in collect_run_spans(paragraph) {
        if span.length == 0 || span.start + span.length <= start || span.start >= end {
            continue;
        }
        let Some(run) = path_to_element(paragraph, &span.path) else { continue };
        let direct = run.child(Some(W), "rPr").map(read::read_run_properties).unwrap_or_default();
        out.push(styles.resolve_run(paragraph_style.as_deref(), &direct));
    }

    out
}

/// What a paragraph's text would look like with no run properties of its own.
///
/// The answer for an empty paragraph: a new heading is bold before anything has
/// been typed into it, because the paragraph style says so.
#[must_use]
pub(crate) fn resolved_for_paragraph(
    paragraph: &Element,
    styles: &Styles,
) -> ResolvedRunProperties {
    styles.resolve_run(paragraph_style_of(paragraph).as_deref(), &RunProperties::default())
}

fn path_to_element<'a>(root: &'a Element, path: &[usize]) -> Option<&'a Element> {
    let mut current = root;
    for &index in path {
        current = current.children.get(index)?.as_element()?;
    }
    Some(current)
}

// --- Paragraph formatting ----------------------------------------------------

/// Sets or clears the style of one paragraph element.
pub(crate) fn set_paragraph_style(
    paragraph: &mut Element,
    style: Option<&str>,
    prefix: Option<&str>,
) {
    let properties = paragraph_properties_of(paragraph, prefix);
    properties.remove_children_named(Some(W), "pStyle");
    if let Some(style) = style {
        insert_ordered(properties, valued(prefix, "pStyle", style), PARAGRAPH_PROPERTY_ORDER);
    }
}

/// Sets the alignment of one paragraph element.
pub(crate) fn set_paragraph_alignment(
    paragraph: &mut Element,
    alignment: Alignment,
    prefix: Option<&str>,
) {
    let properties = paragraph_properties_of(paragraph, prefix);
    properties.remove_children_named(Some(W), "jc");
    insert_ordered(
        properties,
        valued(prefix, "jc", alignment.to_attribute()),
        PARAGRAPH_PROPERTY_ORDER,
    );
}

/// Puts one paragraph into a list, or takes it out of one.
pub(crate) fn set_paragraph_numbering(
    paragraph: &mut Element,
    list: Option<NumberingReference>,
    prefix: Option<&str>,
) {
    let properties = paragraph_properties_of(paragraph, prefix);
    properties.remove_children_named(Some(W), "numPr");

    let Some(list) = list else { return };
    let mut reference = Element::new(&name_with(prefix, "numPr"), Some(W));
    reference.push_element(valued(prefix, "ilvl", &list.level.to_string()));
    reference.push_element(valued(prefix, "numId", &list.id.to_string()));
    insert_ordered(properties, reference, PARAGRAPH_PROPERTY_ORDER);
}

/// Sets how far one paragraph is indented from the left margin.
pub(crate) fn set_paragraph_indent(paragraph: &mut Element, twips: i32, prefix: Option<&str>) {
    let properties = paragraph_properties_of(paragraph, prefix);

    // The hanging indent, if there is one, is kept: it is about where the
    // first line sits relative to the rest, not about the margin.
    let hanging = properties
        .child(Some(W), "ind")
        .and_then(|element| element.attribute(Some(W), "hanging"))
        .map(str::to_owned);

    properties.remove_children_named(Some(W), "ind");
    let mut indent = Element::new(&name_with(prefix, "ind"), Some(W));
    indent.set_namespaced_attribute(&name_with(prefix, "start"), W, &twips.to_string());
    indent.set_namespaced_attribute(&name_with(prefix, "left"), W, &twips.to_string());
    if let Some(hanging) = hanging {
        indent.set_namespaced_attribute(&name_with(prefix, "hanging"), W, &hanging);
    }
    insert_ordered(properties, indent, PARAGRAPH_PROPERTY_ORDER);
}

/// Sets every indent of one paragraph at once.
///
/// `first_line` is signed the way the model reads it: positive puts the first
/// line further in than the rest, negative hangs it further out. The format has
/// no signed attribute for that — it has two unsigned ones and expects exactly
/// the right one — so this is where the sign is turned back into a name.
pub(crate) fn set_paragraph_indents(
    paragraph: &mut Element,
    start: i32,
    first_line: i32,
    end: i32,
    prefix: Option<&str>,
) {
    let properties = paragraph_properties_of(paragraph, prefix);
    properties.remove_children_named(Some(W), "ind");

    let mut indent = Element::new(&name_with(prefix, "ind"), Some(W));
    // Both spellings of each: `start`/`end` are what the standard says and
    // `left`/`right` are what every version of Word before 2013 wrote and every
    // version since still reads.
    indent.set_namespaced_attribute(&name_with(prefix, "start"), W, &start.to_string());
    indent.set_namespaced_attribute(&name_with(prefix, "left"), W, &start.to_string());
    indent.set_namespaced_attribute(&name_with(prefix, "end"), W, &end.to_string());
    indent.set_namespaced_attribute(&name_with(prefix, "right"), W, &end.to_string());

    match first_line.cmp(&0) {
        core::cmp::Ordering::Greater => {
            indent.set_namespaced_attribute(
                &name_with(prefix, "firstLine"),
                W,
                &first_line.to_string(),
            );
        }
        core::cmp::Ordering::Less => {
            indent.set_namespaced_attribute(
                &name_with(prefix, "hanging"),
                W,
                &(-first_line).to_string(),
            );
        }
        core::cmp::Ordering::Equal => {}
    }

    insert_ordered(properties, indent, PARAGRAPH_PROPERTY_ORDER);
}

/// Sets the tab stops of one paragraph, replacing whatever it had.
///
/// An empty list takes the `w:tabs` away altogether, so the paragraph falls
/// back to its style and then to the document's default grid — which is what
/// clearing the stops means and what Word leaves behind when you do it.
pub(crate) fn set_paragraph_tab_stops(
    paragraph: &mut Element,
    stops: &[TabStop],
    prefix: Option<&str>,
) {
    let properties = paragraph_properties_of(paragraph, prefix);
    properties.remove_children_named(Some(W), "tabs");
    if stops.is_empty() {
        return;
    }
    let element = crate::edit::tab_stops_element(stops, prefix);
    insert_ordered(properties, element, PARAGRAPH_PROPERTY_ORDER);
}

/// Sets the line spacing of one paragraph.
pub(crate) fn set_paragraph_line_spacing(
    paragraph: &mut Element,
    spacing: Option<LineSpacing>,
    prefix: Option<&str>,
) {
    let properties = paragraph_properties_of(paragraph, prefix);

    // The space before and after a paragraph lives in the same element and is
    // a different thing, so it is carried across rather than thrown away.
    let existing = properties.child(Some(W), "spacing");
    let before =
        existing.and_then(|element| element.attribute(Some(W), "before")).map(str::to_owned);
    let after = existing.and_then(|element| element.attribute(Some(W), "after")).map(str::to_owned);

    properties.remove_children_named(Some(W), "spacing");
    let mut element = Element::new(&name_with(prefix, "spacing"), Some(W));
    if let Some(before) = before {
        element.set_namespaced_attribute(&name_with(prefix, "before"), W, &before);
    }
    if let Some(after) = after {
        element.set_namespaced_attribute(&name_with(prefix, "after"), W, &after);
    }
    if let Some(spacing) = spacing {
        element.set_namespaced_attribute(&name_with(prefix, "line"), W, &spacing.value.to_string());
        let rule = match spacing.rule {
            LineRule::Auto => "auto",
            LineRule::Exact => "exact",
            LineRule::AtLeast => "atLeast",
        };
        element.set_namespaced_attribute(&name_with(prefix, "lineRule"), W, rule);
    }
    insert_ordered(properties, element, PARAGRAPH_PROPERTY_ORDER);
}

/// Removes the direct character formatting from a range of one paragraph.
///
/// Only what the runs themselves say. A character style is left alone, and so
/// is everything the paragraph's style contributes: clearing formatting means
/// undoing what was applied by hand.
pub(crate) fn clear_run_properties(paragraph: &mut Element, start: usize, end: usize) -> bool {
    if end <= start {
        return false;
    }
    split_runs_at(paragraph, start);
    split_runs_at(paragraph, end);

    let mut changed = false;
    for span in collect_run_spans(paragraph) {
        if span.length == 0 || span.start < start || span.start + span.length > end {
            continue;
        }
        let Some(run) = element_at_path_mut(paragraph, &span.path) else { continue };
        if run.child(Some(W), "rPr").is_some() {
            run.remove_children_named(Some(W), "rPr");
            changed = true;
        }
    }

    changed
}

/// Puts a set of borders round one paragraph, or takes them off.
pub(crate) fn set_paragraph_borders(
    paragraph: &mut Element,
    borders: &ParagraphBorders,
    prefix: Option<&str>,
) {
    let properties = paragraph_properties_of(paragraph, prefix);
    properties.remove_children_named(Some(W), "pBdr");

    if borders.is_empty() {
        return;
    }
    insert_ordered(
        properties,
        crate::edit::paragraph_borders_element(borders, prefix),
        PARAGRAPH_PROPERTY_ORDER,
    );
}

/// Sets the colour behind one paragraph, or takes it away.
pub(crate) fn set_paragraph_shading(
    paragraph: &mut Element,
    fill: Option<&str>,
    prefix: Option<&str>,
) {
    let properties = paragraph_properties_of(paragraph, prefix);
    properties.remove_children_named(Some(W), "shd");

    let Some(fill) = fill else { return };
    let mut shading = Element::new(&name_with(prefix, "shd"), Some(W));
    shading.set_namespaced_attribute(&name_with(prefix, "val"), W, "clear");
    shading.set_namespaced_attribute(&name_with(prefix, "color"), W, "auto");
    shading.set_namespaced_attribute(&name_with(prefix, "fill"), W, fill);
    insert_ordered(properties, shading, PARAGRAPH_PROPERTY_ORDER);
}

/// Applies a whole set of paragraph properties to one `w:p`.
///
/// What Word's Paragraph dialog does when it is answered: every property it
/// asks about is written, and the ones it does not ask about are left exactly
/// as they were.
pub(crate) fn apply_paragraph_properties(
    paragraph: &mut Element,
    change: &ParagraphProperties,
    prefix: Option<&str>,
) {
    let properties = paragraph_properties_of(paragraph, prefix);
    write_paragraph_properties(properties, change, prefix);
}

/// The same, into a `w:pPr` wherever that `pPr` lives.
///
/// A paragraph has one, and so does a style, and so does the document's own set
/// of defaults. Word's Set As Default writes into the last of those, and it
/// must write it exactly as a paragraph would.
pub(crate) fn write_paragraph_properties(
    properties: &mut Element,
    change: &ParagraphProperties,
    prefix: Option<&str>,
) {
    // The on-or-off ones, each written where the schema puts it.
    for (local, state) in [
        ("keepNext", change.keep_next),
        ("keepLines", change.keep_lines),
        ("pageBreakBefore", change.page_break_before),
        ("widowControl", change.widow_control),
        ("suppressLineNumbers", change.suppress_line_numbers),
        ("suppressAutoHyphens", change.no_hyphenation),
        ("contextualSpacing", change.contextual_spacing),
        ("mirrorIndents", change.mirror_indents),
    ] {
        let Some(state) = state else { continue };
        set_toggle(properties, local, state, prefix);
    }

    if let Some(alignment) = change.alignment {
        properties.remove_children_named(Some(W), "jc");
        insert_ordered(
            properties,
            valued(prefix, "jc", alignment.to_attribute()),
            PARAGRAPH_PROPERTY_ORDER,
        );
    }
    // Body text is no level at all, which is what saying nothing means — so a
    // change that names none takes the element away rather than writing a zero.
    properties.remove_children_named(Some(W), "outlineLvl");
    if let Some(level) = change.outline_level {
        insert_ordered(
            properties,
            valued(prefix, "outlineLvl", &level.to_string()),
            PARAGRAPH_PROPERTY_ORDER,
        );
    }

    // The three indents share one element, so they are written together and
    // only when the change names at least one of them.
    if change.indent_start.is_some()
        || change.indent_end.is_some()
        || change.indent_first_line.is_some()
    {
        let existing = properties.child(Some(W), "ind");
        let kept = |name: &str, fallback: &str| {
            existing
                .and_then(|element| element.attribute(Some(W), name))
                .or_else(|| existing.and_then(|element| element.attribute(Some(W), fallback)))
                .and_then(|text| text.parse::<i32>().ok())
                .unwrap_or(0)
        };
        let hanging = existing
            .and_then(|element| element.attribute(Some(W), "hanging"))
            .and_then(|text| text.parse::<i32>().ok())
            .map(|value| -value);
        let first = change
            .indent_first_line
            .unwrap_or_else(|| hanging.unwrap_or_else(|| kept("firstLine", "firstLine")));

        let start = change.indent_start.unwrap_or_else(|| kept("start", "left"));
        let end = change.indent_end.unwrap_or_else(|| kept("end", "right"));

        properties.remove_children_named(Some(W), "ind");
        let mut indent = Element::new(&name_with(prefix, "ind"), Some(W));
        // Both spellings of each: `start`/`end` are what the standard says and
        // `left`/`right` are what every version of Word before 2013 wrote and
        // every version since still reads.
        for (name, value) in [("start", start), ("left", start), ("end", end), ("right", end)] {
            indent.set_namespaced_attribute(&name_with(prefix, name), W, &value.to_string());
        }
        // A first line pushed in and one pulled out are two attributes, and
        // writing both would be a paragraph that says two things.
        match first.cmp(&0) {
            core::cmp::Ordering::Greater => {
                indent.set_namespaced_attribute(
                    &name_with(prefix, "firstLine"),
                    W,
                    &first.to_string(),
                );
            }
            core::cmp::Ordering::Less => {
                indent.set_namespaced_attribute(
                    &name_with(prefix, "hanging"),
                    W,
                    &(-first).to_string(),
                );
            }
            core::cmp::Ordering::Equal => {}
        }
        insert_ordered(properties, indent, PARAGRAPH_PROPERTY_ORDER);
    }

    // Space before and after, and the line spacing, share one element too.
    if change.space_before.is_some()
        || change.space_after.is_some()
        || change.line_spacing.is_some()
    {
        let existing = properties.child(Some(W), "spacing");
        let kept = |name: &str| {
            existing
                .and_then(|element| element.attribute(Some(W), name))
                .and_then(|text| text.parse::<i32>().ok())
        };
        let before = change.space_before.or_else(|| kept("before"));
        let after = change.space_after.or_else(|| kept("after"));

        properties.remove_children_named(Some(W), "spacing");
        let mut element = Element::new(&name_with(prefix, "spacing"), Some(W));
        if let Some(before) = before {
            element.set_namespaced_attribute(&name_with(prefix, "before"), W, &before.to_string());
        }
        if let Some(after) = after {
            element.set_namespaced_attribute(&name_with(prefix, "after"), W, &after.to_string());
        }
        if let Some(spacing) = change.line_spacing {
            element.set_namespaced_attribute(
                &name_with(prefix, "line"),
                W,
                &spacing.value.to_string(),
            );
            let rule = match spacing.rule {
                LineRule::Auto => "auto",
                LineRule::Exact => "exact",
                LineRule::AtLeast => "atLeast",
            };
            element.set_namespaced_attribute(&name_with(prefix, "lineRule"), W, rule);
        }
        insert_ordered(properties, element, PARAGRAPH_PROPERTY_ORDER);
    }

    if !change.tab_stops.is_empty() {
        properties.remove_children_named(Some(W), "tabs");
        insert_ordered(
            properties,
            crate::edit::tab_stops_element(&change.tab_stops, prefix),
            PARAGRAPH_PROPERTY_ORDER,
        );
    }
}
