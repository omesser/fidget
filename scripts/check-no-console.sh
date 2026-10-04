#!/usr/bin/env bash
# Fail when a built package would open a console or a terminal. Usage:
# scripts/check-no-console.sh linux|macos|windows [cargo target dir], run after
# `tauri build` has produced the deb, the .app or the exe under target/release.

set -euo pipefail

fail() {
  echo "::error::$*" >&2
  exit 1
}

release=${2:-target}/release

case "${1:-}" in
  windows)
    exe=$release/fidget.exe
    # The PE header offset sits at 0x3C. Subsystem is 92 bytes past it in PE32
    # and PE32+ alike: 2 is WINDOWS_GUI, 3 is WINDOWS_CUI, the console.
    pe=$(od -An -tu4 -j60 -N4 "$exe" | tr -d ' ')
    subsystem=$(od -An -tu2 -j$((pe + 92)) -N2 "$exe" | tr -d ' ')
    [ "$subsystem" = 2 ] ||
      fail "$exe is PE subsystem $subsystem, not 2 (WINDOWS_GUI), so it opens a console"
    echo "$exe is PE subsystem 2 (WINDOWS_GUI)"
    ;;
  linux)
    debs=("$release"/bundle/deb/*.deb)
    root=$(mktemp -d)
    dpkg-deb -x "${debs[0]}" "$root"
    entries=("$root"/usr/share/applications/*.desktop)
    entry=${entries[0]}
    grep -qx 'Terminal=false' "$entry" || fail "${entry#"$root"/} lacks Terminal=false"
    program=$(sed -n 's/^Exec=\([^ ]*\).*/\1/p' "$entry")
    file -b "$root/usr/bin/$program" | grep -q '^ELF' ||
      fail "Exec=$program in ${entry#"$root"/} is not the ELF in usr/bin, so it may wrap a terminal"
    echo "${debs[0]}: Terminal=false, Exec=$program is the ELF"
    ;;
  macos)
    apps=("$release"/bundle/macos/*.app)
    app=${apps[0]}
    wrappers=$(find "$app" -name '*.command')
    [ -z "$wrappers" ] || fail "$app ships a Terminal launcher: $wrappers"
    program=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Contents/Info.plist")
    file -b "$app/Contents/MacOS/$program" | grep -q '^Mach-O' ||
      fail "CFBundleExecutable $program in $app is not a Mach-O, so it may be a shell launcher"
    echo "$app: no .command, CFBundleExecutable $program is a Mach-O"
    ;;
  *)
    fail "usage: $0 linux|macos|windows [cargo target dir]"
    ;;
esac
