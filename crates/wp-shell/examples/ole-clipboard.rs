//! Word's own clipboard format, read and written by OLE itself.
//!
//! A program of its own for `tools/check-clipboard-wine.sh`, which runs it
//! under Wine: the shell puts a copy on the clipboard as Word does — the
//! document as "Embed Source", a compound file, with its "Object
//! Descriptor" — and OLE's own clipboard and storage, which are Wine's here
//! and not this program's, are asked for it: the object as a storage, the
//! class it says, the package in it, and the descriptor. Then the other way
//! round: a storage made by OLE, holding a package, put on the clipboard as
//! Word would put one, and the shell asked what document the clipboard
//! holds. Each finding is a line on standard output; see the script.
//!
//! On anything but Windows it does nothing.

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
fn main() {
    ole::run();
}

#[cfg(windows)]
#[allow(clippy::upper_case_acronyms, non_snake_case)]
mod ole {
    use core::ffi::c_void;
    use core::ptr::null_mut;

    #[repr(C)]
    #[derive(Clone, Copy, PartialEq, Eq)]
    struct Guid {
        d1: u32,
        d2: u16,
        d3: u16,
        d4: [u8; 8],
    }

    /// `Word.Document.12`.
    const WORD: Guid = Guid {
        d1: 0xF475_4C9B,
        d2: 0x64F5,
        d3: 0x4B40,
        d4: [0x8A, 0xF4, 0x67, 0x97, 0x32, 0xAC, 0x06, 0x07],
    };

    #[repr(C)]
    struct FormatEtc {
        format: u16,
        device: *mut c_void,
        aspect: u32,
        index: i32,
        medium: u32,
    }

    #[repr(C)]
    struct Medium {
        kind: u32,
        handle: *mut c_void,
        release: *mut c_void,
    }

    const TYMED_HGLOBAL: u32 = 1;
    const TYMED_ISTORAGE: u32 = 8;
    const STGM_READ: u32 = 0;
    const STGM_WRITE: u32 = 1;
    const STGM_READWRITE: u32 = 2;
    const STGM_SHARE_EXCLUSIVE: u32 = 0x10;
    const STGM_CREATE: u32 = 0x1000;
    const CF_UNICODETEXT: u32 = 13;
    const GMEM_MOVEABLE: u32 = 2;

    #[link(name = "ole32")]
    extern "system" {
        fn OleInitialize(reserved: *mut c_void) -> i32;
        fn OleGetClipboard(object: *mut *mut c_void) -> i32;
        fn ReleaseStgMedium(medium: *mut Medium);
        fn CreateILockBytesOnHGlobal(
            memory: *mut c_void,
            delete_on_release: i32,
            bytes: *mut *mut c_void,
        ) -> i32;
        fn StgCreateDocfileOnILockBytes(
            bytes: *mut c_void,
            mode: u32,
            reserved: u32,
            storage: *mut *mut c_void,
        ) -> i32;
        fn StgOpenStorageOnILockBytes(
            bytes: *mut c_void,
            priority: *mut c_void,
            mode: u32,
            exclude: *mut c_void,
            reserved: u32,
            storage: *mut *mut c_void,
        ) -> i32;
        fn GetHGlobalFromILockBytes(bytes: *mut c_void, memory: *mut *mut c_void) -> i32;
        fn WriteClassStg(storage: *mut c_void, class: *const Guid) -> i32;
        fn ReadClassStg(storage: *mut c_void, class: *mut Guid) -> i32;
    }

    #[link(name = "user32")]
    extern "system" {
        fn RegisterClipboardFormatW(name: *const u16) -> u32;
        fn OpenClipboard(owner: *mut c_void) -> i32;
        fn EmptyClipboard() -> i32;
        fn SetClipboardData(format: u32, memory: *mut c_void) -> *mut c_void;
        fn CloseClipboard() -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalAlloc(flags: u32, size: usize) -> *mut c_void;
        fn GlobalLock(memory: *mut c_void) -> *mut c_void;
        fn GlobalUnlock(memory: *mut c_void) -> i32;
        fn GlobalSize(memory: *mut c_void) -> usize;
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }

    /// A method of a COM object, by its place in the object's table.
    unsafe fn method(object: *mut c_void, at: usize) -> *const c_void {
        let table = *object.cast::<*const *const c_void>();
        *table.add(at)
    }

