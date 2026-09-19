//! A `UserForm`: what it is made of, read out of its designer storage.
//!
//! # Where a form lives
//!
//! A form module is two things in the file. Its code is a module like any
//! other, in the `VBA` storage. Its design — the window, and the boxes and
//! buttons on it — is a storage of its own beside that one, named after the
//! module, written the way Office Forms writes every parent control
//! ([MS-OFORMS] §2.1.2): a stream called `f` holding the form's own
//! properties and one *site* for each control on it, and a stream called `o`
//! holding the controls themselves, one after another, in the order of the
//! sites.
//!
//! # What is read, and how
//!
//! Every structure in that format has the same shape: a version, a size, a
//! *property mask* saying which properties are stored, then the values that
//! are four bytes or fewer in the order of the mask, each aligned to its own
//! size, then the larger ones in the same order. A property that is not
//! stored has its default. So reading is walking the mask: for each bit in
//! turn, if it is set, take the value; if not, take the default. The mask
//! orders below are the specification's, bit by bit, and are the whole of
//! what this file knows.
//!
//! What is taken is what is needed to show the form and run its code: the
//! caption and size of the form; and for each control its name, what kind it
//! is, where it is, how big, its caption or its text, whether it is shown
//! and enabled, its place in the tab order, and whether it is the button
//! Enter or Escape presses. Colours, fonts, pictures, tooltips and the rest
//! are stepped over, by their sizes, which the format gives.
//!
//! # What a list holds
//!
//! Nothing, in the file. A `ListBox` or `ComboBox` is filled by the code —
//! `ListBox1.AddItem "First"` in `UserForm_Initialize` — and the file stores
//! only the control. The items here are for the form while it is running.
//!
//! # Not done, and named here
//!
//! Frames and multi-page controls are storages inside the form's storage,
//! with their own `f` and `o`, and their children are not read; a form with
//! one shows the frame's outline and nothing inside it. Images, spin buttons,
//! scroll bars and tab strips are read as far as their site — name, place,
//! size — and drawn as an empty box. Fonts are not read, so every control is
//! drawn in the one face the program draws its own dialogs in.

use core::fmt;

/// One point in HIMETRIC units, which is what the format measures in:
/// hundredths of a millimetre.
const HIMETRIC_PER_POINT: f32 = 2540.0 / 72.0;

/// What went wrong reading a form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The stream ended before the structure did.
    Short(&'static str),
    /// A version this program does not know.
    Version(&'static str, u8),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Short(what) => write!(f, "the form's {what} ends before it should"),
            Self::Version(what, version) => {
                write!(
                    f,
                    "the form's {what} is version {version}, which this program does not know"
                )
            }
        }
    }
}

impl std::error::Error for Error {}

/// What kind of control one is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Label,
    TextBox,
    CheckBox,
    OptionButton,
    ToggleButton,
    ComboBox,
    ListBox,
    CommandButton,
    /// A frame or a multi-page, whose children live in a storage of their own.
    Frame,
    /// One this program draws as a box: an image, a spin button, a scroll
    /// bar, a tab strip.
    Other,
}

impl Kind {
    /// The name Visual Basic gives the kind, which `TypeName` answers.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Label => "Label",
            Self::TextBox => "TextBox",
            Self::CheckBox => "CheckBox",
            Self::OptionButton => "OptionButton",
            Self::ToggleButton => "ToggleButton",
            Self::ComboBox => "ComboBox",
            Self::ListBox => "ListBox",
            Self::CommandButton => "CommandButton",
            Self::Frame => "Frame",
            Self::Other => "Control",
        }
    }

    /// Whether it has a `Caption` rather than a `Text`.
    #[must_use]
    pub fn has_caption(self) -> bool {
        matches!(
            self,
            Self::Label
                | Self::CheckBox
                | Self::OptionButton
                | Self::ToggleButton
                | Self::CommandButton
                | Self::Frame
        )
    }

    /// Whether it is ticked or not, rather than holding words.
    #[must_use]
    pub fn is_tick(self) -> bool {
        matches!(self, Self::CheckBox | Self::OptionButton | Self::ToggleButton)
    }

    /// Whether it offers a list.
    #[must_use]
    pub fn has_list(self) -> bool {
        matches!(self, Self::ComboBox | Self::ListBox)
    }

    /// The `ClsidCacheIndex` the format writes for it, and the `DisplayStyle`
    /// where the kind is one of the six the format writes as one structure.
    fn cached(self) -> (u16, u8) {
        match self {
            Self::Label => (21, 0),
            Self::TextBox => (15, 1),
            Self::ListBox => (15, 2),
            Self::ComboBox => (15, 3),
            Self::CheckBox => (15, 4),
            Self::OptionButton => (15, 5),
            Self::ToggleButton => (15, 6),
            Self::CommandButton => (17, 0),
            Self::Frame => (14, 0),
            Self::Other => (12, 0),
        }
    }
}

