//! Arranging a tab's panes through `wsb-wezterm cli`, against a real
//! `wsb-wezterm-mux-server` on a socket of its own: each operation, the
//! layout read back, and a second client seeing every change.
#![cfg(unix)]

mod common;

use common::{assert_fills, layout, shape, sized_by_window, split, tab_shape, Relay, Server};
use serde_json::{json, Value};

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
    let mut by_tab =
        serde_json::from_str::<Value>(&server.cli(&["get-layout", "--tab-id", &tab_id])).unwrap();
    assert_eq!(
        by_tab.as_object_mut().unwrap().remove("sized_by_window"),
        Some(Value::Null),
        "no window is attached"
    );
    assert_eq!(by_tab, tab);
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
fn a_move_into_a_tab_too_small_to_split_leaves_the_pane_in_its_tab_and_says_why() {
    let server = server();
    split(&server, 0, &["--right"]);
    let small = new_tab(&server, 0);
    assert_eq!(small, 2);
    let small_tab = layout(&server, small)["tab_id"].to_string();
    server.cli(&[
        "resize-tab",
        "--tab-id",
        &small_tab,
        "--cols",
        "10",
        "--rows",
        "5",
    ]);
    let before = tab_shape(&server, 0);

    let err = server.cli_fails(
        &[
            "split-pane",
            "--pane-id",
            "2",
            "--right",
            "--cells",
            "20",
            "--move-pane-id",
            "1",
        ],
        "",
    );
    assert!(
        err.contains(
            "pane 1 cannot move beside pane 2, and stays where it was: \
             no room for a pane of 20 cells on the right of pane 2, which is 10x5 cells: \
             the split would leave parts of 0x5 and 20x5 cells, \
             and each needs at least 1x1 beside the divider"
        ),
        "{}",
        err
    );

    // Pane 1 is where it was, in its own tab, and its program still takes
    // what is sent to it.
    assert_eq!(tab_shape(&server, 0), before);
    assert_eq!(tab_shape(&server, small), "2:10x5@0,0");
    assert_eq!(tab_ids(&server).len(), 2);
    server.cli(&["send-text", "--pane-id", "1", "--no-paste", "still here\n"]);
    server.wait_for_text(1, "still here\nstill here");
}

#[test]
fn a_move_within_its_tab_is_refused_only_when_the_tab_without_it_has_no_room() {
    let server = server();
    split(&server, 0, &["--right", "--cells", "70"]);
    let before = tab_shape(&server, 0);
    assert_eq!(before, "(0:9x24@0,0 | 1:70x24@10,0)");

    // Once pane 1 is out of the tab, pane 0 has all 24 rows: 30 do not fit.
    let err = server.cli_fails(
        &[
            "split-pane",
            "--pane-id",
            "0",
            "--bottom",
            "--cells",
            "30",
            "--move-pane-id",
            "1",
        ],
        "",
    );
    assert!(
        err.contains(
            "pane 1 cannot move beside pane 0, and stays where it was: \
             no room for a pane of 30 cells below pane 0, which is 80x24 cells: \
             the split would leave parts of 80x0 and 80x30 cells, \
             and each needs at least 1x1 beside the divider"
        ),
        "{}",
        err
    );
    assert_eq!(tab_shape(&server, 0), before);

    // Pane 0 is 9 columns wide now, too narrow for 40 more; with pane 1 out
    // of the tab it has all 80, and the move fits.
    split(
        &server,
        0,
        &["--right", "--cells", "40", "--move-pane-id", "1"],
    );
    assert_eq!(tab_shape(&server, 0), "(0:39x24@0,0 | 1:40x24@40,0)");

    let err = server.cli_fails(
        &[
            "split-pane",
            "--pane-id",
            "1",
            "--left",
            "--move-pane-id",
            "1",
        ],
        "",
    );
    assert!(err.contains("pane 1 cannot move beside itself"), "{}", err);
    assert_eq!(tab_shape(&server, 0), "(0:39x24@0,0 | 1:40x24@40,0)");
}

