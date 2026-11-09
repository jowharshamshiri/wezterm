//! The names this program is known by: its programs, its application id,
//! its configuration file and Lua module, and the folders it keeps state in.
//!
//! wsb-wezterm is a fork of WezTerm, by Wez Furlong, and goes by its own
//! names so that it never takes the place of a WezTerm installed beside it.

/// The program's name: the command line program, the folder under each of the
/// configuration, data, cache and runtime directories, the Lua module a
/// configuration requires, `TERM_PROGRAM`, and the title of a window or pane
/// that has none of its own.
pub const NAME: &str = "wsb-wezterm";

/// The application id: the X11 window class, the Wayland app id, the desktop
/// entry, the macOS bundle identifier, the Windows AppUserModelID, and the
/// id notifications are sent under.
pub const APP_ID: &str = "com.bahramj.wsb-wezterm";

/// The file name of the configuration file.
pub const CONFIG_FILE_NAME: &str = "wsb-wezterm.lua";

/// The GUI program, beside the command line program.
pub const GUI_PROGRAM: &str = if cfg!(windows) {
    "wsb-wezterm-gui.exe"
} else {
    "wsb-wezterm-gui"
};

/// The multiplexer server program, beside the command line program.
pub const MUX_SERVER_PROGRAM: &str = if cfg!(windows) {
    "wsb-wezterm-mux-server.exe"
} else {
    "wsb-wezterm-mux-server"
};
