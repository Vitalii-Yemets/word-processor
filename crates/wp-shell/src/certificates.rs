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
