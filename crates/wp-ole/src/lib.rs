//! The compound file: a file system in a file.
//!
//! # What it is for here
//!
//! Two things, which is why it is a crate of its own rather than a corner of
//! the one that reads `.doc` files. Everything Office wrote before 2007 is one
//! of these, and so is an encrypted document written yesterday: Word puts the
//! whole of a protected `.docx` inside one, as a single stream beside a second
//! stream saying how it was encrypted. So this is read by [`wp_doc`] and read
//! and written by [`wp_crypt`].
//!
//! # The shape of one
//!
//! [MS-CFB]. A header,
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

/// What can be wrong with a compound file.
///
/// The same three the crates that read one need to tell apart: it is not one
/// at all, it stops in the middle, or it says something the format does not
/// allow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Not a compound file at all.
    NotCompound,
    /// The file ends before the structure named does.
    Truncated(&'static str),
    /// The structure named is not as the format says.
    Malformed(&'static str),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotCompound => write!(f, "not a compound file"),
            Self::Truncated(what) => write!(f, "the file ends inside its {what}"),
            Self::Malformed(what) => write!(f, "the file's {what} is not as the format says"),
        }
    }
}

impl std::error::Error for Error {}

/// The end of a chain.
const END_OF_CHAIN: u32 = 0xFFFF_FFFE;
/// A sector nothing uses.
const FREE: u32 = 0xFFFF_FFFF;
/// A sector the FAT itself lives in.
const FAT_SECTOR: u32 = 0xFFFF_FFFD;
/// A sector the FAT's own directory lives in.
const DIFAT_SECTOR: u32 = 0xFFFF_FFFC;
/// How big a sector is in the version this writes. The other size the reader
/// accepts is four thousand and ninety-six, which is version four.
const SECTOR: usize = 512;
/// How many sector numbers one sector of the FAT holds.
const PER_FAT_SECTOR: usize = SECTOR / 4;
/// And one sector of the DIFAT, which keeps its last four bytes for the next
/// one in the chain.
const PER_DIFAT_SECTOR: usize = SECTOR / 4 - 1;
/// How many of them the header holds before a DIFAT sector is needed at all.
const IN_THE_HEADER: usize = 109;
/// How long one directory entry is.
const ENTRY_SIZE: usize = 128;
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
    /// The class of object a storage holds — what program it is — as the
    /// sixteen bytes of its identifier; noughts where none was said.
    pub class: [u8; 16],
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

    /// Finds a stream by walking the directory tree, the way the format
    /// means it to be walked.
    ///
    /// [`Self::stream`] looks through every entry, which is the forgiving
    /// thing to do with a file somebody else wrote. This is the unforgiving
    /// one: it starts at the root's child and turns left or right by the
    /// order the format sorts names in, which is what Word does. A file whose
    /// tree does not agree with its entries is found out here and nowhere
    /// else, so this is what a file written by this program is held to.
    ///
    /// The path is the names to go through: `["Storage", "Stream"]`.
    #[must_use]
    pub fn walk(&self, path: &[&str]) -> Option<Vec<u8>> {
        let mut at = self.entries.first()?.child;
        let mut found = None;
        for (depth, wanted) in path.iter().enumerate() {
            let entry = self.search(at, wanted)?;
            if depth + 1 < path.len() {
                at = entry.child;
            }
            found = Some(entry);
        }
        let found = found?;
        if found.kind != EntryKind::Stream {
            return None;
        }
        self.read_entry(found).ok()
    }

    /// One step of that walk: the entry of this name among a tree of
    /// siblings.
    fn search(&self, from: u32, wanted: &str) -> Option<&Entry> {
        let mut at = from;
        let mut guard = 0;
        while (at as usize) < self.entries.len() {
            let entry = &self.entries[at as usize];
            at = match order(wanted, &entry.name) {
                core::cmp::Ordering::Equal => return Some(entry),
                core::cmp::Ordering::Less => entry.left,
                core::cmp::Ordering::Greater => entry.right,
            };
            guard += 1;
            if guard > self.entries.len() {
                // A tree that turns back on itself, which a file this program
                // did not write may well have.
                return None;
            }
        }
        None
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
        class: bytes[80..96].try_into().unwrap_or([0; 16]),
    }
}

// --- Writing one -------------------------------------------------------------