/// One control on a form.
#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    pub name: String,
    pub kind: Kind,
    /// The words on it, for a kind that has any.
    pub caption: String,
    /// What it holds: a box's text, `1` or `0` for a tick, the chosen item
    /// of a list.
    pub value: String,
    /// Where it is on the form and how big, in points.
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
    pub visible: bool,
    pub enabled: bool,
    /// Its place in the tab order; less than nought is none.
    pub tab_index: i32,
    /// Whether Enter presses it, and whether Escape does.
    pub default: bool,
    pub cancel: bool,
    /// Which option buttons go together: the `GroupName`.
    pub group: String,
    /// What a list offers, which the code puts in.
    pub items: Vec<String>,
    /// Which of them is chosen, from nought; -1 for none.
    pub list_index: i32,
}

impl Control {
    /// A control of a kind at a place, with everything else as the format
    /// defaults it.
    #[must_use]
    pub fn new(name: &str, kind: Kind, left: f32, top: f32, width: f32, height: f32) -> Self {
        Self {
            name: name.to_owned(),
            kind,
            caption: String::new(),
            value: String::new(),
            left,
            top,
            width,
            height,
            visible: true,
            enabled: true,
            tab_index: -1,
            default: false,
            cancel: false,
            group: String::new(),
            items: Vec::new(),
            list_index: -1,
        }
    }

    /// The same with words on it.
    #[must_use]
    pub fn captioned(mut self, caption: &str) -> Self {
        self.caption = caption.to_owned();
        self
    }

    /// Whether a tick is ticked.
    #[must_use]
    pub fn ticked(&self) -> bool {
        self.value == "1" || self.value.eq_ignore_ascii_case("true") || self.value == "-1"
    }
}

/// A form: its window, and what is on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Form {
    pub name: String,
    pub caption: String,
    /// The size of the window's inside, in points.
    pub width: f32,
    pub height: f32,
    pub controls: Vec<Control>,
}

impl Form {
    /// An empty form of the size Word gives a new one.
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            caption: name.to_owned(),
            width: 240.0,
            height: 180.0,
            controls: Vec::new(),
        }
    }

    /// The control of a name, if there is one.
    #[must_use]
    pub fn control(&self, name: &str) -> Option<&Control> {
        self.controls.iter().find(|control| control.name.eq_ignore_ascii_case(name))
    }

    /// And to change it.
    pub fn control_mut(&mut self, name: &str) -> Option<&mut Control> {
        self.controls.iter_mut().find(|control| control.name.eq_ignore_ascii_case(name))
    }

    /// The controls in the order Tab walks them: by `TabIndex`, with the
    /// ones that have none last, as they were drawn.
    #[must_use]
    pub fn tab_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.controls.len())
            .filter(|at| {
                let control = &self.controls[*at];
                control.visible && control.enabled && control.kind != Kind::Label
            })
            .collect();
        order.sort_by_key(|at| {
            let index = self.controls[*at].tab_index;
            (if index < 0 { i32::MAX } else { index }, *at)
        });
        order
    }
}

/// What happened on a form while it was up, which is what its code is told.
#[derive(Clone, Debug, PartialEq)]
pub enum Happening {
    /// The window was shut with its close button.
    Closed,
    /// Something was done to a control: which, what, and what every control
    /// holds now, because typing changes a box without asking anybody.
    On { control: String, event: String, values: Vec<(String, String, i32)> },
}

// --- Reading ---------------------------------------------------------------

/// A cursor over a structure, which aligns each value to its own size from
/// where the structure began, as the format has it.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    what: &'static str,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8], what: &'static str) -> Self {
        Self { bytes, at: 0, what }
    }

    fn align(&mut self, size: usize) {
        while self.at % size != 0 {
            self.at += 1;
        }
    }

    fn take(&mut self, size: usize) -> Result<&'a [u8], Error> {
        let end = self.at.checked_add(size).ok_or(Error::Short(self.what))?;
        let held = self.bytes.get(self.at..end).ok_or(Error::Short(self.what))?;
        self.at = end;
        Ok(held)
    }

    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, Error> {
        self.align(2);
        let held = self.take(2)?;
        Ok(u16::from_le_bytes([held[0], held[1]]))
    }

    fn u32(&mut self) -> Result<u32, Error> {
        self.align(4);
        let held = self.take(4)?;
        Ok(u32::from_le_bytes([held[0], held[1], held[2], held[3]]))
    }

    fn i32(&mut self) -> Result<i32, Error> {
        Ok(self.u32()? as i32)
    }

    /// A string whose length and compression a `CountOfBytesWithCompressionFlag`
    /// gave, padded to four bytes as a string in an extra-data block is.
    fn string(&mut self, count: u32) -> Result<String, Error> {
        let compressed = count & 0x8000_0000 != 0;
        let length = (count & 0x7FFF_FFFF) as usize;
        let held = self.take(length)?;
        let text = if compressed {
            held.iter().map(|byte| char::from(*byte)).collect()
        } else {
            let units: Vec<u16> =
                held.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
            String::from_utf16_lossy(&units)
        };
        self.align(4);
        Ok(text)
    }

    /// Whatever is left, from here.
    fn rest(&self) -> &'a [u8] {
        self.bytes.get(self.at..).unwrap_or_default()
    }
}

/// A size or a position: two HIMETRIC numbers, as points.
fn pair(reader: &mut Reader<'_>) -> Result<(f32, f32), Error> {
    let first = reader.i32()? as f32 / HIMETRIC_PER_POINT;
    let second = reader.i32()? as f32 / HIMETRIC_PER_POINT;
    Ok((first, second))
}

