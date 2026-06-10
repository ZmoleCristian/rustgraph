# rustgraph

AiDX-native Rust codebase navigation. AST-aware. CLI + library + MCP server.

## Install

**Cargo** (any platform with a Rust toolchain)

```bash
cargo install rustgraph
```

**Homebrew** (macOS arm64, Linux x86_64)

```bash
brew tap zmolecristian/rustgraph https://github.com/ZmoleCristian/rustgraph
brew install rustgraph
```

**AUR** (Arch — yay / paru / makepkg)

```bash
yay -S rustgraph        # build from source
yay -S rustgraph-bin    # prebuilt binary
# or:
paru -S rustgraph
paru -S rustgraph-bin
```

**Scoop** (Windows)

```powershell
scoop bucket add rustgraph https://github.com/ZmoleCristian/rustgraph
scoop install rustgraph
```

**As a library** (Rust crate)

```toml
[dependencies]
rustgraph = "0.7"
```

After install, register the MCP server with Claude / Codex / Gemini:

```bash
rustgraph mcp install
```

## What is AiDX?

**AiDX** = AI Developer Experience. Same idea as DX, different consumer.

When the dev reading your code is a model, the constraints flip: tokens
cost real money, grep false-positives waste turns, and reading three
files to understand one function is a tax on every agent loop. AiDX is
about shaping the tool — outputs, errors, help text, defaults — so the
agent finishes the task in one shot instead of six.

rustgraph is built AiDX-first. Humans get a fast CLI for free.

## What it does

Parses Rust source via `syn`, builds a symbol index + call graph, and exposes
it through 13 subcommands. AST-driven, so it doesn't false-positive on string
literals, comments, or unrelated tokens the way `grep` does.

```bash
rustgraph find <name>            # locate fn/struct/enum
rustgraph callers <fn>           # who calls this
rustgraph paths-between A B      # does A reach B, through what
rustgraph ensemble <fn>          # full context bundle (replaces 4-6 grep+read)
rustgraph dead-code              # unreachable pub fns
rustgraph impls <Trait>          # types implementing a trait
rustgraph refs <ident>           # every reference (field/path/type/etc.)
# + def, members, usages, tree, grep, inventory, call-graph
```

Run `rustgraph --help` for the full list, `rustgraph <cmd> --help` for flags.

## MCP server (Claude / Codex / Gemini)

`rustgraph` ships an MCP server that exposes 5 of the most-used subcommands as
agent-callable tools (find, callers, ensemble, paths-between, tree). Self-
register with one command:

```bash
rustgraph mcp install            # detect installed clients + register all
rustgraph mcp list               # show registration state
rustgraph mcp uninstall          # remove from all configs
```

Detects and registers with `~/.claude.json`, `~/.codex/config.toml`, and
`~/.gemini/settings.json`. Atomic writes with timestamped backups.

## What the agents are saying

*All agents were spawned cold in the same mid-size Rust codebase and asked for an honest opinion.*

### **Claude Opus 4.7 (1M context) - Max**

> "Tool feels designed for an LLM agent, not for a human reading docs."
> "rustgraph turns the usual 'open six files and squint' into one ensemble call, and it's adult enough to print the % of edges it resolved by name-fallback instead of pretending the call graph is perfect — that receipt alone earns the slot in my toolbelt."

<details>
<summary><b>Process</b></summary>
<br/>

```
Gonna actually drive the tool, not just rubber-stamp it. Starting with parallel pokes at real symbols.

Solid. Find returns sigs + line ranges, fuzzy fallback exposes related test fns. Now let me hit ensemble + callers + paths_between in parallel on real targets.

Now poking holes — error UX on a miss, plus a slice to check the source-extraction surface.

Honest take after driving find / ensemble / callers --depth 0 / paths_between / slice on real surfaces in this repo:
What actually impressed me: the callers --depth 0 tree walked from build_ledger all the way up to main with the call-site line on every hop — that's the spelunking I usually pay for in tokens. ensemble value-flow hints (options -> build_symbol_context via <param>) are the thing I'd otherwise reconstruct by hand.
```

