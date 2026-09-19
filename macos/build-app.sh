#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "The macOS app must be built on macOS." >&2
  exit 1
fi

repo="$(cd "$(dirname "$0")/.." && pwd)"
output="${1:-$repo/dist/Resopt.app}"
profile="${RESOPT_PROFILE:-release}"
if [[ "$profile" != "release" && "$profile" != "debug" ]]; then
  echo "RESOPT_PROFILE must be release or debug" >&2
  exit 1
fi
if [[ -e "$output" ]]; then
  echo "Output already exists: $output" >&2
  exit 1
fi
target_dir="${CARGO_TARGET_DIR:-$repo/target}"

cargo_args=(build --locked --bin resopt)
swift_args=(-parse-as-library -target "$(uname -m)-apple-macosx13.0")
if [[ "$profile" == "release" ]]; then
  cargo_args+=(--release)
  swift_args+=(-O)
else
  swift_args+=(-g)
fi

cd "$repo"
cargo "${cargo_args[@]}"

app="$output"
mkdir -p "$(dirname "$app")"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$repo/macos/Info.plist" "$app/Contents/Info.plist"
cp "$target_dir/$profile/resopt" "$app/Contents/Resources/resopt"
swiftc "${swift_args[@]}" "$repo/macos/ResoptApp.swift" -o "$app/Contents/MacOS/Resopt"
codesign --force --sign - "$app/Contents/Resources/resopt"
codesign --force --sign - "$app"
echo "Built $app"
