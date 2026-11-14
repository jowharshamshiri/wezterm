#!/usr/bin/env bash
# On a Linux guest: builds wsb-wezterm, wsb-wezterm-gui and
# wsb-wezterm-mux-server with QuickRust, then runs the tests of the
# multiplexer, its protocol and the command line, which drive a real
# multiplexer server, and a window of the GUI on a virtual X display (Xvfb).
#
# Every step runs even when an earlier one failed, and the task fails when
# any did.
set -uo pipefail

cd "$MG_TEST_WORKSPACE"
logs="$MG_TEST_WORKSPACE/out/logs"
mkdir -p "$logs"

say() { printf 'wsb-wezterm-test: %s\n' "$*"; }
die() { say "$*" >&2; exit 1; }

# The guest has no git: the build names itself by `.tag`, which the run
# ships (see mg-test.yaml).
[ -s .tag ] || die "the source has no .tag; write one before the run, as mg-test.yaml says"
say "building $(cat .tag)"

# What installing QuickRust needs and an image may not have.
for tool in curl jq; do
    command -v "$tool" >/dev/null 2>&1 && continue
    sudo DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=600 update -qq \
        || die "apt-get update failed"
    sudo DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=600 install -y -qq "$tool" \
        || die "$tool could not be installed"
done

# Rust is built by QuickRust, the one thing installed: it installs and
# manages the toolchain itself, and decides where builds go. Its home is the
# cache, so the toolchain and its build cache outlive the guest. It comes
# from the release manifest, the archive checked against the manifest's
# SHA-256 and size before anything in it runs.
export QRUST_HOME="$MG_TEST_CACHE_RUST/qrust"
export PATH="$QRUST_HOME/bin:$PATH"
if ! [ -x "$QRUST_HOME/bin/qrust" ]; then
    case "$(uname -m)" in
        x86_64) platform=linux-x86_64 ;;
        aarch64|arm64) platform=linux-arm64 ;;
        *) die "QuickRust publishes no build for $(uname -m)" ;;
    esac
    scratch="$(mktemp -d)"
    curl -fsSL https://release.machinefabric.com/quickrust/manifest -o "$scratch/manifest" \
        || die "the QuickRust release manifest could not be fetched"
    read -r url sha size < <(jq -r --arg platform "$platform" '
        .channels.release as $release
        | $release.versions[$release.latestVersion].platforms[$platform]
            // error("QuickRust \($release.latestVersion) has no \($platform) build")
        | [.installers[] | select(.type == "targz")][0]
            // error("QuickRust \($release.latestVersion) has no archive for \($platform)")
        | "\(.path) \(.sha256) \(.size)"' "$scratch/manifest"
) || die "the QuickRust manifest names no archive for $platform"
    say "installing QuickRust from $url"
    curl -fsSL "$url" -o "$scratch/qrust.tar.gz" || die "the QuickRust archive could not be fetched"
    [ "$(stat -c %s "$scratch/qrust.tar.gz")" = "$size" ] \
        || die "the QuickRust archive is not the size the manifest says"
    echo "$sha  $scratch/qrust.tar.gz" | sha256sum -c --quiet \
        || die "the QuickRust archive does not match the manifest's SHA-256"
    tar xzf "$scratch/qrust.tar.gz" -C "$scratch" || die "the QuickRust archive could not be unpacked"
    "$scratch/quickrust/qrust" setup --no-modify-path || die "qrust setup failed"
    rm -rf "$scratch"
fi
[ "$(command -v cargo)" = "$QRUST_HOME/bin/cargo" ] \
    || die "cargo on PATH is $(command -v cargo), not QuickRust's"
say "$(cargo --version) on $(uname -m)"

# The libraries the GUI builds against, and the tools building needs.
# get-deps checks for Rust last, so it comes after QuickRust.
say "installing the build dependencies"
sudo DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=600 update -qq \
    || die "apt-get update failed"
SUDO="sudo DEBIAN_FRONTEND=noninteractive" sh ./get-deps >"$logs/get-deps.log" 2>&1 \
    || die "get-deps failed (out/logs/get-deps.log)"

failed=()
run() {
    local name="$1"; shift
    say "running $name"
    if "$@" >"$logs/$name.log" 2>&1; then
        say "$name passed"
    else
        say "$name FAILED (out/logs/$name.log)"
        failed+=("$name")
    fi
    grep -E "^test result:|^error|FAILED|panicked|Finished" "$logs/$name.log" | tail -n 40
}

# The window tests show a GUI window on a display of their own and resize
# it with xdotool.
say "installing Xvfb and xdotool"
sudo DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=600 install -y -qq xvfb xdotool \
    || die "Xvfb and xdotool could not be installed"
Xvfb :97 -screen 0 1600x1000x24 -nolisten tcp >"$logs/xvfb.log" 2>&1 &
xvfb=$!
trap 'kill "$xvfb" 2>/dev/null' EXIT
for _ in $(seq 50); do [ -e /tmp/.X11-unix/X97 ] && break; sleep 0.1; done
[ -e /tmp/.X11-unix/X97 ] || die "Xvfb did not start (out/logs/xvfb.log)"
export WSB_WEZTERM_TEST_DISPLAY=:97

run build cargo build -p wezterm -p wezterm-gui -p wezterm-mux-server
run test cargo test --no-fail-fast -p mux -p codec -p wezterm -p wezterm-client -p wezterm-mux-server-impl

if [ "${#failed[@]}" -gt 0 ]; then
    die "failed: ${failed[*]}"
fi
say "built, and every test passed"
