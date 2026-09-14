//! Tests against a real variable font.
//!
//! One file that is a whole family: a set of axes, and deltas saying how every
//! point of every glyph moves as each is turned. The deltas are stored for
//! some points and worked out for the rest, and they belong to regions of the
//! axes rather than to the whole of them — so nothing here can be checked by
//! reading the table. It has to be set somewhere and measured.
//!
//! Inter is installed in the build container for this. It has a weight axis
//! running from Thin to Black and names nine places along it.

use wp_font::{Font, GlyphId, PathCommand};

const FONT_PATH: &str = "/usr/share/fonts/truetype/inter-vf/Inter-roman.var.ttf";

fn load() -> Vec<u8> {
    std::fs::read(FONT_PATH).unwrap_or_else(|error| {
        panic!(
            "cannot read {FONT_PATH}: {error}\n\
             the build image should install fonts-inter-variable; rebuild it with .\\x.ps1 image"
        )
    })
}

/// How much ink a glyph's outline encloses, near enough: the area of the
/// polygon through its points. Heavier letters enclose more.
fn ink(font: &Font<'_>, character: char) -> f32 {
    let glyph = font.glyph_for(character).expect("a letter the font has");
    let outline = font.outline(glyph).expect("an outline").expect("something drawn");

    let mut area = 0.0f32;
    let mut start = None;
    let mut last = None;
    for command in &outline.commands {
        let point = match command {
            PathCommand::MoveTo(point) => {
                start = Some(*point);
                last = Some(*point);
                continue;
            }
            PathCommand::LineTo(point) => *point,
            PathCommand::QuadTo(_, point) | PathCommand::CubicTo(_, _, point) => *point,
            PathCommand::Close => match start {
                Some(point) => point,
                None => continue,
            },
        };
        if let Some(previous) = last {
            area += previous.x * point.y - point.x * previous.y;
        }
        last = Some(point);
    }
    (area / 2.0).abs()
}

#[test]
fn a_variable_font_says_what_it_can_be_set_to() {
    let data = load();
    let font = Font::parse(&data).expect("a real font");

    assert!(font.is_variable(), "Inter is a variable font");
    let axes = font.axes();
    assert!(!axes.is_empty());

    let weight = axes.iter().find(|axis| axis.tag == *b"wght").expect("a weight axis");
    assert!(weight.min < weight.default && weight.default < weight.max);
    assert!(weight.min <= 100.0, "the light end goes at least to Thin");
    assert!(weight.max >= 700.0, "the heavy end goes at least to Bold");
}

#[test]
fn the_instances_are_the_weights_a_font_menu_would_list() {
    let data = load();
    let font = Font::parse(&data).unwrap();

    let instances = font.instances();
    assert!(instances.len() >= 5, "a weight axis is usually named at every stop");
    let named: Vec<&str> =
        instances.iter().filter_map(|instance| instance.name.as_deref()).collect();
    assert!(named.iter().any(|name| name.contains("Bold")), "{named:?}");
    assert!(named.iter().any(|name| name.contains("Thin") || name.contains("Light")), "{named:?}");

    // Every instance says where it sits, one number per axis.
    for instance in &instances {
        assert_eq!(instance.coordinates.len(), font.axes().len(), "{:?}", instance.name);
    }
}

#[test]
fn setting_the_weight_makes_the_letters_heavier() {
    // The whole of what a variable font is for. A reader that ignores the
    // deltas draws every one of these the same, and nothing says so.
    let data = load();
    let font = Font::parse(&data).unwrap();
    let axes = font.axes();
    let weight = axes.iter().position(|axis| axis.tag == *b"wght").expect("a weight axis");

    let at = |value: f32| {
        let mut coordinates: Vec<f32> = axes.iter().map(|axis| axis.default).collect();
        coordinates[weight] = value;
        font.varied(&coordinates)
    };

    let thin = ink(&at(axes[weight].min), 'n');
    let regular = ink(&font, 'n');
    let black = ink(&at(axes[weight].max), 'n');

    assert!(thin < regular, "Thin ({thin}) is not lighter than Regular ({regular})");
    assert!(regular < black, "Regular ({regular}) is not lighter than Black ({black})");
    // And by a long way: a weight axis is not a subtle thing.
    assert!(black > thin * 1.5, "Black ({black}) is barely heavier than Thin ({thin})");
}

