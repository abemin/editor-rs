#!/bin/bash
# Builds dist/<dmg_name>-<version>.dmg and its .sha256.
# Universal (Apple Silicon + Intel) when both Rust targets are installed.
# Signs and notarizes when these are set: APPLE_SIGNING_IDENTITY, APPLE_ID, APPLE_TEAM_ID, APPLE_APP_PASSWORD.
set -euo pipefail
cd "$(dirname "$0")/.."
value() { sed -n "s/^$1 *= *\"\(.*\)\"/\1/p" app.toml; }
name="$(value display_name)"; dmg_name="$(value dmg_name)"; version="$(value version)"
export MACOSX_DEPLOYMENT_TARGET=13.0

have_target() {
  if command -v rustup >/dev/null; then rustup target list --installed | grep -qx "$1"
  else [ -d "$(rustc --print sysroot)/lib/rustlib/$1" ]; fi
}

built=()
for target in aarch64-apple-darwin x86_64-apple-darwin; do
  if have_target "$target"; then
    scripts/bundle.sh release --target "$target"
    built+=("$(scripts/bundle.sh release --target "$target" --print-path)")
  else
    echo "note: $target is not installed, so this build will not include it" >&2
  fi
done
[ ${#built[@]} -gt 0 ] || { echo "no Rust target available" >&2; exit 1; }

stage="$(mktemp -d)"; trap 'rm -rf "$stage"' EXIT
app="$stage/$name.app"
ditto "${built[0]}" "$app"
if [ ${#built[@]} -gt 1 ]; then
  lipo -create "${built[@]/%//Contents/MacOS/EditorRS}" -output "$app/Contents/MacOS/EditorRS"
fi
xattr -cr "$app"

if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
  codesign --force --deep --options runtime --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$app"
  signed="Developer ID"
else
  codesign --force --deep --sign - "$app"
  signed="ad-hoc (unnotarized)"
fi

mkdir -p dist
dmg="dist/$dmg_name-$version.dmg"
rm -f "$dmg"
ln -s /Applications "$stage/Applications"
hdiutil create -quiet -volname "$name" -srcfolder "$stage" -ov -format UDZO "$dmg"

if [ -n "${APPLE_SIGNING_IDENTITY:-}" ] && [ -n "${APPLE_ID:-}" ]; then
  codesign --force --sign "$APPLE_SIGNING_IDENTITY" "$dmg"
  xcrun notarytool submit "$dmg" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD" --wait
  xcrun stapler staple "$dmg"
  signed="Developer ID, notarized"
fi

(cd dist && shasum -a 256 "$(basename "$dmg")" > "$(basename "$dmg").sha256")
echo "Built $dmg"
echo "Architectures: $(lipo -archs "$app/Contents/MacOS/EditorRS")"
echo "Signature: $signed"
