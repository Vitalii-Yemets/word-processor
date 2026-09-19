//! What a diagram's pieces are drawn in: the quick style and the colour
//! list, read.
//!
//! Every piece a layout makes carries a *style label* — `node1`,
//! `sibTrans2D1`, `fgAcc1` — and the two remaining parts say what a piece
//! with that label looks like. The colour list (`colors1.xml`) gives each
//! label the colours its fill, its line and its words are drawn in, as
//! names in the theme with shifts under them, and a *method* for handing
//! them out along a run of pieces: the same colour again and again, the
//! list cycled, or a span from the first colour to the last. The quick
//! style (`quickStyle1.xml`) says, by index into the theme's format scheme,
//! whether a piece has a fill and a line at all.
//!
//! Word's colour lists are what make one arrangement come in six colours
//! from one accent; a diagram drawn without reading them is drawn in the
//! theme's first accent whatever its colours part asks for.

use std::collections::HashMap;

use wp_xml::tree::Element;

use super::colour_of;
use crate::theme::{Slot, Theme};

/// How a list of colours is handed out along a run of pieces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Method {
    /// The first piece takes the first colour, the second the second, and
    /// round again.
    #[default]
    Repeat,
    /// Along the list and back again.
    Cycle,
    /// From the first colour to the last, in as many steps as there are
    /// pieces.
    Span,
}

impl Method {
    fn from_word(word: Option<&str>) -> Self {
        match word {
            Some("cycle") => Self::Cycle,
            Some("span") => Self::Span,
            _ => Self::Repeat,
        }
    }
}

/// What one label's pieces are drawn in.
#[derive(Clone, Debug, Default, PartialEq)]
struct Label {
    fills: Vec<String>,
    fill_method: Method,
    lines: Vec<String>,
    line_method: Method,
    texts: Vec<String>,
    text_method: Method,
    /// The quick style's word on whether the piece has a fill and a line:
    /// an index into the theme's format scheme, nought meaning none.
    fill_ref: Option<u32>,
    line_ref: Option<u32>,
}

/// The two parts read together, against the document's theme.
#[derive(Clone, Debug, Default)]
pub(crate) struct Styling {
    labels: HashMap<String, Label>,
    /// What a piece with no label of its own falls back to.
    accent: String,
    paper: String,
    ink: String,
}

impl Styling {
    /// Reads the colour list and the quick style, either of which may be
    /// missing.
    pub(crate) fn read(colours: Option<&Element>, style: Option<&Element>, theme: &Theme) -> Self {
        let mut styling = Self {
            labels: HashMap::new(),
            accent: theme.color(Slot::Accent1),
            paper: theme.color(Slot::Light1),
            ink: theme.color(Slot::Dark1),
        };
        if let Some(colours) = colours {
            for element in colours.child_elements().filter(|child| child.local_name() == "styleLbl")
            {
                let Some(name) = element.attribute(None, "name") else { continue };
                let label = styling.labels.entry(name.to_owned()).or_default();
                for list in element.child_elements() {
                    let colours: Vec<String> = list
                        .child_elements()
                        .filter(|child| matches!(child.local_name(), "srgbClr" | "schemeClr"))
                        .filter_map(|child| colour_of(child, theme))
                        .collect();
                    let method = Method::from_word(list.attribute(None, "meth"));
                    match list.local_name() {
                        "fillClrLst" => {
                            label.fills = colours;
                            label.fill_method = method;
                        }
                        "linClrLst" => {
                            label.lines = colours;
                            label.line_method = method;
                        }
                        "txFillClrLst" => {
                            label.texts = colours;
                            label.text_method = method;
                        }
                        _ => {}
                    }
                }
            }
        }
        if let Some(style) = style {
            for element in style.child_elements().filter(|child| child.local_name() == "styleLbl") {
                let Some(name) = element.attribute(None, "name") else { continue };
                let label = styling.labels.entry(name.to_owned()).or_default();
                let Some(refs) =
                    element.child_elements().find(|child| child.local_name() == "style")
                else {
                    continue;
                };
                let index = |local: &str| {
                    refs.child_elements()
                        .find(|child| child.local_name() == local)
                        .and_then(|reference| reference.attribute(None, "idx"))
                        .and_then(|value| value.parse().ok())
                };
                label.fill_ref = index("fillRef");
                label.line_ref = index("lnRef");
            }
        }
        styling
    }

    /// The colour a piece is filled with, or nothing for a piece with no
    /// fill.
    pub(crate) fn fill_of(&self, label: &str, index: usize, count: usize) -> Option<String> {
        let found = self.labels.get(label);
        if found.is_some_and(|label| label.fill_ref == Some(0)) {
            return None;
        }
        found
            .and_then(|label| pick(&label.fills, label.fill_method, index, count))
            .or_else(|| Some(self.accent.clone()))
    }

    /// The colour a piece's outline is drawn in, or nothing for none.
    pub(crate) fn line_of(&self, label: &str, index: usize, count: usize) -> Option<String> {
        let found = self.labels.get(label)?;
        if found.line_ref == Some(0) {
            return None;
        }
        pick(&found.lines, found.line_method, index, count)
    }

