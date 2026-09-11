//! Changing a document without disturbing the rest of it.
//!
//! Every operation here works on the element tree and touches only the nodes it
//! must. That is what makes editing an opened document safe: the parts nobody
//! here understands are never rewritten, because they are never visited.
//!
//! # Why replacing text is not simply a string replace
//!
//! Word splits a paragraph's text across runs wherever formatting changes — and
//! also wherever it feels like it, after a spell check or an edit. The word
//! "hello" can easily be stored as `<w:t>he</w:t>` in one run and
//! `<w:t>llo</w:t>` in another, with a bookmark between them. A search that
//! looked at one `w:t` at a time would simply not find it.
//!
//! So a paragraph's text is assembled first, the search runs against that, and
//! each match is then written back into the elements it actually spans.

use wp_xml::tree::{Element, Node};

use crate::model::{
    Alignment, Block, Border, BreakKind, LineRule, Paragraph, ParagraphBorders,
    ParagraphProperties, RevisionKind, Run, RunContent, RunProperties, TabAlignment, TabLeader,
    TabStop, Table, TableBorders,
};
use crate::read::W;

/// The namespace of the reserved `xml` prefix, which `xml:space` belongs to.
pub(crate) const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

/// The order the schema requires for the children of `w:pPr`.
///
/// Word rejects a document whose properties are out of sequence, so anything
/// inserted has to go in the right place rather than simply at the end.
pub(crate) const PARAGRAPH_PROPERTY_ORDER: &[&str] = &[
    "pStyle",
    "keepNext",
    "keepLines",
    "pageBreakBefore",
    "framePr",
    "widowControl",
    "numPr",
    "suppressLineNumbers",
    "suppressAutoHyphens",
    "pBdr",
    "shd",
    "tabs",
    "bidi",
    "spacing",
    "ind",
    "contextualSpacing",
    "mirrorIndents",
    "jc",
    "textDirection",
    "textAlignment",
    "outlineLvl",
    "rPr",
    "sectPr",
];

/// The same, for the children of `w:rPr`.
pub(crate) const RUN_PROPERTY_ORDER: &[&str] = &[
    "rStyle",
    "rFonts",
    "b",
    "bCs",
    "i",
    "iCs",
    "caps",
    "smallCaps",
    "strike",
    "dstrike",
    "vanish",
    "color",
    "spacing",
    "w",
    "kern",
    "position",
    "sz",
    "szCs",
    "highlight",
    "u",
    "effect",
    "bdr",
    "shd",
    "vertAlign",
    "rtl",
    "cs",
    "em",
    "lang",
];

/// Finds the prefix a document uses for a namespace.
///
/// Almost every document binds the WordprocessingML namespace to `w`, but
/// nothing requires it. Building `w:t` into a document that called it something
/// else would produce an undeclared prefix and a file Word refuses to open.
#[must_use]
pub fn prefix_for(root: &Element, namespace: &str) -> Option<String> {
    fn search(element: &Element, namespace: &str) -> Option<Option<String>> {
        for (prefix, uri) in &element.declarations {
            if uri == namespace {
                return Some(prefix.clone());
            }
        }
        element.child_elements().find_map(|child| search(child, namespace))
    }

    // An element already in that namespace also reveals the prefix, which covers
    // documents that declare it somewhere unusual.
    search(root, namespace).unwrap_or_else(|| {
        find_in_namespace(root, namespace).and_then(|found| found.prefix().map(str::to_owned))
    })
}

fn find_in_namespace<'a>(element: &'a Element, namespace: &str) -> Option<&'a Element> {
    if element.namespace.as_deref() == Some(namespace) {
        return Some(element);
    }
    element.child_elements().find_map(|child| find_in_namespace(child, namespace))
}

/// Builds a written element name for the WordprocessingML namespace.
pub(crate) fn name_with(prefix: Option<&str>, local: &str) -> String {
    match prefix {
        Some(prefix) => format!("{prefix}:{local}"),
        None => local.to_owned(),
    }
}

/// Inserts a property where the schema says it belongs.
pub(crate) fn insert_ordered(parent: &mut Element, child: Element, order: &[&str]) {
    let local = child.local_name().to_owned();
    let rank = order.iter().position(|name| *name == local);

    // An unknown property goes at the end, which is the least surprising place
    // for something the order list does not mention.
    let Some(rank) = rank else {
        parent.push_element(child);
        return;
    };

    let position = parent
        .children
        .iter()
        .position(|node| {
            node.as_element().is_some_and(|existing| {
                order
                    .iter()
                    .position(|name| *name == existing.local_name())
                    .is_none_or(|existing_rank| existing_rank > rank)
            })
        })
        .unwrap_or(parent.children.len());

    parent.insert_element(position, child);
}

// --- Finding and replacing text ---------------------------------------------

/// Where one piece of a paragraph's text sits, and what it holds.
pub(crate) struct TextPiece {
    /// Indices into `children` at each level, from the paragraph down.
    pub(crate) path: Vec<usize>,
    pub(crate) text: String,
    /// Byte offset of this piece within the paragraph's assembled text.
    pub(crate) start: usize,
    /// Whether the piece is an element standing for one character rather than
    /// text that can be edited in place — a tab or a line break.
    pub(crate) atomic: bool,
    /// Whether the piece is the cached answer of a field.
    ///
    /// Such text is not typed into: it is what something worked out, and it is
    /// replaced whole the next time anything works the field out. Typing at the
    /// end of a page number must land after the number, not inside it.
    pub(crate) in_field: bool,
}

