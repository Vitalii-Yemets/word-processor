//! The file: where its objects are, and getting them.
//!
//! A file ends with the offset of its cross-reference, which says where
//! every object begins — a table in older files, a stream in newer ones,
//! which may also keep objects packed together inside other streams. A file
//! that has been saved again carries several, each pointing at the one
//! before. When the cross-reference is missing or wrong, which happens,
//! the file is read from the front for everything that looks like an
//! object, which is how every reader repairs one.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::filters;
use super::object::{find, rfind, Dictionary, Lexer, Object, Stream};

/// The objects of one object stream, in order, with their numbers.
type Packed = Vec<(u32, Object)>;

/// Where an object is: at an offset in the file, or inside an object
/// stream.
#[derive(Clone, Copy, Debug)]
enum Location {
    Offset(usize),
    Packed { stream: u32, index: u32 },
}

/// One page, with what it inherits from the page tree.
#[derive(Clone, Debug)]
pub struct PageInfo {
    pub dictionary: Dictionary,
    pub resources: Dictionary,
    /// Left, bottom, right, top, in points.
    pub media_box: [f64; 4],
    pub rotate: i32,
}

pub struct File<'a> {
    bytes: &'a [u8],
    locations: RefCell<HashMap<u32, Location>>,
    trailer: RefCell<Dictionary>,
    cache: RefCell<HashMap<u32, Rc<Object>>>,
    /// The object streams already unpacked, by number: the objects in
    /// each, in order, with their numbers.
    unpacked: RefCell<HashMap<u32, Rc<Packed>>>,
    rebuilt: RefCell<bool>,
}

impl<'a> File<'a> {
    /// Reads the cross-reference and the trailer. A file with neither that
    /// can be read is rebuilt by scanning.
    pub fn open(bytes: &'a [u8]) -> Result<Self, super::Error> {
        if find(&bytes[..bytes.len().min(1024)], b"%PDF").is_none() {
            return Err(super::Error::NotPdf);
        }
        let file = Self {
            bytes,
            locations: RefCell::new(HashMap::new()),
            trailer: RefCell::new(Dictionary::new()),
            cache: RefCell::new(HashMap::new()),
            unpacked: RefCell::new(HashMap::new()),
            rebuilt: RefCell::new(false),
        };
        let read = file.read_cross_references();
        if read.is_err() || !file.trailer.borrow().contains_key("Root") {
            file.rebuild();
        }
        if file.trailer.borrow().get("Encrypt").is_some() {
            return Err(super::Error::Encrypted);
        }
        Ok(file)
    }

    pub fn trailer(&self) -> Dictionary {
        self.trailer.borrow().clone()
    }

    /// Follows `startxref` and every `/Prev` and `/XRefStm` from it.
    fn read_cross_references(&self) -> Result<(), ()> {
        let tail_start = self.bytes.len().saturating_sub(2048);
        let tail = &self.bytes[tail_start..];
        let at = rfind(tail, b"startxref").ok_or(())?;
        let mut lexer = Lexer::from(self.bytes, tail_start + at + 9);
        let Some(Object::Number(offset)) = lexer.next_object() else { return Err(()) };
        let mut pending = vec![offset as usize];
        let mut seen = HashSet::new();
        let mut first = true;
        while let Some(offset) = pending.pop() {
            if !seen.insert(offset) || offset >= self.bytes.len() {
                continue;
            }
            let trailer = self.read_cross_reference_at(offset)?;
            for key in ["Prev", "XRefStm"] {
                if let Some(next) = trailer.get(key).and_then(Object::as_integer) {
                    if let Ok(next) = usize::try_from(next) {
                        // The hybrid file's stream is read before the older
                        // table it points past, so it is pushed last.
                        pending.push(next);
                    }
                }
            }
            let mut held = self.trailer.borrow_mut();
            for (key, value) in trailer {
                if first || !held.contains_key(&key) {
                    held.insert(key, value);
                }
            }
            first = false;
        }
        Ok(())
    }

