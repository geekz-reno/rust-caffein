#!/bin/bash
set -euo pipefail

if [[ "$(uname -s)" != Darwin ]]; then
    printf '%s\n' 'Building Caffeine.app requires macOS.' >&2
    exit 1
fi
if [[ $# -gt 1 ]]; then
    printf 'Usage: %s [aarch64-apple-darwin|x86_64-apple-darwin]\n' "$0" >&2
    exit 1
fi

root="$(cd "$(dirname "$0")/.." && pwd)"
target="${1:-${CARGO_BUILD_TARGET:-}}"
case "$target" in
    ''|aarch64-apple-darwin|x86_64-apple-darwin) ;;
    *) printf 'Unsupported macOS target: %s\n' "$target" >&2; exit 1 ;;
esac
target_dir="${CARGO_TARGET_DIR:-$root/target}"
case "$target_dir" in
    /*) ;;
    *) target_dir="$PWD/$target_dir" ;;
esac

export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}"
build_args=(build --manifest-path "$root/Cargo.toml" --locked --release --target-dir "$target_dir")
release_dir="$target_dir/release"
if [[ -n "$target" ]]; then
    build_args+=(--target "$target")
    release_dir="$target_dir/$target/release"
fi
cargo "${build_args[@]}"

work_dir="$(mktemp -d "$release_dir/.caffeine-bundle.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT
bundle="$work_dir/Caffeine.app"
contents="$bundle/Contents"
resources="$contents/Resources"
iconset="$work_dir/Caffeine.iconset"
mkdir -p "$contents/MacOS" "$resources" "$iconset"
cp "$release_dir/caffeine-rust" "$contents/MacOS/caffeine-rust"
cp "$root/resources/Info.plist" "$contents/Info.plist"
cp "$root/LICENSE" "$resources/LICENSE"
cp "$root/resources/active.png" "$root/resources/active@2x.png" \
    "$root/resources/inactive.png" "$root/resources/inactive@2x.png" "$resources/"
cp "$root/resources/Logo.pdf" "$resources/"
cp -R "$root"/resources/*.lproj "$resources/"

# Keep only the upstream PNG source in version control. Generate the icon here.
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$root/resources/Cup.png" \
        --out "$iconset/icon_${size}x${size}.png" >/dev/null
    retina_size=$((size * 2))
    sips -z "$retina_size" "$retina_size" "$root/resources/Cup.png" \
        --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$resources/Caffeine.icns"
codesign --force --sign - "$bundle"

app="$release_dir/Caffeine.app"
rm -rf "$app"
mv "$bundle" "$app"
printf 'Built %s\n' "$app"
