//! DEFLATE compression and checksums — the foundation for reading and writing `.docx`.
//!
//! A `.docx` file is a ZIP container (OPC, ECMA-376 Part 2), and ZIP stores its
//! entries using DEFLATE (RFC 1951). Every other layer of this project sits on
//! top of this one, so it is written first and depends on nothing but `std`.
//!
//! Implemented strictly against the specifications:
//!   * RFC 1950 — zlib format (also required for PNG);
//!   * RFC 1951 — DEFLATE format;
//!   * ISO 3309 / ITU-T V.42 — CRC-32 with polynomial 0xEDB88320.
//!
//! # Example
//!
//! ```
//! use wp_deflate::{compress, inflate};
//!
//! let original = b"<w:p><w:r><w:t>Hello</w:t></w:r></w:p>";
//! let packed = compress(original);
//! assert_eq!(inflate(&packed).unwrap(), original);
//! ```

#![forbid(unsafe_code)]

pub mod adler32;
pub mod crc32;
pub mod deflate;
pub mod inflate;

mod tables;

pub use adler32::{adler32, Adler32};
pub use crc32::{crc32, Crc32};
pub use deflate::{compress, compress_stored, compress_zlib};
pub use inflate::{inflate, inflate_limited, inflate_zlib, Error};