    /// One cross-reference section, table or stream, giving its trailer.
    fn read_cross_reference_at(&self, offset: usize) -> Result<Dictionary, ()> {
        let mut lexer = Lexer::from(self.bytes, offset);
        lexer.skip_whitespace();
        if self.bytes[lexer.at..].starts_with(b"xref") {
            lexer.at += 4;
            return self.read_table(lexer);
        }
        // A stream: `n g obj << /Type /XRef ... >> stream`.
        lexer.object_header().ok_or(())?;
        let Some(Object::Stream(stream)) = lexer.next_object() else { return Err(()) };
        self.read_stream_table(&stream)?;
        Ok(stream.dictionary)
    }

    fn read_table(&self, mut lexer: Lexer<'a>) -> Result<Dictionary, ()> {
        loop {
            lexer.skip_whitespace();
            if self.bytes[lexer.at..].starts_with(b"trailer") {
                lexer.at += 7;
                return match lexer.next_object() {
                    Some(Object::Dictionary(trailer)) => Ok(trailer),
                    _ => Err(()),
                };
            }
            let Some(Object::Number(start)) = lexer.next_object() else { return Err(()) };
            let Some(Object::Number(count)) = lexer.next_object() else { return Err(()) };
            let start = start as u32;
            let count = count.max(0.0) as u32;
            for index in 0..count {
                lexer.skip_whitespace();
                // Entries are twenty bytes, but a file written by hand may
                // have them shorter: read tokens rather than bytes.
                let Some(Object::Number(offset)) = lexer.next_object() else { return Err(()) };
                let Some(Object::Number(_generation)) = lexer.next_object() else {
                    return Err(());
                };
                let Some(Object::Operator(kind)) = lexer.next_object() else { return Err(()) };
                if kind == "n" {
                    self.locations
                        .borrow_mut()
                        .entry(start + index)
                        .or_insert(Location::Offset(offset as usize));
                }
            }
        }
    }

    fn read_stream_table(&self, stream: &Stream) -> Result<(), ()> {
        let data = self.decode(stream).0;
        let widths: Vec<usize> = stream
            .dictionary
            .get("W")
            .and_then(Object::as_array)
            .ok_or(())?
            .iter()
            .map(|w| w.as_integer().unwrap_or(0).max(0) as usize)
            .collect();
        if widths.len() < 3 {
            return Err(());
        }
        let size = stream.dictionary.get("Size").and_then(Object::as_integer).unwrap_or(0);
        let index: Vec<i64> = match stream.dictionary.get("Index").and_then(Object::as_array) {
            Some(items) => items.iter().map(|i| i.as_integer().unwrap_or(0)).collect(),
            None => vec![0, size],
        };
        let entry_length: usize = widths.iter().sum();
        if entry_length == 0 {
            return Err(());
        }
        let mut entries = data.chunks_exact(entry_length);
        for pair in index.chunks_exact(2) {
            let (start, count) = (pair[0].max(0) as u32, pair[1].max(0) as u32);
            for number in start..start.saturating_add(count) {
                let Some(entry) = entries.next() else { return Ok(()) };
                let mut fields = [1u64, 0, 0];
                let mut at = 0;
                for (field, &width) in fields.iter_mut().zip(&widths) {
                    if width > 0 {
                        *field = entry[at..at + width]
                            .iter()
                            .fold(0u64, |acc, &b| (acc << 8) | u64::from(b));
                        at += width;
                    }
                }
                let location = match fields[0] {
                    1 => Location::Offset(fields[1] as usize),
                    2 => Location::Packed { stream: fields[1] as u32, index: fields[2] as u32 },
                    _ => continue,
                };
                self.locations.borrow_mut().entry(number).or_insert(location);
            }
        }
        Ok(())
    }