    unsafe fn release(object: *mut c_void) {
        let call: unsafe extern "system" fn(*mut c_void) -> u32 =
            core::mem::transmute(method(object, 2));
        call(object);
    }

    /// Everything in a stream of a storage, read through the stream.
    unsafe fn read_stream(storage: *mut c_void, name: &str) -> Option<Vec<u8>> {
        let open: unsafe extern "system" fn(
            *mut c_void,
            *const u16,
            *mut c_void,
            u32,
            u32,
            *mut *mut c_void,
        ) -> i32 = core::mem::transmute(method(storage, 4));
        let mut stream = null_mut();
        let name = wide(name);
        if open(
            storage,
            name.as_ptr(),
            null_mut(),
            STGM_READ | STGM_SHARE_EXCLUSIVE,
            0,
            &mut stream,
        ) != 0
        {
            return None;
        }
        let read: unsafe extern "system" fn(*mut c_void, *mut u8, u32, *mut u32) -> i32 =
            core::mem::transmute(method(stream, 3));
        let mut out = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let mut got = 0u32;
            let result = read(stream, chunk.as_mut_ptr(), chunk.len() as u32, &mut got);
            out.extend_from_slice(&chunk[..got as usize]);
            if result != 0 || got == 0 {
                break;
            }
        }
        release(stream);
        Some(out)
    }

    /// A package-shaped run of bytes long enough to need sectors of its own.
    fn package(seed: u8) -> Vec<u8> {
        let mut out = b"PK\x03\x04".to_vec();
        out.extend((0..9000u32).map(|at| (at as u8).wrapping_mul(seed)));
        out
    }

    pub(super) fn run() {
        // SAFETY: every call is to the documented functions with the
        // arguments they take; every object is released once and used only
        // before that; every handle comes from the call that makes it.
        unsafe {
            OleInitialize(null_mut());
            let embed_source = RegisterClipboardFormatW(wide("Embed Source").as_ptr());
            let descriptor = RegisterClipboardFormatW(wide("Object Descriptor").as_ptr());

            // --- The shell's copy, as OLE reads it ----------------------------
            let written = package(3);
            let contents = wp_shell::clipboard::Contents {
                text: Some("Copied".to_owned()),
                document: Some(written.clone()),
                ..wp_shell::clipboard::Contents::default()
            };
            println!("put: {}", wp_shell::clipboard::set_contents(&contents));

            let mut object = null_mut();
            println!("ole clipboard: {}", OleGetClipboard(&mut object) == 0);
            if object.is_null() {
                return;
            }
            let get_data: unsafe extern "system" fn(
                *mut c_void,
                *const FormatEtc,
                *mut Medium,
            ) -> i32 = core::mem::transmute(method(object, 3));
            let mut asked = FormatEtc {
                format: embed_source as u16,
                device: null_mut(),
                aspect: 1,
                index: -1,
                medium: TYMED_ISTORAGE,
            };
            let mut medium = Medium { kind: 0, handle: null_mut(), release: null_mut() };
            let mut storage = null_mut();
            if get_data(object, &asked, &mut medium) == 0 && medium.kind == TYMED_ISTORAGE {
                println!("embed source: a storage from OLE");
                storage = medium.handle;
            } else {
                // Not offered as a storage: taken as the bytes of one, and
                // opened by OLE's own reader of compound files.
                asked.medium = TYMED_HGLOBAL;
                medium = Medium { kind: 0, handle: null_mut(), release: null_mut() };
                if get_data(object, &asked, &mut medium) == 0 {
                    let mut bytes = null_mut();
                    CreateILockBytesOnHGlobal(medium.handle, 0, &mut bytes);
                    let opened = StgOpenStorageOnILockBytes(
                        bytes,
                        null_mut(),
                        STGM_READ | STGM_SHARE_EXCLUSIVE,
                        null_mut(),
                        0,
                        &mut storage,
                    );
                    println!("embed source: opened by OLE from its bytes: {}", opened == 0);
                }
            }
            if !storage.is_null() {
                let mut class = Guid { d1: 0, d2: 0, d3: 0, d4: [0; 8] };
                ReadClassStg(storage, &mut class);
                println!("class: word document: {}", class == WORD);
                let package = read_stream(storage, "Package");
                println!("package: the same: {}", package.as_deref() == Some(&written[..]));
                let names = read_stream(storage, "\u{1}CompObj").unwrap_or_default();
                let names = String::from_utf8_lossy(&names).into_owned();
                println!("comp obj: names word: {}", names.contains("Word.Document.12"));
                println!(
                    "ole stream: {}",
                    read_stream(storage, "\u{1}Ole").map(|s| s.len()) == Some(20)
                );
            }

            let asked = FormatEtc {
                format: descriptor as u16,
                device: null_mut(),
                aspect: 1,
                index: -1,
                medium: TYMED_HGLOBAL,
            };
            let mut medium = Medium { kind: 0, handle: null_mut(), release: null_mut() };
            if get_data(object, &asked, &mut medium) == 0 {
                let size = GlobalSize(medium.handle);
                let at = GlobalLock(medium.handle).cast::<u8>();
                let bytes = core::slice::from_raw_parts(at, size).to_vec();
                GlobalUnlock(medium.handle);
                let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
                let class = Guid {
                    d1: u32_at(4),
                    d2: u16::from_le_bytes([bytes[8], bytes[9]]),
                    d3: u16::from_le_bytes([bytes[10], bytes[11]]),
                    d4: bytes[12..20].try_into().unwrap(),
                };
                let name_at = u32_at(44) as usize;
                let units: Vec<u16> = bytes[name_at..]
                    .chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .take_while(|unit| *unit != 0)
                    .collect();
                println!(
                    "descriptor: class word: {}, called: {}",
                    class == WORD,
                    String::from_utf16_lossy(&units)
                );
                ReleaseStgMedium(&mut medium);
            }
            release(object);

            // --- A storage OLE made, as the shell reads it --------------------
            let given = package(7);
            let mut bytes = null_mut();
            CreateILockBytesOnHGlobal(null_mut(), 0, &mut bytes);
            let mut storage = null_mut();
            StgCreateDocfileOnILockBytes(
                bytes,
                STGM_CREATE | STGM_READWRITE | STGM_SHARE_EXCLUSIVE,
                0,
                &mut storage,
            );
            WriteClassStg(storage, &WORD);
            let create: unsafe extern "system" fn(
                *mut c_void,
                *const u16,
                u32,
                u32,
                u32,
                *mut *mut c_void,
            ) -> i32 = core::mem::transmute(method(storage, 3));
            let mut stream = null_mut();
            let name = wide("Package");
            create(
                storage,
                name.as_ptr(),
                STGM_CREATE | STGM_WRITE | STGM_SHARE_EXCLUSIVE,
                0,
                0,
                &mut stream,
            );
            let write: unsafe extern "system" fn(*mut c_void, *const u8, u32, *mut u32) -> i32 =
                core::mem::transmute(method(stream, 4));
            let mut wrote = 0u32;
            write(stream, given.as_ptr(), given.len() as u32, &mut wrote);
            release(stream);
            let commit: unsafe extern "system" fn(*mut c_void, u32) -> i32 =
                core::mem::transmute(method(storage, 9));
            commit(storage, 0);
            release(storage);
            let mut memory = null_mut();
            GetHGlobalFromILockBytes(bytes, &mut memory);
            let size = GlobalSize(memory);
            let source = GlobalLock(memory).cast::<u8>();
            let file = core::slice::from_raw_parts(source, size).to_vec();
            GlobalUnlock(memory);
            release(bytes);
            println!(
                "made by OLE: a compound file: {}",
                file.starts_with(&[0xD0, 0xCF, 0x11, 0xE0])
            );

            let put = |format: u32, data: &[u8]| {
                let memory = GlobalAlloc(GMEM_MOVEABLE, data.len());
                let at = GlobalLock(memory).cast::<u8>();
                core::ptr::copy_nonoverlapping(data.as_ptr(), at, data.len());
                GlobalUnlock(memory);
                SetClipboardData(format, memory);
            };
            OpenClipboard(null_mut());
            EmptyClipboard();
            put(embed_source, &file);
            let text: Vec<u8> =
                wide("Copied in Word").iter().flat_map(|u| u.to_le_bytes()).collect();
            put(CF_UNICODETEXT, &text);
            CloseClipboard();
            let read = wp_shell::clipboard::contents();
            println!("read back: the same: {}", read.document.as_deref() == Some(&given[..]));
        }
    }
}
