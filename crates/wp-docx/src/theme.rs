//! The document's theme: the colours and fonts everything else is named after.
//!
//! # Why a document does not say what colour its text is
//!
//! Open a heading Word made and it does not say `4472C4`. It says
//! `w:themeColor="accent1"` — a *name*, resolved through `word/theme/theme1.xml`
//! to a colour. Change the theme and every heading in the document changes with
//! it, because none of them ever said a colour in the first place.
//!
//! The same is true of the fonts. A document made by Word says
//! `w:asciiTheme="minorHAnsi"`, not `Calibri`. Calibri is only what the Office
//! theme's minor font happens to be.
//!
//! # What that means for a program that ignores it
//!
//! It shows the wrong font and the wrong colour for nearly every document Word
//! has ever saved — not occasionally, but as the normal case. That is why this
//! is here.
//!
//! # What is read and what is not
//!
//! The colour scheme and the font scheme. Not the format scheme, which is the
//! fills, lines and effects that shapes take from the theme — there are no
//! shapes here to take them yet.

use wp_xml::tree::{Element, Node, XmlTree};

use crate::Document;

/// The namespace the theme is written in.
///
/// DrawingML, not WordprocessingML: a theme belongs to the whole of Office and
/// is the same part in a spreadsheet or a presentation.
pub const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

/// Relationship type of the theme part.
const RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";

/// The colours a theme names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// The first dark colour, which the text usually resolves to.
    Dark1,
    /// The first light one, which the page usually resolves to.
    Light1,
    Dark2,
    Light2,
    Accent1,
    Accent2,
    Accent3,
    Accent4,
    Accent5,
    Accent6,
    Hyperlink,
    FollowedHyperlink,
}

impl Slot {
    /// The element the scheme keeps it in.
    #[must_use]
    fn element(self) -> &'static str {
        match self {
            Self::Dark1 => "dk1",
            Self::Light1 => "lt1",
            Self::Dark2 => "dk2",
            Self::Light2 => "lt2",
            Self::Accent1 => "accent1",
            Self::Accent2 => "accent2",
            Self::Accent3 => "accent3",
            Self::Accent4 => "accent4",
            Self::Accent5 => "accent5",
            Self::Accent6 => "accent6",
            Self::Hyperlink => "hlink",
            Self::FollowedHyperlink => "folHlink",
        }
    }

    /// The slot a `w:themeColor` names.
    ///
    /// Two vocabularies for the same twelve colours: the drawing side calls
    /// them dark and light, the text side calls them text and background. They
    /// are the same slots, mapped the way Word maps them by default — text to
    /// the darks and background to the lights.
    #[must_use]
    pub fn from_word(name: &str) -> Option<Self> {
        Some(match name {
            "dark1" | "text1" => Self::Dark1,
            "light1" | "background1" => Self::Light1,
            "dark2" | "text2" => Self::Dark2,
            "light2" | "background2" => Self::Light2,
            "accent1" => Self::Accent1,
            "accent2" => Self::Accent2,
            "accent3" => Self::Accent3,
            "accent4" => Self::Accent4,
            "accent5" => Self::Accent5,
            "accent6" => Self::Accent6,
            "hyperlink" => Self::Hyperlink,
            "followedHyperlink" => Self::FollowedHyperlink,
            _ => return None,
        })
    }

    /// Every slot, in the order a scheme writes them.
    pub const ALL: &'static [Self] = &[
        Self::Dark1,
        Self::Light1,
        Self::Dark2,
        Self::Light2,
        Self::Accent1,
        Self::Accent2,
        Self::Accent3,
        Self::Accent4,
        Self::Accent5,
        Self::Accent6,
        Self::Hyperlink,
        Self::FollowedHyperlink,
    ];
}

/// Which of the two fonts a `w:rFonts` theme attribute names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontSlot {
    /// The heading font.
    Major,
    /// The body font.
    Minor,
}

