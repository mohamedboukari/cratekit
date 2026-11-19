#!/usr/bin/env bash
# Downloads Microsoft's official ONNX Runtime build for this machine's OS and
# CPU from NuGet, checks the package against a pinned SHA-256, and copies the
# library into <dest dir>. Prints the library's path. (#115; docs/ARCHITECTURE.md,
# Stems notes, has the per-target table.)
#
#   fetch-onnxruntime.sh <dest dir>
#
# 1.32.0 everywhere except Intel Macs, which get 1.23.2 (the last Intel build).
# Downloads are kept in ORT_CACHE (default ~/.cache/cratekit-onnxruntime).
set -euo pipefail

DEST="${1:?usage: fetch-onnxruntime.sh <dest dir>}"
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

version=1.32.0
case "$(uname -s)" in
  Darwin)
    if [ "$arch" = x64 ]; then
      version=1.23.2
    fi
    rid="osx-$arch"
    file=libonnxruntime.dylib
    ;;
  Linux)
    rid="linux-$arch"
    file=libonnxruntime.so
    ;;
  MINGW* | MSYS* | CYGWIN*)
    rid="win-$arch"
    file=onnxruntime.dll
    ;;
  *)
    echo "unsupported OS: $(uname -s)" >&2
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

unzip -o -j -q "$package" "runtimes/$rid/native/$file" -d "$DEST"
echo "$DEST/$file"
