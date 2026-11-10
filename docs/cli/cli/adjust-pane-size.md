# `wsb-wezterm cli adjust-pane-size DIRECTION`

{{since('20230712-072601-f4abf8fd')}}

*Run `wsb-wezterm cli adjust-pane-size --help` to see more help*

Moves a divider of the current pane, or of the pane specified by `--pane-id`,
by `--amount` cells (1 by default) in the specified direction: the divider of
the nearest split around the pane that divides that way. `Left` and `Right`
move the divider of the nearest side-by-side split, `Up` and `Down` that of
the nearest stacked split. The divider stops where the panes on either side
are one cell. A pane with no such split is left as it is, and so is a tab with
a zoomed pane.

The panes beside the divider take the change first, as
[resize-split](resize-split.md) describes. To set a split's size in cells,
use that command.

Possible values for `DIRECTION` are shown below; the direction is matched
ignoring case so you can use `left` rather than `Left`:

* `Left`, `Right`, `Up`, `Down`

## Synopsis

```console
{% include "../../examples/cmd-synopsis-wsb-wezterm-cli-adjust-pane-size--help.txt" %}
```
