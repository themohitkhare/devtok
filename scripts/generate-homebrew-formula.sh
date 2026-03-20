#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/generate-homebrew-formula.sh <version> <repo> <sha256-arm64> <sha256-x64> <sha256-linux-x64>
EOF
}

if [[ $# -ne 5 ]]; then
  usage >&2
  exit 1
fi

version="$1"
repo="$2"
sha_arm64="$3"
sha_x64="$4"
sha_linux="$5"

cat <<EOF
class Acs < Formula
  desc "Auto Consulting Service CLI"
  homepage "https://github.com/${repo}"
  version "${version}"

  if OS.mac? && Hardware::CPU.arm?
    url "https://github.com/${repo}/releases/download/v#{version}/acs-#{version}-macos-arm64.tar.gz"
    sha256 "${sha_arm64}"
  elsif OS.mac? && Hardware::CPU.intel?
    url "https://github.com/${repo}/releases/download/v#{version}/acs-#{version}-macos-x64.tar.gz"
    sha256 "${sha_x64}"
  elsif OS.linux? && Hardware::CPU.intel?
    url "https://github.com/${repo}/releases/download/v#{version}/acs-#{version}-linux-x64.tar.gz"
    sha256 "${sha_linux}"
  else
    odie "ACS does not publish binaries for #{OS}/#{Hardware::CPU.arch}"
  end

  def install
    bin.install "acs"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/acs --version")
  end
end
EOF