    /// Reads the whole file for objects, when the cross-reference cannot
    /// be trusted. A later copy of an object replaces an earlier one, since
    /// that is how a file is saved again.
    fn rebuild(&self) {
        if self.rebuilt.replace(true) {
            return;
        }
        let mut locations = HashMap::new();
        let mut trailer = Dictionary::new();
        let mut packed_streams = Vec::new();
        let bytes = self.bytes;
        let mut at = 0;
        while let Some(found) = find(&bytes[at..], b"obj") {
            let position = at + found;
            at = position + 3;
            let Some((number, start)) = object_start_before(bytes, position) else { continue };
            locations.insert(number, Location::Offset(start));
            // A look at the object, for a cross-reference stream's trailer
            // keys and for object streams to unpack after.
            let mut lexer = Lexer::from(bytes, position + 3);
            if let Some(object) = lexer.next_object() {
                if let Some(dictionary) = object.as_dictionary() {
                    match dictionary.get("Type").and_then(Object::as_name) {
                        Some("XRef") => {
                            for key in ["Root", "Info", "ID"] {
                                if let Some(value) = dictionary.get(key) {
                                    trailer.insert(key.to_owned(), value.clone());
                                }
                            }
                        }
                        Some("ObjStm") => packed_streams.push(number),
                        Some("Catalog") => {
                            trailer
                                .entry("Root".to_owned())
                                .or_insert(Object::Reference(number, 0));
                        }
                        _ => {}
                    }
                }
                at = lexer.at.max(at);
            }
        }
        // The trailers written as such, the last word winning.
        let mut search = 0;
        while let Some(found) = find(&bytes[search..], b"trailer") {
            let mut lexer = Lexer::from(bytes, search + found + 7);
            if let Some(Object::Dictionary(found)) = lexer.next_object() {
                for (key, value) in found {
                    trailer.insert(key, value);
                }
            }
            search += found + 7;
        }
        *self.locations.borrow_mut() = locations;
        self.cache.borrow_mut().clear();
        self.unpacked.borrow_mut().clear();
        {
            let mut held = self.trailer.borrow_mut();
            for (key, value) in trailer {
                held.insert(key, value);
            }
        }
        // Objects inside object streams, where no copy stands alone.
        for stream_number in packed_streams {
            if let Some(objects) = self.unpack(stream_number) {
                for (index, (number, _)) in objects.iter().enumerate() {
                    self.locations
                        .borrow_mut()
                        .entry(*number)
                        .or_insert(Location::Packed { stream: stream_number, index: index as u32 });
                }
            }
        }
        // A catalog the trailer does not name, found by looking.
        let has_root = self.trailer.borrow().contains_key("Root");
        if !has_root {
            let numbers: Vec<u32> = self.locations.borrow().keys().copied().collect();
            for number in numbers {
                let object = self.object(number);
                if object.as_dictionary().and_then(|d| d.get("Type")).and_then(Object::as_name)
                    == Some("Catalog")
                {
                    self.trailer
                        .borrow_mut()
                        .insert("Root".to_owned(), Object::Reference(number, 0));
                    break;
                }
            }
        }
    }

    /// The object of a number, or null when there is none.
    pub fn object(&self, number: u32) -> Rc<Object> {
        if let Some(object) = self.cache.borrow().get(&number) {
            return Rc::clone(object);
        }
        // Held as a marker against a reference loop while it is read.
        self.cache.borrow_mut().insert(number, Rc::new(Object::Null));
        let object = self.read_object(number).unwrap_or_else(|| {
            // Not where the cross-reference said: rebuild once and look
            // again.
            if !*self.rebuilt.borrow() {
                self.rebuild();
                self.cache.borrow_mut().insert(number, Rc::new(Object::Null));
                self.read_object(number).unwrap_or(Object::Null)
            } else {
                Object::Null
            }
        });
        let object = Rc::new(object);
        self.cache.borrow_mut().insert(number, Rc::clone(&object));
        object
    }

    fn read_object(&self, number: u32) -> Option<Object> {
        let location = *self.locations.borrow().get(&number)?;
        match location {
            Location::Offset(offset) => {
                if offset >= self.bytes.len() {
                    return None;
                }
                let mut lexer = Lexer::from(self.bytes, offset);
                let (found, _) = lexer.object_header()?;
                if found != number {
                    return None;
                }
                let object = lexer.next_object()?;
                Some(match object {
                    Object::Operator(word) if word == "endobj" => Object::Null,
                    other => other,
                })
            }
            Location::Packed { stream, index } => {
                let objects = self.unpack(stream)?;
                let (found, object) = objects.get(index as usize)?;
                if *found == number {
                    return Some(object.clone());
                }
                objects.iter().find(|(n, _)| *n == number).map(|(_, object)| object.clone())
            }
        }
    }

