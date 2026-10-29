//! `wezterm cli relay` against a real `wezterm-mux-server`, on a socket of
//! its own. The server is the one built beside this crate's `wezterm`:
//! `cargo build -p wezterm-mux-server` first.
#![cfg(unix)]

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(20);

struct Server {
    dir: tempfile::TempDir,
    config: PathBuf,
    socket: PathBuf,
    child: Child,
}

impl Server {
    /// A server whose first pane runs `cat`, 40x10.
    fn start() -> Self {
        // A unix socket path is limited to about 100 bytes.
        let dir = tempfile::Builder::new()
            .prefix("relay")
            .tempdir_in("/tmp")
            .unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = dir.path().join("s");
        let config = dir.path().join("wezterm.lua");
        std::fs::write(
            &config,
            format!(
                "return {{ unix_domains = {{ {{ name = 't', socket_path = '{}' }} }}, \
                 initial_cols = 40, initial_rows = 10 }}\n",
                socket.display()
            ),
        )
        .unwrap();
        let server = Path::new(env!("CARGO_BIN_EXE_wezterm")).with_file_name("wezterm-mux-server");
        assert!(
            server.exists(),
            "{} is missing; build it with: cargo build -p wezterm-mux-server",
            server.display()
        );
        let mut command = Command::new(server);
        command
            .args(["--config-file"])
            .arg(&config)
            .args(["--", "cat"]);
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
            .env("WEZTERM_UNIX_SOCKET", socket)
            .env("WEZTERM_CONFIG_FILE", config)
            .env("XDG_RUNTIME_DIR", dir)
            .env_remove("WEZTERM_PANE")
    }

    fn wezterm(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_wezterm"));
        command.arg("--config-file").arg(&self.config);
        Self::environment(&mut command, self.dir.path(), &self.socket, &self.config);
        command
    }

    fn cli(&self, args: &[&str]) -> String {
        let output = self
            .wezterm()
            .args(["cli", "--no-auto-start"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "wezterm cli {:?}: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn panes(&self) -> Vec<Value> {
        serde_json::from_str(&self.cli(&["list", "--format", "json"])).unwrap()
    }

    fn relay(&self, pane: u64, extra: &[&str]) -> Relay {
        let mut child = self
            .wezterm()
            .args(["cli", "--no-auto-start", "relay", "--pane-id"])
            .arg(pane.to_string())
            .args(extra)
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
                    .unwrap_or_else(|err| panic!("{line:?} is not JSON: {err}"));
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
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

struct Relay {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<Value>,
    /// Every `screen` event's data so far.
    screen: String,
}

impl Relay {
    fn send(&mut self, event: Value) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{}", event).unwrap();
        stdin.flush().unwrap();
    }

    /// The first event of type `t`, keeping the screen's data on the way.
    fn expect(&mut self, t: &str) -> Value {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = self
                .rx
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("no {t:?} event; the screen was {:?}", self.screen));
            if event["t"] == "screen" {
                self.screen.push_str(event["data"].as_str().unwrap());
            }
            if event["t"] == t {
                return event;
            }
        }
    }

    fn expect_screen_containing(&mut self, text: &str) {
        while !self.screen.contains(text) {
            self.expect("screen");
        }
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

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
    relay.expect_screen_containing("hi\u{1b}[2;1Hhi");

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
    relay.expect("hello");
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
    relay.expect_screen_containing("ok");
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
