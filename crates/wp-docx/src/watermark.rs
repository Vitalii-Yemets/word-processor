//! The watermark: the word printed faintly behind the text of every page.
//!
//! # Why it lives in the header
//!
//! A watermark is not in the document. It is a drawing in the *header*, which
//! is what makes it appear on every page without being stored on every page —
//! the same trick a page number uses, for the same reason.
//!
//! # Why it is written in VML
//!
//! Word writes a watermark as a VML shape: an older drawing language kept in
//! the format for exactly this kind of thing. Not because VML is good, but
//! because a watermark written any other way is one Word does not recognise as
//! a watermark — it would show as an ordinary drawing, and Design ▸ Watermark
//! would say there is none.
//!
//! So this writes what Word writes. What this program *draws* is worked out
//! from the same shape, so the two agree.

use wp_xml::tree::{Element, Node, XmlTree};

use crate::furniture::{Furniture, Preset};
use crate::history::EditKind;
use crate::model::Alignment;
use crate::{read, Document};

/// The namespace VML shapes are written in.
const V: &str = "urn:schemas-microsoft-com:vml";
/// And the one the office-wide shape attributes are in.
const O: &str = "urn:schemas-microsoft-com:office:office";

/// The name Word gives the shape, and looks for when it asks whether a document
/// has a watermark.
const SHAPE_ID: &str = "PowerPlusWaterMarkObject";

/// The shape type a text watermark uses, which Word writes out in full.
const SHAPE_TYPE: &str = "_x0000_t136";

/// The grey Word uses when nobody picks a colour.
pub const DEFAULT_COLOR: &str = "C0C0C0";

/// A word printed behind the text of every page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Watermark {
    pub text: String,
    /// Six hex digits.
    pub color: String,
    /// Whether it runs corner to corner rather than straight across.
    pub diagonal: bool,
    pub font: String,
}

impl Default for Watermark {
    fn default() -> Self {
        Self {
            text: String::new(),
            color: DEFAULT_COLOR.to_owned(),
            diagonal: true,
            font: "Calibri".to_owned(),
        }
    }
}

impl Watermark {
    /// A diagonal grey watermark saying whatever it is given.
    #[must_use]
    pub fn saying(text: &str) -> Self {
        Self { text: text.to_owned(), ..Self::default() }
    }

    /// How far round it is turned, in degrees clockwise.
    ///
    /// Word writes 315, which is the same turn as −45 and the one that puts the
    /// text corner to corner rising to the right.
    #[must_use]
    pub fn rotation(&self) -> f32 {
        if self.diagonal {
            315.0
        } else {
            0.0
        }
    }

    /// The ready-made ones Word offers.
    #[must_use]
    pub fn presets() -> Vec<Self> {
        ["CONFIDENTIAL", "DO NOT COPY", "DRAFT", "SAMPLE", "URGENT", "ASAP"]
            .iter()
            .map(|text| Self::saying(text))
            .collect()
    }
}

impl Document {
    /// The watermark, if the document has one.
    #[must_use]
    pub fn watermark(&self) -> Option<Watermark> {
        let part = self.furniture_part(Furniture::Header)?;
        let text = self.package().xml_part(&part)?.ok()?;
        let tree = XmlTree::parse(&text).ok()?;
        read_watermark(&tree.root)
    }

    /// Puts a watermark behind every page, or takes the one there away.
    pub fn set_watermark(&mut self, wanted: Option<&Watermark>) -> bool {
        if self.watermark().as_ref() == wanted {
            return false;
        }
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        // A watermark needs somewhere to live. A document with no header gets
        // an empty one, which shows nothing and holds the shape.
        if self.furniture_part(Furniture::Header).is_none() {
            if wanted.is_none() {
                return false;
            }
            if self.set_furniture(Furniture::Header, Preset::Blank, Alignment::Start, "").is_err() {
                return false;
            }
        }

        let Some(part) = self.furniture_part(Furniture::Header) else { return false };
        let Some(Ok(text)) = self.package().xml_part(&part) else { return false };
        let Ok(mut tree) = XmlTree::parse(&text) else { return false };

        let Some(body) = find_mut(&mut tree.root, "hdr") else { return false };
        remove_watermark(body);
        if let Some(wanted) = wanted {
            body.insert_element(0, watermark_paragraph(wanted));
        }

        let Ok(xml) = tree.to_xml() else { return false };
        let content_type =
            "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";
        self.package_mut().add_part(&part, content_type, xml.into_bytes());
        self.mark_modified();
        true
    }
}

