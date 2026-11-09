#!/usr/bin/env bash
# Packages a release build of wsb-wezterm for the platform it runs on:
# a zip holding wsb-wezterm.app on macOS, a zip and an installer on Windows,
# an rpm on Fedora, CentOS and SUSE, a deb on Debian, Ubuntu and Pop!_OS,
# and an apk on Alpine. The programs are those `cargo build --release` made
# in TARGET_DIR (the first argument, `target` when there is none).
set -x
set -e

TARGET_DIR=${1:-target}

TAG_NAME=${TAG_NAME:-$(git -c "core.abbrev=8" show -s "--format=%cd-%h" "--date=format:%Y%m%d-%H%M%S")}

HERE=$(pwd)

if test -z "${SUDO+x}" && hash sudo 2>/dev/null; then
  SUDO="sudo"
fi

if test -e /etc/os-release; then
  . /etc/os-release
fi

echo "OSTYPE is $OSTYPE"

case $OSTYPE in
  darwin*)
    zipdir=wsb-wezterm-macos-$TAG_NAME
    zipname=$zipdir.zip
    app=$zipdir/wsb-wezterm.app
    rm -rf $zipdir $zipname
    mkdir $zipdir
    cp -r assets/macos/wsb-wezterm.app $zipdir/
    # Omit MetalANGLE for now; it's a bit laggy compared to CGL,
    # and on M1/Big Sur, CGL is implemented in terms of Metal anyway
    rm $app/*.dylib
    mkdir -p $app/Contents/MacOS
    mkdir -p $app/Contents/Resources
    cp -r assets/shell-integration/* $app/Contents/Resources
    cp -r assets/shell-completion $app/Contents/Resources
    tic -x -o $app/Contents/Resources/terminfo termwiz/data/wsb-wezterm.terminfo

    for bin in wsb-wezterm wsb-wezterm-mux-server wsb-wezterm-gui wsb-wezterm-strip-ansi-escapes ; do
      # A plain `cargo build --release` makes a package for one architecture.
      if [[ -f $TARGET_DIR/release/$bin ]] ; then
        cp $TARGET_DIR/release/$bin $app/Contents/MacOS/$bin
      else
        # `cargo build --target XXX --release` for each architecture puts
        # the programs in `$TARGET_DIR/XXX/release`; `lipo` assembles them
        # into a Universal ("fat") binary.
        lipo $TARGET_DIR/*/release/$bin -output $app/Contents/MacOS/$bin -create
      fi
    done

    set +x
    if [ -n "$MACOS_TEAM_ID" ] ; then
      MACOS_PW=$(echo $MACOS_CERT_PW | base64 --decode)

      # Remove pesky additional quotes from default-keychain output
      def_keychain=$(eval echo $(security default-keychain -d user))
      echo "Default keychain is $def_keychain"
      echo "Speculative delete of build.keychain"
      security delete-keychain build.keychain || true
      echo "Create build.keychain"
      security create-keychain -p "$MACOS_PW" build.keychain
      echo "Make build.keychain the default"
      security default-keychain -d user -s build.keychain
      echo "Unlock build.keychain"
      security unlock-keychain -p "$MACOS_PW" build.keychain
      echo "Import .p12 data"
      echo $MACOS_CERT | base64 --decode > /tmp/certificate.p12
      security import /tmp/certificate.p12 -k build.keychain -P "$MACOS_PW" -T /usr/bin/codesign
      rm /tmp/certificate.p12
      echo "Grant apple tools access to build.keychain"
      security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$MACOS_PW" build.keychain
      echo "Codesign"
      /usr/bin/codesign --keychain build.keychain --force --options runtime \
        --entitlements ci/macos-entitlement.plist --deep --sign "$MACOS_TEAM_ID" $app/
      echo "Restore default keychain"
      security default-keychain -d user -s $def_keychain
      echo "Remove build.keychain"
      security delete-keychain build.keychain || true
    fi

    set -x
    zip -r $zipname $zipdir
    set +x

    if [ -n "$MACOS_TEAM_ID" ] ; then
      echo "Notarize"
      xcrun notarytool submit $zipname --wait --team-id "$MACOS_TEAM_ID" --apple-id "$MACOS_APPLEID" --password "$MACOS_APP_PW"
    fi
    set -x
    ;;
  msys|cygwin)
    zipdir=wsb-wezterm-windows-$TAG_NAME
    zipname=$zipdir.zip
    instname=wsb-wezterm-${TAG_NAME}-setup
    rm -rf $zipdir $zipname
    mkdir $zipdir
    cp $TARGET_DIR/release/wsb-wezterm.exe \
      $TARGET_DIR/release/wsb-wezterm-mux-server.exe \
      $TARGET_DIR/release/wsb-wezterm-gui.exe \
      $TARGET_DIR/release/wsb-wezterm-strip-ansi-escapes.exe \
      $TARGET_DIR/release/wsb_wezterm.pdb \
      assets/windows/conhost/conpty.dll \
      assets/windows/conhost/OpenConsole.exe \
      assets/windows/angle/libEGL.dll \
      assets/windows/angle/libGLESv2.dll \
      $zipdir
    mkdir $zipdir/mesa
    cp $TARGET_DIR/release/mesa/opengl32.dll \
        $zipdir/mesa
    7z a -tzip $zipname $zipdir
    iscc.exe -DMyAppVersion=${TAG_NAME} -F${instname} ci/windows-installer.iss
    ;;
  linux-gnu|linux)
    distro=$(lsb_release -is 2>/dev/null || sh -c "source /etc/os-release && echo \$NAME")
    distver=$(lsb_release -rs 2>/dev/null || sh -c "source /etc/os-release && echo \$VERSION_ID")
    case "$distro" in
      *Fedora*|*CentOS*|*SUSE*)
        WSB_WEZTERM_RPM_VERSION=$(echo ${TAG_NAME} | tr - _)
        distroid=$(sh -c "source /etc/os-release && echo \$ID" | tr - _)
        distver=$(sh -c "source /etc/os-release && echo \$VERSION_ID" | tr - _)

        cat > wsb-wezterm.spec <<EOF
