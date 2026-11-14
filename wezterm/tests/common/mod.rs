//! A real `wsb-wezterm-mux-server` on a socket of its own, driven by the
//! `wsb-wezterm` built beside it: `cargo build -p wezterm-mux-server` first.
#![allow(dead_code)]

use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};
use wezterm_term::color::ColorPalette;
use wezterm_term::{Terminal, TerminalConfiguration, TerminalSize};

pub const WAIT: Duration = Duration::from_secs(20);

pub struct Server {
    dir: tempfile::TempDir,
    config: PathBuf,
    socket: PathBuf,
    pub child: Child,
}

impl Server {
    /// A server whose first pane runs `cat`, 40x10.
    pub fn start() -> Self {
        Self::running(&["cat"])
    }

    /// A server whose first pane runs `program`, 40x10.
    pub fn running(program: &[&str]) -> Self {
        Self::sized(40, 10, program)
    }

    /// A server whose first pane runs `program` in a tab of `cols` by `rows`
    /// cells. A pane spawned without a program runs `cat`.
    pub fn sized(cols: usize, rows: usize, program: &[&str]) -> Self {
        // A unix socket path is limited to about 100 bytes.
        let dir = tempfile::Builder::new()
            .prefix("relay")
            .tempdir_in("/tmp")
            .unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = dir.path().join("s");
        let config = dir.path().join("wsb-wezterm.lua");
        std::fs::write(
            &config,
            format!(
                "return {{ unix_domains = {{ {{ name = 't', socket_path = '{}' }} }}, \
                 initial_cols = {cols}, initial_rows = {rows}, default_prog = {{ 'cat' }} }}\n",
                socket.display()
            ),
        )
        .unwrap();
        let server =
            Path::new(env!("CARGO_BIN_EXE_wsb-wezterm")).with_file_name("wsb-wezterm-mux-server");
        assert!(
            server.exists(),
            "{} is missing; build it with: cargo build -p wezterm-mux-server",
            server.display()
        );
        let mut command = Command::new(server);
        command
            .args(["--config-file"])
            .arg(&config)
            .arg("--")
            .args(program);
        let child = Self::environment(&mut command, dir.path(), &socket, &config)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + WAIT;
        while !socket.exists() {
            assert!(
                Instant::now() < deadline,
                "the server never made its socket"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        Self {
            dir,
            config,
            socket,
            child,
        }
    }

    fn environment<'a>(
        command: &'a mut Command,
        dir: &Path,
        socket: &Path,
        config: &Path,
    ) -> &'a mut Command {
        command
            .env("WSB_WEZTERM_UNIX_SOCKET", socket)
            .env("WSB_WEZTERM_CONFIG_FILE", config)
            .env("XDG_RUNTIME_DIR", dir)
            .env_remove("WSB_WEZTERM_PANE")
    }

    pub fn wezterm(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_wsb-wezterm"));
        command.arg("--config-file").arg(&self.config);
        Self::environment(&mut command, self.dir.path(), &self.socket, &self.config);
        command
    }

    /// What `wsb-wezterm cli` prints, failing the test if it fails.
    pub fn cli(&self, args: &[&str]) -> String {
        self.cli_with_input(args, "")
    }

    /// What `wsb-wezterm cli` prints given `input` on stdin, failing the
    /// test if it fails.
    pub fn cli_with_input(&self, args: &[&str], input: &str) -> String {
        let output = self.run_cli(args, input);
        assert!(
            output.status.success(),
            "wsb-wezterm cli {:?}: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    /// What `wsb-wezterm cli` says on stderr, failing the test unless it
    /// fails.
    pub fn cli_fails(&self, args: &[&str], input: &str) -> String {
        let output = self.run_cli(args, input);
        assert!(
            !output.status.success(),
            "wsb-wezterm cli {:?} succeeded: {}",
            args,
            String::from_utf8_lossy(&output.stdout)
        );
        String::from_utf8(output.stderr).unwrap()
    }

    fn run_cli(&self, args: &[&str], input: &str) -> std::process::Output {
        let mut child = self
            .wezterm()
            .args(["cli", "--no-auto-start"])
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    pub fn panes(&self) -> Vec<Value> {
        serde_json::from_str(&self.cli(&["list", "--format", "json"])).unwrap()
    }

    /// A relay of the pane `pane`, with `extra` arguments.
    pub fn relay(&self, pane: u64, extra: &[&str]) -> Relay {
        let pane = pane.to_string();
        self.relay_with(&[&["--pane-id", &pane], extra].concat())
    }

    /// A relay with `args`.
    pub fn relay_with(&self, args: &[&str]) -> Relay {
        let mut child = self
            .wezterm()
            .args(["cli", "--no-auto-start", "relay"])
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let value: Value = serde_json::from_str(&line)
                    .unwrap_or_else(|err| panic!("{:?} is not JSON: {}", line, err));
                if tx.send(value).is_err() {
                    break;
                }
            }
        });
        Relay {
            stdin: child.stdin.take(),
            child,
            rx,
            screen: String::new(),
            shown: None,
        }
    }
}

