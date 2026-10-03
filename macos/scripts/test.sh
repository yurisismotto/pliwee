#!/usr/bin/env bash
# Runs the Swift test suite (PliweeKitTests).
#
#   macos/scripts/test.sh [swift test arguments…]
#
# With Xcode selected this is plain `swift test`. With only the Command Line
# Tools, SwiftPM's default build system does not load the Swift Testing macro
# plugin that the Command Line Tools ship, and every `@Test` fails to expand
# ("plugin for module 'TestingMacros' not found"). The plugin is there; it is
# loaded explicitly. This changes how the tests are compiled, never which
# tests run or what they assert.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

args=()
developer_dir="$(xcode-select -p)"
plugin="$developer_dir/usr/lib/swift/host/plugins/testing/libTestingMacros.dylib"
case "$developer_dir" in
    *CommandLineTools*)
        [ -f "$plugin" ] || { echo "test.sh: $plugin not found" >&2; exit 1; }
        args+=(-Xswiftc -load-plugin-library -Xswiftc "$plugin")
        ;;
esac

exec swift test ${args[@]+"${args[@]}"} "$@"
