//! Text effects: shadow, outline, glow and reflection.
//!
//! # Why these live in a namespace of their own
//!
//! ECMA-376 has one text effect, `w:effect`, and it animates: blinking
//! backgrounds and marching ants, which Word stopped drawing years ago. The
//! effects behind Word's Text Effects button are newer than the standard, so
//! they are written in Microsoft's own namespace — `w14` — alongside the
//! standard properties inside `w:rPr`.
//!
//! That is allowed by markup compatibility: the root element lists `w14` as
//! ignorable, and a reader that has never heard of it skips those elements and
//! still reads the text. So a document written here opens in a reader that
//! knows nothing about effects, and opens in Word with the effects on. Both of
//! those matter, and only declaring the namespace properly gets both.
//!
//! # What is drawn and what is only stored
//!
//! All four are drawn, approximately: a shadow as an offset copy, an outline as
//! copies pushed out around the letter under the fill, a glow as fainter copies
//! further out, a reflection as a mirrored copy fading downwards. Word blurs its
//! shadow and its glow properly; this does not. The distances and angles Word
//! stores are written at their usual values rather than offered as settings,
//! because a person choosing "glow" is choosing a look, not a blur radius in
//! English metric units.

use wp_xml::tree::Element;

/// Microsoft's WordprocessingML extensions, where these elements live.
pub const W14: &str = "http://schemas.microsoft.com/office/word/2010/wordml";
/// Markup compatibility, which is what makes them ignorable.
pub const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
/// The prefix bound to the extension namespace in a document written here.
pub const W14_PREFIX: &str = "w14";

/// One of the looks the Text Effects button offers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Effect {
    /// Plain letters. First in the gallery, because taking an effect off is
    /// asked for as often as putting one on.
    #[default]
    None,
    Shadow,
    Outline,
    Glow,
    Reflection,
}

impl Effect {
    /// What the gallery offers, in the order it offers it.
    pub const CHOICES: &'static [Self] =
        &[Self::None, Self::Shadow, Self::Outline, Self::Glow, Self::Reflection];
    /// The ones that draw something.
    pub const DRAWN: &'static [Self] = &[Self::Shadow, Self::Outline, Self::Glow, Self::Reflection];

    /// What the element is called inside `w:rPr`, for the ones that have one.
    #[must_use]
    pub fn local_name(self) -> Option<&'static str> {
        match self {
            Self::None => Option::None,
            Self::Shadow => Some("shadow"),
            Self::Outline => Some("textOutline"),
            Self::Glow => Some("glow"),
            Self::Reflection => Some("reflection"),
        }
    }

    #[must_use]
    pub fn from_local_name(name: &str) -> Option<Self> {
        Self::DRAWN.iter().copied().find(|effect| effect.local_name() == Some(name))
    }

    /// What to call it in a menu.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "No effect",
            Self::Shadow => "Shadow",
            Self::Outline => "Outline",
            Self::Glow => "Glow",
            Self::Reflection => "Reflection",
        }
    }

    /// The colour it is drawn in when nobody says.
    ///
    /// A shadow is grey because a shadow is; an outline is black because it is
    /// a line round a letter; a glow is Word's own light blue, which is the
    /// colour its gallery glows in.
    #[must_use]
    pub fn default_color(self) -> Option<&'static str> {
        match self {
            Self::Shadow => Some("808080"),
            Self::Outline => Some("000000"),
            Self::Glow => Some("00B0F0"),
            // A reflection is the text upside down, so it takes the text's
            // colour and has none of its own.
            Self::None | Self::Reflection => Option::None,
        }
    }
}

/// An effect together with the colour it is drawn in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextEffect {
    pub effect: Effect,
    /// Six hex digits, or nothing for an effect that takes the text's colour.
    pub color: Option<String>,
}

impl TextEffect {
    /// The effect as the gallery offers it, in its usual colour.
    #[must_use]
    pub fn plain(effect: Effect) -> Self {
        Self { effect, color: effect.default_color().map(str::to_owned) }
    }
}

/// The effect a `w:rPr` names, if it names one.
#[must_use]
pub fn read_effect(properties: &Element) -> Option<TextEffect> {
    for child in properties.child_elements() {
        if child.namespace.as_deref() != Some(W14) {
            continue;
        }
        let Some(effect) = Effect::from_local_name(child.local_name()) else {
            continue;
        };
        return Some(TextEffect { effect, color: color_within(child) });
    }
    None
}

/// The `w14:srgbClr` inside an effect, wherever it is nested.
fn color_within(element: &Element) -> Option<String> {
    if let Some(value) =
        element.child(Some(W14), "srgbClr").and_then(|color| color.attribute(Some(W14), "val"))
    {
        return Some(value.to_owned());
    }
    element.child_elements().find_map(color_within)
}