    /// The objects packed in an object stream, in order.
    fn unpack(&self, number: u32) -> Option<Rc<Packed>> {
        if let Some(objects) = self.unpacked.borrow().get(&number) {
            return Some(Rc::clone(objects));
        }
        let holder = self.object(number);
        let stream = holder.as_stream()?;
        let data = self.decode(stream).0;
        let count = self.get(&stream.dictionary, "N").as_integer().unwrap_or(0).max(0) as usize;
        let first = self.get(&stream.dictionary, "First").as_integer().unwrap_or(0).max(0) as usize;
        let mut header = Lexer::new(&data);
        let mut objects = Vec::with_capacity(count);
        for _ in 0..count {
            let Some(Object::Number(object_number)) = header.next_object() else { break };
            let Some(Object::Number(offset)) = header.next_object() else { break };
            let at = first + offset as usize;
            if at >= data.len() {
                continue;
            }
            let mut lexer = Lexer::from(&data, at);
            let object = lexer.next_object().unwrap_or(Object::Null);
            objects.push((object_number as u32, object));
        }
        let objects = Rc::new(objects);
        self.unpacked.borrow_mut().insert(number, Rc::clone(&objects));
        Some(objects)
    }

    /// The object itself where a reference is given, followed as far as it
    /// goes.
    pub fn resolve(&self, object: &Object) -> Object {
        let mut current = object.clone();
        for _ in 0..32 {
            match current {
                Object::Reference(number, _) => current = (*self.object(number)).clone(),
                other => return other,
            }
        }
        Object::Null
    }

    /// A dictionary's entry, resolved.
    pub fn get(&self, dictionary: &Dictionary, key: &str) -> Object {
        match dictionary.get(key) {
            Some(value) => self.resolve(value),
            None => Object::Null,
        }
    }

    /// A stream's bytes with its filters undone, and the name of the
    /// picture filter left on them if one was.
    pub fn decode(&self, stream: &Stream) -> (Vec<u8>, Option<String>) {
        let filters: Vec<String> = match self.get(&stream.dictionary, "Filter") {
            Object::Name(name) => vec![name],
            Object::Array(names) => names
                .iter()
                .filter_map(|name| self.resolve(name).as_name().map(str::to_owned))
                .collect(),
            _ => Vec::new(),
        };
        let parameters: Vec<Option<Dictionary>> = match self.get(&stream.dictionary, "DecodeParms")
        {
            Object::Dictionary(parameters) => vec![Some(parameters)],
            Object::Array(items) => {
                items.iter().map(|item| self.resolve(item).as_dictionary().cloned()).collect()
            }
            _ => Vec::new(),
        };
        let mut data = stream.data.clone();
        for (index, filter) in filters.iter().enumerate() {
            if filters::is_picture_filter(filter) {
                return (data, Some(filter.clone()));
            }
            let parameters = parameters.get(index).and_then(Option::as_ref).map(|p| {
                // The parameters may hold references.
                p.iter().map(|(k, v)| (k.clone(), self.resolve(v))).collect::<Dictionary>()
            });
            match filters::apply(filter, &data, parameters.as_ref()) {
                Some(out) => data = out,
                None => return (Vec::new(), None),
            }
        }
        (data, None)
    }

    /// The document's catalog.
    pub fn catalog(&self) -> Dictionary {
        let trailer = self.trailer();
        self.get(&trailer, "Root").as_dictionary().cloned().unwrap_or_default()
    }

    /// The document information dictionary, if there is one.
    pub fn info(&self) -> Dictionary {
        let trailer = self.trailer();
        self.get(&trailer, "Info").as_dictionary().cloned().unwrap_or_default()
    }

