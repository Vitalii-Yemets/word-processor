//! What a shape is drawn with besides its fill and its line: `a:effectLst`.
//!
//! The shadow under it, the shadow inside it, the glow round it, the soft edge
//! that fades it out, and the reflection beneath it. These are effects on a
//! *drawing* and live in the drawing namespace beside the fill; the ones behind
//! Word's Text Effects button are a different thing in a different namespace
//! and are [`crate::effects`].
//!
//! # The units
//!
//! The format's own, kept as the format states them. A distance or a blur is in
//! English metric units, an angle in sixtieths of a degree measured clockwise
//! from three o'clock, and an amount — how solid a colour is, how far down a
//! reflection has faded — is in hundred-thousandths. Nothing is turned into
//! pixels here: how big a pixel is depends on the zoom, and a document read at
//! one zoom and written at another would come out with different numbers in it.

use wp_xml::tree::Element;

/// Everything `a:effectLst` can say about a shape.
///
/// All of it is optional and most shapes have none of it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Effects {
    /// The shadow cast behind the shape.
    pub outer_shadow: Option<Shadow>,
    /// And the one cast inside it, which makes the shape look like a hole.
    pub inner_shadow: Option<Shadow>,
    pub glow: Option<Glow>,
    /// How far in from its edge the shape fades away, in English metric units.
    pub soft_edge_emu: i64,
    pub reflection: Option<Reflection>,
}

impl Effects {
    /// Whether there is anything here at all.
    #[must_use]
    pub fn is_nothing(&self) -> bool {
        self.outer_shadow.is_none()
            && self.inner_shadow.is_none()
            && self.glow.is_none()
            && self.soft_edge_emu == 0
            && self.reflection.is_none()
    }
}

/// A shadow, inside the shape or outside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shadow {
    /// Six hex digits, as everything else here says a colour.
    pub colour: String,
    /// How solid that colour is, in hundred-thousandths: Word's own shadow is
    /// 40,000, which is to say two fifths.
    pub alpha: i32,
    pub blur_emu: i64,
    /// How far the shadow falls, and which way.
    pub distance_emu: i64,
    pub direction: i32,
}

/// A glow: a colour spreading out of the shape's edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Glow {
    pub colour: String,
    pub alpha: i32,
    pub radius_emu: i64,
}

/// A reflection: the shape again, upside down and fading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reflection {
    pub blur_emu: i64,
    /// How solid it starts and how solid it ends, in hundred-thousandths, and
    /// how far down it has faded to the second of those.
    pub start_alpha: i32,
    pub end_alpha: i32,
    pub end_at: i32,
    /// How far below the shape it starts.
    pub distance_emu: i64,
}

/// What a shape's properties say it is drawn with.
#[must_use]
pub fn read_effects(properties: &Element) -> Effects {
    let Some(list) = child(properties, "effectLst") else {
        return Effects::default();
    };
    Effects {
        outer_shadow: child(list, "outerShdw").map(read_shadow),
        inner_shadow: child(list, "innerShdw").map(read_shadow),
        glow: child(list, "glow").map(|glow| Glow {
            colour: colour_of(glow).unwrap_or_else(|| "000000".to_owned()),
            alpha: alpha_of(glow),
            radius_emu: number(glow, "rad"),
        }),
        soft_edge_emu: child(list, "softEdge").map_or(0, |edge| number(edge, "rad")),
        reflection: child(list, "reflection").map(|reflection| Reflection {
            blur_emu: number(reflection, "blurRad"),
            start_alpha: whole(reflection, "stA", 50_000),
            end_alpha: whole(reflection, "endA", 300),
            end_at: whole(reflection, "endPos", 35_000),
            distance_emu: number(reflection, "dist"),
        }),
    }
}

fn read_shadow(element: &Element) -> Shadow {
    Shadow {
        colour: colour_of(element).unwrap_or_else(|| "000000".to_owned()),
        alpha: alpha_of(element),
        blur_emu: number(element, "blurRad"),
        distance_emu: number(element, "dist"),
        direction: whole(element, "dir", 0),
    }
}