/// Writes a list of things into the directory, and says where each of them
/// went.
///
/// Recursive, because a storage may hold storages: what comes back is the
/// numbers of the entries at this level, and each storage's own children have
/// been written and joined to it before it is returned.
fn lay_out(items: &[Item], written: &mut Vec<Written>) -> Vec<usize> {
    let mut here = Vec::new();
    for item in items {
        here.push(written.len());
        match item {
            Item::Stream { name, bytes } => {
                written.push(Written::stream(name, bytes.clone()));
            }
            Item::Storage { name, items } => {
                let at = written.len();
                written.push(Written::storage(name));
                let inside = lay_out(items, written);
                let child = tree(written, &inside);
                written[at].child = child;
            }
        }
    }
    here
}

/// A compound file being built: a tree of storages and streams.
///
/// # Why building it is harder than reading it
///
/// Because everything in the file refers to everything else by sector number,
/// and the tables that say which sector is which live in sectors of their own.
/// The FAT has to describe the sectors the FAT is in; if it grows into one more
/// sector, that sector has to be described too, and it may be the sector that
/// makes it grow again. So the sizes are settled first, by going round until
/// they stop changing, and only then is anything written.
#[derive(Clone, Debug, Default)]
pub struct Builder {
    /// The things at the top of the file, in the order they were added.
    items: Vec<Item>,
    /// The class of object the whole file holds, said on its root.
    class: [u8; 16],
}

#[derive(Clone, Debug)]
pub enum Item {
    Stream {
        name: String,
        bytes: Vec<u8>,
    },
    /// A storage and everything in it, to any depth: a storage may hold
    /// storages, which is what the description of an encrypted document is
    /// made of.
    Storage {
        name: String,
        items: Vec<Item>,
    },
}

impl Item {
    /// A stream of bytes under a name.
    #[must_use]
    pub fn stream(name: &str, bytes: Vec<u8>) -> Self {
        Self::Stream { name: name.to_owned(), bytes }
    }

    /// A storage holding whatever is given.
    #[must_use]
    pub fn storage(name: &str, items: Vec<Item>) -> Self {
        Self::Storage { name: name.to_owned(), items }
    }
}

