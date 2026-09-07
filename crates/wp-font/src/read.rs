//! Reading big-endian numbers out of a font file.
//!
//! Every read is bounds-checked. A font file is data from outside the program —
//! it can be truncated, corrupt, or deliberately malformed — and an offset in it
//! points wherever it likes. Nothing here may panic on bad input.

use crate::Error;

/// A cursor over a font file.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    /// A cursor starting at an absolute offset.
    pub(crate) fn at(data: &'a [u8], offset: usize) -> Result<Self, Error> {
        if offset > data.len() {
            return Err(Error::OutOfBounds);
        }
        Ok(Self { data, offset })
    }

    pub(crate) fn position(&self) -> usize {
        self.offset
    }

    pub(crate) fn seek(&mut self, offset: usize) -> Result<(), Error> {
        if offset > self.data.len() {
            return Err(Error::OutOfBounds);
        }
        self.offset = offset;
        Ok(())
    }

    pub(crate) fn skip(&mut self, count: usize) -> Result<(), Error> {
        let next = self.offset.checked_add(count).ok_or(Error::OutOfBounds)?;
        self.seek(next)
    }

    pub(crate) fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.offset)
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], Error> {
        let end = self.offset.checked_add(count).ok_or(Error::OutOfBounds)?;
        let slice = self.data.get(self.offset..end).ok_or(Error::OutOfBounds)?;
        self.offset = end;
        Ok(slice)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn i8(&mut self) -> Result<i8, Error> {
        Ok(self.u8()? as i8)
    }

    pub(crate) fn u16(&mut self) -> Result<u16, Error> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    pub(crate) fn i16(&mut self) -> Result<i16, Error> {
        Ok(self.u16()? as i16)
    }

    pub(crate) fn u32(&mut self) -> Result<u32, Error> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// A four-character table tag, such as `glyf`.
    pub(crate) fn tag(&mut self) -> Result<[u8; 4], Error> {
        let bytes = self.take(4)?;
        Ok([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    /// A 2.14 fixed-point number, used by composite glyph transforms.
    pub(crate) fn f2dot14(&mut self) -> Result<f32, Error> {
        Ok(f32::from(self.i16()?) / 16384.0)
    }

    pub(crate) fn bytes(&mut self, count: usize) -> Result<&'a [u8], Error> {
        self.take(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_big_endian_values() {
        let data = [0x00, 0x01, 0xFF, 0xFE, 0x12, 0x34, 0x56, 0x78];
        let mut reader = Reader::new(&data);

        assert_eq!(reader.u16().unwrap(), 1);
        assert_eq!(reader.i16().unwrap(), -2);
        assert_eq!(reader.u32().unwrap(), 0x1234_5678);
    }

    #[test]
    fn reading_past_the_end_is_an_error_not_a_panic() {
        // A font file can be truncated, and its internal offsets can point
        // anywhere at all.
        let data = [0x00, 0x01];
        let mut reader = Reader::new(&data);

        assert!(reader.u32().is_err());
        assert!(Reader::at(&data, 99).is_err());
    }

    #[test]
    fn f2dot14_covers_the_range_composite_glyphs_use() {
        let data = [0x40, 0x00, 0xC0, 0x00, 0x00, 0x00];
        let mut reader = Reader::new(&data);

        assert_eq!(reader.f2dot14().unwrap(), 1.0);
        assert_eq!(reader.f2dot14().unwrap(), -1.0);
        assert_eq!(reader.f2dot14().unwrap(), 0.0);
    }
}
