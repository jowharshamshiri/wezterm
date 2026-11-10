# `wsb-wezterm cli swap-panes`

{{since('nightly')}}

*Run `wsb-wezterm cli swap-panes --help` to see more help*

Swaps two panes: `--pane-id` (by default the current pane, see
[Targeting Panes](index.md#targeting-panes)) and `--with`. Each takes the
other's place and size. They may be in the same tab or in two.

In one tab, the focus stays with the pane that had it, in its new place.
Between two tabs, each tab's focused place keeps the focus. A zoomed pane in
either tab is unzoomed first.

```console
$ wsb-wezterm cli swap-panes --pane-id 0 --with 3
```

The panes of a domain this multiplexer only mirrors (a GUI attached to a
multiplexer server, or tmux) are refused: run the command against that
domain's multiplexer instead.

## Synopsis

```console
{% include "../../examples/cmd-synopsis-wsb-wezterm-cli-swap-panes--help.txt" %}
```