    /// The colour a piece's words are written in.
    pub(crate) fn text_of(&self, label: &str, index: usize, count: usize) -> String {
        let named = self
            .labels
            .get(label)
            .and_then(|label| pick(&label.texts, label.text_method, index, count));
        // Words on a filled piece read in the paper's colour; words on
        // nothing read in the ink's.
        named.unwrap_or_else(|| {
            if self.fill_of(label, index, count).is_some() {
                self.paper.clone()
            } else {
                self.ink.clone()
            }
        })
    }

    /// The first accent, for a piece with no label: what a diagram is drawn
    /// in when nothing says otherwise.
    pub(crate) fn accent(&self) -> &str {
        &self.accent
    }
}

/// The colour a piece takes from a list, by its place among the pieces.
fn pick(colours: &[String], method: Method, index: usize, count: usize) -> Option<String> {
    if colours.is_empty() {
        return None;
    }
    let at = match method {
        Method::Repeat => index % colours.len(),
        Method::Cycle => {
            // Along and back: 0 1 2 1 0 1 2 …
            let period = (colours.len() * 2).saturating_sub(2).max(1);
            let step = index % period;
            if step < colours.len() {
                step
            } else {
                period - step
            }
        }
        Method::Span => {
            if count <= 1 {
                0
            } else {
                (index * (colours.len() - 1) + (count - 1) / 2) / (count - 1)
            }
        }
    };
    colours.get(at.min(colours.len() - 1)).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_xml::tree::XmlTree;

    fn styling(colours: &str, style: &str) -> Styling {
        let colours = XmlTree::parse(colours).expect("the colours parse");
        let style = XmlTree::parse(style).expect("the style parses");
        Styling::read(Some(&colours.root), Some(&style.root), &Theme::default())
    }

    const COLOURS: &str = r#"<dgm:colorsDef xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
        <dgm:styleLbl name="node1"><dgm:fillClrLst meth="repeat"><a:srgbClr val="FF0000"/><a:srgbClr val="00FF00"/></dgm:fillClrLst>
        <dgm:linClrLst meth="repeat"><a:srgbClr val="0000FF"/></dgm:linClrLst>
        <dgm:txFillClrLst meth="repeat"><a:srgbClr val="FFFFFF"/></dgm:txFillClrLst></dgm:styleLbl>
        <dgm:styleLbl name="span"><dgm:fillClrLst meth="span"><a:srgbClr val="000000"/><a:srgbClr val="FFFFFF"/></dgm:fillClrLst></dgm:styleLbl>
        </dgm:colorsDef>"#;
    const STYLE: &str = r#"<dgm:styleDef xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
        <dgm:styleLbl name="node1"><dgm:style><a:lnRef idx="0"/><a:fillRef idx="1"/></dgm:style></dgm:styleLbl>
        <dgm:styleLbl name="bare"><dgm:style><a:lnRef idx="1"/><a:fillRef idx="0"/></dgm:style></dgm:styleLbl>
        </dgm:styleDef>"#;

    #[test]
    fn a_label_hands_its_colours_out_in_turn() {
        let styling = styling(COLOURS, STYLE);
        assert_eq!(styling.fill_of("node1", 0, 3).as_deref(), Some("FF0000"));
        assert_eq!(styling.fill_of("node1", 1, 3).as_deref(), Some("00FF00"));
        assert_eq!(styling.fill_of("node1", 2, 3).as_deref(), Some("FF0000"));
        assert_eq!(styling.text_of("node1", 0, 3), "FFFFFF");
    }

    #[test]
    fn the_quick_style_says_whether_there_is_a_line_or_a_fill_at_all() {
        let styling = styling(COLOURS, STYLE);
        // A line index of nought is no line, whatever colour the list names.
        assert_eq!(styling.line_of("node1", 0, 1), None);
        // And a fill index of nought is no fill.
        assert_eq!(styling.fill_of("bare", 0, 1), None);
    }

    #[test]
    fn a_label_nobody_named_is_the_first_accent() {
        let styling = styling(COLOURS, STYLE);
        assert_eq!(styling.fill_of("nothing", 0, 1).as_deref(), Some(styling.accent()));
        assert_eq!(styling.text_of("nothing", 0, 1), Theme::default().color(Slot::Light1));
    }

    #[test]
    fn a_span_runs_from_the_first_colour_to_the_last() {
        let styling = styling(COLOURS, STYLE);
        assert_eq!(styling.fill_of("span", 0, 2).as_deref(), Some("000000"));
        assert_eq!(styling.fill_of("span", 1, 2).as_deref(), Some("FFFFFF"));
    }

    #[test]
    fn a_cycle_goes_along_and_back() {
        let colours = vec!["A".to_owned(), "B".to_owned(), "C".to_owned()];
        let picked: Vec<String> =
            (0..6).map(|i| pick(&colours, Method::Cycle, i, 6).unwrap()).collect();
        assert_eq!(picked, vec!["A", "B", "C", "B", "A", "B"]);
    }
}