    /// The pages in order, each with what the tree above it gives it.
    pub fn pages(&self) -> Vec<PageInfo> {
        let catalog = self.catalog();
        let mut pages = Vec::new();
        let root = self.get(&catalog, "Pages");
        let mut seen = HashSet::new();
        if let Some(root) = root.as_dictionary() {
            let inherited = Inherited::default();
            self.walk_pages(root, &inherited, &mut pages, &mut seen, 0);
        }
        if pages.is_empty() {
            // No tree to speak of: every page object there is, in number
            // order.
            self.rebuild();
            let mut numbers: Vec<u32> = self.locations.borrow().keys().copied().collect();
            numbers.sort_unstable();
            for number in numbers {
                let object = self.object(number);
                if let Some(dictionary) = object.as_dictionary() {
                    if dictionary.get("Type").and_then(Object::as_name) == Some("Page") {
                        let inherited = self.inherited_up(dictionary, 0);
                        pages.push(self.page_of(dictionary, &inherited));
                    }
                }
            }
        }
        pages
    }

    fn walk_pages(
        &self,
        node: &Dictionary,
        inherited: &Inherited,
        pages: &mut Vec<PageInfo>,
        seen: &mut HashSet<usize>,
        depth: usize,
    ) {
        if depth > 64 {
            return;
        }
        let inherited = inherited.with(self, node);
        let kind = node.get("Type").and_then(Object::as_name);
        let kids = self.get(node, "Kids");
        // A page says so, or is a leaf with content and no name.
        let is_page = kind == Some("Page")
            || (kind.is_none() && kids.as_array().is_none() && node.contains_key("Contents"));
        match kids.as_array() {
            _ if is_page => pages.push(self.page_of(node, &inherited)),
            Some(kids) => {
                for kid in kids {
                    if let Some((number, _)) = kid.as_reference() {
                        if !seen.insert(number as usize) {
                            continue;
                        }
                    }
                    if let Some(kid) = self.resolve(kid).as_dictionary() {
                        self.walk_pages(kid, &inherited, pages, seen, depth + 1);
                    }
                }
            }
            _ => {}
        }
    }

    /// What a page inherits, found by walking up from it: for pages found
    /// without a tree.
    fn inherited_up(&self, page: &Dictionary, depth: usize) -> Inherited {
        let mut chain = vec![page.clone()];
        let mut current = page.clone();
        for _ in 0..(64 - depth.min(64)) {
            let Some(parent) = self.get(&current, "Parent").as_dictionary().cloned() else { break };
            chain.push(parent.clone());
            current = parent;
        }
        let mut inherited = Inherited::default();
        for node in chain.iter().rev() {
            inherited = inherited.with(self, node);
        }
        inherited
    }

    fn page_of(&self, page: &Dictionary, inherited: &Inherited) -> PageInfo {
        let media_box = inherited.media_box.unwrap_or([0.0, 0.0, 612.0, 792.0]);
        let media_box = [
            media_box[0].min(media_box[2]),
            media_box[1].min(media_box[3]),
            media_box[0].max(media_box[2]),
            media_box[1].max(media_box[3]),
        ];
        PageInfo {
            dictionary: page.clone(),
            resources: inherited.resources.clone().unwrap_or_default(),
            media_box,
            rotate: inherited.rotate.unwrap_or(0).rem_euclid(360),
        }
    }

    /// A page's content: its streams' bytes, one after another with a
    /// line end between, since a stream may end mid-token.
    pub fn content_of(&self, page: &Dictionary) -> Vec<u8> {
        let mut out = Vec::new();
        match self.get(page, "Contents") {
            Object::Stream(stream) => out = self.decode(&stream).0,
            Object::Array(parts) => {
                for part in parts {
                    if let Some(stream) = self.resolve(&part).as_stream() {
                        out.extend_from_slice(&self.decode(stream).0);
                        out.push(b'\n');
                    }
                }
            }
            _ => {}
        }
        out
    }