impl Builder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a stream at the top of the file.
    pub fn stream(&mut self, name: &str, bytes: Vec<u8>) -> &mut Self {
        self.items.push(Item::Stream { name: name.to_owned(), bytes });
        self
    }

    /// Adds a storage holding the named streams.
    pub fn storage(&mut self, name: &str, streams: Vec<(String, Vec<u8>)>) -> &mut Self {
        let items = streams.into_iter().map(|(name, bytes)| Item::stream(&name, bytes)).collect();
        self.items.push(Item::storage(name, items));
        self
    }

    /// Says what class of object the file holds — which program's — by the
    /// sixteen bytes of its identifier, in the order the file keeps them.
    /// An object embedded in another program's document is known by this.
    pub fn class(&mut self, class: [u8; 16]) -> &mut Self {
        self.class = class;
        self
    }

    /// Adds anything at all at the top of the file, storages inside storages
    /// included.
    pub fn item(&mut self, item: Item) -> &mut Self {
        self.items.push(item);
        self
    }

    /// The bytes of the finished file.
    #[must_use]
    pub fn build(&self) -> Vec<u8> {
        // What goes in the directory, in the order the entries are numbered:
        // the root first, then everything at the top, then the contents of
        // each storage. The order in the file does not matter — the trees
        // below do — but it has to be settled before anything can point at
        // anything.
        let mut written: Vec<Written> = vec![Written { class: self.class, ..Written::root() }];
        let top = lay_out(&self.items, &mut written);
        written[0].child = tree(&mut written, &top);

        // The streams themselves. Long ones get sectors of their own; short
        // ones are packed into the mini stream, which is the root's.
        let mut sectors: Vec<u8> = Vec::new();
        let mut fat: Vec<u32> = Vec::new();
        let mut mini_stream: Vec<u8> = Vec::new();
        let mut mini_fat: Vec<u32> = Vec::new();

        // The root is left out: its own stream is the mini stream, which is
        // made out of the others and so cannot be put anywhere until they
        // have been.
        for entry in written.iter_mut().skip(1) {
            if entry.kind != EntryKind::Stream {
                continue;
            }
            let bytes = std::mem::take(&mut entry.bytes);
            if bytes.is_empty() {
                entry.start = END_OF_CHAIN;
                continue;
            }
            if bytes.len() < MINI_CUTOFF as usize {
                let first = mini_stream.len() / MINI_SECTOR;
                let count = bytes.len().div_ceil(MINI_SECTOR);
                mini_stream.extend_from_slice(&bytes);
                mini_stream.resize((first + count) * MINI_SECTOR, 0);
                for step in 0..count {
                    let number = (first + step) as u32;
                    mini_fat.push(if step + 1 == count { END_OF_CHAIN } else { number + 1 });
                }
                entry.start = first as u32;
            } else {
                entry.start = add_chain(&mut sectors, &mut fat, &bytes);
            }
        }

        // The mini stream is a stream like any other and lives on the root.
        written[0].size = mini_stream.len() as u64;
        written[0].start = add_chain(&mut sectors, &mut fat, &mini_stream);

        let mini_fat_start = add_chain(&mut sectors, &mut fat, &numbers(&mini_fat));
        let mini_fat_sectors = mini_fat.len().div_ceil(PER_FAT_SECTOR);

        // The directory, padded out to a whole sector with unused entries.
        let mut directory = Vec::with_capacity(written.len() * ENTRY_SIZE);
        for entry in &written {
            directory.extend_from_slice(&entry.to_bytes());
        }
        while directory.len() % SECTOR != 0 {
            directory.extend_from_slice(&Written::unused().to_bytes());
        }
        let directory_sectors = directory.len() / SECTOR;
        let first_directory = add_chain(&mut sectors, &mut fat, &directory);

        // Now the two tables that describe the sectors they are in. Going
        // round until the count stops changing: one more FAT sector may need
        // one more DIFAT sector, which may need one more FAT sector.
        let content = fat.len();
        let (fat_sectors, difat_sectors) = settle(content);

        // The FAT says what each of its own sectors is, and the DIFAT's.
        fat.resize(content + fat_sectors + difat_sectors, FREE);
        for step in 0..fat_sectors {
            fat[content + step] = FAT_SECTOR;
        }
        for step in 0..difat_sectors {
            fat[content + fat_sectors + step] = DIFAT_SECTOR;
        }
        fat.resize(fat_sectors * PER_FAT_SECTOR, FREE);
        sectors.extend_from_slice(&numbers(&fat));

        // The DIFAT: the first hundred and nine FAT sectors are named in the
        // header and the rest in a chain of sectors after the FAT.
        let fat_places: Vec<u32> = (0..fat_sectors).map(|step| (content + step) as u32).collect();
        let mut difat = Vec::new();
        for step in 0..difat_sectors {
            let from = IN_THE_HEADER + step * PER_DIFAT_SECTOR;
            let mut sector: Vec<u32> =
                fat_places.iter().skip(from).take(PER_DIFAT_SECTOR).copied().collect();
            sector.resize(PER_DIFAT_SECTOR, FREE);
            sector.push(if step + 1 == difat_sectors {
                END_OF_CHAIN
            } else {
                (content + fat_sectors + step + 1) as u32
            });
            difat.extend(sector);
        }
        sectors.extend_from_slice(&numbers(&difat));

        let mut out = header(
            fat_sectors,
            first_directory,
            directory_sectors,
            mini_fat_start,
            mini_fat_sectors,
            difat_sectors,
            content + fat_sectors,
            &fat_places,
        );
        out.extend_from_slice(&sectors);
        out
    }
}

/// How many sectors the FAT and the DIFAT need to describe `content` sectors
/// and themselves.
fn settle(content: usize) -> (usize, usize) {
    let (mut fat_sectors, mut difat_sectors) = (0usize, 0usize);
    loop {
        let total = content + fat_sectors + difat_sectors;
        let wanted_fat = total.div_ceil(PER_FAT_SECTOR).max(1);
        let wanted_difat = wanted_fat.saturating_sub(IN_THE_HEADER).div_ceil(PER_DIFAT_SECTOR);
        if wanted_fat == fat_sectors && wanted_difat == difat_sectors {
            return (fat_sectors, difat_sectors);
        }
        fat_sectors = wanted_fat;
        difat_sectors = wanted_difat;
    }
}

/// Puts bytes into sectors of their own and links them up.
///
/// Gives back the first sector, or the end-of-chain marker for nothing at
/// all, which is what a stream of no bytes says for its start.
fn add_chain(sectors: &mut Vec<u8>, fat: &mut Vec<u32>, bytes: &[u8]) -> u32 {
    if bytes.is_empty() {
        return END_OF_CHAIN;
    }
    let first = fat.len() as u32;
    let count = bytes.len().div_ceil(SECTOR);
    sectors.extend_from_slice(bytes);
    sectors.resize(sectors.len() + (count * SECTOR - bytes.len()), 0);
    for step in 0..count {
        fat.push(if step + 1 == count { END_OF_CHAIN } else { first + step as u32 + 1 });
    }
    first
}