/// Takes every effect off a `w:rPr`.
pub(crate) fn remove_effects(properties: &mut Element) {
    for effect in Effect::DRAWN {
        if let Some(local) = effect.local_name() {
            properties.remove_children_named(Some(W14), local);
        }
    }
}

/// Builds the element that says the effect, for the effects that are one.
#[must_use]
pub(crate) fn effect_element(wanted: &TextEffect, prefix: &str) -> Option<Element> {
    let local = wanted.effect.local_name()?;
    let named = |local: &str| format!("{prefix}:{local}");
    let mut element = Element::new(&named(local), Some(W14));
    let mut set = |name: &str, value: &str| {
        element.set_namespaced_attribute(&named(name), W14, value);
    };

    match wanted.effect {
        Effect::None => return None,
        Effect::Shadow => {
            // Down and to the right by three quarters of a point, which is
            // where Word's own shadow falls. Angles are in sixtieths of a
            // degree, so 2,700,000 is 45°.
            set("blurRad", "50800");
            set("dist", "38100");
            set("dir", "2700000");
            set("sx", "100000");
            set("sy", "100000");
            set("kx", "0");
            set("ky", "0");
            set("algn", "tl");
        }
        Effect::Outline => {
            // Three quarters of a point wide, in English metric units.
            set("w", "9525");
            set("cap", "flat");
            set("cmpd", "sng");
            set("algn", "ctr");
        }
        Effect::Glow => {
            // Five points of halo.
            set("rad", "63500");
        }
        Effect::Reflection => {
            // Straight down, starting a little over half opaque and fading to
            // nothing. The negative vertical scale is what turns it over.
            set("blurRad", "6350");
            set("stA", "55000");
            set("stPos", "0");
            set("endA", "300");
            set("endPos", "45500");
            set("dist", "0");
            set("dir", "5400000");
            set("fadeDir", "5400000");
            set("sx", "100000");
            set("sy", "-100000");
            set("kx", "0");
            set("ky", "0");
            set("algn", "bl");
        }
    }

    if let Some(color) = &wanted.color {
        // An outline is a line, so its colour is a fill inside the line; the
        // other two colour the effect directly.
        match wanted.effect {
            Effect::Outline => {
                let mut fill = Element::new(&named("solidFill"), Some(W14));
                fill.push_element(solid_color(color, prefix, None));
                element.push_element(fill);
                let mut dash = Element::new(&named("prstDash"), Some(W14));
                dash.set_namespaced_attribute(&named("val"), W14, "solid");
                element.push_element(dash);
                element.push_element(Element::new(&named("round"), Some(W14)));
            }
            Effect::Shadow | Effect::Glow => {
                element.push_element(solid_color(color, prefix, Some("60000")));
            }
            Effect::None | Effect::Reflection => {}
        }
    }

    Some(element)
}

/// A colour, and how much of it shows through.
fn solid_color(hex: &str, prefix: &str, alpha: Option<&str>) -> Element {
    let named = |local: &str| format!("{prefix}:{local}");
    let mut color = Element::new(&named("srgbClr"), Some(W14));
    color.set_namespaced_attribute(&named("val"), W14, hex);
    if let Some(alpha) = alpha {
        let mut element = Element::new(&named("alpha"), Some(W14));
        element.set_namespaced_attribute(&named("val"), W14, alpha);
        color.push_element(element);
    }
    color
}

