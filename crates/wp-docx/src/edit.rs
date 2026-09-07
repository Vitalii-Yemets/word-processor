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

use crate::model::{Alignment, Block, BreakKind, Paragraph, Run, RunContent, RunProperties, Table};
use crate::read::W;

/// The namespace of the reserved `xml` prefix, which `xml:space` belongs to.
const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

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
fn name_with(prefix: Option<&str>, local: &str) -> String {
    match prefix {
        Some(prefix) => format!("{prefix}:{local}"),
        None => local.to_owned(),
    }
}

// --- Finding and replacing text ---------------------------------------------

/// Where one `w:t` element sits, and what it holds.
struct TextPiece {
    /// Indices into `children` at each level, from the paragraph down.
    path: Vec<usize>,
    text: String,
    /// Byte offset of this piece within the paragraph's assembled text.
    start: usize,
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

    let prefix = prefix_for(root, W);
    let mut replaced = 0;
    replace_in_paragraphs(root, needle, replacement, prefix.as_deref(), &mut replaced);
    replaced
}

fn replace_in_paragraphs(
    element: &mut Element,
    needle: &str,
    replacement: &str,
    prefix: Option<&str>,
    replaced: &mut usize,
) {
    if element.is(Some(W), "p") {
        *replaced += replace_in_paragraph(element, needle, replacement, prefix);
        return;
    }
    for child in element.child_elements_mut() {
        replace_in_paragraphs(child, needle, replacement, prefix, replaced);
    }
}

/// Replaces inside one paragraph. Returns the number of occurrences changed.
fn replace_in_paragraph(
    paragraph: &mut Element,
    needle: &str,
    replacement: &str,
    prefix: Option<&str>,
) -> usize {
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
        let rebuilt = rebuild_piece(piece, &matches, replacement);
        if rebuilt == piece.text {
            continue;
        }
        if let Some(element) = element_at_path_mut(paragraph, &piece.path) {
            element.set_text(&rebuilt);
            preserve_space_if_needed(element, &rebuilt, prefix);
        }
    }

    matches.len()
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

        let inside_match =
            matches.iter().any(|(start, end)| absolute >= *start && absolute < *end);
        let character = piece.text[local..].chars().next().expect("on a character boundary");
        if !inside_match {
            out.push(character);
        }
        local += character.len_utf8();
    }

    out
}

/// Collects every `w:t` under a paragraph, in document order.
fn collect_text_pieces(paragraph: &Element) -> Vec<TextPiece> {
    let mut pieces = Vec::new();
    let mut path = Vec::new();
    let mut offset = 0usize;
    walk_text_pieces(paragraph, &mut path, &mut offset, &mut pieces);
    pieces
}

fn walk_text_pieces(
    element: &Element,
    path: &mut Vec<usize>,
    offset: &mut usize,
    pieces: &mut Vec<TextPiece>,
) {
    for (index, node) in element.children.iter().enumerate() {
        let Node::Element(child) = node else { continue };
        if child.namespace.as_deref() != Some(W) {
            continue;
        }

        // Deleted text is not part of what the document says, so it is not
        // searched and never rewritten.
        if child.local_name() == "del" {
            continue;
        }

        path.push(index);
        if child.local_name() == "t" {
            let text = child.text_content();
            let length = text.len();
            pieces.push(TextPiece { path: path.clone(), text, start: *offset });
            *offset += length;
        } else {
            walk_text_pieces(child, path, offset, pieces);
        }
        path.pop();
    }
}

/// Resolves a child path back to a mutable element.
fn element_at_path_mut<'a>(root: &'a mut Element, path: &[usize]) -> Option<&'a mut Element> {
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
fn preserve_space_if_needed(element: &mut Element, text: &str, prefix: Option<&str>) {
    let _ = prefix;
    let needs_it = text.starts_with(char::is_whitespace) || text.ends_with(char::is_whitespace);
    if needs_it {
        element.set_namespaced_attribute("xml:space", XML_NAMESPACE, "preserve");
    } else {
        element.remove_attribute("xml:space");
    }
}

// --- Building elements from the model ---------------------------------------

