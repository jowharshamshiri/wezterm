//! `wsb-wezterm cli relay` against a real `wsb-wezterm-mux-server`, on a socket of
//! its own. The server is the one built beside this crate's `wsb-wezterm`:
//! `cargo build -p wezterm-mux-server` first.
#![cfg(unix)]

mod common;

use common::{Server, WAIT};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

#[test]
fn a_relay_mirrors_a_pane_and_types_into_it() {
    let server = Server::start();
    let pane = server.panes()[0]["pane_id"].as_u64().unwrap();
    let mut relay = server.relay(pane, &[]);

    let hello = relay.expect("hello");
    assert_eq!(
        (hello["cols"].as_u64(), hello["rows"].as_u64()),
        (Some(40), Some(10))
    );
    assert_eq!(hello["palette"]["ansi"].as_array().unwrap().len(), 16);
    assert_eq!(hello["title"], "cat");
    assert_eq!(hello["sized_by_input"], false);

    let clients: Vec<Value> =
        serde_json::from_str(&server.cli(&["list-clients", "--format", "json"])).unwrap();
    assert_eq!(
        clients
            .iter()
            .map(|c| c["role"].clone())
            .collect::<Vec<_>>(),
        vec![json!("relay")]
    );

    // The terminal echoes what is typed, and cat prints it again.
    relay.send(json!({"t": "text", "text": "hi"}));
    relay.send(json!({"t": "key", "key": "Enter"}));
    relay.expect_screen_showing("hi\nhi");

    relay.send(json!({"t": "repaint"}));
    let repaint = relay.expect("screen");
    assert!(repaint["data"].as_str().unwrap().contains("\u{1b}[2J"));
}

#[test]
fn a_relay_never_resizes_a_pane_it_was_not_given() {
    let server = Server::start();
    let pane = server.panes()[0]["pane_id"].as_u64().unwrap();
    let mut relay = server.relay(pane, &[]);
    relay.expect("hello");
    relay.send(json!({"t": "resize", "cols": 20, "rows": 5}));
    let error = relay.expect("error");
    assert!(error["message"]
        .as_str()
        .unwrap()
        .contains("--sized-by-input"));
    assert_eq!(server.panes()[0]["size"]["cols"], 40);
}

#[test]
fn a_relay_sized_by_input_resizes_its_pane() {
    let server = Server::start();
    let pane = server.panes()[0]["pane_id"].as_u64().unwrap();
    let mut relay = server.relay(pane, &["--sized-by-input"]);
    assert_eq!(relay.expect("hello")["sized_by_input"], true);
    relay.send(json!({"t": "resize", "cols": 30, "rows": 6}));
    let size = relay.expect("size");
    assert_eq!(
        (size["cols"].as_u64(), size["rows"].as_u64()),
        (Some(30), Some(6))
    );
    assert_eq!(server.panes()[0]["size"]["cols"], 30);
}

#[test]
fn a_relay_refuses_what_is_not_an_event_and_keeps_going() {
    let server = Server::start();
    let pane = server.panes()[0]["pane_id"].as_u64().unwrap();
    let mut relay = server.relay(pane, &[]);
    relay.expect("hello");
    relay.send(json!({"t": "exec", "command": "true"}));
    assert!(relay.expect("error")["message"]
        .as_str()
        .unwrap()
        .contains("not a relay event"));
    relay.send(json!({"t": "key", "key": "a", "mods": "LEADER"}));
    relay.expect("error");
    relay.send(json!({"t": "text", "text": "ok"}));
    relay.expect_screen_showing("ok");
}

#[test]
fn a_relay_ends_with_its_pane() {
    let server = Server::start();
    let pane = server.panes()[0]["pane_id"].as_u64().unwrap();
    let mut relay = server.relay(pane, &[]);
    relay.expect("hello");
    server.cli(&["kill-pane", "--pane-id", &pane.to_string()]);
    relay.expect("exit");
    assert!(relay.child.wait().unwrap().success());
}

