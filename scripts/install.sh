#!/bin/sh
set -eu

REPO="${ACS_REPOSITORY:-themohitkhare/devtok}"
INSTALL_DIR="${ACS_INSTALL_DIR:-/usr/local/bin}"
BIN_PATH="${INSTALL_DIR}/acs"
API_BASE="${ACS_GITHUB_API_BASE:-https://api.github.com}"

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

need_cmd curl
need_cmd tar
need_cmd mktemp

os="$(uname -s)"
arch="$(uname -m)"

case "${os}:${arch}" in
  Darwin:arm64) target="macos-arm64" ;;
  Darwin:x86_64) target="macos-x64" ;;
  Linux:x86_64) target="linux-x64" ;;
  *)
    echo "unsupported platform: ${os}/${arch}" >&2
    exit 1
    ;;
esac

release_json="$(curl -fsSL \
  -H 'Accept: application/vnd.github+json' \
  -H 'User-Agent: acs-install-script' \
  "${API_BASE%/}/repos/${REPO}/releases/latest")"

tag="$(printf '%s' "$release_json" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
if [ -z "$tag" ]; then
  echo "failed to resolve latest release tag from GitHub API" >&2
  exit 1
fi

version="${tag#v}"
asset="acs-${version}-${target}.tar.gz"
download_url="https://github.com/${REPO}/releases/download/${tag}/${asset}"

tmpdir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT INT TERM

archive_path="${tmpdir}/${asset}"
curl -fsSL "$download_url" -o "$archive_path"
tar -xzf "$archive_path" -C "$tmpdir"

mkdir -p "$INSTALL_DIR"
install -m 755 "${tmpdir}/acs" "$BIN_PATH"

echo "installed acs ${version} to ${BIN_PATH}"
