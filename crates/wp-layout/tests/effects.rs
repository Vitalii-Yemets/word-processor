//! Text effects, as pixels rather than as markup.
//!
//! The document side of this is tested where it is written; what these ask is
//! whether anything actually appears on the page, and in the right place. An
//! effect that is stored perfectly and drawn nowhere is not a feature.

use wp_docx::effects::Effect;
use wp_layout::{FontLibrary, GlyphEffect, LayoutEngine, Renderer};
use wp_raster::{Canvas, Color};

/// The fonts, read once per test. Leaked because a library caches lazily and
/// the tests run on several threads; there is nothing to free at the end of a
/// test binary anyway.
fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// Draws one word, with the effect given to every glyph of it.
fn drawn(effect: Option<GlyphEffect>) -> Canvas {
    let library = library();
    let mut engine = LayoutEngine::new(library);
    let mut renderer = Renderer::new(library);

    let mut line = engine.simple_line("Ha", 60.0, 120.0, 40.0, Color::BLACK);
    for glyph in &mut line.glyphs {
        glyph.effect = effect;
    }

    let mut canvas = Canvas::filled(300, 240, Color::WHITE);
    renderer.draw_onto(&mut canvas, &line, 0.0, 0.0);
    canvas
}

/// How many pixels are not the paper.
fn inked(canvas: &Canvas) -> usize {
    let mut count = 0;
    for y in 0..canvas.height() {
        for x in 0..canvas.width() {
            if canvas.pixel(x, y) != Color::WHITE {
                count += 1;
            }
        }
    }
    count
}

/// The lowest row holding anything.
fn lowest_ink(canvas: &Canvas) -> Option<usize> {
    (0..canvas.height())
        .rev()
        .find(|y| (0..canvas.width()).any(|x| canvas.pixel(x, *y) != Color::WHITE))
}

fn effect(kind: Effect) -> Option<GlyphEffect> {
    Some(GlyphEffect { kind, color: Color::rgb(0, 176, 240) })
}

#[test]
fn plain_text_is_drawn_with_nothing_round_it() {
    assert!(inked(&drawn(None)) > 0, "the test needs some text to begin with");
}

#[test]
fn every_effect_puts_more_ink_on_the_page_than_none_at_all() {
    let plain = inked(&drawn(None));
    for kind in Effect::DRAWN {
        let with = inked(&drawn(effect(*kind)));
        assert!(with > plain, "{} drew {with} pixels against {plain} plain", kind.label());
    }
}

#[test]
fn no_effect_at_all_draws_exactly_what_plain_text_draws() {
    let plain = inked(&drawn(None));
    assert_eq!(inked(&drawn(effect(Effect::None))), plain);
}

#[test]
fn a_glow_spreads_further_than_an_outline() {
    let outline = inked(&drawn(effect(Effect::Outline)));
    let glow = inked(&drawn(effect(Effect::Glow)));
    assert!(glow > outline, "a glow of {glow} pixels against an outline of {outline}");
}

#[test]
fn a_reflection_is_drawn_below_the_letters() {
    let plain = lowest_ink(&drawn(None)).expect("some text");
    let reflected = lowest_ink(&drawn(effect(Effect::Reflection))).expect("some text");
    assert!(reflected > plain, "the reflection ended at {reflected}, the text at {plain}");
}

#[test]
fn the_letters_themselves_are_drawn_over_the_effect() {
    // The middle of a letter must still be the text's colour, not the glow's:
    // an effect drawn on top of the text would hide it.
    let canvas = drawn(effect(Effect::Glow));
    let black = (0..canvas.height())
        .flat_map(|y| (0..canvas.width()).map(move |x| (x, y)))
        .filter(|(x, y)| canvas.pixel(*x, *y) == Color::BLACK)
        .count();
    assert!(black > 0, "the text was buried under its own glow");
}
