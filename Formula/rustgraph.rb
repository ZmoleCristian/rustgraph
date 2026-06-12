class Rustgraph < Formula
  desc "Rust code navigation built for AiDX — AST-aware, MCP-native, token-efficient"
  homepage "https://github.com/ZmoleCristian/rustgraph"
  version "0.8.1"
  license "0BSD"

  on_macos do
    on_arm do
      url "https://github.com/ZmoleCristian/rustgraph/releases/download/v0.8.1/rustgraph-aarch64-apple-darwin.tar.gz"
      sha256 "2728c79cc9fc22082ebff4071f23c3468ad34ed5b82e675f77c4589a4c289a40"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZmoleCristian/rustgraph/releases/download/v0.8.1/rustgraph-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "a5329f42a4ed95a245ab9aa32938466758f4655dbca1970f888c5cacbaa7a11a"
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
