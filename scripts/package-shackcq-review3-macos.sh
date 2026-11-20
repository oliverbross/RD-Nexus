#!/bin/sh
set -eu

app_path="src-tauri/target/release/bundle/macos/ShackCQ Desktop Nexus Review 3.app"
out_dir="src-tauri/target/release/bundle/macos"
zip_path="$out_dir/ShackCQ_Desktop_Nexus_Review_1.14.0-review.3_aarch64.zip"
dmg_path="$out_dir/ShackCQ_Desktop_Nexus_Review_1.14.0-review.3_plain_aarch64.dmg"
stage_path=$(mktemp -d /tmp/shackcq-review3-dmg.XXXXXX)

cleanup_stage() {
  case "$stage_path" in
    /tmp/shackcq-review3-dmg.*) /bin/rm -rf -- "$stage_path" ;;
  esac
}
trap cleanup_stage EXIT HUP INT TERM

codesign --force --deep --sign - "$app_path"
codesign --verify --deep --strict --verbose=2 "$app_path"
/bin/rm -f -- "$zip_path" "$dmg_path"
ditto -c -k --sequesterRsrc --keepParent "$app_path" "$zip_path"
ditto "$app_path" "$stage_path/ShackCQ Desktop Nexus Review 3.app"
ln -s /Applications "$stage_path/Applications"
hdiutil create -ov -format UDZO -volname "ShackCQ Desktop Review 3" -srcfolder "$stage_path" "$dmg_path"
plutil -p "$app_path/Contents/Info.plist" | rg 'CFBundleShortVersionString|CFBundleVersion|CFBundleIdentifier'
shasum -a 256 "$app_path/Contents/MacOS/shackcq-desktop-nexus-review" "$zip_path" "$dmg_path"
stat -f '%N %z bytes %Sm' "$zip_path" "$dmg_path"
