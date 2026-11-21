#!/usr/bin/env bash
# Downloads Microsoft's official ONNX Runtime build for this machine's OS and
# CPU from NuGet, checks the package against a pinned SHA-256, and copies the
# library into <dest dir>. Prints the library's path. (#115, #31;
# docs/ARCHITECTURE.md, Stems notes, has the per-target table.)
#
#   fetch-onnxruntime.sh [--universal] [--vc-runtime] <dest dir>
#
#   --universal   macOS: one library with both the arm64 and the x86_64 slice
#                 (for the universal app), combined with lipo.
#   --vc-runtime  Windows: also copy the Visual C++ runtime DLLs the library
#                 imports, from this machine's Visual Studio, next to it
#                 (Microsoft's "local deployment"; the installer ships them).
#
# 1.23.2 on macOS (both slices; the last Intel build, and faster on the CPU
# than 1.32 on Apple Silicon), 1.32.0 on Windows and Linux.
# Downloads are kept in ORT_CACHE (default ~/.cache/cratekit-onnxruntime).
set -euo pipefail

universal=0
vc_runtime=0
while [ $# -gt 1 ]; do
  case "$1" in
    --universal) universal=1 ;;
    --vc-runtime) vc_runtime=1 ;;
    *)
      echo "unknown option: $1" >&2
      exit 1
      ;;
  esac
  shift
done
DEST="${1:?usage: fetch-onnxruntime.sh [--universal] [--vc-runtime] <dest dir>}"
CACHE="${ORT_CACHE:-$HOME/.cache/cratekit-onnxruntime}"

# Pinned packages: version and SHA-256 of the .nupkg.
SHA_1_32_0="a9150bc2992cbbb6722070da9910e1ffa989a8bfc67bdd5bb8e745ca755641db"
SHA_1_23_2="25fe172f7fcdf34f2b5b02c8b997f6b88abc282956849d4af4335af23d0c4e4a"

# The runner's architecture when CI says it (Git Bash on Windows ARM64 may run
# emulated, so `uname -m` would not be the machine's), else uname.
arch="${RUNNER_ARCH:-$(uname -m)}"
case "$arch" in
  X64 | x86_64 | amd64) arch=x64 ;;
  ARM64 | arm64 | aarch64) arch=arm64 ;;
  *)
    echo "unsupported CPU: $arch" >&2
    exit 1
    ;;
esac

os="$(uname -s)"
case "$os" in
  Darwin)
    version=1.23.2
    file=libonnxruntime.dylib
    ;;
  Linux)
    version=1.32.0
    file=libonnxruntime.so
    ;;
  MINGW* | MSYS* | CYGWIN*)
    version=1.32.0
    file=onnxruntime.dll
    ;;
  *)
    echo "unsupported OS: $os" >&2
    exit 1
    ;;
esac
case "$version" in
  1.32.0) expected="$SHA_1_32_0" ;;
  1.23.2) expected="$SHA_1_23_2" ;;
esac

sha256() {
  if command -v sha256sum > /dev/null; then
    sha256sum "$1" | cut -d ' ' -f 1
  else
    shasum -a 256 "$1" | cut -d ' ' -f 1
  fi
}

mkdir -p "$CACHE" "$DEST"
package="$CACHE/microsoft.ml.onnxruntime.$version.nupkg"
if [ ! -f "$package" ] || [ "$(sha256 "$package")" != "$expected" ]; then
  curl --fail --silent --show-error --location --retry 3 --output "$package.part" \
    "https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime/$version/microsoft.ml.onnxruntime.$version.nupkg"
  mv "$package.part" "$package"
fi
actual="$(sha256 "$package")"
if [ "$actual" != "$expected" ]; then
  echo "checksum mismatch for ONNX Runtime $version: expected $expected, got $actual" >&2
  exit 1
fi

# One platform folder of the package into a directory.
extract() {
  unzip -o -j -q "$package" "runtimes/$1/native/$file" -d "$2"
}

case "$os" in
  Darwin)
    if [ "$universal" = 1 ]; then
      work=$(mktemp -d)
      extract osx-arm64 "$work/arm64"
      extract osx-x64 "$work/x64"
      lipo -create "$work/arm64/$file" "$work/x64/$file" -output "$DEST/$file"
      rm -rf "$work"
    else
      extract "osx-$arch" "$DEST"
    fi
    ;;
  Linux)
    extract "linux-$arch" "$DEST"
    ;;
  *)
    extract "win-$arch" "$DEST"
    ;;
esac

if [ "$vc_runtime" = 1 ]; then
  case "$os" in
    MINGW* | MSYS* | CYGWIN*) ;;
    *)
      echo "--vc-runtime is for Windows only" >&2
      exit 1
      ;;
  esac
  # The DLLs the library imports from the Visual C++ runtime, read with the
  # Rust toolchain's llvm-objdump.
  host=$(rustc -vV | sed -n 's/^host: //p')
  objdump="$(rustc --print sysroot)/lib/rustlib/$host/bin/llvm-objdump.exe"
  needed=$("$objdump" -p "$DEST/$file" | sed -n 's/^ *DLL Name: //p' | grep -iE '^(vcruntime|msvcp|concrt|vcomp)' || true)
  # Visual Studio's redistributable folder for this CPU.
  vswhere="/c/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe"
  install=$(cygpath -u "$("$vswhere" -latest -products '*' -property installationPath | tr -d '\r')")
  crt=$(find "$install/VC/Redist/MSVC" -maxdepth 3 -type d -path "*/$arch/Microsoft.VC14*.CRT" | sort | tail -n 1)
  if [ -z "$crt" ]; then
    echo "no Visual C++ redistributable folder for $arch under $install" >&2
    exit 1
  fi
  for dll in $needed; do
    source=$(find "$crt" -maxdepth 1 -iname "$dll" | head -n 1)
    if [ -z "$source" ]; then
      echo "$dll is not in $crt" >&2
      exit 1
    fi
    cp "$source" "$DEST/"
  done
fi

echo "$DEST/$file"