/// The character an element stands for, if it stands for one.
///
/// A tab and a line break are elements, not text, but a person moving the caret
/// through a paragraph passes over them like any other character. Counting them
/// as one character each is what lets the caret sit either side of a tab and
/// Backspace delete it — and it is what makes the offsets the layout engine
/// produces and the offsets the editor uses the same numbers.
#[must_use]
pub(crate) fn atomic_text(element: &Element) -> Option<&'static str> {
    // An equation stands in the text as one character, the same as a picture.
    if element.namespace.as_deref() == Some(crate::math::MATH_NAMESPACE)
        && matches!(element.local_name(), "oMath" | "oMathPara")
    {
        return Some("\u{1}");
    }
    if element.namespace.as_deref() != Some(W) {
        return None;
    }
    match element.local_name() {
        "tab" => Some("\t"),
        "br" => Some("\n"),
        // A picture stands in the text as one character, so the caret can be
        // put either side of it and Backspace can reach it. U+0001 rather than
        // the object replacement character because it has to be one byte long:
        // the layout counts a picture as one byte of the paragraph as well, and
        // an offset has to mean the same thing in both.
        "drawing" => Some("\u{1}"),
        // The mark that points at a footnote or an endnote is one character
        // too, for the same reasons.
        "footnoteReference" | "endnoteReference" => Some("\u{2}"),
        _ => None,
    }
}

/// Replaces every occurrence of `needle` in the document, returning how many
/// were changed.
///
/// The search is case-sensitive and works across run boundaries. An empty
/// `needle` matches nothing, rather than looping forever on the empty string.
pub fn replace_text(root: &mut Element, needle: &str, replacement: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }

    let mut replaced = 0;
    replace_in_paragraphs(root, needle, replacement, &mut replaced);
    replaced
}

fn replace_in_paragraphs(
    element: &mut Element,
    needle: &str,
    replacement: &str,
    replaced: &mut usize,
) {
    if element.is(Some(W), "p") {
        *replaced += replace_in_paragraph(element, needle, replacement);
        return;
    }
    for child in element.child_elements_mut() {
        replace_in_paragraphs(child, needle, replacement, replaced);
    }
}

/// Replaces inside one paragraph. Returns the number of occurrences changed.
fn replace_in_paragraph(paragraph: &mut Element, needle: &str, replacement: &str) -> usize {
    let pieces = collect_text_pieces(paragraph);
    if pieces.is_empty() {
        return 0;
    }

    let full_text: String = pieces.iter().map(|piece| piece.text.as_str()).collect();
    let matches = find_all(&full_text, needle);
    if matches.is_empty() {
        return 0;
    }

    // Rewriting is done piece by piece against absolute offsets, so the order
    // does not matter and no offset ever goes stale.
    for piece in &pieces {
        // A tab or a break is an element, not text: there is nothing in it to
        // rewrite, and a match that runs over one leaves it where it is.
        if piece.atomic {
            continue;
        }
        let rebuilt = rebuild_piece(piece, &matches, replacement);
        if rebuilt == piece.text {
            continue;
        }
        if let Some(element) = element_at_path_mut(paragraph, &piece.path) {
            element.set_text(&rebuilt);
            preserve_space_if_needed(element, &rebuilt);
        }
    }

    matches.len()
}

/// Replaces several stretches of one paragraph at once.
///
/// Given ranges into the paragraph's assembled text, and what each becomes.
/// The ranges must not overlap and must be in order, which is what
/// [`crate::translate::Glossary::matches`] gives.
///
/// Returns how many were replaced. Works the same way one replacement does:
/// each new word is written into the run its match starts in, and the
/// characters it covers are dropped wherever they were.
pub(crate) fn replace_ranges(paragraph: &mut Element, ranges: &[(usize, usize, String)]) -> usize {
    let pieces = collect_text_pieces(paragraph);
    if pieces.is_empty() || ranges.is_empty() {
        return 0;
    }

    for piece in &pieces {
        // A tab or a break is an element, not text: there is nothing in it to
        // rewrite.
        if piece.atomic {
            continue;
        }
        let rebuilt = rebuild_piece_with(piece, ranges);
        if rebuilt == piece.text {
            continue;
        }
        if let Some(element) = element_at_path_mut(paragraph, &piece.path) {
            element.set_text(&rebuilt);
            preserve_space_if_needed(element, &rebuilt);
        }
    }
    ranges.len()
}

/// Works out what one `w:t` should now contain, with a replacement per match.
fn rebuild_piece_with(piece: &TextPiece, ranges: &[(usize, usize, String)]) -> String {
    let mut out = String::with_capacity(piece.text.len());
    let mut local = 0usize;

    while local < piece.text.len() {
        let absolute = piece.start + local;

        if let Some((_, end, replacement)) = ranges.iter().find(|(start, _, _)| *start == absolute)
        {
            out.push_str(replacement);
            local = end.saturating_sub(piece.start).min(piece.text.len());
            continue;
        }

        let inside = ranges.iter().any(|(start, end, _)| absolute >= *start && absolute < *end);
        let character = piece.text[local..].chars().next().expect("on a character boundary");
        if !inside {
            out.push(character);
        }
        local += character.len_utf8();
    }
    out
}
/// Every non-overlapping occurrence, as byte ranges.
fn find_all(haystack: &str, needle: &str) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut from = 0usize;
    while let Some(offset) = haystack[from..].find(needle) {
        let start = from + offset;
        let end = start + needle.len();
        found.push((start, end));
        from = end;
    }
    found
}

/// Works out what one `w:t` should now contain.
///
/// A replacement is written into the piece where its match *starts*; characters
/// covered by a match are dropped wherever they were. A match spanning three
/// runs therefore leaves the replacement in the first and empties the other two.
fn rebuild_piece(piece: &TextPiece, matches: &[(usize, usize)], replacement: &str) -> String {
    let mut out = String::with_capacity(piece.text.len());
    let mut local = 0usize;

    while local < piece.text.len() {
        let absolute = piece.start + local;

        if let Some(&(_, end)) = matches.iter().find(|(start, _)| *start == absolute) {
            out.push_str(replacement);
            // Skip the matched text, which may run past the end of this piece.
            local = end.saturating_sub(piece.start).min(piece.text.len());
            continue;
        }

        let inside_match = matches.iter().any(|(start, end)| absolute >= *start && absolute < *end);
        let character = piece.text[local..].chars().next().expect("on a character boundary");
        if !inside_match {
            out.push(character);
        }
        local += character.len_utf8();
    }

    out
}

