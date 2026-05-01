class Rustgraph < Formula
  desc "Rust code navigation built for AiDX — AST-aware, MCP-native, token-efficient"
  homepage "https://github.com/ZmoleCristian/rustgraph"
  version "0.7.11"
  license "0BSD"

  on_macos do
    on_arm do
      url "https://github.com/ZmoleCristian/rustgraph/releases/download/v0.7.11/rustgraph-aarch64-apple-darwin.tar.gz"
      sha256 "51c8dad7ad2a03d1c215482609fd87c234aab7b765e1a321dd8019d767b1a000"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZmoleCristian/rustgraph/releases/download/v0.7.11/rustgraph-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "bae967d31ae0c4bef8d80322ee0498f2982d12e2a4d9008932de1e2b7a01317c"
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