    /// A rectangle's four numbers.
    pub fn rectangle(&self, object: &Object) -> Option<[f64; 4]> {
        let items = self.resolve(object);
        let items = items.as_array()?;
        if items.len() < 4 {
            return None;
        }
        let mut out = [0.0; 4];
        for (slot, item) in out.iter_mut().zip(items) {
            *slot = self.resolve(item).as_number()?;
        }
        Some(out)
    }
}

/// What a page may take from the nodes above it.
#[derive(Clone, Debug, Default)]
struct Inherited {
    resources: Option<Dictionary>,
    media_box: Option<[f64; 4]>,
    rotate: Option<i32>,
}

impl Inherited {
    fn with(&self, file: &File<'_>, node: &Dictionary) -> Self {
        let mut next = self.clone();
        if let Some(resources) = file.get(node, "Resources").as_dictionary() {
            next.resources = Some(resources.clone());
        }
        if let Some(media_box) = node.get("MediaBox").and_then(|b| file.rectangle(b)) {
            next.media_box = Some(media_box);
        }
        if let Some(rotate) = file.get(node, "Rotate").as_integer() {
            next.rotate = Some(rotate as i32);
        }
        next
    }
}

/// Given the position of an `obj` keyword, the object number before it and
/// where that number starts — or nothing if it is not an object header.
fn object_start_before(bytes: &[u8], position: usize) -> Option<(u32, usize)> {
    let mut back = position;
    while back > 0 && super::object::is_whitespace(bytes[back - 1]) {
        back -= 1;
    }
    let generation_end = back;
    while back > 0 && bytes[back - 1].is_ascii_digit() {
        back -= 1;
    }
    if back == generation_end || back == 0 || !super::object::is_whitespace(bytes[back - 1]) {
        return None;
    }
    while back > 0 && super::object::is_whitespace(bytes[back - 1]) {
        back -= 1;
    }
    let number_end = back;
    while back > 0 && bytes[back - 1].is_ascii_digit() {
        back -= 1;
    }
    if back == number_end {
        return None;
    }
    if back > 0
        && !super::object::is_whitespace(bytes[back - 1])
        && !super::object::is_delimiter(bytes[back - 1])
    {
        return None;
    }
    let number = std::str::from_utf8(&bytes[back..number_end]).ok()?.parse().ok()?;
    Some((number, back))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: &[u8] = b"%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 100] >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /Contents 4 0 R >> endobj\n4 0 obj << /Length 5 >> stream\nhello\nendstream endobj\ntrailer << /Root 1 0 R >>\n%%EOF";

    #[test]
    fn a_file_without_a_cross_reference_is_rebuilt() {
        let file = File::open(SMALL).unwrap();
        let pages = file.pages();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].media_box, [0.0, 0.0, 200.0, 100.0]);
        assert_eq!(file.content_of(&pages[0].dictionary), b"hello");
    }

    #[test]
    fn a_cross_reference_table_is_followed() {
        let mut bytes = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        let objects: [&[u8]; 4] = [
            b"1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n",
            b"2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n",
            b"3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 4 0 R >> endobj\n",
            b"4 0 obj << /Length 2 >> stream\nhi\nendstream endobj\n",
        ];
        for object in objects {
            offsets.push(bytes.len());
            bytes.extend_from_slice(object);
        }
        let xref_at = bytes.len();
        bytes.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
        for offset in &offsets {
            bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        bytes.extend_from_slice(
            format!("trailer << /Size 5 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF").as_bytes(),
        );
        let file = File::open(&bytes).unwrap();
        assert!(!*file.rebuilt.borrow());
        let pages = file.pages();
        assert_eq!(pages.len(), 1);
        assert_eq!(file.content_of(&pages[0].dictionary), b"hi");
    }

    #[test]
    fn an_encrypted_file_is_refused() {
        let bytes = b"%PDF-1.4\n1 0 obj << /Type /Catalog >> endobj\ntrailer << /Root 1 0 R /Encrypt 5 0 R >>";
        assert!(matches!(File::open(bytes), Err(super::super::Error::Encrypted)));
    }
}
