wsb-wezterm should run on any modern unix as well as Windows 10 and macOS.

* Install `rustup` to get the `rust` compiler installed on your system.
  [Install rustup](https://rust-lang.org/tools/install/).
* Rust version 1.71 or later is required
* Build in release mode: `cargo build --release`
* Run it via either `cargo run --release --bin wsb-wezterm` or `target/release/wsb-wezterm`

You will need a collection of support libraries; the [`get-deps`](https://github.com/jowharshamshiri/wsb-wezterm/blob/main/get-deps) script will
attempt to install them for you.

```console
$ curl https://sh.rustup.rs -sSf | sh -s
$ git clone --depth=1 --branch=main --recursive https://github.com/jowharshamshiri/wsb-wezterm.git
$ cd wsb-wezterm
$ git submodule update --init --recursive
$ ./get-deps
$ cargo build --release
$ cargo run --release --bin wsb-wezterm -- start
```

**If you get an error about zlib then you most likely didn't initialize the submodules;
take a closer look at the instructions!**

A build names itself, as `wsb-wezterm --version` prints it, by the commit git
describes. A source with no git, such as a copy of the files alone, names it
in a file `.tag` at its top, holding what
`git -c core.abbrev=8 show -s --format=%cd-%h --date=format:%Y%m%d-%H%M%S`
prints for its commit. A source with neither is refused, rather than built
without a name.

To package the build for the platform you are on, run `ci/deploy.sh` from the
root of the source tree after `cargo build --release -p wezterm -p wezterm-gui
-p wezterm-mux-server -p strip-ansi-escapes`.

### Building without Wayland support on Unix systems

By default, support for both X11 and Wayland is included on Unix systems.
If your distribution has X11 but not Wayland, then you can build wsb-wezterm without
Wayland support by changing the `build` invocation:

```console
$ cargo build --release --no-default-features --features vendored-fonts
```

Building without X11 is not supported.

### Building on Windows

When installing Rust, you must use select the MSVC version of Rust. It is the
only supported way to build wsb-wezterm.

On Windows, instead of using `get-deps`, the only other dependency that you need is
[Strawberry Perl](https://strawberryperl.com). You must ensure that you have
your `PATH` environment set up to find that particular `perl.exe` ahead of any
other perl that you may have installed on your system. This particular version
of perl is required to build openssl on Windows.

```console
$ set PATH=c:\Strawberry\perl\bin;%PATH%
```

Packaging on Windows needs 7-Zip and Inno Setup (`iscc.exe`) on `PATH`, and the
bash of Git for Windows to run `ci/deploy.sh`.
