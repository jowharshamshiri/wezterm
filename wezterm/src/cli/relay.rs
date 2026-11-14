//! `wsb-wezterm cli relay`: one pane of a multiplexer, mirrored for another
//! program over stdin and stdout.
//!
//! The relay attaches to the multiplexer as a client, as a GUI does, and
//! keeps a mirror of the pane. It writes the pane's screen as escape
//! sequences: a full paint first, then only what changed, as a terminal
//! emulator that displays them (xterm.js in a web page, say) needs. What
//! the other program sends back are input events, not bytes: keys, text,
//! paste and mouse. The multiplexer encodes them with the modes of the
//! terminal in the pane (application cursor keys, bracketed paste, the
//! kitty keyboard protocol, mouse reporting), exactly as it does for a
//! GUI, so the program in the pane cannot tell the relay from a keyboard.
//!
//! The relay never resizes the pane unless it was started with
//! `--sized-by-input`, and never tells the multiplexer it has focus: the
//! pane keeps the size and focus the other clients give it.
//!
//! Both directions are JSON, one object per line, told apart by `t`.
//!
//! Input reaches the pane in the order it was read: each request to the
//! multiplexer is sent once the one before has been taken. When stdin ends,
//! the relay sends what it still holds before it exits, and a request the
//! multiplexer refused makes it exit with an error.

use crate::cli::layout::{tab_of_pane, tab_tree, TabLayout};
use anyhow::{anyhow, bail, Context};
use clap::Parser;
use codec::ListPanesResponse;
use config::UnixDomain;
use mux::client::ClientRole;
use mux::domain::Domain;
use mux::pane::{Pane, PaneId};
use mux::renderable::StableCursorPosition;
use mux::tab::TabId;
use mux::{Mux, MuxNotification};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::future::Future;
use std::io::{BufRead, Read, Write};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use termwiz::cell::CellAttributes;
use termwiz::color::ColorAttribute;
use termwiz::input::KeyInput;
use termwiz::render::terminfo::TerminfoRenderer;
use termwiz::render::RenderTty;
use termwiz::surface::{Change, CursorShape, CursorVisibility, Position, Surface};
use wezterm_client::client::Client;
use wezterm_client::domain::{ClientDomain, ClientDomainConfig};
use wezterm_client::mirror::TabNotSized;
use wezterm_client::pane::ClientPane;
use wezterm_term::color::ColorPalette;
use wezterm_term::{
    Alert, KeyCode, KeyModifiers, Line, MouseButton, MouseEvent, MouseEventKind, StableRowIndex,
    TerminalSize,
};

/// How long output is gathered before a frame is drawn.
const FRAME_DELAY: Duration = Duration::from_millis(8);
/// How often the pane is polled, which also notices one that is gone.
const POLL_INTERVAL: Duration = Duration::from_millis(500);
/// The longest line of input accepted: a paste of this size is plenty.
const MAX_INPUT_LINE: usize = 4 * 1024 * 1024;

#[derive(Debug, Parser, Clone)]
#[command(group(clap::ArgGroup::new("what").required(true).args(["pane_id", "tab_of_pane"])))]
pub struct RelayCommand {
    /// The pane to relay, as `wsb-wezterm cli list` shows it.
    #[arg(long = "pane-id")]
    pane_id: Option<PaneId>,

    /// Relay every pane of the tab that holds this pane, and the tab's
    /// layout, following the pane when it moves to another tab. Once the
    /// pane is gone, the relay goes on with the tab it was last in.
    #[arg(long = "tab-of-pane")]
    tab_of_pane: Option<PaneId>,

    /// Let `resize` events size the pane, which must be alone in its tab, or
    /// with `--tab-of-pane` the tab as a whole. Without it the pane or tab
    /// keeps the size its other clients give it, and `resize` is refused.
    #[arg(long = "sized-by-input")]
    sized_by_input: bool,

    /// With `--tab-of-pane`: relay only the tab's layout, each time it
    /// changes, and no pane: no `hello` or `screen`, and input takes only a
    /// `resize` of the tab.
    #[arg(long = "layout-only", requires = "tab_of_pane")]
    layout_only: bool,
}

/// Run the relay until stdin ends or the pane does. It runs on its own
/// executor rather than `wsb-wezterm cli`'s: the client's panes spawn tasks
/// that need one able to run them from any thread.
pub fn run(
    opts: &crate::Opt,
    prefer_mux: bool,
    class: &str,
    cmd: RelayCommand,
) -> anyhow::Result<()> {
    crate::init_config(opts)?;
    let unix = Client::compute_unix_domain(prefer_mux, class)?;

    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);
    let executor = promise::spawn::SimpleExecutor::new();

    promise::spawn::spawn(async move {
        let mut out = Output::new(std::io::stdout());
        let code = match relay(unix, cmd, &mut out).await {
            Ok(()) => 0,
            Err(err) => {
                // A relay whose stdout is gone has nobody to tell.
                out.send(&Out::Error {
                    message: format!("{:#}", err),
                    sized_by_window: None,
                })
                .ok();
                log::error!("{:#}", err);
                1
            }
        };
        std::process::exit(code);
    })
    .detach();

    loop {
        executor.tick()?;
    }
}

/// What the relay writes.
#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Out {
    /// First, once: the pane's size, colors, title and mouse mode, and
    /// whether `resize` events size it (`--sized-by-input`).
    Hello {
        cols: usize,
        rows: usize,
        palette: Palette,
        title: String,
        mouse_grab: bool,
        sized_by_input: bool,
    },
    /// Escape sequences that bring a terminal of the pane's size from
    /// what the relay last sent to what the pane shows now.
    Screen {
        data: String,
    },
    /// The pane changed size; a full `screen` paint follows.
    Size {
        cols: usize,
        rows: usize,
    },
    Title {
        text: String,
    },
    /// Whether the program in the pane takes the mouse: while it does,
    /// mouse events are its; otherwise they select and scroll.
    MouseGrab {
        on: bool,
    },
    /// The program in the pane set the clipboard (OSC 52).
    Clipboard {
        text: Option<String>,
    },
    Palette {
        palette: Palette,
    },
    Bell,
    /// The pane, or with `--tab-of-pane` the tab, is gone; the relay ends.
    Exit,
    /// With `--tab-of-pane`: the tab's layout, first and whenever it
    /// changes.
    Layout {
        tab: TabLayout,
    },
    /// With `--tab-of-pane`: the pane left the tab.
    Gone,
    /// An input line was refused, an event did not reach the pane, or the
    /// relay failed. A resize of the tab that the multiplexer refused
    /// because a desktop window sizes the tab says the rule and each window
    /// in `sized_by_window`.
    Error {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        sized_by_window: Option<String>,
    },
}

