---
hide:
  - navigation
---

wsb-wezterm is built from its source, on Linux, macOS and Windows:
[Build from source](install/source.md) says how. `ci/deploy.sh` then packages
the build for the platform it runs on: a deb on Debian and Ubuntu, an rpm on
Fedora, CentOS and SUSE, an apk on Alpine, `wsb-wezterm.app` in a zip on macOS,
and a zip and an installer on Windows.

wsb-wezterm installs under its own names, beside any wsb-wezterm that is installed:
its programs are `wsb-wezterm`, `wsb-wezterm-gui` and `wsb-wezterm-mux-server`,
and it reads its configuration from `wsb-wezterm.lua`
(see [Configuration Files](config/files.md)).
