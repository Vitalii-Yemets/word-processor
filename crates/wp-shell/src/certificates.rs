//! The certificates the machine trusts.
//!
//! # Why this is asked of the system and not answered here
//!
//! A signature is worth something because somebody the machine already trusts
//! stands behind the certificate that made it. Who that is was decided by
//! whoever set the machine up — the operating system ships a list, an
//! organisation adds its own, a person can add one. A program that carried its
//! own list would be granting a trust nobody asked it to grant, and a person
//! who stopped trusting an issuer on their machine would find this program
//! still trusting it.
//!
//! So the list comes from the machine, and this module is the two ways of
//! asking for it.
//!
//! # Windows
//!
//! `CertOpenSystemStoreW` on the store called ROOT, and then every certificate
//! in it. The functions are found at run time rather than linked, for the
//! reason the rest of this crate finds its newer functions at run time: a
//! program that will not start because a library is missing is worse than one
//! that says it could not read the store.
//!
//! # Linux
//!
//! There is no one answer, which is why the list here is a list. Every
//! distribution keeps the same certificates somewhere slightly different: one
//! bundle of them all, or a directory of one file each, under `/etc/ssl` or
//! `/etc/pki`. Whichever is there is read; they are all the same format, which
//! is PEM — base 64 between two marker lines.
//!
//! `SSL_CERT_FILE` and `SSL_CERT_DIR` are looked at first, because that is how
//! everything else on the system is told to look somewhere else.

/// Every certificate the machine trusts, each as the bytes it was written in.
///
/// Empty where the store cannot be read, which is not the same as a machine
/// that trusts nobody — and the difference matters, so whoever calls this is
/// expected to say "this machine's list of trusted issuers could not be read"
/// rather than "this certificate is not trusted".
#[must_use]
pub fn trusted_roots() -> Vec<Vec<u8>> {
    #[cfg(windows)]
    {
        windows_roots()
    }
    #[cfg(not(windows))]
    {
        unix_roots()
    }
}

/// Reads every certificate out of a PEM file's text.
///
/// A PEM file holds any number of them, one after another, and may hold other
/// things as well — keys, parameters, comments between them. Only the blocks
/// marked as certificates are taken.
#[must_use]
pub fn from_pem(text: &str) -> Vec<Vec<u8>> {
    const OPEN: &str = "-----BEGIN CERTIFICATE-----";
    const CLOSE: &str = "-----END CERTIFICATE-----";

    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(OPEN) {
        let body = &rest[start + OPEN.len()..];
        let Some(end) = body.find(CLOSE) else { break };
        let base64: String = body[..end].chars().filter(|c| !c.is_whitespace()).collect();
        let der = decode_base64(base64.as_bytes());
        if !der.is_empty() {
            out.push(der);
        }
        rest = &body[end + CLOSE.len()..];
    }
    out
}

/// Base 64 back into bytes.
///
/// Written here rather than borrowed from the crate that has one: this crate
/// is the one that talks to operating systems and depends on nothing that
/// does not have to, and forty lines is cheaper than a dependency in the
/// other direction.
fn decode_base64(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut held = 0u32;
    let mut bits = 0u32;
    for byte in text {
        let value = match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a') + 26,
            b'0'..=b'9' => u32::from(byte - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => continue,
        };
        held = (held << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((held >> bits) & 0xFF) as u8);
        }
    }
    out
}

#[cfg(not(windows))]
fn unix_roots() -> Vec<Vec<u8>> {
    /// One bundle holding all of them, wherever this system keeps it.
    const BUNDLES: &[&str] = &[
        "/etc/ssl/certs/ca-certificates.crt",
        "/etc/pki/tls/certs/ca-bundle.crt",
        "/etc/ssl/ca-bundle.pem",
        "/etc/ssl/cert.pem",
        "/usr/local/share/certs/ca-root-nss.crt",
    ];
    /// Or a directory with one file each.
    const DIRECTORIES: &[&str] = &["/etc/ssl/certs", "/etc/pki/tls/certs", "/etc/ca-certificates"];

    let mut out = Vec::new();
    let mut seen: Vec<Vec<u8>> = Vec::new();

    let told = std::env::var("SSL_CERT_FILE").ok().filter(|value| !value.is_empty());
    for path in told.iter().map(String::as_str).chain(BUNDLES.iter().copied()) {
        if let Ok(text) = std::fs::read_to_string(path) {
            take(from_pem(&text), &mut out, &mut seen);
            // One bundle is the whole list; there is no point reading a
            // second copy of the same certificates under another name.
            if !out.is_empty() {
                return out;
            }
        }
    }

    let told = std::env::var("SSL_CERT_DIR").ok().filter(|value| !value.is_empty());
    for folder in told.iter().map(String::as_str).chain(DIRECTORIES.iter().copied()) {
        let Ok(entries) = std::fs::read_dir(folder) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            // A directory of certificates is full of symbolic links named by
            // hash, each pointing at one of the files beside it; reading both
            // is reading everything twice, and `seen` is what stops that.
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            take(from_pem(&text), &mut out, &mut seen);
        }
        if !out.is_empty() {
            return out;
        }
    }
    out
}

