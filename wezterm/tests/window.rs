//! A wsb-wezterm window attached to a real `wsb-wezterm-mux-server`: it sizes
//! the tabs of the workspace it shows, as a whole, and no other client resizes
//! them. Needs an X display, `WSB_WEZTERM_TEST_DISPLAY`, and
//! `cargo build -p wezterm-gui -p wezterm-mux-server` first.
#![cfg(unix)]

mod common;

use common::{assert_fills, display, layout, sized_by_window, split, Server, WAIT};
use serde_json::{json, Value};
use std::process::Command;
use std::time::{Duration, Instant};

/// How long a window may take to start and attach: a debug build drawing
/// without a GPU on a busy machine is slow to.
const ATTACH: Duration = Duration::from_secs(90);

/// Waits up to `wait` until `check` gives a value, `what` naming it.
fn wait_for<T>(what: &str, wait: Duration, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + wait;
    loop {
        if let Some(found) = check() {
            return found;
        }
        assert!(Instant::now() < deadline, "never: {}", what);
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// The GUIs attached to the server, as `list-clients` lists them.
fn windows(server: &Server) -> Vec<Value> {
    let clients: Vec<Value> =
        serde_json::from_str(&server.cli(&["list-clients", "--format", "json"])).unwrap();
    clients
        .into_iter()
        .filter(|client| client["role"] == "gui")
        .collect()
}

/// `xdotool` on `display`, failing the test if it fails.
fn xdotool(display: &str, args: &[&str]) -> String {
    let output = Command::new("xdotool")
        .args(args)
        .env("DISPLAY", display)
        .output()
        .expect("xdotool runs");
    assert!(
        output.status.success(),
        "xdotool {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn a_window_sizes_the_tabs_of_the_workspace_it_shows_as_a_whole_and_a_relay_does_not() {
    let display = display();
    let server = Server::sized(80, 24, &["cat"]);
    // The overseers' side panel: split right, its right side split down.
    split(&server, 0, &["--right", "--cells", "30"]);
    split(&server, 1, &["--bottom"]);
    let elsewhere: u64 = server
        .cli(&[
            "spawn",
            "--new-window",
            "--workspace",
            "elsewhere",
            "--",
            "cat",
        ])
        .trim()
        .parse()
        .unwrap();
    assert_eq!(sized_by_window(&server, 0), Value::Null, "no window yet");

    let _window = server.window(&display, 80, 24);
    // The window tells the server the workspace it shows.
    let gui = wait_for(
        "a window attached, showing the default workspace",
        ATTACH,
        || {
            windows(&server)
                .into_iter()
                .find(|gui| gui["workspace"] == "default")
        },
    );
    let named = format!(
        "the wsb-wezterm window of {}@{} (process {})",
        gui["username"].as_str().unwrap(),
        gui["hostname"].as_str().unwrap(),
        gui["pid"]
    );
    // A window attaching to a tab takes the tab's size for itself.
    let shown = layout(&server, 0);
    assert_fills(&shown);
    let rule = sized_by_window(&server, 0);
    let said = rule
        .as_str()
        .unwrap_or_else(|| panic!("no rule is said: {}", rule));
    assert!(
        said.contains("a desktop window sizes each tab of the workspace it shows")
            && said.contains("is in workspace \"default\"")
            && said.contains(&named),
        "{}",
        said
    );

    // A relay's resize of that tab is refused, naming the rule and the
    // window, and the tab keeps the window's size.
    let mut relay = server.relay_with(&["--tab-of-pane", "0", "--sized-by-input"]);
    relay.expect("layout");
    relay.send(json!({"t": "resize", "cols": 160, "rows": 48}));
    let err = relay.expect("error");
    assert_eq!(err["sized_by_window"], rule, "{}", err);
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .contains("a resize of the tab to 160x48 was refused"),
        "{}",
        err
    );
    drop(relay);
    assert_eq!(layout(&server, 0), shown);
    // As is `cli resize-tab`'s, which prints the rule and exits with 3.
    let output = server
        .wezterm()
        .args([
            "cli",
            "--no-auto-start",
            "resize-tab",
            "--pane-id",
            "0",
            "--cols",
            "160",
            "--rows",
            "48",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3), "{:?}", output);
    let printed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(printed, json!({"sized_by_window": rule}));
    assert_eq!(layout(&server, 0), shown);

    // A tab of a workspace the window does not show is the relay's to size.
    assert_eq!(sized_by_window(&server, elsewhere), Value::Null);
    server.cli_with_input(
        &[
            "relay",
            "--tab-of-pane",
            &elsewhere.to_string(),
            "--sized-by-input",
        ],
        "{\"t\":\"resize\",\"cols\":160,\"rows\":48}\n",
    );
    let other = layout(&server, elsewhere);
    assert_eq!((&other["cols"], &other["rows"]), (&json!(160), &json!(48)));

    // The window resized resizes its tab, as a whole.
    let pid = gui["pid"].to_string();
    let found = xdotool(
        &display,
        &["search", "--sync", "--onlyvisible", "--pid", &pid],
    );
    let ids: Vec<&str> = found.split_whitespace().collect();
    assert_eq!(
        ids.len(),
        1,
        "the window is the one on the display: {}",
        found
    );
    xdotool(&display, &["windowsize", "--sync", ids[0], "1400", "900"]);
    let resized = wait_for("the window's resize reached the tab", WAIT, || {
        let tab = layout(&server, 0);
        (tab["cols"] != shown["cols"] || tab["rows"] != shown["rows"]).then_some(tab)
    });
    assert_fills(&resized);
    assert_eq!(
        common::shape(&resized["root"]).matches(':').count(),
        3,
        "the tab keeps its three panes: {}",
        resized
    );
}
