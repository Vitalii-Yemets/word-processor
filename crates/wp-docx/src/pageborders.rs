//! The border round a page.
//!
//! # Why it is not a paragraph border
//!
//! Because it belongs to the paper rather than to the text. A paragraph border
//! is drawn round the band a paragraph fills and moves when the text moves; a
//! page border is drawn a fixed distance in from the edge of the sheet, is the
//! same on every page of the section, and is there whether or not there is any
//! text on the page. They are written in different places for the same reason:
//! `w:pBdr` inside a paragraph's properties, `w:pgBorders` inside the section's.
//!
//! # What the file says
//!
//! `w:pgBorders` carries the four edges as children, and three attributes of
//! its own: `w:display` says which pages of the section get one, `w:offsetFrom`
//! whether the distance is measured from the edge of the paper or from the
//! text, and `w:zOrder` whether it is drawn in front of the text (which nobody
//! wants and Word defaults away from). The distance itself is on each edge, as
//! `w:space`, in whole points.

use wp_xml::tree::Element;

use crate::model::Border;
use crate::read::{self, W};
use crate::{edit, page, Document, EditKind};

/// Which pages of a section carry the border.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Display {
    #[default]
    AllPages,
    /// Only the first page of the section, which is what a title page wants.
    FirstPage,
    /// Every page but the first, which is the other half of the same idea.
    NotFirstPage,
}

impl Display {
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::AllPages => "allPages",
            Self::FirstPage => "firstPage",
            Self::NotFirstPage => "notFirstPage",
        }
    }

    #[must_use]
    pub fn from_word(text: &str) -> Self {
        match text {
            "firstPage" => Self::FirstPage,
            "notFirstPage" => Self::NotFirstPage,
            _ => Self::AllPages,
        }
    }

    /// Whether a page of a section carries the border.
    #[must_use]
    pub fn covers(self, page_of_section: usize) -> bool {
        match self {
            Self::AllPages => true,
            Self::FirstPage => page_of_section == 0,
            Self::NotFirstPage => page_of_section > 0,
        }
    }
}

/// The border round the pages of one section.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageBorders {
    pub top: Option<Border>,
    pub start: Option<Border>,
    pub bottom: Option<Border>,
    pub end: Option<Border>,
    /// Which pages get one.
    pub display: Display,
    /// Whether the distance is measured from the edge of the paper rather than
    /// from the text. Word's own default is from the edge.
    pub from_text: bool,
    /// How far in the border sits, in whole points — the unit `w:space` uses.
    ///
    /// Word's own default is twenty-four points from the edge of the paper, and
    /// its dialog will not take more than thirty-one, because that is as far as
    /// the attribute goes.
    pub distance: u32,
}

/// The furthest in a page border may sit, which is as far as `w:space` reaches.
pub const FURTHEST: u32 = 31;

/// Where Word puts one when it is first asked for.
pub const USUAL_DISTANCE: u32 = 24;

impl PageBorders {
    /// Whether any edge is drawn at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        [&self.top, &self.start, &self.bottom, &self.end]
            .into_iter()
            .flatten()
            .all(|border| !border.is_visible())
    }

    /// A line on all four edges: what Word's "Box" setting draws.
    #[must_use]
    pub fn box_all(line: &Border) -> Self {
        Self {
            top: Some(line.clone()),
            start: Some(line.clone()),
            bottom: Some(line.clone()),
            end: Some(line.clone()),
            distance: USUAL_DISTANCE,
            ..Self::default()
        }
    }
}

impl Document {
    /// The border round the pages of one section.
    #[must_use]
    pub fn page_borders_of(&self, section: usize) -> PageBorders {
        crate::sections::properties_of(&self.tree().root, section)
            .and_then(read_page_borders)
            .unwrap_or_default()
    }

    /// The border round the pages of the caret's section.
    #[must_use]
    pub fn page_borders(&self) -> PageBorders {
        self.page_borders_of(self.section_here())
    }

    /// Puts a border round the pages of the caret's section, or takes it off.
    pub fn set_page_borders(&mut self, wanted: &PageBorders) -> bool {
        if self.page_borders() == *wanted {
            return false;
        }
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();
        let Some(section) = self.section_properties_mut(prefix.as_deref()) else {
            return false;
        };
        write_page_borders(section, wanted, prefix.as_deref());
        self.mark_modified();
        true
    }