impl FontSlot {
    /// The slot a `w:asciiTheme` names.
    ///
    /// The suffix says which script the font is for — `HAnsi` is the Latin one,
    /// and it is the only one this reads, because the layout asks for one font
    /// name and not four.
    #[must_use]
    pub fn from_word(name: &str) -> Option<Self> {
        if name.starts_with("major") {
            return Some(Self::Major);
        }
        if name.starts_with("minor") {
            return Some(Self::Minor);
        }
        None
    }
}

/// A colour named rather than written out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeColor {
    pub slot: Slot,
    /// Lightens the colour towards white. 255 leaves it alone.
    pub tint: Option<u8>,
    /// Darkens it towards black. 255 leaves it alone.
    pub shade: Option<u8>,
}

/// How much of a shadow the theme puts under the shapes in a document.
///
/// Word's Design ▸ Effects gallery, reduced to what it actually changes for a
/// plain shape: how far the shadow falls and how heavy it is. The gallery has
/// fifteen entries; behind them are a handful of shadows and a great many
/// gradients and bevels that a shape with a flat fill never shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Effect {
    /// Flat shapes, which is what a new document has.
    #[default]
    None,
    /// A soft shadow close under the shape.
    Subtle,
    /// Further out and darker.
    Moderate,
    /// A shadow nobody could miss.
    Intense,
}

impl Effect {
    pub const ALL: &'static [Self] = &[Self::None, Self::Subtle, Self::Moderate, Self::Intense];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "No effects",
            Self::Subtle => "Subtle shadow",
            Self::Moderate => "Moderate shadow",
            Self::Intense => "Intense shadow",
        }
    }

    /// How far the shadow falls, in English metric units, and how much of it
    /// shows through — the two numbers the format writes.
    #[must_use]
    pub fn shadow(self) -> Option<(i64, u32)> {
        match self {
            Self::None => Option::None,
            // A point and a half, a quarter opaque.
            Self::Subtle => Some((19_050, 25_000)),
            Self::Moderate => Some((38_100, 40_000)),
            Self::Intense => Some((63_500, 60_000)),
        }
    }

    /// The one whose shadow falls this far, for reading a theme back.
    #[must_use]
    pub fn from_distance(distance: i64) -> Self {
        // The nearest of the three, so a theme Word wrote with a distance of
        // its own still reads as the effect it looks like.
        Self::ALL
            .iter()
            .copied()
            .filter_map(|effect| effect.shadow().map(|(far, _)| (effect, far)))
            .min_by_key(|(_, far)| (far - distance).abs())
            .map_or(Self::None, |(effect, _)| effect)
    }
}
/// The colours and fonts a document is named after.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    /// What the theme calls itself.
    pub name: String,
    /// The twelve colours, each as six hex digits, in [`Slot::ALL`] order.
    pub colors: Vec<String>,
    /// The heading font and the body font.
    pub major_font: String,
    pub minor_font: String,
    /// The shadow the theme puts under a shape.
    pub effect: Effect,
}

impl Default for Theme {
    /// The Office theme, which is what a document with no theme part gets.
    ///
    /// Not an invention: these are the values in the theme Word puts in every
    /// new document, and a document that leaves the part out is one Word shows
    /// with exactly these.
    fn default() -> Self {
        Self {
            name: "Office".to_owned(),
            colors: [
                "000000", "FFFFFF", "44546A", "E7E6E6", "4472C4", "ED7D31", "A5A5A5", "FFC000",
                "5B9BD5", "70AD47", "0563C1", "954F72",
            ]
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
            major_font: "Calibri Light".to_owned(),
            minor_font: "Calibri".to_owned(),
            effect: Effect::None,
        }
    }
}

impl Theme {
    /// One of the colours, as six hex digits.
    #[must_use]
    pub fn color(&self, slot: Slot) -> String {
        let at = Slot::ALL.iter().position(|entry| *entry == slot).unwrap_or(0);
        self.colors.get(at).cloned().unwrap_or_else(|| "000000".to_owned())
    }

    /// One of the two fonts.
    #[must_use]
    pub fn font(&self, slot: FontSlot) -> String {
        match slot {
            FontSlot::Major => self.major_font.clone(),
            FontSlot::Minor => self.minor_font.clone(),
        }
    }

