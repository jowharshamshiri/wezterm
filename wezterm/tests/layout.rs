//! Arranging a tab's panes through `wsb-wezterm cli`, against a real
//! `wsb-wezterm-mux-server` on a socket of its own: each operation, the
//! layout read back, and a second client seeing every change.
#![cfg(unix)]

mod common;

use common::{Relay, Server};
use serde_json::{json, Value};

/// The layout of the tab holding `pane`, as `get-layout` prints it.
fn layout(server: &Server, pane: u64) -> Value {
    serde_json::from_str(&server.cli(&["get-layout", "--pane-id", &pane.to_string()])).unwrap()
}

/// A tree in short: a pane as `id:COLSxROWS@LEFT,TOP`, a horizontal split as
/// `(first | second)`, a vertical one as `(first / second)`.
fn shape(node: &Value) -> String {
    if let Some(pane) = node.get("pane") {
        return format!(
            "{}:{}x{}@{},{}",
            pane["id"], pane["cols"], pane["rows"], pane["left"], pane["top"]
        );
    }
    let split = &node["split"];
    let divider = match split["direction"].as_str().unwrap() {
        "horizontal" => "|",
        "vertical" => "/",
        other => panic!("a split's direction is {:?}", other),
    };
    format!(
        "({} {divider} {})",
        shape(&split["first"]),
        shape(&split["second"])
    )
}

/// The shape of the tab holding `pane`.
fn tab_shape(server: &Server, pane: u64) -> String {
    shape(&layout(server, pane)["root"])
}

/// Splits `pane` with `args`, returning the new pane.
fn split(server: &Server, pane: u64, args: &[&str]) -> u64 {
    let pane = pane.to_string();
    let out = server.cli(&[&["split-pane", "--pane-id", &pane], args].concat());
    out.trim().parse().unwrap()
}

/// A new tab beside the tab of `pane`, running `cat`; returns its pane.
fn new_tab(server: &Server, pane: u64) -> u64 {
    let out = server.cli(&["spawn", "--pane-id", &pane.to_string(), "--", "cat"]);
    out.trim().parse().unwrap()
}

fn tab_ids(server: &Server) -> Vec<u64> {
    let mut tabs: Vec<u64> = server
        .panes()
        .iter()
        .map(|pane| pane["tab_id"].as_u64().unwrap())
        .collect();
    tabs.dedup();
    tabs
}

fn server() -> Server {
    Server::sized(80, 24, &["cat"])
}

#[test]
fn a_layout_is_the_tree_with_each_panes_place_size_and_program() {
    let server = server();
    assert_eq!(split(&server, 0, &["--right", "--cells", "30"]), 1);
    assert_eq!(split(&server, 1, &["--bottom", "--cells", "10"]), 2);
    let tab = layout(&server, 2);
    assert_eq!(
        shape(&tab["root"]),
        "(0:49x24@0,0 | (1:30x13@50,0 / 2:30x10@50,14))"
    );
    assert_eq!(
        (tab["cols"].as_u64(), tab["rows"].as_u64()),
        (Some(80), Some(24))
    );
    assert_eq!(tab["active"], 2);
    assert_eq!(tab["zoomed"], Value::Null);
    assert_eq!(tab["tab_id"], server.panes()[0]["tab_id"]);
    let root = &tab["root"]["split"];
    assert_eq!(
        (&root["left"], &root["top"], &root["cols"], &root["rows"]),
        (&json!(0), &json!(0), &json!(80), &json!(24))
    );
    let pane = &root["first"]["pane"];
    assert_eq!(pane["title"], "cat");
    assert!(
        pane["process"].as_str().unwrap().ends_with("/cat"),
        "{}",
        pane
    );
    assert!(
        pane["tty"].as_str().unwrap().starts_with("/dev/"),
        "{}",
        pane
    );
    // Any pane of the tab, or the tab itself, names it.
    let tab_id = tab["tab_id"].to_string();
    assert_eq!(
        serde_json::from_str::<Value>(&server.cli(&["get-layout", "--tab-id", &tab_id])).unwrap(),
        tab
    );
    assert!(server
        .cli_fails(&["get-layout", "--tab-id", "99"], "")
        .contains("there is no tab 99"));
}

