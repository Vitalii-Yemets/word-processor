//! Drawing: paths, anti-aliased rasterization, a pixel canvas, and PNG output.
//!
//! This is the whole of the drawing stack, written from scratch. Nothing here
//! knows about documents or fonts — it fills shapes — which is what lets the
//! same code serve the screen, the printer and an exported image.
//!
//! # Example
//!
//! ```
//! use wp_raster::{Canvas, Color, Path};
//!
//! let mut canvas = Canvas::filled(64, 64, Color::WHITE);
//! canvas.fill_path(&Path::rectangle(8.0, 8.0, 48.0, 48.0), Color::rgb(0, 0, 200));
//!
//! let png = wp_raster::encode_png(&canvas);
//! assert_eq!(&png[1..4], b"PNG");
//! ```

#![forbid(unsafe_code)]

mod canvas;
mod path;
mod png;
mod raster;

pub use canvas::{Canvas, Color};
pub use path::{Command, Path, Point, Transform};
pub use png::encode as encode_png;
pub use raster::{Mask, Rasterizer};
