# This fork

This is [jowharshamshiri](https://github.com/jowharshamshiri)'s fork of [WezTerm](https://github.com/wezterm/wezterm). Its `main` branch is WezTerm's `main` plus the changes below, and it is not sent upstream. [toolset](https://github.com/jowharshamshiri/toolset) builds it from source at a pinned commit and installs it on Linux, macOS, and Windows.

## Changes from WezTerm

### Tabs

- **Drag tabs to reorder them.** Drag a tab along the tab bar with the left mouse button. From [wezterm#6527](https://github.com/wezterm/wezterm/pull/6527) by @Jefffrey, and [wezterm#7926](https://github.com/wezterm/wezterm/pull/7926) by @jayjohnston, which redraws the tab bar during the drag and adds a floating preview to the fancy tab bar.
- **Tab widths that fit the bar.** When the retro tab bar's titles do not all fit, a title shorter than an even share keeps its full width, and the room it leaves goes to the longer titles, instead of every tab being cut to an even share (`wezterm-gui/src/tabbar.rs`, `tab_widths`).
- **Right-clicking a tab no longer opens the tab navigator.** It opens the context menu instead (below). `ShowTabNavigator` is still available as a key assignment.

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

### Multiplexer clients

These affect a window attached to a mux server, such as with `wezterm connect`:

- **A re-sync takes the server's tab sizes without sending them back.** A client re-syncs whenever the server reports a tab resized, and used to send the size it had just been told back to the server as a new request. While the window was resized, tabs bounced between sizes, with the programs in them redrawing at each, and could be left at a stale size.
- **A pane's resizes are sent one at a time, the latest size last.** Before, each one was sent on its own, unordered, and an earlier size could be the last one applied.

## Keeping up with WezTerm

`main` takes WezTerm's `main` by merging, never by rebasing, so every commit toolset has pinned stays on the branch.

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
