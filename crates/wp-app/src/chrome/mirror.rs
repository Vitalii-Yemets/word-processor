//! Which way round the window is, for the parts of it that are asked
//! where something was drawn.
//!
//! # Why this is not a field on every widget
//!
//! Because the answer is the same for all of them at once. A window read
//! right to left is drawn turned about by the canvas — see
//! [`wp_raster::Canvas::set_mirror`] — so nothing that draws a button has
//! to know which way the window reads. But a button that has been drawn
//! has to be findable again under the pointer, and the pointer's position
//! comes from the desktop in the window's own coordinates, which do not
//! turn. So the one fact "the window is turned, and this wide" is kept
//! here, where the drawing put it, and every widget turns the point it is
//! asked about before looking for what is under it.
//!
//! The alternative — handing the width to every widget as it is made —
//! was tried on paper and thrown away: a widget made in one place and
//! asked in another would be a widget somebody forgets to tell.

use std::cell::Cell;

thread_local! {
    /// The window's width, where the window is turned about; nothing
    /// where it is read the usual way.
    static ABOUT: Cell<Option<f32>> = const { Cell::new(None) };
}

/// Says which way the window is drawn. Called once, where the window is
/// painted, before anything is drawn into it.
pub fn set(width: Option<f32>) {
    ABOUT.with(|slot| slot.set(width.filter(|width| width.is_finite() && *width > 0.0)));
}

/// A point of the window's, where the drawing put it.
///
/// Its own inverse, and nothing at all in a window read the usual way.
#[must_use]
pub fn flip(x: i32) -> i32 {
    match ABOUT.with(Cell::get) {
        Some(width) => width.round() as i32 - x,
        None => x,
    }
}

/// The same, for a measurement rather than a whole pixel.
#[must_use]
pub fn flip_f(x: f32) -> f32 {
    match ABOUT.with(Cell::get) {
        Some(width) => width - x,
        None => x,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_read_the_usual_way_turns_nothing() {
        set(None);
        assert_eq!(flip(10), 10);
    }

    #[test]
    fn a_turned_window_gives_back_the_other_side_and_is_its_own_inverse() {
        set(Some(1000.0));
        assert_eq!(flip(10), 990);
        assert_eq!(flip(flip(10)), 10);
        set(None);
    }
}
