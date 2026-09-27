#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
profile="release"
tauri_args=(
  --features radio,custom-protocol
  --bundles app
  --config '{"bundle":{"createUpdaterArtifacts":false}}'
)
if [[ "${1:-}" == "--debug" ]]; then
  profile="debug"
  tauri_args+=(--debug)
  shift
fi
app="$repo/src-tauri/target/$profile/bundle/macos/RD-Nexus.app"
app_hamlib="$app/Contents/Resources/resources/hamlib"
app_neural="$app/Contents/Resources/resources/neural-dx"
app_openham="$app/Contents/Resources/resources/openhamclock"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "scripts/build-macos.sh is for macOS." >&2
  exit 1
fi

cd "$repo/src-tauri"
cargo tauri build "${tauri_args[@]}" "$@"

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

# Neural DX is an integral bundled companion, not an optional README-only resource. Verify the
# assembled app rather than the staging directory so a Tauri glob/layout regression cannot pass.
neural="$app_neural/neural-dx-watcher"
for file in "$neural" "$app_neural/provenance.json" "$app_neural/UPSTREAM-README.txt" \
  "$app_neural/SOURCE-PERMISSION.txt"; do
  [[ -s "$file" ]] || {
    echo "missing bundled Neural DX resource: $file" >&2
    exit 1
  }
done
[[ -x "$neural" ]] || { echo "bundled Neural DX sidecar is not executable: $neural" >&2; exit 1; }
expected_neural_commit="$(jq -er '.commit' "$repo/docs/neural-dx/upstream.json")"
bundled_neural_commit="$(jq -er '.commit' "$app_neural/provenance.json")"
[[ "$bundled_neural_commit" == "$expected_neural_commit" ]] || {
  echo "bundled Neural DX pin mismatch: $bundled_neural_commit != $expected_neural_commit" >&2
  exit 1
}
[[ "$(jq -er '.platform' "$app_neural/provenance.json")" == "darwin" ]] || {
  echo "bundled Neural DX provenance is not macOS" >&2
  exit 1
}
[[ "$(jq -er '.arch' "$app_neural/provenance.json")" == "arm64" ]] || {
  echo "bundled Neural DX provenance is not Apple Silicon" >&2
  exit 1
}
file "$neural" | grep -q 'Mach-O.*arm64' || {
  echo "bundled Neural DX sidecar is not an arm64 Mach-O executable" >&2
  exit 1
}
neural_tmp="$(mktemp -d)"
trap 'rm -rf "$neural_tmp"' EXIT
neural_self_test="$(cd "$neural_tmp" && NEURAL_DX_SELF_TEST=1 "$neural")"
expected_neural_version="$(jq -er '.version' "$repo/docs/neural-dx/upstream.json")"
printf '%s\n' "$neural_self_test" \
  | node "$repo/scripts/check-neural-dx-self-test.mjs" --version "$expected_neural_version"

# OpenHamClock's build receipt is specific to this staging run. Compare it and the executable
# bytes against the assembled app so a resource-glob or bundle-layout regression cannot pass by
# checking only the staging directory.
node "$repo/scripts/check-openhamclock-package.mjs" \
  --target macos-arm64 --bundle-resources "$app/Contents/Resources"
[[ -x "$app_openham/openhamclock" ]] || {
  echo "bundled OpenHamClock executable is not executable: $app_openham/openhamclock" >&2
  exit 1
}

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
