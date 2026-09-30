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
    TabStop, Table, TableBorders, TableLook,
};
use crate::read::W;

/// The namespace of the reserved `xml` prefix, which `xml:space` belongs to.
pub(crate) const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

// --- The order of properties --------------------------------------------------
//
// Every property container is a sequence in the schema, not a bag: its
// children have one order, and a file with them in any other is one the
// schema forbids. Word happens to open such a file without a word — K4 asked
// it — but a validator flags it and another reader may hold to it. So the
// lists below are the schema's whole sequences, ECMA-376 Part 1 in its
// transitional form, and every element this program builds or changes one of
// these containers with goes in through [`insert_ordered`] with the list for
// that container: one order for each, kept here and nowhere else.
//
// An entry may name more than one element, separated by `|`: those are a
// choice the schema lets come in any order among themselves. An entry with
// `w14:` in front names one of Word 2010's elements — the text effects and
// the OpenType features of a run — in the place Word's schema gives it. And
// `*` is where any other element of another namespace goes: after the
// schema's own properties and before the record of a change to them.

/// The order the schema requires for the children of `w:pPr` — `CT_PPr`, of
/// which a style's, a list level's and a recorded change's paragraph
/// properties are each a part, in the same order.
pub(crate) const PARAGRAPH_PROPERTY_ORDER: &[&str] = &[
    "pStyle",
    "keepNext",
    "keepLines",
    "pageBreakBefore",
    "framePr",
    "widowControl",
    "numPr",
    "suppressLineNumbers",
    "pBdr",
    "shd",
    "tabs",
    "suppressAutoHyphens",
    "kinsoku",
    "wordWrap",
    "overflowPunct",
    "topLinePunct",
    "autoSpaceDE",
    "autoSpaceDN",
    "bidi",
    "adjustRightInd",
    "snapToGrid",
    "spacing",
    "ind",
    "contextualSpacing",
    "mirrorIndents",
    "suppressOverlap",
    "jc",
    "textDirection",
    "textAlignment",
    "textboxTightWrap",
    "outlineLvl",
    "divId",
    "cnfStyle",
    "rPr",
    "sectPr",
    "*",
    "pPrChange",
];

/// The same, for the children of `w:rPr` — `CT_RPr`. The four marks of a
/// tracked change at the front belong only to the paragraph mark's run
/// properties, `CT_ParaRPr`, which is the same sequence with them first.
///
/// Word 2010's own properties come after the standard's, in the order of its
/// extension of the type ([MS-DOCX]): the text effects, then the OpenType
/// features. Word writes them there — every run of the templates that come
/// with Office that has one has it after the last `w:` property.
pub(crate) const RUN_PROPERTY_ORDER: &[&str] = &[
    "ins",
    "del",
    "moveFrom",
    "moveTo",
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
    "outline",
    "shadow",
    "emboss",
    "imprint",
    "noProof",
    "snapToGrid",
    "vanish",
    "webHidden",
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
    "fitText",
    "vertAlign",
    "rtl",
    "cs",
    "em",
    "lang",
    "eastAsianLayout",
    "specVanish",
    "oMath",
    "w14:glow",
    "w14:shadow",
    "w14:reflection",
    "w14:textOutline",
    "w14:textFill",
    "w14:scene3d",
    "w14:props3d",
    "w14:ligatures",
    "w14:numForm",
    "w14:numSpacing",
    "w14:stylisticSets",
    "w14:cntxtAlts",
    "*",
    "rPrChange",
];

/// And of `w:tblPr` — `CT_TblPr`, a table style's being the same without
/// the change.
pub(crate) const TABLE_PROPERTY_ORDER: &[&str] = &[
    "tblStyle",
    "tblpPr",
    "tblOverlap",
    "bidiVisual",
    "tblStyleRowBandSize",
    "tblStyleColBandSize",
    "tblW",
    "jc",
    "tblCellSpacing",
    "tblInd",
    "tblBorders",
    "shd",
    "tblLayout",
    "tblCellMar",
    "tblLook",
    "tblCaption",
    "tblDescription",
    "*",
    "tblPrChange",
];

/// And of `w:trPr` — `CT_TrPr`.
pub(crate) const ROW_PROPERTY_ORDER: &[&str] = &[
    "cnfStyle",
    "divId",
    "gridBefore",
    "gridAfter",
    "wBefore",
    "wAfter",
    "cantSplit",
    "trHeight",
    "tblHeader",
    "tblCellSpacing",
    "jc",
    "hidden",
    "ins",
    "del",
    "*",
    "trPrChange",
];

/// And of `w:tcPr` — `CT_TcPr`.
pub(crate) const CELL_PROPERTY_ORDER: &[&str] = &[
    "cnfStyle",
    "tcW",
    "gridSpan",
    "hMerge",
    "vMerge",
    "tcBorders",
    "shd",
    "noWrap",
    "tcMar",
    "textDirection",
    "tcFitText",
    "vAlign",
    "hideMark",
    "headers",
    "cellIns|cellDel|cellMerge",
    "*",
    "tcPrChange",
];

/// And of `w:sectPr` — `CT_SectPr`. The header and footer references come
/// before everything else, in any order among themselves; a list without
/// them put a property added later in front of them.
pub(crate) const SECTION_PROPERTY_ORDER: &[&str] = &[
    "headerReference|footerReference",
    "footnotePr",
    "endnotePr",
    "type",
    "pgSz",
    "pgMar",
    "paperSrc",
    "pgBorders",
    "lnNumType",
    "pgNumType",
    "cols",
    "formProt",
    "vAlign",
    "noEndnote",
    "titlePg",
    "textDirection",
    "bidi",
    "rtlGutter",
    "docGrid",
    "printerSettings",
    "*",
    "sectPrChange",
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
    // An unknown property goes at the end, which is the least surprising place
    // for something the order list does not mention.
    let Some(rank) = rank_in(order, &child) else {
        parent.push_element(child);
        return;
    };

    // After everything that comes before it or beside it, and so in front of
    // the first thing that comes after it — or that the list does not know,
    // which is taken to come after everything it does.
    let position = parent
        .children
        .iter()
        .position(|node| {
            node.as_element().is_some_and(|existing| {
                rank_in(order, existing).is_none_or(|existing_rank| existing_rank > rank)
            })
        })
        .unwrap_or(parent.children.len());

    parent.insert_element(position, child);
}

