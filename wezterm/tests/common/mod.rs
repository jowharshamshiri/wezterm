//! A real `wsb-wezterm-mux-server` on a socket of its own, driven by the
//! `wsb-wezterm` built beside it: `cargo build -p wezterm-mux-server` first.
#![allow(dead_code)]

use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

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
        }
    }
}

impl Server {
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

pub struct Relay {
    pub child: Child,
    pub stdin: Option<ChildStdin>,
    rx: Receiver<Value>,
    /// Every `screen` event's data so far.
    pub screen: String,
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
            if event["t"] == "screen" {
                self.screen.push_str(event["data"].as_str().unwrap());
            }
            if wanted(&event) {
                return event;
            }
        }
    }

    /// The next event, if one comes within `wait`.
    pub fn next_within(&mut self, wait: Duration) -> Option<Value> {
        self.rx.recv_timeout(wait).ok()
    }

    pub fn expect_screen_containing(&mut self, text: &str) {
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
