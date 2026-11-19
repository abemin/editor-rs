#!/bin/bash
# Builds the app bundle: scripts/bundle.sh <debug|release> [--print-path] [--target <triple>]
set -euo pipefail
cd "$(dirname "$0")/.."
profile="${1:-debug}"; shift || true
print_only=0; target=""
while [ $# -gt 0 ]; do
  case "$1" in
    --print-path) print_only=1 ;;
    --target) target="$2"; shift ;;
  esac
  shift
done
name=$(sed -n 's/^display_name *= *"\(.*\)"/\1/p' app.toml)
dir="target/${target:+$target/}$profile"
app="$PWD/$dir/$name.app"
if [ $print_only -eq 1 ]; then echo "$app"; exit 0; fi
flags=(); [ "$profile" = release ] && flags+=(--release); [ -n "$target" ] && flags+=(--target "$target")
cargo build -q -p editor-app ${flags[@]+"${flags[@]}"}
rm -rf "$app"; mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$dir/EditorRS" "$app/Contents/MacOS/EditorRS"
plist=$(mktemp); "$dir/EditorRS" --print-info-plist > "$plist"; cp "$plist" "$app/Contents/Info.plist"; rm -f "$plist"
cp assets/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"
codesign --force --sign - "$app" 2>/dev/null