/// Turns a paragraph from the model into an element ready to insert.
#[must_use]
pub fn paragraph_element(paragraph: &Paragraph, prefix: Option<&str>) -> Element {
    let mut element = Element::new(&name_with(prefix, "p"), Some(W));

    let has_properties =
        paragraph.style.is_some() || paragraph.alignment.is_some() || paragraph.right_to_left;
    if has_properties {
        let mut properties = Element::new(&name_with(prefix, "pPr"), Some(W));
        if let Some(style) = &paragraph.style {
            properties.push_element(valued(prefix, "pStyle", style));
        }
        if paragraph.right_to_left {
            properties.push_element(Element::new(&name_with(prefix, "bidi"), Some(W)));
        }
        if let Some(alignment) = paragraph.alignment {
            properties.push_element(valued(prefix, "jc", alignment.to_attribute()));
        }
        element.push_element(properties);
    }

    for run in &paragraph.runs {
        element.push_element(run_element(run, prefix));
    }

    element
}

/// Turns a run from the model into an element.
#[must_use]
pub fn run_element(run: &Run, prefix: Option<&str>) -> Element {
    let mut element = Element::new(&name_with(prefix, "r"), Some(W));

    if !run.properties.is_default() {
        element.push_element(run_properties_element(&run.properties, prefix));
    }

    for piece in &run.content {
        match piece {
            RunContent::Text(text) => {
                let mut node = Element::new(&name_with(prefix, "t"), Some(W));
                node.set_text(text);
                // Always written: a run's text is content, and the cost of the
                // attribute is far smaller than the cost of losing a space.
                node.set_namespaced_attribute("xml:space", XML_NAMESPACE, "preserve");
                element.push_element(node);
            }
            RunContent::Break(kind) => {
                let mut node = Element::new(&name_with(prefix, "br"), Some(W));
                match kind {
                    BreakKind::Line => {}
                    BreakKind::Page => node.set_namespaced_attribute("w:type", W, "page"),
                    BreakKind::Column => node.set_namespaced_attribute("w:type", W, "column"),
                }
                element.push_element(node);
            }
            RunContent::Tab => {
                element.push_element(Element::new(&name_with(prefix, "tab"), Some(W)));
            }
        }
    }

    element
}

fn run_properties_element(properties: &RunProperties, prefix: Option<&str>) -> Element {
    let mut element = Element::new(&name_with(prefix, "rPr"), Some(W));

    // The order matters: Word rejects a document whose run properties are not
    // in the sequence the schema declares.
    if let Some(style) = &properties.style {
        element.push_element(valued(prefix, "rStyle", style));
    }
    if let Some(font) = &properties.font {
        let mut fonts = Element::new(&name_with(prefix, "rFonts"), Some(W));
        for attribute in ["ascii", "hAnsi", "cs", "eastAsia"] {
            fonts.set_namespaced_attribute(&name_with(prefix, attribute), W, font);
        }
        element.push_element(fonts);
    }
    if properties.bold {
        element.push_element(Element::new(&name_with(prefix, "b"), Some(W)));
        element.push_element(Element::new(&name_with(prefix, "bCs"), Some(W)));
    }
    if properties.italic {
        element.push_element(Element::new(&name_with(prefix, "i"), Some(W)));
        element.push_element(Element::new(&name_with(prefix, "iCs"), Some(W)));
    }
    if properties.strike {
        element.push_element(Element::new(&name_with(prefix, "strike"), Some(W)));
    }
    if let Some(color) = &properties.color {
        element.push_element(valued(prefix, "color", color));
    }
    if let Some(half_points) = properties.size_half_points {
        let size = half_points.to_string();
        element.push_element(valued(prefix, "sz", &size));
        element.push_element(valued(prefix, "szCs", &size));
    }
    if properties.underline {
        element.push_element(valued(prefix, "u", "single"));
    }
    if let Some(language) = &properties.language {
        element.push_element(valued(prefix, "lang", language));
    }
    if properties.right_to_left {
        element.push_element(Element::new(&name_with(prefix, "rtl"), Some(W)));
    }

    element
}

