#!/usr/bin/env bash
# Checks the built installers against the minimums in docs/ARCHITECTURE.md
# (Platforms and portability) and reports their sizes. Used by installers.yml;
# runs locally too, after `bun run tauri build`.
#
#   check-installer.sh <release dir>    e.g. target/release, target/universal-apple-darwin/release
#
# macOS:   every architecture of the app binary has `minos` equal to
#          bundle.macOS.minimumSystemVersion, and so does Info.plist.
# Linux:   no ELF file in the AppImage or the .deb needs a glibc newer than
#          MAX_GLIBC (2.35, Ubuntu 22.04); the .deb depends on ALSA, which
#          audio output loads at run time.
# Windows: the app imports no Visual C++ runtime DLL (Tauri links it statically),
#          so users never need a redistributable.
# All:     size against the budget: 90 MB for an AppImage (it bundles
#          WebKitGTK), 60 MB for the others (a warning, not a failure).
set -euo pipefail

RELEASE_DIR="${1:?usage: check-installer.sh <release dir>}"
BUNDLE_DIR="$RELEASE_DIR/bundle"
MAX_GLIBC="${MAX_GLIBC:-2.35}"
SUMMARY="${GITHUB_STEP_SUMMARY:-/dev/stdout}"
failed=0

fail() {
  echo "::error::$1"
  failed=1
}

# Adds a row to the installer's table in the job summary.
row() {
  echo "| $1 | $2 |" >> "$SUMMARY"
}

# Size against the budget for this kind of installer.
check_size() {
  local installer="$1" budget_mb=60 bytes size_mb
  case "$installer" in
    *.AppImage) budget_mb=90 ;;
  esac
  bytes=$(wc -c < "$installer" | tr -d ' ')
  size_mb=$(awk "BEGIN { printf \"%.1f\", $bytes / 1048576 }")
  row "Size" "$size_mb MB (budget $budget_mb MB)"
  if awk "BEGIN { exit !($bytes > $budget_mb * 1048576) }"; then
    echo "::warning::$(basename "$installer") is $size_mb MB, over the $budget_mb MB budget"
  fi
}

# The highest glibc any ELF file under a folder needs, against MAX_GLIBC.
check_glibc() {
  local root="$1" highest="0" highest_file="" file need
  while IFS= read -r -d '' file; do
    if [ "$(head -c 4 "$file" | tail -c 3)" = "ELF" ]; then
      need=$(objdump -T "$file" 2> /dev/null | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -V | tail -n 1 || true)
      if [ -n "$need" ] && [ "$(printf '%s\n%s\n' "$need" "$highest" | sort -V | tail -n 1)" = "$need" ]; then
        highest="$need"
        highest_file="${file#"$root"/}"
      fi
    fi
  done < <(find "$root" -type f -print0)
  row "Highest glibc needed" "$highest (${highest_file:-none}), maximum $MAX_GLIBC"
  if [ "$(printf '%s\n%s\n' "$highest" "$MAX_GLIBC" | sort -V | tail -n 1)" != "$MAX_GLIBC" ]; then
    fail "${highest_file:-a binary} needs glibc $highest, newer than $MAX_GLIBC"
  fi
}

check_macos() {
  local installer="$1" expected mount app plist bin archs arch minos
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
    row "$(basename "$bin")" "$archs, minos $minos (expected $expected)"
  done
  hdiutil detach "$mount" > /dev/null
}

check_linux() {
  local installer="$1" work depends
  work=$(mktemp -d)
  case "$installer" in
    *.AppImage)
      cp "$installer" "$work/app.AppImage"
      chmod +x "$work/app.AppImage"
      # Extracting needs no FUSE.
      (cd "$work" && ./app.AppImage --appimage-extract > /dev/null)
      check_glibc "$work/squashfs-root"
      ;;
    *.deb)
      dpkg-deb -x "$installer" "$work/root"
      check_glibc "$work/root"
      depends=$(dpkg-deb -f "$installer" Depends)
      row "Depends" "$depends"
      echo "$depends" | grep -q 'libasound2' || fail "$(basename "$installer") does not depend on libasound2 (ALSA, needed for audio output)"
      ;;
  esac
}

check_windows() {
  local host objdump exe dlls runtime
  host=$(rustc -vV | sed -n 's/^host: //p')
  objdump="$(rustc --print sysroot)/lib/rustlib/$host/bin/llvm-objdump.exe"
  exe=$(find "$RELEASE_DIR" -maxdepth 1 -name '*.exe' | head -n 1)
  dlls=$("$objdump" -p "$exe" | sed -n 's/^ *DLL Name: //p' | sort -u)
  row "DLLs imported" "$(echo "$dlls" | tr '\n' ' ')"
  runtime=$(echo "$dlls" | grep -iE '^(vcruntime|msvcp|concrt|vcomp)' || true)
  [ -z "$runtime" ] || fail "$(basename "$exe") imports the Visual C++ runtime: $runtime"
}

found=0
while IFS= read -r -d '' installer; do
  found=1
  {
    echo "### $(basename "$installer")"
    echo ""
    echo "| Check | Result |"
    echo "| --- | --- |"
  } >> "$SUMMARY"
  check_size "$installer"
  case "$(uname -s)" in
    Darwin) check_macos "$installer" ;;
    Linux) check_linux "$installer" ;;
    MINGW* | MSYS* | CYGWIN*) check_windows ;;
    *) fail "unknown platform $(uname -s)" ;;
  esac
  echo "" >> "$SUMMARY"
done < <(find "$BUNDLE_DIR" -maxdepth 2 -type f \( -name '*.dmg' -o -name '*.msi' -o -name '*.AppImage' -o -name '*.deb' \) -print0)

if [ "$found" -eq 0 ]; then
  echo "::error::no installer found under $BUNDLE_DIR"
  exit 1
fi
exit "$failed"