/// The pane's colors, as `#rrggbb`.
#[derive(Debug, Serialize, PartialEq)]
pub struct Palette {
    foreground: String,
    background: String,
    cursor_fg: String,
    cursor_bg: String,
    cursor_border: String,
    selection_fg: String,
    selection_bg: String,
    /// The 16 ANSI colors, normal then bright.
    ansi: Vec<String>,
}

impl From<&ColorPalette> for Palette {
    fn from(palette: &ColorPalette) -> Self {
        Self {
            foreground: palette.foreground.to_rgb_string(),
            background: palette.background.to_rgb_string(),
            cursor_fg: palette.cursor_fg.to_rgb_string(),
            cursor_bg: palette.cursor_bg.to_rgb_string(),
            cursor_border: palette.cursor_border.to_rgb_string(),
            selection_fg: palette.selection_fg.to_rgb_string(),
            selection_bg: palette.selection_bg.to_rgb_string(),
            ansi: palette.colors.0[..16]
                .iter()
                .map(|c| c.to_rgb_string())
                .collect(),
        }
    }
}

/// What the relay reads.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "t", rename_all = "snake_case", deny_unknown_fields)]
pub enum In {
    /// A key, named as in a key binding (`"a"`, `"Enter"`, `"UpArrow"`,
    /// `"F5"`), with modifiers such as `"CTRL|SHIFT"`.
    Key {
        key: String,
        #[serde(default)]
        mods: String,
    },
    /// Typed text, each character a key press.
    Text {
        text: String,
    },
    Paste {
        text: String,
    },
    /// A mouse event at a cell of the visible screen, counted from 0.
    Mouse {
        kind: MouseKind,
        x: usize,
        y: i64,
        button: Button,
        #[serde(default)]
        mods: String,
    },
    /// Move the view `rows` rows into the scrollback (positive) or back
    /// toward the bottom (negative).
    Scroll {
        rows: isize,
    },
    /// Follow the bottom of the pane again.
    Bottom,
    /// Size the pane; only with `--sized-by-input`.
    Resize {
        cols: usize,
        rows: usize,
    },
    /// Paint the whole screen again.
    Repaint,
}

#[derive(Debug, Deserialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum MouseKind {
    Press,
    Release,
    Move,
}

#[derive(Debug, Deserialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum Button {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    None,
}

/// A key as the pane receives it.
pub fn key_input(
    key: &str,
    mods: &str,
    swap_backspace_and_delete: bool,
) -> anyhow::Result<(KeyCode, KeyModifiers)> {
    let named = wezterm_input_types::KeyCode::try_from(key).map_err(|err| anyhow!(err))?;
    let mods = modifiers(mods)?;
    match KeyInput::from_key_code(&named, swap_backspace_and_delete) {
        KeyInput::Code(code) => Ok((code, mods)),
        KeyInput::Composed(_) | KeyInput::None => bail!("{key:?} sends nothing to a terminal"),
    }
}

/// Modifiers such as `"CTRL|SHIFT"`: shift, alt, ctrl and super only.
pub fn modifiers(mods: &str) -> anyhow::Result<KeyModifiers> {
    let parsed = KeyModifiers::try_from(mods.to_string()).map_err(|err| anyhow!(err))?;
    let allowed =
        KeyModifiers::SHIFT | KeyModifiers::ALT | KeyModifiers::CTRL | KeyModifiers::SUPER;
    if !allowed.contains(parsed) {
        bail!("{mods:?} names a modifier other than SHIFT, ALT, CTRL and SUPER");
    }
    Ok(parsed)
}

pub fn mouse_event(
    kind: MouseKind,
    x: usize,
    y: i64,
    button: Button,
    mods: &str,
) -> anyhow::Result<MouseEvent> {
    Ok(MouseEvent {
        kind: match kind {
            MouseKind::Press => MouseEventKind::Press,
            MouseKind::Release => MouseEventKind::Release,
            MouseKind::Move => MouseEventKind::Move,
        },
        x,
        y,
        x_pixel_offset: 0,
        y_pixel_offset: 0,
        button: match button {
            Button::Left => MouseButton::Left,
            Button::Middle => MouseButton::Middle,
            Button::Right => MouseButton::Right,
            Button::WheelUp => MouseButton::WheelUp(1),
            Button::WheelDown => MouseButton::WheelDown(1),
            Button::None => MouseButton::None,
        },
        modifiers: modifiers(mods)?,
    })
}

/// The screen as last sent, and what turns a new one into escapes.
pub struct Screen {
    surface: Surface,
    renderer: TerminfoRenderer,
    painted: bool,
    /// The cursor as last sent.
    cursor: Option<Cursor>,
}

/// Where the cursor is drawn, if it is on the screen being shown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cursor {
    pub x: usize,
    pub row: usize,
    pub shape: CursorShape,
    pub visibility: CursorVisibility,
}

impl Screen {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            surface: Surface::new(cols, rows),
            renderer: termwiz_funcs::new_wezterm_terminfo_renderer(),
            painted: false,
            cursor: None,
        }
    }

    pub fn size(&self) -> (usize, usize) {
        self.surface.dimensions()
    }

    /// Forget what was sent: the next update paints everything.
    pub fn invalidate(&mut self) {
        let (cols, rows) = self.size();
        *self = Self::new(cols, rows);
    }

    /// Escapes that make the screen show `lines` (one per row) and the
    /// cursor; empty when nothing changed.
    pub fn update(
        &mut self,
        mut lines: Vec<Line>,
        cursor: Option<Cursor>,
    ) -> anyhow::Result<String> {
        let (cols, rows) = self.size();
        if lines.len() != rows {
            bail!("{} lines for a screen of {} rows", lines.len(), rows);
        }
        for line in &mut lines {
            // A line shorter than the screen must still clear what was
            // to its right.
            let seqno = line.current_seqno();
            line.resize(cols, seqno);
        }
        let mut changes = vec![];
        if !self.painted {
            changes.push(Change::AllAttributes(CellAttributes::blank()));
            changes.push(Change::ClearScreen(ColorAttribute::Default));
            self.painted = true;
        }
        changes.extend(self.surface.diff_lines(lines.iter().collect()));
        // Drawing moves the terminal's cursor, so it is placed again after
        // any; otherwise only when it moved or changed.
        if changes.is_empty() && cursor == self.cursor {
            return Ok(String::new());
        }
        self.cursor = cursor;
        match cursor {
            Some(cursor) => {
                changes.push(Change::CursorPosition {
                    x: Position::Absolute(cursor.x),
                    y: Position::Absolute(cursor.row),
                });
                changes.push(Change::CursorShape(cursor.shape));
                changes.push(Change::CursorVisibility(cursor.visibility));
            }
            None => changes.push(Change::CursorVisibility(CursorVisibility::Hidden)),
        }
        self.surface.add_changes(changes.clone());
        let mut target = Target {
            bytes: vec![],
            size: (cols, rows),
        };
        self.renderer.render_to(&changes, &mut target)?;
        Ok(String::from_utf8(target.bytes).context("escapes are not UTF-8")?)
    }
}