/// The header every structure begins with: the version, and how many bytes
/// the mask and the two data blocks come to.
fn header(reader: &mut Reader<'_>, wanted: u8) -> Result<usize, Error> {
    let _minor = reader.u8()?;
    let major = reader.u8()?;
    if major != wanted {
        return Err(Error::Version(reader.what, major));
    }
    Ok(reader.u16()? as usize)
}

/// One site: what the form stores about a control, apart from the control.
struct Site {
    name: String,
    id: i32,
    flags: u32,
    stream_size: usize,
    tab_index: i32,
    class: u16,
    left: f32,
    top: f32,
}

/// Reads the form out of its two streams.
pub fn read(f: &[u8], o: &[u8], name: &str) -> Result<Form, Error> {
    let mut form = Form::new(name);
    let mut reader = Reader::new(f, "window");
    let size = header(&mut reader, 4)?;
    let mask = reader.u32()?;

    // The data block, in the order of the mask.
    let mut caption_length = 0u32;
    let mut booleans = 0x0000_0004u32;
    if mask & (1 << 1) != 0 {
        reader.u32()?; // BackColor
    }
    if mask & (1 << 2) != 0 {
        reader.u32()?; // ForeColor
    }
    if mask & (1 << 3) != 0 {
        reader.u32()?; // NextAvailableID
    }
    if mask & (1 << 6) != 0 {
        booleans = reader.u32()?;
    }
    if mask & (1 << 7) != 0 {
        reader.u8()?; // BorderStyle
    }
    if mask & (1 << 8) != 0 {
        reader.u8()?; // MousePointer
    }
    if mask & (1 << 9) != 0 {
        reader.u8()?; // ScrollBars
    }
    if mask & (1 << 13) != 0 {
        reader.i32()?; // GroupCnt
    }
    if mask & (1 << 15) != 0 {
        reader.u16()?; // MouseIcon
    }
    if mask & (1 << 16) != 0 {
        reader.u8()?; // Cycle
    }
    if mask & (1 << 17) != 0 {
        reader.u8()?; // SpecialEffect
    }
    if mask & (1 << 18) != 0 {
        reader.u32()?; // BorderColor
    }
    if mask & (1 << 19) != 0 {
        caption_length = reader.u32()?;
    }
    if mask & (1 << 20) != 0 {
        reader.u16()?; // Font
    }
    if mask & (1 << 21) != 0 {
        reader.u16()?; // Picture
    }
    if mask & (1 << 22) != 0 {
        reader.u32()?; // Zoom
    }
    if mask & (1 << 23) != 0 {
        reader.u8()?; // PictureAlignment
    }
    if mask & (1 << 25) != 0 {
        reader.u8()?; // PictureSizeMode
    }
    if mask & (1 << 26) != 0 {
        reader.u32()?; // ShapeCookie
    }
    if mask & (1 << 27) != 0 {
        reader.u32()?; // DrawBuffer
    }
    reader.align(4);

    // The extra data block, likewise.
    if mask & (1 << 10) != 0 {
        let (width, height) = pair(&mut reader)?;
        form.width = width;
        form.height = height;
    }
    if mask & (1 << 11) != 0 {
        pair(&mut reader)?; // LogicalSize
    }
    if mask & (1 << 12) != 0 {
        pair(&mut reader)?; // ScrollPosition
    }
    if mask & (1 << 19) != 0 {
        form.caption = reader.string(caption_length)?;
    }
    // Whatever the size said and this did not read, which is nothing when
    // the two agree, and a property this program does not know when they
    // do not.
    reader.at = reader.at.max(4 + size);

    // The stream data: pictures and a font, each a GUID and a structure
    // that says its own size.
    if mask & (1 << 15) != 0 {
        skip_picture(&mut reader)?;
    }
    if mask & (1 << 20) != 0 {
        skip_font(&mut reader)?;
    }
    if mask & (1 << 21) != 0 {
        skip_picture(&mut reader)?;
    }

    // The sites: first the class table, unless the form says it kept none.
    if booleans & (1 << 15) == 0 {
        let classes = reader.u16()?;
        for _ in 0..classes {
            let _version = reader.u16()?;
            let size = reader.u16()? as usize;
            reader.take(size)?;
        }
    }
    let count = reader.u32()? as usize;
    let _bytes = reader.u32()?;
    // The depths and types, which say how many sites there are in what
    // order, and are padded to four bytes as a whole.
    let start = reader.at;
    let mut listed = 0usize;
    while listed < count {
        let _depth = reader.u8()?;
        let type_or_count = reader.u8()?;
        if type_or_count & 0x80 != 0 {
            let _type = reader.u8()?;
            listed += usize::from(type_or_count & 0x7F);
        } else {
            listed += 1;
        }
    }
    let used = reader.at - start;
    reader.take((4 - used % 4) % 4)?;

    let mut sites = Vec::with_capacity(count);
    for _ in 0..count {
        sites.push(site(&mut reader)?);
    }

    // And the controls themselves, each as many bytes of the object stream
    // as its site said.
    let mut at = 0usize;
    for site in sites {
        let end = at.saturating_add(site.stream_size).min(o.len());
        let bytes = o.get(at..end).unwrap_or_default();
        at = end;
        let mut control = Control::new(&site.name, Kind::Other, site.left, site.top, 0.0, 0.0);
        control.visible = site.flags & (1 << 1) != 0;
        control.default = site.flags & (1 << 2) != 0;
        control.cancel = site.flags & (1 << 3) != 0;
        control.tab_index = site.tab_index;
        let _ = site.id;
        match site.class {
            21 => label(bytes, &mut control)?,
            17 => command_button(bytes, &mut control)?,
            15 | 23..=28 => morph(bytes, site.class, &mut control)?,
            14 | 57 => control.kind = Kind::Frame,
            _ => {}
        }
        form.controls.push(control);
    }
    Ok(form)
}