#[test]
fn a_split_with_no_room_starts_no_program() {
    let server = server();
    let err = server.cli_fails(
        &["split-pane", "--pane-id", "0", "--top", "--cells", "24"],
        "",
    );
    assert!(
        err.contains(
            "no room for a pane of 24 cells above pane 0, which is 80x24 cells: \
             the split would leave parts of 80x24 and 80x0 cells, \
             and each needs at least 1x1 beside the divider"
        ),
        "{}",
        err
    );
    assert_eq!(server.panes().len(), 1);
    assert_eq!(tab_shape(&server, 0), "0:80x24@0,0");
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
    assert_eq!(hello["sized_by_input"], false, "{}", hello);

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
        assert_eq!(hello["sized_by_input"], true, "{}", hello);
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

#[test]
fn a_tab_relay_whose_input_ends_has_sized_the_tab_first() {
    // A program sizing a tab writes one resize and closes the relay's input:
    // the relay ends only once the server has given the tab that size, as a
    // whole, for a tab of one pane, of one split, of a split within a split
    // either way, and of three panes side by side. Every tree fills the tab.
    for (splits, sized) in [
        (&[][..], "0:160x48@0,0"),
        (&[(0, "--right")][..], "(0:79x48@0,0 | 1:80x48@80,0)"),
        (
            &[(0, "--right"), (1, "--bottom")][..],
            "(0:79x48@0,0 | (1:80x23@80,0 / 2:80x24@80,24))",
        ),
        (
            &[(0, "--bottom"), (1, "--right")][..],
            "(0:160x23@0,0 / (1:79x24@0,24 | 2:80x24@80,24))",
        ),
        (
            &[(0, "--right"), (1, "--right")][..],
            "(0:79x48@0,0 | (1:59x48@80,0 | 2:20x48@140,0))",
        ),
    ] {
        let server = server();
        for (pane, side) in splits {
            split(&server, *pane, &[side]);
        }
        server.cli_with_input(
            &["relay", "--tab-of-pane", "0", "--sized-by-input"],
            "{\"t\":\"resize\",\"cols\":160,\"rows\":48}\n",
        );
        let tab = layout(&server, 0);
        assert_eq!(
            (
                tab["cols"].as_u64(),
                tab["rows"].as_u64(),
                shape(&tab["root"])
            ),
            (Some(160), Some(48), sized.to_string()),
            "{}",
            tab
        );
        assert_fills(&tab);
        assert_eq!(
            sized_by_window(&server, 0),
            Value::Null,
            "no window is attached"
        );
    }
}

#[test]
fn a_pane_of_a_tab_with_others_is_not_resized_alone() {
    // A side panel: a pane split right at 32 cells, its right side
    // split down. Resizing pane 0 alone used to give it the size while its
    // tab kept the rest, which then did not add up: a 120x48 split of pane 0
    // at 87x48 and a right side of 32x24. The tab's layout sizes its panes,
    // so the server refuses it and changes nothing.
    let server = Server::sized(120, 24, &["cat"]);
    split(&server, 0, &["--right", "--cells", "32"]);
    split(&server, 1, &["--bottom"]);
    let before = layout(&server, 0);
    assert_eq!(
        shape(&before["root"]),
        "(0:87x24@0,0 | (1:32x11@88,0 / 2:32x12@88,12))"
    );
    let mut relay = server.relay(0, &["--sized-by-input"]);
    relay.expect("hello");
    relay.send(json!({"t": "resize", "cols": 87, "rows": 48}));
    let err = relay.expect("error");
    let tab = before["tab_id"].as_u64().unwrap();
    assert!(
        err["message"].as_str().unwrap().contains(&format!(
            "pane 0 is one of the panes 0, 1, 2 of tab {tab}, whose layout sizes them"
        )),
        "{}",
        err
    );
    assert_eq!(err.get("sized_by_window"), None, "{}", err);
    drop(relay);
    let after = layout(&server, 0);
    assert_eq!(after, before);
    assert_fills(&after);
}

#[test]
fn resize_tab_sizes_a_tab_as_a_whole() {
    let server = server();
    split(&server, 0, &["--right", "--cells", "30"]);
    split(&server, 1, &["--bottom"]);
    let said = server.cli(&[
        "resize-tab",
        "--pane-id",
        "1",
        "--cols",
        "100",
        "--rows",
        "30",
    ]);
    assert_eq!(said.trim(), r#"{"sized_by_window":null}"#);
    let tab = layout(&server, 0);
    assert_eq!(
        (&tab["cols"], &tab["rows"]),
        (&json!(100), &json!(30)),
        "{}",
        tab
    );
    assert_fills(&tab);
    let id = tab["tab_id"].to_string();
    let refused = server.cli_fails(
        &["resize-tab", "--tab-id", &id, "--cols", "2", "--rows", "30"],
        "",
    );
    assert!(
        refused.contains(&format!(
            "tab {id} cannot be 2x30: its layout of panes 0, 1, 2 needs at least 3x3 cells"
        )),
        "{}",
        refused
    );
    assert_eq!(layout(&server, 0), tab);
    assert!(server
        .cli_fails(
            &[
                "resize-tab",
                "--tab-id",
                "99",
                "--cols",
                "80",
                "--rows",
                "24"
            ],
            ""
        )
        .contains("there is no tab 99"));
}

#[test]
fn a_tab_is_not_resized_below_what_its_layout_needs() {
    let server = server();
    split(&server, 0, &["--right"]);
    split(&server, 1, &["--right"]);
    let before = layout(&server, 0);
    let tab = before["tab_id"].as_u64().unwrap();
    let mut relay = server.relay_with(&["--tab-of-pane", "0", "--sized-by-input"]);
    relay.expect("layout");
    relay.send(json!({"t": "resize", "cols": 3, "rows": 2}));
    let err = relay.expect("error");
    assert!(
        err["message"].as_str().unwrap().contains(&format!(
            "tab {tab} cannot be 3x2: its layout of panes 0, 1, 2 needs at least 5x1 cells"
        )),
        "{}",
        err
    );
    drop(relay);
    assert_eq!(layout(&server, 0), before);
}

#[test]
fn a_layout_only_relay_sends_the_tabs_layout_as_it_changes_and_no_pane() {
    let server = server();
    let mut relay = server.relay_with(&["--tab-of-pane", "0", "--layout-only"]);
    // Every event is the tab's layout, or the error an input line earns.
    let layout_shaped = |relay: &mut Relay, wanted: &str| {
        relay.expect_where(&format!("a layout of {wanted}"), |event| {
            assert!(
                event["t"] == "layout" || event["t"] == "error",
                "a layout-only relay sent {}",
                event
            );
            event["t"] == "layout" && shape(&event["tab"]["root"]) == wanted
        })
    };
    layout_shaped(&mut relay, "0:80x24@0,0");
    split(&server, 0, &["--right", "--cells", "30"]);
    layout_shaped(&mut relay, "(0:49x24@0,0 | 1:30x24@50,0)");
    split(&server, 1, &["--bottom", "--cells", "10"]);
    let nested = layout_shaped(&mut relay, "(0:49x24@0,0 | (1:30x13@50,0 / 2:30x10@50,14))");
    assert_eq!(nested["tab"], layout(&server, 0));
    server.cli(&["kill-pane", "--pane-id", "2"]);
    layout_shaped(&mut relay, "(0:49x24@0,0 | 1:30x24@50,0)");

    relay.send(json!({"t": "text", "text": "x", "pane": 0}));
    let err = relay.expect("error");
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .contains("a layout-only relay relays no pane"),
        "{}",
        err
    );
    // Its input ending ends it.
    drop(relay.stdin.take());
    assert!(relay.child.wait().unwrap().success());
}