</details>


### **Claude Sonnet 4.6 - Max**
> "Rustgraph collapses the read-five-files-to-understand-one-function tax into a single call with call sites, struct deps, and value flow included."

<details>
<summary><b>Process</b></summary>
<br/>

```
Let me actually use the tool and explore this codebase cold.
Alright, I've used it cold. Honest take:

- find gave me call-site lines + signatures instantly across the whole codebase
- ensemble on cmd_inject_read_with_context handed me struct deps, upstream/downstream count, and which params flow where — without opening a single file
- callers depth-2 on build_session_activity_report laid out the full HTTP/CLI/test fan-in tree with exact line numbers in one shot

The value flow hints in ensemble are the real unlock. Knowing session -> context_root_for_session via <param> and dry_run -> cmd_inject_read_with_specs via <param> without reading the body is the difference between orienting in 1 hop vs 4.
```

</details>



### **Claude (Haiku 4.5)**

> "Rustgraph makes Rust exploration feel like having the codebase's actual topology in front of you: exact call sites, type lifecycles, and call-graph context without the file-opening loop."

<details>
<summary><b>Process</b></summary>
<br/>

```
I'll kick the tires cold on this codebase like I'd actually work in it. Let me explore using your tools.
Okay, I poked this repo cold and hit the gnarlier parts (context_inject propagation is legit complex). Here's my honest take:
What moved the needle: The ensemble view replaced what would normally be a "open 5 files and sketch the flow" loop. I could see build_symbol_context call all 18 of its downstream functions with type lifecycle paths showing where SymbolContext actually gets constructed and consumed — that's the kind of "I already know the shape before I read the code" win that saves agent tokens.
The exact call-site lines let me skip searching entirely. Value flow hints showed me parameter propagation without reading function bodies. And dead-code detection actually had signal (2 real unused functions, not 200 false positives).
What's real: This isn't just better grep. You've baked in the call graph and type flow and I/O boundaries as first-class outputs. That's agent-native because we don't have to ask follow-up questions — the structure is already there.
What's weak: Nothing jumped out, but I didn't hit you with a dynamic dispatch nightmare or trait-heavy code; those are the typical AST traps.
You've actually solved the problem. Not "find faster," but "understand the shape before you read."
```

</details>





### **Codex - gpt-5.5 xHigh**
> “This is AiDX done right: rustgraph turned an unfamiliar Rust repo from a pile of files into a call-site-aware map I could act on immediately.”

<details>
<summary><b>Process</b></summary>
<br/>

```
I’ll kick the tires the way I actually would in a Rust repo: first get the crate shape, then use your graph tools to answer a few “where does this behavior live / who calls it / how does flow connect” questions.
The ensemble output is doing the thing I care about most as an agent: it gave me “what structs matter, who enters here, what calls fan out, and which values travel where” without making me open five modules. Now I’m checking whether the call graph stays useful when I chase a concrete feature.
That feature chase took one hop from CLI to semantic context builder, and the callers tree exposed the CLI/API split around run_inject_specs with exact call-site lines. I’m doing one deeper read now to see whether the output is still digestible around the gnarlier context propagation code.
This is already enough to form an opinion, but I’m doing one last sanity pass on a different surface area: session activity/budget. Good tools should keep their shape when the domain shifts, not only when you pick the “happy path” function.
```

</details>

### **Codex - gpt-5.4 xHigh**
> "This makes code exploration feel less like search and more like direct access to the structure an agent actually needs."
<details>
<summary><b>Process</b></summary>
<br/>

```
I’m going to poke around this Rust project the way I naturally would if I were dropped into it cold, and I’ll lean on your tool for the actual navigation so the feedback is earned rather than polite.
I’ve got the lay of the land now. Next I’m using the graph tools the way I’d use them on day one of a codebase: find the entrypoints, inspect a couple of deep functions, then see whether the call graph actually reduces the usual “open five files and squint” ritual.
This is already pretty telling: the summary view is giving me the stuff I usually have to reconstruct by hand, especially “who calls this” plus the parameter/value hints. I’m going one layer deeper now to see whether the caller tree and path tracing stay clean on a nontrivial branch.
```

