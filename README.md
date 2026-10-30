## Changes from WezTerm

### Tabs

- **Drag tabs to reorder them.** Drag a tab along the tab bar with the left mouse button. From [wezterm#6527](https://github.com/wezterm/wezterm/pull/6527) by @Jefffrey, and [wezterm#7926](https://github.com/wezterm/wezterm/pull/7926) by @jayjohnston, which redraws the tab bar during the drag and adds a floating preview to the fancy tab bar.
- **Tab widths that fit the bar.** When the retro tab bar's titles do not all fit, a title shorter than an even share keeps its full width, and the room it leaves goes to the longer titles, instead of every tab being cut to an even share (`wezterm-gui/src/tabbar.rs`, `tab_widths`).
- **Right-clicking a tab no longer opens the tab navigator.** It opens the context menu instead (below). `ShowTabNavigator` is still available as a key assignment.

### Tab and window variables

- **Named values on tabs and windows that only programs set.** `wezterm cli set-tab-var NAME VALUE` and `wezterm cli set-window-var NAME VALUE` (or `--unset NAME`), and `tab:set_var()` / `window:set_var()` in Lua, store values a program recognizes a tab or window by later. `wezterm cli list --format json` reports them as `tab_vars` and `window_vars`, and Lua reads them with `get_vars()`. Unlike titles, which any program in a pane can change with an escape sequence, nothing running in the panes can touch them. A multiplexer server's values are mirrored by every GUI attached to it, and a value a GUI sets on a server's tab or window is set on the server. This changes the multiplexer protocol.

### Window placement and zoom in Lua

- **What a configuration needs to give a new window the placement and zoom an earlier one had**, which WezTerm keeps only while a window is open:
  - `window:get_position()` reports the top left of the window's content area, in the coordinates `window:set_position()` takes, so moving a window to its reported position leaves it where it is. On Wayland, which tells applications nothing of where their windows are, it returns `nil` ([docs](docs/config/lua/window/get_position.md)).
  - `window:get_dimensions()` also reports `is_maximized` ([docs](docs/config/lua/window/get_dimensions.md)).
  - `window:get_font_scale()` and `window:set_font_scale(scale)` read and set the zoom that IncreaseFontSize, DecreaseFontSize and ResetFontSize change ([docs](docs/config/lua/window/get_font_scale.md), [docs](docs/config/lua/window/set_font_scale.md)).
  - A window applies the changes Lua asks of it, `set_inner_size`, `set_font_scale`, `set_position`, `maximize`, `restore` and `toggle_fullscreen`, in the order asked. Before, a move or a maximize went straight to the window and could overtake a resize asked for first.
- **On X11 a window declares static gravity** in its size hints, the meaning `set_position` gives a position: where the content area goes, not the frame the window manager draws around it. The window manager reads a window's position by the window's gravity whenever it configures it; declaring none, a window placed with `set_position` was placed by its frame, a title bar lower, the next time it was resized or restored from maximized (Openbox does this).

### Context menu

- **A right-click menu, drawn by WezTerm, the same on every platform.**
  - In a terminal: Copy (greyed out without a selection), Paste, Split Right, Split Down, Close Pane, New Tab, and Command Palette, with their shortcuts.
  - On the tab bar: New Tab, Close Tab (the tab clicked), Minimize, Maximize/Restore, and Close Window.
  - It uses the command palette's font and colours, and is navigated with the mouse or the arrow keys, Enter, and Escape.
  - A program that tracks the mouse still gets the right-click; Shift+right-click always opens the menu.
  - It is the new `ShowContextMenu` key assignment, bound to a right-click by default ([docs](docs/config/lua/keyassignment/ShowContextMenu.md)).

### Wayland

- **A window frame on desktops that leave decorations to the application, such as GNOME.** WezTerm draws its own title bar and borders when the compositor refuses server-side decorations, rather than showing no frame at all. Dragging an empty part of the tab bar moves the window. From [wezterm#8145](https://github.com/wezterm/wezterm/pull/8145) by @AlexKichkailo.
- **That frame is counted in the window geometry.** The size the compositor suggests includes the frame, and the geometry WezTerm declares now does too. Before, resizing from a border left the terminal smaller than the window and stopped following the pointer.
- **Resize cursors on the frame's borders and corners while hovering them**, not only once a drag has begun.
- **A click on the title bar stays on the title bar.** Clicking the frame of a window that did not have the keyboard selected the tab under the click as well: the keyboard entering the window replaced the surface pointer events were sent to, so the press went to the terminal at the title bar's coordinates, which are the tab bar's. Each pointer event now goes to the surface it arrived on.

### macOS

- **The title bar shows the terminal's background, not the desktop.** An opaque window's background is now the color scheme's background color rather than clear. Where macOS draws the title bar translucent, it showed whatever was behind the window, leaving only the title and the window buttons visible.

### Relaying a pane

- **`wezterm cli relay --pane-id ID` lets another program show a pane and type into it**: a terminal emulator in a web page, for instance. The relay attaches to the multiplexer as a GUI does and writes the pane's screen as escape sequences, a full paint and then only what changed, one JSON event per line on stdout. It reads keys, text, paste and mouse events on stdin, and the multiplexer encodes them with the modes of the terminal in the pane, so the program in it receives what a keyboard would send. It never resizes the pane unless started with `--sized-by-input`, and never takes focus. Scrollback, title, clipboard (OSC 52), bell and the pane's colors come through as events too ([docs](docs/cli/cli/relay.md)).
- **Clients say what they are for.** A client attaching to a multiplexer registers as a `gui`, a `relay`, or a short-lived `command`, and `wezterm cli list-clients` shows it, so a program can tell whether a GUI is showing a multiplexer's windows. A relay, which shows no windows, also leaves the panes' colors to the GUIs, where a GUI tells the server the colors it uses for every pane.
- **A key is turned into terminal input in one place** (`termwiz::input::KeyInput`), which the GUI and the relay share.
- **A multiplexer with no domain of its own makes no ssh-agent link.** `wezterm cli proxy` and the relay run no programs that would use one, and each run used to leave an `agent.PID` link behind in the runtime directory.

### Multiplexer clients

These affect a window attached to a mux server, such as with `wezterm connect`:

- **A re-sync takes the server's tab sizes without sending them back.** A client re-syncs whenever the server reports a tab resized, and used to send the size it had just been told back to the server as a new request. While the window was resized, tabs bounced between sizes, with the programs in them redrawing at each, and could be left at a stale size.
- **A pane's resizes are sent one at a time, the latest size last.** Before, each one was sent on its own, unordered, and an earlier size could be the last one applied.
- **Closing a window closes its tabs.** Closing a window whose tabs a server holds kills those tabs in the server, as closing each tab does. Before, it detached the window from the server's whole domain instead: every other window of that server disappeared with it, and the programs in all of them, the closed window's included, ran on. Quitting WezTerm still leaves a server's programs running. A window WezTerm fails to create still detaches its domain, so a failure ends no programs.

Several of these change the multiplexer protocol (codec version 48): GUIs, servers and `wezterm cli` must be built from this fork alike.

---

# Wez's Terminal

<img height="128" alt="WezTerm Icon" src="https://raw.githubusercontent.com/wezterm/wezterm/main/assets/icon/wezterm-icon.svg" align="left"> *A GPU-accelerated cross-platform terminal emulator and multiplexer written by <a href="https://github.com/wez">@wez</a> and implemented in <a href="https://www.rust-lang.org/">Rust</a>*

User facing docs and guide at: https://wezterm.org/

![Screenshot](docs/screenshots/two.png)

*Screenshot of wezterm on macOS, running vim*

## Installation

https://wezterm.org/installation

## Getting help

This is a spare time project, so please bear with me.  There are a couple of channels for support:

* You can use the [GitHub issue tracker](https://github.com/wezterm/wezterm/issues) to see if someone else has a similar issue, or to file a new one.
* Start or join a thread in our [GitHub Discussions](https://github.com/wezterm/wezterm/discussions); if you have general
  questions or want to chat with other wezterm users, you're welcome here!
* There is a [Matrix room via Element.io](https://matrix.to/#/#wezterm:matrix.org)
  for (potentially!) real time discussions.

The GitHub Discussions and Element/Gitter rooms are better suited for questions
than bug reports, but don't be afraid to use whichever you are most comfortable
using and we'll work it out.

## Supporting the Project

If you use and like WezTerm, please consider sponsoring it: your support helps
to cover the fees required to maintain the project and to validate the time
spent working on it!

[Read more about sponsoring](https://wezterm.org/sponsor.html).

* [![Sponsor WezTerm](https://img.shields.io/github/sponsors/wez?label=Sponsor%20WezTerm&logo=github&style=for-the-badge)](https://github.com/sponsors/wez)
* [Patreon](https://patreon.com/WezFurlong)
* [Ko-Fi](https://ko-fi.com/wezfurlong)
* [Liberapay](https://liberapay.com/wez)