struct Target {
    bytes: Vec<u8>,
    size: (usize, usize),
}

impl Write for Target {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.bytes.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl RenderTty for Target {
    fn get_size_in_cells(&mut self) -> termwiz::Result<(usize, usize)> {
        Ok(self.size)
    }
}

struct Output<W: Write> {
    writer: W,
}

impl<W: Write> Output<W> {
    fn new(writer: W) -> Self {
        Self { writer }
    }

    fn send(&mut self, out: &Out) -> anyhow::Result<()> {
        self.send_for(None, out)
    }

    /// Writes `out`, tagged with the pane it is about when there is one.
    fn send_for(&mut self, pane: Option<PaneId>, out: &Out) -> anyhow::Result<()> {
        let mut line = serde_json::to_vec(out)?;
        if let Some(pane) = pane {
            // An event is an object: the pane goes in as its last field.
            if line.pop() != Some(b'}') {
                bail!(
                    "an event is not a JSON object: {}",
                    String::from_utf8_lossy(&line)
                );
            }
            line.extend_from_slice(format!(",\"pane\":{pane}}}").as_bytes());
        }
        line.push(b'\n');
        self.writer.write_all(&line).context("writing to stdout")?;
        self.writer.flush().context("writing to stdout")
    }
}

/// What the relay hears of, the panes named by their ids in the relay's
/// own mux.
enum Event {
    Output(PaneId),
    Frame(PaneId),
    Tick,
    Alert(PaneId, Alert),
    Clipboard(PaneId, Option<String>),
    Removed(PaneId),
    /// A tab's layout changed in the relay's mirror of the multiplexer.
    LayoutChanged,
    /// The multiplexer's panes, as listed for a tab relay.
    Listed(anyhow::Result<ListPanesResponse>),
    Input(String),
    InputFailed(String),
    InputEnded,
    /// A request to the pane failed.
    RequestFailed(Failure),
}

/// A request that failed: the message names it, and says the rule when a
/// desktop window sizes the tab it was to resize.
#[derive(Clone, Debug, PartialEq)]
struct Failure {
    message: String,
    sized_by_window: Option<String>,
}

impl Failure {
    fn event(&self) -> Out {
        Out::Error {
            message: self.message.clone(),
            sized_by_window: self.sized_by_window.clone(),
        }
    }
}

/// A request to the pane, and what it was for, to name it if it fails.
struct Request {
    what: String,
    send: Pin<Box<dyn Future<Output = anyhow::Result<()>>>>,
}

/// Sends the requests in the order they come, each once the one before has
/// ended, so the pane receives them in that order; `failed` hears of each
/// that failed as it does. Ends once `queue` is closed and every request in
/// it has been sent, with the failures.
async fn send_requests(
    queue: smol::channel::Receiver<Request>,
    failed: impl Fn(&Failure),
) -> Vec<Failure> {
    let mut failures = vec![];
    while let Ok(request) = queue.recv().await {
        if let Err(err) = request.send.await {
            let failure = match err.downcast_ref::<TabNotSized>() {
                Some(TabNotSized::SizedByWindow(rule)) => Failure {
                    message: format!("{} was refused: {rule}", request.what),
                    sized_by_window: Some(rule.clone()),
                },
                _ => Failure {
                    message: format!("{} did not reach the pane: {:#}", request.what, err),
                    sized_by_window: None,
                },
            };
            failed(&failure);
            failures.push(failure);
        }
    }
    failures
}

/// The relay's own end of the queue of requests to the panes.
struct Requests {
    sender: smol::channel::Sender<Request>,
}

impl Requests {
    /// Queues a request to a pane, behind those already made.
    fn request(
        &self,
        what: String,
        send: impl Future<Output = anyhow::Result<()>> + 'static,
    ) -> anyhow::Result<()> {
        self.sender
            .try_send(Request {
                what,
                send: Box::pin(send),
            })
            .map_err(|_| anyhow!("the relay no longer sends requests to the pane"))
    }
}

async fn relay<W: Write>(
    unix: UnixDomain,
    cmd: RelayCommand,
    out: &mut Output<W>,
) -> anyhow::Result<()> {
    let mux = Mux::get();
    let domain = Arc::new(ClientDomain::new(
        ClientDomainConfig::Unix(unix),
        ClientRole::Relay,
    ));
    let as_domain: Arc<dyn Domain> = domain.clone();
    mux.add_domain(&as_domain);
    as_domain.attach(None).await?;

    let (tx, rx) = smol::channel::unbounded::<Event>();

    // A subscriber runs while the mux and, for output, the pane are
    // locked: it only passes the news on.
    mux.subscribe({
        let tx = tx.clone();
        move |notification| {
            let event = match notification {
                MuxNotification::PaneOutput(id) => Event::Output(id),
                MuxNotification::Alert { pane_id, alert } => Event::Alert(pane_id, alert),
                MuxNotification::AssignClipboard {
                    pane_id, clipboard, ..
                } => Event::Clipboard(pane_id, clipboard),
                MuxNotification::PaneRemoved(id) => Event::Removed(id),
                MuxNotification::TabLayoutChanged(_)
                | MuxNotification::TabAddedToWindow { .. }
                | MuxNotification::PaneAdded(_) => Event::LayoutChanged,
                _ => return !tx.is_closed(),
            };
            tx.try_send(event).is_ok()
        }
    });

    std::thread::spawn({
        let tx = tx.clone();
        move || {
            let stdin = std::io::stdin();
            let mut reader = stdin.lock();
            let mut line = String::new();
            loop {
                line.clear();
                let event = match (&mut reader)
                    .take(MAX_INPUT_LINE as u64 + 1)
                    .read_line(&mut line)
                {
                    Ok(0) => Event::InputEnded,
                    Ok(_) if line.len() > MAX_INPUT_LINE => Event::InputFailed(format!(
                        "an input line is longer than {MAX_INPUT_LINE} bytes"
                    )),
                    Ok(_) => Event::Input(line.trim_end_matches(['\r', '\n']).to_string()),
                    Err(err) => Event::InputFailed(format!("reading stdin: {err}")),
                };
                let last = !matches!(event, Event::Input(_));
                if tx.send_blocking(event).is_err() || last {
                    break;
                }
            }
        }
    });

    promise::spawn::spawn({
        let tx = tx.clone();
        async move {
            loop {
                smol::Timer::after(POLL_INTERVAL).await;
                if tx.send(Event::Tick).await.is_err() {
                    break;
                }
            }
        }
    })
    .detach();

    let (sender, queue) = smol::channel::unbounded::<Request>();
    let requests = Rc::new(Requests { sender });
    let sending = promise::spawn::spawn({
        let tx = tx.clone();
        send_requests(queue, move |failure| {
            tx.try_send(Event::RequestFailed(failure.clone())).ok();
        })
    });

    let mut failures_reported = 0;
    let result: anyhow::Result<()> = async {
        let mut relay: Box<dyn Relaying<W>> = match (cmd.pane_id, cmd.tab_of_pane) {
            (Some(pane_id), None) => {
                let local_id = domain
                    .remote_to_local_pane_id(pane_id)
                    .ok_or_else(|| anyhow!("the multiplexer has no pane {}", pane_id))?;
                let pane = mux
                    .get_pane(local_id)
                    .ok_or_else(|| anyhow!("pane {} went away while attaching", pane_id))?;
                let mut relay =
                    PaneRelay::new(pane, None, cmd.sized_by_input, tx, Rc::clone(&requests))?;
                relay.hello(out)?;
                Box::new(relay)
            }
            (None, Some(pane_id)) => {
                let client = ClientDomain::get_client_inner_for_domain(domain.domain_id())?
                    .client
                    .clone();
                let panes = client.list_panes().await?;
                if tab_of_pane(&panes, pane_id).is_none() {
                    bail!("the multiplexer has no pane {}", pane_id);
                }
                let mut relay = TabRelay {
                    follow: pane_id,
                    tab_id: None,
                    layout: None,
                    panes: BTreeMap::new(),
                    domain: Arc::clone(&domain),
                    client,
                    layout_only: cmd.layout_only,
                    sized_by_input: cmd.sized_by_input,
                    tx,
                    requests: Rc::clone(&requests),
                    listing: false,
                    list_again: false,
                };
                match relay.apply(panes, out)? {
                    Flow::Continue => {}
                    Flow::Stop => bail!("the tab of pane {} went away while attaching", pane_id),
                }
                Box::new(relay)
            }
            _ => bail!("relay one pane with --pane-id, or a tab with --tab-of-pane"),
        };
        while let Ok(event) = rx.recv().await {
            let flow = match event {
                Event::Input(line) => {
                    if let Err(err) = relay.input(&line, out) {
                        out.send(&Out::Error {
                            message: format!("{:#}", err),
                            sized_by_window: None,
                        })?;
                    }
                    Flow::Continue
                }
                Event::InputFailed(message) => bail!(message),
                Event::InputEnded => Flow::Stop,
                Event::RequestFailed(failure) => {
                    failures_reported += 1;
                    out.send(&failure.event())?;
                    Flow::Continue
                }
                event => relay.handle(event, out)?,
            };
            if let Flow::Stop = flow {
                break;
            }
        }
        Ok(())
    }
    .await;

    // Whatever ended the relay, the requests it made are sent first, in
    // order; dropping the last handle on the queue closes it.
    drop(requests);
    let failures = sending.await;
    for failure in &failures[failures_reported..] {
        out.send(&failure.event())?;
    }
    result?;
    if !failures.is_empty() {
        bail!(
            "{} of the input events did not reach the pane",
            failures.len()
        );
    }
    Ok(())
}

enum Flow {
    Continue,
    Stop,
}

/// A relay of one pane, or of a tab.
trait Relaying<W: Write> {
    /// What the relay heard of, other than input.
    fn handle(&mut self, event: Event, out: &mut Output<W>) -> anyhow::Result<Flow>;
    /// A line of input.
    fn input(&mut self, line: &str, out: &mut Output<W>) -> anyhow::Result<()>;
}

/// One pane, mirrored: with `tag`, a pane of a tab relay, whose events name
/// it.
struct PaneRelay {
    pane: Arc<dyn Pane>,
    tag: Option<PaneId>,
    sized_by_input: bool,
    tx: smol::channel::Sender<Event>,
    requests: Rc<Requests>,
    screen: Screen,
    /// The stable row at the top of the view while it is scrolled back;
    /// `None` follows the bottom of the pane.
    view_top: Option<StableRowIndex>,
    frame_pending: bool,
    title: String,
    mouse_grab: bool,
}

impl PaneRelay {
    fn new(
        pane: Arc<dyn Pane>,
        tag: Option<PaneId>,
        sized_by_input: bool,
        tx: smol::channel::Sender<Event>,
        requests: Rc<Requests>,
    ) -> anyhow::Result<Self> {
        if pane.downcast_ref::<ClientPane>().is_none() {
            bail!("pane {} is not a multiplexer client's pane", pane.pane_id());
        }
        let dims = pane.get_dimensions();
        Ok(Self {
            requests,
            screen: Screen::new(dims.cols, dims.viewport_rows),
            title: pane.get_title(),
            mouse_grab: pane.is_mouse_grabbed(),
            pane,
            tag,
            sized_by_input,
            tx,
            view_top: None,
            frame_pending: false,
        })
    }

