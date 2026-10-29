# `wezterm cli relay --pane-id ID`

{{since('nightly')}}

*Run `wezterm cli relay --help` to see more help*

Relays one pane to another program over stdin and stdout, so that the
program can show the pane and type into it: a terminal emulator in a web
page, for instance.

The relay attaches to the multiplexer as a client, as a GUI does, and keeps
a mirror of the pane. It writes the pane's screen as escape sequences, a full
paint first and then only what changed, for a terminal emulator to display.
What it reads back are input events rather than bytes. The multiplexer
encodes keys, paste and mouse events with the modes of the terminal in the
pane (application cursor keys, bracketed paste, the kitty keyboard protocol,
mouse reporting), as it does for a GUI, so the program in the pane receives
exactly what a keyboard would send.

The relay does not change the pane's size unless it is started with
`--sized-by-input`, and never claims focus: the pane keeps the size and focus
its other clients give it. [wezterm cli list-clients](list-clients.md) shows
the relay with the role `relay`.

It ends when its input ends, or with an `exit` event when the pane does.

## Events

Both directions carry one JSON object per line, told apart by its `t` field.

The relay writes:

| `t` | Fields | Meaning |
|---|---|---|
| `hello` | `cols`, `rows`, `palette`, `title`, `mouse_grab` | First, once. `palette` holds `foreground`, `background`, `cursor_fg`, `cursor_bg`, `cursor_border`, `selection_fg`, `selection_bg` and the 16 `ansi` colors, as `#rrggbb`. |
| `screen` | `data` | Escape sequences that bring a terminal of the pane's size from what was last sent to what the pane shows. |
| `size` | `cols`, `rows` | The pane was resized; a full paint follows. |
| `title` | `text` | The pane's title changed. |
| `mouse_grab` | `on` | Whether the program in the pane takes the mouse. While it does, mouse events belong to it; otherwise they are for selecting and scrolling. |
| `clipboard` | `text` | The program set the clipboard (OSC 52); `null` clears it. |
| `palette` | `palette` | The pane's colors changed. |
| `bell` | | The program rang the bell. |
| `exit` | | The pane is gone. |
| `error` | `message` | An input line was refused, or the relay failed. |

The relay reads:

| `t` | Fields | Meaning |
|---|---|---|
| `key` | `key`, `mods` | A key press. `key` is named as in a [key binding](../../config/keys.md): `"a"`, `"Enter"`, `"Escape"`, `"Backspace"`, `"Tab"`, `"UpArrow"`, `"PageDown"`, `"F5"`. `mods` combines `SHIFT`, `ALT`, `CTRL` and `SUPER` with `\|`, and may be left out. |
| `text` | `text` | Typed text; each character is a key press. |
| `paste` | `text` | A paste, bracketed when the program asked for that. |
| `mouse` | `kind`, `x`, `y`, `button`, `mods` | `kind` is `press`, `release` or `move`; `x` and `y` are a cell of the visible screen counted from 0; `button` is `left`, `middle`, `right`, `wheel_up`, `wheel_down` or `none`. |
| `scroll` | `rows` | Move the view that many rows into the scrollback (positive) or back toward the bottom (negative). Input returns the view to the bottom. |
| `bottom` | | Follow the bottom of the pane again. |
| `resize` | `cols`, `rows` | Resize the pane. Refused unless the relay was started with `--sized-by-input`. |
| `repaint` | | Paint the whole screen again. |

A line that is not one of these is answered with an `error` event, and the
relay goes on.

```console
$ wezterm cli relay --pane-id 0
{"t":"hello","cols":80,"rows":24,"palette":{...},"title":"bash","mouse_grab":false}
{"t":"screen","data":"\u001b[H\u001b[2J..."}
{"t":"text","text":"ls"}
{"t":"key","key":"Enter"}
{"t":"screen","data":"..."}
```

## Synopsis

```console
{% include "../../examples/cmd-synopsis-wezterm-cli-relay--help.txt" %}
```