/// A run of sector numbers as the bytes a table is written in.
fn numbers(values: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 4);
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
    while out.len() % SECTOR != 0 {
        // A table that does not fill its last sector is padded with free
        // sectors, not with zeroes: nought is sector nought.
        out.extend_from_slice(&FREE.to_le_bytes());
    }
    out
}

/// Builds a balanced tree out of entries that are already in the order the
/// format sorts them, and gives back its root.
///
/// The format holds the contents of each storage as a red-black tree, sorted
/// by the length of the name and then by the name itself in capitals. A
/// program reading one walks the tree to find a name, so the order has to
/// hold; the colours it does not check, and everything here is black.
fn tree(written: &mut [Written], members: &[usize]) -> u32 {
    let mut sorted = members.to_vec();
    sorted.sort_by(|left, right| order(&written[*left].name, &written[*right].name));
    balance(written, &sorted)
}

fn balance(written: &mut [Written], sorted: &[usize]) -> u32 {
    if sorted.is_empty() {
        return NO_ENTRY;
    }
    let middle = sorted.len() / 2;
    let at = sorted[middle];
    let left = balance(written, &sorted[..middle]);
    let right = balance(written, &sorted[middle + 1..]);
    written[at].left = left;
    written[at].right = right;
    at as u32
}

/// The order the format puts names in: shorter first, then by the name in
/// capitals. Not alphabetical order, and a reader that assumed it was would
/// fail to find half the streams in a file Office wrote.
fn order(left: &str, right: &str) -> core::cmp::Ordering {
    let length = left.encode_utf16().count().cmp(&right.encode_utf16().count());
    length.then_with(|| {
        let capitals = |name: &str| -> Vec<u16> {
            name.encode_utf16()
                .map(|unit| match char::from_u32(u32::from(unit)) {
                    Some(letter) => letter.to_uppercase().next().map_or(unit, |up| up as u16),
                    None => unit,
                })
                .collect()
        };
        capitals(left).cmp(&capitals(right))
    })
}

/// Nothing there: what an entry writes for a child or a sibling it has not
/// got.
const NO_ENTRY: u32 = 0xFFFF_FFFF;

/// One directory entry on its way into the file.
#[derive(Clone, Debug)]
struct Written {
    name: String,
    kind: EntryKind,
    bytes: Vec<u8>,
    start: u32,
    size: u64,
    left: u32,
    right: u32,
    child: u32,
    /// The class of object a storage holds, which only the root is given.
    class: [u8; 16],
}

impl Written {
    fn root() -> Self {
        Self {
            name: "Root Entry".to_owned(),
            kind: EntryKind::Root,
            bytes: Vec::new(),
            start: END_OF_CHAIN,
            size: 0,
            left: NO_ENTRY,
            right: NO_ENTRY,
            child: NO_ENTRY,
            class: [0; 16],
        }
    }

    fn stream(name: &str, bytes: Vec<u8>) -> Self {
        Self {
            name: name.to_owned(),
            kind: EntryKind::Stream,
            size: bytes.len() as u64,
            bytes,
            start: END_OF_CHAIN,
            left: NO_ENTRY,
            right: NO_ENTRY,
            child: NO_ENTRY,
            class: [0; 16],
        }
    }

    fn storage(name: &str) -> Self {
        Self { kind: EntryKind::Storage, ..Self::stream(name, Vec::new()) }
    }

    fn unused() -> Self {
        Self { kind: EntryKind::Unused, ..Self::stream("", Vec::new()) }
    }

    fn to_bytes(&self) -> [u8; ENTRY_SIZE] {
        let mut out = [0u8; ENTRY_SIZE];
        let units: Vec<u16> = self.name.encode_utf16().take(31).collect();
        for (at, unit) in units.iter().enumerate() {
            out[at * 2..at * 2 + 2].copy_from_slice(&unit.to_le_bytes());
        }
        // The length counts the ending nought, and an unused entry has none.
        let length = if self.kind == EntryKind::Unused { 0 } else { (units.len() + 1) * 2 };
        out[64..66].copy_from_slice(&(length as u16).to_le_bytes());
        out[66] = match self.kind {
            EntryKind::Storage => 1,
            EntryKind::Stream => 2,
            EntryKind::Root => 5,
            EntryKind::Unused => 0,
        };
        // Black, which is the colour a reader does not look at.
        out[67] = 1;
        out[68..72].copy_from_slice(&self.left.to_le_bytes());
        out[72..76].copy_from_slice(&self.right.to_le_bytes());
        out[76..80].copy_from_slice(&self.child.to_le_bytes());
        out[80..96].copy_from_slice(&self.class);
        out[116..120].copy_from_slice(&self.start.to_le_bytes());
        out[120..128].copy_from_slice(&self.size.to_le_bytes());
        out
    }
}