    fn send<W: Write>(&self, out: &mut Output<W>, event: &Out) -> anyhow::Result<()> {
        out.send_for(self.tag, event)
    }

    fn hello<W: Write>(&mut self, out: &mut Output<W>) -> anyhow::Result<()> {
        let (cols, rows) = self.screen.size();
        self.send(
            out,
            &Out::Hello {
                cols,
                rows,
                palette: Palette::from(&self.pane.palette()),
                title: self.title.clone(),
                mouse_grab: self.mouse_grab,
                sized_by_input: self.sized_by_input,
            },
        )?;
        self.frame(out)
    }

    /// Polls the pane, as a GUI does when it paints; true once it is dead.
    fn poll(&self) -> bool {
        let dims = self.pane.get_dimensions();
        let top = dims.physical_top;
        self.pane
            .get_changed_since(top..top + dims.viewport_rows as StableRowIndex, 0);
        self.pane.is_dead()
    }

    fn alert<W: Write>(&self, alert: Alert, out: &mut Output<W>) -> anyhow::Result<()> {
        match alert {
            Alert::Bell => self.send(out, &Out::Bell),
            Alert::PaletteChanged => self.send(
                out,
                &Out::Palette {
                    palette: Palette::from(&self.pane.palette()),
                },
            ),
            _ => Ok(()),
        }
    }

    fn schedule_frame(&mut self) {
        if self.frame_pending {
            return;
        }
        self.frame_pending = true;
        let tx = self.tx.clone();
        let pane_id = self.pane.pane_id();
        promise::spawn::spawn(async move {
            smol::Timer::after(FRAME_DELAY).await;
            tx.send(Event::Frame(pane_id)).await.ok();
        })
        .detach();
    }

    fn scheduled_frame<W: Write>(&mut self, out: &mut Output<W>) -> anyhow::Result<()> {
        self.frame_pending = false;
        self.frame(out)
    }

    /// The pane, as the multiplexer client's pane it was checked to be when
    /// the relay took it.
    fn client_pane(&self) -> &ClientPane {
        self.pane
            .downcast_ref::<ClientPane>()
            .expect("the relay checked its pane is a client's when it took it")
    }