#[test]
fn a_letter_does_not_come_apart_when_it_is_set() {
    // The deltas are stored for some points and worked out for the rest. Get
    // that wrong and a letter tears open at every point the font did not
    // bother to give a delta for — which shows up as an outline reaching far
    // outside where it says it does.
    let data = load();
    let font = Font::parse(&data).unwrap();
    let axes = font.axes();
    let coordinates: Vec<f32> = axes
        .iter()
        .map(|axis| if axis.tag == *b"wght" { axis.max } else { axis.default })
        .collect();
    let black = font.varied(&coordinates);
    let units = f32::from(font.units_per_em());

    for character in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".chars() {
        let glyph = black.glyph_for(character).expect("a letter");
        let outline = black.outline(glyph).unwrap().expect("an outline");
        assert!(
            f32::from(outline.bounds.max_y) < units * 1.5,
            "{character} reaches {} where the em is {units}",
            outline.bounds.max_y
        );
        assert!(f32::from(outline.bounds.min_y) > -units, "{character} falls off the bottom");
        assert!(
            f32::from(outline.bounds.max_x) < units * 2.0,
            "{character} reaches {} across",
            outline.bounds.max_x
        );
    }
}

#[test]
fn a_heavier_letter_is_a_wider_letter() {
    // The widths vary too, in a table of their own. Text set without them is
    // text set at the wrong width — every line breaks in the wrong place.
    let data = load();
    let font = Font::parse(&data).unwrap();
    let axes = font.axes();
    let weight = axes.iter().position(|axis| axis.tag == *b"wght").unwrap();

    let at = |value: f32| {
        let mut coordinates: Vec<f32> = axes.iter().map(|axis| axis.default).collect();
        coordinates[weight] = value;
        font.varied(&coordinates)
    };
    let glyph = font.glyph_for('m').unwrap();

    let thin = at(axes[weight].min).advance(glyph);
    let regular = font.advance(glyph);
    let black = at(axes[weight].max).advance(glyph);

    assert!(thin < regular, "Thin m is {thin}, Regular {regular}");
    assert!(regular < black, "Regular m is {regular}, Black {black}");
}

#[test]
fn an_ordinary_font_has_no_axes_and_is_not_changed_by_being_set() {
    let data = std::fs::read("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf").unwrap();
    let font = Font::parse(&data).unwrap();

    assert!(!font.is_variable());
    assert!(font.axes().is_empty());
    assert!(font.instances().is_empty());

    let set = font.varied(&[900.0]);
    assert!(!set.is_varied());
    let glyph = font.glyph_for('n').unwrap();
    assert_eq!(font.outline(glyph).unwrap(), set.outline(glyph).unwrap());
}

#[test]
fn a_font_left_where_it_was_drawn_is_the_font_it_was_drawn_as() {
    // Asking for the default instance must not send every glyph through the
    // deltas, and must give back exactly what the font holds.
    let data = load();
    let font = Font::parse(&data).unwrap();
    let defaults: Vec<f32> = font.axes().iter().map(|axis| axis.default).collect();
    let same = font.varied(&defaults);

    assert!(!same.is_varied());
    for character in "aeiouAEIOU".chars() {
        let glyph = font.glyph_for(character).unwrap();
        assert_eq!(font.outline(glyph).unwrap(), same.outline(glyph).unwrap());
    }
}

#[test]
fn every_glyph_can_be_drawn_at_every_named_instance() {
    let data = load();
    let font = Font::parse(&data).unwrap();

    for instance in font.instances() {
        let set = font.varied(&instance.coordinates);
        for id in 0..font.glyph_count() {
            set.outline(GlyphId(id))
                .unwrap_or_else(|error| panic!("glyph {id} at {:?}: {error}", instance.name));
        }
    }
}

#[test]
fn an_accented_letter_keeps_its_accent_over_the_letter() {
    // A composite glyph varies twice over: the letter and the accent each move
    // as the weight changes, and the composite's own deltas say where the
    // accent goes once they have. A reader that applies only the first leaves
    // the accent where it was drawn, sitting on top of a letter that grew.
    let data = load();
    let font = Font::parse(&data).unwrap();
    let axes = font.axes();
    let coordinates: Vec<f32> = axes
        .iter()
        .map(|axis| if axis.tag == *b"wght" { axis.max } else { axis.default })
        .collect();
    let black = font.varied(&coordinates);

    for character in "éàïôüñçÉÀÏÔÜÑÇ".chars() {
        let Some(glyph) = black.glyph_for(character) else { continue };
        let heavy = black.outline(glyph).unwrap().expect("an outline");
        let plain = font.outline(glyph).unwrap().expect("an outline");

        // The accent sits above the letter in both, and the heavy one is not
        // wildly taller: an accent left behind shows up as a glyph that is
        // either much taller or much shorter than it should be.
        let grew = f32::from(heavy.bounds.max_y) / f32::from(plain.bounds.max_y);
        assert!(
            (0.85..=1.2).contains(&grew),
            "{character} reaches {} where the plain one reaches {}",
            heavy.bounds.max_y,
            plain.bounds.max_y
        );
    }
}
