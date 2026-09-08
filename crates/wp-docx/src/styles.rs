//! Style definitions and how formatting is inherited.
//!
//! What a paragraph actually looks like is never written in one place. A run
//! inherits from its character style, that from the paragraph style, that from
//! the style it is based on, and the chain ends at the document defaults. Each
//! layer contributes only what it says, and anything it leaves unset falls
//! through to the layer below.
//!
//! Getting this wrong is not subtle: a heading would render as body text, and a
//! run that switches bold off would come out bold. So resolution is done here,
//! once, rather than guessed at wherever formatting is needed.
//!
//! The order Word applies, and the order used here:
//!
//! 1. `w:docDefaults`
//! 2. the default paragraph style, if one is marked as such
//! 3. the paragraph style, with everything it is based on applied first
//! 4. properties written directly on the paragraph
//! 5. the character style named by the run
//! 6. properties written directly on the run

use wp_xml::tree::Element;

use crate::model::{
    ParagraphProperties, ResolvedParagraphProperties, ResolvedRunProperties, RunProperties,
    TableBorders, Underline,
};
use crate::read::{read_paragraph_properties, read_run_properties, read_table_borders, value, W};
use crate::theme::Theme;

/// What a style can be applied to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StyleKind {
    Paragraph,
    Character,
    Table,
    Numbering,
    /// A kind this model does not name.
    Other,
}

impl StyleKind {
    fn from_attribute(value: Option<&str>) -> Self {
        match value {
            Some("paragraph") => Self::Paragraph,
            Some("character") => Self::Character,
            Some("table") => Self::Table,
            Some("numbering") => Self::Numbering,
            _ => Self::Other,
        }
    }
}

/// One style definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Style {
    /// The identifier documents refer to, such as `Heading1`.
    pub id: String,
    /// The name shown to the user, such as "heading 1".
    pub name: Option<String>,
    pub kind: StyleKind,
    /// The style this one starts from.
    pub based_on: Option<String>,
    /// The style applied to the next paragraph when the user presses Enter.
    pub next: Option<String>,
    /// Whether this is the default style for its kind.
    pub is_default: bool,
    pub paragraph: ParagraphProperties,
    pub run: RunProperties,
    /// The lines a table style draws. Empty for every other kind of style.
    ///
    /// This is where most real tables get their borders: the table itself says
    /// only "TableGrid", and the grid lines live in the style.
    pub table_borders: TableBorders,
}

/// Every style in a document, and the document defaults.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Styles {
    default_paragraph: ParagraphProperties,
    default_run: RunProperties,
    styles: Vec<Style>,
    /// The colours and fonts the styles are named after.
    ///
    /// Kept here rather than beside the document because this is where names
    /// are turned into what they stand for, and a name resolved anywhere else
    /// would be resolved twice or not at all.
    theme: Theme,
}

/// How deep a chain of `w:basedOn` references is followed.
///
/// A document can name a style that is based on itself, directly or through a
/// ring. Following that would not terminate, so the chain is bounded.
const MAX_STYLE_DEPTH: usize = 32;

impl Styles {
    /// The same styles, told what the theme is.
    #[must_use]
    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// The theme the styles resolve names against.
    #[must_use]
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Reads a `w:styles` element.
    #[must_use]
    pub fn parse(root: &Element) -> Self {
        let mut result = Self::default();

        if let Some(defaults) = root.child(Some(W), "docDefaults") {
            if let Some(run) = defaults
                .child(Some(W), "rPrDefault")
                .and_then(|element| element.child(Some(W), "rPr"))
            {
                result.default_run = read_run_properties(run);
            }
            if let Some(paragraph) = defaults
                .child(Some(W), "pPrDefault")
                .and_then(|element| element.child(Some(W), "pPr"))
            {
                result.default_paragraph = read_paragraph_properties(paragraph);
            }
        }

        for definition in root.children_named(Some(W), "style") {
            let Some(id) = definition.attribute(Some(W), "styleId") else {
                continue;
            };
            result.styles.push(Style {
                id: id.to_owned(),
                name: definition.child(Some(W), "name").and_then(value).map(str::to_owned),
                kind: StyleKind::from_attribute(definition.attribute(Some(W), "type")),
                based_on: definition.child(Some(W), "basedOn").and_then(value).map(str::to_owned),
                next: definition.child(Some(W), "next").and_then(value).map(str::to_owned),
                is_default: definition
                    .attribute(Some(W), "default")
                    .is_some_and(|flag| !matches!(flag, "0" | "false")),
                paragraph: definition
                    .child(Some(W), "pPr")
                    .map(read_paragraph_properties)
                    .unwrap_or_default(),
                run: definition.child(Some(W), "rPr").map(read_run_properties).unwrap_or_default(),
                table_borders: definition
                    .child(Some(W), "tblPr")
                    .and_then(|properties| properties.child(Some(W), "tblBorders"))
                    .map(read_table_borders)
                    .unwrap_or_default(),
            });
        }

        result
    }

