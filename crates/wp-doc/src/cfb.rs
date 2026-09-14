//! The compound file: a file system in a file.
//!
//! [MS-CFB]. Everything Office wrote before 2007 is one of these: a header,
//! a table of which sector follows which — the FAT, the same idea as a
//! floppy disk's — a directory of named streams, and the streams' bytes
//! scattered through the sectors in whatever order they were written. Small
//! streams are packed into one stream of their own and cut into sixty-four
//! byte mini-sectors with a FAT of their own, because a sector of five
//! hundred and twelve bytes is a lot for a stream of forty.
//!
//! Reading one is following chains: the header names the first sector of
//! the FAT's own directory (the DIFAT), the FAT names each stream's next
//! sector, the directory names each stream's first. This reads the whole
//! file into memory and follows them there, which is what a document is.

use crate::Error;

/// The end of a chain.
const END_OF_CHAIN: u32 = 0xFFFF_FFFE;
/// A sector nothing uses.
const FREE: u32 = 0xFFFF_FFFF;
/// Streams shorter than this live in the mini stream.
const MINI_CUTOFF: u32 = 4096;
const MINI_SECTOR: usize = 64;

/// An open compound file: its sectors, its FATs, and its directory.
#[derive(Debug)]
pub struct CompoundFile {
    bytes: Vec<u8>,
    sector_size: usize,
    fat: Vec<u32>,
    mini_fat: Vec<u32>,
    entries: Vec<Entry>,
    /// The mini stream, read whole: the root entry's stream.
    mini_stream: Vec<u8>,
}

/// One entry of the directory: a stream or a storage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    start: u32,
    size: u64,
    /// The entry's place in the tree: its left and right siblings and its
    /// child, or none.
    left: u32,
    right: u32,
    child: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Storage,
    Stream,
    Root,
    Unused,
}

impl CompoundFile {
    /// Opens a file, checking it is one and reading its tables.
    pub fn open(bytes: Vec<u8>) -> Result<Self, Error> {
        const SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
        if !bytes.starts_with(&SIGNATURE) {
            return Err(Error::NotCompound);
        }
        let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
        let u32_at = |at: usize| {
            u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
        };
        if bytes.len() < 512 {
            return Err(Error::Truncated("header"));
        }
        let sector_shift = u16_at(0x1E);
        let sector_size = match sector_shift {
            9 => 512,
            12 => 4096,
            _ => return Err(Error::Malformed("sector size")),
        };
        let fat_sectors = u32_at(0x2C) as usize;
        let first_directory = u32_at(0x30);
        let first_mini_fat = u32_at(0x3C);
        let mini_fat_sectors = u32_at(0x40) as usize;
        let first_difat = u32_at(0x44);
        let difat_sectors = u32_at(0x48) as usize;

        // The DIFAT: the first hundred and nine FAT sector numbers are in the
        // header, the rest in a chain of sectors of their own.
        let mut difat: Vec<u32> = (0..109).map(|index| u32_at(0x4C + index * 4)).collect();
        let mut next = first_difat;
        for _ in 0..difat_sectors {
            if next >= END_OF_CHAIN {
                break;
            }
            let sector = sector_bytes(&bytes, sector_size, next)?;
            let per_sector = sector_size / 4 - 1;
            for index in 0..per_sector {
                difat.push(u32::from_le_bytes([
                    sector[index * 4],
                    sector[index * 4 + 1],
                    sector[index * 4 + 2],
                    sector[index * 4 + 3],
                ]));
            }
            next = u32::from_le_bytes([
                sector[per_sector * 4],
                sector[per_sector * 4 + 1],
                sector[per_sector * 4 + 2],
                sector[per_sector * 4 + 3],
            ]);
        }

        // The FAT itself, sector by sector as the DIFAT names them.
        let mut fat = Vec::with_capacity(fat_sectors * sector_size / 4);
        for &number in difat.iter().take(fat_sectors) {
            if number >= END_OF_CHAIN {
                continue;
            }
            let sector = sector_bytes(&bytes, sector_size, number)?;
            fat.extend(
                sector
                    .chunks_exact(4)
                    .map(|four| u32::from_le_bytes([four[0], four[1], four[2], four[3]])),
            );
        }

        let mut file = Self {
            bytes,
            sector_size,
            fat,
            mini_fat: Vec::new(),
            entries: Vec::new(),
            mini_stream: Vec::new(),
        };

        // The directory: a chain of sectors of hundred-and-twenty-eight-byte
        // entries.
        let directory = file.chain(first_directory)?;
        for entry in directory.chunks_exact(128) {
            file.entries.push(parse_entry(entry));
        }
        if file.entries.is_empty() {
            return Err(Error::Malformed("directory"));
        }

        // The mini FAT and the mini stream, which is the root entry's.
        if mini_fat_sectors > 0 && first_mini_fat < END_OF_CHAIN {
            let mini = file.chain(first_mini_fat)?;
            file.mini_fat = mini
                .chunks_exact(4)
                .map(|four| u32::from_le_bytes([four[0], four[1], four[2], four[3]]))
                .collect();
        }
        let root = file.entries[0].clone();
        if root.size > 0 && root.start < END_OF_CHAIN {
            let mut stream = file.chain(root.start)?;
            stream.truncate(root.size as usize);
            file.mini_stream = stream;
        }
        Ok(file)
    }

