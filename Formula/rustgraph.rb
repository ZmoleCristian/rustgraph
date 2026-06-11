class Rustgraph < Formula
  desc "Rust code navigation built for AiDX — AST-aware, MCP-native, token-efficient"
  homepage "https://github.com/ZmoleCristian/rustgraph"
  version "0.8.0"
  license "0BSD"

  on_macos do
    on_arm do
      url "https://github.com/ZmoleCristian/rustgraph/releases/download/v0.8.0/rustgraph-aarch64-apple-darwin.tar.gz"
      sha256 "af267bb0c6f3c01e30463fcaa6bedd42b363138cebcafbef71b1dd6539870651"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ZmoleCristian/rustgraph/releases/download/v0.8.0/rustgraph-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "5731f88411fb21670ac2e64922d1357316772c9d37b9885d0978a519e3f9bfb2"
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
