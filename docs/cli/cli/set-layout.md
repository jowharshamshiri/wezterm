# `wsb-wezterm cli set-layout`

{{since('nightly')}}

*Run `wsb-wezterm cli set-layout --help` to see more help*

Gives a tab a whole layout in one step, read as JSON from stdin, and prints
the layout the tab then has, as [get-layout](get-layout.md) does.

The tab is the one given by `--tab-id`, else the tab that holds `--pane-id`,
else the tab of the current pane (see [Targeting Panes](index.md#targeting-panes)).

```console
$ wsb-wezterm cli set-layout --pane-id 0 < layout.json
```

where `layout.json` holds:

```json
{"root": {"split": {"direction": "horizontal", "first_cells": 30,
  "first": {"pane": {"id": 0}},
  "second": {"split": {"direction": "vertical", "first_percent": 30,
    "first": {"pane": {"spawn": {"args": ["htop"]}}},
    "second": {"pane": {"id": 7, "active": true}}}}}}}
```

The layout is `{"root": NODE}`, and a `NODE` is a split or a pane:

* `{"split": {"direction": D, "first": NODE, "second": NODE}}` divides its place
  in two. `direction` is `horizontal` (side by side, `first` on the left) or
  `vertical` (stacked, `first` on top). The size of `first` along the split is
  `"first_cells": N` cells, or `"first_percent": P` percent of the cells both
  parts share (all but the divider's); without either, half of them. The
  second part gets the rest.
* `{"pane": {"id": N}}` is a pane already in the multiplexer: one of this tab,
  or of another tab, which moves here. A tab left without panes closes.
* `{"pane": {"spawn": {"args": [...], "cwd": DIR}}}` is a new pane, spawned in
  the domain of the tab's focused pane. Without `args` it runs the default
  program; without `cwd` it starts in the focused pane's working directory.
  A relative `cwd` is taken from the command's own.
* A pane may also have `"active": true`, to get the focus, or
  `"zoomed": true`, to be zoomed and focused. One pane at most has either.
  Without them, the focused pane keeps the focus while it is in the tab.

The multiplexer applies the layout in one step, and every client attached to
the tab shows it.

The command refuses a layout, changing nothing and naming the reason, when:

* it leaves out a pane of the tab: close the pane first, or place it;
* it names a pane twice, or a pane that does not exist;
* a split's sizes do not fit: every pane needs at least one cell each way,
  and every divider one cell. The error names the split by its place, such as
  `root.second.first`;
* it has a field this page does not list. The layout `get-layout` prints is
  not taken back as it is, because its places, sizes and titles cannot be set.

It also refuses the panes of a domain this multiplexer only mirrors (a GUI
attached to a multiplexer server, or tmux): run the command against that
domain's multiplexer instead.

## Synopsis

```console
{% include "../../examples/cmd-synopsis-wsb-wezterm-cli-set-layout--help.txt" %}
```
