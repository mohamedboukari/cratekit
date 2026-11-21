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
# Stems:   every package carries ONNX Runtime (#31): on macOS with a slice for
#          each architecture of the app and a minimum no newer than the app's;
#          on Windows with every Visual C++ runtime DLL it imports next to it,
#          and nothing else outside Windows' own DLLs.
# All:     size against the budget: 100 MB for an AppImage (it bundles
#          WebKitGTK and ONNX Runtime), 60 MB for the others (a warning, not
#          a failure).
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
    *.AppImage) budget_mb=100 ;;
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
  local library="$app/Contents/Resources/onnxruntime/libonnxruntime.dylib" lib_archs lib_minos
  if [ -f "$library" ]; then
    lib_archs=$(lipo -archs "$library")
    for arch in $archs; do
      case " $lib_archs " in
        *" $arch "*) ;;
        *) fail "ONNX Runtime has no $arch slice (it has: $lib_archs)" ;;
      esac
      lib_minos=$(vtool -arch "$arch" -show-build "$library" | awk '/minos/ { print $2; exit }')
      if [ "$(printf '%s\n%s\n' "$lib_minos" "$expected" | sort -V | tail -n 1)" != "$expected" ]; then
        fail "ONNX Runtime ($arch) needs macOS $lib_minos, newer than the app's $expected"
      fi
    done
    row "ONNX Runtime" "$lib_archs, minos $lib_minos"
  else
    fail "the app does not contain ONNX Runtime (Contents/Resources/onnxruntime/libonnxruntime.dylib)"
  fi
  hdiutil detach "$mount" > /dev/null
}

# Fails unless the folder holds ONNX Runtime's Linux library somewhere.
need_linux_onnxruntime() {
  local found
  found=$(find "$1" -type f -name libonnxruntime.so | head -n 1)
  if [ -n "$found" ]; then
    row "ONNX Runtime" "${found#"$1"/}"
  else
    fail "the package does not contain ONNX Runtime (libonnxruntime.so)"
  fi
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
      need_linux_onnxruntime "$work/squashfs-root"
      ;;
    *.deb)
      dpkg-deb -x "$installer" "$work/root"
      check_glibc "$work/root"
      need_linux_onnxruntime "$work/root"
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
  check_windows_onnxruntime "$1" "$objdump"
}

# Extracts the MSI (an administrative install into a folder) and checks the
# ONNX Runtime folder: every DLL there imports only DLLs that are bundled next
# to it or are Windows' own (System32, API sets). A Visual C++ runtime DLL
# must be bundled even when this machine has it installed.
check_windows_onnxruntime() {
  local installer="$1" objdump="$2" work msi out folder dll name imports import
  work=$(mktemp -d)
  msi=$(cygpath -w "$installer")
  out=$(cygpath -w "$work")
  powershell.exe -NoProfile -Command "\$p = Start-Process msiexec.exe -ArgumentList @('/a', '\"$msi\"', '/qn', 'TARGETDIR=\"$out\"') -Wait -PassThru; exit \$p.ExitCode"
  folder=$(find "$work" -type f -iname onnxruntime.dll -exec dirname {} \; | head -n 1)
  if [ -z "$folder" ]; then
    fail "the installer does not contain ONNX Runtime (onnxruntime.dll)"
    return
  fi
  row "ONNX Runtime folder" "$(find "$folder" -maxdepth 1 -type f -iname '*.dll' -exec basename {} \; | sort | tr '\n' ' ')"
  for dll in "$folder"/*.dll; do
    name=$(basename "$dll")
    imports=$("$objdump" -p "$dll" | sed -n 's/^ *DLL Name: //p' | sort -u)
    for import in $imports; do
      if [ -n "$(find "$folder" -maxdepth 1 -iname "$import" | head -n 1)" ]; then
        continue
      fi
      case "$(echo "$import" | tr '[:upper:]' '[:lower:]')" in
        vcruntime* | msvcp* | concrt* | vcomp*)
          fail "$name imports $import, which is not bundled next to it"
          ;;
        api-ms-win-* | ext-ms-win-*) ;;
        *)
          [ -n "$(find /c/Windows/System32 -maxdepth 1 -iname "$import" | head -n 1)" ] ||
            fail "$name imports $import, which is neither bundled nor part of Windows"
          ;;
      esac
    done
  done
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
    MINGW* | MSYS* | CYGWIN*) check_windows "$installer" ;;
    *) fail "unknown platform $(uname -s)" ;;
  esac
  echo "" >> "$SUMMARY"
done < <(find "$BUNDLE_DIR" -maxdepth 2 -type f \( -name '*.dmg' -o -name '*.msi' -o -name '*.AppImage' -o -name '*.deb' \) -print0)

if [ "$found" -eq 0 ]; then
  echo "::error::no installer found under $BUNDLE_DIR"
  exit 1
fi
exit "$failed"