/// The five hundred and twelve bytes at the front.
#[allow(clippy::too_many_arguments)]
fn header(
    fat_sectors: usize,
    first_directory: u32,
    directory_sectors: usize,
    first_mini_fat: u32,
    mini_fat_sectors: usize,
    difat_sectors: usize,
    first_difat: usize,
    fat_places: &[u32],
) -> Vec<u8> {
    let mut out = vec![0u8; SECTOR];
    out[..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
    // Version three, little-endian, five hundred and twelve byte sectors and
    // sixty-four byte mini-sectors: what Office writes and what every reader
    // of one has understood since 1995.
    // The minor version first and the major after it, as the format lays
    // them out: written the other way round they said version 62, which a
    // reader that checks turns away.
    out[0x18..0x1A].copy_from_slice(&0x003Eu16.to_le_bytes());
    out[0x1A..0x1C].copy_from_slice(&3u16.to_le_bytes());
    out[0x1C..0x1E].copy_from_slice(&0xFFFEu16.to_le_bytes());
    out[0x1E..0x20].copy_from_slice(&9u16.to_le_bytes());
    out[0x20..0x22].copy_from_slice(&6u16.to_le_bytes());
    // Version three does not count its directory sectors: the format says
    // the field must be nought, and a strict reader holds it to that.
    let _ = directory_sectors;
    out[0x28..0x2C].copy_from_slice(&0u32.to_le_bytes());
    out[0x2C..0x30].copy_from_slice(&(fat_sectors as u32).to_le_bytes());
    out[0x30..0x34].copy_from_slice(&first_directory.to_le_bytes());
    out[0x38..0x3C].copy_from_slice(&MINI_CUTOFF.to_le_bytes());
    out[0x3C..0x40].copy_from_slice(&first_mini_fat.to_le_bytes());
    out[0x40..0x44].copy_from_slice(&(mini_fat_sectors as u32).to_le_bytes());
    out[0x44..0x48].copy_from_slice(
        &(if difat_sectors == 0 { END_OF_CHAIN } else { first_difat as u32 }).to_le_bytes(),
    );
    out[0x48..0x4C].copy_from_slice(&(difat_sectors as u32).to_le_bytes());
    for index in 0..IN_THE_HEADER {
        let at = 0x4C + index * 4;
        let value = fat_places.get(index).copied().unwrap_or(FREE);
        out[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The header's versions where the format puts them: the minor at 0x18
    /// and the major at 0x1A. Written the other way round the file said it
    /// was version sixty-two, which this reader never looked at and Word and
    /// LibreOffice both refuse — every encrypted document and macro project
    /// this program wrote went out that way until a `.doc` was held to
    /// LibreOffice.
    #[test]
    fn the_header_says_version_three_where_the_format_says_it() {
        let bytes = Builder::new().stream("A", vec![1; 10]).build();
        assert_eq!(&bytes[0x18..0x1A], &0x003Eu16.to_le_bytes(), "the minor version");
        assert_eq!(&bytes[0x1A..0x1C], &3u16.to_le_bytes(), "the major version");
        assert_eq!(&bytes[0x1C..0x1E], &0xFFFEu16.to_le_bytes(), "the byte order");
        assert_eq!(&bytes[0x28..0x2C], &[0, 0, 0, 0], "version three counts no directory sectors");
    }

    /// The class of object the file holds, on its root, where a program
    /// that is handed an embedded object looks to see whose it is.
    #[test]
    fn the_root_says_the_class_it_is_given() {
        let class = [7u8, 1, 2, 3, 4, 5, 6, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let bytes = Builder::new().class(class).stream("Package", vec![1; 10]).build();
        let file = CompoundFile::open(bytes).expect("a compound file");
        assert_eq!(file.entries()[0].kind, EntryKind::Root);
        assert_eq!(file.entries()[0].class, class);
        let stream = file.entries().iter().find(|entry| entry.name == "Package").expect("it");
        assert_eq!(stream.class, [0; 16], "a stream has none");
    }

    #[test]
    fn something_that_is_not_a_compound_file_is_refused() {
        assert_eq!(CompoundFile::open(b"PK".to_vec()).err(), Some(Error::NotCompound));
        assert_eq!(CompoundFile::open(Vec::new()).err(), Some(Error::NotCompound));
    }

    /// Streams on both sides of the line between a sector of its own and a
    /// place in the mini stream, which is where a writer goes wrong.
    #[test]
    fn what_was_written_reads_back_however_long_it_is() {
        let lengths = [0usize, 1, 63, 64, 65, 4095, 4096, 4097, 511, 512, 513, 20_000];
        let mut builder = Builder::new();
        for (index, length) in lengths.iter().enumerate() {
            let bytes: Vec<u8> = (0..*length).map(|at| (at + index) as u8).collect();
            builder.stream(&format!("Stream {index}"), bytes);
        }
        let file = CompoundFile::open(builder.build()).expect("what was written is one");

        for (index, length) in lengths.iter().enumerate() {
            let wanted: Vec<u8> = (0..*length).map(|at| (at + index) as u8).collect();
            let name = format!("Stream {index}");
            assert_eq!(file.stream(&name).as_deref(), Some(&wanted[..]), "{name} by its entry");
            assert_eq!(
                file.walk(&[&name]).as_deref(),
                Some(&wanted[..]),
                "{name} by walking the tree"
            );
        }
    }

    #[test]
    fn a_storage_and_what_is_inside_it() {
        let mut builder = Builder::new();
        builder.stream("EncryptionInfo", vec![1; 200]);
        builder.storage(
            "{6}DataSpaces",
            vec![("Version".to_owned(), vec![2; 76]), ("DataSpaceMap".to_owned(), vec![3; 112])],
        );
        let file = CompoundFile::open(builder.build()).expect("a compound file");

        assert_eq!(file.walk(&["EncryptionInfo"]), Some(vec![1; 200]));
        assert_eq!(file.walk(&["{6}DataSpaces", "Version"]), Some(vec![2; 76]));
        assert_eq!(file.walk(&["{6}DataSpaces", "DataSpaceMap"]), Some(vec![3; 112]));
        assert_eq!(file.walk(&["{6}DataSpaces"]), None, "a storage is not a stream");
        assert_eq!(file.walk(&["Version"]), None, "and what is inside one is not at the top");
    }

    /// A hundred and nine sectors of the FAT is as far as the header goes;
    /// past that the FAT needs a directory of its own, and that is the part
    /// of this format a writer is most likely to leave out.
    #[test]
    fn a_stream_too_big_for_the_fat_the_header_can_hold() {
        // Past 109 × 128 × 512 bytes, which is a little over seven million.
        let long: Vec<u8> = (0..8_000_000u32).map(|at| (at >> 3) as u8).collect();
        let mut builder = Builder::new();
        builder.stream("EncryptedPackage", long.clone());
        let bytes = builder.build();

        let file = CompoundFile::open(bytes).expect("a compound file");
        assert_eq!(file.walk(&["EncryptedPackage"]), Some(long));
    }

    #[test]
    fn the_names_are_ordered_as_the_format_orders_them() {
        use core::cmp::Ordering;
        // Shorter first, whatever the letters.
        assert_eq!(order("zz", "aaa"), Ordering::Less);
        // Then by the name in capitals, so that case does not part two names
        // a reader would take for the same one.
        assert_eq!(order("abc", "ABC"), Ordering::Equal);
        assert_eq!(order("abc", "abd"), Ordering::Less);
        assert_eq!(order("{6}Primary", "{6}Primary"), Ordering::Equal);
    }

    /// Every name findable by the walk, with enough of them that the tree has
    /// to be a tree and not a list.
    #[test]
    fn a_great_many_streams_are_all_still_found_by_walking() {
        let mut builder = Builder::new();
        let names: Vec<String> = (0..200).map(|index| format!("Stream {index}")).collect();
        for (index, name) in names.iter().enumerate() {
            builder.stream(name, vec![index as u8; 40]);
        }
        let file = CompoundFile::open(builder.build()).expect("a compound file");
        for (index, name) in names.iter().enumerate() {
            assert_eq!(file.walk(&[name.as_str()]), Some(vec![index as u8; 40]), "{name}");
        }
    }
}
