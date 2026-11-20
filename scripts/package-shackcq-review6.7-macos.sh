#!/bin/sh
set -eu

app_path="src-tauri/target/release/bundle/macos/ShackCQ Desktop Nexus Review 6.7.app"
out_dir="src-tauri/target/release/bundle/macos"
zip_path="$out_dir/ShackCQ_Desktop_Nexus_Review_1.14.0-review.6.7_aarch64.zip"
dmg_path="$out_dir/ShackCQ_Desktop_Nexus_Review_1.14.0-review.6.7_plain_aarch64.dmg"
identity="Developer ID Application: Oliver Bross (4WCMQ4U946)"

test -d "$app_path"
codesign --force --deep --options runtime --timestamp --entitlements src-tauri/Entitlements.plist --sign "$identity" "$app_path"
codesign --verify --deep --strict "$app_path"
ditto -c -k --sequesterRsrc --keepParent "$app_path" "$zip_path"

stage_path="$(mktemp -d /tmp/shackcq-review6.7-dmg.XXXXXX)"
cleanup() { rm -rf "$stage_path"; }
trap cleanup EXIT INT TERM
ditto "$app_path" "$stage_path/ShackCQ Desktop Nexus Review 6.7.app"
hdiutil create -ov -format UDZO -volname "ShackCQ Desktop Review 6.7" -srcfolder "$stage_path" "$dmg_path"
shasum -a 256 "$zip_path" "$dmg_path" "$app_path/Contents/MacOS/shackcq-desktop-nexus-review"