    /// The borders a table style asks for, following what it is based on.
    #[must_use]
    pub fn resolve_table_borders(&self, id: Option<&str>) -> TableBorders {
        let mut accumulated = TableBorders::default();
        if let Some(id) = id {
            for style in self.chain(id) {
                accumulated = accumulated.overlaid_with(&style.table_borders);
            }
        }
        accumulated
    }

    /// Every style, in the order the document declares them.
    #[must_use]
    pub fn all(&self) -> &[Style] {
        &self.styles
    }

    /// Looks a style up by identifier.
    ///
    /// Identifiers are compared without regard to ASCII case, which is what Word
    /// does — a document referring to `heading1` finds `Heading1`.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Style> {
        self.styles.iter().find(|style| style.id.eq_ignore_ascii_case(id))
    }

    /// The style marked as the default for a kind, if any.
    #[must_use]
    pub fn default_style(&self, kind: StyleKind) -> Option<&Style> {
        self.styles.iter().find(|style| style.is_default && style.kind == kind)
    }

    /// The chain of styles behind an identifier, outermost ancestor first.
    ///
    /// The result ends with the named style itself, so applying the chain in
    /// order gives the right precedence.
    #[must_use]
    pub fn chain(&self, id: &str) -> Vec<&Style> {
        let mut chain = Vec::new();
        let mut current = self.get(id);

        while let Some(style) = current {
            if chain.len() >= MAX_STYLE_DEPTH
                || chain.iter().any(|seen: &&Style| seen.id == style.id)
            {
                // A ring of basedOn references, which a valid document does not
                // have but a damaged one might.
                break;
            }
            chain.push(style);
            current = style.based_on.as_deref().and_then(|parent| self.get(parent));
        }

        chain.reverse();
        chain
    }

    /// Works out what a paragraph's formatting actually is.
    #[must_use]
    pub fn resolve_paragraph(&self, direct: &ParagraphProperties) -> ResolvedParagraphProperties {
        let mut accumulated = self.default_paragraph.clone();

        if let Some(default) = self.default_style(StyleKind::Paragraph) {
            accumulated = accumulated.overlaid_with(&default.paragraph);
        }
        if let Some(id) = &direct.style {
            for style in self.chain(id) {
                accumulated = accumulated.overlaid_with(&style.paragraph);
            }
        }
        accumulated = accumulated.overlaid_with(direct);

        ResolvedParagraphProperties {
            alignment: accumulated.alignment.unwrap_or_default(),
            right_to_left: accumulated.right_to_left.unwrap_or(false),
            indent_start: accumulated.indent_start.unwrap_or(0),
            indent_end: accumulated.indent_end.unwrap_or(0),
            indent_first_line: accumulated.indent_first_line.unwrap_or(0),
            space_before: accumulated.space_before.unwrap_or(0),
            space_after: accumulated.space_after.unwrap_or(0),
            line_spacing: accumulated.line_spacing,
            keep_next: accumulated.keep_next.unwrap_or(false),
            keep_lines: accumulated.keep_lines.unwrap_or(false),
            page_break_before: accumulated.page_break_before.unwrap_or(false),
            // Word turns widow control on unless a document says otherwise.
            widow_control: accumulated.widow_control.unwrap_or(true),
            outline_level: accumulated.outline_level,
            borders: accumulated.borders,
            shading: accumulated.shading,
            numbering: accumulated.numbering,
            tab_stops: accumulated.tab_stops,
        }
    }

    /// Works out what a run's formatting actually is.
    ///
    /// The paragraph's style matters here as well as the run's own: a run inside
    /// a heading is bold because the *paragraph* style says so.
    #[must_use]
    pub fn resolve_run(
        &self,
        paragraph_style: Option<&str>,
        direct: &RunProperties,
    ) -> ResolvedRunProperties {
        let mut accumulated = self.default_run.clone();

        if let Some(default) = self.default_style(StyleKind::Paragraph) {
            accumulated = accumulated.overlaid_with(&default.run);
        }
        if let Some(id) = paragraph_style {
            for style in self.chain(id) {
                accumulated = accumulated.overlaid_with(&style.run);
            }
        }
        if let Some(id) = &direct.style {
            for style in self.chain(id) {
                accumulated = accumulated.overlaid_with(&style.run);
            }
        }
        accumulated = accumulated.overlaid_with(direct);

        // A name is resolved only where nothing was written out: a run that
        // says both means the written colour, and the name beside it is the
        // answer somebody worked out last.
        let color = accumulated
            .color
            .clone()
            .or_else(|| accumulated.color_theme.as_ref().map(|named| self.theme.resolve(named)));
        let font = accumulated
            .font
            .clone()
            .or_else(|| accumulated.font_theme.map(|slot| self.theme.font(slot)));

        ResolvedRunProperties {
            bold: accumulated.bold.unwrap_or(false),
            italic: accumulated.italic.unwrap_or(false),
            strike: accumulated.strike.unwrap_or(false),
            right_to_left: accumulated.right_to_left.unwrap_or(false),
            underline: accumulated.underline.unwrap_or(Underline::None),
            size_half_points: accumulated.size_half_points.unwrap_or(20),
            color,
            highlight: accumulated.highlight,
            vertical_align: accumulated.vertical_align.unwrap_or_default(),
            font,
            language: accumulated.language,
            // "No effect" is what a run says to overrule an effect its style
            // put on it, and it resolves to nothing rather than to itself.
            effect: accumulated.effect.filter(|value| value.effect != crate::effects::Effect::None),
        }
    }
}