/// Where an element stands in an order, or `None` when the order does not
/// name it.
///
/// By its name and its namespace, not its name alone: Word 2010's shadow is
/// `w14:shadow`, and taken for `w:shadow` it would be put among the schema's
/// own properties, ahead of the colour. An entry without a prefix is a name
/// in the word-processing namespace and one with `w14:` a name in Word 2010's;
/// any other element — and one of Word 2010's the list does not name — stands
/// at the order's `*`, or is unknown to an order without one.
#[must_use]
pub(crate) fn rank_in(order: &[&str], element: &Element) -> Option<usize> {
    let local = element.local_name();
    let named =
        |wanted: &dyn Fn(&str) -> bool| order.iter().position(|entry| entry.split('|').any(wanted));
    let found = match element.namespace.as_deref() {
        Some(W) => return named(&|name| name == local),
        Some(crate::effects::W14) => named(&|name| name.strip_prefix("w14:") == Some(local)),
        _ => None,
    };
    found.or_else(|| order.iter().position(|entry| *entry == "*"))
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
    // So does ink, and it is not in the word-processing namespace either: it
    // is an extension. See [`crate::ink`].
    if element.namespace.as_deref() == Some(crate::ink::W14)
        && element.local_name() == "contentPart"
    {
        return Some("\u{1}");
    }
    if element.namespace.as_deref() != Some(W) {
        return None;
    }
    match element.local_name() {
        "tab" => Some("\t"),
        "br" => Some("\n"),
        // The hyphens Word writes as elements stand for one character each,
        // and the character is the one they mean: an offset means the same
        // thing whether the document spells it as an element or as text.
        "softHyphen" => Some("\u{00AD}"),
        "noBreakHyphen" => Some("\u{2011}"),
        // A picture stands in the text as one character, so the caret can be
        // put either side of it and Backspace can reach it. U+0001 rather than
        // the object replacement character because it has to be one byte long:
        // the layout counts a picture as one byte of the paragraph as well, and
        // an offset has to mean the same thing in both.
        "drawing" => Some("\u{1}"),
        // The older wrappers — a picture written the way Word wrote one
        // before 2007, or an object embedded with its preview — stand for
        // one character too, when they hold a picture the reader takes.
        "pict" | "object" if holds_picture(element) => Some("\u{1}"),
        // The mark that points at a footnote or an endnote is one character
        // too, for the same reasons.
        "footnoteReference" | "endnoteReference" => Some("\u{2}"),
        _ => None,
    }
}