    /// The same round every section, which is Word's "Whole document".
    pub fn set_page_borders_everywhere(&mut self, wanted: &PageBorders) -> bool {
        let sections = self.sections().len();
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let mut changed = false;
        for index in 0..sections {
            let Some(section) =
                crate::sections::properties_of_mut(&mut self.tree_mut().root, index)
            else {
                continue;
            };
            let before = section.child(Some(W), "pgBorders").cloned();
            write_page_borders(section, wanted, prefix.as_deref());
            changed |= section.child(Some(W), "pgBorders").cloned() != before;
        }
        if changed {
            self.mark_modified();
        }
        changed
    }
}

/// Reads `w:pgBorders` out of a `w:sectPr`.
fn read_page_borders(section: &Element) -> Option<PageBorders> {
    let element = section.child(Some(W), "pgBorders")?;
    let edge = |local: &str| {
        element.child(Some(W), local).map(read::read_border).filter(Border::is_visible)
    };

    // The distance is on each edge rather than on the group, so the first edge
    // that names one speaks for all of them — which is how Word writes it and
    // how its dialog reads it back.
    let distance = ["top", "left", "bottom", "right"]
        .into_iter()
        .filter_map(|local| element.child(Some(W), local))
        .find_map(|edge| edge.attribute(Some(W), "space").and_then(|text| text.parse().ok()))
        .unwrap_or(USUAL_DISTANCE);

    Some(PageBorders {
        top: edge("top"),
        start: edge("left"),
        bottom: edge("bottom"),
        end: edge("right"),
        display: element.attribute(Some(W), "display").map(Display::from_word).unwrap_or_default(),
        from_text: element.attribute(Some(W), "offsetFrom") == Some("text"),
        distance: distance.min(FURTHEST),
    })
}

/// Writes it back, taking the whole element away when nothing is drawn.
fn write_page_borders(section: &mut Element, wanted: &PageBorders, prefix: Option<&str>) {
    section.remove_children_named(Some(W), "pgBorders");
    if wanted.is_empty() {
        return;
    }

    let name = |local: &str| edit::name_with(prefix, local);
    let element = page::section_child(section, prefix, "pgBorders");
    element.set_namespaced_attribute(
        &name("offsetFrom"),
        W,
        if wanted.from_text { "text" } else { "page" },
    );
    element.set_namespaced_attribute(&name("display"), W, wanted.display.word());

    // The four edges, in the order the schema wants them.
    for (local, border) in [
        ("top", &wanted.top),
        ("left", &wanted.start),
        ("bottom", &wanted.bottom),
        ("right", &wanted.end),
    ] {
        let Some(border) = border.as_ref().filter(|border| border.is_visible()) else {
            continue;
        };
        let mut edge = Element::new(&name(local), Some(W));
        edge.set_namespaced_attribute(&name("val"), W, &border.style);
        edge.set_namespaced_attribute(&name("sz"), W, &border.size.to_string());
        edge.set_namespaced_attribute(
            &name("space"),
            W,
            &wanted.distance.min(FURTHEST).to_string(),
        );
        edge.set_namespaced_attribute(&name("color"), W, border.color.as_deref().unwrap_or("auto"));
        element.push_element(edge);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_pages_a_display_covers() {
        assert!(Display::AllPages.covers(0));
        assert!(Display::AllPages.covers(7));

        assert!(Display::FirstPage.covers(0));
        assert!(!Display::FirstPage.covers(1));

        assert!(!Display::NotFirstPage.covers(0));
        assert!(Display::NotFirstPage.covers(1));
    }

    #[test]
    fn every_display_word_writes_comes_back_as_itself() {
        for display in [Display::AllPages, Display::FirstPage, Display::NotFirstPage] {
            assert_eq!(Display::from_word(display.word()), display);
        }
        // And anything else is every page, which is what Word does with a
        // value it does not know.
        assert_eq!(Display::from_word("occasionally"), Display::AllPages);
    }

    #[test]
    fn nothing_drawn_is_empty_however_it_is_written() {
        assert!(PageBorders::default().is_empty());

        // An edge written as "none" is an edge that draws nothing, and a
        // document full of those has no border round its pages.
        let nothing = Border { style: "none".to_owned(), size: 4, color: None };
        assert!(PageBorders::box_all(&nothing).is_empty());

        let line = Border { style: "single".to_owned(), size: 4, color: None };
        assert!(!PageBorders::box_all(&line).is_empty());
    }
}