/// Collects every `w:t` under a paragraph, in document order.
pub(crate) fn collect_text_pieces(paragraph: &Element) -> Vec<TextPiece> {
    let mut pieces = Vec::new();
    let mut path = Vec::new();
    let mut offset = 0usize;
    walk_text_pieces(paragraph, &mut path, &mut offset, &mut pieces, false);
    pieces
}

fn walk_text_pieces(
    element: &Element,
    path: &mut Vec<usize>,
    offset: &mut usize,
    pieces: &mut Vec<TextPiece>,
    in_field: bool,
) {
    // A field written the long way is a run of markers among the runs; the text
    // between `separate` and `end` is the field's answer, and typing at the end
    // of it must land after the field rather than inside it. See
    // [`crate::fields`].
    let mut in_complex = false;

    for (index, node) in element.children.iter().enumerate() {
        let Node::Element(child) = node else { continue };
        // Everything but WordprocessingML is skipped, except an equation: it
        // is in a namespace of its own and stands for one character of the
        // text, so it has to be counted.
        let is_math = child.namespace.as_deref() == Some(crate::math::MATH_NAMESPACE);
        if child.namespace.as_deref() != Some(W) && !is_math {
            continue;
        }

        // Deleted text is not part of what the document says, so it is not
        // searched and never rewritten.
        if child.local_name() == "del" {
            continue;
        }

        if child.local_name() == "r" {
            match crate::fields::marker_of(child) {
                Some(crate::fields::Marker::Separate) => {
                    in_complex = true;
                    continue;
                }
                Some(crate::fields::Marker::End) => {
                    in_complex = false;
                    continue;
                }
                Some(crate::fields::Marker::Begin) => continue,
                None => {}
            }
        }

        path.push(index);
        let inside_field = in_field || in_complex || child.local_name() == "fldSimple";
        if child.local_name() == "t" {
            let text = child.text_content();
            let length = text.len();
            pieces.push(TextPiece {
                path: path.clone(),
                text,
                start: *offset,
                atomic: false,
                in_field: inside_field,
            });
            *offset += length;
        } else if let Some(text) = atomic_text(child) {
            pieces.push(TextPiece {
                path: path.clone(),
                text: text.to_owned(),
                start: *offset,
                atomic: true,
                in_field: inside_field,
            });
            *offset += text.len();
        } else {
            walk_text_pieces(child, path, offset, pieces, inside_field);
        }
        path.pop();
    }
}

/// Resolves a child path back to an element.
pub(crate) fn element_at_path<'a>(root: &'a Element, path: &[usize]) -> Option<&'a Element> {
    let mut current = root;
    for &index in path {
        current = current.children.get(index)?.as_element()?;
    }
    Some(current)
}

/// Resolves a child path back to a mutable element.
pub(crate) fn element_at_path_mut<'a>(
    root: &'a mut Element,
    path: &[usize],
) -> Option<&'a mut Element> {
    let mut current = root;
    for &index in path {
        current = current.children.get_mut(index)?.as_element_mut()?;
    }
    Some(current)
}

/// Adds `xml:space="preserve"` when the text needs it, and removes it when it
/// does not.
///
/// Without the attribute a leading or trailing space is collapsed away on the
/// next read, and two words silently run together.
pub(crate) fn preserve_space_if_needed(element: &mut Element, text: &str) {
    let needs_it = text.starts_with(char::is_whitespace) || text.ends_with(char::is_whitespace);
    if needs_it {
        element.set_namespaced_attribute("xml:space", XML_NAMESPACE, "preserve");
    } else {
        element.remove_attribute("xml:space");
    }
}

// --- Building elements from the model ---------------------------------------

/// An element carrying only a `w:val` attribute, the format's commonest shape.
pub(crate) fn valued(prefix: Option<&str>, local: &str, value: &str) -> Element {
    let mut element = Element::new(&name_with(prefix, local), Some(W));
    element.set_namespaced_attribute(&name_with(prefix, "val"), W, value);
    element
}

/// An on/off element, written only when it says something.
pub(crate) fn toggle(prefix: Option<&str>, local: &str, state: bool) -> Element {
    if state {
        Element::new(&name_with(prefix, local), Some(W))
    } else {
        // Explicitly off, which is how a run overrides its style.
        valued(prefix, local, "0")
    }
}

/// Turns a list of tab stops into a `w:tabs`.
///
/// In order along the line, because that is the order Word writes them in and
/// the order anything reading them back expects.
#[must_use]
pub fn tab_stops_element(stops: &[TabStop], prefix: Option<&str>) -> Element {
    let mut element = Element::new(&name_with(prefix, "tabs"), Some(W));
    let mut sorted = stops.to_vec();
    sorted.sort_by_key(|stop| stop.position);
    for stop in sorted {
        let mut tab = Element::new(&name_with(prefix, "tab"), Some(W));
        tab.set_namespaced_attribute(&name_with(prefix, "val"), W, stop.alignment.word());
        if stop.leader != TabLeader::None {
            tab.set_namespaced_attribute(&name_with(prefix, "leader"), W, stop.leader.word());
        }
        tab.set_namespaced_attribute(&name_with(prefix, "pos"), W, &stop.position.to_string());
        element.push_element(tab);
    }
    element
}