/// Whether one of the older wrappers holds a picture the reader would take:
/// an image behind `v:imagedata` or `a:blip`. One that holds a shape drawn
/// in VML holds nothing this program models, and counts for nothing.
pub(crate) fn holds_picture(element: &Element) -> bool {
    element
        .child_elements()
        .any(|child| matches!(child.local_name(), "imagedata" | "blip") || holds_picture(child))
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
        // Everything Word wrote twice — once as what it means and once as what
        // an older reader can draw — is one thing in the text, and which of
        // the two is counted has to be the one the reader read, or every
        // offset after it is out by one. See [`crate::read`].
        if child.local_name() == "AlternateContent"
            && matches!(child.namespace.as_deref(), None | Some(crate::read::MC))
        {
            path.push(index);
            walk_alternate(child, path, offset, pieces, in_field || in_complex);
            path.pop();
            continue;
        }

        // Everything but WordprocessingML is skipped, except an equation and
        // ink: each is in a namespace of its own and stands for one character
        // of the text, so both have to be counted.
        let is_math = child.namespace.as_deref() == Some(crate::math::MATH_NAMESPACE);
        let is_ink = child.namespace.as_deref() == Some(crate::ink::W14)
            && child.local_name() == "contentPart";
        if child.namespace.as_deref() != Some(W) && !is_math && !is_ink {
            continue;
        }

        // Deleted text is not part of what the document says, so it is not
        // searched and never rewritten.
        if child.local_name() == "del" {
            continue;
        }

        // A word with its reading over it is one piece of the paragraph: the
        // word is text the document says and the reading is an annotation
        // about it, so counting the reading's letters would put every offset
        // after it out by as many as the reading is long.
        //
        // And it is one piece rather than a place to walk into: inside a ruby
        // is not somewhere anything is typed. Typing at the end of one puts a
        // word after the ruby, which is what Word does — a reader who types
        // after 漢字 is writing the next word, not adding to the one with the
        // reading over it.
        if child.local_name() == "ruby" && child.namespace.as_deref() == Some(W) {
            let text = ruby_base_text(child);
            if !text.is_empty() {
                path.push(index);
                pieces.push(TextPiece {
                    path: path.clone(),
                    text: text.clone(),
                    start: *offset,
                    atomic: true,
                    in_field: in_field || in_complex,
                });
                path.pop();
                *offset += text.len();
            }
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

/// Walks whichever of the ways of writing the same thing was read.
///
/// The same rule the reader follows, asked the same way: the first choice that
/// comes to anything, and the fallback when none of them did. Two layers
/// counting the characters of a paragraph differently is a caret that lands a
/// character out from where it was put.
fn walk_alternate(
    element: &Element,
    path: &mut Vec<usize>,
    offset: &mut usize,
    pieces: &mut Vec<TextPiece>,
    in_field: bool,
) {
    let mut fallback = None;
    for (index, node) in element.children.iter().enumerate() {
        let Node::Element(child) = node else { continue };
        match child.local_name() {
            "Choice" => {
                let before = pieces.len();
                path.push(index);
                walk_text_pieces(child, path, offset, pieces, in_field);
                path.pop();
                if pieces.len() > before {
                    return;
                }
            }
            "Fallback" => fallback = Some(index),
            _ => {}
        }
    }

    let Some(index) = fallback else { return };
    let Some(Node::Element(child)) = element.children.get(index) else { return };
    path.push(index);
    walk_text_pieces(child, path, offset, pieces, in_field);
    path.pop();
}

/// The word under a reading: what the document says where a ruby stands.
fn ruby_base_text(ruby: &Element) -> String {
    let Some(base) = ruby.child(Some(W), "rubyBase") else { return String::new() };
    crate::position::paragraph_text(base)
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

/// A measurement in twentieths of a point, as the format writes one: a width
/// and the unit it is in.
pub(crate) fn measured(prefix: Option<&str>, local: &str, twips: i32) -> Element {
    let mut element = Element::new(&name_with(prefix, local), Some(W));
    element.set_namespaced_attribute(&name_with(prefix, "w"), W, &twips.to_string());
    element.set_namespaced_attribute(&name_with(prefix, "type"), W, "dxa");
    element
}

/// The four sides of a `w:tblCellMar` or a `w:tcMar`.
///
/// A side nobody stated is left out rather than written as nothing: leaving it
/// out is how a cell says "whatever the table says", and writing a zero would
/// be saying something else.
pub(crate) fn cell_margins_element(
    local: &str,
    margins: &crate::model::CellMargins,
    prefix: Option<&str>,
) -> Element {
    let mut element = Element::new(&name_with(prefix, local), Some(W));
    for (side, twips) in [
        ("top", margins.top),
        ("start", margins.start),
        ("bottom", margins.bottom),
        ("end", margins.end),
    ] {
        if let Some(twips) = twips {
            element.push_element(measured(prefix, side, twips));
        }
    }
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
    // Each property made in whatever order is easiest to read, and put where
    // the schema has it at the end: the order is the list's to know, not the
    // order of the lines below — which had `w:contextualSpacing` in front of
    // the spacing and the indents it follows.
    let mut children = Vec::new();
    if let Some(style) = &properties.style {
        children.push(valued(prefix, "pStyle", style));
    }
    if let Some(state) = properties.keep_next {
        children.push(toggle(prefix, "keepNext", state));
    }
    if let Some(state) = properties.keep_lines {
        children.push(toggle(prefix, "keepLines", state));
    }
    if let Some(state) = properties.page_break_before {
        children.push(toggle(prefix, "pageBreakBefore", state));
    }
    if let Some(state) = properties.widow_control {
        children.push(toggle(prefix, "widowControl", state));
    }
    if let Some(state) = properties.suppress_line_numbers {
        children.push(toggle(prefix, "suppressLineNumbers", state));
    }
    if let Some(state) = properties.no_hyphenation {
        children.push(toggle(prefix, "suppressAutoHyphens", state));
    }
    if let Some(state) = properties.contextual_spacing {
        children.push(toggle(prefix, "contextualSpacing", state));
    }
    if let Some(state) = properties.mirror_indents {
        children.push(toggle(prefix, "mirrorIndents", state));
    }
    if let Some(numbering) = properties.numbering {
        let mut reference = Element::new(&name_with(prefix, "numPr"), Some(W));
        reference.push_element(valued(prefix, "ilvl", &numbering.level.to_string()));
        reference.push_element(valued(prefix, "numId", &numbering.id.to_string()));
        children.push(reference);
    }
    if !properties.borders.is_empty() {
        children.push(paragraph_borders_element(&properties.borders, prefix));
    }
    if let Some(fill) = &properties.shading {
        let mut shading = Element::new(&name_with(prefix, "shd"), Some(W));
        shading.set_namespaced_attribute(&name_with(prefix, "val"), W, "clear");
        shading.set_namespaced_attribute(&name_with(prefix, "color"), W, "auto");
        shading.set_namespaced_attribute(&name_with(prefix, "fill"), W, fill);
        children.push(shading);
    }
    if !properties.tab_stops.is_empty() {
        children.push(tab_stops_element(&properties.tab_stops, prefix));
    }
    if let Some(state) = properties.right_to_left {
        children.push(toggle(prefix, "bidi", state));
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
        children.push(spacing);
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
        children.push(indent);
    }
    if let Some(alignment) = properties.alignment {
        children.push(valued(prefix, "jc", alignment.to_attribute()));
    }
    if let Some(level) = properties.outline_level {
        children.push(valued(prefix, "outlineLvl", &level.to_string()));
    }

    ordered(&name_with(prefix, "pPr"), children, PARAGRAPH_PROPERTY_ORDER)
}

/// A property container holding these children, each where the order puts
/// it, whatever order they were made in.
pub(crate) fn ordered(name: &str, children: Vec<Element>, order: &[&str]) -> Element {
    let mut element = Element::new(name, Some(W));
    for child in children {
        insert_ordered(&mut element, child, order);
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
    for child in runs_elements(&paragraph.runs, prefix) {
        element.push_element(child);
    }

    element
}

/// Turns a paragraph's runs from the model into the elements that go in it.
///
/// Runs a copied link holds go back inside the link, outermost, because a
/// link may hold a change but a change may not hold a link. See
/// [`crate::clipboard::Copied::is_link`]; a link whose relationship has not
/// been made in the part being written — a copy no paste has settled — is
/// left off, and its words written as words.
#[must_use]
pub(crate) fn runs_elements(runs: &[Run], prefix: Option<&str>) -> Vec<Element> {
    /// The link a run stands in, when it can be written here.
    fn link_of(run: &Run) -> Option<&crate::clipboard::Copied> {
        run.content.iter().find_map(|piece| match piece {
            RunContent::Copied(copied) if copied.is_link() && copied.is_settled() => {
                Some(&**copied)
            }
            _ => None,
        })
    }

    let mut out = Vec::new();
    let mut index = 0usize;
    while index < runs.len() {
        let start = index;
        let link = link_of(&runs[index]);
        index += 1;
        while index < runs.len()
            && match (link, link_of(&runs[index])) {
                (Some(one), Some(other)) => one.same_link(other),
                (None, None) => true,
                _ => false,
            }
        {
            index += 1;
        }
        let children = unlinked_elements(&runs[start..index], prefix);
        match link {
            Some(link) => {
                let mut wrapper = link.element().clone();
                for child in children {
                    wrapper.push_element(child);
                }
                out.push(wrapper);
            }
            None => out.extend(children),
        }
    }
    out
}

/// The same for runs no link holds.
///
/// Runs that belong to the same field or the same tracked change go back
/// inside one wrapper, or the field would be lost and only its cached answer
/// would remain, and a change would stop being a change. What is inside a
/// change's wrapper is written as runs, because a simple field is not
/// something the format lets a change hold; the answer stays, as text.
fn unlinked_elements(runs: &[Run], prefix: Option<&str>) -> Vec<Element> {
    let mut out = Vec::new();
    let mut index = 0usize;
    while index < runs.len() {
        let run = &runs[index];

        if let Some(revision) = &run.revision {
            let mut wrapper = Element::new(&name_with(prefix, revision.kind.element()), Some(W));
            wrapper.set_namespaced_attribute(&name_with(prefix, "id"), W, &revision.id.to_string());
            wrapper.set_namespaced_attribute(&name_with(prefix, "author"), W, &revision.author);
            wrapper.set_namespaced_attribute(&name_with(prefix, "date"), W, &revision.date);

            let deleted = revision.kind == RevisionKind::Deleted;
            while index < runs.len() && runs[index].revision.as_ref() == Some(revision) {
                for child in revised_run_elements(&runs[index], prefix, deleted) {
                    wrapper.push_element(child);
                }
                index += 1;
            }
            out.push(wrapper);
            continue;
        }

        let Some(instruction) = &run.field else {
            out.extend(run_elements(run, prefix));
            index += 1;
            continue;
        };

        let mut field = Element::new(&name_with(prefix, "fldSimple"), Some(W));
        field.set_namespaced_attribute(&name_with(prefix, "instr"), W, &format!(" {instruction} "));
        while index < runs.len()
            && runs[index].revision.is_none()
            && runs[index].field.as_deref() == Some(instruction.as_str())
        {
            for child in run_elements(&runs[index], prefix) {
                field.push_element(child);
            }
            index += 1;
        }
        out.push(field);
    }
    out
}

/// Turns run properties into a `w:rPr`.
#[must_use]
pub fn run_properties_element(properties: &RunProperties, prefix: Option<&str>) -> Element {
    // As for a paragraph: made in any order and put in the schema's. The
    // lines below had the strike in front of the capitals, the scale in
    // front of the spacing and the size behind the highlight.
    let mut children = Vec::new();
    if let Some(style) = &properties.style {
        children.push(valued(prefix, "rStyle", style));
    }
    if properties.font.is_some() || properties.font_theme.is_some() {
        let mut fonts = Element::new(&name_with(prefix, "rFonts"), Some(W));
        // All four scripts get the same family: without w:cs, right-to-left and
        // East Asian text would silently fall back to a different font.
        if let Some(font) = &properties.font {
            for attribute in ["ascii", "hAnsi", "cs", "eastAsia"] {
                fonts.set_namespaced_attribute(&name_with(prefix, attribute), W, font);
            }
        }
        // And the theme's slot beside the names, which is what a document
        // Word made says of nearly every run. The slot is what Word follows,
        // so that a change of theme changes the font; left off, the text kept
        // the typeface the theme had when it was read and stopped following.
        if let Some(slot) = properties.font_theme {
            for (attribute, name) in slot.words() {
                fonts.set_namespaced_attribute(&name_with(prefix, attribute), W, name);
            }
        }
        children.push(fonts);
    }
    if let Some(state) = properties.bold {
        children.push(toggle(prefix, "b", state));
        // Complex-script text takes its weight from w:bCs, not w:b.
        children.push(toggle(prefix, "bCs", state));
    }
    if let Some(state) = properties.italic {
        children.push(toggle(prefix, "i", state));
        children.push(toggle(prefix, "iCs", state));
    }
    if let Some(state) = properties.strike {
        children.push(toggle(prefix, "strike", state));
    }
    if let Some(state) = properties.double_strike {
        children.push(toggle(prefix, "dstrike", state));
    }
    if let Some(state) = properties.caps {
        children.push(toggle(prefix, "caps", state));
    }
    if let Some(state) = properties.small_caps {
        children.push(toggle(prefix, "smallCaps", state));
    }
    if let Some(state) = properties.hidden {
        children.push(toggle(prefix, "vanish", state));
    }
    if let Some(state) = properties.no_proof {
        children.push(toggle(prefix, "noProof", state));
    }
    if let Some(scale) = properties.scale {
        children.push(valued(prefix, "w", &scale.to_string()));
    }
    if let Some(spacing) = properties.spacing_twentieths {
        children.push(valued(prefix, "spacing", &spacing.to_string()));
    }
    if let Some(position) = properties.position_half_points {
        children.push(valued(prefix, "position", &position.to_string()));
    }
    if let Some(kerning) = properties.kerning_half_points {
        children.push(valued(prefix, "kern", &kerning.to_string()));
    }
    if properties.color.is_some() || properties.color_theme.is_some() {
        // `w:val` has to be there whatever else is: the schema requires it.
        // A colour known only by the theme's name for it says `auto`, which
        // is what the reader takes for no colour written out — Word takes
        // the name over the value either way, and writes the value only as
        // the answer it last worked out.
        let mut color = valued(prefix, "color", properties.color.as_deref().unwrap_or("auto"));
        if let Some(named) = &properties.color_theme {
            let mut set = |local: &str, value: &str| {
                color.set_namespaced_attribute(&name_with(prefix, local), W, value);
            };
            set("themeColor", named.slot.word());
            // Two hex digits each, which is how the reader reads them.
            if let Some(tint) = named.tint {
                set("themeTint", &format!("{tint:02X}"));
            }
            if let Some(shade) = named.shade {
                set("themeShade", &format!("{shade:02X}"));
            }
        }
        children.push(color);
    }
    if let Some(highlight) = &properties.highlight {
        children.push(valued(prefix, "highlight", highlight));
    }
    if let Some(alignment) = properties.vertical_align {
        children.push(valued(prefix, "vertAlign", alignment.to_attribute()));
    }
    if let Some(half_points) = properties.size_half_points {
        let size = half_points.to_string();
        children.push(valued(prefix, "sz", &size));
        children.push(valued(prefix, "szCs", &size));
    }
    if let Some(underline) = &properties.underline {
        let mut line = valued(prefix, "u", underline.to_attribute());
        // The colour of the line rides on the same element as its style: the
        // format has no `w:uColor`.
        if let Some(color) = &properties.underline_color {
            line.set_namespaced_attribute(&name_with(prefix, "color"), W, color);
        }
        children.push(line);
    }
    if let Some(state) = properties.right_to_left {
        children.push(toggle(prefix, "rtl", state));
    }
    if let Some(language) = &properties.language {
        children.push(valued(prefix, "lang", language));
    }
    if let Some(layout) = properties.east_asian_layout.filter(|layout| !layout.is_empty()) {
        children.push(layout.element(prefix));
    }
    // Word 2010's text effect, which the order puts after the standard's
    // properties and in front of the OpenType features. "No effect" is said
    // by writing nothing, as the command that takes one off says it.
    if let Some(effect) = properties
        .effect
        .as_ref()
        .and_then(|wanted| crate::effects::effect_element(wanted, crate::effects::W14_PREFIX))
    {
        children.push(effect);
    }

    let mut element = ordered(&name_with(prefix, "rPr"), children, RUN_PROPERTY_ORDER);
    // In a namespace of their own, which the order puts after the standard
    // properties; the writer of them puts them there, and in their own order.
    if let Some(wanted) = &properties.open_type {
        crate::typography::write_open_type(&mut element, wanted);
    }
    // The properties say for themselves what their Word 2010 prefix means,
    // as an equation and a content part do: a run written from the model
    // goes into whatever part is being edited — a paste, a building block,
    // a comparison — and that part's root need not have declared it. A
    // document made from a model declares it on its root as well, and marks
    // it ignorable there; saying it twice is still XML.
    crate::effects::declare_where_used(&mut element);
    element
}

/// Turns a run from the model into the elements that write it.
///
/// One `w:r`, nearly always. An equation is not something a run can hold —
/// it is a sibling of the runs, in the namespace equations live in — so a run
/// with one in it is written as the run before the equation, the equation,
/// and the run after it, each with the run's own properties.
#[must_use]
pub fn run_elements(run: &Run, prefix: Option<&str>) -> Vec<Element> {
    revised_run_elements(run, prefix, false)
}

/// The same, writing `w:delText` instead of `w:t` for deleted text.
///
/// The format insists on the different name: a reader that shows the document
/// as it would be with every change accepted skips `w:delText` and keeps `w:t`,
/// and it can only do that if the two are told apart by name.
#[must_use]
pub fn revised_run_elements(run: &Run, prefix: Option<&str>, deleted: bool) -> Vec<Element> {
    let mut out = Vec::new();
    let mut element = run_shell(run, prefix);
    let mut holds = false;
    for piece in &run.content {
        match placed(piece, prefix, deleted) {
            Placed::InRun(children) => {
                holds |= !children.is_empty();
                for child in children {
                    element.push_element(child);
                }
            }
            Placed::Beside(beside) => {
                if holds {
                    out.push(core::mem::replace(&mut element, run_shell(run, prefix)));
                    holds = false;
                }
                out.push(beside);
            }
        }
    }
    if holds || out.is_empty() {
        out.push(element);
    }
    out
}

/// A `w:r` with the run's properties and nothing in it yet.
fn run_shell(run: &Run, prefix: Option<&str>) -> Element {
    let mut element = Element::new(&name_with(prefix, "r"), Some(W));

    if !run.properties.is_empty() || run.format_change.is_some() {
        let mut properties = run_properties_element(&run.properties, prefix);
        // A tracked change to the formatting, last in the properties: who,
        // when, and what they were before.
        if let Some(change) = &run.format_change {
            let mut record = Element::new(&name_with(prefix, "rPrChange"), Some(W));
            record.set_namespaced_attribute(&name_with(prefix, "id"), W, &change.id.to_string());
            record.set_namespaced_attribute(&name_with(prefix, "author"), W, &change.author);
            if !change.date.is_empty() {
                record.set_namespaced_attribute(&name_with(prefix, "date"), W, &change.date);
            }
            record.push_element(run_properties_element(&change.before, prefix));
            insert_ordered(&mut properties, record, RUN_PROPERTY_ORDER);
            // And on the outer properties too when only the record uses the
            // prefix: rejecting the change lifts the record's up into them.
            crate::effects::declare_where_used(&mut properties);
        }
        element.push_element(properties);
    }
    element
}

/// Where one piece of a run is written.
enum Placed {
    /// Inside the run, as these elements.
    InRun(Vec<Element>),
    /// Beside it, as an equation is.
    Beside(Element),
}

/// Writes one piece of a run.
///
/// # The drawings
///
/// A drawing read from a document and edited where it stands is edited in
/// its own element and never comes through here; one that is copied comes
/// through as the element it was — see [`RunContent::Copied`] — and one
/// that reaches here as the model alone is written from what the model says.
/// That is a drawing in a part being written again from its own model, or
/// one a program built. Either way the relationship it names is one of the
/// part it is written into, which is what a model read from that part says;
/// a paste, which cannot know that of a model, refuses one — see
/// [`crate::clipboard`].
fn placed(piece: &RunContent, prefix: Option<&str>, deleted: bool) -> Placed {
    match piece {
        RunContent::Text(text) => {
            // A tab inside a run's text is an element of its own in the
            // format, not a character: Word writes `w:tab` and reads a literal
            // tab in `w:t` as nothing at all. So the text goes in as pieces
            // with tabs between them, and something built from plain text with
            // tabs in it comes out right.
            let local = if deleted { "delText" } else { "t" };
            let pieces: Vec<&str> = text.split('\t').collect();
            let mut out = Vec::new();
            for (index, piece) in pieces.iter().enumerate() {
                if index > 0 {
                    out.push(Element::new(&name_with(prefix, "tab"), Some(W)));
                }
                // Nothing between two tabs writes nothing — but a run whose
                // whole text is empty keeps its `w:t`, because that is where a
                // field puts its answer.
                if piece.is_empty() && pieces.len() > 1 {
                    continue;
                }
                let mut node = Element::new(&name_with(prefix, local), Some(W));
                node.set_text(piece);
                // Always written: a run's text is content, and the cost of the
                // attribute is far smaller than the cost of losing a space.
                node.set_namespaced_attribute("xml:space", XML_NAMESPACE, "preserve");
                out.push(node);
            }
            Placed::InRun(out)
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
            Placed::InRun(vec![node])
        }
        RunContent::Tab => Placed::InRun(vec![Element::new(&name_with(prefix, "tab"), Some(W))]),
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
            Placed::InRun(vec![tab])
        }
        RunContent::Picture(picture) => Placed::InRun(vec![picture_element(picture, prefix)]),
        RunContent::Chart(chart) => Placed::InRun(vec![crate::chart::chart_drawing(
            &chart.relationship,
            chart.width_emu,
            chart.height_emu,
            prefix,
        )]),
        RunContent::Ink(ink) => Placed::InRun(vec![crate::ink::reference_element(ink, prefix)]),
        RunContent::Diagram(diagram) => {
            Placed::InRun(vec![crate::diagram::reference_element(diagram, prefix)])
        }
        // A group from its model: what each member is, where it sits in the
        // group, and the group's own box. See [`crate::group::model_element`]
        // for what that leaves out.
        RunContent::Group(group) => Placed::InRun(vec![crate::group::model_element(group, prefix)]),
        // Written back exactly as it was read, because nothing here knows
        // what it is. See [`crate::model::RunContent::Carried`].
        RunContent::Carried(element_of) => Placed::InRun(vec![(**element_of).clone()]),
        // An equation stands beside the runs, and says for itself what its
        // prefix means: a part written from a model need not have declared
        // it, and without the declaration it is not XML.
        RunContent::Math(math) => {
            let mut element = crate::math::math_element(math, crate::math::MATH_PREFIX);
            element.declarations.push((
                Some(crate::math::MATH_PREFIX.to_owned()),
                crate::math::MATH_NAMESPACE.to_owned(),
            ));
            Placed::Beside(element)
        }
        // A shape, unlike a picture, needs no part and no relationship —
        // it is described entirely by its own element — so it can be written
        // out from the model.
        RunContent::Shape(shape) => {
            Placed::InRun(vec![crate::shapes::shape_element(shape, prefix)])
        }
        // A ruby is written out from the model: it is two lists of runs and
        // nothing else — no part, no relationship — so nothing is lost by
        // rebuilding it.
        RunContent::Ruby(ruby) => Placed::InRun(vec![crate::ruby::ruby_element(ruby, prefix)]),
        RunContent::NoteReference { id, endnote } => {
            let local = if *endnote { "endnoteReference" } else { "footnoteReference" };
            let mut node = Element::new(&name_with(prefix, local), Some(W));
            node.set_namespaced_attribute(&name_with(prefix, "id"), W, &id.to_string());
            Placed::InRun(vec![node])
        }
        // A copy is its own element, once a paste has made it this part's;
        // until then it names the relationships of the part it was copied
        // from, and written here it would point at whatever this part's
        // relationships of the same names are. Only a paste writes one of
        // those — see [`crate::clipboard::Copied::is_settled`].
        // A link's mark is not in the run: the run is in the link, which
        // [`runs_elements`] puts round it.
        RunContent::Copied(copied) => {
            if copied.is_link() || !copied.is_settled() {
                Placed::InRun(Vec::new())
            } else if copied.is_equation() {
                Placed::Beside(copied.element().clone())
            } else {
                Placed::InRun(vec![copied.element().clone()])
            }
        }
    }
}

/// A picture's drawing, built from what the model says of it: how big it is,
/// where it floats, how it is turned, what it shows and where a press on it
/// goes.
///
/// What the model does not say — a crop, an effect, a recolouring — is not
/// written, which is why a picture is carried in its own element rather than
/// rebuilt wherever that can be done.
fn picture_element(picture: &crate::model::Picture, prefix: Option<&str>) -> Element {
    let mut drawing = drawing_element(
        &picture.relationship,
        picture.width_emu.max(1),
        picture.height_emu.max(1),
        prefix,
    );
    if let Some(properties) = find_named_mut(&mut drawing, "docPr") {
        if let Some(description) = picture.description.as_deref().filter(|said| !said.is_empty()) {
            properties.set_attribute("descr", description);
        }
        if let Some(link) = &picture.link {
            let mut click = Element::new("a:hlinkClick", Some(DRAWING_MAIN));
            click.declarations.push((Some("a".to_owned()), DRAWING_MAIN.to_owned()));
            click.declarations.push((Some("r".to_owned()), RELATIONSHIPS.to_owned()));
            click.set_namespaced_attribute("r:id", RELATIONSHIPS, link);
            properties.push_element(click);
        }
    }
    if picture.turned != crate::floating::Turned::default() {
        crate::floating::turn(&mut drawing, picture.turned);
    }
    if let Some(anchor) = &picture.anchor {
        crate::floating::set_anchor_on(&mut drawing, Some(anchor), prefix);
    }
    drawing
}

/// The first element of a local name at or under a root.
pub(crate) fn find_named_mut<'a>(root: &'a mut Element, local: &str) -> Option<&'a mut Element> {
    if root.local_name() == local {
        return Some(root);
    }
    root.child_elements_mut().find_map(|child| find_named_mut(child, local))
}

/// Turns paragraph borders into a `w:pBdr`.
///
/// The sides are written `w:left` and `w:right`, which is what Word writes
/// and what the transitional schema — the one every `.docx` is in — names
/// them. `w:start` and `w:end` are the strict schema's names: this program
/// reads both, but LibreOffice drops a paragraph's side borders written that
/// way.
pub(crate) fn paragraph_borders_element(
    borders: &ParagraphBorders,
    prefix: Option<&str>,
) -> Element {
    let mut element = Element::new(&name_with(prefix, "pBdr"), Some(W));
    for (name, border) in [
        ("top", &borders.top),
        ("left", &borders.start),
        ("bottom", &borders.bottom),
        ("right", &borders.end),
        ("between", &borders.between),
    ] {
        let Some(border) = border else { continue };
        element.push_element(border_element(name, border, prefix));
    }
    element
}

/// Turns table borders into a `w:tblBorders`, in the order the schema wants.
pub(crate) fn table_borders_element(borders: &TableBorders, prefix: Option<&str>) -> Element {
    borders_element("tblBorders", borders, prefix)
}

/// The same under another name: a cell's `w:tcBorders` has the same edges.
pub(crate) fn borders_element(
    local: &str,
    borders: &TableBorders,
    prefix: Option<&str>,
) -> Element {
    let mut element = Element::new(&name_with(prefix, local), Some(W));

    // The transitional names, as for a paragraph's.
    for (name, border) in [
        ("top", &borders.top),
        ("left", &borders.start),
        ("bottom", &borders.bottom),
        ("right", &borders.end),
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

/// Declares an extension namespace on the root, and says it may be ignored.
///
/// # Why both halves
///
/// Because an extension is only safe if a reader that does not know it can skip
/// it. Without the declaration the prefix means nothing and the file is not
/// XML; without `mc:Ignorable` a strict reader stops at the element it does not
/// know instead of passing over it, and the document fails to open in exactly
/// the reader the extension was supposed to be safe in.
///
/// Word writes both on `w:document` for every extension it uses, and so does
/// this. See [`crate::anchor::WP14`] and [`crate::effects::W14`].
pub(crate) fn declare_extension(root: &mut Element, prefix: &str, uri: &str) {
    /// The namespace the "which of these may be ignored" attribute lives in.
    const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";

    if !root.declarations.iter().any(|(_, found)| found == uri) {
        root.declarations.push((Some(prefix.to_owned()), uri.to_owned()));
    }
    if !root.declarations.iter().any(|(_, found)| found == MC) {
        root.declarations.push((Some("mc".to_owned()), MC.to_owned()));
    }

    let already = root.attribute(Some(MC), "Ignorable").unwrap_or_default().to_owned();
    if already.split_whitespace().any(|name| name == prefix) {
        return;
    }
    let listed = if already.is_empty() { prefix.to_owned() } else { format!("{already} {prefix}") };
    root.set_namespaced_attribute("mc:Ignorable", MC, &listed);
}

/// Turns a table from the model into an element ready to insert.
#[must_use]
pub fn table_element(table: &Table, prefix: Option<&str>) -> Element {
    let mut element = Element::new(&name_with(prefix, "tbl"), Some(W));

    // Each container's properties are put where the schema has them, as a
    // paragraph's are: the lines below had the table's borders and its layout
    // in front of the spacing between its cells.
    let mut properties = Vec::new();
    if let Some(style) = &table.style {
        properties.push(valued(prefix, "tblStyle", style));
    }
    // How wide the table would like to be, which is half of Word's AutoFit; the
    // other half is `w:tblLayout` below. See [`crate::model::TableFit`].
    let mut width = Element::new(&name_with(prefix, "tblW"), Some(W));
    match table.fit {
        crate::model::TableFit::Window(percent) => {
            let fiftieths = percent.clamp(1, 100) * 50;
            width.set_namespaced_attribute(&name_with(prefix, "w"), W, &fiftieths.to_string());
            width.set_namespaced_attribute(&name_with(prefix, "type"), W, "pct");
        }
        _ => {
            width.set_namespaced_attribute(&name_with(prefix, "w"), W, "0");
            width.set_namespaced_attribute(&name_with(prefix, "type"), W, "auto");
        }
    }
    properties.push(width);
    if !table.borders.is_empty() {
        properties.push(table_borders_element(&table.borders, prefix));
    }
    if table.fit == crate::model::TableFit::Fixed {
        let mut layout = Element::new(&name_with(prefix, "tblLayout"), Some(W));
        layout.set_namespaced_attribute(&name_with(prefix, "type"), W, "fixed");
        properties.push(layout);
    }
    // The room inside every cell, and the room between them. Written only when
    // the table asks for something other than what Word does by itself.
    if let Some(spacing) = table.cell_spacing.filter(|twips| *twips > 0) {
        properties.push(measured(prefix, "tblCellSpacing", spacing));
    }
    if !table.cell_margins.is_empty() {
        properties.push(cell_margins_element("tblCellMar", &table.cell_margins, prefix));
    }
    // How far the table is set in from the margin, which may be out into it:
    // Word's own tables sit a cell's margin to the left, so that the text in
    // the first column lines up with the text above. Written only when it is
    // set in or out, because nothing is what a table that says nothing has.
    if table.indent != 0 {
        properties.push(measured(prefix, "tblInd", table.indent));
    }
    // Which parts of the table its style may treat specially. Always
    // written, as Word writes it on every table it makes: the model has no
    // "unsaid", and a file that left it out would leave the answer to each
    // reader's own idea of what a table with no look has.
    properties.push(table_look_element(table.look, prefix));
    element.push_element(ordered(&name_with(prefix, "tblPr"), properties, TABLE_PROPERTY_ORDER));

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

    // Which column of the grid each cell begins in, row by row: a cell
    // merged with the ones below it says so only by the cell under it being
    // a continuation, and that cell is found by its column.
    let columns: Vec<Vec<u32>> = table
        .rows
        .iter()
        .map(|row| {
            let mut at = 0;
            row.cells
                .iter()
                .map(|cell| {
                    let column = at;
                    at += cell.span.max(1);
                    column
                })
                .collect()
        })
        .collect();

    for (row_index, row) in table.rows.iter().enumerate() {
        let mut row_element = Element::new(&name_with(prefix, "tr"), Some(W));

        // What the row itself says: how tall it is, and whether it is repeated
        // at the top of every page. Both were dropped here, so a table written
        // from the model came out with rows of whatever height their contents
        // happened to be and a header row that was a header no longer — which
        // is what a spreadsheet pasted in, a list converted to a table and a
        // new table all went through.
        let mut row_properties = Vec::new();
        if let Some(twips) = row.height {
            let mut height = Element::new(&name_with(prefix, "trHeight"), Some(W));
            height.set_namespaced_attribute(&name_with(prefix, "val"), W, &twips.to_string());
            height.set_namespaced_attribute(
                &name_with(prefix, "hRule"),
                W,
                if row.height_exact { "exact" } else { "atLeast" },
            );
            row_properties.push(height);
        }
        if row.is_header {
            row_properties.push(valued(prefix, "tblHeader", "true"));
        }
        if !row_properties.is_empty() {
            row_element.push_element(ordered(
                &name_with(prefix, "trPr"),
                row_properties,
                ROW_PROPERTY_ORDER,
            ));
        }

        for (cell_index, cell) in row.cells.iter().enumerate() {
            let mut cell_element = Element::new(&name_with(prefix, "tc"), Some(W));

            let mut cell_properties = Vec::new();
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
            cell_properties.push(cell_width);
            if cell.span > 1 {
                cell_properties.push(valued(prefix, "gridSpan", &cell.span.to_string()));
            }
            // A cell merged with the one above it, and the first of such a
            // run, which the format marks as where the merge restarts.
            let column = columns[row_index][cell_index];
            let continued_below = table.rows.get(row_index + 1).is_some_and(|below| {
                below
                    .cells
                    .iter()
                    .zip(&columns[row_index + 1])
                    .any(|(under, at)| *at == column && under.merged_upwards)
            });
            if cell.merged_upwards {
                cell_properties.push(Element::new(&name_with(prefix, "vMerge"), Some(W)));
            } else if continued_below {
                cell_properties.push(valued(prefix, "vMerge", "restart"));
            }
            // Its own lines and its own colour, over whatever the table says.
            if !cell.borders.is_empty() {
                cell_properties.push(borders_element("tcBorders", &cell.borders, prefix));
            }
            if let Some(fill) = &cell.shading {
                let mut shading = Element::new(&name_with(prefix, "shd"), Some(W));
                shading.set_namespaced_attribute(&name_with(prefix, "val"), W, "clear");
                shading.set_namespaced_attribute(&name_with(prefix, "color"), W, "auto");
                shading.set_namespaced_attribute(&name_with(prefix, "fill"), W, fill);
                cell_properties.push(shading);
            }
            // Room this cell keeps clear inside itself, where it asks for
            // something other than the table's.
            if !cell.margins.is_empty() {
                cell_properties.push(cell_margins_element("tcMar", &cell.margins, prefix));
            }
            // Written only when the text is turned, because the ordinary way up
            // is what a cell that says nothing means.
            if cell.direction.is_turned() {
                cell_properties.push(valued(prefix, "textDirection", cell.direction.word()));
            }
            // Written only when the text does not sit where a cell that says
            // nothing puts it, which is at the top. After the direction,
            // which is where the schema has it.
            if cell.vertical != crate::table_properties::CellAlignment::Top {
                cell_properties.push(valued(prefix, "vAlign", cell.vertical.word()));
            }
            cell_element.push_element(ordered(
                &name_with(prefix, "tcPr"),
                cell_properties,
                CELL_PROPERTY_ORDER,
            ));

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

/// A `w:tblLook`: which parts of a table its style may treat specially.
///
/// The number as well as the attributes, because a reader that knows only
/// the old form must see the same table as one that knows the new. Word 2007
/// wrote the number alone and every Word since writes both, for the same
/// reason. See [`TableLook`] on why two of the six are written the other way
/// up.
pub(crate) fn table_look_element(look: TableLook, prefix: Option<&str>) -> Element {
    let mut element = Element::new(&name_with(prefix, "tblLook"), Some(W));

    let switches = [
        ("firstRow", 0x0020, look.first_row),
        ("lastRow", 0x0040, look.last_row),
        ("firstColumn", 0x0080, look.first_column),
        ("lastColumn", 0x0100, look.last_column),
        ("noHBand", 0x0200, !look.banded_rows),
        ("noVBand", 0x0400, !look.banded_columns),
    ];
    let bits =
        switches.iter().filter(|(_, _, on)| *on).fold(0u32, |bits, (_, mask, _)| bits | mask);
    element.set_namespaced_attribute(&name_with(prefix, "val"), W, &format!("{bits:04X}"));
    for (local, _, on) in switches {
        element.set_namespaced_attribute(&name_with(prefix, local), W, if on { "1" } else { "0" });
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
    append_element(body, block_element(block, prefix));
}

/// The same for a block already written.
pub(crate) fn append_element(body: &mut Element, element: Element) {
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
///
/// Measured by the walk that makes the paragraph's text, so that the two can
/// never disagree: a shape Word wrote twice over is one character here as it
/// is to the caret, and a word under a reading is as long as the word.
#[must_use]
pub(crate) fn measured_length(element: &Element) -> usize {
    if element.namespace.as_deref() == Some(W) && element.local_name() == "t" {
        return element.text_content().len();
    }
    if let Some(text) = atomic_text(element) {
        return text.len();
    }
    if element.namespace.as_deref() == Some(W) && element.local_name() == "del" {
        return 0;
    }
    if element.namespace.as_deref() == Some(W) && element.local_name() == "ruby" {
        return ruby_base_text(element).len();
    }
    let mut pieces = Vec::new();
    if element.local_name() == "AlternateContent"
        && matches!(element.namespace.as_deref(), None | Some(crate::read::MC))
    {
        walk_alternate(element, &mut Vec::new(), &mut 0, &mut pieces, false);
    } else if element.namespace.as_deref() == Some(W) {
        pieces = collect_text_pieces(element);
    }
    pieces.iter().map(|piece| piece.text.len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ChartReference, DiagramReference, InkReference, Picture};

    /// A paragraph holding the elements a run was written as, read back.
    fn read_back(run: &Run) -> Vec<RunContent> {
        let mut paragraph = Element::new("w:p", Some(W));
        paragraph.declarations.push((Some("w".to_owned()), W.to_owned()));
        for element in run_elements(run, Some("w")) {
            paragraph.push_element(element);
        }
        crate::read::read_paragraph_for_display(&paragraph)
            .runs
            .into_iter()
            .flat_map(|run| run.content)
            .collect()
    }

    fn holding(piece: RunContent) -> Run {
        Run { content: vec![piece], ..Run::default() }
    }

    #[test]
    fn a_picture_is_written_from_its_model_and_reads_back_as_itself() {
        let picture = Picture {
            relationship: "rId5".to_owned(),
            width_emu: 100,
            height_emu: 200,
            description: Some("A view".to_owned()),
            ..Picture::default()
        };
        let back = read_back(&holding(RunContent::Picture(Box::new(picture.clone()))));
        let [RunContent::Picture(read)] = back.as_slice() else { panic!("{back:?}") };
        assert_eq!(read.relationship, "rId5");
        assert_eq!((read.width_emu, read.height_emu), (100, 200));
        assert_eq!(read.description.as_deref(), Some("A view"));
    }

    #[test]
    fn a_chart_is_written_from_its_model_and_reads_back_as_itself() {
        let chart =
            ChartReference { relationship: "rId6".to_owned(), width_emu: 300, height_emu: 400 };
        let back = read_back(&holding(RunContent::Chart(chart.clone())));
        assert_eq!(back, vec![RunContent::Chart(chart)]);
    }

    #[test]
    fn ink_is_written_from_its_model_and_reads_back_as_itself() {
        let ink = InkReference {
            relationship: "rId7".to_owned(),
            name: "Ink 1".to_owned(),
            width_emu: 500,
            height_emu: 600,
            ..InkReference::default()
        };
        let back = read_back(&holding(RunContent::Ink(ink.clone())));
        assert_eq!(back, vec![RunContent::Ink(ink)]);
    }

    #[test]
    fn a_diagram_is_written_from_its_model_and_reads_back_as_itself() {
        let diagram = DiagramReference {
            relationship: "rId8".to_owned(),
            layout: "rId9".to_owned(),
            style: "rId10".to_owned(),
            colours: "rId11".to_owned(),
            name: "Diagram 1".to_owned(),
            width_emu: 700,
            height_emu: 800,
            ..DiagramReference::default()
        };
        let back = read_back(&holding(RunContent::Diagram(diagram.clone())));
        let [RunContent::Diagram(read)] = back.as_slice() else { panic!("{back:?}") };
        assert_eq!(read.relationship, "rId8");
        assert_eq!(
            (read.layout.as_str(), read.style.as_str(), read.colours.as_str()),
            ("rId9", "rId10", "rId11")
        );
        assert_eq!((read.width_emu, read.height_emu), (700, 800));
    }

    #[test]
    fn a_group_is_written_from_its_model_and_reads_back_as_itself() {
        use crate::group::{Group, Inside, Member};
        let square = crate::shapes::Shape {
            name: "Square".to_owned(),
            width_emu: 100,
            height_emu: 100,
            ..crate::shapes::Shape::default()
        };
        let picture = Picture {
            relationship: "rId12".to_owned(),
            width_emu: 100,
            height_emu: 100,
            ..Picture::default()
        };
        let group = Group {
            name: "Group 1".to_owned(),
            width_emu: 200,
            height_emu: 100,
            members: vec![
                Member {
                    x_emu: 0,
                    y_emu: 0,
                    width_emu: 100,
                    height_emu: 100,
                    what: Inside::Shape(Box::new(square)),
                },
                Member {
                    x_emu: 100,
                    y_emu: 0,
                    width_emu: 100,
                    height_emu: 100,
                    what: Inside::Picture(Box::new(picture)),
                },
            ],
            ..Group::default()
        };
        let back = read_back(&holding(RunContent::Group(group)));
        let [RunContent::Group(read)] = back.as_slice() else { panic!("{back:?}") };
        assert_eq!((read.width_emu, read.height_emu), (200, 100));
        assert_eq!(read.members.len(), 2);
        assert!(matches!(read.members[0].what, Inside::Shape(_)));
        let Inside::Picture(inside) = &read.members[1].what else { panic!("{:?}", read.members) };
        assert_eq!(inside.relationship, "rId12");
        assert_eq!(read.fractions(&read.members[1]), (0.5, 0.0, 0.5, 1.0));
    }

    #[test]
    fn an_equation_is_written_beside_the_run_it_was_in() {
        let run = Run {
            content: vec![
                RunContent::Text("x = ".to_owned()),
                RunContent::Math(crate::math::parse("a/b")),
                RunContent::Text(" then".to_owned()),
            ],
            ..Run::default()
        };
        let elements = run_elements(&run, Some("w"));
        let names: Vec<&str> = elements.iter().map(Element::local_name).collect();
        assert_eq!(names, vec!["r", "oMath", "r"]);
        let back = read_back(&run);
        assert!(
            matches!(
                back.as_slice(),
                [RunContent::Text(_), RunContent::Math(_), RunContent::Text(_)]
            ),
            "{back:?}"
        );
    }

    #[test]
    fn a_shape_twice_over_measures_one_character_as_it_does_to_the_caret() {
        let mut alternate = Element::new("mc:AlternateContent", Some(crate::read::MC));
        let mut choice = Element::new("mc:Choice", Some(crate::read::MC));
        choice.push_element(Element::new("w:drawing", Some(W)));
        alternate.push_element(choice);
        let mut holder = Element::new("w:r", Some(W));
        holder.push_element(alternate);
        assert_eq!(measured_length(&holder), 1);
    }

    /// An element for each name of an order, the extensions in their own
    /// namespace and the choices one name each.
    fn every_child_of(order: &[&str]) -> Vec<Element> {
        order
            .iter()
            .filter(|entry| **entry != "*")
            .map(|entry| {
                let name = entry.split('|').next().expect("a name");
                match name.strip_prefix("w14:") {
                    Some(local) => Element::new(&format!("w14:{local}"), Some(crate::effects::W14)),
                    None => Element::new(&format!("w:{name}"), Some(W)),
                }
            })
            .collect()
    }

    #[test]
    fn every_child_of_every_container_goes_in_where_its_order_puts_it() {
        for order in [
            PARAGRAPH_PROPERTY_ORDER,
            RUN_PROPERTY_ORDER,
            TABLE_PROPERTY_ORDER,
            ROW_PROPERTY_ORDER,
            CELL_PROPERTY_ORDER,
            SECTION_PROPERTY_ORDER,
        ] {
            let wanted: Vec<String> =
                every_child_of(order).iter().map(|child| child.name.clone()).collect();
            // Backwards, then from the middle outwards: no order the list has.
            let mut scrambled = every_child_of(order);
            scrambled.reverse();
            let middle = scrambled.len() / 2;
            scrambled.rotate_left(middle);
            let mut container = Element::new("w:container", Some(W));
            for child in scrambled {
                insert_ordered(&mut container, child, order);
            }
            let found: Vec<String> =
                container.child_elements().map(|child| child.name.clone()).collect();
            assert_eq!(found, wanted);
        }
    }

    #[test]
    fn an_extension_goes_by_its_namespace_and_not_by_its_local_name() {
        // Word 2010's shadow and the standard's share a local name; the
        // extension's goes after the colour, the standard's before it.
        let mut properties = Element::new("w:rPr", Some(W));
        insert_ordered(&mut properties, valued(Some("w"), "color", "FF0000"), RUN_PROPERTY_ORDER);
        let extension = Element::new("w14:shadow", Some(crate::effects::W14));
        insert_ordered(&mut properties, extension, RUN_PROPERTY_ORDER);
        insert_ordered(&mut properties, Element::new("w:shadow", Some(W)), RUN_PROPERTY_ORDER);
        let found: Vec<&str> =
            properties.child_elements().map(|child| child.name.as_str()).collect();
        assert_eq!(found, ["w:shadow", "w:color", "w14:shadow"]);

        // And one the lists do not name goes where their `*` is: after the
        // standard's properties and before the record of a change.
        let change = Element::new("w:rPrChange", Some(W));
        insert_ordered(&mut properties, change, RUN_PROPERTY_ORDER);
        let unknown = Element::new("w15:something", Some("urn:elsewhere"));
        insert_ordered(&mut properties, unknown, RUN_PROPERTY_ORDER);
        let found: Vec<&str> =
            properties.child_elements().map(|child| child.name.as_str()).collect();
        assert_eq!(found, ["w:shadow", "w:color", "w14:shadow", "w15:something", "w:rPrChange"]);
    }

    #[test]
    fn a_header_and_a_footer_reference_stand_together_in_front() {
        let mut section = Element::new("w:sectPr", Some(W));
        for name in ["w:pgSz", "w:titlePg", "w:headerReference", "w:footerReference"] {
            insert_ordered(&mut section, Element::new(name, Some(W)), SECTION_PROPERTY_ORDER);
        }
        insert_ordered(
            &mut section,
            Element::new("w:headerReference", Some(W)),
            SECTION_PROPERTY_ORDER,
        );
        let found: Vec<&str> = section.child_elements().map(|child| child.name.as_str()).collect();
        assert_eq!(
            found,
            ["w:headerReference", "w:footerReference", "w:headerReference", "w:pgSz", "w:titlePg"]
        );
    }
}