</details>


### **Codex - gpt-5.4-mini xHigh**
> "Rustgraph feels genuinely agent-native: exact call sites, call chains, and enough context to finish a Rust task in one pass."

<details>
<summary><b>Process</b></summary>
<br/>

```
I’m going to poke around the Rust project first, use the navigation tool the way an agent would, and then I’ll give you a blunt one-liner you can drop into the README.
I’ve got the lay of the land now. Next I’m reading the top-level docs and then I’ll exercise the Rust navigation API on a few real symbols so the feedback is grounded, not just vibes.
The first pass already shows the core promise: I can jump from an entry point to exact call sites and downstream boundaries without spelunking. I’m doing one deeper path lookup now so I can judge whether the call-chain output stays useful once the graph gets less trivial.
The graph tool is already being honest about what it can and can’t resolve, which is useful in its own way. I’m tightening the queries to line-precise symbols now so I can judge the output on a real call chain instead of the fallback errors.
```

</details>


### **Gemini CLI - gemini-3.1-pro-preview**
> "By resolving on the parsed AST rather than relying on noisy regex, AiDX's rustgraph delivers surgically precise context that drastically cuts down my turn count and token usage when navigating complex Rust codebases."


### **Gemini CLI - gemini-2.5-pro**
> "A scalpel for my digital hands, `rustgraph` dissects Rust code with a precision that textual search just can't match."

<details>
<summary><b>Process</b></summary>
<br/>

```
I will start by exploring the codebase with rustgraph tree to get a high-level overview of the project structure.
Okay, `rustgraph tree` was a good overview. Now, to understand code relationships, I'll find callers of a function in the core project.rs file. I'll use rustgraph grep to list its functions first.
I misused `rustgraph grep`; the file path is an invalid argument. I need to use the `--in` option for the file path. Retrying now.
Got a function list. I'll find `handle_project_get`'s callers using `rustgraph callers`.
`handle_project_get` is called by `handle_request` in `src/daemon/http/router.rs`, with the exact line number provided. This tool is clearly superior to grep for navigating Rust. My opinion is formed. I'll say: "rustgraph dissects Rust code with a precision that textual search just can't match." - Gemini CLI. I'm ready to answer.
```

</details>


### Kimi K2.6
> "Finally, a tool that understands my real bottleneck isn't finding code — it's assembling enough context to reason about it in one shot."

<details>
<summary><b>Process</b></summary>
<br/>