/// One `OleSiteConcreteControl`.
fn site(reader: &mut Reader<'_>) -> Result<Site, Error> {
    let begin = reader.at;
    // A site aligns from its own beginning, as every structure does.
    let mut inner = Reader::new(reader.rest(), "control's site");
    let _version = inner.u16()?;
    let size = inner.u16()? as usize;
    let mask = inner.u32()?;

    let mut name_length = 0u32;
    let mut tag_length = 0u32;
    let mut site = Site {
        name: String::new(),
        id: 0,
        flags: 0x0000_0033,
        stream_size: 0,
        tab_index: -1,
        class: 0x7FFF,
        left: 0.0,
        top: 0.0,
    };
    if mask & (1 << 0) != 0 {
        name_length = inner.u32()?;
    }
    if mask & (1 << 1) != 0 {
        tag_length = inner.u32()?;
    }
    if mask & (1 << 2) != 0 {
        site.id = inner.i32()?;
    }
    if mask & (1 << 3) != 0 {
        inner.i32()?; // HelpContextID
    }
    if mask & (1 << 4) != 0 {
        site.flags = inner.u32()?;
    }
    if mask & (1 << 5) != 0 {
        site.stream_size = inner.u32()? as usize;
    }
    if mask & (1 << 6) != 0 {
        site.tab_index = i32::from(inner.u16()? as i16);
    }
    if mask & (1 << 7) != 0 {
        site.class = inner.u16()?;
    }
    if mask & (1 << 9) != 0 {
        inner.u16()?; // GroupID
    }
    let mut tip_length = 0u32;
    let mut key_length = 0u32;
    let mut source_length = 0u32;
    let mut row_length = 0u32;
    if mask & (1 << 11) != 0 {
        tip_length = inner.u32()?;
    }
    if mask & (1 << 12) != 0 {
        key_length = inner.u32()?;
    }
    if mask & (1 << 13) != 0 {
        source_length = inner.u32()?;
    }
    if mask & (1 << 14) != 0 {
        row_length = inner.u32()?;
    }
    inner.align(4);

    if mask & (1 << 0) != 0 {
        site.name = inner.string(name_length)?;
    }
    if mask & (1 << 1) != 0 {
        inner.string(tag_length)?;
    }
    if mask & (1 << 8) != 0 {
        let (left, top) = pair(&mut inner)?;
        site.left = left;
        site.top = top;
    }
    if mask & (1 << 11) != 0 {
        inner.string(tip_length)?;
    }
    if mask & (1 << 12) != 0 {
        inner.string(key_length)?;
    }
    if mask & (1 << 13) != 0 {
        inner.string(source_length)?;
    }
    if mask & (1 << 14) != 0 {
        inner.string(row_length)?;
    }
    // The size says where the next site begins, whatever was read.
    reader.at = begin + 4 + size;
    if reader.at > reader.bytes.len() {
        return Err(Error::Short("control's site"));
    }
    Ok(site)
}

/// A `GuidAndPicture`: a GUID, a preamble, a size, and that many bytes.
fn skip_picture(reader: &mut Reader<'_>) -> Result<(), Error> {
    reader.take(16)?;
    let _preamble = reader.take(4)?;
    let size = {
        let held = reader.take(4)?;
        u32::from_le_bytes([held[0], held[1], held[2], held[3]]) as usize
    };
    reader.take(size)?;
    Ok(())
}

/// A `GuidAndFont`: a GUID saying which of two structures follows.
fn skip_font(reader: &mut Reader<'_>) -> Result<(), Error> {
    const STD_FONT: [u8; 16] = [
        0x03, 0x52, 0xE3, 0x0B, 0x91, 0x8F, 0xCE, 0x11, 0x9D, 0xE3, 0x00, 0xAA, 0x00, 0x4B, 0xB8,
        0x51,
    ];
    let guid = reader.take(16)?;
    if guid == STD_FONT {
        // Version, charset, flags, weight, height, and the face by length.
        reader.take(1 + 2 + 1 + 2 + 4)?;
        let length = usize::from(reader.take(1)?[0]);
        reader.take(length)?;
    } else {
        // TextProps, which carries its own size after a two-byte version.
        reader.take(2)?;
        let size = {
            let held = reader.take(2)?;
            usize::from(u16::from_le_bytes([held[0], held[1]]))
        };
        reader.take(size)?;
    }
    Ok(())
}