    fn input<W: Write>(&mut self, input: In, out: &mut Output<W>) -> anyhow::Result<()> {
        let swap = config::configuration().swap_backspace_and_delete;
        match input {
            In::Key { key: name, mods } => {
                let (key, mods) = key_input(&name, &mods, swap)?;
                self.follow_bottom(out)?;
                let send = self.client_pane().key_down_request(key, mods);
                self.requests.request(format!("the key {name:?}"), send)?;
            }
            In::Text { text } => {
                // Every character is checked before any is sent.
                let keys = text
                    .chars()
                    .map(|c| key_input(&c.to_string(), "", swap))
                    .collect::<anyhow::Result<Vec<_>>>()?;
                self.follow_bottom(out)?;
                for (c, (key, mods)) in text.chars().zip(keys) {
                    let send = self.client_pane().key_down_request(key, mods);
                    self.requests.request(format!("the typed {c:?}"), send)?;
                }
            }
            In::Paste { text } => {
                self.follow_bottom(out)?;
                let send = self.client_pane().paste_request(&text);
                self.requests.request(
                    format!("a paste of {} characters", text.chars().count()),
                    send,
                )?;
            }
            In::Mouse {
                kind,
                x,
                y,
                button,
                mods,
            } => {
                let (cols, rows) = self.screen.size();
                if x >= cols || y < 0 || y as usize >= rows {
                    bail!("({x}, {y}) is outside the {cols}x{rows} screen");
                }
                let event = mouse_event(kind, x, y, button, &mods)?;
                self.follow_bottom(out)?;
                let send = self.client_pane().mouse_event_request(event);
                self.requests
                    .request(format!("a mouse {kind:?} at ({x}, {y})"), send)?;
            }
            In::Scroll { rows } => {
                let dims = self.pane.get_dimensions();
                let base = self.view_top.unwrap_or(dims.physical_top);
                let top =
                    (base - rows as StableRowIndex).clamp(dims.scrollback_top, dims.physical_top);
                self.view_top = (top < dims.physical_top).then_some(top);
                self.frame(out)?;
            }
            In::Bottom => self.follow_bottom(out)?,
            In::Resize { cols, rows } => {
                if self.tag.is_some() {
                    bail!(
                        "a pane of a tab relay is sized by the tab's layout; \
                         resize the tab, with no \"pane\""
                    );
                }
                if !self.sized_by_input {
                    bail!("this relay does not size its pane; start it with --sized-by-input");
                }
                if cols == 0 || rows == 0 {
                    bail!("a pane cannot be {cols}x{rows}");
                }
                let dims = self.pane.get_dimensions();
                let send = self.client_pane().resize_request(TerminalSize {
                    cols,
                    rows,
                    pixel_width: 0,
                    pixel_height: 0,
                    dpi: dims.dpi,
                })?;
                self.requests
                    .request(format!("a resize to {cols}x{rows}"), send)?;
            }
            In::Repaint => {
                self.screen.invalidate();
                self.frame(out)?;
            }
        }
        Ok(())
    }

    fn follow_bottom<W: Write>(&mut self, out: &mut Output<W>) -> anyhow::Result<()> {
        if self.view_top.take().is_some() {
            self.frame(out)?;
        }
        Ok(())
    }

    fn frame<W: Write>(&mut self, out: &mut Output<W>) -> anyhow::Result<()> {
        let dims = self.pane.get_dimensions();
        let (cols, rows) = (dims.cols, dims.viewport_rows);
        if (cols, rows) != self.screen.size() {
            self.screen = Screen::new(cols, rows);
            self.send(out, &Out::Size { cols, rows })?;
        }
        let top = match self.view_top {
            Some(top) if top >= dims.scrollback_top && top < dims.physical_top => top,
            _ => {
                self.view_top = None;
                dims.physical_top
            }
        };
        let (first, lines) = self.pane.get_lines(top..top + rows as StableRowIndex);
        if first != top || lines.len() != rows {
            bail!(
                "asked for {rows} rows from {top}, the pane gave {} from {first}",
                lines.len()
            );
        }
        let cursor = if self.view_top.is_none() {
            cursor_on_screen(self.pane.get_cursor_position(), top, rows)
        } else {
            None
        };
        let data = self.screen.update(lines, cursor)?;
        if !data.is_empty() {
            self.send(out, &Out::Screen { data })?;
        }
        let title = self.pane.get_title();
        if title != self.title {
            self.title = title.clone();
            self.send(out, &Out::Title { text: title })?;
        }
        let grab = self.pane.is_mouse_grabbed();
        if grab != self.mouse_grab {
            self.mouse_grab = grab;
            self.send(out, &Out::MouseGrab { on: grab })?;
        }
        Ok(())
    }
}

impl<W: Write> Relaying<W> for PaneRelay {
    fn handle(&mut self, event: Event, out: &mut Output<W>) -> anyhow::Result<Flow> {
        let mine = self.pane.pane_id();
        match event {
            Event::Output(id) if id == mine => self.schedule_frame(),
            Event::Frame(id) if id == mine => self.scheduled_frame(out)?,
            Event::Tick => {
                if self.poll() {
                    out.send(&Out::Exit)?;
                    return Ok(Flow::Stop);
                }
            }
            Event::Alert(id, alert) if id == mine => self.alert(alert, out)?,
            Event::Clipboard(id, text) if id == mine => out.send(&Out::Clipboard { text })?,
            Event::Removed(id) if id == mine => {
                out.send(&Out::Exit)?;
                return Ok(Flow::Stop);
            }
            _ => {}
        }
        Ok(Flow::Continue)
    }

    fn input(&mut self, line: &str, out: &mut Output<W>) -> anyhow::Result<()> {
        let input: In = serde_json::from_str(line).context("an input line is not a relay event")?;
        PaneRelay::input(self, input, out)
    }
}

/// Every pane of a tab, and its layout, following one pane from tab to tab.
struct TabRelay {
    /// The pane whose tab is relayed, by its multiplexer id.
    follow: PaneId,
    /// The tab relayed, by its multiplexer id.
    tab_id: Option<TabId>,
    /// The layout as last sent.
    layout: Option<TabLayout>,
    /// The tab's panes, by their multiplexer ids.
    panes: BTreeMap<PaneId, PaneRelay>,
    domain: Arc<ClientDomain>,
    client: Client,
    sized_by_input: bool,
    /// Only the layout is relayed: the panes are neither mirrored nor
    /// greeted.
    layout_only: bool,
    tx: smol::channel::Sender<Event>,
    requests: Rc<Requests>,
    /// A listing of the multiplexer's panes is on its way.
    listing: bool,
    /// The layout changed again while it was: list once more.
    list_again: bool,
}

impl TabRelay {
    /// Lists the multiplexer's panes, to learn the tab's layout: one listing
    /// at a time, and one more after it for what changed meanwhile.
    fn list(&mut self) {
        if self.listing {
            self.list_again = true;
            return;
        }
        self.listing = true;
        let client = self.client.clone();
        let tx = self.tx.clone();
        promise::spawn::spawn(async move {
            tx.send(Event::Listed(client.list_panes().await)).await.ok();
        })
        .detach();
    }

