# `wsb-wezterm cli resize-split`

{{since('nightly')}}

*Run `wsb-wezterm cli resize-split --help` to see more help*

Sets the size of a split, given by the pane before its divider: the split
whose divider is to the right of the pane (`--direction right`) or below it
(`--direction down`). That is the nearest split around the pane, dividing
that way, whose first part holds the pane. The split's first part, left of or
above the divider, gets `--first-cells` cells, and its second part the rest.

The panes beside the divider take the change first: a part that shrinks
gives up cells from the pane nearest the divider, down to one cell, and then
from the next; a part that grows gives the cells to the pane nearest the
divider.

This is what dragging a divider does. In this tab, where panes 0 and 2 share
the left half:

```
+-------+-------+---------------+
|   0   |   2   |       1       |
+-------+-------+---------------+
```

```console
$ wsb-wezterm cli resize-split --pane-id 2 --direction right --first-cells 50
$ wsb-wezterm cli resize-split --pane-id 0 --direction right --first-cells 10
```

The first command moves the divider before pane 1, making the left half 50
columns; the second moves the divider between panes 0 and 2, making pane 0
10 columns.

The command fails, changing nothing, when the pane has no divider on that
side, when the size would leave a pane less than a cell (the error names the
sizes the split can take), and while a pane of the tab is zoomed.

To move a divider by a number of cells, see
[adjust-pane-size](adjust-pane-size.md).

## Synopsis

```console
{% include "../../examples/cmd-synopsis-wsb-wezterm-cli-resize-split--help.txt" %}
```