/// Adds what has not been seen before.
#[cfg(not(windows))]
fn take(found: Vec<Vec<u8>>, out: &mut Vec<Vec<u8>>, seen: &mut Vec<Vec<u8>>) {
    for der in found {
        if seen.contains(&der) {
            continue;
        }
        seen.push(der.clone());
        out.push(der);
    }
}

#[cfg(windows)]
fn windows_roots() -> Vec<Vec<u8>> {
    use core::ffi::c_void;

    /// What `CertEnumCertificatesInStore` hands back: the encoding, a pointer
    /// to the bytes, how many there are, and things this does not use.
    #[repr(C)]
    struct CertContext {
        encoding: u32,
        encoded: *const u8,
        length: u32,
        info: *const c_void,
        store: *const c_void,
    }

    type OpenStore = unsafe extern "system" fn(*const u16, *const u16) -> *const c_void;
    type NextCertificate =
        unsafe extern "system" fn(*const c_void, *const CertContext) -> *const CertContext;
    type CloseStore = unsafe extern "system" fn(*const c_void, u32) -> i32;

    let mut out = Vec::new();
    unsafe {
        let open = crate::windows::library_function("crypt32.dll", b"CertOpenSystemStoreW\0");
        let next =
            crate::windows::library_function("crypt32.dll", b"CertEnumCertificatesInStore\0");
        let close = crate::windows::library_function("crypt32.dll", b"CertCloseStore\0");
        if open.is_null() || next.is_null() || close.is_null() {
            return out;
        }
        let open: OpenStore = core::mem::transmute(open);
        let next: NextCertificate = core::mem::transmute(next);
        let close: CloseStore = core::mem::transmute(close);

        let name: Vec<u16> = "ROOT\0".encode_utf16().collect();
        let store = open(core::ptr::null(), name.as_ptr());
        if store.is_null() {
            return out;
        }

        let mut context = next(store, core::ptr::null());
        while !context.is_null() {
            let found = &*context;
            if !found.encoded.is_null() && found.length > 0 {
                out.push(
                    core::slice::from_raw_parts(found.encoded, found.length as usize).to_vec(),
                );
            }
            context = next(store, context);
        }
        // Every context the walk handed back was freed by the walk itself,
        // which is what passing the last one back in means.
        close(store, 0);
    }
    out
}

// --- The person's own certificates, which the system holds ------------------

/// One of the person's own certificates, as the system has it.
///
/// The certificate itself travels — it is public, and it goes into the
/// signature — but the key does not and cannot: what comes back from
/// [`sign_with_held`] is a signature, made by the system, over bytes handed to
/// it. That is the whole point of a store: a key that can be copied out is a
/// key that has been copied out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    /// The certificate as it was written, which is what goes into a signature.
    pub certificate: Vec<u8>,
    /// What the system calls it, which is what a person recognises it by.
    pub name: String,
}

