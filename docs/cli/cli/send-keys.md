# `wsb-wezterm cli send-keys`

{{since('nightly')}}

*Run `wsb-wezterm cli send-keys --help` to see more help*

Presses keys in a pane, in order, as if they were typed on a keyboard. The
terminal in the pane encodes each key in the modes its program asked for,
such as application cursor keys or the kitty keyboard protocol.

Each key is named as in a [key binding](../../config/keys.md): `a`, `Enter`,
`Escape`, `Tab`, `UpArrow`, `PageDown`, `F5`. Modifiers come first, joined
with `|`, then a `+` and the key: `CTRL+c`, `CTRL|SHIFT+t`, `ALT+Enter`.
`CTRL++` is Ctrl and the `+` key. The modifiers are `SHIFT`, `ALT`, `CTRL` and
`SUPER`.

```console
$ wsb-wezterm cli send-keys --pane-id 3 CTRL+c
$ wsb-wezterm cli send-keys --pane-id 3 l s Enter
```

Every key is checked before any is sent: a name that is not a key, or a key
that sends nothing to a terminal, fails the command and sends none.

To send text as a paste, or as raw bytes, see [send-text](send-text.md).

## Synopsis

```console
{% include "../../examples/cmd-synopsis-wsb-wezterm-cli-send-keys--help.txt" %}
```
