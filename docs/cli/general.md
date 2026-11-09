# Command Line

This section documents the wsb-wezterm command line.

*Note that `wsb-wezterm --help` or `wsb-wezterm SUBCOMMAND --help` will show the precise
set of options that are applicable to your installed version of wsb-wezterm.*

wsb-wezterm is deployed with two major executables:

* `wsb-wezterm` (or `wsb-wezterm.exe` on Windows) - for interacting with wsb-wezterm from the terminal
* `wsb-wezterm-gui` (or `wsb-wezterm-gui.exe` on Windows) - for spawning wsb-wezterm from a desktop environment

You will typically use `wsb-wezterm` when scripting wsb-wezterm; it knows when to
delegate to `wsb-wezterm-gui` under the covers.

If you are setting up a launcher for wsb-wezterm to run in the Windows GUI
environment then you will want to explicitly target `wsb-wezterm-gui` so that
Windows itself doesn't pop up a console host for its logging output.

!!! note
    `wsb-wezterm-gui.exe --help` will not output anything to a console when
    run on Windows systems, because it runs in the Windows GUI subsystem and has no
    connection to the console.  You can use `wsb-wezterm.exe --help` to see information
    about the various commands; it will delegate to `wsb-wezterm-gui.exe` when
    appropriate.

## Synopsis

```console
{% include "../examples/cmd-synopsis-wsb-wezterm--help.txt" %}
```
