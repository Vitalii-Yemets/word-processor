//! The desktop's portal: what a program asks the desktop to do for it, over
//! D-Bus, where it may not do the thing itself.
//!
//! # Why a screenshot has to be asked for
//!
//! Because on Wayland a program sees its own windows and nothing else. That
//! is the protocol's rule and the point of it — a program that could read
//! the screen could read the password somebody else is typing — so Word's
//! Screenshot button, which on X or Windows photographs the screen itself,
//! has to ask the desktop instead. The desktop's answer is the portal:
//! `org.freedesktop.portal.Screenshot`, which the desktop carries out in
//! its own way — photographing the whole screen, or letting the person drag
//! out the rectangle they want, which is Word's Screen Clipping — and hands
//! back as a file.
//!
//! # How the answer comes back
//!
//! Not as the call's reply. The call is answered at once with the path of
//! a request object, and the picture follows, whenever the person has
//! finished choosing, as that object's `Response` signal: a number saying
//! whether it was done, cancelled or failed, and the file's address. The
//! path is one the program can work out in advance from its name on the bus
//! and a token of its own, and the bus is told to pass the signal on before
//! the call goes, so that a desktop that answers at once is not answering
//! into nothing.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use super::dbus::{self, Connection, Kind, Message, Value};

const DESKTOP: &str = "org.freedesktop.portal.Desktop";
const DESKTOP_PATH: &str = "/org/freedesktop/portal/desktop";
const SCREENSHOT: &str = "org.freedesktop.portal.Screenshot";

/// How a screenshot is to be taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Taking {
    /// The whole screen, with no questions.
    Screen,
    /// A rectangle the person drags out.
    Clipping,
}

/// The session bus, where the portal is.
fn session() -> Option<Connection> {
    Connection::open(&dbus::session_address()?).ok()
}

/// Whether the desktop's portal will take a screenshot at all.
pub(crate) fn offers_screenshot() -> bool {
    let Some(mut bus) = session() else { return false };
    let asked = Message::call(
        DESKTOP,
        DESKTOP_PATH,
        "org.freedesktop.DBus.Properties",
        "Get",
        vec![Value::str(SCREENSHOT), Value::str("version")],
    );
    bus.call(&asked, Duration::from_secs(3)).is_ok()
}

/// A screenshot, as the portal hands it over: the bytes of the file it
/// wrote, which is a PNG. Nothing if there is no portal, or the person
/// cancelled, or the desktop could not do it.
pub(crate) fn screenshot(taking: Taking) -> Option<Vec<u8>> {
    static ASKED: AtomicU32 = AtomicU32::new(0);
    let mut bus = session()?;
    let token = format!("wp{}_{}", std::process::id(), ASKED.fetch_add(1, Ordering::Relaxed));
    let expected = request_path(&bus.name, &token);
    listen_for_response(&mut bus, &expected)?;

    let interactive = taking == Taking::Clipping;
    let options = vec![
        option("handle_token", Value::str(&token)),
        option("interactive", Value::Bool(interactive)),
        option("modal", Value::Bool(true)),
    ];
    let call = Message::call(
        DESKTOP,
        DESKTOP_PATH,
        SCREENSHOT,
        "Screenshot",
        // No parent window: naming one on Wayland takes a protocol this
        // program does not speak, and the portal takes none as a window
        // of no one's.
        vec![Value::str(""), Value::Array("{sv}".to_owned(), options)],
    );
    let reply = bus.call(&call, Duration::from_secs(10)).ok()?;
    let handle = reply.body.first().and_then(Value::as_str)?.to_owned();
    // An old portal names the request itself; it is listened for too.
    if handle != expected {
        listen_for_response(&mut bus, &handle)?;
    }

    // A person dragging out a rectangle takes their time; the whole screen
    // does not.
    let wait = match taking {
        Taking::Screen => Duration::from_secs(30),
        Taking::Clipping => Duration::from_secs(300),
    };
    let response = bus.wait_for(wait, |message| {
        message.kind == Kind::Signal
            && message.member.as_deref() == Some("Response")
            && message.path.as_deref() == Some(handle.as_str())
    })?;
    // Nought is done; one is cancelled; two is anything else.
    if response.body.first().and_then(Value::as_i64) != Some(0) {
        return None;
    }
    let address = response.body.get(1).and_then(|results| result(results, "uri"))?;
    let path = file_of(&address)?;
    std::fs::read(path).ok()
}

/// Where the portal will put the request for a call made with a token:
/// under the caller's name on the bus, its dots made underscores.
fn request_path(name: &str, token: &str) -> String {
    let sender = name.trim_start_matches(':').replace('.', "_");
    format!("{DESKTOP_PATH}/request/{sender}/{token}")
}

/// Asks the bus to pass on the answer to a request.
fn listen_for_response(bus: &mut Connection, path: &str) -> Option<()> {
    let rule = format!(
        "type='signal',interface='org.freedesktop.portal.Request',member='Response',path='{path}'"
    );
    let asked = Message::call(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "AddMatch",
        vec![Value::str(&rule)],
    );
    bus.call(&asked, Duration::from_secs(3)).ok().map(|_| ())
}

/// One entry of a dictionary of options.
fn option(key: &str, value: Value) -> Value {
    Value::DictEntry(Box::new(Value::str(key)), Box::new(Value::Variant(Box::new(value))))
}

/// A string out of a dictionary of results.
fn result(results: &Value, key: &str) -> Option<String> {
    results.items().iter().find_map(|entry| match entry {
        Value::DictEntry(found, value) if found.as_str() == Some(key) => {
            value.inner().as_str().map(str::to_owned)
        }
        _ => None,
    })
}

/// The file a `file:` address names on this machine.
fn file_of(address: &str) -> Option<PathBuf> {
    super::files::paths_of_uri_list(address).into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_is_found_where_the_portal_will_put_it() {
        assert_eq!(
            request_path(":1.42", "wp7_0"),
            "/org/freedesktop/portal/desktop/request/1_42/wp7_0"
        );
    }

    #[test]
    fn the_file_is_read_out_of_the_results() {
        let results = Value::Array(
            "{sv}".to_owned(),
            vec![option("uri", Value::str("file:///home/ann/Pictures/Screenshot%20one.png"))],
        );
        let address = result(&results, "uri").expect("the address");
        assert_eq!(file_of(&address), Some(PathBuf::from("/home/ann/Pictures/Screenshot one.png")));
        assert_eq!(result(&results, "other"), None);
    }
}
