#!/usr/bin/env bash
# Hermetic package-shape tests: no Nexus/release build and no host packaging tools.
set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
inspector="$script_dir/inspect-dx-chaser-package.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

manifest="$script_dir/../integrations/mshv-auto-dx-chaser/UPSTREAM.toml"
revision="$(awk -F ' *= *' '$1 == "revision" { value=$2; gsub(/^"|"$/, "", value); print value; exit }' "$manifest")"
fixture="$tmp/payload with spaces"
mkdir -p "$fixture/usr/bin"
printf 'MZ synthetic Nexus executable\n%s\nDX Chaser\n' "$revision" > "$fixture/usr/bin/Nexus.exe"
chmod +x "$fixture/usr/bin/Nexus.exe"

expect_pass() {
  "$inspector" "$1" >/dev/null
}

expect_fail() {
  if "$inspector" "$1" >"$tmp/unexpected.out" 2>&1; then
    echo "negative fixture unexpectedly passed: $1" >&2
    exit 1
  fi
}

# Expanded bundle and macOS .app.
expect_pass "$fixture"
mac_app="$tmp/Nexus Test.app"
mkdir -p "$mac_app/Contents/MacOS"
cp "$fixture/usr/bin/Nexus.exe" "$mac_app/Contents/MacOS/Nexus"
expect_pass "$mac_app"

# Raw Windows executable. Its parent also contains a good sibling; the bad file
# must still fail, proving the inspector never scans the artifact's directory.
raw_dir="$tmp/raw windows"
mkdir -p "$raw_dir"
cp "$fixture/usr/bin/Nexus.exe" "$raw_dir/Nexus.exe"
printf 'MZ stale executable\n' > "$raw_dir/stale.exe"
expect_pass "$raw_dir/Nexus.exe"
expect_fail "$raw_dir/stale.exe"

# Stub only the extractors. Each format still takes the production script's real
# dispatch/extraction/candidate-selection path.
fakebin="$tmp/fakebin"
mkdir -p "$fakebin"
export DX_CHASER_FIXTURE_PAYLOAD="$fixture"

cat > "$fakebin/dpkg-deb" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
test "$1" = -x
mkdir -p "$3"
cp -R "$DX_CHASER_FIXTURE_PAYLOAD/." "$3/"
STUB

cat > "$fakebin/7z" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
destination=""
for arg in "$@"; do case "$arg" in -o*) destination="${arg#-o}" ;; esac; done
test -n "$destination"
mkdir -p "$destination"
cp -R "$DX_CHASER_FIXTURE_PAYLOAD/." "$destination/"
STUB

cat > "$fakebin/hdiutil" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
case "$1" in
  attach)
    destination=""
    previous=""
    for arg in "$@"; do
      test "$previous" = -mountpoint && destination="$arg"
      previous="$arg"
    done
    test -n "$destination"
    mkdir -p "$destination/Nexus.app/Contents/MacOS"
    cp "$DX_CHASER_FIXTURE_PAYLOAD/usr/bin/Nexus.exe" "$destination/Nexus.app/Contents/MacOS/Nexus"
    ;;
  detach) ;;
  *) exit 2 ;;
esac
STUB
chmod +x "$fakebin/dpkg-deb" "$fakebin/7z" "$fakebin/hdiutil"
export PATH="$fakebin:$PATH"

touch "$tmp/Nexus.deb" "$tmp/Nexus.msi" "$tmp/Nexus-setup.exe" "$tmp/Nexus.dmg"
expect_pass "$tmp/Nexus.deb"
expect_pass "$tmp/Nexus.msi"
expect_pass "$tmp/Nexus-setup.exe"
expect_pass "$tmp/Nexus.dmg"

# A synthetic executable AppImage creates the same squashfs-root layout as
# --appimage-extract, while avoiding an AppImage/release build.
appimage="$tmp/Nexus.AppImage"
cat > "$appimage" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
test "${1:-}" = --appimage-extract
mkdir -p squashfs-root
cp -R "$DX_CHASER_FIXTURE_PAYLOAD/." squashfs-root/
STUB
chmod +x "$appimage"
expect_pass "$appimage"

# Neither half alone is sufficient, and two different executables cannot pool
# their markers into a false pass.
bad="$tmp/bad bundle"
mkdir -p "$bad"
printf 'MZ\n%s\n' "$revision" > "$bad/Nexus.exe"
expect_fail "$bad"
printf 'MZ\nDX Chaser\n' > "$bad/Nexus.exe"
expect_fail "$bad"
printf 'MZ\n%s\n' "$revision" > "$bad/Nexus.exe"
printf 'MZ\nDX Chaser\n' > "$bad/Nexus"
expect_fail "$bad"

echo "DX Chaser package inspector tests passed (app, DMG, exe, MSI/NSIS, deb, AppImage)"
