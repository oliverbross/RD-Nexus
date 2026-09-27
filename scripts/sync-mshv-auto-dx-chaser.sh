#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
manifest="$script_dir/../integrations/mshv-auto-dx-chaser/UPSTREAM.toml"
read_manifest_value() {
  key="$1"
  awk -F ' *= *' -v key="$key" '$1 == key { value=$2; gsub(/^"|"$/, "", value); print value; exit }' "$manifest"
}
repo="$(read_manifest_value repository)"
branch="$(read_manifest_value branch)"
pin="$(read_manifest_value revision)"
test -n "$repo" && test -n "$branch" && test -n "$pin" || {
  echo "invalid DX Chaser upstream manifest: $manifest" >&2
  exit 2
}

usage() {
  echo "usage: $0 --check | --stage /absolute/empty/review-directory" >&2
  exit 2
}

case "${1:-}" in
  --check)
    test "$#" -eq 1 || usage
    head="$(git ls-remote "$repo" "refs/heads/$branch" | awk '{print $1}')"
    test -n "$head" || { echo "could not resolve upstream master" >&2; exit 1; }
    if test "$head" != "$pin"; then
      echo "MSHV Auto DX Chaser changed: reviewed=$pin upstream=$head" >&2
      echo "stage and review the new revision before updating UPSTREAM.toml" >&2
      exit 1
    fi
    echo "MSHV Auto DX Chaser pin is current: $pin"
    ;;
  --stage)
    test "$#" -eq 2 || usage
    destination="$2"
    case "$destination" in /*) ;; *) echo "stage directory must be absolute" >&2; exit 2;; esac
    if test -e "$destination" && test -n "$(find "$destination" -mindepth 1 -maxdepth 1 -print -quit)"; then
      echo "stage directory must not exist or must be empty: $destination" >&2
      exit 2
    fi
    mkdir -p "$destination"
    git clone --filter=blob:none "$repo" "$destination"
    git -C "$destination" checkout --detach "origin/$branch"
    echo "staged $(git -C "$destination" rev-parse HEAD) at $destination"
    echo "review against $pin; downloading is not parity approval"
    ;;
  *) usage ;;
esac
