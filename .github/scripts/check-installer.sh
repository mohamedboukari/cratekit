#!/usr/bin/env bash
# Checks one built installer against the minimums in docs/ARCHITECTURE.md
# (Platforms and portability) and reports its size. Used by installers.yml;
# runs locally too, after `bun run tauri build`.
#
#   check-installer.sh <release dir>    e.g. target/release, target/universal-apple-darwin/release
#
# macOS:   every architecture of the app binary has `minos` equal to
#          bundle.macOS.minimumSystemVersion, and so does Info.plist.
# Linux:   no ELF file in the AppImage needs a glibc newer than MAX_GLIBC (2.35, Ubuntu 22.04).
# Windows: the app imports no Visual C++ runtime DLL (Tauri links it statically),
#          so users never need a redistributable.
# All:     installer size against the 60 MB budget (a warning, not a failure).
set -euo pipefail

RELEASE_DIR="${1:?usage: check-installer.sh <release dir>}"
BUNDLE_DIR="$RELEASE_DIR/bundle"
MAX_GLIBC="${MAX_GLIBC:-2.35}"
BUDGET_MB=60
SUMMARY="${GITHUB_STEP_SUMMARY:-/dev/stdout}"
failed=0

fail() {
  echo "::error::$1"
  failed=1
}

# The installer, whatever the platform.
installer=$(find "$BUNDLE_DIR" -maxdepth 2 -type f \( -name '*.dmg' -o -name '*.msi' -o -name '*.AppImage' \) | head -n 1)
if [ -z "$installer" ]; then
  echo "::error::no installer found under $BUNDLE_DIR"
  exit 1
fi
bytes=$(wc -c < "$installer" | tr -d ' ')
size_mb=$(awk "BEGIN { printf \"%.1f\", $bytes / 1048576 }")
{
  echo "### $(basename "$installer")"
  echo ""
  echo "| Check | Result |"
  echo "| --- | --- |"
  echo "| Size | $size_mb MB (budget $BUDGET_MB MB) |"
} >> "$SUMMARY"
if awk "BEGIN { exit !($bytes > $BUDGET_MB * 1048576) }"; then
  echo "::warning::$(basename "$installer") is $size_mb MB, over the $BUDGET_MB MB budget"
fi

case "$(uname -s)" in
  Darwin)
    expected=$(jq -r '.bundle.macOS.minimumSystemVersion' src-tauri/tauri.conf.json)
    mount=$(mktemp -d)
    hdiutil attach -nobrowse -readonly -mountpoint "$mount" "$installer" > /dev/null
    app=$(find "$mount" -maxdepth 1 -name '*.app' | head -n 1)
    plist=$(/usr/libexec/PlistBuddy -c 'Print :LSMinimumSystemVersion' "$app/Contents/Info.plist")
    [ "$plist" = "$expected" ] || fail "Info.plist LSMinimumSystemVersion is $plist, expected $expected"
    for bin in "$app"/Contents/MacOS/*; do
      archs=$(lipo -archs "$bin")
      for arch in $archs; do
        minos=$(vtool -arch "$arch" -show-build "$bin" | awk '/minos/ { print $2; exit }')
        [ "$minos" = "$expected" ] || fail "$(basename "$bin") ($arch) has minos $minos, expected $expected"
      done
      echo "| $(basename "$bin") | $archs, minos $minos (expected $expected) |" >> "$SUMMARY"
    done
    hdiutil detach "$mount" > /dev/null
    ;;
  Linux)
    work=$(mktemp -d)
    cp "$installer" "$work/app.AppImage"
    chmod +x "$work/app.AppImage"
    # Extracting needs no FUSE.
    (cd "$work" && ./app.AppImage --appimage-extract > /dev/null)
    highest="0"
    while IFS= read -r -d '' file; do
      if [ "$(head -c 4 "$file" | tail -c 3)" = "ELF" ]; then
        need=$(objdump -T "$file" 2> /dev/null | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -V | tail -n 1 || true)
        if [ -n "$need" ] && [ "$(printf '%s\n%s\n' "$need" "$highest" | sort -V | tail -n 1)" = "$need" ]; then
          highest="$need"
          highest_file="${file#"$work"/squashfs-root/}"
        fi
      fi
    done < <(find "$work/squashfs-root" -type f -print0)
    echo "| Highest glibc needed | $highest (${highest_file:-none}), maximum $MAX_GLIBC |" >> "$SUMMARY"
    if [ "$(printf '%s\n%s\n' "$highest" "$MAX_GLIBC" | sort -V | tail -n 1)" != "$MAX_GLIBC" ]; then
      fail "${highest_file:-a binary} needs glibc $highest, newer than $MAX_GLIBC"
    fi
    ;;
  MINGW* | MSYS* | CYGWIN*)
    host=$(rustc -vV | sed -n 's/^host: //p')
    objdump="$(rustc --print sysroot)/lib/rustlib/$host/bin/llvm-objdump.exe"
    exe=$(find "$RELEASE_DIR" -maxdepth 1 -name '*.exe' | head -n 1)
    dlls=$("$objdump" -p "$exe" | sed -n 's/^ *DLL Name: //p' | sort -u)
    echo "| DLLs imported | $(echo "$dlls" | tr '\n' ' ') |" >> "$SUMMARY"
    runtime=$(echo "$dlls" | grep -iE '^(vcruntime|msvcp|concrt|vcomp)' || true)
    [ -z "$runtime" ] || fail "$(basename "$exe") imports the Visual C++ runtime: $runtime"
    ;;
  *)
    fail "unknown platform $(uname -s)"
    ;;
esac

echo "" >> "$SUMMARY"
exit "$failed"
