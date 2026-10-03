#!/usr/bin/env bash
# Verify that a distributable contains the compiled DX Chaser implementation.
#
# This deliberately inspects the package payload, not its parent directory or the
# source checkout. Tauri embeds both the Rust revision and the production web UI
# in the Nexus executable, so requiring both markers in the same packaged binary
# catches stale UI-only and native-only builds.
set -euo pipefail

usage() {
  echo "usage: $0 /absolute/Nexus.app-or-package" >&2
  exit 2
}

test "$#" -eq 1 || usage
artifact="$1"
case "$artifact" in /*) ;; *) echo "artifact path must be absolute" >&2; exit 2 ;; esac
test -e "$artifact" || { echo "artifact does not exist: $artifact" >&2; exit 2; }

script_dir="$(cd "$(dirname "$0")" && pwd)"
manifest="$script_dir/../integrations/mshv-auto-dx-chaser/UPSTREAM.toml"
revision="$(awk -F ' *= *' '$1 == "revision" { value=$2; gsub(/^"|"$/, "", value); print value; exit }' "$manifest")"
test -n "$revision" || { echo "missing revision in $manifest" >&2; exit 2; }
ui_marker="DX Chaser"
work="$(mktemp -d)"
mounted=""

cleanup() {
  if test -n "$mounted"; then
    hdiutil detach "$mounted" >/dev/null 2>&1 || true
  fi
  rm -rf "$work"
}
trap cleanup EXIT HUP INT TERM

seven_zip() {
  for candidate in 7z 7zz 7za; do
    if command -v "$candidate" >/dev/null 2>&1; then
      command -v "$candidate"
      return 0
    fi
  done
  return 1
}

extract_7z() {
  archive="$1"
  destination="$2"
  zipper="$(seven_zip)" || {
    echo "7z, 7zz, or 7za is required to inspect $(basename "$archive")" >&2
    return 2
  }
  mkdir -p "$destination"
  "$zipper" x -y "-o$destination" "$archive" >/dev/null
}

root=""
direct_executable=""

if test -d "$artifact"; then
  root="$artifact"
else
  case "$artifact" in
    *.deb)
      root="$work/deb"
      mkdir -p "$root"
      if command -v dpkg-deb >/dev/null 2>&1; then
        dpkg-deb -x "$artifact" "$root"
      else
        echo "dpkg-deb is required to inspect a Debian package" >&2
        exit 2
      fi
      ;;
    *.AppImage)
      root="$work/appimage"
      mkdir -p "$root"
      cp "$artifact" "$work/package.AppImage"
      chmod +x "$work/package.AppImage"
      (cd "$root" && APPIMAGE_EXTRACT_AND_RUN=1 "$work/package.AppImage" --appimage-extract >/dev/null)
      root="$root/squashfs-root"
      ;;
    *.dmg)
      command -v hdiutil >/dev/null 2>&1 || {
        echo "hdiutil is required to inspect a DMG (run this gate on macOS)" >&2
        exit 2
      }
      mounted="$work/dmg"
      mkdir -p "$mounted"
      hdiutil attach -readonly -nobrowse -mountpoint "$mounted" "$artifact" >/dev/null
      root="$mounted"
      ;;
    *.msi)
      root="$work/windows"
      extract_7z "$artifact" "$root"
      ;;
    *.exe)
      # A raw Nexus.exe is inspectable as-is. An NSIS installer is an archive
      # containing Nexus.exe. Accept the outer file only when it has both markers;
      # otherwise try extraction, then fall back to reporting the raw PE's missing
      # marker when it was not an archive.
      if grep -a -F -q "$revision" "$artifact" && grep -a -F -q "$ui_marker" "$artifact"; then
        direct_executable="$artifact"
      else
        root="$work/windows"
        if ! extract_7z "$artifact" "$root"; then
          root=""
          direct_executable="$artifact"
        fi
      fi
      ;;
    *.zip)
      root="$work/zip"
      mkdir -p "$root"
      command -v unzip >/dev/null 2>&1 || { echo "unzip is required to inspect a zip" >&2; exit 2; }
      unzip -q "$artifact" -d "$root"
      ;;
    *.tar|*.tar.gz|*.tgz)
      root="$work/tar"
      mkdir -p "$root"
      tar -xf "$artifact" -C "$root"
      ;;
    *)
      # This is intentionally a raw-file path, not a fallback archive scan.
      # It supports extensionless development binaries without making unknown
      # installers appear verified because a sibling package was good.
      direct_executable="$artifact"
      ;;
  esac
fi

candidates="$work/candidates"
: > "$candidates"
if test -n "$direct_executable"; then
  printf '%s\n' "$direct_executable" > "$candidates"
else
  test -n "$root" && test -d "$root" || { echo "package extraction produced no directory" >&2; exit 1; }
  find "$root" -type f \( -iname 'Nexus' -o -iname 'Nexus.exe' -o -iname 'RD-Nexus' -o -iname 'RD-Nexus.exe' \) -print > "$candidates"
fi

test -s "$candidates" || {
  echo "package contains no Nexus executable: $artifact" >&2
  exit 1
}

revision_seen=0
ui_seen=0
while IFS= read -r executable; do
  has_revision=0
  has_ui=0
  grep -a -F -q "$revision" "$executable" && has_revision=1
  grep -a -F -q "$ui_marker" "$executable" && has_ui=1
  test "$has_revision" -eq 1 && revision_seen=1
  test "$has_ui" -eq 1 && ui_seen=1
  if test "$has_revision" -eq 1 && test "$has_ui" -eq 1; then
    echo "DX Chaser package content verified: revision=$revision executable=$executable artifact=$artifact"
    exit 0
  fi
done < "$candidates"

test "$revision_seen" -eq 1 || echo "DX Chaser upstream revision missing from packaged Nexus executable" >&2
test "$ui_seen" -eq 1 || echo "DX Chaser UI missing from packaged Nexus executable" >&2
if test "$revision_seen" -eq 1 && test "$ui_seen" -eq 1; then
  echo "DX Chaser native and UI markers were split across different executables" >&2
fi
exit 1
