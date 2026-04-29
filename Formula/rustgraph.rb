class Rustgraph < Formula
  desc "Rust code navigation built for AiDX — AST-aware, MCP-native, token-efficient"
  homepage "https://github.com/ZmoleCristian/rustgraph"
  version "0.7.8"
  license "0BSD"

  on_macos do
    on_arm do
      url "https://github.com/ZmoleCristian/rustgraph/releases/download/v0.7.8/rustgraph-aarch64-apple-darwin.tar.gz"
      sha256 "14bd41c339d2a408046e3ec4a228fa38263415fe72f5b3438cca68893f5188e4"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZmoleCristian/rustgraph/releases/download/v0.7.8/rustgraph-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "bc4a910f2c8c761e0095d095253520e942b64769db40dd3d82af25f14f63fffe"
    end
  end

  def install
    bin.install "rustgraph"
    man1.install "man/rustgraph.1" if File.exist?("man/rustgraph.1")
  end

  def caveats
    <<~EOS
      Register the MCP server with Claude / Codex / Gemini:
        rustgraph mcp install

      List:      rustgraph mcp list
      Uninstall: rustgraph mcp uninstall
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/rustgraph --version")
  end
end