    /// Every stream and storage, in directory order.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The bytes of a stream, by name. A name is looked for anywhere in the
    /// tree, because the streams a document needs are all at the top and a
    /// name does not repeat among them.
    #[must_use]
    pub fn stream(&self, name: &str) -> Option<Vec<u8>> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.kind == EntryKind::Stream && entry.name == name)?;
        self.read_entry(entry).ok()
    }

    /// The bytes of a stream entry: from the mini stream if it is short, from
    /// the sectors if it is not.
    fn read_entry(&self, entry: &Entry) -> Result<Vec<u8>, Error> {
        if entry.size == 0 {
            return Ok(Vec::new());
        }
        let size = entry.size as usize;
        if entry.size < u64::from(MINI_CUTOFF) {
            let mut out = Vec::with_capacity(size);
            let mut sector = entry.start;
            let mut guard = 0;
            while sector < END_OF_CHAIN && out.len() < size {
                let at = sector as usize * MINI_SECTOR;
                let piece = self
                    .mini_stream
                    .get(at..(at + MINI_SECTOR).min(self.mini_stream.len()))
                    .ok_or(Error::Truncated("mini stream"))?;
                out.extend_from_slice(piece);
                sector = *self.mini_fat.get(sector as usize).ok_or(Error::Malformed("mini FAT"))?;
                guard += 1;
                if guard > self.mini_fat.len() + 1 {
                    return Err(Error::Malformed("mini FAT chain"));
                }
            }
            out.truncate(size);
            return Ok(out);
        }
        let mut out = self.chain(entry.start)?;
        out.truncate(size);
        Ok(out)
    }

    /// Every sector of a chain, one after another.
    fn chain(&self, first: u32) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        let mut sector = first;
        let mut guard = 0;
        while sector < END_OF_CHAIN {
            out.extend_from_slice(sector_bytes(&self.bytes, self.sector_size, sector)?);
            sector = *self.fat.get(sector as usize).ok_or(Error::Malformed("FAT"))?;
            guard += 1;
            if guard > self.fat.len() + 1 {
                return Err(Error::Malformed("FAT chain"));
            }
        }
        Ok(out)
    }
}

/// The bytes of one sector. Sector zero begins after the header, which is
/// one sector long whatever the sector size.
fn sector_bytes(bytes: &[u8], sector_size: usize, number: u32) -> Result<&[u8], Error> {
    if number == FREE {
        return Err(Error::Malformed("free sector in a chain"));
    }
    let start = (number as usize + 1) * sector_size;
    bytes.get(start..start + sector_size).ok_or(Error::Truncated("sector"))
}

fn parse_entry(bytes: &[u8]) -> Entry {
    let name_length = usize::from(u16::from_le_bytes([bytes[64], bytes[65]]));
    let units: Vec<u16> = bytes[..name_length.min(64)]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    let name = String::from_utf16_lossy(&units);
    let kind = match bytes[66] {
        1 => EntryKind::Storage,
        2 => EntryKind::Stream,
        5 => EntryKind::Root,
        _ => EntryKind::Unused,
    };
    let u32_at =
        |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let size = u64::from_le_bytes([
        bytes[120], bytes[121], bytes[122], bytes[123], bytes[124], bytes[125], bytes[126],
        bytes[127],
    ]);
    Entry {
        name,
        kind,
        start: u32_at(116),
        // A version 3 file writes the high half as rubbish sometimes.
        size: size & 0xFFFF_FFFF,
        left: u32_at(68),
        right: u32_at(72),
        child: u32_at(76),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn something_that_is_not_a_compound_file_is_refused() {
        assert_eq!(CompoundFile::open(b"PK\x03\x04".to_vec()).err(), Some(Error::NotCompound));
        assert_eq!(CompoundFile::open(Vec::new()).err(), Some(Error::NotCompound));
    }
}