/// And the same written back out, or nothing when there is nothing to write.
///
/// An empty `a:effectLst` and none at all are the same thing to every reader,
/// and Word writes neither for a shape with no effects on it.
#[must_use]
pub fn effects_element(effects: &Effects) -> Option<Element> {
    if effects.is_nothing() {
        return None;
    }
    let mut list = Element::new("a:effectLst", Some(crate::shapes::A));
    if let Some(shadow) = &effects.outer_shadow {
        list.push_element(shadow_element("a:outerShdw", shadow));
    }
    if let Some(shadow) = &effects.inner_shadow {
        list.push_element(shadow_element("a:innerShdw", shadow));
    }
    if let Some(glow) = &effects.glow {
        let mut element = Element::new("a:glow", Some(crate::shapes::A));
        element.set_attribute("rad", &glow.radius_emu.to_string());
        element.push_element(colour_element(&glow.colour, glow.alpha));
        list.push_element(element);
    }
    if effects.soft_edge_emu > 0 {
        let mut element = Element::new("a:softEdge", Some(crate::shapes::A));
        element.set_attribute("rad", &effects.soft_edge_emu.to_string());
        list.push_element(element);
    }
    if let Some(reflection) = &effects.reflection {
        let mut element = Element::new("a:reflection", Some(crate::shapes::A));
        element.set_attribute("blurRad", &reflection.blur_emu.to_string());
        element.set_attribute("stA", &reflection.start_alpha.to_string());
        element.set_attribute("endA", &reflection.end_alpha.to_string());
        element.set_attribute("endPos", &reflection.end_at.to_string());
        element.set_attribute("dist", &reflection.distance_emu.to_string());
        // Straight down, which is where a reflection goes: the format says so
        // as an angle like every other direction.
        element.set_attribute("dir", "5400000");
        element.set_attribute("sy", "-100000");
        element.set_attribute("algn", "bl");
        list.push_element(element);
    }
    Some(list)
}

fn shadow_element(name: &str, shadow: &Shadow) -> Element {
    let mut element = Element::new(name, Some(crate::shapes::A));
    element.set_attribute("blurRad", &shadow.blur_emu.to_string());
    element.set_attribute("dist", &shadow.distance_emu.to_string());
    element.set_attribute("dir", &shadow.direction.to_string());
    element.push_element(colour_element(&shadow.colour, shadow.alpha));
    element
}

/// A colour with an amount of it, which is how the format says "black at two
/// fifths".
fn colour_element(colour: &str, alpha: i32) -> Element {
    let mut element = Element::new("a:srgbClr", Some(crate::shapes::A));
    element.set_attribute("val", colour);
    if alpha < 100_000 {
        let mut amount = Element::new("a:alpha", Some(crate::shapes::A));
        amount.set_attribute("val", &alpha.to_string());
        element.push_element(amount);
    }
    element
}

fn child<'a>(parent: &'a Element, local: &str) -> Option<&'a Element> {
    parent.child_elements().find(|child| child.local_name() == local)
}

fn colour_of(parent: &Element) -> Option<String> {
    child(parent, "srgbClr")?.attribute_by_name("val").map(str::to_uppercase)
}

/// How solid the colour is, or solid when nothing says otherwise.
fn alpha_of(parent: &Element) -> i32 {
    child(parent, "srgbClr")
        .and_then(|colour| child(colour, "alpha"))
        .and_then(|alpha| alpha.attribute_by_name("val"))
        .and_then(|value| value.parse().ok())
        .unwrap_or(100_000)
}

fn number(element: &Element, name: &str) -> i64 {
    element.attribute_by_name(name).and_then(|value| value.parse().ok()).unwrap_or(0)
}

fn whole(element: &Element, name: &str, fallback: i32) -> i32 {
    element.attribute_by_name(name).and_then(|value| value.parse().ok()).unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shape_with_no_effects_writes_none() {
        assert!(effects_element(&Effects::default()).is_none());
    }

    #[test]
    fn a_colour_that_says_nothing_about_its_amount_is_solid() {
        let mut colour = Element::new("a:srgbClr", Some(crate::shapes::A));
        colour.set_attribute("val", "FF0000");
        let mut shadow = Element::new("a:outerShdw", Some(crate::shapes::A));
        shadow.push_element(colour);
        assert_eq!(alpha_of(&shadow), 100_000);
    }
}
