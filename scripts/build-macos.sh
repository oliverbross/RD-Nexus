#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
profile="release"
tauri_args=()
if [[ "${1:-}" == "--debug" ]]; then
  profile="debug"
  tauri_args+=(--debug)
  shift
fi
app="$repo/src-tauri/target/$profile/bundle/macos/RD-Nexus.app"
hamlib="$repo/src-tauri/resources/hamlib"
app_hamlib="$app/Contents/Resources/resources/hamlib"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "scripts/build-macos.sh is for macOS." >&2
  exit 1
fi

cd "$repo/src-tauri"
cargo tauri build --features radio,custom-protocol --bundles app \
  --config '{"bundle":{"createUpdaterArtifacts":false}}' \
  "${tauri_args[@]}" "$@"

# Tauri 2's resource glob currently errors while processing these dylibs. Keep the normal
# source config explicit, copy the two libraries after bundling, and re-sign the finished app.
mkdir -p "$app_hamlib"
ditto "$hamlib/libhamlib.4.dylib" "$app_hamlib/libhamlib.4.dylib"
ditto "$hamlib/libusb-1.0.0.dylib" "$app_hamlib/libusb-1.0.0.dylib"
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict --verbose=2 "$app"

echo "$app"