    /// What a named colour actually comes out as, tint and shade applied.
    #[must_use]
    pub fn resolve(&self, wanted: &ThemeColor) -> String {
        let base = self.color(wanted.slot);
        let Some((red, green, blue)) = split(&base) else { return base };

        let (red, green, blue) = match (wanted.tint, wanted.shade) {
            (Some(tint), _) => (tinted(red, tint), tinted(green, tint), tinted(blue, tint)),
            (None, Some(shade)) => (shaded(red, shade), shaded(green, shade), shaded(blue, shade)),
            (None, None) => (red, green, blue),
        };
        format!("{red:02X}{green:02X}{blue:02X}")
    }

    /// Reads a theme out of its part.
    #[must_use]
    pub fn parse(root: &Element) -> Self {
        let mut theme = Self { colors: Vec::new(), ..Self::default() };
        if let Some(name) = root.attribute_by_name("name") {
            theme.name = name.to_owned();
        }

        let elements = find(root, "themeElements");
        let scheme = elements.and_then(|elements| child(elements, "clrScheme"));
        let fallback = Self::default();

        for slot in Slot::ALL {
            let colour = scheme
                .and_then(|scheme| child(scheme, slot.element()))
                .and_then(read_color)
                .unwrap_or_else(|| fallback.color(*slot));
            theme.colors.push(colour);
        }

        if let Some(fonts) = elements.and_then(|elements| child(elements, "fontScheme")) {
            if let Some(name) = read_latin(fonts, "majorFont") {
                theme.major_font = name;
            }
            if let Some(name) = read_latin(fonts, "minorFont") {
                theme.minor_font = name;
            }
            if let Some(name) = fonts.attribute_by_name("name") {
                // The font scheme's name is what Word shows in the Fonts list,
                // and it is not always the theme's own.
                let _ = name;
            }
        }
        // The shadow the theme puts under a shape, read from the strongest of
        // the three effect styles that has one.
        theme.effect = find(root, "effectStyleLst")
            .into_iter()
            .flat_map(|list| list.child_elements())
            .filter_map(|style| find(style, "outerShdw"))
            .filter_map(|shadow| shadow.attribute_by_name("dist"))
            .filter_map(|value| value.trim().parse::<i64>().ok())
            .max()
            // Written as one level per style, so the strongest is the level
            // itself doubled — undone here to get back the effect asked for.
            .map(|distance| Effect::from_distance(distance / 2))
            .unwrap_or(Effect::None);

        theme
    }
}

impl Document {
    /// The document's theme, or the Office one when it has no theme part.
    #[must_use]
    pub fn theme(&self) -> Theme {
        let Some(part) = self.theme_part() else { return Theme::default() };
        let Some(Ok(text)) = self.package().xml_part(&part) else { return Theme::default() };
        let Ok(tree) = XmlTree::parse(&text) else { return Theme::default() };
        Theme::parse(&tree.root)
    }

    /// The name of the theme part, if the document has one.
    #[must_use]
    pub fn theme_part(&self) -> Option<String> {
        let relationships = self.package().relationships(self.main_part()).ok()?;
        let relationship = relationships.single_by_type(RELATIONSHIP)?;
        relationship.resolved_target(self.main_part())?.ok()
    }
}

/// A colour's three bytes, if it is six hex digits.
fn split(value: &str) -> Option<(u8, u8, u8)> {
    let value = value.trim_start_matches('#');
    if value.len() != 6 {
        return None;
    }
    let byte = |at: usize| u8::from_str_radix(&value[at..at + 2], 16).ok();
    Some((byte(0)?, byte(2)?, byte(4)?))
}

/// Lightens one component towards white.
///
/// A tint of 255 leaves the colour alone and one of 0 makes it white, which is
/// the way round the format defines it — the number is how much of the original
/// survives, not how much white is added.
fn tinted(component: u8, tint: u8) -> u8 {
    let weight = f32::from(tint) / 255.0;
    let value = f32::from(component) * weight + 255.0 * (1.0 - weight);
    value.round().clamp(0.0, 255.0) as u8
}

