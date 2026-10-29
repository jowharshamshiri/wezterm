//! `wezterm cli relay`: one pane of a multiplexer, mirrored for another
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

use anyhow::{anyhow, bail, Context};
use clap::Parser;
use config::UnixDomain;
use mux::client::ClientRole;
use mux::domain::Domain;
use mux::pane::{Pane, PaneId};
use mux::renderable::StableCursorPosition;
use mux::{Mux, MuxNotification};
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::io::{BufRead, Read, Write};
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
pub struct RelayCommand {
    /// The pane to relay, as `wezterm cli list` shows it.
    #[arg(long = "pane-id")]
    pane_id: PaneId,

    /// Let `resize` events size the pane. Without it the pane keeps the
    /// size its other clients give it, and `resize` is refused.
    #[arg(long = "sized-by-input")]
    sized_by_input: bool,
}

/// Run the relay until stdin ends or the pane does. It runs on its own
/// executor rather than `wezterm cli`'s: the client's panes spawn tasks
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
    /// First, once: the pane's size, colors, title and mouse mode.
    Hello {
        cols: usize,
        rows: usize,
        palette: Palette,
        title: String,
        mouse_grab: bool,
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
    /// The pane is gone; the relay ends.
    Exit,
    /// An input line was refused, or the relay failed.
    Error {
        message: String,
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
        let mut line = serde_json::to_vec(out)?;
        line.push(b'\n');
        self.writer.write_all(&line).context("writing to stdout")?;
        self.writer.flush().context("writing to stdout")
    }
}

enum Event {
    Output,
    Frame,
    Tick,
    Alert(Alert),
    Clipboard(Option<String>),
    Removed,
    Input(String),
    InputFailed(String),
    InputEnded,
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
    let local_id = domain
        .remote_to_local_pane_id(cmd.pane_id)
        .ok_or_else(|| anyhow!("the multiplexer has no pane {}", cmd.pane_id))?;
    let pane = mux
        .get_pane(local_id)
        .ok_or_else(|| anyhow!("pane {} went away while attaching", cmd.pane_id))?;

    let (tx, rx) = smol::channel::unbounded::<Event>();

