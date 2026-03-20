class Acs < Formula
  desc "Auto Consulting Service CLI"
  homepage "https://github.com/themohitkhare/devtok"
  version "0.1.0"

  if OS.mac? && Hardware::CPU.arm?
    url "https://github.com/themohitkhare/devtok/releases/download/v#{version}/acs-#{version}-macos-arm64.tar.gz"
    sha256 :no_check
  elsif OS.mac? && Hardware::CPU.intel?
    url "https://github.com/themohitkhare/devtok/releases/download/v#{version}/acs-#{version}-macos-x64.tar.gz"
    sha256 :no_check
  elsif OS.linux? && Hardware::CPU.intel?
    url "https://github.com/themohitkhare/devtok/releases/download/v#{version}/acs-#{version}-linux-x64.tar.gz"
    sha256 :no_check
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