/// Turns paragraph properties into a `w:pPr`.
#[must_use]
pub fn paragraph_properties_element(
    properties: &ParagraphProperties,
    prefix: Option<&str>,
) -> Element {
    let mut element = Element::new(&name_with(prefix, "pPr"), Some(W));
    // Built in schema order, so nothing has to be sorted afterwards.
    if let Some(style) = &properties.style {
        element.push_element(valued(prefix, "pStyle", style));
    }
    if let Some(state) = properties.keep_next {
        element.push_element(toggle(prefix, "keepNext", state));
    }
    if let Some(state) = properties.keep_lines {
        element.push_element(toggle(prefix, "keepLines", state));
    }
    if let Some(state) = properties.page_break_before {
        element.push_element(toggle(prefix, "pageBreakBefore", state));
    }
    if let Some(state) = properties.widow_control {
        element.push_element(toggle(prefix, "widowControl", state));
    }
    if let Some(state) = properties.suppress_line_numbers {
        element.push_element(toggle(prefix, "suppressLineNumbers", state));
    }
    if let Some(state) = properties.no_hyphenation {
        element.push_element(toggle(prefix, "suppressAutoHyphens", state));
    }
    if let Some(state) = properties.contextual_spacing {
        element.push_element(toggle(prefix, "contextualSpacing", state));
    }
    if let Some(state) = properties.mirror_indents {
        element.push_element(toggle(prefix, "mirrorIndents", state));
    }
    if let Some(numbering) = properties.numbering {
        let mut reference = Element::new(&name_with(prefix, "numPr"), Some(W));
        reference.push_element(valued(prefix, "ilvl", &numbering.level.to_string()));
        reference.push_element(valued(prefix, "numId", &numbering.id.to_string()));
        element.push_element(reference);
    }
    if !properties.borders.is_empty() {
        element.push_element(paragraph_borders_element(&properties.borders, prefix));
    }
    if let Some(fill) = &properties.shading {
        let mut shading = Element::new(&name_with(prefix, "shd"), Some(W));
        shading.set_namespaced_attribute(&name_with(prefix, "val"), W, "clear");
        shading.set_namespaced_attribute(&name_with(prefix, "color"), W, "auto");
        shading.set_namespaced_attribute(&name_with(prefix, "fill"), W, fill);
        element.push_element(shading);
    }
    if !properties.tab_stops.is_empty() {
        element.push_element(tab_stops_element(&properties.tab_stops, prefix));
    }
    if let Some(state) = properties.right_to_left {
        element.push_element(toggle(prefix, "bidi", state));
    }
    if properties.space_before.is_some()
        || properties.space_after.is_some()
        || properties.line_spacing.is_some()
    {
        let mut spacing = Element::new(&name_with(prefix, "spacing"), Some(W));
        if let Some(before) = properties.space_before {
            spacing.set_namespaced_attribute(&name_with(prefix, "before"), W, &before.to_string());
        }
        if let Some(after) = properties.space_after {
            spacing.set_namespaced_attribute(&name_with(prefix, "after"), W, &after.to_string());
        }
        if let Some(line) = properties.line_spacing {
            spacing.set_namespaced_attribute(
                &name_with(prefix, "line"),
                W,
                &line.value.to_string(),
            );
            let rule = match line.rule {
                LineRule::Auto => "auto",
                LineRule::Exact => "exact",
                LineRule::AtLeast => "atLeast",
            };
            spacing.set_namespaced_attribute(&name_with(prefix, "lineRule"), W, rule);
        }
        element.push_element(spacing);
    }
    if properties.indent_start.is_some()
        || properties.indent_end.is_some()
        || properties.indent_first_line.is_some()
    {
        let mut indent = Element::new(&name_with(prefix, "ind"), Some(W));
        if let Some(start) = properties.indent_start {
            indent.set_namespaced_attribute(&name_with(prefix, "start"), W, &start.to_string());
        }
        if let Some(end) = properties.indent_end {
            indent.set_namespaced_attribute(&name_with(prefix, "end"), W, &end.to_string());
        }
        if let Some(first) = properties.indent_first_line {
            // A negative first-line indent is written as a hanging indent.
            let (name, amount) = if first < 0 { ("hanging", -first) } else { ("firstLine", first) };
            indent.set_namespaced_attribute(&name_with(prefix, name), W, &amount.to_string());
        }
        element.push_element(indent);
    }
    if let Some(alignment) = properties.alignment {
        element.push_element(valued(prefix, "jc", alignment.to_attribute()));
    }
    if let Some(level) = properties.outline_level {
        element.push_element(valued(prefix, "outlineLvl", &level.to_string()));
    }

    element
}

/// Turns a paragraph from the model into an element ready to insert.
#[must_use]
pub fn paragraph_element(paragraph: &Paragraph, prefix: Option<&str>) -> Element {
    let mut element = Element::new(&name_with(prefix, "p"), Some(W));

    if !paragraph.properties.is_empty() {
        element.push_element(paragraph_properties_element(&paragraph.properties, prefix));
    }
    // Runs that belong to the same field or the same tracked change go back
    // inside one wrapper, or the field would be lost and only its cached answer
    // would remain, and a change would stop being a change.
    let mut index = 0usize;
    while index < paragraph.runs.len() {
        let run = &paragraph.runs[index];

        if let Some(revision) = &run.revision {
            let mut wrapper = Element::new(&name_with(prefix, revision.kind.element()), Some(W));
            wrapper.set_namespaced_attribute(&name_with(prefix, "id"), W, &revision.id.to_string());
            wrapper.set_namespaced_attribute(&name_with(prefix, "author"), W, &revision.author);
            wrapper.set_namespaced_attribute(&name_with(prefix, "date"), W, &revision.date);

            let deleted = revision.kind == RevisionKind::Deleted;
            while index < paragraph.runs.len()
                && paragraph.runs[index].revision.as_ref() == Some(revision)
            {
                wrapper.push_element(revised_run_element(&paragraph.runs[index], prefix, deleted));
                index += 1;
            }
            element.push_element(wrapper);
            continue;
        }

        // An equation is not written inside a run: it is a sibling of the runs,
        // in the namespace equations live in.
        if let Some(crate::model::RunContent::Math(math)) = run.content.first() {
            if run.content.len() == 1 {
                element.push_element(crate::math::math_element(math, crate::math::MATH_PREFIX));
                index += 1;
                continue;
            }
        }

        let Some(instruction) = &run.field else {
            element.push_element(run_element(run, prefix));
            index += 1;
            continue;
        };

        let mut field = Element::new(&name_with(prefix, "fldSimple"), Some(W));
        field.set_namespaced_attribute(&name_with(prefix, "instr"), W, &format!(" {instruction} "));
        while index < paragraph.runs.len()
            && paragraph.runs[index].field.as_deref() == Some(instruction.as_str())
        {
            field.push_element(run_element(&paragraph.runs[index], prefix));
            index += 1;
        }
        element.push_element(field);
    }

    element
}