#[test]
fn a_pane_is_split_on_any_side_at_a_size() {
    let server = server();
    assert_eq!(split(&server, 0, &["--left", "--cells", "20"]), 1);
    assert_eq!(tab_shape(&server, 0), "(1:20x24@0,0 | 0:59x24@21,0)");
    assert_eq!(split(&server, 0, &["--top", "--percent", "25"]), 2);
    assert_eq!(
        tab_shape(&server, 0),
        "(1:20x24@0,0 | (2:59x6@21,0 / 0:59x17@21,7))"
    );
    assert_eq!(split(&server, 1, &["--bottom", "--cells", "4"]), 3);
    assert_eq!(split(&server, 0, &["--right", "--cells", "9"]), 4);
    assert_eq!(
        tab_shape(&server, 0),
        "((1:20x19@0,0 / 3:20x4@0,20) | (2:59x6@21,0 / (0:49x17@21,7 | 4:9x17@71,7)))"
    );
}

#[test]
fn a_split_is_sized_by_the_pane_before_its_divider() {
    let server = server();
    split(&server, 0, &["--right"]);
    split(&server, 0, &["--right"]);
    assert_eq!(
        tab_shape(&server, 0),
        "((0:19x24@0,0 | 2:19x24@20,0) | 1:40x24@40,0)"
    );
    // Pane 2 is before the root divider; the columns it gains go to it,
    // beside the divider.
    server.cli(&[
        "resize-split",
        "--pane-id",
        "2",
        "--direction",
        "right",
        "--first-cells",
        "50",
    ]);
    assert_eq!(
        tab_shape(&server, 0),
        "((0:19x24@0,0 | 2:30x24@20,0) | 1:29x24@51,0)"
    );
    server.cli(&[
        "resize-split",
        "--pane-id",
        "0",
        "--direction",
        "right",
        "--first-cells",
        "10",
    ]);
    assert_eq!(
        tab_shape(&server, 0),
        "((0:10x24@0,0 | 2:39x24@11,0) | 1:29x24@51,0)"
    );
    let err = server.cli_fails(
        &[
            "resize-split",
            "--pane-id",
            "1",
            "--direction",
            "right",
            "--first-cells",
            "10",
        ],
        "",
    );
    assert!(
        err.contains("there is no divider to the right of pane 1"),
        "{}",
        err
    );
    let err = server.cli_fails(
        &[
            "resize-split",
            "--pane-id",
            "2",
            "--direction",
            "down",
            "--first-cells",
            "10",
        ],
        "",
    );
    assert!(err.contains("there is no divider below pane 2"), "{}", err);
    let err = server.cli_fails(
        &[
            "resize-split",
            "--pane-id",
            "2",
            "--direction",
            "right",
            "--first-cells",
            "79",
        ],
        "",
    );
    assert!(
        err.contains("can give its first part 3 to 78 columns, not 79"),
        "{}",
        err
    );
    assert_eq!(
        tab_shape(&server, 0),
        "((0:10x24@0,0 | 2:39x24@11,0) | 1:29x24@51,0)"
    );
}

#[test]
fn adjust_pane_size_moves_the_divider_of_the_pane_it_names() {
    let server = server();
    split(&server, 0, &["--right"]);
    split(&server, 1, &["--bottom"]);
    server.cli(&["activate-pane", "--pane-id", "0"]);
    assert_eq!(
        tab_shape(&server, 0),
        "(0:39x24@0,0 | (1:40x11@40,0 / 2:40x12@40,12))"
    );
    // Pane 0 has the focus and no split across it: the divider moved is
    // pane 1's.
    server.cli(&[
        "adjust-pane-size",
        "--pane-id",
        "1",
        "--amount",
        "3",
        "Down",
    ]);
    assert_eq!(
        tab_shape(&server, 0),
        "(0:39x24@0,0 | (1:40x14@40,0 / 2:40x9@40,15))"
    );
    server.cli(&[
        "adjust-pane-size",
        "--pane-id",
        "2",
        "--amount",
        "5",
        "Left",
    ]);
    assert_eq!(
        tab_shape(&server, 0),
        "(0:34x24@0,0 | (1:45x14@35,0 / 2:45x9@35,15))"
    );
}

#[test]
fn a_pane_moves_to_a_place_in_its_tab_in_another_tab_or_to_a_new_tab() {
    let server = server();
    split(&server, 0, &["--right"]);
    // Within the tab: pane 1 goes below pane 0.
    split(&server, 0, &["--bottom", "--move-pane-id", "1"]);
    assert_eq!(tab_shape(&server, 0), "(0:80x11@0,0 / 1:80x12@0,12)");

    // From another tab: pane 2 goes left of pane 0, and its tab closes.
    let other = new_tab(&server, 0);
    assert_eq!(other, 2);
    assert_eq!(tab_ids(&server).len(), 2);
    split(
        &server,
        0,
        &["--left", "--cells", "10", "--move-pane-id", "2"],
    );
    assert_eq!(
        tab_shape(&server, 0),
        "((2:10x11@0,0 | 0:69x11@11,0) / 1:80x12@0,12)"
    );
    assert_eq!(tab_ids(&server).len(), 1);

    // To a tab of its own.
    server.cli(&["move-pane-to-new-tab", "--pane-id", "1"]);
    assert_eq!(tab_ids(&server).len(), 2);
    assert_eq!(tab_shape(&server, 1), "1:80x24@0,0");
    assert_eq!(tab_shape(&server, 0), "(2:10x24@0,0 | 0:69x24@11,0)");
}