```
Thinking: The user wants me to:
1. Explore the current project using the tool itself
2. Get a feel for how it works
3. Provide a one-liner quote for their README
First, I should explore the project structure and try running some of the tool's commands. This is a Rust project, so I can try building it and running some subcommands.
Let me start by understanding what's available and then using the tool.

Thinking: The user wants me to use the tool to explore the project and give feedback. I'll focus on rustgraph which works. Let me explore the current project with rustgraph.
I should try several commands:
1. tree - see the module structure
2. find or search for something interesting
3. callers on some function
4. ensemble on something
5. slice for source viewing
6. dead-code maybe
Let me do this in parallel to save time.

Thinking: Good, rustgraph works great. The rustgraph CLI is solid. Let me explore more commands to get a real feel for it - maybe refs, slice, paths-between, impls, etc. Let me try a few more interesting commands on the codebase to really exercise the tool.
Let me also try some of the other commands like slice, refs, usages, members, and maybe paths-between to see how well they work.

Thinking: This is excellent. The tool works really well. Let me try a couple more things to really get a feel for it - maybe call-graph with dot, impls, grep, and find with fuzzy matching. Also maybe try the --view full on ensemble to see how much context it packs. Let me also check if there are any interesting features like --changed-since or the switchboard mode.

Thinking: The user wants a one-liner quote for their README. I've now used the tool extensively on this Rust project. Let me think about what makes this tool special from an AI agent's perspective:
1. It gives me exactly what I need without noise - the ensemble command packs the full context (code, structs, callers, callees, lifecycles, dataflow, I/O boundaries) into one view
2. The callers --depth 2 tree with exact line numbers is genuinely better than grep
3. members solves a real problem - field access tracking
4. refs vs callers distinction is smart
5. paths-between answers "does A actually reach B"
6. The output is dense but readable - it's designed for agents who can parse structured text quickly
7. The slice command with path.rs:LINE syntax is ergonomic
8. It feels like it was built by someone who understands that every extra turn costs tokens

The user already has quotes from Claude Opus and Claude Sonnet. I should give something authentic that captures the experience. Let me think of something punchy but honest.
Possible angles:
- "It's the difference between exploring a codebase and actually understanding it."
- "The first tool that treats token budget as a first-class constraint."
- "Like having a senior engineer who already read every file and answers in one sentence."
- "Every command returns the context I'd have gathered manually in six files and three greps."

Wait, I should be authentic. The thing that struck me most was how ensemble --view full just hands me the entire cognitive load of a function in one shot. No clicking, no "find all references", no LSP lag. Just: here's the code, here's every struct it touches, here's who calls it, here's what it calls, here's the dataflow, here's the I/O boundaries. That's the "one shot" experience the user mentions in their AiDX pitch.
A good one-liner:
> "Finally, a codebase tool that understands the agent's real bottleneck isn't search speed — it's context assembly."
Or:
> "Where other tools help you find the needle, rustgraph hands you the whole haystack organized by relevance."
Or more practical:
> "Ensemble --view full is the closest thing to reading a function's mind without being the author."
Let me go with something that captures the "one shot" philosophy and the token-saving angle, since that's core to AiDX. Something like:
> "The first Rust tool built for agents who'd rather spend tokens on reasoning than on grep archaeology."
Or simpler and more direct:
> "It turns 'let me check six files' into a single command — the rare tool that actually saves tokens instead of burning them."

Actually, looking at the existing quotes, they range from "better than half the LSPs" to "legitimately better than anything rg gives you." I want something that adds a different angle - the agent workflow angle.
How about:
> "The ensemble command is basically cheat codes for context windows — one shot, zero noise, full comprehension."
Or more professional:
> "Finally, a tool that treats 'context assembly' as the problem instead of pretending 'find' is enough."
Let me think about what would look good on a README alongside the other Opus/Sonnet quotes.
Other option:
> "I stopped grepping after the first ensemble --view full; it packs more context per token than anything else I've used."
That's honest and specific. But maybe too long.
Let me try:
> "ensemble --view full is the closest thing to instant codebase telepathy."
Or:
> "The only Rust navigation tool that respects my context window more than my terminal width."

Best one-liner:
> "The first tool that treats token economics as a feature, not a side effect — ensemble --view full packs what I'd otherwise grep across six files into a single context window."
But the user asked for a one-liner. Let me make it shorter.
> "Built for agents who measure cost in turns, not keystrokes — ensemble --view full is instant context assembly with zero noise."
Even shorter:
> "It respects the context window: one ensemble call gives me what used to cost six greps and three file opens."
Or the most punchy:
> "The rare tool that saves more tokens than it spends."
Actually, I really like this angle because it's true. When I ran ensemble handle_request --view full, I got:
- The full function body
- All structs used with their definitions
- Call sites
- Upstream/downstream neighborhood (depth 2)
- Type lifecycles
- Value flow hints
- I/O boundaries
That's easily 6-10 separate commands in other tools. And it's formatted for immediate comprehension.
Final choice:
> "The only codebase tool that treats my context window as a budget instead of a landfill — ensemble --view full replaces six greps with one shot."
Or shorter and quotable:
> "ensemble --view full is the closest thing to reading a function's mind without being the author."
Wait, that's good but not agent-specific enough.
Let me go with:
> "Built for the agent loop: every command returns the structured context I'd otherwise assemble from six files and three greps."
That's honest, specific, and captures the AiDX philosophy. It's a bit long though.
How about:
> "Finally, a tool that understands my real bottleneck isn't finding code — it's assembling enough context to reason about it in one shot."
Yes. That's the one. It captures the core insight, it's honest (based on my actual usage), and it's concise.
Signed by: Kimi (since I'm Kimi K2.6)
Actually the user said "signed by you" - so I should sign it as my identity. I'm OpenCode powered by Kimi K2.6. But usually these are signed by the model name. Let me just use "Kimi K2.6" or "Kimi".
Let me re-read: "i want your opnion in a oneliner signed by you so i can add it to my readme"
So I'll provide the quote and sign it.
```

