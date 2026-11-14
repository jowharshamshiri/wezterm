# `wsb-wezterm cli resize-tab`

{{since('nightly')}}

*Run `wsb-wezterm cli resize-tab --help` to see more help*

Sizes a tab as a whole: `--cols` columns and `--rows` rows. The multiplexer
shares the size out among the tab's panes by its layout, as it does when a
window is resized, and every client sees the new layout.

The tab is the one given by `--tab-id`, else the tab that holds `--pane-id`,
else the tab of the current pane (see [Targeting Panes](index.md#targeting-panes)).

```console
$ wsb-wezterm cli resize-tab --pane-id 0 --cols 160 --rows 48
{"sized_by_window":null}
```

Once the tab has the size, the command prints `{"sized_by_window":null}` and
exits with status 0.

A desktop window sizes every tab of the workspace it shows. Where one shows
the tab's workspace, the multiplexer leaves the tab as the window sized it,
and the command prints the rule and each window, by its user, machine and
process, and exits with status 3:

```console
$ wsb-wezterm cli resize-tab --pane-id 0 --cols 160 --rows 48
{"sized_by_window":"a desktop window sizes each tab of the workspace it shows, and tab 0 is in workspace \"default\", which the wsb-wezterm window of me@box (process 4242) shows"}
```

[get-layout](get-layout.md) says the same for a tab in `sized_by_window`.

The command fails, changing nothing, when there is no such tab, and when the
tab's layout does not fit the size: it needs a cell for each pane and one for
each divider, and the error names the tab, its panes and the least size.

## Synopsis

```console
{% include "../../examples/cmd-synopsis-wsb-wezterm-cli-resize-tab--help.txt" %}
```