/// A `LabelControl`.
fn label(bytes: &[u8], control: &mut Control) -> Result<(), Error> {
    control.kind = Kind::Label;
    let mut reader = Reader::new(bytes, "label");
    header(&mut reader, 2)?;
    let mask = reader.u32()?;
    let mut caption_length = 0u32;
    if mask & (1 << 0) != 0 {
        reader.u32()?; // ForeColor
    }
    if mask & (1 << 1) != 0 {
        reader.u32()?; // BackColor
    }
    if mask & (1 << 2) != 0 {
        control.enabled = reader.u32()? & (1 << 1) != 0;
    }
    if mask & (1 << 3) != 0 {
        caption_length = reader.u32()?;
    }
    if mask & (1 << 4) != 0 {
        reader.u32()?; // PicturePosition
    }
    if mask & (1 << 6) != 0 {
        reader.u8()?; // MousePointer
    }
    if mask & (1 << 7) != 0 {
        reader.u32()?; // BorderColor
    }
    if mask & (1 << 8) != 0 {
        reader.u16()?; // BorderStyle
    }
    if mask & (1 << 9) != 0 {
        reader.u16()?; // SpecialEffect
    }
    if mask & (1 << 10) != 0 {
        reader.u16()?; // Picture
    }
    if mask & (1 << 11) != 0 {
        reader.u16()?; // Accelerator
    }
    if mask & (1 << 12) != 0 {
        reader.u16()?; // MouseIcon
    }
    reader.align(4);
    if mask & (1 << 3) != 0 {
        control.caption = reader.string(caption_length)?;
    }
    if mask & (1 << 5) != 0 {
        let (width, height) = pair(&mut reader)?;
        control.width = width;
        control.height = height;
    }
    Ok(())
}

/// A `CommandButtonControl`.
fn command_button(bytes: &[u8], control: &mut Control) -> Result<(), Error> {
    control.kind = Kind::CommandButton;
    let mut reader = Reader::new(bytes, "button");
    header(&mut reader, 2)?;
    let mask = reader.u32()?;
    let mut caption_length = 0u32;
    if mask & (1 << 0) != 0 {
        reader.u32()?; // ForeColor
    }
    if mask & (1 << 1) != 0 {
        reader.u32()?; // BackColor
    }
    if mask & (1 << 2) != 0 {
        control.enabled = reader.u32()? & (1 << 1) != 0;
    }
    if mask & (1 << 3) != 0 {
        caption_length = reader.u32()?;
    }
    if mask & (1 << 4) != 0 {
        reader.u32()?; // PicturePosition
    }
    if mask & (1 << 6) != 0 {
        reader.u8()?; // MousePointer
    }
    if mask & (1 << 7) != 0 {
        reader.u16()?; // Picture
    }
    if mask & (1 << 8) != 0 {
        reader.u16()?; // Accelerator
    }
    if mask & (1 << 10) != 0 {
        reader.u16()?; // MouseIcon
    }
    reader.align(4);
    if mask & (1 << 3) != 0 {
        control.caption = reader.string(caption_length)?;
    }
    if mask & (1 << 5) != 0 {
        let (width, height) = pair(&mut reader)?;
        control.width = width;
        control.height = height;
    }
    Ok(())
}

/// A `MorphDataControl`, which is six kinds of control in one structure:
/// its `DisplayStyle` says which.
fn morph(bytes: &[u8], class: u16, control: &mut Control) -> Result<(), Error> {
    let mut reader = Reader::new(bytes, "box");
    header(&mut reader, 2)?;
    let low = reader.u32()?;
    let high = reader.u32()?;
    let mut style = match class {
        23 => 1u8,
        24 => 2,
        25 => 3,
        26 => 4,
        27 => 5,
        28 => 6,
        _ => 1,
    };
    let (mut value_length, mut caption_length, mut group_length) = (0u32, 0u32, 0u32);
    if low & (1 << 0) != 0 {
        control.enabled = reader.u32()? & (1 << 1) != 0;
    }
    if low & (1 << 1) != 0 {
        reader.u32()?; // BackColor
    }
    if low & (1 << 2) != 0 {
        reader.u32()?; // ForeColor
    }
    if low & (1 << 3) != 0 {
        reader.u32()?; // MaxLength
    }
    if low & (1 << 4) != 0 {
        reader.u8()?; // BorderStyle
    }
    if low & (1 << 5) != 0 {
        reader.u8()?; // ScrollBars
    }
    if low & (1 << 6) != 0 {
        style = reader.u8()?;
    }
    if low & (1 << 7) != 0 {
        reader.u8()?; // MousePointer
    }
    if low & (1 << 9) != 0 {
        reader.u16()?; // PasswordChar
    }
    if low & (1 << 10) != 0 {
        reader.u32()?; // ListWidth
    }
    if low & (1 << 11) != 0 {
        reader.u16()?; // BoundColumn
    }
    if low & (1 << 12) != 0 {
        reader.u16()?; // TextColumn
    }
    if low & (1 << 13) != 0 {
        reader.u16()?; // ColumnCount
    }
    if low & (1 << 14) != 0 {
        reader.u16()?; // ListRows
    }
    if low & (1 << 15) != 0 {
        reader.u16()?; // cColumnInfo
    }
    if low & (1 << 16) != 0 {
        reader.u8()?; // MatchEntry
    }
    if low & (1 << 17) != 0 {
        reader.u8()?; // ListStyle
    }
    if low & (1 << 18) != 0 {
        reader.u8()?; // ShowDropButtonWhen
    }
    if low & (1 << 20) != 0 {
        reader.u8()?; // DropButtonStyle
    }
    if low & (1 << 21) != 0 {
        reader.u8()?; // MultiSelect
    }
    if low & (1 << 22) != 0 {
        value_length = reader.u32()?;
    }
    if low & (1 << 23) != 0 {
        caption_length = reader.u32()?;
    }
    if low & (1 << 24) != 0 {
        reader.u32()?; // PicturePosition
    }
    if low & (1 << 25) != 0 {
        reader.u32()?; // BorderColor
    }
    if low & (1 << 26) != 0 {
        reader.u32()?; // SpecialEffect
    }
    if low & (1 << 27) != 0 {
        reader.u16()?; // MouseIcon
    }
    if low & (1 << 28) != 0 {
        reader.u16()?; // Picture
    }
    if low & (1 << 29) != 0 {
        reader.u16()?; // Accelerator
    }
    if high & 1 != 0 {
        group_length = reader.u32()?;
    }
    reader.align(4);

    if low & (1 << 8) != 0 {
        let (width, height) = pair(&mut reader)?;
        control.width = width;
        control.height = height;
    }
    if low & (1 << 22) != 0 {
        control.value = reader.string(value_length)?;
    }
    if low & (1 << 23) != 0 {
        control.caption = reader.string(caption_length)?;
    }
    if high & 1 != 0 {
        control.group = reader.string(group_length)?;
    }
    control.kind = match style {
        2 => Kind::ListBox,
        3 | 7 => Kind::ComboBox,
        4 => Kind::CheckBox,
        5 => Kind::OptionButton,
        6 => Kind::ToggleButton,
        _ => Kind::TextBox,
    };
    Ok(())
}

