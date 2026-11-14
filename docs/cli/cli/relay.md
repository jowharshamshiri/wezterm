# `wsb-wezterm cli relay`

{{since('nightly')}}

*Run `wsb-wezterm cli relay --help` to see more help*

Relays one pane (`--pane-id ID`), or a whole tab
(`--tab-of-pane ID`, see [Relaying a tab](#relaying-a-tab)), to another
program over stdin and stdout, so that the program can show the panes and
type into them: a terminal emulator in a web page, for instance.

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
its other clients give it. [wsb-wezterm cli list-clients](list-clients.md) shows
the relay with the role `relay`.

It ends when its input ends, or with an `exit` event when the pane does.

Input reaches the pane in the order the relay read it: the relay sends each
key, paste, mouse event and resize once the multiplexer has taken the one
before. When its input ends, the relay first sends everything it has read, so
a program may write its last events and close the relay's stdin at once. An
event the multiplexer did not take is reported with an `error` event naming
it, and a relay that ends with such events left exits with a failure status.

## Events

Both directions carry one JSON object per line, told apart by its `t` field.

The relay writes:

| `t` | Fields | Meaning |
|---|---|---|
| `hello` | `cols`, `rows`, `palette`, `title`, `mouse_grab`, `sized_by_input` | First, once. `palette` holds `foreground`, `background`, `cursor_fg`, `cursor_bg`, `cursor_border`, `selection_fg`, `selection_bg` and the 16 `ansi` colors, as `#rrggbb`. `sized_by_input` says whether `resize` events size the pane. |
| `screen` | `data` | Escape sequences that bring a terminal of the pane's size from what was last sent to what the pane shows. |
| `size` | `cols`, `rows` | The pane was resized; a full paint follows. |
| `title` | `text` | The pane's title changed. |
| `mouse_grab` | `on` | Whether the program in the pane takes the mouse. While it does, mouse events belong to it; otherwise they are for selecting and scrolling. |
| `clipboard` | `text` | The program set the clipboard (OSC 52); `null` clears it. |
| `palette` | `palette` | The pane's colors changed. |
| `bell` | | The program rang the bell. |
| `exit` | | The pane is gone. |
| `error` | `message`, `sized_by_window` | An input line was refused, an event did not reach the pane, or the relay failed. `sized_by_window` is there only for a resize of the tab the multiplexer refused because a desktop window sizes the tab: the rule and each window, as [get-layout](get-layout.md) says them. |

The relay reads:

| `t` | Fields | Meaning |
|---|---|---|
| `key` | `key`, `mods` | A key press. `key` is named as in a [key binding](../../config/keys.md): `"a"`, `"Enter"`, `"Escape"`, `"Backspace"`, `"Tab"`, `"UpArrow"`, `"PageDown"`, `"F5"`. `mods` combines `SHIFT`, `ALT`, `CTRL` and `SUPER` with `\|`, and may be left out. |
| `text` | `text` | Typed text; each character is a key press. |
| `paste` | `text` | A paste, bracketed when the program asked for that. |
| `mouse` | `kind`, `x`, `y`, `button`, `mods` | `kind` is `press`, `release` or `move`; `x` and `y` are a cell of the visible screen counted from 0; `button` is `left`, `middle`, `right`, `wheel_up`, `wheel_down` or `none`. |
| `scroll` | `rows` | Move the view that many rows into the scrollback (positive) or back toward the bottom (negative). Input returns the view to the bottom. |
| `bottom` | | Follow the bottom of the pane again. |
| `resize` | `cols`, `rows` | Resize the pane, which must be alone in its tab: a pane of a tab with others is sized by the tab's layout, and the multiplexer refuses it, naming the tab and its panes. Refused unless the relay was started with `--sized-by-input`. |
| `repaint` | | Paint the whole screen again. |

A line that is not one of these is answered with an `error` event, and the
relay goes on.

```console
$ wsb-wezterm cli relay --pane-id 0
{"t":"hello","cols":80,"rows":24,"palette":{...},"title":"bash","mouse_grab":false,"sized_by_input":false}
{"t":"screen","data":"\u001b[H\u001b[2J..."}
{"t":"text","text":"ls"}
{"t":"key","key":"Enter"}
{"t":"screen","data":"..."}
```

## Relaying a tab

With `--tab-of-pane ID` the relay relays every pane of the tab that holds the
pane `ID`, and the tab's layout, over one connection. It follows the pane:
when the pane moves to another tab, the relay goes with it. Once the pane is
gone, the relay goes on with the tab it was last in, and ends with an `exit`
event when that tab is gone.

The events above are sent for each pane of the tab, each with a `pane` field
naming it. `hello` comes when a pane enters the tab (or the relay starts),
before its first `screen`; its `sized_by_input` says whether `resize` events
size the tab. The relay also writes:

| `t` | Fields | Meaning |
|---|---|---|
| `layout` | `tab` | The tab's layout, as [get-layout](get-layout.md) prints it: first, and whenever it changes (a split, a resize, a move, a swap, a change of focus or zoom, a pane closing, the tab's size). |
| `gone` | `pane` | The pane left the tab: it closed, or moved to another tab. |

When the followed pane moves to another tab, the relay sends `gone` for each
pane it leaves behind, then the new tab's `layout`, then `hello` for each of
its other panes.

Every event the relay reads names its pane with a `pane` field, except
`resize`: a `resize` without `pane` sizes the whole tab, and only with
`--sized-by-input`. It is one request to the multiplexer, which shares the
size out among the tab's panes by its layout and tells every client the new
layout. A pane's own size comes from the tab's layout, so a `resize` naming a
pane is refused. A tab's resize counts as sent once the multiplexer has
answered it, so a relay whose input ends after a `resize` stops only once the
tab has that size. The multiplexer refuses it, leaving the tab as it is, when
a desktop window shows the tab's workspace and so sizes the tab (the `error`
event's `sized_by_window` says the rule and each window), and when the tab's
layout does not fit the size (one cell per pane and one per divider).

### Following a tab's layout

With `--layout-only` as well, the relay sends only the tab's `layout` events,
and `exit` when the tab is gone: no pane's `hello`, `screen` or other events.
Its input takes only a `resize` of the tab (with `--sized-by-input`); an
event naming a pane is refused. A program that keeps a record of a tab's
layout runs it for as long as it keeps the record, and closes its stdin to
end it.

```console
$ wsb-wezterm cli relay --tab-of-pane 0 --layout-only
{"t":"layout","tab":{"tab_id":0,"window_id":0,"cols":80,"rows":24,"active":0,"zoomed":null,"root":{"pane":{...}}}}
{"t":"layout","tab":{"tab_id":0,"window_id":0,"cols":80,"rows":24,"active":1,"zoomed":null,"root":{"split":{...}}}}
```

```console
$ wsb-wezterm cli relay --tab-of-pane 0
{"t":"layout","tab":{"tab_id":0,"window_id":0,"cols":80,"rows":24,"active":1,"zoomed":null,"root":{"split":{...}}}}
{"t":"hello","cols":39,"rows":24,"palette":{...},"title":"bash","mouse_grab":false,"sized_by_input":false,"pane":0}
{"t":"screen","data":"...","pane":0}
{"t":"hello","cols":40,"rows":24,"palette":{...},"title":"bash","mouse_grab":false,"sized_by_input":false,"pane":1}
{"t":"screen","data":"...","pane":1}
{"t":"text","text":"ls","pane":1}
{"t":"key","key":"Enter","pane":1}
```

## Synopsis

```console
{% include "../../examples/cmd-synopsis-wsb-wezterm-cli-relay--help.txt" %}
```