/// Darkens one component towards black.
fn shaded(component: u8, shade: u8) -> u8 {
    let weight = f32::from(shade) / 255.0;
    (f32::from(component) * weight).round().clamp(0.0, 255.0) as u8
}

/// A child by local name, whatever prefix it was written with.
fn child<'a>(parent: &'a Element, local: &str) -> Option<&'a Element> {
    parent.child_elements().find(|child| child.local_name() == local)
}

/// The named element anywhere under a root.
fn find<'a>(root: &'a Element, local: &str) -> Option<&'a Element> {
    if root.local_name() == local {
        return Some(root);
    }
    root.child_elements().find_map(|child| find(child, local))
}

/// The colour inside one slot of a scheme.
///
/// Two ways of writing one: as six hex digits, or as a system colour with the
/// hex digits kept beside it for anything that cannot ask the system. The
/// second is what `dk1` and `lt1` normally are.
fn read_color(slot: &Element) -> Option<String> {
    for entry in slot.child_elements() {
        match entry.local_name() {
            "srgbClr" => {
                if let Some(value) = entry.attribute_by_name("val") {
                    return Some(value.to_uppercase());
                }
            }
            "sysClr" => {
                if let Some(value) = entry.attribute_by_name("lastClr") {
                    return Some(value.to_uppercase());
                }
            }
            _ => {}
        }
    }
    None
}

/// The Latin typeface of one of the two font slots.
fn read_latin(scheme: &Element, slot: &str) -> Option<String> {
    let latin = child(child(scheme, slot)?, "latin")?;
    let typeface = latin.attribute_by_name("typeface")?;
    (!typeface.is_empty()).then(|| typeface.to_owned())
}

/// Content type of the theme part.
const CONTENT_TYPE: &str = "application/vnd.openxmlformats-officedocument.theme+xml";

/// Where the theme part goes when the document has none.
const DEFAULT_PART: &str = "word/theme/theme1.xml";

impl Document {
    /// Writes a theme, replacing the one the document had.
    ///
    /// Only the colours and the fonts are replaced. Anything else the part held
    /// — the format scheme, and whatever a program that is not this one put
    /// there — is left as it was, because a theme is not this program's to
    /// rewrite wholesale.
    pub fn set_theme(&mut self, wanted: &Theme) -> Result<bool, crate::Error> {
        if self.theme() == *wanted {
            return Ok(false);
        }

        let mut root = self.theme_root(wanted);
        let Some(elements) = find_mut(&mut root, "themeElements") else { return Ok(false) };
        elements.children.retain(|node| match node {
            Node::Element(element) => {
                !matches!(element.local_name(), "clrScheme" | "fontScheme" | "fmtScheme")
            }
            _ => true,
        });
        // The schema wants the colours first, then the fonts, then the formats.
        elements.insert_element(0, colour_scheme(wanted));
        elements.insert_element(1, font_scheme(wanted));
        elements.insert_element(2, format_scheme(wanted));

        root.set_attribute("name", &wanted.name);
        self.save_theme_root(root)?;
        self.mark_modified();
        Ok(true)
    }

    /// The theme part's tree, read from the package or made afresh.
    fn theme_root(&self, wanted: &Theme) -> Element {
        if let Some(part) = self.theme_part() {
            if let Some(Ok(text)) = self.package().xml_part(&part) {
                if let Ok(tree) = XmlTree::parse(&text) {
                    return tree.root;
                }
            }
        }

        let mut root = Element::new("a:theme", Some(A));
        root.declarations.push((Some("a".to_owned()), A.to_owned()));
        let mut elements = Element::new("a:themeElements", Some(A));
        elements.push_element(format_scheme(wanted));
        root.push_element(elements);
        root
    }

