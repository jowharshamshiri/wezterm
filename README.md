# wsb-wezterm

wsb-wezterm is a fork of [WezTerm](https://github.com/wezterm/wezterm), the
GPU-accelerated cross-platform terminal emulator and multiplexer written by
[Wez Furlong](https://github.com/wez). WezTerm is Wez Furlong's work, and the
fork claims no part of it: [LICENSE.md](LICENSE.md) carries WezTerm's MIT licence
and Wez Furlong's copyright notice, which cover this code as they cover WezTerm's.
WezTerm's own documentation is at <https://wezterm.org/>, and its release
history at <https://wezterm.org/changelog.html>.

## Names

wsb-wezterm goes by its own names, so that it never takes the place of a WezTerm
installed beside it, and the two keep their configuration and state apart:

| | wsb-wezterm |
|---|---|
| Programs | `wsb-wezterm`, `wsb-wezterm-gui`, `wsb-wezterm-mux-server` |
| Application id (window class, desktop entry, macOS bundle identifier) | `com.bahramj.wsb-wezterm` |
| macOS application | `wsb-wezterm.app` |
| Configuration file | `$WSB_WEZTERM_CONFIG_FILE`, else `wsb-wezterm.lua` in `$XDG_CONFIG_HOME/wsb-wezterm` or `~/.config/wsb-wezterm`, else `~/.wsb-wezterm.lua` |
| Lua module | `require 'wsb-wezterm'` |
| Environment variables | `WSB_WEZTERM_*`, such as `WSB_WEZTERM_PANE` and `WSB_WEZTERM_UNIX_SOCKET`; `TERM_PROGRAM` is `wsb-wezterm` |
| terminfo entry | `wsb-wezterm` |
| Data, cache and runtime directories | `wsb-wezterm` in each |
| Shell integration | `wsb-wezterm.sh`, whose user variables are `WSB_WEZTERM_PROG`, `WSB_WEZTERM_USER`, `WSB_WEZTERM_HOST` and `WSB_WEZTERM_IN_TMUX` |
| Configuration values that name the program | `remote_wsb_wezterm_path`; an SSH domain's `multiplexing = "WsbWezterm"`; the ssh_config options `wsb_wezterm_ssh_backend` and `wsb_wezterm_ssh_verbose` |

## Changes from WezTerm

### Updates

- **wsb-wezterm checks for no updates.** WezTerm asks GitHub once a day for its newest release and offers to download it; a wsb-wezterm build is installed by whoever built it, so it asks nothing, and the `check_for_updates`, `check_for_updates_interval_seconds` and `show_update_window` options are gone. The Help menu's documentation entry opens this fork's documentation.

### Tabs

- **Drag tabs to reorder them.** Drag a tab along the tab bar with the left mouse button. From [wezterm#6527](https://github.com/wezterm/wezterm/pull/6527) by @Jefffrey, and [wezterm#7926](https://github.com/wezterm/wezterm/pull/7926) by @jayjohnston, which redraws the tab bar during the drag and adds a floating preview to the fancy tab bar.
- **Tab widths that fit the bar.** When the retro tab bar's titles do not all fit, a title shorter than an even share keeps its full width, and the room it leaves goes to the longer titles, instead of every tab being cut to an even share (`wezterm-gui/src/tabbar.rs`, `tab_widths`).
- **Right-clicking a tab no longer opens the tab navigator.** It opens the context menu instead (below). `ShowTabNavigator` is still available as a key assignment.

### Tab and window variables

- **Named values on tabs and windows that only programs set.** `wsb-wezterm cli set-tab-var NAME VALUE` and `wsb-wezterm cli set-window-var NAME VALUE` (or `--unset NAME`), and `tab:set_var()` / `window:set_var()` in Lua, store values a program recognizes a tab or window by later. `wsb-wezterm cli list --format json` reports them as `tab_vars` and `window_vars`, and Lua reads them with `get_vars()`. Unlike titles, which any program in a pane can change with an escape sequence, nothing running in the panes can touch them. A multiplexer server's values are mirrored by every GUI attached to it, and a value a GUI sets on a server's tab or window is set on the server. This changes the multiplexer protocol.

### Window placement and zoom in Lua

- **What a configuration needs to give a new window the placement and zoom an earlier one had**, which wsb-wezterm keeps only while a window is open:
  - `window:get_position()` reports the top left of the window's content area, in the coordinates `window:set_position()` takes, so moving a window to its reported position leaves it where it is. On Wayland, which tells applications nothing of where their windows are, it returns `nil` ([docs](docs/config/lua/window/get_position.md)).
  - `window:get_dimensions()` also reports `is_maximized` ([docs](docs/config/lua/window/get_dimensions.md)).
  - `window:get_font_scale()` and `window:set_font_scale(scale)` read and set the zoom that IncreaseFontSize, DecreaseFontSize and ResetFontSize change ([docs](docs/config/lua/window/get_font_scale.md), [docs](docs/config/lua/window/set_font_scale.md)).
  - `window:get_content_size()` and `window:set_content_size(width, height)` read and set the size of the window's content area in the units the windowing system keeps it in while the window moves between screens: points on macOS, logical pixels on Wayland, and pixels on X11 and Windows. Unlike `get_dimensions()`'s pixels, which double on a Retina display, a size read on one screen gives a window the same size on every other. `set_content_size` converts it for the screen the window is on when it applies, after a `set_position` asked for before it, and keeps the content area's top left where it is ([docs](docs/config/lua/window/get_content_size.md), [docs](docs/config/lua/window/set_content_size.md)).
  - A window applies the changes Lua asks of it, `set_inner_size`, `set_font_scale`, `set_position`, `maximize`, `restore` and `toggle_fullscreen`, in the order asked. Before, a move or a maximize went straight to the window and could overtake a resize asked for first.
- **On X11 a window declares static gravity** in its size hints, the meaning `set_position` gives a position: where the content area goes, not the frame the window manager draws around it. The window manager reads a window's position by the window's gravity whenever it configures it; declaring none, a window placed with `set_position` was placed by its frame, a title bar lower, the next time it was resized or restored from maximized (Openbox does this).

### Window size

- **A window is no larger than its screen.** `window:set_inner_size()`, `window:set_content_size()`, and wsb-wezterm's own resizes (such as zooming with `adjust_window_size_when_changing_font_size`) give a window at most the visible area of its screen, as the windowing system limits a resize by the user: the screen less the menu bar and the Dock on macOS, less the taskbar on Windows, the bounds the compositor suggests on Wayland, and the whole screen on X11, which tells windows nothing of panels. Before, a size larger than the screen was applied as asked ([docs](docs/config/lua/window/set_inner_size.md)).
- **A window larger than the GPU can draw is not painted, and wsb-wezterm logs an error, rather than aborting.** The OpenGL renderer panicked on a viewport larger than `GL_MAX_VIEWPORT_DIMS` ("Viewport dimensions are too large"), and WebGPU on a surface larger than its largest texture; on macOS the panic happened where it cannot unwind, which aborts the process. Such a window now stays unpainted until it is resized to a size the GPU can draw.

### Context menu

- **A right-click menu, drawn by wsb-wezterm, the same on every platform.**
  - In a terminal: Copy (greyed out without a selection), Paste, Split Right, Split Down, Close Pane, New Tab, and Command Palette, with their shortcuts.
  - On the tab bar: New Tab, Close Tab (the tab clicked), Minimize, Maximize/Restore, and Close Window.
  - It uses the command palette's font and colours, and is navigated with the mouse or the arrow keys, Enter, and Escape.
  - A program that tracks the mouse still gets the right-click; Shift+right-click always opens the menu.
  - It is the new `ShowContextMenu` key assignment, bound to a right-click by default ([docs](docs/config/lua/keyassignment/ShowContextMenu.md)).

### Wayland

- **A window frame on desktops that leave decorations to the application, such as GNOME.** WezTerm draws its own title bar and borders when the compositor refuses server-side decorations, rather than showing no frame at all. Dragging an empty part of the tab bar moves the window. From [wezterm#8145](https://github.com/wezterm/wezterm/pull/8145) by @AlexKichkailo.
- **That frame is counted in the window geometry.** The size the compositor suggests includes the frame, and the geometry wsb-wezterm declares now does too. Before, resizing from a border left the terminal smaller than the window and stopped following the pointer.
- **Resize cursors on the frame's borders and corners while hovering them**, not only once a drag has begun.
- **A click on the title bar stays on the title bar.** Clicking the frame of a window that did not have the keyboard selected the tab under the click as well: the keyboard entering the window replaced the surface pointer events were sent to, so the press went to the terminal at the title bar's coordinates, which are the tab bar's. Each pointer event now goes to the surface it arrived on.

### macOS

- **The title bar shows the terminal's background, not the desktop.** An opaque window's background is now the color scheme's background color rather than clear. Where macOS draws the title bar translucent, it showed whatever was behind the window, leaving only the title and the window buttons visible.

### Arranging panes

- **A tab's panes can be arranged freely through `wsb-wezterm cli` and the multiplexer protocol**, so a program, a web page or a Claude Code session can lay out the tab it works in ([docs](docs/cli/cli/index.markdown#arranging-a-tabs-panes)):
  - `get-layout` prints a tab's tree of splits as JSON: each pane's place and size, its title, working directory, foreground program and terminal device, and which pane is focused and zoomed ([docs](docs/cli/cli/get-layout.md)).
  - `set-layout` gives a tab a whole layout in one step, from a JSON tree of splits whose sizes are cells or percentages and whose panes are the tab's, panes moved in from other tabs, or new ones ([docs](docs/cli/cli/set-layout.md)).
  - `resize-split` sets a split's size in cells, as dragging its divider does ([docs](docs/cli/cli/resize-split.md)); `swap-panes` swaps two panes, in one tab or two ([docs](docs/cli/cli/swap-panes.md)); `send-keys` presses keys in a pane, such as `CTRL+c` ([docs](docs/cli/cli/send-keys.md)).
  - `relay --tab-of-pane ID` relays every pane of a tab and the tab's layout over one connection, following the pane from tab to tab ([docs](docs/cli/cli/relay.md#relaying-a-tab)).
  - `cli list --format json` reports each pane's `foreground_process_name`.
- **Every client hears of every change to a tab's layout.** The multiplexer sends `TabLayoutChanged` (WezTerm's `TabResized`) whenever a tab's panes, their places or sizes, the focused pane or the zoomed pane change. WezTerm sent it only for resizes, so a split, a closed pane, a swap or a focus change made by one client reached another only when something else made it re-sync. This changes the multiplexer protocol.
- **`adjust-pane-size --pane-id ID` moves the divider of that pane.** It moved the divider of the tab's focused pane, whichever pane was named.
- **A tab is resized as a whole, by the multiplexer.** A client resizing a tab sends the tab's new size in one request (`ResizeTab`), and the multiplexer shares it out among the panes by the tab's layout and tells every client the new layout. A GUI window's resize, a divider dragged in a GUI and `relay --tab-of-pane` all ask the multiplexer this way; a client's mirror of a tab never asks a size for each of its panes. WezTerm's GUI resized a tab pane by pane, one request each, and the multiplexer rebuilt each split's size from its panes' sizes: between those requests, or for good when the client stopped part way, the tab's tree did not add up. A pane is resized on its own only when it is alone in its tab (`relay --pane-id ID --sized-by-input`); in a tab with others it is refused, naming the tab, its panes and that the tab's layout sizes them. `resize-tab` sizes a tab from the command line ([docs](docs/cli/cli/resize-tab.md)). This changes the multiplexer protocol.
- **A desktop window sizes the tabs of the workspace it shows, and only another window competes with it.** A GUI tells the multiplexer the workspace it shows, at each connection and whenever it switches, and `list-clients` lists it. A resize asked by any other client, a relay or a command, of a tab in a workspace a GUI shows is refused, leaving the tab as the window sized it, and the refusal names the rule and each window by its user, machine and process. `get-layout` says the same for each tab, in `sized_by_window`. A GUI attached but showing another workspace does not stop it.
- **Moving a divider resizes the panes nested beyond it.** The panes nearest the divider give or take the cells first, and none goes below one cell. WezTerm kept the first pane of a nested split at its size, so dragging a divider into a nested split shrank the pane beyond it to nothing and pushed it past the edge of the tab.
- **A split with no room is refused before any pane is made or moved, naming the sizes.** `split-pane` checks that both parts and the divider fit before it starts a program or takes a `--move-pane-id` pane out of its tab; a pane moved within its own tab is measured for on the tab without it. WezTerm took the moved pane out of its tab, or started the new program, and only then found "No space for split!": the pane's program ran on in no tab, out of reach of the multiplexer and shown nowhere ([docs](docs/cli/cli/split-pane.md#no-room-for-the-split)).

### Relaying a pane

- **`wsb-wezterm cli relay --pane-id ID` lets another program show a pane and type into it**: a terminal emulator in a web page, for instance. The relay attaches to the multiplexer as a GUI does and writes the pane's screen as escape sequences, a full paint and then only what changed, one JSON event per line on stdout. It reads keys, text, paste and mouse events on stdin, and the multiplexer encodes them with the modes of the terminal in the pane, so the program in it receives what a keyboard would send. It never resizes the pane unless started with `--sized-by-input`, and never takes focus. Scrollback, title, clipboard (OSC 52), bell and the pane's colors come through as events too ([docs](docs/cli/cli/relay.md)).
- **Clients say what they are for.** A client attaching to a multiplexer registers as a `gui`, a `relay`, or a short-lived `command`, and `wsb-wezterm cli list-clients` shows it, so a program can tell whether a GUI is showing a multiplexer's windows. A relay, which shows no windows, also leaves the panes' colors to the GUIs, where a GUI tells the server the colors it uses for every pane.
- **Each cell's grapheme stays in its cell.** A program can write graphemes in cells of their own that, read one after another, make one: an emoji and a skin tone modifier, two regional indicators, a letter and a combining mark. A surface's changes (`wezterm-surface`'s `diff_lines` and the rest, which the relay writes its escapes from) place such a cell with an explicit cursor position, so the terminal reading them draws each where the pane holds it, rather than joining the two in the cells of one and drawing the rest of the line early.
- **A key is turned into terminal input in one place** (`termwiz::input::KeyInput`), which the GUI and the relay share.
- **A multiplexer with no domain of its own makes no ssh-agent link.** `wsb-wezterm cli proxy` and the relay run no programs that would use one, and each run used to leave an `agent.PID` link behind in the runtime directory.

### Multiplexer clients

These affect a window attached to a mux server, such as with `wsb-wezterm connect`:

- **A re-sync takes the server's tab sizes without sending them back.** A client re-syncs whenever the server reports a change to a tab's layout, and used to send the size it had just been told back to the server as a new request. While the window was resized, tabs bounced between sizes, with the programs in them redrawing at each, and could be left at a stale size.
- **A pane's resizes are sent one at a time, the latest size last.** Before, each one was sent on its own, unordered, and an earlier size could be the last one applied.
- **Closing a window closes its tabs.** Closing a window whose tabs a server holds kills those tabs in the server, as closing each tab does. Before, it detached the window from the server's whole domain instead: every other window of that server disappeared with it, and the programs in all of them, the closed window's included, ran on. Quitting wsb-wezterm still leaves a server's programs running. A window wsb-wezterm fails to create still detaches its domain, so a failure ends no programs.

Several of these change the multiplexer protocol (codec version 50): GUIs, servers and `wsb-wezterm cli` must be built from this fork alike.

## Building

[docs/install/source.md](docs/install/source.md) says how to build wsb-wezterm
from source, and `ci/deploy.sh` packages a build for the platform it runs on.

A build names itself by the commit git describes, or by `.tag` in a source
without git; a source with neither is refused, rather than built without a
name ([docs](docs/install/source.md)).

## Supporting WezTerm

If you like WezTerm, please consider sponsoring Wez Furlong's work on it:

* [GitHub Sponsors](https://github.com/sponsors/wez)
* [Patreon](https://patreon.com/WezFurlong)
* [Ko-Fi](https://ko-fi.com/wezfurlong)
* [Liberapay](https://liberapay.com/wez)