Name: wsb-wezterm
Version: ${WSB_WEZTERM_RPM_VERSION}
Release: 1.${distroid}${distver}
Packager: Bahram Joharshamshiri <jowharshamshiri@gmail.com>
License: MIT
URL: https://github.com/jowharshamshiri/wsb-wezterm
Summary: wsb-wezterm, a fork of WezTerm.
Requires: wsb-wezterm-common, wsb-wezterm-gui, wsb-wezterm-mux-server

%global debug_package %{nil}

%description
wsb-wezterm is a fork of WezTerm, Wez Furlong's terminal emulator, with
support for modern features such as fonts with ligatures, hyperlinks,
tabs and multiple windows.

# Subpackage: wsb-wezterm-common
%package -n wsb-wezterm-common
Summary: wsb-wezterm - Common CLI components
Requires: openssl
%description -n wsb-wezterm-common
wsb-wezterm-common provides the base CLI launcher and utilities shared by
all wsb-wezterm components.

# Subpackage: wsb-wezterm-gui
%package -n wsb-wezterm-gui
Summary: wsb-wezterm - GUI and multiplexer
Requires: wsb-wezterm-common
%if 0%{?suse_version}
Requires: dbus-1, fontconfig, libxcb1, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libwayland-egl1, libwayland-cursor0, Mesa-libEGL1, libxcb-keysyms1, libxcb-ewmh2, libxcb-icccm4
%else
Requires: dbus, fontconfig, libxcb, libxkbcommon, libxkbcommon-x11, libwayland-client, libwayland-egl, libwayland-cursor, mesa-libEGL, xcb-util-keysyms, xcb-util-wm
%endif
%description -n wsb-wezterm-gui
wsb-wezterm-gui is a GPU-accelerated cross-platform terminal emulator with
support for modern features such as fonts with ligatures, hyperlinks,
tabs and multiple windows.

# Subpackage: wsb-wezterm-mux-server
%package -n wsb-wezterm-mux-server
Summary: wsb-wezterm - Multiplexer server (headless)
Requires: openssl
%description -n wsb-wezterm-mux-server
wsb-wezterm-mux-server is a headless terminal multiplexer that can be used
as a session manager for terminal sessions, without requiring X11,
Wayland, or other GUI libraries.

%build
echo build