/// Turns run properties into a `w:rPr`.
#[must_use]
pub fn run_properties_element(properties: &RunProperties, prefix: Option<&str>) -> Element {
    let mut element = Element::new(&name_with(prefix, "rPr"), Some(W));

    if let Some(style) = &properties.style {
        element.push_element(valued(prefix, "rStyle", style));
    }
    if let Some(font) = &properties.font {
        let mut fonts = Element::new(&name_with(prefix, "rFonts"), Some(W));
        // All four scripts get the same family: without w:cs, right-to-left and
        // East Asian text would silently fall back to a different font.
        for attribute in ["ascii", "hAnsi", "cs", "eastAsia"] {
            fonts.set_namespaced_attribute(&name_with(prefix, attribute), W, font);
        }
        element.push_element(fonts);
    }
    if let Some(state) = properties.bold {
        element.push_element(toggle(prefix, "b", state));
        // Complex-script text takes its weight from w:bCs, not w:b.
        element.push_element(toggle(prefix, "bCs", state));
    }
    if let Some(state) = properties.italic {
        element.push_element(toggle(prefix, "i", state));
        element.push_element(toggle(prefix, "iCs", state));
    }
    if let Some(state) = properties.strike {
        element.push_element(toggle(prefix, "strike", state));
    }
    if let Some(state) = properties.double_strike {
        element.push_element(toggle(prefix, "dstrike", state));
    }
    if let Some(state) = properties.caps {
        element.push_element(toggle(prefix, "caps", state));
    }
    if let Some(state) = properties.small_caps {
        element.push_element(toggle(prefix, "smallCaps", state));
    }
    if let Some(state) = properties.hidden {
        element.push_element(toggle(prefix, "vanish", state));
    }
    if let Some(scale) = properties.scale {
        element.push_element(valued(prefix, "w", &scale.to_string()));
    }
    if let Some(spacing) = properties.spacing_twentieths {
        element.push_element(valued(prefix, "spacing", &spacing.to_string()));
    }
    if let Some(position) = properties.position_half_points {
        element.push_element(valued(prefix, "position", &position.to_string()));
    }
    if let Some(kerning) = properties.kerning_half_points {
        element.push_element(valued(prefix, "kern", &kerning.to_string()));
    }
    if let Some(color) = &properties.color {
        element.push_element(valued(prefix, "color", color));
    }
    if let Some(highlight) = &properties.highlight {
        element.push_element(valued(prefix, "highlight", highlight));
    }
    if let Some(alignment) = properties.vertical_align {
        element.push_element(valued(prefix, "vertAlign", alignment.to_attribute()));
    }
    if let Some(half_points) = properties.size_half_points {
        let size = half_points.to_string();
        element.push_element(valued(prefix, "sz", &size));
        element.push_element(valued(prefix, "szCs", &size));
    }
    if let Some(underline) = &properties.underline {
        let mut line = valued(prefix, "u", underline.to_attribute());
        // The colour of the line rides on the same element as its style: the
        // format has no `w:uColor`.
        if let Some(color) = &properties.underline_color {
            line.set_namespaced_attribute(&name_with(prefix, "color"), W, color);
        }
        element.push_element(line);
    }
    if let Some(state) = properties.right_to_left {
        element.push_element(toggle(prefix, "rtl", state));
    }
    if let Some(language) = &properties.language {
        element.push_element(valued(prefix, "lang", language));
    }
    // Last, because it is in a namespace of its own and the schema wants the
    // standard properties in their standard order before it.
    if let Some(wanted) = &properties.open_type {
        crate::typography::write_open_type(&mut element, wanted);
    }

    element
}

/// Turns a run from the model into an element.
#[must_use]
pub fn run_element(run: &Run, prefix: Option<&str>) -> Element {
    revised_run_element(run, prefix, false)
}