impl Server {
    /// A wsb-wezterm window on the X display `display`, attached to this
    /// server (`wsb-wezterm connect`), its terminal `cols` by `rows` cells to
    /// start with. It shows the default workspace.
    pub fn window(&self, display: &str, cols: usize, rows: usize) -> Window {
        let config = self.dir.path().join("window.lua");
        std::fs::write(
            &config,
            format!(
                "return {{ unix_domains = {{ {{ name = 't', socket_path = '{}' }} }}, \
                 initial_cols = {cols}, initial_rows = {rows} }}\n",
                self.socket.display()
            ),
        )
        .unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_wsb-wezterm"));
        command
            .arg("--config-file")
            .arg(&config)
            .args(["connect", "t"]);
        let child = Self::environment(&mut command, self.dir.path(), &self.socket, &config)
            .env("DISPLAY", display)
            .env_remove("WAYLAND_DISPLAY")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Window { child }
    }

    /// Wait until the pane shows `text`.
    pub fn wait_for_text(&self, pane: u64, text: &str) {
        let deadline = Instant::now() + WAIT;
        loop {
            let shown = self.cli(&["get-text", "--pane-id", &pane.to_string()]);
            if shown.contains(text) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the pane never showed {:?}; it shows {:?}",
                text,
                shown
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

/// A wsb-wezterm window, ended with the test.
pub struct Window {
    pub child: Child,
}

impl Drop for Window {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

/// The X display a test with a window shows it on, `WSB_WEZTERM_TEST_DISPLAY`
/// (a virtual one, such as an Xvfb's, will do).
pub fn display() -> String {
    std::env::var("WSB_WEZTERM_TEST_DISPLAY").unwrap_or_else(|_| {
        panic!(
            "WSB_WEZTERM_TEST_DISPLAY is not set: name the X display a wsb-wezterm window \
             is shown on, such as :97 of an Xvfb"
        )
    })
}

pub struct Relay {
    pub child: Child,
    pub stdin: Option<ChildStdin>,
    rx: Receiver<Value>,
    /// Every `screen` event's data so far.
    pub screen: String,
    /// A terminal showing what the relay painted for one pane, as the
    /// program the relay serves would: made by the pane's `hello`, remade by
    /// its `size` events.
    shown: Option<Terminal>,
}

#[derive(Debug)]
struct Shown;

impl TerminalConfiguration for Shown {
    fn color_palette(&self) -> ColorPalette {
        ColorPalette::default()
    }
}

fn terminal(event: &Value) -> Terminal {
    let size = |field: &str| event[field].as_u64().unwrap() as usize;
    Terminal::new(
        TerminalSize {
            cols: size("cols"),
            rows: size("rows"),
            pixel_width: 0,
            pixel_height: 0,
            dpi: 0,
        },
        Arc::new(Shown),
        "relay-test",
        "0",
        Box::new(std::io::sink()),
    )
}

impl Relay {
    pub fn send(&mut self, event: Value) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{}", event).unwrap();
        stdin.flush().unwrap();
    }

    /// The first event of type `t`, keeping the screen's data on the way.
    pub fn expect(&mut self, t: &str) -> Value {
        self.expect_where(&format!("{t:?}"), |event| event["t"] == t)
    }

    /// The next event `wanted` takes, `what` naming it; keeps the screen's
    /// data on the way.
    pub fn expect_where(&mut self, what: &str, wanted: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = self
                .rx
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("no {what} event; the screen was {:?}", self.screen));
            // The screen of a single pane's relay, whose events name no pane.
            if event.get("pane").is_none() {
                match event["t"].as_str() {
                    Some("hello") | Some("size") => self.shown = Some(terminal(&event)),
                    Some("screen") => {
                        let data = event["data"].as_str().unwrap();
                        self.screen.push_str(data);
                        if let Some(shown) = self.shown.as_mut() {
                            shown.advance_bytes(data);
                        }
                    }
                    _ => {}
                }
            }
            if wanted(&event) {
                return event;
            }
        }
    }

    /// The lines a terminal shows after the relay's paints, without their
    /// trailing blanks, joined with newlines.
    pub fn shown(&self) -> String {
        let Some(shown) = self.shown.as_ref() else {
            return String::new();
        };
        let screen = shown.screen();
        let rows = screen.physical_rows as i64;
        screen
            .lines_in_phys_range(screen.phys_range(&(0..rows)))
            .iter()
            .map(|line| line.as_str().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Waits for paints after which a terminal shows `text`, however the
    /// relay split them into frames.
    pub fn expect_screen_showing(&mut self, text: &str) {
        while !self.shown().contains(text) {
            let shown = self.shown();
            self.expect_where(
                &format!("screen showing {text:?} (it shows {shown:?})"),
                |e| e["t"] == "screen",
            );
        }
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

/// The layout of the tab holding `pane`, as `get-layout` prints it, without
/// `sized_by_window` ([`sized_by_window`]): as a relay's layout event has it.
pub fn layout(server: &Server, pane: u64) -> Value {
    let mut shown = get_layout(server, pane);
    let said = shown.as_object_mut().unwrap().remove("sized_by_window");
    assert!(
        said.is_some(),
        "get-layout says whether a window sizes the tab: {}",
        shown
    );
    shown
}

/// What `get-layout` prints for the tab holding `pane`.
pub fn get_layout(server: &Server, pane: u64) -> Value {
    serde_json::from_str(&server.cli(&["get-layout", "--pane-id", &pane.to_string()])).unwrap()
}

/// Whether a desktop window sizes the tab holding `pane`, as `get-layout`
/// says it: the rule and each window, or null.
pub fn sized_by_window(server: &Server, pane: u64) -> Value {
    get_layout(server, pane)["sized_by_window"].clone()
}

/// Checks that each split of the tab `tab` (a layout) is filled by its first
/// part, the one-cell divider and its second part, with every part where the
/// split puts it: a tab resized pane by pane, half way, does not add up.
pub fn assert_fills(tab: &Value) {
    fn fills(node: &Value, left: u64, top: u64, cols: u64, rows: u64, tab: &Value) {
        let (place, split) = match node.get("pane") {
            Some(pane) => (pane, None),
            None => (&node["split"], Some(&node["split"])),
        };
        let at = |field: &str| place[field].as_u64().unwrap();
        assert_eq!(
            (at("left"), at("top"), at("cols"), at("rows")),
            (left, top, cols, rows),
            "{} is not where its split puts it in {tab}",
            shape(node)
        );
        let Some(split) = split else {
            return;
        };
        let first = |field: &str| {
            let first = &split["first"];
            first.get("pane").unwrap_or(&first["split"])[field]
                .as_u64()
                .unwrap()
        };
        if split["direction"] == "horizontal" {
            let first_cols = first("cols");
            assert!(
                first_cols + 1 < cols,
                "{} leaves no second part in {tab}",
                shape(node)
            );
            fills(&split["first"], left, top, first_cols, rows, tab);
            fills(
                &split["second"],
                left + first_cols + 1,
                top,
                cols - first_cols - 1,
                rows,
                tab,
            );
        } else {
            let first_rows = first("rows");
            assert!(
                first_rows + 1 < rows,
                "{} leaves no second part in {tab}",
                shape(node)
            );
            fills(&split["first"], left, top, cols, first_rows, tab);
            fills(
                &split["second"],
                left,
                top + first_rows + 1,
                cols,
                rows - first_rows - 1,
                tab,
            );
        }
    }
    let side = |field: &str| tab[field].as_u64().unwrap();
    fills(&tab["root"], 0, 0, side("cols"), side("rows"), tab);
}

/// A tree in short: a pane as `id:COLSxROWS@LEFT,TOP`, a horizontal split as
/// `(first | second)`, a vertical one as `(first / second)`.
pub fn shape(node: &Value) -> String {
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
pub fn tab_shape(server: &Server, pane: u64) -> String {
    shape(&layout(server, pane)["root"])
}

/// Splits `pane` with `args`, returning the new pane.
pub fn split(server: &Server, pane: u64, args: &[&str]) -> u64 {
    let pane = pane.to_string();
    let out = server.cli(&[&["split-pane", "--pane-id", &pane], args].concat());
    out.trim().parse().unwrap()
}
