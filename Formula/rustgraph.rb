class Rustgraph < Formula
  desc "Rust code navigation built for AiDX — AST-aware, MCP-native, token-efficient"
  homepage "https://github.com/ZmoleCristian/rustgraph"
  version "0.7.10"
  license "0BSD"

  on_macos do
    on_arm do
      url "https://github.com/ZmoleCristian/rustgraph/releases/download/v0.7.10/rustgraph-aarch64-apple-darwin.tar.gz"
      sha256 "57e732687f9bcf7a593159ba0b989c100777c0553cc1d5b0408c3969b2204f19"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZmoleCristian/rustgraph/releases/download/v0.7.10/rustgraph-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "cc4a35baa4b0e99a7b777ecfcac80fb96b260a9080ec345e4f12f26dfa1800ff"
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