/// The same, writing `w:delText` instead of `w:t` for deleted text.
///
/// The format insists on the different name: a reader that shows the document
/// as it would be with every change accepted skips `w:delText` and keeps `w:t`,
/// and it can only do that if the two are told apart by name.
#[must_use]
pub fn revised_run_element(run: &Run, prefix: Option<&str>, deleted: bool) -> Element {
    let mut element = Element::new(&name_with(prefix, "r"), Some(W));

    if !run.properties.is_empty() {
        element.push_element(run_properties_element(&run.properties, prefix));
    }

    for piece in &run.content {
        match piece {
            RunContent::Text(text) => {
                // A tab inside a run's text is an element of its own in the
                // format, not a character: Word writes `w:tab` and reads a
                // literal tab in `w:t` as nothing at all. So the text goes in
                // as pieces with tabs between them, and something built from
                // plain text with tabs in it comes out right.
                let local = if deleted { "delText" } else { "t" };
                let pieces: Vec<&str> = text.split('\t').collect();
                for (index, piece) in pieces.iter().enumerate() {
                    if index > 0 {
                        element.push_element(Element::new(&name_with(prefix, "tab"), Some(W)));
                    }
                    // Nothing between two tabs writes nothing — but a run whose
                    // whole text is empty keeps its `w:t`, because that is where
                    // a field puts its answer.
                    if piece.is_empty() && pieces.len() > 1 {
                        continue;
                    }
                    let mut node = Element::new(&name_with(prefix, local), Some(W));
                    node.set_text(piece);
                    // Always written: a run's text is content, and the cost of
                    // the attribute is far smaller than the cost of losing a
                    // space.
                    node.set_namespaced_attribute("xml:space", XML_NAMESPACE, "preserve");
                    element.push_element(node);
                }
            }
            RunContent::Break(kind) => {
                let mut node = Element::new(&name_with(prefix, "br"), Some(W));
                match kind {
                    BreakKind::Line => {}
                    BreakKind::Page => {
                        node.set_namespaced_attribute(&name_with(prefix, "type"), W, "page");
                    }
                    BreakKind::Column => {
                        node.set_namespaced_attribute(&name_with(prefix, "type"), W, "column");
                    }
                }
                element.push_element(node);
            }
            RunContent::Tab => {
                element.push_element(Element::new(&name_with(prefix, "tab"), Some(W)));
            }
            RunContent::PositionTab(alignment) => {
                let mut tab = Element::new(&name_with(prefix, "ptab"), Some(W));
                tab.set_namespaced_attribute(
                    &name_with(prefix, "alignment"),
                    W,
                    match alignment {
                        TabAlignment::Center => "center",
                        TabAlignment::End => "right",
                        _ => "left",
                    },
                );
                // Measured from the margins, which is what makes it keep its
                // place when the indents change.
                tab.set_namespaced_attribute(&name_with(prefix, "relativeTo"), W, "margin");
                tab.set_namespaced_attribute(&name_with(prefix, "leader"), W, "none");
                element.push_element(tab);
            }
            // Building a drawing means writing four namespaces of DrawingML
            // and adding a part and a relationship for the picture itself.
            // Nothing here creates a picture yet, and one read from a document
            // is carried through in its own element rather than rebuilt — so
            // there is nothing to write, and pretending otherwise would lose
            // the picture.
            RunContent::Picture(_) => {}
            // Nor a chart: it is a part of the package, carried through in
            // its own element rather than rebuilt from the model.
            RunContent::Chart(_) => {}
            // An equation is not written from inside a run: it is a sibling
            // of the runs, and `paragraph_element` writes it there.
            RunContent::Math(_) => {}
            // A shape, unlike a picture, needs no part and no relationship —
            // it is described entirely by its own element — so it can be
            // written out from the model.
            RunContent::Shape(shape) => {
                element.push_element(crate::shapes::shape_element(shape, prefix));
            }
            RunContent::NoteReference { id, endnote } => {
                let local = if *endnote { "endnoteReference" } else { "footnoteReference" };
                let mut node = Element::new(&name_with(prefix, local), Some(W));
                node.set_namespaced_attribute(&name_with(prefix, "id"), W, &id.to_string());
                element.push_element(node);
            }
        }
    }

    element
}

/// Turns table borders into a `w:tblBorders`, in the order the schema wants.
/// Turns paragraph borders into a `w:pBdr`.
pub(crate) fn paragraph_borders_element(
    borders: &ParagraphBorders,
    prefix: Option<&str>,
) -> Element {
    let mut element = Element::new(&name_with(prefix, "pBdr"), Some(W));
    for (name, border) in [
        ("top", &borders.top),
        ("start", &borders.start),
        ("bottom", &borders.bottom),
        ("end", &borders.end),
        ("between", &borders.between),
    ] {
        let Some(border) = border else { continue };
        element.push_element(border_element(name, border, prefix));
    }
    element
}

pub(crate) fn table_borders_element(borders: &TableBorders, prefix: Option<&str>) -> Element {
    let mut element = Element::new(&name_with(prefix, "tblBorders"), Some(W));

    for (name, border) in [
        ("top", &borders.top),
        ("start", &borders.start),
        ("bottom", &borders.bottom),
        ("end", &borders.end),
        ("insideH", &borders.inside_horizontal),
        ("insideV", &borders.inside_vertical),
    ] {
        let Some(border) = border else { continue };
        element.push_element(border_element(name, border, prefix));
    }

    element
}

/// One edge of a border, wherever it appears.
pub(crate) fn border_element(name: &str, border: &Border, prefix: Option<&str>) -> Element {
    let mut side = Element::new(&name_with(prefix, name), Some(W));
    side.set_namespaced_attribute(&name_with(prefix, "val"), W, &border.style);
    side.set_namespaced_attribute(&name_with(prefix, "sz"), W, &border.size.to_string());
    side.set_namespaced_attribute(&name_with(prefix, "space"), W, "1");
    side.set_namespaced_attribute(
        &name_with(prefix, "color"),
        W,
        border.color.as_deref().unwrap_or("auto"),
    );
    // Written only when they are on. Word leaves them out otherwise, and a
    // document full of `w:shadow="0"` is a document that says nothing twice.
    if border.shadow {
        side.set_namespaced_attribute(&name_with(prefix, "shadow"), W, "1");
    }
    if border.frame {
        side.set_namespaced_attribute(&name_with(prefix, "frame"), W, "1");
    }
    side
}

/// The namespaces a drawing is written in.
///
/// Four of them, each for a different layer of the same picture: where it sits
/// in the text, what shape it is, what it is filled with, and which part of the
/// package holds the bytes. A drawing that declares one of them wrongly is a
/// drawing Word refuses to open.
pub(crate) const DRAWING_WORDPROCESSING: &str =
    "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
