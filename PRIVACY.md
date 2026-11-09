# Privacy Policy for wsb-wezterm

No data about your device(s) or wsb-wezterm usage leave your device.
wsb-wezterm makes no network requests of its own: it connects only where
your configuration or your commands ask it to, such as to an SSH host or a
multiplexer server.

## Data Maintained by wsb-wezterm

wsb-wezterm maintains some historical data, such as recent searches or action
usage, in some of its overlays such as the debug overlay and character
selector, in order to make your usage more convenient. It is used only
by the local process, and care is taken to limit access for the associated
files on disk to only your local user identity.

wsb-wezterm tracks the output from the commands that you have executed in
a scrollback buffer.  That scrollback buffer is an in-memory structure that
is not visible to other users of the machine.

## macOS and Data permissions

On macOS, when a GUI application that has a "bundle" launches child processes
(eg: wsb-wezterm, running your shell, and your shell running the programs which you
direct it to run), any permissioned resource access that may be attempted by
those child processes will be reported as though wsb-wezterm is attempting to
access those resources.

The result is that from time to time you may see a dialog about wsb-wezterm
accessing your Contacts if run a `find` command that happens to step through
the portion of your filesystem where the contacts are stored.  Or perhaps you
are running a utility that accesses your camera; it will appear as though
wsb-wezterm is accessing those resources, but it is not: there is no logic within
wsb-wezterm to attempt to access your contacts, camera or any other sensitive
information.

## Third-Party Builds

The above is true of the wsb-wezterm source code at
https://github.com/jowharshamshiri/wsb-wezterm and the binaries built from it.

If you obtained a pre-built wsb-wezterm binary from some other source be aware that
the person(s) building those versions may have modified them to behave
differently from the source version.