/// Every certificate in the person's own store that has a key to sign with.
///
/// Empty on a system with no such store, which is every system but Windows:
/// there is no one place a person's certificates live on Linux, and inventing
/// one would be inventing a store rather than reading one. What is read there
/// is a folder, and that is [`wp_app`]'s business rather than this crate's.
///
/// Certificates without a key are left out. A certificate whose key is
/// somewhere else is somebody else's certificate as far as signing goes, and
/// offering it would be offering to do something that cannot be done.
#[must_use]
pub fn held_certificates() -> Vec<Held> {
    #[cfg(windows)]
    {
        windows_held()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Signs a hash with the key the system holds for a certificate.
///
/// `hash` is the digest itself and not the message: this is the one place in
/// the program where the hashing and the signing happen on opposite sides of a
/// wall, because the system will sign a hash and will not be handed a
/// document. `algorithm` is what the system calls the hash — `SHA256` and the
/// like — which is how it knows what to write into the padding.
///
/// `None` where there is no such store, no such certificate in it, no key
/// behind that certificate, or the system refused. Which of those it was is
/// not reported, because the only one a person can do anything about is the
/// certificate, and they picked that off a list this module made.
#[must_use]
pub fn sign_with_held(certificate: &[u8], algorithm: &str, hash: &[u8]) -> Option<Vec<u8>> {
    #[cfg(windows)]
    {
        windows_sign(certificate, algorithm, hash)
    }
    #[cfg(not(windows))]
    {
        let _ = (certificate, algorithm, hash);
        None
    }
}

/// What Windows calls the store a person's own certificates are in.
///
/// Word signs from this one, and so does everything else on the machine that
/// signs as the person rather than as the machine.
#[cfg(windows)]
const PERSONAL: &str = "MY\0";

/// The shape `CertEnumCertificatesInStore` hands back.
///
/// Named here as well as in [`windows_roots`] because the two walks are
/// otherwise unrelated and a shared private type between them would be one
/// more thing to keep in step for no gain.
#[cfg(windows)]
#[repr(C)]
struct CertContext {
    encoding: u32,
    encoded: *const u8,
    length: u32,
    info: *const core::ffi::c_void,
    store: *const core::ffi::c_void,
}

/// What `NCryptSignHash` is told about the padding.
///
/// PKCS#1 v1.5, which is what a `.docx` signature uses and what
/// [`wp_rsa`] does on the other path. The one field is the name of the hash,
/// which the padding writes down beside the digest.
#[cfg(windows)]
#[repr(C)]
struct Pkcs1PaddingInfo {
    algorithm: *const u16,
}

/// Every certificate in the personal store, with a key, as the system has it.
#[cfg(windows)]
fn windows_held() -> Vec<Held> {
    use core::ffi::c_void;

    type OpenStore = unsafe extern "system" fn(*const u16, *const u16) -> *const c_void;
    type NextCertificate =
        unsafe extern "system" fn(*const c_void, *const CertContext) -> *const CertContext;
    type CloseStore = unsafe extern "system" fn(*const c_void, u32) -> i32;
    type ContextProperty =
        unsafe extern "system" fn(*const CertContext, u32, *mut c_void, *mut u32) -> i32;
    type NameString = unsafe extern "system" fn(
        *const CertContext,
        u32,
        u32,
        *const c_void,
        *mut u16,
        u32,
    ) -> u32;

    /// The property that says a certificate has a key behind it, and where.
    const KEY_PROVIDER: u32 = 2;
    /// The name a person would recognise, which is what Word's own list shows.
    const SIMPLE_DISPLAY_NAME: u32 = 4;

    let mut out = Vec::new();
    unsafe {
        let open = crate::windows::library_function("crypt32.dll", b"CertOpenSystemStoreW\0");
        let next =
            crate::windows::library_function("crypt32.dll", b"CertEnumCertificatesInStore\0");
        let close = crate::windows::library_function("crypt32.dll", b"CertCloseStore\0");
        let property =
            crate::windows::library_function("crypt32.dll", b"CertGetCertificateContextProperty\0");
        let named = crate::windows::library_function("crypt32.dll", b"CertGetNameStringW\0");
        if open.is_null()
            || next.is_null()
            || close.is_null()
            || property.is_null()
            || named.is_null()
        {
            return out;
        }
        let open: OpenStore = core::mem::transmute(open);
        let next: NextCertificate = core::mem::transmute(next);
        let close: CloseStore = core::mem::transmute(close);
        let property: ContextProperty = core::mem::transmute(property);
        let named: NameString = core::mem::transmute(named);

        let store_name: Vec<u16> = PERSONAL.encode_utf16().collect();
        let store = open(core::ptr::null(), store_name.as_ptr());
        if store.is_null() {
            return out;
        }

        let mut context = next(store, core::ptr::null());
        while !context.is_null() {
            let found = &*context;
            if !found.encoded.is_null() && found.length > 0 {
                // Whether there is a key behind it, asked of the property
                // rather than by acquiring the key: acquiring one can put a
                // dialog on the screen asking for a smart card, and a list
                // being drawn is no place for that.
                let mut size = 0u32;
                let has_key =
                    property(context, KEY_PROVIDER, core::ptr::null_mut(), &mut size) != 0;
                if has_key {
                    let mut name = vec![0u16; 256];
                    let written = named(
                        context,
                        SIMPLE_DISPLAY_NAME,
                        0,
                        core::ptr::null(),
                        name.as_mut_ptr(),
                        name.len() as u32,
                    );
                    // What comes back counts the terminator, and a name of
                    // nothing but a terminator is no name at all.
                    let name = if written > 1 {
                        String::from_utf16_lossy(&name[..(written - 1) as usize])
                    } else {
                        String::new()
                    };
                    out.push(Held {
                        certificate: core::slice::from_raw_parts(
                            found.encoded,
                            found.length as usize,
                        )
                        .to_vec(),
                        name,
                    });
                }
            }
            context = next(store, context);
        }
        // Every context the walk handed back was freed by the walk itself,
        // which is what passing the last one back in means.
        close(store, 0);
    }
    out
}

/// Hands a hash to the system and takes back a signature.
#[cfg(windows)]
fn windows_sign(certificate: &[u8], algorithm: &str, hash: &[u8]) -> Option<Vec<u8>> {
    use core::ffi::c_void;

    type OpenStore = unsafe extern "system" fn(*const u16, *const u16) -> *const c_void;
    type NextCertificate =
        unsafe extern "system" fn(*const c_void, *const CertContext) -> *const CertContext;
    type CloseStore = unsafe extern "system" fn(*const c_void, u32) -> i32;
    type FreeContext = unsafe extern "system" fn(*const CertContext) -> i32;
    type DuplicateContext = unsafe extern "system" fn(*const CertContext) -> *const CertContext;
    type AcquireKey = unsafe extern "system" fn(
        *const CertContext,
        u32,
        *const c_void,
        *mut usize,
        *mut u32,
        *mut i32,
    ) -> i32;
    type SignHash = unsafe extern "system" fn(
        usize,
        *const c_void,
        *const u8,
        u32,
        *mut u8,
        u32,
        *mut u32,
        u32,
    ) -> i32;
    type FreeObject = unsafe extern "system" fn(usize) -> i32;

    /// Ask for a key of the newer sort, which is the one `NCryptSignHash`
    /// takes. The older interface is a different call with a different handle,
    /// and a certificate whose key is only reachable that way is left alone
    /// rather than half-supported.
    const ONLY_NEWER_KEY: u32 = 0x0004_0000;
    /// And do not put a dialog on the screen. A program that signs should ask
    /// before it signs, not while it is signing; where the key needs a card or
    /// a PIN this fails and says so, which is the truthful answer.
    const WITHOUT_ASKING: u32 = 0x0000_0040;
    /// Padding as PKCS#1 v1.5, which is what a `.docx` signature is.
    const PKCS1_PADDING: u32 = 0x0000_0002;

    unsafe {
        let open = crate::windows::library_function("crypt32.dll", b"CertOpenSystemStoreW\0");
        let next =
            crate::windows::library_function("crypt32.dll", b"CertEnumCertificatesInStore\0");
        let close = crate::windows::library_function("crypt32.dll", b"CertCloseStore\0");
        let free_context =
            crate::windows::library_function("crypt32.dll", b"CertFreeCertificateContext\0");
        let duplicate =
            crate::windows::library_function("crypt32.dll", b"CertDuplicateCertificateContext\0");
        let acquire =
            crate::windows::library_function("crypt32.dll", b"CryptAcquireCertificatePrivateKey\0");
        let sign = crate::windows::library_function("ncrypt.dll", b"NCryptSignHash\0");
        let free_key = crate::windows::library_function("ncrypt.dll", b"NCryptFreeObject\0");
        if open.is_null()
            || next.is_null()
            || close.is_null()
            || free_context.is_null()
            || duplicate.is_null()
            || acquire.is_null()
            || sign.is_null()
            || free_key.is_null()
        {
            return None;
        }
        let open: OpenStore = core::mem::transmute(open);
        let next: NextCertificate = core::mem::transmute(next);
        let close: CloseStore = core::mem::transmute(close);
        let free_context: FreeContext = core::mem::transmute(free_context);
        let duplicate: DuplicateContext = core::mem::transmute(duplicate);
        let acquire: AcquireKey = core::mem::transmute(acquire);
        let sign: SignHash = core::mem::transmute(sign);
        let free_key: FreeObject = core::mem::transmute(free_key);

        let store_name: Vec<u16> = PERSONAL.encode_utf16().collect();
        let store = open(core::ptr::null(), store_name.as_ptr());
        if store.is_null() {
            return None;
        }

        // The certificate is found by its own bytes. Windows has ways of
        // searching a store, and every one of them wants a different handle
        // or a hash worked out first; the bytes are what the caller already
        // has, and a store holds tens of certificates rather than thousands.
        let mut wanted = core::ptr::null();
        let mut context = next(store, core::ptr::null());
        while !context.is_null() {
            let found = &*context;
            let same = !found.encoded.is_null()
                && found.length as usize == certificate.len()
                && core::slice::from_raw_parts(found.encoded, found.length as usize) == certificate;
            if same {
                // Kept out of the walk, because the walk frees whatever it is
                // handed next and this one has to outlive it.
                wanted = duplicate(context);
                break;
            }
            context = next(store, context);
        }
        if !context.is_null() {
            // The walk was stopped early, so the context it stopped on is
            // this code's to free rather than the walk's.
            free_context(context);
        }
        close(store, 0);
        if wanted.is_null() {
            return None;
        }

        let mut key = 0usize;
        let mut kind = 0u32;
        let mut ours = 0i32;
        let got = acquire(
            wanted,
            ONLY_NEWER_KEY | WITHOUT_ASKING,
            core::ptr::null(),
            &mut key,
            &mut kind,
            &mut ours,
        );
        free_context(wanted);
        if got == 0 || key == 0 {
            return None;
        }

        let named: Vec<u16> = format!("{algorithm}\0").encode_utf16().collect();
        let padding = Pkcs1PaddingInfo { algorithm: named.as_ptr() };
        let padding_ptr: *const c_void = core::ptr::from_ref(&padding).cast();

        // Asked twice, as every Windows call that returns a buffer is: once
        // for the length, once for the bytes.
        let mut needed = 0u32;
        let measured = sign(
            key,
            padding_ptr,
            hash.as_ptr(),
            hash.len() as u32,
            core::ptr::null_mut(),
            0,
            &mut needed,
            PKCS1_PADDING,
        );
        if measured != 0 || needed == 0 {
            if ours != 0 {
                free_key(key);
            }
            return None;
        }

        let mut signature = vec![0u8; needed as usize];
        let mut written = 0u32;
        let made = sign(
            key,
            padding_ptr,
            hash.as_ptr(),
            hash.len() as u32,
            signature.as_mut_ptr(),
            signature.len() as u32,
            &mut written,
            PKCS1_PADDING,
        );
        if ours != 0 {
            free_key(key);
        }
        if made != 0 {
            return None;
        }
        signature.truncate(written as usize);
        Some(signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A certificate is a sequence, which starts with 0x30. Enough to say the
    /// base 64 came apart into the right sort of thing.
    fn looks_like_a_certificate(der: &[u8]) -> bool {
        der.first() == Some(&0x30) && der.len() > 100
    }

    #[test]
    fn a_pem_file_with_nothing_in_it_holds_no_certificates() {
        assert!(from_pem("").is_empty());
        assert!(from_pem("nothing here at all").is_empty());
        assert!(from_pem("-----BEGIN CERTIFICATE-----\nAAAA").is_empty(), "no end marker");
    }

    #[test]
    fn base_64_comes_apart_the_way_it_went_together() {
        // The standard's own examples, which have all three lengths in them.
        assert_eq!(decode_base64(b"TWFu"), b"Man");
        assert_eq!(decode_base64(b"TWE="), b"Ma");
        assert_eq!(decode_base64(b"TQ=="), b"M");
        assert_eq!(decode_base64(b"TWFuTWFu"), b"ManMan");
        // And the whitespace a PEM file is full of.
        assert_eq!(decode_base64(b"TWFu\n TWFu\r\n"), b"ManMan");
    }

    #[test]
    fn the_personal_store_is_read_or_is_empty_and_never_a_crash() {
        // The same bargain the roots are read on, and the one that matters
        // more here: a machine with no such store has to come back with
        // nothing rather than with a guess, because what is listed is what a
        // person will be offered to sign with.
        for held in held_certificates() {
            assert!(looks_like_a_certificate(&held.certificate), "not a certificate");
        }
    }

    #[test]
    fn signing_with_a_certificate_no_store_holds_is_refused() {
        // Not answered with an empty signature or a panic: a signature that
        // was not made has to come back as one that was not made. On a
        // machine with a store this asks it about a certificate that is not
        // in it, and on one without it asks nothing at all; both say no.
        let nobodys = vec![0x30, 0x82, 0x01, 0x00];
        assert_eq!(sign_with_held(&nobodys, "SHA256", &[0u8; 32]), None);
    }

    #[test]
    fn the_machines_own_store_is_read_or_says_nothing() {
        // What this asserts is that asking does not panic and that whatever
        // comes back is certificate-shaped. How many there are is the
        // machine's business: a build container has a full list, and a
        // stripped one may have none.
        for der in trusted_roots() {
            assert!(looks_like_a_certificate(&der), "something that is not a certificate");
        }
    }

    #[test]
    #[cfg(not(windows))]
    fn the_build_image_has_a_list_and_it_is_read() {
        // The image this is tested in carries ca-certificates, so an empty
        // answer here is a bug in the reading and not a bare machine.
        let roots = trusted_roots();
        assert!(roots.len() > 20, "only {} certificates were read", roots.len());
    }
}
