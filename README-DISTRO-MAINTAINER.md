# Notes for distro maintainers

`ci/deploy.sh` packages a release build of wsb-wezterm for the platform it
runs on. It is likely a bit more coarse than most distros would want in their
official packages, but it should give a sense of what is intended to go where.

## Versioning

wsb-wezterm's version number is derived from the date of the commit it was
built from:

```
git -c "core.abbrev=8" show -s "--format=%cd-%h" "--date=format:%Y%m%d-%H%M%S"
```

If you are not building wsb-wezterm from its git repo, wsb-wezterm reads a file
named `.tag` in the root of its source tree to determine the version: write the
version above into it.

If you must decorate wsb-wezterm's version, then it is recommend that you append
your supplemental extra version information on the end of wsb-wezterm's
version.

For example: `YYYYMMDD-HHMMSS-HASH-EXTRA`.

If `-` is illegal in your package management system, then it is recommended
that you substitute either `.` or `_` to separate the portions of the version
string.

## Binaries

* `wsb-wezterm-mux-server` - multiplexer server. No gui required. Likely want this
  in a separate package from `wsb-wezterm-gui` so it can run on a headless system.
* `wsb-wezterm-gui` - the GUI portion of the terminal
* `wsb-wezterm` - the CLI and frontend that knows how to launch the GUI. It is
  desirable to have this available to both the multiplexer server and the gui.
* `wsb-wezterm-strip-ansi-escapes` - a utility that can filter escapes out of stdin; useful
  for de-fanging text when composing eg: OSC 0/1 title text.

## Additional Resources

* `assets/shell-integration`, `assets/shell-completion`: should be deployed along with the `wsb-wezterm` executable
* `assets/wsb-wezterm.desktop` (as `com.bahramj.wsb-wezterm.desktop`),
  `assets/wsb-wezterm.appdata.xml` (as `com.bahramj.wsb-wezterm.appdata.xml`),
  `assets/wsb-wezterm-nautilus.py`: should be deployed along with `wsb-wezterm-gui`
* `termwiz/data/wsb-wezterm.terminfo`: the `wsb-wezterm` terminfo entry, compiled with `tic -x`

## Building wsb-wezterm

### Un-bundling vendored fonts

By default, wsb-wezterm will compile in a handful of fonts in order to provide a
consistent out of the box experience on all platforms with minimal installation
hassle.

If your distribution offers those fonts as installable packages, then it is
recommended that you skip compiling in that font by disabling the associated
feature:

* `vendor-nerd-font-symbols-font` - causes [Symbols Nerd Font
  Mono](https://github.com/ryanoasis/nerd-fonts/blob/master/patched-fonts/NerdFontsSymbolsOnly/complete/Symbols-1000-em%20Nerd%20Font%20Complete%20Mono.ttf)
  to be compiled in.
* `vendor-jetbrains-font` - causes `JetBrains Mono` to be compiled in
* `vendor-roboto-font` - causes `Roboto` to be compiled in
* `vendor-noto-emoji-font` - causes `Noto Color Emoji` to be compiled in.
* `vendored-fonts` - causes all of the above `vendor-*-font` features to be enabled

Note that wsb-wezterm requires at least the following fonts to be available, either
on the system or built-in, in its default configuration, in order to start
correctly:

* `JetBrains Mono`
* `Roboto`

### Building without wayland support

If your distro doesn't include any support for Wayland, you will need to
disable that feature when you build wsb-wezterm:

```
cargo build --release -p wezterm-gui --no-default-features --features vendored-fonts
```