pub(crate) const DRAWING_MAIN: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const DRAWING_PICTURE: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
pub(crate) const RELATIONSHIPS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// Builds the `w:drawing` that puts a picture in the line of text.
///
/// Written out in full because every element of it is required: the extent
/// twice over, the shape properties, the fill, and the identifiers Word uses to
/// name the picture in its own interface.
#[must_use]
pub fn drawing_element(
    relationship: &str,
    width_emu: i64,
    height_emu: i64,
    prefix: Option<&str>,
) -> Element {
    let mut drawing = Element::new(&name_with(prefix, "drawing"), Some(W));

    let mut inline = Element::new("wp:inline", Some(DRAWING_WORDPROCESSING));
    inline.declarations.push((Some("wp".to_owned()), DRAWING_WORDPROCESSING.to_owned()));
    for side in ["distT", "distB", "distL", "distR"] {
        inline.set_attribute(side, "0");
    }

    let mut extent = Element::new("wp:extent", Some(DRAWING_WORDPROCESSING));
    extent.set_attribute("cx", &width_emu.to_string());
    extent.set_attribute("cy", &height_emu.to_string());
    inline.push_element(extent);

    let mut properties = Element::new("wp:docPr", Some(DRAWING_WORDPROCESSING));
    properties.set_attribute("id", "1");
    properties.set_attribute("name", "Picture 1");
    inline.push_element(properties);

    let mut graphic = Element::new("a:graphic", Some(DRAWING_MAIN));
    graphic.declarations.push((Some("a".to_owned()), DRAWING_MAIN.to_owned()));

    let mut data = Element::new("a:graphicData", Some(DRAWING_MAIN));
    data.set_attribute("uri", DRAWING_PICTURE);

    let mut picture = Element::new("pic:pic", Some(DRAWING_PICTURE));
    picture.declarations.push((Some("pic".to_owned()), DRAWING_PICTURE.to_owned()));

    let mut non_visual = Element::new("pic:nvPicPr", Some(DRAWING_PICTURE));
    let mut non_visual_properties = Element::new("pic:cNvPr", Some(DRAWING_PICTURE));
    non_visual_properties.set_attribute("id", "0");
    non_visual_properties.set_attribute("name", "Picture 1");
    non_visual.push_element(non_visual_properties);
    non_visual.push_element(Element::new("pic:cNvPicPr", Some(DRAWING_PICTURE)));
    picture.push_element(non_visual);

    let mut fill = Element::new("pic:blipFill", Some(DRAWING_PICTURE));
    let mut blip = Element::new("a:blip", Some(DRAWING_MAIN));
    blip.set_namespaced_attribute("r:embed", RELATIONSHIPS, relationship);
    blip.declarations.push((Some("r".to_owned()), RELATIONSHIPS.to_owned()));
    fill.push_element(blip);
    let mut stretch = Element::new("a:stretch", Some(DRAWING_MAIN));
    stretch.push_element(Element::new("a:fillRect", Some(DRAWING_MAIN)));
    fill.push_element(stretch);
    picture.push_element(fill);

    let mut shape = Element::new("pic:spPr", Some(DRAWING_PICTURE));
    let mut transform = Element::new("a:xfrm", Some(DRAWING_MAIN));
    let mut offset = Element::new("a:off", Some(DRAWING_MAIN));
    offset.set_attribute("x", "0");
    offset.set_attribute("y", "0");
    transform.push_element(offset);
    let mut size = Element::new("a:ext", Some(DRAWING_MAIN));
    size.set_attribute("cx", &width_emu.to_string());
    size.set_attribute("cy", &height_emu.to_string());
    transform.push_element(size);
    shape.push_element(transform);

    let mut geometry = Element::new("a:prstGeom", Some(DRAWING_MAIN));
    geometry.set_attribute("prst", "rect");
    geometry.push_element(Element::new("a:avLst", Some(DRAWING_MAIN)));
    shape.push_element(geometry);
    picture.push_element(shape);

    data.push_element(picture);
    graphic.push_element(data);
    inline.push_element(graphic);
    drawing.push_element(inline);
    drawing
}

/// Turns a table from the model into an element ready to insert.
#[must_use]
pub fn table_element(table: &Table, prefix: Option<&str>) -> Element {
    let mut element = Element::new(&name_with(prefix, "tbl"), Some(W));

    let mut properties = Element::new(&name_with(prefix, "tblPr"), Some(W));
    if let Some(style) = &table.style {
        properties.push_element(valued(prefix, "tblStyle", style));
    }
    let mut width = Element::new(&name_with(prefix, "tblW"), Some(W));
    width.set_namespaced_attribute(&name_with(prefix, "w"), W, "0");
    width.set_namespaced_attribute(&name_with(prefix, "type"), W, "auto");
    properties.push_element(width);
    if !table.borders.is_empty() {
        properties.push_element(table_borders_element(&table.borders, prefix));
    }
    element.push_element(properties);

    // The grid decides the geometry, so it is written even when every column is
    // the same width: a table without one is a table whose columns are guesses.
    if !table.grid.is_empty() {
        let mut grid = Element::new(&name_with(prefix, "tblGrid"), Some(W));
        for column in &table.grid {
            let mut entry = Element::new(&name_with(prefix, "gridCol"), Some(W));
            entry.set_namespaced_attribute(&name_with(prefix, "w"), W, &column.to_string());
            grid.push_element(entry);
        }
        element.push_element(grid);
    }

    for row in &table.rows {
        let mut row_element = Element::new(&name_with(prefix, "tr"), Some(W));
        for cell in &row.cells {
            let mut cell_element = Element::new(&name_with(prefix, "tc"), Some(W));

            let mut cell_properties = Element::new(&name_with(prefix, "tcPr"), Some(W));
            let mut cell_width = Element::new(&name_with(prefix, "tcW"), Some(W));
            match cell.width {
                Some(twips) => {
                    cell_width.set_namespaced_attribute(
                        &name_with(prefix, "w"),
                        W,
                        &twips.to_string(),
                    );
                    cell_width.set_namespaced_attribute(&name_with(prefix, "type"), W, "dxa");
                }
                None => {
                    cell_width.set_namespaced_attribute(&name_with(prefix, "w"), W, "0");
                    cell_width.set_namespaced_attribute(&name_with(prefix, "type"), W, "auto");
                }
            }
            cell_properties.push_element(cell_width);
            if cell.span > 1 {
                cell_properties.push_element(valued(prefix, "gridSpan", &cell.span.to_string()));
            }
            cell_element.push_element(cell_properties);

            if cell.blocks.is_empty() {
                // A cell must contain at least one paragraph; Word rejects a
                // document where one does not.
                cell_element.push_element(Element::new(&name_with(prefix, "p"), Some(W)));
            } else {
                for block in &cell.blocks {
                    cell_element.push_element(block_element(block, prefix));
                }
            }

            row_element.push_element(cell_element);
        }
        element.push_element(row_element);
    }

    element
}

