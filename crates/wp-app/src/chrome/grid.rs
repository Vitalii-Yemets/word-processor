//! The little grid that drops open under the Table button.
//!
//! Word asks for a table's size by letting you sweep a pointer across a grid of
//! squares, and says "4x3 Table" above it while you do. It is worth copying:
//! nobody has to think in numbers, and the answer is one press away.
//!
//! A dialog with two spin boxes would be more general and much worse.

use wp_layout::{LayoutEngine, Renderer};
use wp_raster::Canvas;

use super::theme::Theme;

/// How many columns the grid offers, which is what Word offers.
pub const COLUMNS: usize = 10;
/// How many rows.
pub const ROWS: usize = 8;

/// The side of one square, and the gap between two of them.
const CELL: f32 = 16.0;
const GAP: f32 = 2.0;
/// Room above the grid for the "4x3 Table" caption.
const CAPTION: f32 = 22.0;
const PADDING: f32 = 8.0;

/// The grid, and how much of it the pointer has swept over.
#[derive(Clone, Copy, Debug)]
pub struct TableGrid {
    left: f32,
    top: f32,
    /// How many rows and columns are lit, counting from one. `None` until the
    /// pointer is over the grid at all.
    chosen: Option<(usize, usize)>,
}

impl TableGrid {
    #[must_use]
    pub fn new(left: f32, top: f32) -> Self {
        Self { left, top, chosen: None }
    }

    #[must_use]
    pub fn width() -> f32 {
        COLUMNS as f32 * (CELL + GAP) - GAP + PADDING * 2.0
    }

    #[must_use]
    pub fn height() -> f32 {
        ROWS as f32 * (CELL + GAP) - GAP + PADDING * 2.0 + CAPTION
    }

    /// Whether a point is inside the whole panel, grid and caption alike.
    #[must_use]
    pub fn covers(&self, x: i32, y: i32) -> bool {
        let (x, y) = (x as f32, y as f32);
        x >= self.left
            && x < self.left + Self::width()
            && y >= self.top
            && y < self.top + Self::height()
    }

    /// How many rows and columns the point under the pointer stands for.
    #[must_use]
    pub fn hit(&self, x: i32, y: i32) -> Option<(usize, usize)> {
        let inside_x = x as f32 - self.left - PADDING;
        let inside_y = y as f32 - self.top - PADDING - CAPTION;
        if inside_x < 0.0 || inside_y < 0.0 {
            return None;
        }
        let column = (inside_x / (CELL + GAP)) as usize;
        let row = (inside_y / (CELL + GAP)) as usize;
        (column < COLUMNS && row < ROWS).then(|| (row + 1, column + 1))
    }

    /// Lights up as much of the grid as the pointer has reached. Returns
    /// whether that changed anything.
    pub fn hover(&mut self, x: i32, y: i32) -> bool {
        let found = self.hit(x, y);
        // Sweeping off the grid leaves the last size lit rather than going
        // blank, because a pointer that strays a pixel should not undo a
        // choice that is about to be pressed.
        let changed = found.is_some() && found != self.chosen;
        if found.is_some() {
            self.chosen = found;
        }
        changed
    }

    pub fn draw(
        &self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        theme: &Theme,
    ) {
        let (left, top) = (self.left as i32, self.top as i32);
        let (width, height) = (Self::width() as i32, Self::height() as i32);

        canvas.fill_rect(left, top, width, height, theme.field);
        outline(canvas, left, top, width, height, theme.field_edge);

        // The caption, which is what Word puts there: "4x3 Table", or an
        // instruction while nothing is chosen.
        let caption = match self.chosen {
            Some((rows, columns)) => format!("{columns}x{rows} Table"),
            None => "Insert Table".to_owned(),
        };
        let line = engine.simple_line(
            &caption,
            self.left + PADDING,
            self.top + CAPTION - 6.0,
            9.0,
            theme.text,
        );
        renderer.draw_onto(canvas, &line, 0.0, 0.0);

        for row in 0..ROWS {
            for column in 0..COLUMNS {
                let x = self.left + PADDING + column as f32 * (CELL + GAP);
                let y = self.top + PADDING + CAPTION + row as f32 * (CELL + GAP);
                let lit = self.chosen.is_some_and(|(rows, columns)| row < rows && column < columns);
                let fill = if lit { theme.accent } else { theme.hover };
                canvas.fill_rect(x as i32, y as i32, CELL as i32, CELL as i32, fill);
                outline(canvas, x as i32, y as i32, CELL as i32, CELL as i32, theme.field_edge);
            }
        }
    }
}

fn outline(canvas: &mut Canvas, x: i32, y: i32, width: i32, height: i32, colour: wp_raster::Color) {
    canvas.fill_rect(x, y, width, 1, colour);
    canvas.fill_rect(x, y + height - 1, width, 1, colour);
    canvas.fill_rect(x, y, 1, height, colour);
    canvas.fill_rect(x + width - 1, y, 1, height, colour);
}
