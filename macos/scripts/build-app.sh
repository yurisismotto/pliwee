#!/usr/bin/env bash
# Builds Pliwee.app: the Rust agent and CLI, the SwiftUI app, and the bundle
# that holds them.
#
#   macos/scripts/build-app.sh [--debug] [--arch arm64|x86_64|universal]
#                              [--out DIR] [--sign IDENTITY]
#
# Produces  <out>/Pliwee.app   (default out: macos/build)
#
#   Contents/MacOS/Pliwee        the SwiftUI application
#   Contents/MacOS/pliweed       the agent, registered as a launchd agent
#   Contents/Helpers/pliwee      the command-line tool — not beside the app
#                                in MacOS/: APFS is case-insensitive by
#                                default, and `pliwee` would overwrite `Pliwee`
#   Contents/Library/LaunchAgents/io.github.yurisismotto.pliwee.daemon.plist
#   Contents/Resources/AppIcon.icns, pliwee-mark.svg, pliwee-mark-mono.svg
#
# Needs Rust (rustup) and the Xcode Command Line Tools. Xcode itself is not
# needed. The person who runs the result needs neither: the bundle is
# self-contained.
#
# Signing: without --sign the bundle is signed ad hoc ("-"), which is what a
# local build can do. That is enough to run on the Mac that built it, and it
# is NOT a distribution signature: Gatekeeper rejects an ad-hoc app copied to
# another Mac, and nothing here notarizes. --sign takes a Developer ID
# Application identity from the keychain and signs with the hardened runtime
# and a secure timestamp; notarization (`notarytool submit`, `stapler
# staple`) is a separate, documented release step — see macos/README.md.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MACOS="$ROOT/macos"
DESKTOP="$ROOT/desktop"
PROFILE=release
ARCH="$(uname -m)"
OUT="$MACOS/build"
IDENTITY="-"
LABEL="io.github.yurisismotto.pliwee.daemon"

die() { echo "build-app: $*" >&2; exit 1; }
note() { echo "build-app: $*"; }

while [ $# -gt 0 ]; do
    case "$1" in
        --debug) PROFILE=debug; shift ;;
        --arch) ARCH="${2:?}"; shift 2 ;;
        --out) OUT="${2:?}"; shift 2 ;;
        --sign) IDENTITY="${2:?}"; shift 2 ;;
        -h|--help) sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) die "unknown argument: $1" ;;
    esac
done

case "$ARCH" in
    arm64) ARCHS=(arm64) ;;
    x86_64) ARCHS=(x86_64) ;;
    universal) ARCHS=(arm64 x86_64) ;;
    *) die "--arch must be arm64, x86_64 or universal, not $ARCH" ;;
esac

# --- preconditions -----------------------------------------------------------
# Every tool is looked for before anything is built: a missing one must stop
# the build here, by name, not halfway through with a bundle half assembled.
export PATH="$HOME/.cargo/bin:$PATH"
for tool in cargo rustup swift iconutil codesign plutil lipo; do
    command -v "$tool" >/dev/null 2>&1 || die "$tool not found on PATH"
done
[ "$(uname -s)" = Darwin ] || die "this builds a macOS bundle and must run on macOS"

rust_target() {
    case "$1" in
        arm64) echo aarch64-apple-darwin ;;
        x86_64) echo x86_64-apple-darwin ;;
    esac
}

VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$DESKTOP/Cargo.toml" | head -n 1)"
[ -n "$VERSION" ] || die "could not read the workspace version from desktop/Cargo.toml"

# --- one label, three places --------------------------------------------------
# launchd knows the agent by this label; the agent recognises its own job by
# it to choose its log file; the app registers the plist by it. A mismatch
# would build a bundle whose service starts and then cannot be found.
PLIST_SRC="$MACOS/Resources/LaunchAgents/$LABEL.plist"
[ -f "$PLIST_SRC" ] || die "missing $PLIST_SRC"
plist_label="$(plutil -extract Label raw -o - "$PLIST_SRC")"
[ "$plist_label" = "$LABEL" ] || die "plist Label is '$plist_label', expected '$LABEL'"
grep -q "pub const LAUNCHD_LABEL: &str = \"$LABEL\";" "$DESKTOP/platform-macos/src/lib.rs" \
    || die "pliwee_macos::LAUNCHD_LABEL does not say $LABEL"
grep -q "static let label = \"$LABEL\"" "$MACOS/Sources/Pliwee/Services/AgentService.swift" \
    || die "AgentService.label does not say $LABEL"
note "launchd label agrees in all three places: $LABEL"

# --- Rust: the agent and the CLI ----------------------------------------------
RUST_OUTS=()
for arch in "${ARCHS[@]}"; do
    target="$(rust_target "$arch")"
    # Captured, then searched: never `| grep -q`, whose early exit kills the
    # producer with SIGPIPE and, under pipefail, turns a match into a miss.
    installed="$(rustup target list --installed)"
    grep -qx "$target" <<<"$installed" \
        || die "Rust target $target is not installed: rustup target add $target"
    note "cargo build ($PROFILE, $target): pliwee-daemon, pliwee-cli"
    cargo_profile=()
    [ "$PROFILE" = release ] && cargo_profile=(--release)
    ( cd "$DESKTOP" && cargo build --locked ${cargo_profile[@]+"${cargo_profile[@]}"} --target "$target" \
        -p pliwee-daemon -p pliwee-cli )
    RUST_OUTS+=("$DESKTOP/target/$target/$PROFILE")
done

