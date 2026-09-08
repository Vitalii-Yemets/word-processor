//! The drawings on the buttons.
//!
//! # Where they come from
//!
//! They are **Fluent UI System Icons**, which Microsoft publishes under the MIT
//! licence, built into the program from `crates/wp-app/assets/icons`. That set
//! is the icon language Office and Windows are drawn in, so using it gets the
//! look this program is trying to match, drawn by the people who designed it.
//!
//! Word's *own* button art is a different thing: it is Microsoft's product
//! artwork, not licensed for reuse, and it is not what is here.
//!
//! An earlier version of this file drew every icon by hand out of rectangles.
//! At sixteen pixels that is a losing game — a hand-drawn diagonal is a
//! staircase, a hand-drawn curve is a smudge, and a hundred and seventy of them
//! drawn by one person in one afternoon do not look like a set.
//!
//! # How they are drawn
//!
//! Each file is an outline in a twenty- or twenty-four-unit box. `wp-svg` reads
//! it into a path, which goes to the same rasterizer that draws the letters of
//! the document — so an icon is anti-aliased exactly as text is, and scales to
//! any size without falling apart.
//!
//! Reading is done once per icon and kept, because a repaint draws dozens of
//! them and parsing on every frame would show.
//!
//! Where a letter *is* the meaning — B for bold, I for italic — the letter is
//! drawn instead, set in the very formatting the button applies. That needs no
//! translating, which matters for a program meant to be used in every language
//! Word supports.

use std::cell::RefCell;

use wp_raster::{Canvas, Color};
use wp_svg::Drawing;

#[cfg(test)]
use super::icon_catalogue::ALL;
pub use super::icon_catalogue::{Icon, COUNT};

/// The size a small icon is drawn at, in pixels.
///
/// Twenty rather than sixteen: it is the size the small drawings are composed
/// at, so every edge the designer put on a pixel boundary lands on one.
pub const SIZE: f32 = 20.0;

/// The size on the tall button at the front of a group.
pub const LARGE_SIZE: f32 = 32.0;

thread_local! {
    /// The drawings read so far, by icon and by size.
    ///
    /// A repaint draws dozens of icons and there are only a hundred and seventy
    /// of them, so each is read once and kept for the life of the process.
    /// Thread-local because the window is drawn on one thread, and that saves
    /// taking a lock for every button.
    static CACHE: RefCell<Vec<[Option<Drawing>; 2]>> =
        RefCell::new((0..COUNT).map(|_| [None, None]).collect());
}

/// Draws an icon with its top-left corner at a point, at the small size.
pub fn draw(canvas: &mut Canvas, icon: Icon, x: f32, y: f32, color: Color) {
    draw_sized(canvas, icon, x, y, SIZE, color);
}

/// Draws an icon at a size of your choosing.
pub fn draw_sized(canvas: &mut Canvas, icon: Icon, x: f32, y: f32, size: f32, color: Color) {
    if icon == Icon::None || size <= 0.0 {
        return;
    }
    // The larger art is used from the point where its extra detail can show.
    let large = size > SIZE * 1.2;

    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let slot = &mut cache[icon as usize][usize::from(large)];
        if slot.is_none() {
            // A drawing that will not read is left out rather than bringing the
            // window down: a missing icon is a blemish, not a failure.
            *slot = Drawing::parse(icon.source(large)).ok();
        }
        let Some(drawing) = slot.as_ref() else { return };

        for shape in drawing.placed(x, y, size) {
            canvas.fill_path(&shape.outline, color);
        }
    });
}

/// A band of colour under an icon, for the ones that carry one.
///
/// The highlight and font-colour buttons show what they would apply, which is
/// the only way to tell what pressing them will do without opening anything.
pub fn draw_color_band(canvas: &mut Canvas, x: f32, y: f32, size: f32, color: Color) {
    let thickness = (size / 5.0).round().max(2.0);
    canvas.fill_rect(x as i32, (y + size - thickness) as i32, size as i32, thickness as i32, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every drawing named in the catalogue has to read, at both sizes.
    ///
    /// Without this a mistyped name in `tools/icons.list` leaves one button
    /// blank, and nobody finds out until they open that tab.
    #[test]
    fn every_drawing_reads_at_both_sizes() {
        for icon in ALL.iter().copied().filter(|icon| *icon != Icon::None) {
            for large in [false, true] {
                let source = icon.source(large);
                assert!(!source.is_empty(), "{icon:?} has no file at large={large}");
                let drawing = Drawing::parse(source)
                    .unwrap_or_else(|error| panic!("{icon:?} at large={large}: {error}"));
                assert!(!drawing.shapes.is_empty(), "{icon:?} at large={large} draws nothing");
            }
        }
    }

    /// The cache is indexed by `icon as usize`, so the enum has to be dense and
    /// in the same order as the array.
    #[test]
    fn the_array_position_is_the_enum_value() {
        for (index, icon) in ALL.iter().enumerate() {
            assert_eq!(*icon as usize, index, "{icon:?} is out of place");
        }
    }

    /// Nothing is drawn twice, which would mean two buttons look identical.
    ///
    /// Some sharing is deliberate — Word draws Header and Cover Page with the
    /// same motif — so this only reports how much of it there is, and fails if
    /// it grows past what was decided on.
    #[test]
    fn few_buttons_share_a_drawing() {
        let mut sources: Vec<&str> = ALL
            .iter()
            .copied()
            .filter(|icon| *icon != Icon::None)
            .map(|icon| icon.source(false))
            .collect();
        let total = sources.len();
        sources.sort_unstable();
        sources.dedup();
        let shared = total - sources.len();
        assert!(shared <= 20, "{shared} buttons share a drawing with another, which is too many");
    }

    #[test]
    fn drawing_paints_something_onto_the_canvas() {
        let mut canvas = Canvas::new(32, 32);
        canvas.clear(Color::WHITE);
        draw(&mut canvas, Icon::Save, 6.0, 6.0, Color::BLACK);
        let painted = (0..32)
            .flat_map(|y| (0..32).map(move |x| (x, y)))
            .filter(|(x, y)| canvas.pixel(*x, *y) != Color::WHITE)
            .count();
        assert!(painted > 40, "the save icon covered only {painted} pixels");
    }

    #[test]
    fn an_icon_of_none_draws_nothing() {
        let mut canvas = Canvas::new(24, 24);
        canvas.clear(Color::WHITE);
        draw(&mut canvas, Icon::None, 2.0, 2.0, Color::BLACK);
        assert!((0..24).all(|y| (0..24).all(|x| canvas.pixel(x, y) == Color::WHITE)));
    }
}
