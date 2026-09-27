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
app_hamlib="$app/Contents/Resources/resources/hamlib"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "scripts/build-macos.sh is for macOS." >&2
  exit 1
fi

cd "$repo/src-tauri"
cargo tauri build --features radio,custom-protocol --bundles app \
  --config '{"bundle":{"createUpdaterArtifacts":false}}' \
  "${tauri_args[@]}" "$@"

# The dylibs are Tauri resources, not post-bundle mutations: DMG and updater archives must contain
# the same CAT runtime bytes the .app was signed over. This script never copies or changes bundle
# resources after assembly; it only seals the outer app when an unsigned local build needs it.
for file in rigctld rigctl rotctld rotctl libhamlib.4.dylib libusb-1.0.0.dylib; do
  [[ -s "$app_hamlib/$file" ]] || {
    echo "missing bundled Hamlib runtime: $app_hamlib/$file" >&2
    exit 1
  }
done
"$app_hamlib/rigctld" --version >/dev/null

# A local Tauri build has no Developer ID identity, but macOS still leaves an outer signature
# placeholder that fails its resource seal once bundled resources are present. Seal only the
# outer app, non-recursively, and carry the production audio-input entitlement. Nested Mach-O
# files keep their own signatures. CI release builds set APPLE_SIGNING_IDENTITY and are already
# signed/notarized by Tauri, so they must never be modified here.
if [[ -z "${APPLE_SIGNING_IDENTITY:-}" ]]; then
  codesign --force --sign - --entitlements "$repo/src-tauri/Entitlements.plist" "$app"
fi
codesign --verify --deep --strict --verbose=2 "$app"
codesign -d --entitlements - --xml "$app" 2>/dev/null \
  | plutil -convert json -o - - \
  | jq -e '.["com.apple.security.device.audio-input"] == true' >/dev/null

echo "$app"