/// Reads a watermark out of a header, if one is in there.
fn read_watermark(element: &Element) -> Option<Watermark> {
    for child in element.child_elements() {
        if child.namespace.as_deref() == Some(V) && child.local_name() == "shape" {
            if let Some(found) = read_shape(child) {
                return Some(found);
            }
        }
        if let Some(found) = read_watermark(child) {
            return Some(found);
        }
    }
    None
}

/// Reads one VML shape, if it is a watermark.
fn read_shape(shape: &Element) -> Option<Watermark> {
    let path = shape.child_elements().find(|child| child.local_name() == "textpath")?;
    let text = path.attribute_by_name("string")?;
    if text.is_empty() {
        return None;
    }

    let style = shape.attribute_by_name("style").unwrap_or_default();
    let color = shape
        .attribute_by_name("fillcolor")
        .map(|value| value.trim_start_matches('#').to_uppercase())
        .unwrap_or_else(|| DEFAULT_COLOR.to_owned());

    Some(Watermark {
        text: text.to_owned(),
        color,
        // Anything turned at all is diagonal; only a rotation of nothing is
        // the straight-across one.
        diagonal: style_value(style, "rotation").is_some_and(|value| value.trim() != "0"),
        font: style_value(path.attribute_by_name("style").unwrap_or_default(), "font-family")
            .map(|value| value.trim_matches(['"', '\'']).to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "Calibri".to_owned()),
    })
}

/// One property out of a VML `style` attribute, which is CSS by another name.
fn style_value<'a>(style: &'a str, wanted: &str) -> Option<&'a str> {
    style.split(';').find_map(|piece| {
        let (name, value) = piece.split_once(':')?;
        (name.trim() == wanted).then_some(value)
    })
}

/// Takes every watermark paragraph out of a header.
fn remove_watermark(body: &mut Element) {
    body.children.retain(|node| match node {
        Node::Element(element) => !holds_watermark(element),
        _ => true,
    });
}

/// Whether an element holds a watermark shape anywhere inside it.
fn holds_watermark(element: &Element) -> bool {
    if element.namespace.as_deref() == Some(V)
        && element.local_name() == "shape"
        && element.attribute_by_name("id").is_some_and(|id| id.starts_with(SHAPE_ID))
    {
        return true;
    }
    element.child_elements().any(holds_watermark)
}

/// The paragraph a watermark is drawn by.
fn watermark_paragraph(watermark: &Watermark) -> Element {
    let mut picture = Element::new("w:pict", Some(read::W));
    picture.declarations.push((Some("v".to_owned()), V.to_owned()));
    picture.declarations.push((Some("o".to_owned()), O.to_owned()));
    picture.push_element(shape_type());
    picture.push_element(shape(watermark));

    let mut run = Element::new("w:r", Some(read::W));
    run.push_element(picture);
    let mut paragraph = Element::new("w:p", Some(read::W));
    paragraph.push_element(run);
    paragraph
}

/// The definition of the shape a text watermark is drawn with.
///
/// Word writes this out beside every watermark rather than relying on it being
/// known, and a file without it is one Word repairs.
fn shape_type() -> Element {
    let mut element = Element::new("v:shapetype", Some(V));
    element.set_attribute("id", SHAPE_TYPE);
    element.set_attribute("coordsize", "21600,21600");
    element.set_attribute("o:spt", "136");
    element.set_attribute("adj", "10800");
    element.set_attribute("path", "m@7,0l@8,0m@5,21600l@6,21600e");

    let mut path = Element::new("v:path", Some(V));
    path.set_attribute("textpathok", "t");
    element.push_element(path);
    element
}