    fn by_local_id(&mut self, local_id: PaneId) -> Option<&mut PaneRelay> {
        self.panes
            .values_mut()
            .find(|relay| relay.pane.pane_id() == local_id)
    }

    /// Takes in a listing of the multiplexer's panes: sends `gone` for each
    /// pane that left the tab, the tab's layout if it changed, and `hello`
    /// for each pane that came. Stops once the tab is gone.
    fn apply<W: Write>(
        &mut self,
        panes: ListPanesResponse,
        out: &mut Output<W>,
    ) -> anyhow::Result<Flow> {
        let tab_id = tab_of_pane(&panes, self.follow).or(self.tab_id);
        let layout = tab_id
            .and_then(|tab_id| tab_tree(&panes, tab_id))
            .and_then(TabLayout::from_tree);
        let Some(layout) = layout else {
            for (pane_id, _) in std::mem::take(&mut self.panes) {
                out.send_for(Some(pane_id), &Out::Gone)?;
            }
            out.send(&Out::Exit)?;
            return Ok(Flow::Stop);
        };
        self.tab_id = Some(layout.tab_id);
        let ids = layout.pane_ids();
        let left: Vec<PaneId> = self
            .panes
            .keys()
            .filter(|id| !ids.contains(id))
            .copied()
            .collect();
        for pane_id in left {
            self.panes.remove(&pane_id);
            out.send_for(Some(pane_id), &Out::Gone)?;
        }
        if self.layout.as_ref() != Some(&layout) {
            out.send(&Out::Layout {
                tab: layout.clone(),
            })?;
            self.layout = Some(layout);
        }
        if self.layout_only {
            return Ok(Flow::Continue);
        }
        let mux = Mux::get();
        for pane_id in ids {
            if self.panes.contains_key(&pane_id) {
                continue;
            }
            // A pane the relay's mirror does not have yet comes with the
            // mirror's next update, which lists the panes again.
            let Some(pane) = self
                .domain
                .remote_to_local_pane_id(pane_id)
                .and_then(|local_id| mux.get_pane(local_id))
            else {
                continue;
            };
            let mut relay = PaneRelay::new(
                pane,
                Some(pane_id),
                self.sized_by_input,
                self.tx.clone(),
                Rc::clone(&self.requests),
            )?;
            relay.hello(out)?;
            self.panes.insert(pane_id, relay);
        }
        Ok(Flow::Continue)
    }

    /// Sizes the whole tab: the multiplexer shares the size out among its
    /// panes by the tab's layout, and every client sees the new layout. The
    /// request is queued, so a relay whose input ends has its answer before
    /// it stops; a refusal, because a desktop window sizes the tab or the
    /// layout does not fit the size, comes back as an error event.
    fn resize(&self, cols: usize, rows: usize) -> anyhow::Result<()> {
        if !self.sized_by_input {
            bail!("this relay does not size its tab; start it with --sized-by-input");
        }
        if cols == 0 || rows == 0 {
            bail!("a tab cannot be {cols}x{rows}");
        }
        let tab_id = self.tab_id.expect("a tab relay has its tab once it starts");
        let dpi = self
            .domain
            .remote_to_local_tab_id(tab_id)
            .and_then(|local_id| Mux::get().get_tab(local_id))
            .map_or(0, |tab| tab.get_size().dpi);
        let sized = self.domain.resize_remote_tab(
            tab_id,
            TerminalSize {
                cols,
                rows,
                pixel_width: 0,
                pixel_height: 0,
                dpi,
            },
        )?;
        self.requests.request(
            format!("a resize of the tab to {cols}x{rows}"),
            async move { sized.await.map_err(anyhow::Error::from) },
        )
    }
}

impl<W: Write> Relaying<W> for TabRelay {
    fn handle(&mut self, event: Event, out: &mut Output<W>) -> anyhow::Result<Flow> {
        match event {
            Event::Output(id) => {
                if let Some(relay) = self.by_local_id(id) {
                    relay.schedule_frame();
                }
            }
            Event::Frame(id) => {
                if let Some(relay) = self.by_local_id(id) {
                    relay.scheduled_frame(out)?;
                }
            }
            Event::Tick => {
                for relay in self.panes.values() {
                    relay.poll();
                }
                let known = self.layout.as_ref().map(|layout| layout.pane_ids().len());
                if !self.layout_only && known != Some(self.panes.len()) {
                    self.list();
                }
            }
            Event::Alert(id, alert) => {
                if let Some(relay) = self.by_local_id(id) {
                    relay.alert(alert, out)?;
                }
            }
            Event::Clipboard(id, text) => {
                if let Some(relay) = self.by_local_id(id) {
                    relay.send(out, &Out::Clipboard { text })?;
                }
            }
            Event::Removed(_) | Event::LayoutChanged => self.list(),
            Event::Listed(panes) => {
                self.listing = false;
                if let Flow::Stop = self.apply(panes?, out)? {
                    return Ok(Flow::Stop);
                }
                if std::mem::take(&mut self.list_again) {
                    self.list();
                }
            }
            Event::Input(_) | Event::InputFailed(_) | Event::InputEnded => {}
            Event::RequestFailed(_) => {}
        }
        Ok(Flow::Continue)
    }

