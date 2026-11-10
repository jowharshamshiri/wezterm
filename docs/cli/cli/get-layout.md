# `wsb-wezterm cli get-layout`

{{since('nightly')}}

*Run `wsb-wezterm cli get-layout --help` to see more help*

Prints a tab's layout as JSON: its tree of splits, and for each pane where it
is, how big it is, and what runs in it, with the focused and the zoomed pane.

The tab is the one given by `--tab-id`, else the tab that holds `--pane-id`,
else the tab of the current pane (see [Targeting Panes](index.md#targeting-panes)).

```console
$ wsb-wezterm cli get-layout --pane-id 0
{
  "tab_id": 0,
  "window_id": 0,
  "cols": 80,
  "rows": 24,
  "active": 2,
  "zoomed": null,
  "root": {
    "split": {
      "direction": "horizontal",
      "left": 0, "top": 0, "cols": 80, "rows": 24,
      "first": {
        "pane": {
          "id": 0, "left": 0, "top": 0, "cols": 49, "rows": 24,
          "title": "bash", "cwd": "file://host/home/me",
          "process": "/usr/bin/bash", "tty": "/dev/pts/3"
        }
      },
      "second": {
        "split": {
          "direction": "vertical",
          "left": 50, "top": 0, "cols": 30, "rows": 24,
          "first": { "pane": { "id": 1, "left": 50, "top": 0, "cols": 30, "rows": 13, ... } },
          "second": { "pane": { "id": 2, "left": 50, "top": 14, "cols": 30, "rows": 10, ... } }
        }
      }
    }
  }
}
```

## The layout

| Field | Meaning |
|---|---|
| `tab_id`, `window_id` | The tab, and the window it is in. |
| `cols`, `rows` | The size of the tab's tree, in cells. |
| `active` | The focused pane. |
| `zoomed` | The zoomed pane, which then fills the tab, or `null`. |
| `root` | The tree: a `pane` or a `split`. |

A `split` divides its place in two:

| Field | Meaning |
|---|---|
| `direction` | `horizontal` puts its parts side by side, `first` on the left; `vertical` stacks them, `first` on top. |
| `left`, `top`, `cols`, `rows` | Its place, in cells from the tab's top left. |
| `first`, `second` | Its parts, each a `pane` or a `split`. A divider one cell wide sits between them: right after `first` in a horizontal split, below it in a vertical one. |

A `pane`:

| Field | Meaning |
|---|---|
| `id` | The pane's id, as every `wsb-wezterm cli` command takes it. |
| `left`, `top`, `cols`, `rows` | Its place and size, in cells from the tab's top left. |
| `title` | Its title. |
| `cwd` | Its working directory as a URL, or `null` where the pane cannot tell. |
| `process` | The program in its foreground, as the path of its executable, or `null` where the pane cannot tell. |
| `tty` | The name of its terminal device, or `null` where it has none. |

While a pane is zoomed, the tree keeps the places the panes have when it is
not, and the zoomed pane's terminal is the size of the tab.

[wsb-wezterm cli relay --tab-of-pane](relay.md#relaying-a-tab) sends this
layout every time it changes.

## Synopsis

```console
{% include "../../examples/cmd-synopsis-wsb-wezterm-cli-get-layout--help.txt" %}
```