/// Declares the extension namespace on the root, and says it may be ignored.
///
/// Without the second half a strict reader stops at an element it does not know
/// instead of skipping it, and the document fails to open in exactly the reader
/// this was supposed to be safe in.
pub(crate) fn declare_namespace(root: &mut Element) {
    if !root.declarations.iter().any(|(_, uri)| uri == W14) {
        root.declarations.push((Some(W14_PREFIX.to_owned()), W14.to_owned()));
    }
    if !root.declarations.iter().any(|(_, uri)| uri == MC) {
        root.declarations.push((Some("mc".to_owned()), MC.to_owned()));
    }

    let already = root.attribute(Some(MC), "Ignorable").unwrap_or_default().to_owned();
    if already.split_whitespace().any(|name| name == W14_PREFIX) {
        return;
    }
    let listed =
        if already.is_empty() { W14_PREFIX.to_owned() } else { format!("{already} {W14_PREFIX}") };
    root.set_namespaced_attribute("mc:Ignorable", MC, &listed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_drawn_effect_survives_being_named_and_read_back() {
        for effect in Effect::DRAWN {
            let local = effect.local_name().expect("a drawn effect has an element");
            assert_eq!(Effect::from_local_name(local), Some(*effect));
        }
    }

    #[test]
    fn no_effect_at_all_is_not_an_element() {
        assert_eq!(Effect::None.local_name(), None);
    }

    #[test]
    fn a_name_nobody_here_knows_is_not_an_effect() {
        assert_eq!(Effect::from_local_name("wobble"), None);
    }

    #[test]
    fn every_choice_says_what_it_is_called() {
        for effect in Effect::CHOICES {
            assert!(!effect.label().is_empty());
        }
    }

    #[test]
    fn the_gallery_offers_taking_the_effect_off_as_well_as_putting_one_on() {
        assert_eq!(Effect::CHOICES.len(), Effect::DRAWN.len() + 1);
        assert_eq!(Effect::CHOICES.first(), Some(&Effect::None));
    }

    #[test]
    fn an_effect_is_written_in_the_extension_namespace() {
        let element = effect_element(&TextEffect::plain(Effect::Glow), W14_PREFIX).expect("a glow");
        assert_eq!(element.namespace.as_deref(), Some(W14));
        assert_eq!(element.local_name(), "glow");
    }

    #[test]
    fn no_effect_writes_no_element() {
        assert!(effect_element(&TextEffect::plain(Effect::None), W14_PREFIX).is_none());
    }

    #[test]
    fn a_glow_says_how_wide_it_is() {
        let element = effect_element(&TextEffect::plain(Effect::Glow), W14_PREFIX).expect("a glow");
        assert_eq!(element.attribute(Some(W14), "rad"), Some("63500"));
    }

    #[test]
    fn an_outline_holds_its_colour_inside_a_fill() {
        let element =
            effect_element(&TextEffect::plain(Effect::Outline), W14_PREFIX).expect("an outline");
        let fill = element.child(Some(W14), "solidFill").expect("a fill");
        assert!(fill.child(Some(W14), "srgbClr").is_some());
    }

    #[test]
    fn a_reflection_has_no_colour_of_its_own() {
        let element = effect_element(&TextEffect::plain(Effect::Reflection), W14_PREFIX)
            .expect("a reflection");
        assert!(color_within(&element).is_none());
        // It is upside down, which is what the negative vertical scale says.
        assert_eq!(element.attribute(Some(W14), "sy"), Some("-100000"));
    }

    #[test]
    fn every_drawn_effect_reads_back_out_of_the_properties_it_was_written_into() {
        for effect in Effect::DRAWN {
            let wanted = TextEffect::plain(*effect);
            let mut properties = Element::new("w:rPr", Some(crate::read::W));
            properties.push_element(effect_element(&wanted, W14_PREFIX).expect("an element"));
            assert_eq!(read_effect(&properties), Some(wanted), "{}", effect.label());
        }
    }

    #[test]
    fn properties_saying_nothing_have_no_effect() {
        let properties = Element::new("w:rPr", Some(crate::read::W));
        assert_eq!(read_effect(&properties), None);
    }

    #[test]
    fn taking_the_effects_off_leaves_the_rest_of_the_properties() {
        let mut properties = Element::new("w:rPr", Some(crate::read::W));
        properties.push_element(
            effect_element(&TextEffect::plain(Effect::Shadow), W14_PREFIX).expect("a shadow"),
        );
        properties.push_element(Element::new("w:b", Some(crate::read::W)));

        remove_effects(&mut properties);
        assert_eq!(read_effect(&properties), None);
        assert!(properties.child(Some(crate::read::W), "b").is_some());
    }

    #[test]
    fn the_namespace_is_declared_and_marked_ignorable() {
        let mut root = Element::new("w:document", Some(crate::read::W));
        declare_namespace(&mut root);
        assert!(root.declarations.iter().any(|(_, uri)| uri == W14));
        assert_eq!(root.attribute(Some(MC), "Ignorable"), Some("w14"));
    }

    #[test]
    fn declaring_it_twice_says_it_once() {
        let mut root = Element::new("w:document", Some(crate::read::W));
        declare_namespace(&mut root);
        declare_namespace(&mut root);
        assert_eq!(root.declarations.iter().filter(|(_, uri)| uri == W14).count(), 1);
        assert_eq!(root.attribute(Some(MC), "Ignorable"), Some("w14"));
    }

    #[test]
    fn a_document_that_already_ignores_something_keeps_ignoring_it() {
        let mut root = Element::new("w:document", Some(crate::read::W));
        root.set_namespaced_attribute("mc:Ignorable", MC, "w15");
        declare_namespace(&mut root);
        assert_eq!(root.attribute(Some(MC), "Ignorable"), Some("w15 w14"));
    }
}