    // A subscriber runs while the mux and, for output, the pane are
    // locked: it only passes the news on.
    mux.subscribe({
        let tx = tx.clone();
        move |notification| {
            let event = match notification {
                MuxNotification::PaneOutput(id) if id == local_id => Event::Output,
                MuxNotification::Alert { pane_id, alert } if pane_id == local_id => {
                    Event::Alert(alert)
                }
                MuxNotification::AssignClipboard {
                    pane_id, clipboard, ..
                } if pane_id == local_id => Event::Clipboard(clipboard),
                MuxNotification::PaneRemoved(id) if id == local_id => Event::Removed,
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

    let mut relay = Relay::new(pane, cmd.sized_by_input, tx);
    relay.hello(out)?;
    while let Ok(event) = rx.recv().await {
        match relay.handle(event, out)? {
            Flow::Continue => {}
            Flow::Stop => break,
        }
    }
    Ok(())
}

enum Flow {
    Continue,
    Stop,
}

struct Relay {
    pane: Arc<dyn Pane>,
    sized_by_input: bool,
    tx: smol::channel::Sender<Event>,
    screen: Screen,
    /// The stable row at the top of the view while it is scrolled back;
    /// `None` follows the bottom of the pane.
    view_top: Option<StableRowIndex>,
    frame_pending: bool,
    title: String,
    mouse_grab: bool,
}

impl Relay {
    fn new(pane: Arc<dyn Pane>, sized_by_input: bool, tx: smol::channel::Sender<Event>) -> Self {
        let dims = pane.get_dimensions();
        Self {
            screen: Screen::new(dims.cols, dims.viewport_rows),
            title: pane.get_title(),
            mouse_grab: pane.is_mouse_grabbed(),
            pane,
            sized_by_input,
            tx,
            view_top: None,
            frame_pending: false,
        }
    }

    fn hello<W: Write>(&mut self, out: &mut Output<W>) -> anyhow::Result<()> {
        let (cols, rows) = self.screen.size();
        out.send(&Out::Hello {
            cols,
            rows,
            palette: Palette::from(&self.pane.palette()),
            title: self.title.clone(),
            mouse_grab: self.mouse_grab,
        })?;
        self.frame(out)
    }

    fn handle<W: Write>(&mut self, event: Event, out: &mut Output<W>) -> anyhow::Result<Flow> {
        match event {
            Event::Output => self.schedule_frame(),
            Event::Frame => {
                self.frame_pending = false;
                self.frame(out)?;
            }
            Event::Tick => {
                // Polls the pane, as a GUI does when it paints.
                let dims = self.pane.get_dimensions();
                let top = dims.physical_top;
                self.pane
                    .get_changed_since(top..top + dims.viewport_rows as StableRowIndex, 0);
                if self.pane.is_dead() {
                    out.send(&Out::Exit)?;
                    return Ok(Flow::Stop);
                }
            }
            Event::Alert(Alert::Bell) => out.send(&Out::Bell)?,
            Event::Alert(Alert::PaletteChanged) => out.send(&Out::Palette {
                palette: Palette::from(&self.pane.palette()),
            })?,
            Event::Alert(_) => {}
            Event::Clipboard(text) => out.send(&Out::Clipboard { text })?,
            Event::Removed => {
                out.send(&Out::Exit)?;
                return Ok(Flow::Stop);
            }
            Event::Input(line) => {
                if let Err(err) = self.input(&line, out) {
                    out.send(&Out::Error {
                        message: format!("{:#}", err),
                    })?;
                }
            }
            Event::InputFailed(message) => bail!(message),
            Event::InputEnded => return Ok(Flow::Stop),
        }
        Ok(Flow::Continue)
    }

    fn schedule_frame(&mut self) {
        if self.frame_pending {
            return;
        }
        self.frame_pending = true;
        let tx = self.tx.clone();
        promise::spawn::spawn(async move {
            smol::Timer::after(FRAME_DELAY).await;
            tx.send(Event::Frame).await.ok();
        })
        .detach();
    }

    fn input<W: Write>(&mut self, line: &str, out: &mut Output<W>) -> anyhow::Result<()> {
        let input: In = serde_json::from_str(line).context("an input line is not a relay event")?;
        let swap = config::configuration().swap_backspace_and_delete;
        match input {
            In::Key { key, mods } => {
                let (key, mods) = key_input(&key, &mods, swap)?;
                self.follow_bottom(out)?;
                self.pane.key_down(key, mods)?;
            }
            In::Text { text } => {
                self.follow_bottom(out)?;
                for c in text.chars() {
                    let (key, mods) = key_input(&c.to_string(), "", swap)?;
                    self.pane.key_down(key, mods)?;
                }
            }
            In::Paste { text } => {
                self.follow_bottom(out)?;
                self.pane.send_paste(&text)?;
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
                self.pane.mouse_event(event)?;
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
                if !self.sized_by_input {
                    bail!("this relay does not size its pane; start it with --sized-by-input");
                }
                if cols == 0 || rows == 0 {
                    bail!("a pane cannot be {cols}x{rows}");
                }
                let dims = self.pane.get_dimensions();
                self.pane.resize(TerminalSize {
                    cols,
                    rows,
                    pixel_width: 0,
                    pixel_height: 0,
                    dpi: dims.dpi,
                })?;
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
            out.send(&Out::Size { cols, rows })?;
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
            out.send(&Out::Screen { data })?;
        }
        let title = self.pane.get_title();
        if title != self.title {
            self.title = title.clone();
            out.send(&Out::Title { text: title })?;
        }
        let grab = self.pane.is_mouse_grabbed();
        if grab != self.mouse_grab {
            self.mouse_grab = grab;
            out.send(&Out::MouseGrab { on: grab })?;
        }
        Ok(())
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
    use termwiz::cell::Intensity;

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

    /// What a terminal of this size shows after these escapes.
    fn shown(cols: usize, rows: usize, escapes: &[String]) -> Vec<String> {
        let mut parser = termwiz::escape::parser::Parser::new();
        let mut surface = Surface::new(cols, rows);
        for data in escapes {
            parser.parse(data.as_bytes(), |action| {
                use termwiz::escape::Action;
                match action {
                    Action::Print(c) => {
                        surface.add_change(Change::Text(c.to_string()));
                    }
                    Action::PrintString(s) => {
                        surface.add_change(Change::Text(s));
                    }
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
        surface
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
    fn output_is_one_tagged_object_per_line() {
        let mut out = Output::new(vec![]);
        out.send(&Out::Size { cols: 80, rows: 24 }).unwrap();
        out.send(&Out::Bell).unwrap();
        assert_eq!(
            String::from_utf8(out.writer).unwrap(),
            "{\"t\":\"size\",\"cols\":80,\"rows\":24}\n{\"t\":\"bell\"}\n"
        );
    }
}
