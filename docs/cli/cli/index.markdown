# `wsb-wezterm cli`

The *cli* subcommand interacts with a running wsb-wezterm GUI or multiplexer
instance, and can be used to spawn programs and manipulate tabs and panes.

# Targeting the correct instance

There may be multiple GUI processes running in addition to a multiplexer server.
wsb-wezterm uses the following logic to decide which one it should connect to.

* If the `--prefer-mux` flag is passed, then the `wsb-wezterm.lua` config file is
  consulted to determine the first *unix domain* defined by the config.
* If the `$WSB_WEZTERM_UNIX_SOCKET` environment variable is set, use that location
  to identify the running instance
* Try to locate a running GUI instance. The `--class` argument specifies an
  optional window class that can be used to select the appropriate GUI window
  if that GUI window was also spawned using `--class` to override the default.

# Targeting Panes

Various subcommands target panes via a (typically optional) `--pane-id` argument.

The following rules are used to determine a pane if `--pane-id` is not specified:

* If the `$WSB_WEZTERM_PANE` environment variable is set, it will be used
* The list of clients is retrieved and sorted by the most recently interacted
  session. The focused pane id from that session is used

See also: [wsb-wezterm cli list](list.md)

# Arranging a tab's panes

Any pane of a tab can be placed, sized and moved freely. Every change is made
by the multiplexer, so every client attached to the tab, GUI windows and
relays alike, shows the same layout at once, and each of them hears of each
change.

| To | Use |
|---|---|
| Read the tab's tree of splits, each pane's place and size, what runs in it, and which pane is focused and zoomed | [get-layout](get-layout.md) |
| Split any pane on any side, at a size | [split-pane](split-pane.md) |
| Set a split's size in cells | [resize-split](resize-split.md) |
| Move a divider by some cells | [adjust-pane-size](adjust-pane-size.md) |
| Move a pane beside another, in its tab or another | [split-pane --move-pane-id](split-pane.md#moving-a-pane) |
| Move a pane to a tab of its own | [move-pane-to-new-tab](move-pane-to-new-tab.md) |
| Swap two panes | [swap-panes](swap-panes.md) |
| Focus, zoom or close a pane | [activate-pane](activate-pane.md), [zoom-pane](zoom-pane.md), [kill-pane](kill-pane.md) |
| Set the whole layout in one step | [set-layout](set-layout.md) |
| Type text or keys into a pane, and read its text | [send-text](send-text.md), [send-keys](send-keys.md), [get-text](get-text.md) |
| Follow every change of the layout, and every pane's screen | [relay --tab-of-pane](relay.md#relaying-a-tab) |

All of these are requests of the multiplexer protocol, so they work the same
for a multiplexer reached over SSH or TLS as for a local one. A client of the
protocol is sent `TabLayoutChanged` with the tab's id whenever the tab's
panes, their places or sizes, the focused pane or the zoomed pane change, and
reads the new layout with `ListPanes`.

# Available Subcommands

