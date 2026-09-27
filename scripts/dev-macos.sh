#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "scripts/dev-macos.sh is for macOS." >&2
  exit 1
fi

# Tauri's dev runner keeps Vite alive for instant React/CSS HMR and lets Cargo reuse its
# incremental cache for Rust changes. The bundled Hamlib tools inherit this path so their two
# dylibs can stay in the source tree instead of being copied into every development build.
export DYLD_LIBRARY_PATH="$repo/src-tauri/resources/hamlib${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}"

cd "$repo/src-tauri"
exec cargo tauri dev --features radio "$@"
