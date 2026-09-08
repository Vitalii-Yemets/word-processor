//! Drawing text through a transform, which is how a watermark is turned.

use wp_layout::{FontLibrary, LayoutEngine, Renderer};
use wp_raster::{Canvas, Color, Transform};

/// The fonts, read once per test.
///
/// Leaked rather than kept: a `FontLibrary` caches lazily and so is not
/// shareable between threads, and the tests run on several. There is nothing
/// to free at the end of a test binary anyway.
fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// Where the ink is: the smallest box holding every pixel that is not the
/// background, as left, top, right, bottom.
fn ink(canvas: &Canvas) -> Option<(usize, usize, usize, usize)> {
    let (mut left, mut top, mut right, mut bottom) = (usize::MAX, usize::MAX, 0usize, 0usize);
    let mut found = false;
    for y in 0..canvas.height() {
        for x in 0..canvas.width() {
            if canvas.pixel(x, y) != Color::WHITE {
                found = true;
                left = left.min(x);
                top = top.min(y);
                right = right.max(x);
                bottom = bottom.max(y);
            }
        }
    }
    found.then_some((left, top, right, bottom))
}

/// Draws "Hello" onto a white canvas through a transform.
fn drawn(transform: &Transform) -> Canvas {
    let library = library();
    let mut engine = LayoutEngine::new(library);
    let mut renderer = Renderer::new(library);
    let line = engine.simple_line("Hello", 0.0, 0.0, 40.0, Color::BLACK);

    let mut canvas = Canvas::filled(400, 400, Color::WHITE);
    renderer.draw_transformed(&mut canvas, &line, transform);
    canvas
}

#[test]
fn text_drawn_through_a_move_lands_where_it_was_moved_to() {
    let canvas = drawn(&Transform::translate(100.0, 200.0));
    let Some((left, top, right, bottom)) = ink(&canvas) else {
        panic!("nothing was drawn — is there a font on this machine?");
    };
    assert!(left >= 95, "the ink starts at {left}, not near 100");
    // The baseline is at 200, so the letters sit above it.
    assert!(top < 200 && bottom <= 210, "the ink runs from {top} to {bottom}");
    assert!(right > left, "the ink has no width");
}

#[test]
fn a_quarter_turn_makes_the_text_taller_than_it_is_wide() {
    let flat = drawn(&Transform::translate(100.0, 200.0));
    let turned = drawn(&Transform::translate(100.0, 200.0).then(&Transform::rotate_about(
        core::f32::consts::FRAC_PI_2,
        200.0,
        200.0,
    )));

    let (flat_left, flat_top, flat_right, flat_bottom) = ink(&flat).expect("flat ink");
    let (turned_left, turned_top, turned_right, turned_bottom) = ink(&turned).expect("turned ink");

    assert!(flat_right - flat_left > flat_bottom - flat_top, "flat text is wider than it is tall");
    assert!(
        turned_bottom - turned_top > turned_right - turned_left,
        "turned text should be taller than it is wide"
    );
}

#[test]
fn a_turn_about_a_point_keeps_the_ink_on_the_canvas() {
    // What a diagonal watermark does: turned an eighth of a circle about the
    // middle of the page.
    let turned = drawn(&Transform::translate(150.0, 200.0).then(&Transform::rotate_about(
        -core::f32::consts::FRAC_PI_4,
        200.0,
        200.0,
    )));
    let (left, top, right, bottom) = ink(&turned).expect("turned ink");
    assert!(right < 400 && bottom < 400, "the ink ran off the canvas");
    assert!(left > 0 && top > 0, "the ink ran off the top or the left");
    // Turned up to the right, so it is neither flat nor upright.
    assert!(right - left > 10 && bottom - top > 10, "a diagonal has both width and height");
}

#[test]
fn a_full_turn_draws_the_same_thing_as_no_turn_at_all() {
    let flat = drawn(&Transform::translate(150.0, 200.0));
    let turned = drawn(&Transform::translate(150.0, 200.0).then(&Transform::rotate_about(
        core::f32::consts::TAU,
        200.0,
        200.0,
    )));
    let flat_ink = ink(&flat).expect("flat ink");
    let turned_ink = ink(&turned).expect("turned ink");

    // Within a pixel: a full turn is exact in theory and not in floating point.
    let near = |left: usize, right: usize| left.abs_diff(right) <= 1;
    assert!(near(flat_ink.0, turned_ink.0), "{flat_ink:?} against {turned_ink:?}");
    assert!(near(flat_ink.1, turned_ink.1), "{flat_ink:?} against {turned_ink:?}");
    assert!(near(flat_ink.2, turned_ink.2), "{flat_ink:?} against {turned_ink:?}");
    assert!(near(flat_ink.3, turned_ink.3), "{flat_ink:?} against {turned_ink:?}");
}
