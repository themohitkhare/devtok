#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/package-release.sh <target-name> <binary-path> <output-dir> [version]
EOF
}

if [[ $# -lt 3 || $# -gt 4 ]]; then
  usage >&2
  exit 1
fi

target_name="$1"
binary_path="$2"
output_dir="$3"
version="${4:-$(cargo metadata --no-deps --format-version 1 | sed -n 's/.*"version":"\([^"]*\)".*/\1/p' | head -n 1)}"

if [[ ! -f "$binary_path" ]]; then
  echo "binary not found: $binary_path" >&2
  exit 1
fi

mkdir -p "$output_dir"

staging_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$staging_dir"
}
trap cleanup EXIT

cp "$binary_path" "$staging_dir/acs"

if command -v strip >/dev/null 2>&1; then
  strip "$staging_dir/acs" || true
fi

if command -v upx >/dev/null 2>&1; then
  upx --best --lzma "$staging_dir/acs"
elif [[ "${ACS_REQUIRE_UPX:-0}" == "1" ]]; then
  echo "upx is required but not installed" >&2
  exit 1
else
  echo "warning: upx not found, shipping uncompressed binary" >&2
fi

archive_name="acs-${version}-${target_name}.tar.gz"
tar -C "$staging_dir" -czf "$output_dir/$archive_name" acs

if command -v shasum >/dev/null 2>&1; then
  shasum -a 256 "$output_dir/$archive_name"
elif command -v sha256sum >/dev/null 2>&1; then
  sha256sum "$output_dir/$archive_name"
fi