#[test]
fn swapped_panes_trade_places_in_a_tab_and_between_tabs() {
    let server = server();
    split(&server, 0, &["--right", "--cells", "30"]);
    assert_eq!(layout(&server, 0)["active"], 1);
    server.cli(&["swap-panes", "--pane-id", "0", "--with", "1"]);
    assert_eq!(tab_shape(&server, 0), "(1:49x24@0,0 | 0:30x24@50,0)");
    // The focus went with pane 1.
    assert_eq!(layout(&server, 0)["active"], 1);

    let other = new_tab(&server, 0);
    server.cli(&["swap-panes", "--pane-id", "0", "--with", &other.to_string()]);
    assert_eq!(tab_shape(&server, 1), "(1:49x24@0,0 | 2:30x24@50,0)");
    assert_eq!(tab_shape(&server, 0), "0:80x24@0,0");

    assert!(server
        .cli_fails(&["swap-panes", "--pane-id", "0", "--with", "0"], "")
        .contains("pane 0 cannot be swapped with itself"));
    assert!(server
        .cli_fails(&["swap-panes", "--pane-id", "0", "--with", "42"], "")
        .contains("there is no pane 42"));
}

#[test]
fn a_pane_is_focused_zoomed_and_closed() {
    let server = server();
    split(&server, 0, &["--right"]);
    assert_eq!(layout(&server, 0)["active"], 1);
    server.cli(&["activate-pane", "--pane-id", "0"]);
    assert_eq!(layout(&server, 0)["active"], 0);

    server.cli(&["zoom-pane", "--pane-id", "1"]);
    let tab = layout(&server, 0);
    assert_eq!((&tab["zoomed"], &tab["active"]), (&json!(1), &json!(1)));
    // The tree keeps the places the panes have unzoomed.
    assert_eq!(shape(&tab["root"]), "(0:39x24@0,0 | 1:40x24@40,0)");
    server.cli(&["zoom-pane", "--pane-id", "1", "--unzoom"]);
    assert_eq!(layout(&server, 0)["zoomed"], Value::Null);

    server.cli(&["kill-pane", "--pane-id", "1"]);
    assert_eq!(tab_shape(&server, 0), "0:80x24@0,0");
}

/// Three nested splits: an existing pane, a pane spawned with a program, a
/// pane moved in from another tab (focused), and a pane spawned with the
/// default program.
const NESTED: &str = r#"{"root": {"split": {"direction": "horizontal", "first_cells": 30,
    "first": {"pane": {"id": 0}},
    "second": {"split": {"direction": "vertical", "first_cells": 8,
        "first": {"pane": {"spawn": {"args": ["cat"]}}},
        "second": {"split": {"direction": "horizontal", "first_percent": 50,
            "first": {"pane": {"id": 1, "active": true}},
            "second": {"pane": {"spawn": {}}}}}}}}}}"#;

#[test]
fn a_layout_set_from_a_description_reads_back_as_the_same_tree_and_sizes() {
    let server = server();
    assert_eq!(new_tab(&server, 0), 1);
    let printed: Value =
        serde_json::from_str(&server.cli_with_input(&["set-layout", "--pane-id", "0"], NESTED))
            .unwrap();
    let read = layout(&server, 0);
    assert_eq!(printed, read);
    assert_eq!(
        shape(&read["root"]),
        "(0:30x24@0,0 | (2:49x8@31,0 / (1:24x15@31,9 | 3:24x15@56,9)))"
    );
    assert_eq!(read["active"], 1);
    // Pane 1's tab, left empty, closed.
    assert_eq!(tab_ids(&server).len(), 1);
    // Each pane's terminal has its place's size.
    for pane in server.panes() {
        let id = pane["pane_id"].as_u64().unwrap();
        let place = find_pane(&read["root"], id).unwrap();
        assert_eq!(
            (&pane["size"]["cols"], &pane["size"]["rows"]),
            (&place["cols"], &place["rows"]),
            "pane {id}"
        );
    }
}