pub(crate) fn block_element(block: &Block, prefix: Option<&str>) -> Element {
    match block {
        Block::Paragraph(paragraph) => paragraph_element(paragraph, prefix),
        Block::Table(table) => table_element(table, prefix),
    }
}

/// Appends a block to the end of a body, before the section properties.
///
/// `w:sectPr` must remain the last child of `w:body`; a document with anything
/// after it is rejected.
pub fn append_block(body: &mut Element, block: &Block, prefix: Option<&str>) {
    let element = block_element(block, prefix);
    match body.position_of(Some(W), "sectPr") {
        Some(index) => body.insert_element(index, element),
        None => body.push_element(element),
    }
}

/// Finds the paragraph at a given index among the body's direct children.
fn paragraph_at(body: &mut Element, index: usize) -> Option<&mut Element> {
    body.child_elements_mut().filter(|element| element.is(Some(W), "p")).nth(index)
}

/// Ensures a paragraph has a `w:pPr` and returns it.
pub(crate) fn paragraph_properties_of<'a>(
    paragraph: &'a mut Element,
    prefix: Option<&str>,
) -> &'a mut Element {
    if paragraph.child(Some(W), "pPr").is_none() {
        // Paragraph properties must come first inside the paragraph.
        paragraph.insert_element(0, Element::new(&name_with(prefix, "pPr"), Some(W)));
    }
    paragraph.child_mut(Some(W), "pPr").expect("just ensured")
}

/// Sets the style of the paragraph at a given index in the body.
///
/// Returns whether a paragraph was found at that index.
pub fn set_paragraph_style(
    body: &mut Element,
    index: usize,
    style: Option<&str>,
    prefix: Option<&str>,
) -> bool {
    let Some(paragraph) = paragraph_at(body, index) else {
        return false;
    };
    let properties = paragraph_properties_of(paragraph, prefix);

    properties.remove_children_named(Some(W), "pStyle");
    if let Some(style) = style {
        insert_ordered(properties, valued(prefix, "pStyle", style), PARAGRAPH_PROPERTY_ORDER);
    }
    true
}

/// Sets the alignment of the paragraph at a given index in the body.
pub fn set_paragraph_alignment(
    body: &mut Element,
    index: usize,
    alignment: Alignment,
    prefix: Option<&str>,
) -> bool {
    let Some(paragraph) = paragraph_at(body, index) else {
        return false;
    };
    let properties = paragraph_properties_of(paragraph, prefix);

    properties.remove_children_named(Some(W), "jc");
    insert_ordered(
        properties,
        valued(prefix, "jc", alignment.to_attribute()),
        PARAGRAPH_PROPERTY_ORDER,
    );
    true
}

/// Sets or clears one on/off character property on every run of a paragraph.
pub fn set_run_toggle(
    body: &mut Element,
    index: usize,
    local: &str,
    state: Option<bool>,
    prefix: Option<&str>,
) -> bool {
    let Some(paragraph) = paragraph_at(body, index) else {
        return false;
    };

    let mut runs: Vec<&mut Element> = Vec::new();
    collect_run_elements(paragraph, &mut runs);
    for run in runs {
        if run.child(Some(W), "rPr").is_none() {
            run.insert_element(0, Element::new(&name_with(prefix, "rPr"), Some(W)));
        }
        let properties = run.child_mut(Some(W), "rPr").expect("just ensured");
        properties.remove_children_named(Some(W), local);
        if let Some(state) = state {
            insert_ordered(properties, toggle(prefix, local, state), RUN_PROPERTY_ORDER);
        }
    }
    true
}

fn collect_run_elements<'a>(parent: &'a mut Element, out: &mut Vec<&'a mut Element>) {
    for child in parent.child_elements_mut() {
        if child.namespace.as_deref() != Some(W) {
            continue;
        }
        if child.local_name() == "r" {
            out.push(child);
        } else if child.local_name() != "del" {
            collect_run_elements(child, out);
        }
    }
}

/// Where among a paragraph's children something belonging at a text offset goes.
///
/// Used by anything that inserts a thing which is *between* runs rather than
/// inside one — a comment anchor, a tracked-change wrapper. Split the runs at
/// the offset first, or this lands on a boundary that does not exist yet.
#[must_use]
pub(crate) fn child_position_at_offset(paragraph: &Element, offset: usize) -> usize {
    let mut seen = 0usize;
    for (index, node) in paragraph.children.iter().enumerate() {
        let Node::Element(child) = node else { continue };
        if child.namespace.as_deref() != Some(W) || child.local_name() == "del" {
            continue;
        }
        if seen >= offset {
            return index;
        }
        seen += measured_length(child);
    }
    paragraph.children.len()
}

/// How many characters of a paragraph's text an element accounts for.
#[must_use]
pub(crate) fn measured_length(element: &Element) -> usize {
    if element.namespace.as_deref() == Some(W) && element.local_name() == "t" {
        return element.text_content().len();
    }
    if let Some(text) = atomic_text(element) {
        return text.len();
    }
    element
        .child_elements()
        .filter(|child| child.namespace.as_deref() == Some(W) && child.local_name() != "del")
        .map(measured_length)
        .sum()
}