// --- Writing ---------------------------------------------------------------

/// A structure being written: values aligned to their size from its start.
#[derive(Default)]
struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    fn align(&mut self, size: usize) {
        while self.bytes.len() % size != 0 {
            self.bytes.push(0);
        }
    }

    fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn u16(&mut self, value: u16) {
        self.align(2);
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.align(4);
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.u32(value as u32);
    }

    /// A string, compressed where every character fits a byte, padded to
    /// four; gives back what the data block's count must say.
    fn string(&mut self, text: &str) -> u32 {
        let count = if text.chars().all(|character| (character as u32) < 0x100) {
            for character in text.chars() {
                self.bytes.push(character as u32 as u8);
            }
            #[allow(clippy::cast_possible_truncation)]
            {
                text.chars().count() as u32 | 0x8000_0000
            }
        } else {
            let units: Vec<u16> = text.encode_utf16().collect();
            for unit in &units {
                self.bytes.extend_from_slice(&unit.to_le_bytes());
            }
            #[allow(clippy::cast_possible_truncation)]
            {
                (units.len() * 2) as u32
            }
        };
        self.align(4);
        count
    }

    fn pair(&mut self, first: f32, second: f32) {
        #[allow(clippy::cast_possible_truncation)]
        {
            self.i32((first * HIMETRIC_PER_POINT).round() as i32);
            self.i32((second * HIMETRIC_PER_POINT).round() as i32);
        }
    }
}

/// The size of a string in the data block's count, before it is written.
fn string_count(text: &str) -> u32 {
    #[allow(clippy::cast_possible_truncation)]
    if text.chars().all(|character| (character as u32) < 0x100) {
        text.chars().count() as u32 | 0x8000_0000
    } else {
        (text.encode_utf16().count() * 2) as u32
    }
}

/// A structure: the version, the size, the mask, and the two blocks, put
/// together so that the size is right.
fn structure(major: u8, mask: &[u8], data: &[u8], extra: &[u8]) -> Vec<u8> {
    let mut out = vec![0x00, major];
    #[allow(clippy::cast_possible_truncation)]
    out.extend_from_slice(&((mask.len() + data.len() + extra.len()) as u16).to_le_bytes());
    out.extend_from_slice(mask);
    out.extend_from_slice(data);
    out.extend_from_slice(extra);
    out
}

/// Writes a form as its two streams, `f` and `o`, the way the format has
/// them — the same way the reader above reads them, which is what the
/// tests hold the two to.
#[must_use]
pub fn write(form: &Form) -> (Vec<u8>, Vec<u8>) {
    let mut objects = Vec::new();
    let mut sites = Vec::new();
    for (at, control) in form.controls.iter().enumerate() {
        let bytes = match control.kind {
            Kind::Label => captioned_control(control),
            Kind::CommandButton => captioned_control(control),
            Kind::Frame | Kind::Other => Vec::new(),
            _ => morph_control(control),
        };
        sites.push(site_bytes(control, at, bytes.len()));
        objects.extend_from_slice(&bytes);
    }

    // The form's own properties: its caption, its size, and the buffer the
    // format insists on.
    let mut mask = 0u32;
    let mut data = Writer::default();
    let mut extra = Writer::default();
    // No class table, which is the flag saying so, with the form enabled.
    mask |= 1 << 6;
    data.u32(0x0000_0004 | (1 << 15));
    mask |= 1 << 19;
    data.u32(string_count(&form.caption));
    mask |= 1 << 27;
    data.u32(0x0001_0000);
    data.align(4);
    mask |= 1 << 10;
    extra.pair(form.width, form.height);
    extra.string(&form.caption);
    let mut f = structure(4, &mask.to_le_bytes(), &data.bytes, &extra.bytes);

    // The sites: how many, how many bytes, one depth-and-type entry each,
    // padded, then the sites themselves.
    let mut depths = Vec::new();
    for _ in &sites {
        depths.push(0u8);
        depths.push(0x01);
    }
    while depths.len() % 4 != 0 {
        depths.push(0);
    }
    let bytes: usize = depths.len() + sites.iter().map(Vec::len).sum::<usize>();
    #[allow(clippy::cast_possible_truncation)]
    {
        f.extend_from_slice(&(sites.len() as u32).to_le_bytes());
        f.extend_from_slice(&(bytes as u32).to_le_bytes());
    }
    f.extend_from_slice(&depths);
    for site in sites {
        f.extend_from_slice(&site);
    }
    (f, objects)
}