fn find_pane(node: &Value, id: u64) -> Option<Value> {
    if let Some(pane) = node.get("pane") {
        return (pane["id"] == id).then(|| pane.clone());
    }
    find_pane(&node["split"]["first"], id).or_else(|| find_pane(&node["split"]["second"], id))
}

#[test]
fn a_layout_that_cannot_be_set_changes_nothing_and_says_why() {
    let server = server();
    split(&server, 0, &["--right"]);
    let before = tab_shape(&server, 0);
    let refused = |layout: &str| server.cli_fails(&["set-layout", "--pane-id", "0"], layout);

    let err = refused(r#"{"root": {"pane": {"id": 0}}}"#);
    assert!(
        err.contains("the layout leaves out pane 1 of tab"),
        "{}",
        err
    );
    let err = refused(
        r#"{"root": {"split": {"direction": "horizontal", "first_cells": 79,
            "first": {"pane": {"id": 0}}, "second": {"pane": {"id": 1}}}}}"#,
    );
    assert!(
        err.contains("root divides 80 columns: its first part can have 1 to 78, not 79"),
        "{}",
        err
    );
    let err = refused(
        r#"{"root": {"split": {"direction": "horizontal",
            "first": {"pane": {"id": 0}}, "second": {"pane": {"id": 0}}}}}"#,
    );
    assert!(
        err.contains("the layout names pane 0 more than once"),
        "{}",
        err
    );
    let err = refused(
        r#"{"root": {"split": {"direction": "horizontal",
            "first": {"pane": {"id": 0}}, "second": {"pane": {"id": 1, "cols": 40}}}}}"#,
    );
    assert!(err.contains("not one set-layout takes"), "{}", err);
    let err = refused(
        r#"{"root": {"split": {"direction": "horizontal",
            "first": {"pane": {"id": 0, "active": true}}, "second": {"pane": {"id": 1, "active": true}}}}}"#,
    );
    assert!(
        err.contains("the layout makes 2 panes active; one at most"),
        "{}",
        err
    );
    assert_eq!(tab_shape(&server, 0), before);
    assert_eq!(server.panes().len(), 2);
}

#[test]
fn keys_and_text_reach_a_pane_and_its_text_is_read() {
    let server = Server::sized(
        80,
        24,
        &["sh", "-c", "trap 'echo interrupted' INT; cat; sleep 60"],
    );
    server.cli(&["send-keys", "--pane-id", "0", "h", "i", "Enter"]);
    // The terminal echoes the line, and cat prints it again.
    server.wait_for_text(0, "hi\nhi");
    server.cli(&["send-text", "--pane-id", "0", "--no-paste", "there\n"]);
    server.wait_for_text(0, "there\nthere");
    server.cli(&["send-keys", "--pane-id", "0", "CTRL+c"]);
    server.wait_for_text(0, "interrupted");
    let err = server.cli_fails(&["send-keys", "--pane-id", "0", "x", "NotAKey"], "");
    assert!(err.contains("the key \"NotAKey\""), "{}", err);
    assert!(!server
        .cli(&["get-text", "--pane-id", "0"])
        .contains("interrupted\nx"));
}

/// Waits for the relay's next layout event that is the tab as `get-layout`
/// reads it now.
fn sees(relay: &mut Relay, server: &Server, pane: u64) -> Value {
    let want = layout(server, pane);
    relay.expect_where(&format!("layout {want}"), |event| {
        event["t"] == "layout" && event["tab"] == want
    })
}