/// The shape itself.
fn shape(watermark: &Watermark) -> Element {
    let mut element = Element::new("v:shape", Some(V));
    element.set_attribute("id", SHAPE_ID);
    element.set_attribute("type", &format!("#{SHAPE_TYPE}"));
    element.set_attribute("fillcolor", &format!("#{}", watermark.color.to_lowercase()));
    // A watermark has no outline; the letters are the shape.
    element.set_attribute("stroked", "f");
    element.set_attribute("o:allowincell", "f");
    element.set_attribute(
        "style",
        &format!(
            "position:absolute;margin-left:0;margin-top:0;width:468pt;height:117pt;\
             rotation:{};z-index:-251658752;mso-position-horizontal:center;\
             mso-position-horizontal-relative:margin;mso-position-vertical:center;\
             mso-position-vertical-relative:margin",
            watermark.rotation() as i32
        ),
    );

    let mut path = Element::new("v:textpath", Some(V));
    path.set_attribute("style", &format!("font-family:\"{}\";font-size:1pt", watermark.font));
    path.set_attribute("string", &watermark.text);
    element.push_element(path);
    element
}

/// The named element anywhere under a root, to be written into.
fn find_mut<'a>(root: &'a mut Element, local: &str) -> Option<&'a mut Element> {
    if root.local_name() == local {
        return Some(root);
    }
    root.child_elements_mut().find_map(|child| find_mut(child, local))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_watermark_is_grey_and_diagonal_unless_it_is_told_otherwise() {
        let watermark = Watermark::saying("DRAFT");
        assert_eq!(watermark.color, DEFAULT_COLOR);
        assert!(watermark.diagonal);
        assert_eq!(watermark.rotation(), 315.0);
    }

    #[test]
    fn a_flat_watermark_is_not_turned_at_all() {
        let watermark = Watermark { diagonal: false, ..Watermark::saying("DRAFT") };
        assert_eq!(watermark.rotation(), 0.0);
    }

    #[test]
    fn a_shape_survives_being_written_and_read_back() {
        let watermark = Watermark::saying("CONFIDENTIAL");
        assert_eq!(read_shape(&shape(&watermark)), Some(watermark));
    }

    #[test]
    fn a_flat_one_reads_back_flat() {
        let watermark = Watermark { diagonal: false, ..Watermark::saying("SAMPLE") };
        assert_eq!(read_shape(&shape(&watermark)), Some(watermark));
    }

    #[test]
    fn a_style_is_read_one_property_at_a_time() {
        let style = "position:absolute;rotation:315;z-index:-251658752";
        assert_eq!(style_value(style, "rotation"), Some("315"));
        assert_eq!(style_value(style, "position"), Some("absolute"));
        assert_eq!(style_value(style, "width"), None);
    }

    #[test]
    fn the_presets_are_the_ones_word_offers() {
        let presets = Watermark::presets();
        assert!(presets.iter().any(|preset| preset.text == "CONFIDENTIAL"));
        assert!(presets.iter().any(|preset| preset.text == "DRAFT"));
        assert!(presets.iter().all(|preset| preset.diagonal));
    }

    #[test]
    fn a_paragraph_that_holds_one_is_recognised() {
        let paragraph = watermark_paragraph(&Watermark::saying("DRAFT"));
        assert!(holds_watermark(&paragraph));
    }

    #[test]
    fn an_ordinary_paragraph_is_not() {
        let mut paragraph = Element::new("w:p", Some(read::W));
        paragraph.push_element(Element::new("w:r", Some(read::W)));
        assert!(!holds_watermark(&paragraph));
    }
}