# --- Swift: the application ----------------------------------------------------
# Each architecture's binary is copied out as soon as it is built: SwiftPM's
# build system writes every architecture to the same product path, so the
# second build would otherwise replace the first before `lipo` sees it
# (measured: "lipo: same architectures (x86_64) found").
SWIFT_STAGE="$(mktemp -d)"
SWIFT_OUTS=()
for arch in "${ARCHS[@]}"; do
    note "swift build ($PROFILE, $arch): Pliwee"
    ( cd "$MACOS" && swift build -c "$PROFILE" --arch "$arch" --product Pliwee )
    bin="$(cd "$MACOS" && swift build -c "$PROFILE" --arch "$arch" --show-bin-path)"
    mkdir -p "$SWIFT_STAGE/$arch"
    cp "$bin/Pliwee" "$SWIFT_STAGE/$arch/Pliwee"
    SWIFT_OUTS+=("$SWIFT_STAGE/$arch")
done

# --- the bundle ----------------------------------------------------------------
APP="$OUT/Pliwee.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Helpers" "$APP/Contents/Resources" \
    "$APP/Contents/Library/LaunchAgents"

# One binary per executable, `lipo`-merged when universal.
place() { # path inside Contents/, then one source per arch
    local name="$1"; shift
    if [ $# -eq 1 ]; then
        cp "$1" "$APP/Contents/$name"
    else
        lipo -create "$@" -output "$APP/Contents/$name"
    fi
}
sources() { local file="$1"; shift; for d in "$@"; do echo "$d/$file"; done; }
# shellcheck disable=SC2046
place MacOS/Pliwee $(sources Pliwee "${SWIFT_OUTS[@]}")
# shellcheck disable=SC2046
place MacOS/pliweed $(sources pliweed "${RUST_OUTS[@]}")
# shellcheck disable=SC2046
place Helpers/pliwee $(sources pliwee "${RUST_OUTS[@]}")

sed "s/@VERSION@/$VERSION/g" "$MACOS/Resources/Info.plist" > "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist" >/dev/null || die "Info.plist does not lint"
cp "$PLIST_SRC" "$APP/Contents/Library/LaunchAgents/"
printf 'APPL????' > "$APP/Contents/PkgInfo"

# Brand artwork, copied unchanged from the one source the other platforms use.
ASSETS="$ROOT/docs/design/assets"
cp "$ASSETS/pliwee-mark.svg" "$ASSETS/pliwee-mark-mono.svg" "$APP/Contents/Resources/"
ICONSET="$(mktemp -d)/AppIcon.iconset"
swift "$MACOS/scripts/render-icon.swift" "$ASSETS/pliwee-app-icon.svg" "$ICONSET"
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"
rm -rf "$(dirname "$ICONSET")"

# --- what was built is what was meant ------------------------------------------
# Exactly these three executables, each a Mach-O with every requested
# architecture. Named and counted, not "more than zero": the first build of
# this script put the CLI beside the app in MacOS/, where the case-insensitive
# filesystem let `pliwee` overwrite `Pliwee` — a bundle that opened the CLI —
# and only the count noticed.
executables=("$APP/Contents/MacOS/Pliwee" "$APP/Contents/MacOS/pliweed" "$APP/Contents/Helpers/pliwee")
found=$(find "$APP/Contents/MacOS" "$APP/Contents/Helpers" -type f | wc -l | tr -d ' ')
[ "$found" -eq 3 ] || die "expected 3 executables in MacOS/ and Helpers/, found $found"
for exe in "${executables[@]}"; do
    [ -f "$exe" ] || die "missing $exe"
    archs="$(lipo -archs "$exe")"
    for arch in "${ARCHS[@]}"; do
        case " $archs " in *" $arch "*) ;; *) die "$(basename "$exe") lacks $arch (has: $archs)" ;; esac
    done
done

# Identity, not just presence: the app binary links SwiftUI and the agent does
# not. Grepped from a file, never through a pipe into `grep -q` (AGENTS.md).
links="$(mktemp)"
otool -L "$APP/Contents/MacOS/Pliwee" > "$links"
grep -q 'SwiftUI.framework' "$links" || die "Contents/MacOS/Pliwee is not the SwiftUI application"
otool -L "$APP/Contents/MacOS/pliweed" > "$links"
if grep -q 'SwiftUI.framework' "$links"; then die "Contents/MacOS/pliweed links SwiftUI; wrong binary"; fi
rm -f "$links"

# --- signing -------------------------------------------------------------------
# Inside out: the helpers first, then the bundle, which seals them. The
# hardened runtime is on in both modes so that what is tested locally is what
# a Developer ID build would run under.
sign_flags=(--force --options runtime)
if [ "$IDENTITY" = "-" ]; then
    sign_flags+=(--sign - --timestamp=none)
    note "signing ad hoc (local use only; not a distribution signature)"
else
    sign_flags+=(--sign "$IDENTITY" --timestamp)
    note "signing with: $IDENTITY"
fi
# Each helper gets an identifier under the bundle's. Without one, codesign
# names an ad-hoc binary `pliweed-<cdhash>`, and macOS's local network
# privacy — which tracks code by its signature — finds no application to
# attribute the agent to: measured, `nehelper` logs "Failed to find
# pliweed-… using neagent" and the agent's multicast never reaches the LAN.
codesign "${sign_flags[@]}" --identifier "$LABEL" "$APP/Contents/MacOS/pliweed"
codesign "${sign_flags[@]}" --identifier io.github.yurisismotto.pliwee.cli "$APP/Contents/Helpers/pliwee"
codesign "${sign_flags[@]}" --identifier io.github.yurisismotto.pliwee "$APP"
codesign --verify --strict --deep "$APP" || die "codesign verification failed"

rm -rf "$SWIFT_STAGE"
note "built $APP"
note "  version $VERSION, ${ARCHS[*]}, $PROFILE"
for exe in "${executables[@]}"; do
    note "  ${exe#"$APP/Contents/"}: $(lipo -archs "$exe"), $(du -h "$exe" | cut -f1)"
done