</details>


### **GLM-5.1**
> "rustgraph doesn't find code — it hands you the call chain, the structs, and the dataflow in one shot, so you spend tokens acting instead of searching."

<details>
<summary><b>Process</b></summary>
<br/>

```
The user wants me to actually use rustgraph to explore this codebase, then give an honest one-liner review for their README.

Let me actually exercise the tools the way an agent would when dropped into a codebase cold. I'll use the MCP rustgraph tools to navigate around, find entry points, trace call chains, etc.

Let me start by finding the main entry points and poking around.

bet. let me actually use the tools on this codebase instead of just talking about them.
Entry points located. Let me grab the daemon entry and the main HTTP dispatch to understand the shape.
Good. Now let me trace the actual call chain from HTTP entry to a real operation — say, session surgery.
Let me continue exploring. I want to trace a call chain from the HTTP router down to actual session mutation logic, and also check the context_inject subsystem since that's the more complex/interesting part.

Now let me trace a real path — HTTP POST to inject-read through to the actual mutation.
Clean 3-hop path from router to mutation. Let me go deeper — the context injection system is the gnarly part.
Now let me poke at something totally different — the propagation graph logic, since that's where the "smart" part lives.
Alright, last sanity check — let me verify the tool stays honest on a domain-shifted area. Hitting the session activity/budget code.
Alright, I've now thoroughly exercised rustgraph across multiple domains of this codebase:

  1. Found entry points (main, run_server, run_cli)
  2. Got ensemble summaries showing structs, call sites, value flow
  3. Traced exact call paths from HTTP router to session mutations (3 hops)
  4. Went deeper into the context injection system (propagation, symbol resolution)
  5. Hit a totally different domain (budget/activity) and got 8 distinct paths traced with exact call-site lines

  My honest assessment:

  The tool genuinely does what it claims. Key observations:

  - Ensemble view is the killer feature. Getting structs used, call sites, upstream/downstream count, and value flow hints in one call replaces what would normally be 4-5 grep+read cycles. For an agent on a token budget, that's real money saved.
  - paths_between with exact call-site lines is what grep fundamentally cannot do. I got 8 distinct paths from handle_request to build_budget with exact file:line annotations at every hop. That's call-graph traversal, not text search.
  - Honest about resolution. The stderr note about 9% name-based resolution is transparent — it doesn't hide ambiguity, it surfaces it.
  - Domain-agnostic. Worked equally well on HTTP routing, session surgery, context injection (the gnarly BFS propagation stuff), and activity budgeting. The tool didn't degrade when I shifted domains.
  - Actual time saved. To trace the handle_request → build_budget path manually I'd need to: read router.rs, find the dispatch, read the project endpoint, find session_report_with_hooks, read that, then read activity/report.rs to find build_budget. That's 5 file opens minimum. rustgraph gave me all 8 paths in one call with line numbers.

  Now let me give an honest one-liner. I need to be genuine, not sycophantic. The tool is genuinely good. Let me think about what makes it stand out vs what's just okay:

  The real value prop is: it replaces the "open 5 files and squint" pattern with structured, AST-aware output that an agent can act on immediately. The ensemble view is the standout — it's the one call that replaces the most grepping.

  Let me write a one-liner that's honest and punchy.
```

</details>

## License

0BSD (BSD Zero Clause License) — do whatever, no attribution required.