impl crate::Document {
    /// Tells the styles which theme to resolve names against.
    ///
    /// Called when the theme is replaced, so that a colour named after it comes
    /// out the new colour without the document being closed and opened again.
    pub(crate) fn set_styles_theme(&mut self, theme: Theme) {
        self.styles_mut().theme = theme;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_xml::tree::XmlTree;

    fn styles_from(inner: &str) -> Styles {
        let source = format!("<w:styles xmlns:w=\"{W}\">{inner}</w:styles>");
        let tree = XmlTree::parse(&source).unwrap();
        Styles::parse(&tree.root)
    }

    const SAMPLE: &str = r#"
        <w:docDefaults><w:rPrDefault><w:rPr>
            <w:rFonts w:ascii="Calibri"/><w:sz w:val="22"/>
        </w:rPr></w:rPrDefault></w:docDefaults>
        <w:style w:type="paragraph" w:default="1" w:styleId="Normal">
            <w:name w:val="Normal"/>
        </w:style>
        <w:style w:type="paragraph" w:styleId="Heading1">
            <w:name w:val="heading 1"/><w:basedOn w:val="Normal"/>
            <w:pPr><w:outlineLvl w:val="0"/><w:spacing w:before="240"/></w:pPr>
            <w:rPr><w:b/><w:sz w:val="32"/></w:rPr>
        </w:style>
        <w:style w:type="paragraph" w:styleId="Heading2">
            <w:name w:val="heading 2"/><w:basedOn w:val="Heading1"/>
            <w:rPr><w:sz w:val="26"/></w:rPr>
        </w:style>
        <w:style w:type="character" w:styleId="Emphasis">
            <w:rPr><w:i/></w:rPr>
        </w:style>
    "#;

    #[test]
    fn document_defaults_apply_when_nothing_else_does() {
        let styles = styles_from(SAMPLE);
        let resolved = styles.resolve_run(None, &RunProperties::default());

        assert_eq!(resolved.size_half_points, 22);
        assert_eq!(resolved.font.as_deref(), Some("Calibri"));
        assert!(!resolved.bold);
    }

    #[test]
    fn a_paragraph_style_reaches_the_runs_inside_it() {
        // A run in a heading is bold because the paragraph style says so, not
        // because the run does.
        let styles = styles_from(SAMPLE);
        let resolved = styles.resolve_run(Some("Heading1"), &RunProperties::default());

        assert!(resolved.bold, "the heading style should have made it bold");
        assert_eq!(resolved.size_half_points, 32);
        // Inherited through basedOn from the document defaults.
        assert_eq!(resolved.font.as_deref(), Some("Calibri"));
    }

    #[test]
    fn a_style_inherits_from_the_one_it_is_based_on() {
        let styles = styles_from(SAMPLE);
        let resolved = styles.resolve_run(Some("Heading2"), &RunProperties::default());

        // Its own size wins, but the boldness comes from Heading1.
        assert_eq!(resolved.size_half_points, 26);
        assert!(resolved.bold);
    }

    #[test]
    fn direct_formatting_beats_the_style() {
        let styles = styles_from(SAMPLE);
        let direct = RunProperties { size_half_points: Some(48), ..RunProperties::default() };
        let resolved = styles.resolve_run(Some("Heading1"), &direct);

        assert_eq!(resolved.size_half_points, 48);
        assert!(resolved.bold, "what the run did not override should still apply");
    }

    #[test]
    fn a_run_can_switch_off_what_its_style_turned_on() {
        // This is the case a plain bool cannot represent, and the reason the
        // authored properties are all Option.
        let styles = styles_from(SAMPLE);
        let direct = RunProperties { bold: Some(false), ..RunProperties::default() };
        let resolved = styles.resolve_run(Some("Heading1"), &direct);

        assert!(!resolved.bold, "the run explicitly turned bold off");
        assert_eq!(resolved.size_half_points, 32, "the rest of the style still applies");
    }

    #[test]
    fn a_character_style_applies_on_top_of_the_paragraph_style() {
        let styles = styles_from(SAMPLE);
        let direct =
            RunProperties { style: Some("Emphasis".to_owned()), ..RunProperties::default() };
        let resolved = styles.resolve_run(Some("Heading1"), &direct);

        assert!(resolved.italic, "the character style should apply");
        assert!(resolved.bold, "and the paragraph style should still apply");
    }

    #[test]
    fn paragraph_properties_resolve_through_the_chain() {
        let styles = styles_from(SAMPLE);
        let direct = ParagraphProperties {
            style: Some("Heading2".to_owned()),
            ..ParagraphProperties::default()
        };
        let resolved = styles.resolve_paragraph(&direct);

        assert_eq!(resolved.outline_level, Some(0), "inherited from Heading1");
        assert_eq!(resolved.space_before, 240);
        assert!(resolved.widow_control, "Word turns this on unless told otherwise");
    }

    #[test]
    fn style_identifiers_ignore_ascii_case() {
        let styles = styles_from(SAMPLE);
        assert!(styles.get("heading1").is_some());
        assert!(styles.get("HEADING1").is_some());
    }

    #[test]
    fn a_ring_of_based_on_references_does_not_hang() {
        // A damaged document can say A is based on B and B on A. Following that
        // would never terminate.
        let styles = styles_from(
            r#"<w:style w:type="paragraph" w:styleId="A"><w:basedOn w:val="B"/></w:style>
               <w:style w:type="paragraph" w:styleId="B"><w:basedOn w:val="A"/></w:style>"#,
        );

        assert_eq!(styles.chain("A").len(), 2);
        let _ = styles.resolve_run(Some("A"), &RunProperties::default());
    }

    #[test]
    fn an_unknown_style_falls_back_to_the_defaults() {
        let styles = styles_from(SAMPLE);
        let direct =
            ParagraphProperties { style: Some("NoSuchStyle".to_owned()), ..Default::default() };

        let resolved = styles.resolve_paragraph(&direct);
        assert_eq!(resolved.space_before, 0);

        let run = styles.resolve_run(Some("NoSuchStyle"), &RunProperties::default());
        assert_eq!(run.size_half_points, 22, "the document defaults still apply");
    }

    // --- Theme names ----------------------------------------------------------

    use crate::theme::{FontSlot, Slot, Theme, ThemeColor};

    /// A theme that is nothing like the Office one, so a resolved value cannot
    /// be right by accident.
    fn distinctive() -> Theme {
        Theme {
            name: "Test".to_owned(),
            colors: Slot::ALL.iter().map(|_| "112233".to_owned()).collect(),
            major_font: "Georgia".to_owned(),
            minor_font: "Verdana".to_owned(),
            effect: crate::theme::Effect::None,
        }
    }

    #[test]
    fn a_colour_named_after_the_theme_resolves_to_the_theme_colour() {
        let styles = styles_from(SAMPLE).with_theme(distinctive());
        let direct = RunProperties {
            color_theme: Some(ThemeColor { slot: Slot::Accent1, tint: None, shade: None }),
            ..RunProperties::default()
        };
        assert_eq!(styles.resolve_run(None, &direct).color.as_deref(), Some("112233"));
    }

    #[test]
    fn a_font_named_after_the_theme_resolves_to_the_theme_font() {
        let styles = styles_from(SAMPLE).with_theme(distinctive());
        let heading =
            RunProperties { font_theme: Some(FontSlot::Major), ..RunProperties::default() };
        assert_eq!(styles.resolve_run(None, &heading).font.as_deref(), Some("Georgia"));

        let body = RunProperties { font_theme: Some(FontSlot::Minor), ..RunProperties::default() };
        assert_eq!(styles.resolve_run(None, &body).font.as_deref(), Some("Verdana"));
    }

    #[test]
    fn a_colour_written_out_wins_over_the_name_beside_it() {
        let styles = styles_from(SAMPLE).with_theme(distinctive());
        let direct = RunProperties {
            color: Some("C00000".to_owned()),
            color_theme: Some(ThemeColor { slot: Slot::Accent1, tint: None, shade: None }),
            ..RunProperties::default()
        };
        assert_eq!(
            styles.resolve_run(None, &direct).color.as_deref(),
            Some("C00000"),
            "the name is the answer somebody worked out last, not the answer"
        );
    }

    #[test]
    fn a_font_written_out_wins_over_the_name_beside_it() {
        let styles = styles_from(SAMPLE).with_theme(distinctive());
        let direct = RunProperties {
            font: Some("Courier New".to_owned()),
            font_theme: Some(FontSlot::Minor),
            ..RunProperties::default()
        };
        assert_eq!(styles.resolve_run(None, &direct).font.as_deref(), Some("Courier New"));
    }

    #[test]
    fn a_name_is_inherited_down_the_style_chain_like_anything_else() {
        let styles = styles_from(
            r#"<w:style w:type="paragraph" w:styleId="Named">
                 <w:name w:val="Named"/>
                 <w:rPr><w:color w:themeColor="accent1"/><w:rFonts w:asciiTheme="majorHAnsi"/></w:rPr>
               </w:style>"#,
        )
        .with_theme(distinctive());

        let run = styles.resolve_run(Some("Named"), &RunProperties::default());
        assert_eq!(run.color.as_deref(), Some("112233"));
        assert_eq!(run.font.as_deref(), Some("Georgia"));
    }

    #[test]
    fn a_run_of_its_own_overrides_a_name_it_inherited() {
        let styles = styles_from(
            r#"<w:style w:type="paragraph" w:styleId="Named">
                 <w:name w:val="Named"/>
                 <w:rPr><w:rFonts w:asciiTheme="majorHAnsi"/></w:rPr>
               </w:style>"#,
        )
        .with_theme(distinctive());

        let direct = RunProperties { font: Some("Arial".to_owned()), ..RunProperties::default() };
        assert_eq!(styles.resolve_run(Some("Named"), &direct).font.as_deref(), Some("Arial"));
    }

    #[test]
    fn styles_told_nothing_resolve_against_the_office_theme() {
        let styles = styles_from(SAMPLE);
        let direct =
            RunProperties { font_theme: Some(FontSlot::Minor), ..RunProperties::default() };
        assert_eq!(styles.resolve_run(None, &direct).font.as_deref(), Some("Calibri"));
    }

    #[test]
    fn a_tint_is_read_off_the_run_and_applied() {
        let properties = read_run_properties(
            &XmlTree::parse(&format!(
                r#"<w:rPr xmlns:w="{W}"><w:color w:themeColor="accent1" w:themeTint="00"/></w:rPr>"#
            ))
            .unwrap()
            .root,
        );
        let named = properties.color_theme.clone().expect("a named colour");
        assert_eq!(named.slot, Slot::Accent1);
        assert_eq!(named.tint, Some(0));
        assert_eq!(distinctive().resolve(&named), "FFFFFF", "no tint at all is white");
    }

    #[test]
    fn a_run_that_names_no_theme_carries_no_name() {
        let properties = read_run_properties(
            &XmlTree::parse(&format!(
                r#"<w:rPr xmlns:w="{W}"><w:color w:val="C00000"/><w:rFonts w:ascii="Arial"/></w:rPr>"#
            ))
            .unwrap()
            .root,
        );
        assert!(properties.color_theme.is_none());
        assert!(properties.font_theme.is_none());
        assert_eq!(properties.color.as_deref(), Some("C00000"));
    }

    #[test]
    fn a_colour_of_auto_is_no_colour_rather_than_the_word_auto() {
        let properties = read_run_properties(
            &XmlTree::parse(&format!(r#"<w:rPr xmlns:w="{W}"><w:color w:val="auto"/></w:rPr>"#))
                .unwrap()
                .root,
        );
        assert_eq!(properties.color, None);
    }
}
