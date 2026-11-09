#!/usr/bin/env bash
set -x
rm -rf AppDir *.AppImage *.zsync
set -e

mkdir AppDir

install -Dsm755 -t AppDir/usr/bin target/release/wsb-wezterm-mux-server
install -Dsm755 -t AppDir/usr/bin target/release/wsb-wezterm
install -Dsm755 -t AppDir/usr/bin target/release/wsb-wezterm-gui
install -Dsm755 -t AppDir/usr/bin target/release/strip-ansi-escapes
install -Dm644 assets/icon/terminal.png AppDir/usr/share/icons/hicolor/128x128/apps/com.bahramj.wsb-wezterm.png
install -Dm644 assets/wsb-wezterm.desktop AppDir/usr/share/applications/com.bahramj.wsb-wezterm.desktop
install -Dm644 assets/wsb-wezterm.appdata.xml AppDir/usr/share/metainfo/com.bahramj.wsb-wezterm.appdata.xml
install -Dm644 assets/wsb-wezterm-nautilus.py AppDir/usr/share/nautilus-python/extensions/wsb-wezterm-nautilus.py

[ -x /tmp/linuxdeploy ] || ( curl -L 'https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage' -o /tmp/linuxdeploy && chmod +x /tmp/linuxdeploy )

TAG_NAME=${TAG_NAME:-$(git -c "core.abbrev=8" show -s "--format=%cd-%h" "--date=format:%Y%m%d-%H%M%S")}
distro=$(lsb_release -is 2>/dev/null || sh -c "source /etc/os-release && echo \$NAME")
distver=$(lsb_release -rs 2>/dev/null || sh -c "source /etc/os-release && echo \$VERSION_ID")

OUTPUT=wsb-wezterm-$TAG_NAME-$distro$distver.AppImage

# Munge the path so that it finds our appstreamcli wrapper
PATH="$PWD/ci:$PATH" \
VERSION="$TAG_NAME" \
OUTPUT="$OUTPUT" \
  /tmp/linuxdeploy \
  --exclude-library='libwayland-client.so.0' \
  --appdir AppDir \
  --output appimage \
  --desktop-file assets/wsb-wezterm.desktop