    /// Writes the part back, adding the relationship the first time.
    fn save_theme_root(&mut self, root: Element) -> Result<(), crate::Error> {
        let tree = XmlTree {
            standalone: Some(true),
            has_declaration: true,
            doctype: None,
            before_root: Vec::new(),
            root,
            after_root: Vec::new(),
        };
        let part = self.theme_part().unwrap_or_else(|| DEFAULT_PART.to_owned());
        let xml =
            tree.to_xml().map_err(|source| crate::Error::Xml { part: part.clone(), source })?;
        self.package_mut().add_part(&part, CONTENT_TYPE, xml.into_bytes());

        let main_part = self.main_part().to_owned();
        let mut relationships = self
            .package()
            .relationships(&main_part)
            .unwrap_or_else(|_| wp_opc::Relationships::new(&main_part));
        if relationships.single_by_type(RELATIONSHIP).is_none() {
            let target = part.strip_prefix("word/").unwrap_or(&part).to_owned();
            relationships.add(RELATIONSHIP, &target, wp_opc::TargetMode::Internal);
            self.package_mut().set_relationships(&relationships)?;
        }

        // The styles resolve names against the theme, so they are told the new
        // one straight away rather than at the next opening.
        let theme = self.theme();
        self.set_styles_theme(theme);
        Ok(())
    }
}

/// The twelve colours as a scheme.
fn colour_scheme(theme: &Theme) -> Element {
    let mut scheme = Element::new("a:clrScheme", Some(A));
    scheme.set_attribute("name", &theme.name);
    for slot in Slot::ALL {
        let mut holder = Element::new(&format!("a:{}", slot.element()), Some(A));
        let mut colour = Element::new("a:srgbClr", Some(A));
        colour.set_attribute("val", &theme.color(*slot));
        holder.push_element(colour);
        scheme.push_element(holder);
    }
    scheme
}

/// The two fonts as a scheme.
///
/// Each slot names a font for Latin text and two empty ones for East Asian and
/// complex scripts. The empty ones are not padding: the schema requires all
/// three, and a font scheme missing them is one Word repairs.
fn font_scheme(theme: &Theme) -> Element {
    let mut scheme = Element::new("a:fontScheme", Some(A));
    scheme.set_attribute("name", &theme.name);

    for (slot, name) in [("majorFont", &theme.major_font), ("minorFont", &theme.minor_font)] {
        let mut holder = Element::new(&format!("a:{slot}"), Some(A));
        for (element, typeface) in [("latin", name.as_str()), ("ea", ""), ("cs", "")] {
            let mut font = Element::new(&format!("a:{element}"), Some(A));
            font.set_attribute("typeface", typeface);
            holder.push_element(font);
        }
        scheme.push_element(holder);
    }
    scheme
}

/// The plainest format scheme the schema will accept.
///
/// Fills, lines and effects for shapes to take from the theme. There are no
/// shapes here yet, so these are the simplest that satisfy the schema — three
/// of each, as it requires — rather than the graduated fills Word writes.
fn format_scheme(theme: &Theme) -> Element {
    let mut scheme = Element::new("a:fmtScheme", Some(A));
    scheme.set_attribute("name", "Office");

    let solid = |colour: &str| {
        let mut fill = Element::new("a:solidFill", Some(A));
        let mut reference = Element::new("a:schemeClr", Some(A));
        reference.set_attribute("val", colour);
        fill.push_element(reference);
        fill
    };

    let mut fills = Element::new("a:fillStyleLst", Some(A));
    let mut backgrounds = Element::new("a:bgFillStyleLst", Some(A));
    for _ in 0..3 {
        fills.push_element(solid("phClr"));
        backgrounds.push_element(solid("phClr"));
    }

    let mut lines = Element::new("a:lnStyleLst", Some(A));
    for width in [6350, 12700, 19050] {
        let mut line = Element::new("a:ln", Some(A));
        line.set_attribute("w", &width.to_string());
        line.set_attribute("cap", "flat");
        line.set_attribute("cmpd", "sng");
        line.set_attribute("algn", "ctr");
        line.push_element(solid("phClr"));
        lines.push_element(line);
    }

    let mut effects = Element::new("a:effectStyleLst", Some(A));
    for level in 0..3u32 {
        let mut style = Element::new("a:effectStyle", Some(A));
        let mut list = Element::new("a:effectLst", Some(A));
        // The three styles are meant to be a plain one, a subtle one and a
        // strong one, so the shadow the theme asks for goes on the second and
        // the third, growing.
        if let Some((distance, alpha)) = theme.effect.shadow() {
            if level > 0 {
                list.push_element(outer_shadow(distance * i64::from(level), alpha * level));
            }
        }
        style.push_element(list);
        effects.push_element(style);
    }
    scheme.push_element(fills);
    scheme.push_element(lines);
    scheme.push_element(effects);
    scheme.push_element(backgrounds);
    scheme
}