#[test]
fn a_relay_ends_when_its_input_does() {
    let server = Server::start();
    let pane = server.panes()[0]["pane_id"].as_u64().unwrap();
    let mut relay = server.relay(pane, &[]);
    relay.expect("hello");
    relay.stdin.take();
    assert!(relay.child.wait().unwrap().success());
    assert_eq!(server.panes().len(), 1, "the pane outlives its relay");
}

#[test]
fn a_relay_for_a_pane_that_does_not_exist_fails() {
    let server = Server::start();
    let mut relay = server.relay(999, &[]);
    let error = relay.expect("error");
    assert!(error["message"].as_str().unwrap().contains("no pane 999"));
    assert!(!relay.child.wait().unwrap().success());
}

#[test]
fn a_relay_whose_input_ends_delivers_what_it_was_given_first_in_order() {
    // The program in the pane takes bracketed pastes and records the bytes it
    // reads, raw, until it has as many as a paste of "hello\nworld" and an
    // Enter make.
    let expected = "\u{1b}[200~hello\nworld\u{1b}[201~\r";
    let dir = tempfile::tempdir().unwrap();
    let record = dir.path().join("record");
    let script = format!(
        "stty raw -echo; printf '\\033[?2004hready'; head -c {} > '{}'; sleep 60",
        expected.len(),
        record.display()
    );
    let server = Server::running(&["sh", "-c", &script]);
    let pane = server.panes()[0]["pane_id"].as_u64().unwrap();
    server.wait_for_text(pane, "ready");

    // The input ends as soon as the paste and the Enter are written, before
    // the relay has so much as said hello.
    let mut relay = server.relay(pane, &[]);
    relay.send(json!({"t": "paste", "text": "hello\nworld"}));
    relay.send(json!({"t": "key", "key": "Enter"}));
    relay.stdin.take();
    let status = relay.child.wait().unwrap();
    assert!(status.success(), "the relay exited with {}", status);

    let deadline = Instant::now() + WAIT;
    loop {
        let got = std::fs::read(&record).unwrap_or_default();
        if got.len() >= expected.len() {
            assert_eq!(String::from_utf8_lossy(&got), expected);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the pane received only {:?}",
            String::from_utf8_lossy(&got)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A server whose pane, once `go` exists in the returned folder, copies
/// "héllo 日本 ✓" to the clipboard with OSC 52 ended by ST, then prints
/// "copied".
fn copying_server() -> (tempfile::TempDir, Server) {
    let dir = tempfile::tempdir().unwrap();
    let script = format!(
        "printf ready; while [ ! -e '{}' ]; do sleep 0.05; done; \
         printf '\\033]52;c;aMOpbGxvIOaXpeacrCDinJM=\\033\\\\copied'; sleep 60",
        dir.path().join("go").display()
    );
    let server = Server::running(&["sh", "-c", &script]);
    (dir, server)
}

#[test]
fn a_relay_passes_on_its_pane_s_clipboard_copy() {
    let (dir, server) = copying_server();
    let pane = server.panes()[0]["pane_id"].as_u64().unwrap();
    server.wait_for_text(pane, "ready");
    let mut relay = server.relay(pane, &[]);
    relay.expect("hello");
    std::fs::write(dir.path().join("go"), "").unwrap();
    let copy = relay.expect("clipboard");
    assert_eq!(copy, json!({"t": "clipboard", "text": "héllo 日本 ✓"}));
}

#[test]
fn a_tab_s_relay_passes_on_a_pane_s_clipboard_copy_naming_the_pane() {
    let (dir, server) = copying_server();
    let pane = server.panes()[0]["pane_id"].as_u64().unwrap();
    server.wait_for_text(pane, "ready");
    let mut relay = server.relay_with(&["--tab-of-pane", &pane.to_string()]);
    relay.expect("layout");
    std::fs::write(dir.path().join("go"), "").unwrap();
    let copy = relay.expect("clipboard");
    assert_eq!(
        copy,
        json!({"t": "clipboard", "text": "héllo 日本 ✓", "pane": pane})
    );
}