/// One site, for a control of this many bytes in the object stream.
fn site_bytes(control: &Control, at: usize, stream_size: usize) -> Vec<u8> {
    let mut mask = 0u32;
    let mut data = Writer::default();
    let mut extra = Writer::default();
    mask |= 1 << 0;
    data.u32(string_count(&control.name));
    mask |= 1 << 2;
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    data.i32(at as i32 + 1);
    let mut flags = 0x0000_0033u32;
    if !control.visible {
        flags &= !(1 << 1);
    }
    if control.default {
        flags |= 1 << 2;
    }
    if control.cancel {
        flags |= 1 << 3;
    }
    mask |= 1 << 4;
    data.u32(flags);
    mask |= 1 << 5;
    #[allow(clippy::cast_possible_truncation)]
    data.u32(stream_size as u32);
    mask |= 1 << 6;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    data.u16(control.tab_index.clamp(-1, i32::from(i16::MAX)) as i16 as u16);
    mask |= 1 << 7;
    data.u16(control.kind.cached().0);
    data.align(4);
    extra.string(&control.name);
    mask |= 1 << 8;
    extra.pair(control.left, control.top);
    structure(0, &mask.to_le_bytes(), &data.bytes, &extra.bytes)
}

/// A label or a button: a caption and a size.
fn captioned_control(control: &Control) -> Vec<u8> {
    let mut mask = 0u32;
    let mut data = Writer::default();
    let mut extra = Writer::default();
    if !control.enabled {
        mask |= 1 << 2;
        data.u32(0x0000_0019);
    }
    mask |= 1 << 3;
    data.u32(string_count(&control.caption));
    mask |= 1 << 5;
    data.align(4);
    extra.string(&control.caption);
    extra.pair(control.width, control.height);
    structure(2, &mask.to_le_bytes(), &data.bytes, &extra.bytes)
}