/// The shadow element a theme's effect style carries.
///
/// Straight down and to the right at forty-five degrees, which is where a
/// shadow falls in every default theme Word ships. The angle is in sixtieths of
/// a degree and the alpha in hundredths of a percent, which is how DrawingML
/// counts both.
fn outer_shadow(distance: i64, alpha: u32) -> Element {
    let mut shadow = Element::new("a:outerShdw", Some(A));
    shadow.set_attribute("blurRad", &(distance * 2).to_string());
    shadow.set_attribute("dist", &distance.to_string());
    shadow.set_attribute("dir", "2700000");
    shadow.set_attribute("algn", "tl");
    shadow.set_attribute("rotWithShape", "0");

    let mut colour = Element::new("a:srgbClr", Some(A));
    colour.set_attribute("val", "000000");
    let mut share = Element::new("a:alpha", Some(A));
    share.set_attribute("val", &alpha.min(100_000).to_string());
    colour.push_element(share);
    shadow.push_element(colour);
    shadow
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
    fn a_new_theme_puts_no_shadow_under_anything() {
        assert_eq!(Theme::default().effect, Effect::None);
    }

    #[test]
    fn every_effect_says_what_it_is_called() {
        for effect in Effect::ALL {
            assert!(!effect.label().is_empty());
        }
    }

    #[test]
    fn only_the_effects_that_draw_something_have_a_shadow() {
        assert_eq!(Effect::None.shadow(), None);
        for effect in &Effect::ALL[1..] {
            assert!(effect.shadow().is_some(), "{}", effect.label());
        }
    }

    #[test]
    fn a_stronger_effect_falls_further_and_shows_more() {
        let (subtle, subtle_alpha) = Effect::Subtle.shadow().expect("a shadow");
        let (intense, intense_alpha) = Effect::Intense.shadow().expect("a shadow");
        assert!(intense > subtle);
        assert!(intense_alpha > subtle_alpha);
    }

    #[test]
    fn every_effect_is_read_back_from_the_distance_it_writes() {
        for effect in &Effect::ALL[1..] {
            let (distance, _) = effect.shadow().expect("a shadow");
            assert_eq!(Effect::from_distance(distance), *effect, "{}", effect.label());
        }
    }

    #[test]
    fn an_effect_survives_being_written_into_a_theme_and_read_back() {
        for effect in Effect::ALL {
            let scheme = format_scheme(&Theme { effect: *effect, ..Theme::default() });
            let mut root = Element::new("a:theme", Some(A));
            let mut elements = Element::new("a:themeElements", Some(A));
            elements.push_element(colour_scheme(&Theme::default()));
            elements.push_element(font_scheme(&Theme::default()));
            elements.push_element(scheme);
            root.push_element(elements);

            assert_eq!(Theme::parse(&root).effect, *effect, "{}", effect.label());
        }
    }

    fn theme_from(inner: &str) -> Theme {
        let source = format!(
            "<a:theme xmlns:a=\"{A}\" name=\"Test\"><a:themeElements>{inner}</a:themeElements></a:theme>"
        );
        let tree = XmlTree::parse(&source).expect("a theme");
        Theme::parse(&tree.root)
    }

    #[test]
    fn a_document_with_no_theme_gets_the_office_one() {
        let theme = Theme::default();
        assert_eq!(theme.minor_font, "Calibri");
        assert_eq!(theme.major_font, "Calibri Light");
        assert_eq!(theme.color(Slot::Accent1), "4472C4");
    }

    #[test]
    fn a_colour_scheme_is_read_out_of_its_part() {
        let theme = theme_from(
            r#"<a:clrScheme name="Custom">
                 <a:dk1><a:sysClr val="windowText" lastClr="101010"/></a:dk1>
                 <a:lt1><a:sysClr val="window" lastClr="FAFAFA"/></a:lt1>
                 <a:accent1><a:srgbClr val="c00000"/></a:accent1>
               </a:clrScheme>"#,
        );
        assert_eq!(theme.color(Slot::Dark1), "101010");
        assert_eq!(theme.color(Slot::Light1), "FAFAFA");
        assert_eq!(theme.color(Slot::Accent1), "C00000", "written lower case, read upper");
    }

    #[test]
    fn a_slot_the_scheme_leaves_out_falls_back_to_the_office_colour() {
        let theme = theme_from(r#"<a:clrScheme name="Custom"></a:clrScheme>"#);
        assert_eq!(theme.color(Slot::Accent2), "ED7D31");
    }

    #[test]
    fn a_font_scheme_is_read_out_of_its_part() {
        let theme = theme_from(
            r#"<a:fontScheme name="Custom">
                 <a:majorFont><a:latin typeface="Georgia"/></a:majorFont>
                 <a:minorFont><a:latin typeface="Verdana"/></a:minorFont>
               </a:fontScheme>"#,
        );
        assert_eq!(theme.font(FontSlot::Major), "Georgia");
        assert_eq!(theme.font(FontSlot::Minor), "Verdana");
    }

    #[test]
    fn a_theme_says_what_it_is_called() {
        assert_eq!(theme_from("").name, "Test");
    }

    #[test]
    fn both_vocabularies_name_the_same_slots() {
        assert_eq!(Slot::from_word("text1"), Slot::from_word("dark1"));
        assert_eq!(Slot::from_word("background1"), Slot::from_word("light1"));
        assert_eq!(Slot::from_word("accent3"), Some(Slot::Accent3));
        assert_eq!(Slot::from_word("nothing at all"), None);
    }

    #[test]
    fn the_two_font_slots_are_told_apart_by_what_they_start_with() {
        assert_eq!(FontSlot::from_word("minorHAnsi"), Some(FontSlot::Minor));
        assert_eq!(FontSlot::from_word("majorBidi"), Some(FontSlot::Major));
        assert_eq!(FontSlot::from_word("Calibri"), None);
    }

    #[test]
    fn a_colour_with_no_tint_or_shade_is_itself() {
        let theme = Theme::default();
        let wanted = ThemeColor { slot: Slot::Accent1, tint: None, shade: None };
        assert_eq!(theme.resolve(&wanted), "4472C4");
    }

    #[test]
    fn a_tint_lightens_towards_white() {
        let theme = Theme::default();
        let full = ThemeColor { slot: Slot::Accent1, tint: Some(255), shade: None };
        assert_eq!(theme.resolve(&full), "4472C4", "a full tint changes nothing");

        let none = ThemeColor { slot: Slot::Accent1, tint: Some(0), shade: None };
        assert_eq!(theme.resolve(&none), "FFFFFF", "no tint at all is white");

        let half = ThemeColor { slot: Slot::Accent1, tint: Some(128), shade: None };
        let resolved = theme.resolve(&half);
        assert!(resolved != "4472C4" && resolved != "FFFFFF", "got {resolved}");
    }

    #[test]
    fn a_shade_darkens_towards_black() {
        let theme = Theme::default();
        let full = ThemeColor { slot: Slot::Accent1, tint: None, shade: Some(255) };
        assert_eq!(theme.resolve(&full), "4472C4", "a full shade changes nothing");

        let none = ThemeColor { slot: Slot::Accent1, tint: None, shade: Some(0) };
        assert_eq!(theme.resolve(&none), "000000", "no shade at all is black");
    }

    #[test]
    fn a_colour_that_is_not_six_digits_is_left_as_it_was() {
        let theme = Theme { colors: vec!["nonsense".to_owned(); 12], ..Theme::default() };
        let wanted = ThemeColor { slot: Slot::Accent1, tint: Some(128), shade: None };
        assert_eq!(theme.resolve(&wanted), "nonsense");
    }
}
