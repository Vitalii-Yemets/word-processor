//! Sway's own interface, for putting this program's windows side by side.
//!
//! # Why the compositor is asked directly
//!
//! Because on Wayland a program cannot place its own window — where a
//! window goes is the compositor's to say, which is the protocol's design —
//! and the desktop's portal, which does for a program what it may not do
//! itself, has no interface for arranging windows either: the portal's
//! list is screenshots, screen recording, files, printing and the like, and
//! nothing that moves a window. So Word's Arrange All can only be asked of
//! the compositor itself, in whatever language that compositor speaks. Sway
//! speaks i3's: a socket named in `SWAYSOCK`, messages framed with a magic
//! word, a length and a type, and commands and answers in JSON.
//!
//! # What side by side means to sway
//!
//! Sway tiles: a window's place is its place in a tree of containers, each
//! laying its children out across or down. So the program's windows are
//! made tiled if they were floating, gathered as neighbours in one
//! container — each moved to a mark set on the one before, which keeps
//! them in order — and that container is made to lay them out across. Other
//! programs' windows are not moved, since a person arranging their own
//! documents has not asked for everything else to be rearranged.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use super::json::Json;

/// What every message begins with.
const MAGIC: &[u8; 6] = b"i3-ipc";
/// The two kinds of message sent: a command, and asking for the tree.
const RUN_COMMAND: u32 = 0;
const GET_TREE: u32 = 4;

/// Puts this program's windows side by side, in the order sway holds them.
/// How many there were, or nothing where there is no sway to ask.
pub(crate) fn arrange(pid: u32) -> Option<usize> {
    let mut socket = UnixStream::connect(std::env::var_os("SWAYSOCK")?).ok()?;
    let _ = socket.set_read_timeout(Some(Duration::from_secs(5)));
    let tree = Json::parse(&ask(&mut socket, GET_TREE, "")?)?;
    let mut windows = Vec::new();
    windows_of(&tree, pid, &mut windows);
    if windows.is_empty() {
        return Some(0);
    }
    let answer = ask(&mut socket, RUN_COMMAND, &commands(&windows, pid))?;
    let answers = Json::parse(&answer)?;
    let done = answers
        .elements()
        .iter()
        .all(|each| each.get("success").and_then(Json::as_bool) == Some(true));
    done.then_some(windows.len())
}

/// The commands that gather the windows, tiled and in order, into one
/// container laid out across.
fn commands(windows: &[i64], pid: u32) -> String {
    let mark = format!("wp-arrange-{pid}");
    let mut out = Vec::new();
    for (index, id) in windows.iter().enumerate() {
        out.push(format!("[con_id={id}] floating disable"));
        if index > 0 {
            let before = windows[index - 1];
            out.push(format!("[con_id={id}] move container to mark {mark}"));
            out.push(format!("[con_id={before}] unmark {mark}"));
        }
        out.push(format!("[con_id={id}] mark --add {mark}"));
    }
    if let (Some(first), Some(last)) = (windows.first(), windows.last()) {
        out.push(format!("[con_id={first}] layout splith"));
        out.push(format!("[con_id={last}] unmark {mark}"));
    }
    out.join("; ")
}

/// The windows of a program, found in sway's tree: a window is a container
/// with the program's process number on it, tiled or floating.
fn windows_of(node: &Json, pid: u32, found: &mut Vec<i64>) {
    let ours = node.get("pid").and_then(Json::as_i64) == Some(i64::from(pid));
    let window = matches!(node.get("type").and_then(Json::as_str), Some("con" | "floating_con"));
    if ours && window {
        if let Some(id) = node.get("id").and_then(Json::as_i64) {
            found.push(id);
        }
    }
    for key in ["nodes", "floating_nodes"] {
        for child in node.get(key).map(Json::elements).unwrap_or_default() {
            windows_of(child, pid, found);
        }
    }
}

/// Sends one message and reads its answer.
fn ask(socket: &mut UnixStream, kind: u32, payload: &str) -> Option<String> {
    let mut message = Vec::with_capacity(14 + payload.len());
    message.extend_from_slice(MAGIC);
    message.extend_from_slice(&u32::try_from(payload.len()).ok()?.to_ne_bytes());
    message.extend_from_slice(&kind.to_ne_bytes());
    message.extend_from_slice(payload.as_bytes());
    socket.write_all(&message).ok()?;

    let mut header = [0u8; 14];
    socket.read_exact(&mut header).ok()?;
    if &header[..6] != MAGIC {
        return None;
    }
    let length = u32::from_ne_bytes([header[6], header[7], header[8], header[9]]) as usize;
    let mut body = vec![0u8; length];
    socket.read_exact(&mut body).ok()?;
    String::from_utf8(body).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_s_windows_are_found_tiled_or_floating_and_no_one_else_s() {
        let tree = Json::parse(
            r#"{"id": 1, "type": "root", "nodes": [{"id": 2, "type": "output", "nodes": [
                {"id": 3, "type": "workspace", "nodes": [
                    {"id": 10, "type": "con", "pid": 77, "nodes": []},
                    {"id": 11, "type": "con", "pid": 99, "nodes": []}],
                 "floating_nodes": [{"id": 12, "type": "floating_con", "pid": 77}]}]}]}"#,
        )
        .expect("a tree");
        let mut found = Vec::new();
        windows_of(&tree, 77, &mut found);
        assert_eq!(found, [10, 12]);
    }

    #[test]
    fn the_windows_are_gathered_in_order_and_laid_across() {
        assert_eq!(
            commands(&[10, 12, 15], 7),
            "[con_id=10] floating disable; [con_id=10] mark --add wp-arrange-7; \
             [con_id=12] floating disable; [con_id=12] move container to mark wp-arrange-7; \
             [con_id=10] unmark wp-arrange-7; [con_id=12] mark --add wp-arrange-7; \
             [con_id=15] floating disable; [con_id=15] move container to mark wp-arrange-7; \
             [con_id=12] unmark wp-arrange-7; [con_id=15] mark --add wp-arrange-7; \
             [con_id=10] layout splith; [con_id=15] unmark wp-arrange-7"
        );
    }
}