%install
set -x
cd ${HERE}
mkdir -p %{buildroot}/usr/bin %{buildroot}/etc/profile.d %{buildroot}/usr/share/icons/hicolor/128x128/apps %{buildroot}/usr/share/applications %{buildroot}/usr/share/metainfo %{buildroot}/usr/share/nautilus-python/extensions
install -Dm755 assets/open-wsb-wezterm-here -t %{buildroot}/usr/bin
install -Dsm755 $TARGET_DIR/release/wsb-wezterm -t %{buildroot}/usr/bin
install -Dsm755 $TARGET_DIR/release/wsb-wezterm-gui -t %{buildroot}/usr/bin
install -Dsm755 $TARGET_DIR/release/wsb-wezterm-mux-server -t %{buildroot}/usr/bin
install -Dsm755 $TARGET_DIR/release/wsb-wezterm-strip-ansi-escapes -t %{buildroot}/usr/bin
install -Dm644 assets/shell-integration/* -t %{buildroot}/etc/profile.d
install -Dm644 assets/shell-completion/zsh %{buildroot}/usr/share/zsh/site-functions/_wsb-wezterm
install -Dm644 assets/shell-completion/bash %{buildroot}/etc/bash_completion.d/wsb-wezterm
install -Dm644 assets/icon/terminal.png %{buildroot}/usr/share/icons/hicolor/128x128/apps/com.bahramj.wsb-wezterm.png
install -Dm644 assets/wsb-wezterm.desktop %{buildroot}/usr/share/applications/com.bahramj.wsb-wezterm.desktop
install -Dm644 assets/wsb-wezterm.appdata.xml %{buildroot}/usr/share/metainfo/com.bahramj.wsb-wezterm.appdata.xml
install -Dm644 assets/wsb-wezterm-nautilus.py %{buildroot}/usr/share/nautilus-python/extensions/wsb-wezterm-nautilus.py

%files
# Main package (metapackage) has no files

%files -n wsb-wezterm-common
/usr/bin/wsb-wezterm
/usr/bin/wsb-wezterm-strip-ansi-escapes
/usr/share/zsh/site-functions/_wsb-wezterm
/etc/bash_completion.d/wsb-wezterm
/etc/profile.d/*

%files -n wsb-wezterm-gui
/usr/bin/open-wsb-wezterm-here
/usr/bin/wsb-wezterm-gui
/usr/share/icons/hicolor/128x128/apps/com.bahramj.wsb-wezterm.png
/usr/share/applications/com.bahramj.wsb-wezterm.desktop
/usr/share/metainfo/com.bahramj.wsb-wezterm.appdata.xml
/usr/share/nautilus-python/extensions/wsb-wezterm-nautilus.py*

%files -n wsb-wezterm-mux-server
/usr/bin/wsb-wezterm-mux-server
EOF

        /usr/bin/rpmbuild -bb --rmspec wsb-wezterm.spec --verbose
        ;;
      Ubuntu*|Debian*|Pop)
        rm -rf pkg
        mkdir -p pkg/debian/usr/bin pkg/debian/DEBIAN pkg/debian/usr/share/applications

        cat > pkg/debian/control <<EOF
Package: wsb-wezterm
Version: ${TAG_NAME}
Architecture: $(dpkg-architecture -q DEB_BUILD_ARCH_CPU)
Maintainer: Bahram Joharshamshiri <jowharshamshiri@gmail.com>
Section: utils
Priority: optional
Homepage: https://github.com/jowharshamshiri/wsb-wezterm
Description: wsb-wezterm, a fork of WezTerm.
 wsb-wezterm is a fork of WezTerm, Wez Furlong's terminal emulator, with
 support for modern features such as fonts with ligatures, hyperlinks,
 tabs and multiple windows.
Provides: x-terminal-emulator
Source: wsb-wezterm
EOF

        cat > pkg/debian/postinst <<EOF
#!/bin/sh
set -e
if [ "\$1" = "configure" ] ; then
        update-alternatives --install /usr/bin/x-terminal-emulator x-terminal-emulator /usr/bin/open-wsb-wezterm-here 20
fi
EOF

        cat > pkg/debian/prerm <<EOF
#!/bin/sh
set -e
if [ "\$1" = "remove" ]; then
	update-alternatives --remove x-terminal-emulator /usr/bin/open-wsb-wezterm-here
fi
EOF

        install -Dsm755 -t pkg/debian/usr/bin $TARGET_DIR/release/wsb-wezterm-mux-server
        install -Dsm755 -t pkg/debian/usr/bin $TARGET_DIR/release/wsb-wezterm-gui
        install -Dsm755 -t pkg/debian/usr/bin $TARGET_DIR/release/wsb-wezterm
        install -Dm755 -t pkg/debian/usr/bin assets/open-wsb-wezterm-here
        install -Dsm755 -t pkg/debian/usr/bin $TARGET_DIR/release/wsb-wezterm-strip-ansi-escapes

        deps=$(cd pkg && dpkg-shlibdeps -O -e debian/usr/bin/*)
        mv pkg/debian/postinst pkg/debian/DEBIAN/postinst
        chmod 0755 pkg/debian/DEBIAN/postinst
        mv pkg/debian/prerm pkg/debian/DEBIAN/prerm
        chmod 0755 pkg/debian/DEBIAN/prerm
        mv pkg/debian/control pkg/debian/DEBIAN/control
        sed -i '/^Source:/d' pkg/debian/DEBIAN/control  # The `Source:` field needs to be valid in a binary package
        echo $deps | sed -e 's/shlibs:Depends=/Depends: /' >> pkg/debian/DEBIAN/control
        cat pkg/debian/DEBIAN/control

        install -Dm644 assets/icon/terminal.png pkg/debian/usr/share/icons/hicolor/128x128/apps/com.bahramj.wsb-wezterm.png
        install -Dm644 assets/wsb-wezterm.desktop pkg/debian/usr/share/applications/com.bahramj.wsb-wezterm.desktop
        install -Dm644 assets/wsb-wezterm.appdata.xml pkg/debian/usr/share/metainfo/com.bahramj.wsb-wezterm.appdata.xml
        install -Dm644 assets/wsb-wezterm-nautilus.py pkg/debian/usr/share/nautilus-python/extensions/wsb-wezterm-nautilus.py
        install -Dm644 assets/shell-completion/bash pkg/debian/usr/share/bash-completion/completions/wsb-wezterm
        install -Dm644 assets/shell-completion/zsh pkg/debian/usr/share/zsh/functions/Completion/Unix/_wsb-wezterm
        install -Dm644 assets/shell-integration/* -t pkg/debian/etc/profile.d

        debname=wsb-wezterm-$TAG_NAME.$distro$distver
        arch=$(dpkg-architecture -q DEB_BUILD_ARCH_CPU)
        case $arch in
          amd64)
            ;;
          *)
            debname="${debname}.${arch}"
            ;;
        esac

        fakeroot dpkg-deb --build pkg/debian $debname.deb

        mv pkg/debian pkg/wsb-wezterm
        tar cJf $debname.tar.xz -C pkg wsb-wezterm
        rm -rf pkg
      ;;
    esac
    ;;
  linux-musl)
    case $ID in
      alpine)
        export SUDO=''
        abuild-keygen -a -n -b 8192
        cat > APKBUILD <<EOF
# Maintainer: Bahram Joharshamshiri <jowharshamshiri@gmail.com>
pkgname=wsb-wezterm
pkgver=$(echo "$TAG_NAME" | cut -d'-' -f1-2 | tr - .)
_pkgver=$pkgver
pkgrel=0
pkgdesc="wsb-wezterm, a fork of WezTerm: a GPU-accelerated cross-platform terminal emulator and multiplexer"
license="MIT"
arch="all"
options="!check"
url="https://github.com/jowharshamshiri/wsb-wezterm"
makedepends="cmd:tic"
source="
  $TARGET_DIR/release/wsb-wezterm
  $TARGET_DIR/release/wsb-wezterm-gui
  $TARGET_DIR/release/wsb-wezterm-mux-server
  assets/open-wsb-wezterm-here
  assets/wsb-wezterm.desktop
  assets/wsb-wezterm.appdata.xml
  assets/icon/terminal.png
  assets/icon/wezterm-icon.svg
  termwiz/data/wsb-wezterm.terminfo
"
builddir="\$srcdir"

build() {
  tic -x -o "\$builddir"/terminfo "\$srcdir"/wsb-wezterm.terminfo
}

package() {
  install -Dm755 -t "\$pkgdir"/usr/bin "\$srcdir"/open-wsb-wezterm-here
  install -Dm755 -t "\$pkgdir"/usr/bin "\$srcdir"/wsb-wezterm
  install -Dm755 -t "\$pkgdir"/usr/bin "\$srcdir"/wsb-wezterm-gui
  install -Dm755 -t "\$pkgdir"/usr/bin "\$srcdir"/wsb-wezterm-mux-server

  install -Dm644 "\$srcdir"/wsb-wezterm.desktop "\$pkgdir"/usr/share/applications/com.bahramj.wsb-wezterm.desktop
  install -Dm644 "\$srcdir"/wsb-wezterm.appdata.xml "\$pkgdir"/usr/share/metainfo/com.bahramj.wsb-wezterm.appdata.xml
  install -Dm644 "\$srcdir"/terminal.png "\$pkgdir"/usr/share/icons/hicolor/128x128/apps/com.bahramj.wsb-wezterm.png
  install -Dm644 "\$srcdir"/wezterm-icon.svg "\$pkgdir"/usr/share/icons/hicolor/scalable/apps/com.bahramj.wsb-wezterm.svg
  install -Dm644 "\$builddir"/terminfo/w/wsb-wezterm "\$pkgdir"/usr/share/terminfo/w/wsb-wezterm
}
EOF
        abuild -F checksum
        abuild -Fr
      ;;
    esac
    ;;
  *)
    ;;
esac
