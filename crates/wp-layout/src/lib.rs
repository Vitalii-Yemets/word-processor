//! From a document to pixels: finding fonts, laying text out, drawing it.
//!
//! This is the layer that turns the structure the document model describes into
//! something a person can look at. It sits above the font parser and the
//! rasterizer and below whatever presents the result — a window, a printer, or
//! an image file.
//!
//! # Example
//!
//! ```no_run
//! use wp_docx::Document;
//! use wp_layout::{FontLibrary, LayoutEngine, Renderer};
//! use wp_raster::Color;
//!
//! let bytes = std::fs::read("document.docx")?;
//! let document = Document::open(&bytes)?;
//!
//! let library = FontLibrary::scan_system();
//! let pages = LayoutEngine::new(&library).layout_document(&document);
//!
//! let canvas = Renderer::new(&library).render(&pages[0], Color::WHITE);
//! std::fs::write("page1.png", wp_raster::encode_png(&canvas))?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]

mod layout;
mod library;
mod render;

pub use layout::{Decoration, LayoutEngine, Page, PageMetrics, PositionedGlyph};
pub use library::{Face, FontLibrary};
pub use render::Renderer;
