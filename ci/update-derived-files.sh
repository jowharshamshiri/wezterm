#!/usr/bin/env bash

# Update files that are derived from things baked into the executable,
# from a debug build: `cargo build -p wezterm -p wezterm-gui -p strip-ansi-escapes` first.
set -e

bin=$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')/debug

for shell in bash zsh fish ; do
  $bin/wsb-wezterm shell-completion --shell $shell > assets/shell-completion/$shell
done

for mode in copy_mode search_mode ; do
  fname="docs/examples/default-$(echo $mode | tr _ -)-key-table.markdown"
  # Make a wrapped up version of this as markdown, as
  # gelatyx doesn't understand the file include mechanism
  # when used in a lua block
  echo "\`\`\`lua" > $fname
  $bin/wsb-wezterm -n show-keys --lua --key-table $mode >> $fname
  echo "\`\`\`" >> $fname
done

# For whatever reason, running --help on macOS vs. Linux results in different
# opinions on leading/trailing whitespace. In order to minimize diffs and
# be more consistent, explicitly trim leading/trailing space from the
# output stream.
# <https://unix.stackexchange.com/a/552191/123914>
trim_file() {
  perl -0777 -pe 's/^\n+|\n\K\n+$//g'
}

cargo run --example narrow $bin/wsb-wezterm --help | $bin/wsb-wezterm-strip-ansi-escapes | trim_file > docs/examples/cmd-synopsis-wsb-wezterm--help.txt

for cmd in start ssh serial connect ls-fonts show-keys imgcat set-working-directory record replay  ; do
  fname="docs/examples/cmd-synopsis-wsb-wezterm-${cmd}--help.txt"
  cargo run --example narrow $bin/wsb-wezterm $cmd --help | $bin/wsb-wezterm-strip-ansi-escapes | trim_file > $fname
done

for cmd in \
    activate-pane \
    activate-pane-direction \
    adjust-pane-size \
    activate-tab \
    get-layout \
    get-pane-direction \
    get-text \
    kill-pane \
    list \
    list-clients \
    move-pane-to-new-tab \
    relay \
    rename-workspace \
    resize-split \
    send-keys \
    send-text \
    set-layout \
    set-tab-title \
    set-tab-var \
    set-window-title \
    set-window-var \
    spawn \
    split-pane \
    swap-panes \
    zoom-pane \
    ; do
  fname="docs/examples/cmd-synopsis-wsb-wezterm-cli-${cmd}--help.txt"
  cargo run --example narrow $bin/wsb-wezterm cli $cmd --help | $bin/wsb-wezterm-strip-ansi-escapes | trim_file > $fname
done
