# rustgraph

AST-aware Rust codebase navigation. CLI + library + MCP server.

```bash
cargo install rustgraph
```

```toml
[dependencies]
rustgraph = "0.1"
```

## What it does

Parses Rust source via `syn`, builds a symbol index + call graph, and exposes
it through 14 subcommands. AST-driven, so it doesn't false-positive on string
literals, comments, or unrelated tokens the way `grep` does.

```bash
rustgraph find <name>            # locate fn/struct/enum
rustgraph callers <fn>           # who calls this
rustgraph paths-between A B      # does A reach B, through what
rustgraph ensemble <fn>          # full context bundle (replaces 4-6 grep+read)
rustgraph slice <name>           # exact source of one symbol
rustgraph dead-code              # unreachable pub fns
rustgraph impls <Trait>          # types implementing a trait
rustgraph refs <ident>           # every reference (field/path/type/etc.)
# + def, members, usages, tree, grep, inventory, call-graph
```

Run `rustgraph --help` for the full list, `rustgraph <cmd> --help` for flags.

## MCP server (Claude / Codex / Gemini)

`rustgraph` ships an MCP server that exposes 5 of the most-used subcommands as
agent-callable tools (find, callers, ensemble, paths-between, slice). Self-
register with one command:

```bash
rustgraph mcp install            # detect installed clients + register all
rustgraph mcp status             # show registration state
rustgraph mcp uninstall          # remove from all configs
```

Detects and registers with `~/.claude.json`, `~/.codex/config.toml`, and
`~/.gemini/settings.json`. Atomic writes with timestamped backups.

## Don't trust me. Listen to our customers.

> "best Rust nav CLI I've used" — **Claude Opus**, ⭐ 9/10

> "killer feature: paths-between" — **Gemini 2.5 Pro**, ⭐ 8/10

> "killer feature: ensemble" — **Gemini 3.1 Pro Preview**, ⭐ 9/10

> "one shell call replaced grep → ctags → manual mapping" — anonymous LLM agent, post-task confession

### Hard numbers from 27 rounds of agent probing

- **6/6** LLM agents reach for `rustgraph` as their first tool call when MCP is registered
- **0/12** reached for it when only the skill was installed (skills don't work, MCP does)
- Avg agent rating across rounds: **8.0/10** (peak: 8.11/10)
- 1070 tests, 0 failed
- 411 documented public items
- ~5,800 implementation comments stripped because we control the stack and they were noise

## License

Dual-licensed under either MIT or Apache-2.0 at your option.