/// One of the six the format writes as one structure.
fn morph_control(control: &Control) -> Vec<u8> {
    let mut low = 0u32;
    let mut high = 0u32;
    let mut data = Writer::default();
    let mut extra = Writer::default();
    if !control.enabled {
        low |= 1 << 0;
        data.u32(0x2C80_0819);
    }
    low |= 1 << 6;
    data.u8(control.kind.cached().1);
    low |= 1 << 8;
    if !control.value.is_empty() {
        low |= 1 << 22;
        data.u32(string_count(&control.value));
    }
    if !control.caption.is_empty() {
        low |= 1 << 23;
        data.u32(string_count(&control.caption));
    }
    if !control.group.is_empty() {
        high |= 1;
        data.u32(string_count(&control.group));
    }
    data.align(4);
    extra.pair(control.width, control.height);
    if !control.value.is_empty() {
        extra.string(&control.value);
    }
    if !control.caption.is_empty() {
        extra.string(&control.caption);
    }
    if !control.group.is_empty() {
        extra.string(&control.group);
    }
    let mut mask = low.to_le_bytes().to_vec();
    mask.extend_from_slice(&high.to_le_bytes());
    structure(2, &mask, &data.bytes, &extra.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example() -> Form {
        let mut form = Form::new("UserForm1");
        form.caption = "Ask".to_owned();
        form.width = 240.0;
        form.height = 120.0;
        form.controls.push(
            Control::new("Label1", Kind::Label, 12.0, 12.0, 100.0, 12.0).captioned("Your name"),
        );
        let mut box_ = Control::new("TextBox1", Kind::TextBox, 12.0, 30.0, 200.0, 18.0);
        box_.value = "Bob".to_owned();
        box_.tab_index = 0;
        form.controls.push(box_);
        let mut tick = Control::new("CheckBox1", Kind::CheckBox, 12.0, 54.0, 120.0, 18.0)
            .captioned("Remember");
        tick.value = "1".to_owned();
        tick.tab_index = 1;
        form.controls.push(tick);
        let mut ok =
            Control::new("OK", Kind::CommandButton, 150.0, 84.0, 72.0, 24.0).captioned("OK");
        ok.default = true;
        ok.tab_index = 2;
        form.controls.push(ok);
        let mut hidden =
            Control::new("Secret", Kind::OptionButton, 12.0, 84.0, 60.0, 18.0).captioned("Naïve");
        hidden.visible = false;
        hidden.enabled = false;
        hidden.group = "Ways".to_owned();
        form.controls.push(hidden);
        form
    }

    #[test]
    fn a_form_written_is_the_form_read_back() {
        let form = example();
        let (f, o) = write(&form);
        let read = read(&f, &o, "UserForm1").expect("a form");
        assert_eq!(read.caption, "Ask");
        assert!((read.width - 240.0).abs() < 0.1, "{}", read.width);
        assert!((read.height - 120.0).abs() < 0.1, "{}", read.height);
        assert_eq!(read.controls.len(), 5);
        for (wanted, got) in form.controls.iter().zip(&read.controls) {
            assert_eq!(got.name, wanted.name);
            assert_eq!(got.kind, wanted.kind, "{}", wanted.name);
            assert_eq!(got.caption, wanted.caption, "{}", wanted.name);
            assert_eq!(got.value, wanted.value, "{}", wanted.name);
            assert_eq!(got.group, wanted.group, "{}", wanted.name);
            assert_eq!(got.visible, wanted.visible, "{}", wanted.name);
            assert_eq!(got.enabled, wanted.enabled, "{}", wanted.name);
            assert_eq!(got.default, wanted.default, "{}", wanted.name);
            assert_eq!(got.tab_index, wanted.tab_index, "{}", wanted.name);
            for (one, other) in [
                (got.left, wanted.left),
                (got.top, wanted.top),
                (got.width, wanted.width),
                (got.height, wanted.height),
            ] {
                assert!((one - other).abs() < 0.1, "{}: {one} against {other}", wanted.name);
            }
        }
    }

    #[test]
    fn the_stream_is_laid_out_as_the_specification_says() {
        // Not the reader against the writer but both against the format:
        // the bytes of a form with one button, by hand from [MS-OFORMS].
        let mut form = Form::new("UserForm1");
        form.caption = "Hi".to_owned();
        form.width = 72.0;
        form.height = 36.0;
        form.controls
            .push(Control::new("Go", Kind::CommandButton, 0.0, 0.0, 36.0, 18.0).captioned("Go"));
        let (f, o) = write(&form);

        // The window: version 4; size; mask with BooleanProperties (bit 6),
        // DisplayedSize (10), Caption (19) and DrawBuffer (27).
        assert_eq!(&f[..2], &[0x00, 0x04]);
        let mask = u32::from_le_bytes([f[4], f[5], f[6], f[7]]);
        assert_eq!(mask, (1 << 6) | (1 << 10) | (1 << 19) | (1 << 27));
        // Data block: booleans, caption count (2 bytes, compressed), buffer.
        assert_eq!(&f[8..12], &(0x0000_0004u32 | (1 << 15)).to_le_bytes());
        assert_eq!(&f[12..16], &(2u32 | 0x8000_0000).to_le_bytes());
        assert_eq!(&f[16..20], &0x0001_0000u32.to_le_bytes());
        // Extra: the size in HIMETRIC — 72pt is 2540 — then "Hi" padded.
        assert_eq!(&f[20..24], &2540i32.to_le_bytes());
        assert_eq!(&f[24..28], &1270i32.to_le_bytes());
        assert_eq!(&f[28..32], b"Hi\0\0");
        let size = u16::from_le_bytes([f[2], f[3]]);
        assert_eq!(usize::from(size), 32 - 4);
        // Then one site: count, bytes, one depth-and-type padded to four.
        assert_eq!(&f[32..36], &1u32.to_le_bytes());
        assert_eq!(&f[40..44], &[0x00, 0x01, 0x00, 0x00]);
        // The site's own header and mask: Name, ID, BitFlags,
        // ObjectStreamSize, TabIndex, ClsidCacheIndex, Position.
        assert_eq!(&f[44..46], &[0x00, 0x00]);
        let mask = u32::from_le_bytes([f[48], f[49], f[50], f[51]]);
        assert_eq!(mask, 0b1_1111_0101);
        // The button in the object stream: version 2, mask with Caption (3)
        // and Size (5), the count, then "Go" and the size.
        assert_eq!(&o[..2], &[0x00, 0x02]);
        let mask = u32::from_le_bytes([o[4], o[5], o[6], o[7]]);
        assert_eq!(mask, (1 << 3) | (1 << 5));
        assert_eq!(&o[8..12], &(2u32 | 0x8000_0000).to_le_bytes());
        assert_eq!(&o[12..16], b"Go\0\0");
        assert_eq!(&o[16..20], &1270i32.to_le_bytes());
        assert_eq!(&o[20..24], &635i32.to_le_bytes());

        let read = read(&f, &o, "UserForm1").expect("a form");
        assert_eq!(read.controls[0].caption, "Go");
        assert_eq!(read.controls[0].kind, Kind::CommandButton);
    }

    #[test]
    fn a_short_stream_says_so_rather_than_panicking() {
        let (f, o) = write(&example());
        assert!(matches!(read(&f[..20], &o, "x"), Err(Error::Short(_))));
        // And a control cut short is found out by its own structure.
        assert!(matches!(read(&f, &o[..o.len() / 2], "x"), Err(Error::Short(_))));
    }

    #[test]
    fn the_tab_order_is_by_index_with_labels_left_out() {
        let form = example();
        let order: Vec<&str> =
            form.tab_order().iter().map(|at| form.controls[*at].name.as_str()).collect();
        assert_eq!(order, ["TextBox1", "CheckBox1", "OK"]);
    }
}