/// An element carrying only a `w:val` attribute, the format's commonest shape.
fn valued(prefix: Option<&str>, local: &str, value: &str) -> Element {
    let mut element = Element::new(&name_with(prefix, local), Some(W));
    element.set_namespaced_attribute(&name_with(prefix, "val"), W, value);
    element
}

/// Appends a block to the end of a body, before the section properties.
///
/// `w:sectPr` must remain the last child of `w:body`; a document with anything
/// after it is rejected.
pub fn append_block(body: &mut Element, block: &Block, prefix: Option<&str>) {
    let element = match block {
        Block::Paragraph(paragraph) => paragraph_element(paragraph, prefix),
        Block::Table(table) => table_element(table, prefix),
    };

    match body.position_of(Some(W), "sectPr") {
        Some(index) => body.insert_element(index, element),
        None => body.push_element(element),
    }
}

fn table_element(table: &Table, prefix: Option<&str>) -> Element {
    let mut element = Element::new(&name_with(prefix, "tbl"), Some(W));

    let mut properties = Element::new(&name_with(prefix, "tblPr"), Some(W));
    if let Some(style) = &table.style {
        properties.push_element(valued(prefix, "tblStyle", style));
    }
    let mut width = Element::new(&name_with(prefix, "tblW"), Some(W));
    width.set_namespaced_attribute(&name_with(prefix, "w"), W, "0");
    width.set_namespaced_attribute(&name_with(prefix, "type"), W, "auto");
    properties.push_element(width);
    element.push_element(properties);

    for row in &table.rows {
        let mut row_element = Element::new(&name_with(prefix, "tr"), Some(W));
        for cell in &row.cells {
            let mut cell_element = Element::new(&name_with(prefix, "tc"), Some(W));

            let mut cell_properties = Element::new(&name_with(prefix, "tcPr"), Some(W));
            let mut cell_width = Element::new(&name_with(prefix, "tcW"), Some(W));
            cell_width.set_namespaced_attribute(&name_with(prefix, "w"), W, "0");
            cell_width.set_namespaced_attribute(&name_with(prefix, "type"), W, "auto");
            cell_properties.push_element(cell_width);
            cell_element.push_element(cell_properties);

            if cell.blocks.is_empty() {
                // A cell must contain at least one paragraph; Word rejects a
                // document where one does not.
                cell_element.push_element(Element::new(&name_with(prefix, "p"), Some(W)));
            } else {
                for block in &cell.blocks {
                    let child = match block {
                        Block::Paragraph(paragraph) => paragraph_element(paragraph, prefix),
                        Block::Table(nested) => table_element(nested, prefix),
                    };
                    cell_element.push_element(child);
                }
            }

            row_element.push_element(cell_element);
        }
        element.push_element(row_element);
    }

    element
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
    let Some(paragraph) = body
        .child_elements_mut()
        .filter(|element| element.is(Some(W), "p"))
        .nth(index)
    else {
        return false;
    };

    let properties_name = name_with(prefix, "pPr");
    if paragraph.child(Some(W), "pPr").is_none() {
        // Paragraph properties must come first inside the paragraph.
        paragraph.insert_element(0, Element::new(&properties_name, Some(W)));
    }
    let properties = paragraph.child_mut(Some(W), "pPr").expect("just ensured");

    properties.remove_children_named(Some(W), "pStyle");
    if let Some(style) = style {
        // The style reference must be the first of the paragraph properties.
        properties.insert_element(0, valued(prefix, "pStyle", style));
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
    let Some(paragraph) = body
        .child_elements_mut()
        .filter(|element| element.is(Some(W), "p"))
        .nth(index)
    else {
        return false;
    };

    if paragraph.child(Some(W), "pPr").is_none() {
        paragraph.insert_element(0, Element::new(&name_with(prefix, "pPr"), Some(W)));
    }
    let properties = paragraph.child_mut(Some(W), "pPr").expect("just ensured");

    properties.remove_children_named(Some(W), "jc");
    properties.push_element(valued(prefix, "jc", alignment.to_attribute()));
    true
}