    fn input(&mut self, line: &str, out: &mut Output<W>) -> anyhow::Result<()> {
        let mut value: serde_json::Value =
            serde_json::from_str(line).context("an input line is not a relay event")?;
        let pane = value.as_object_mut().and_then(|event| event.remove("pane"));
        let input: In =
            serde_json::from_value(value).context("an input line is not a relay event")?;
        match (pane, input) {
            (None, In::Resize { cols, rows }) => self.resize(cols, rows),
            (Some(_), _) if self.layout_only => bail!(
                "a layout-only relay relays no pane; its input takes only a resize of the tab"
            ),
            (None, _) => bail!(
                "a tab relay's events name their \"pane\"; only a resize of the whole tab \
                 names none"
            ),
            (Some(pane), input) => {
                let pane_id = pane
                    .as_u64()
                    .ok_or_else(|| anyhow!("\"pane\" is {pane}, not a pane id"))?
                    as PaneId;
                let relay = self
                    .panes
                    .get_mut(&pane_id)
                    .ok_or_else(|| anyhow!("pane {pane_id} is not in the relayed tab"))?;
                relay.input(input, out)
            }
        }
    }
}

fn cursor_on_screen(
    position: StableCursorPosition,
    top: StableRowIndex,
    rows: usize,
) -> Option<Cursor> {
    let row = position.y - top;
    (row >= 0 && (row as usize) < rows).then(|| Cursor {
        x: position.x,
        row: row as usize,
        shape: position.shape,
        visibility: position.visibility,
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use termwiz::cell::{Cell, Intensity};

    fn line(text: &str) -> Line {
        Line::from_text(text, &CellAttributes::blank(), 0, None)
    }

    fn blank(rows: usize) -> Vec<Line> {
        (0..rows).map(|_| line("")).collect()
    }

    fn at(x: usize, row: usize) -> Option<Cursor> {
        Some(Cursor {
            x,
            row,
            shape: CursorShape::Default,
            visibility: CursorVisibility::Visible,
        })
    }

    /// A terminal of this size after these escapes. Like a terminal, it
    /// takes text printed one character after another as one string, so
    /// characters that make one grapheme together share a cell, and any
    /// escape between them keeps them apart.
    fn painted(cols: usize, rows: usize, escapes: &[String]) -> Surface {
        let mut parser = termwiz::escape::parser::Parser::new();
        let mut surface = Surface::new(cols, rows);
        let mut printed = String::new();
        for data in escapes {
            parser.parse(data.as_bytes(), |action| {
                use termwiz::escape::Action;
                match action {
                    Action::Print(c) => {
                        printed.push(c);
                        return;
                    }
                    Action::PrintString(s) => {
                        printed.push_str(&s);
                        return;
                    }
                    _ => {}
                }
                if !printed.is_empty() {
                    surface.add_change(Change::Text(std::mem::take(&mut printed)));
                }
                match action {
                    Action::CSI(termwiz::escape::CSI::Cursor(
                        termwiz::escape::csi::Cursor::Position { line, col },
                    )) => {
                        surface.add_change(Change::CursorPosition {
                            x: Position::Absolute(col.as_zero_based() as usize),
                            y: Position::Absolute(line.as_zero_based() as usize),
                        });
                    }
                    Action::CSI(termwiz::escape::CSI::Edit(
                        termwiz::escape::csi::Edit::EraseInDisplay(_),
                    )) => {
                        surface.add_change(Change::ClearScreen(ColorAttribute::Default));
                    }
                    Action::CSI(termwiz::escape::CSI::Edit(
                        termwiz::escape::csi::Edit::EraseInLine(_),
                    )) => {
                        surface.add_change(Change::ClearToEndOfLine(ColorAttribute::Default));
                    }
                    _ => {}
                }
            });
        }
        if !printed.is_empty() {
            surface.add_change(Change::Text(printed));
        }
        surface
    }

    /// What a terminal of this size shows after these escapes.
    fn shown(cols: usize, rows: usize, escapes: &[String]) -> Vec<String> {
        painted(cols, rows, escapes)
            .screen_lines()
            .iter()
            .map(|l| l.as_str().trim_end().to_string())
            .collect()
    }

    #[test]
    fn the_first_update_paints_everything_and_clears_first() {
        let mut screen = Screen::new(10, 2);
        let data = screen
            .update(vec![line("hello"), line("")], at(5, 0))
            .unwrap();
        assert!(data.contains("hello"), "{:?}", data);
        assert!(
            data.contains("\u{1b}[2J") || data.contains("\u{1b}[H\u{1b}[2J"),
            "{:?}",
            data
        );
        assert_eq!(shown(10, 2, &[data]), vec!["hello", ""]);
    }

    #[test]
    fn an_unchanged_screen_sends_nothing() {
        let mut screen = Screen::new(10, 2);
        screen
            .update(vec![line("hello"), line("")], at(5, 0))
            .unwrap();
        let again = screen
            .update(vec![line("hello"), line("")], at(5, 0))
            .unwrap();
        assert_eq!(again, "");
    }

    #[test]
    fn a_moved_cursor_alone_is_sent() {
        let mut screen = Screen::new(10, 2);
        let first = screen
            .update(vec![line("hello"), line("")], at(5, 0))
            .unwrap();
        let moved = screen
            .update(vec![line("hello"), line("")], at(2, 1))
            .unwrap();
        assert!(moved.contains("\u{1b}[2;3H"), "{:?}", moved);
        assert!(!moved.contains("hello"), "{:?}", moved);
        assert_eq!(shown(10, 2, &[first, moved]), vec!["hello", ""]);
    }

    #[test]
    fn a_shorter_line_clears_what_was_to_its_right() {
        let mut screen = Screen::new(10, 2);
        let first = screen
            .update(vec![line("hello"), line("world")], at(0, 1))
            .unwrap();
        let second = screen
            .update(vec![line("hi"), line("world")], at(2, 0))
            .unwrap();
        assert_eq!(shown(10, 2, &[first, second]), vec!["hi", "world"]);
    }

    #[test]
    fn wide_characters_take_two_cells() {
        let mut screen = Screen::new(6, 1);
        let first = screen.update(vec![line("ab")], at(0, 0)).unwrap();
        let second = screen.update(vec![line("中x")], at(3, 0)).unwrap();
        assert_eq!(shown(6, 1, &[first, second]), vec!["中x"]);
    }

    #[test]
    fn each_grapheme_is_shown_in_the_cell_the_pane_holds_it_in() {
        // A program wrote an emoji and a skin tone modifier in cells of their
        // own, as one that counts widths by character does: the pane holds
        // both, two cells each. A terminal reading their text one after the
        // other would make one emoji of them, and draw what follows two cells
        // early.
        let mut pane = Line::with_width(8, 0);
        pane.set_cell(
            0,
            Cell::new_grapheme_with_width("👍", 2, CellAttributes::blank()),
            0,
        );
        pane.set_cell(
            2,
            Cell::new_grapheme_with_width("🏽", 2, CellAttributes::blank()),
            0,
        );
        pane.set_cell(4, Cell::new('|', CellAttributes::blank()), 0);
        let mut screen = Screen::new(8, 1);
        let data = screen.update(vec![pane], at(5, 0)).unwrap();

        let terminal = painted(8, 1, &[data]);
        let cells: Vec<(usize, String)> = terminal.screen_lines()[0]
            .visible_cells()
            .filter(|cell| cell.str() != " ")
            .map(|cell| (cell.cell_index(), cell.str().to_string()))
            .collect();
        assert_eq!(
            cells,
            vec![
                (0, "👍".to_string()),
                (2, "🏽".to_string()),
                (4, "|".to_string())
            ]
        );
    }

    #[test]
    fn attributes_are_sent() {
        let mut screen = Screen::new(10, 1);
        let mut bold = CellAttributes::blank();
        bold.set_intensity(Intensity::Bold);
        let data = screen
            .update(vec![Line::from_text("loud", &bold, 0, None)], at(4, 0))
            .unwrap();
        assert!(data.contains("\u{1b}[0;1mloud"), "{:?}", data);
    }

    #[test]
    fn a_cursor_off_the_screen_is_hidden() {
        let mut screen = Screen::new(10, 2);
        let data = screen.update(blank(2), None).unwrap();
        assert!(data.contains("\u{1b}[?25l"), "{:?}", data);
    }

    #[test]
    fn invalidating_paints_everything_again() {
        let mut screen = Screen::new(10, 1);
        screen.update(vec![line("hello")], at(0, 0)).unwrap();
        screen.invalidate();
        let data = screen.update(vec![line("hello")], at(0, 0)).unwrap();
        assert!(data.contains("hello"), "{:?}", data);
    }

    #[test]
    fn a_wrong_number_of_lines_is_refused() {
        let mut screen = Screen::new(10, 2);
        assert!(screen.update(blank(1), None).is_err());
    }

    #[test]
    fn input_lines_parse() {
        assert_eq!(
            serde_json::from_str::<In>(r#"{"t":"key","key":"UpArrow","mods":"CTRL"}"#).unwrap(),
            In::Key {
                key: "UpArrow".into(),
                mods: "CTRL".into()
            }
        );
        assert_eq!(
            serde_json::from_str::<In>(r#"{"t":"scroll","rows":-3}"#).unwrap(),
            In::Scroll { rows: -3 }
        );
        assert!(serde_json::from_str::<In>(r#"{"t":"exec","cmd":"rm"}"#).is_err());
        assert!(serde_json::from_str::<In>(r#"{"t":"key","key":"a","extra":1}"#).is_err());
    }

    #[test]
    fn keys_become_what_a_gui_sends() {
        use termwiz::input::KeyCode as KC;
        assert_eq!(
            key_input("a", "CTRL", false).unwrap(),
            (KC::Char('a'), KeyModifiers::CTRL)
        );
        assert_eq!(key_input("Enter", "", false).unwrap().0, KC::Enter);
        assert_eq!(key_input("Escape", "", false).unwrap().0, KC::Escape);
        assert_eq!(key_input("Backspace", "", false).unwrap().0, KC::Backspace);
        assert_eq!(key_input("Backspace", "", true).unwrap().0, KC::Delete);
        assert_eq!(
            key_input("Tab", "SHIFT", false).unwrap(),
            (KC::Tab, KeyModifiers::SHIFT)
        );
        assert_eq!(key_input("F5", "", false).unwrap().0, KC::Function(5));
        assert_eq!(key_input("UpArrow", "", false).unwrap().0, KC::UpArrow);
        assert!(key_input("NotAKey", "", false).is_err());
        assert!(key_input("VoidSymbol", "", false).is_err());
        assert!(key_input("a", "LEADER", false).is_err());
        assert!(key_input("a", "HYPER", false).is_err());
    }

    #[test]
    fn mouse_events_carry_their_cell_and_button() {
        let event = mouse_event(MouseKind::Press, 3, 4, Button::Left, "SHIFT").unwrap();
        assert_eq!(event.kind, MouseEventKind::Press);
        assert_eq!((event.x, event.y), (3, 4));
        assert_eq!(event.button, MouseButton::Left);
        assert_eq!(event.modifiers, KeyModifiers::SHIFT);
        let wheel = mouse_event(MouseKind::Press, 0, 0, Button::WheelUp, "").unwrap();
        assert_eq!(wheel.button, MouseButton::WheelUp(1));
    }

    #[test]
    fn requests_are_sent_one_after_another_and_every_failure_is_named() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let log = Rc::new(RefCell::new(vec![]));
        let (requests, queue) = smol::channel::unbounded();
        for (n, fails) in [(1, false), (2, true), (3, false)] {
            let log = Rc::clone(&log);
            requests
                .try_send(Request {
                    what: format!("request {n}"),
                    send: Box::pin(async move {
                        log.borrow_mut().push(format!("start {n}"));
                        // A request that takes a while must still end before
                        // the next starts.
                        smol::future::yield_now().await;
                        smol::future::yield_now().await;
                        log.borrow_mut().push(format!("end {n}"));
                        if fails {
                            bail!("the pane is gone");
                        }
                        Ok(())
                    }),
                })
                .unwrap();
        }
        // The queue closes with requests still in it: they are sent all the same.
        drop(requests);
        let heard = RefCell::new(vec![]);
        let failures = smol::block_on(send_requests(queue, |failure| {
            heard.borrow_mut().push(failure.clone())
        }));
        assert_eq!(
            *log.borrow(),
            vec!["start 1", "end 1", "start 2", "end 2", "start 3", "end 3"]
        );
        assert_eq!(
            failures,
            vec![Failure {
                message: "request 2 did not reach the pane: the pane is gone".to_string(),
                sized_by_window: None,
            }]
        );
        assert_eq!(heard.into_inner(), failures);
    }

    #[test]
    fn a_resize_a_window_rule_refused_says_the_rule() {
        let (requests, queue) = smol::channel::unbounded::<Request>();
        let rule = "a desktop window sizes each tab of the workspace it shows";
        requests
            .try_send(Request {
                what: "a resize of the tab to 160x48".to_string(),
                send: Box::pin(async move {
                    Err(anyhow::Error::from(TabNotSized::SizedByWindow(
                        rule.to_string(),
                    )))
                }),
            })
            .unwrap();
        drop(requests);
        let failures = smol::block_on(send_requests(queue, |_| {}));
        assert_eq!(
            failures,
            vec![Failure {
                message: format!("a resize of the tab to 160x48 was refused: {}", rule),
                sized_by_window: Some(rule.to_string()),
            }]
        );
        let event = serde_json::to_value(failures[0].event()).unwrap();
        assert_eq!(event["t"], "error");
        assert_eq!(event["sized_by_window"], rule);
    }

    #[test]
    fn output_is_one_tagged_object_per_line() {
        let mut out = Output::new(vec![]);
        out.send(&Out::Size { cols: 80, rows: 24 }).unwrap();
        out.send(&Out::Bell).unwrap();
        assert_eq!(
            String::from_utf8(out.writer).unwrap(),
            "{\"t\":\"size\",\"cols\":80,\"rows\":24}\n{\"t\":\"bell\"}\n"
        );
    }

    #[test]
    fn an_event_of_a_tab_relay_names_its_pane_last() {
        let mut out = Output::new(vec![]);
        out.send_for(Some(7), &Out::Size { cols: 80, rows: 24 })
            .unwrap();
        out.send_for(Some(3), &Out::Gone).unwrap();
        assert_eq!(
            String::from_utf8(out.writer).unwrap(),
            "{\"t\":\"size\",\"cols\":80,\"rows\":24,\"pane\":7}\n{\"t\":\"gone\",\"pane\":3}\n"
        );
    }
}
