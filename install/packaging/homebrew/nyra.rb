# Homebrew formula for Nyra, made from install/packaging/homebrew/. The release workflow fills in the version and
# the SHA-256 sums (install/packaging/fill.sh) and attaches the result to the release as nyra.rb; copy that file into a
# tap (a repository named homebrew-tap, as Formula/nyra.rb) to publish it:
#   brew install SpAkXD/tap/nyra
class Nyra < Formula
  desc "Compact, strictly-typed programming language designed to be written by AI agents"
  homepage "https://nyralang.dev"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/SpAkXD/nyra/releases/download/v{{VERSION}}/nyra-v{{VERSION}}-aarch64-apple-darwin.tar.gz"
      sha256 "{{SHA256_MACOS_ARM64}}"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/SpAkXD/nyra/releases/download/v{{VERSION}}/nyra-v{{VERSION}}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "{{SHA256_LINUX_X64}}"
    end
  end

  def install
    bin.install "nyra"
    doc.install "README.md", "docs"
    pkgshare.install "examples"
  end

  def caveats
    <<~EOS
      Native programs (nyra build, nyra run --native) need a C compiler: the Xcode Command Line
      Tools on macOS (xcode-select --install), gcc on Linux. The interpreter and --js need none.
    EOS
  end

  test do
    (testpath/"hello.nyra").write "print(\"hello\")\n"
    assert_equal "hello", shell_output("#{bin}/nyra run --interp #{testpath}/hello.nyra").strip
    assert_match version.to_s, shell_output("#{bin}/nyra --version")
  end
end