#[test]
fn a_second_client_sees_every_layout_change_and_ends_with_the_same_layout() {
    let server = server();
    let mut relay = server.relay_with(&["--tab-of-pane", "0"]);
    let first = relay.expect("layout");
    assert_eq!(first["tab"], layout(&server, 0));
    assert_eq!(relay.expect("hello")["pane"], 0);

    split(&server, 0, &["--right"]);
    sees(&mut relay, &server, 0);
    relay.expect_where("hello for pane 1", |e| e["t"] == "hello" && e["pane"] == 1);

    server.cli(&[
        "resize-split",
        "--pane-id",
        "0",
        "--direction",
        "right",
        "--first-cells",
        "20",
    ]);
    sees(&mut relay, &server, 0);
    server.cli(&[
        "adjust-pane-size",
        "--pane-id",
        "0",
        "--amount",
        "2",
        "Right",
    ]);
    sees(&mut relay, &server, 0);
    server.cli(&["swap-panes", "--pane-id", "0", "--with", "1"]);
    sees(&mut relay, &server, 0);
    server.cli(&["activate-pane", "--pane-id", "0"]);
    sees(&mut relay, &server, 0);
    server.cli(&["zoom-pane", "--pane-id", "1"]);
    sees(&mut relay, &server, 0);
    server.cli(&["zoom-pane", "--pane-id", "1", "--unzoom"]);
    sees(&mut relay, &server, 0);
    split(&server, 1, &["--bottom", "--move-pane-id", "0"]);
    sees(&mut relay, &server, 0);
    server.cli_with_input(
        &["set-layout", "--pane-id", "0"],
        r#"{"root": {"split": {"direction": "vertical", "first_cells": 5,
            "first": {"pane": {"id": 1}}, "second": {"pane": {"id": 0}}}}}"#,
    );
    sees(&mut relay, &server, 0);
    server.cli(&["kill-pane", "--pane-id", "1"]);
    relay.expect_where("gone for pane 1", |e| e["t"] == "gone" && e["pane"] == 1);
    let last = sees(&mut relay, &server, 0);
    assert_eq!(shape(&last["tab"]["root"]), "0:80x24@0,0");
}

#[test]
fn a_tab_relay_follows_its_pane_to_another_tab_and_ends_with_its_tab() {
    let server = server();
    split(&server, 0, &["--right"]);
    split(&server, 1, &["--bottom"]);
    let mut relay = server.relay_with(&["--tab-of-pane", "1"]);
    relay.expect("layout");
    let hello = relay.expect("hello");
    assert_eq!(hello["sized_by_input"], false, "{hello}");

    // Pane 1 moves to a tab of its own: the panes it leaves are gone.
    server.cli(&["move-pane-to-new-tab", "--pane-id", "1"]);
    relay.expect_where("gone for pane 0", |e| e["t"] == "gone" && e["pane"] == 0);
    relay.expect_where("gone for pane 2", |e| e["t"] == "gone" && e["pane"] == 2);
    let moved = sees(&mut relay, &server, 1);
    assert_eq!(shape(&moved["tab"]["root"]), "1:80x24@0,0");

    // Pane 2 joins it; then pane 1 closes, and the relay keeps the tab.
    split(&server, 1, &["--right", "--move-pane-id", "2"]);
    relay.expect_where("hello for pane 2", |e| e["t"] == "hello" && e["pane"] == 2);
    server.cli(&["kill-pane", "--pane-id", "1"]);
    relay.expect_where("gone for pane 1", |e| e["t"] == "gone" && e["pane"] == 1);
    sees(&mut relay, &server, 2);

    server.cli(&["kill-pane", "--pane-id", "2"]);
    relay.expect_where("gone for pane 2", |e| e["t"] == "gone" && e["pane"] == 2);
    relay.expect("exit");
    assert!(relay.child.wait().unwrap().success());
}

#[test]
fn a_tab_relay_takes_input_for_each_pane_and_sizes_the_tab() {
    let server = server();
    split(&server, 0, &["--right"]);
    let mut relay = server.relay_with(&["--tab-of-pane", "0", "--sized-by-input"]);
    relay.expect("layout");
    // Each pane's hello says the relay's resizes size the tab.
    for pane in [0, 1] {
        let hello = relay.expect_where(&format!("hello for pane {pane}"), |e| {
            e["t"] == "hello" && e["pane"] == pane
        });
        assert_eq!(hello["sized_by_input"], true, "{hello}");
    }
    relay.send(json!({"t": "text", "text": "hi", "pane": 1}));
    relay.send(json!({"t": "key", "key": "Enter", "pane": 1}));
    server.wait_for_text(1, "hi\nhi");
    assert!(!server.cli(&["get-text", "--pane-id", "0"]).contains("hi"));

    relay.send(json!({"t": "text", "text": "hi"}));
    let err = relay.expect("error");
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .contains("name their \"pane\""),
        "{}",
        err
    );
    relay.send(json!({"t": "resize", "cols": 30, "rows": 6, "pane": 0}));
    let err = relay.expect("error");
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .contains("sized by the tab's layout"),
        "{}",
        err
    );
    relay.send(json!({"t": "text", "text": "x", "pane": 7}));
    let err = relay.expect("error");
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .contains("pane 7 is not in the relayed tab"),
        "{}",
        err
    );

    relay.send(json!({"t": "resize", "cols": 60, "rows": 20}));
    let resized = relay.expect_where("a 60x20 layout", |e| {
        e["t"] == "layout" && e["tab"]["cols"] == 60 && e["tab"]["rows"] == 20
    });
    assert_eq!(resized["tab"], layout(&server, 0));
}
