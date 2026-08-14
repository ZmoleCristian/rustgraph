use std::process::{Command, Output};

fn run_rustgraph(args: &[&str]) -> Output {
    let bin = env!("CARGO_BIN_EXE_rustgraph");
    let mut full: Vec<&str> = vec!["--absolute-paths", "--no-auto-path"];
    full.extend_from_slice(args);
    Command::new(bin)
        .args(full)
        .output()
        .expect("run rustgraph")
}

fn help_for(sub: &str) -> String {
    let out = run_rustgraph(&[sub, "--help"]);
    assert!(
        out.status.success(),
        "{} --help should exit 0; stderr={}",
        sub,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn regression_help_subcommand_long_form_omits_global_descriptions() {
    let banned_phrases = ["Fuzzy threshold 0.0-1.0", "Crate root (auto-discovered"];

    for sub in ["callers", "ensemble", "find"] {
        let help = help_for(sub);

        let has_footer_marker =
            help.contains("Globals:") || help.contains("Globals (run `rustgraph --help`");
        assert!(
            has_footer_marker && help.contains("rustgraph --help"),
            "`{} --help` should carry a Globals footer that points users at `rustgraph --help`; got:\n{}",
            sub,
            help
        );

        for banned in &banned_phrases {
            assert!(
                !help.contains(banned),
                "`{} --help` still contains banned global description `{}` — those phrases belong only to the top-level `rustgraph --help` long_about. Full help:\n{}",
                sub,
                banned,
                help
            );
        }
    }
}

#[test]
fn regression_grep_help_documents_quoting() {
    let help = help_for("grep");

    assert!(
        help.contains("single-quote") || help.contains("'") && help.contains("double quote"),
        "grep --help should teach single-quoting the pattern; got:\n{}",
        help
    );

    assert!(
        (help.contains("\\(") && help.contains("\\)"))
            || help.contains("Escape regex parens")
            || help.contains("escape parens"),
        "grep --help should teach `\\(` / `\\)` for escaping parens in shell; got:\n{}",
        help
    );
}

#[test]
fn regression_depth_help_says_zero_is_unlimited() {
    let callers = help_for("callers");
    assert!(
        callers.contains("--depth"),
        "callers --help must list the --depth flag; got:\n{}",
        callers
    );

    assert_block_mentions(&callers, "--depth", "unlimited", 6);

    let paths = help_for("paths-between");
    assert!(
        paths.contains("--depth"),
        "paths-between --help must list --depth; got:\n{}",
        paths
    );
    assert_block_mentions(&paths, "--depth", "unlimited", 6);

    let cg = help_for("call-graph");
    assert!(
        cg.contains("--depth"),
        "call-graph --help must list --depth; got:\n{}",
        cg
    );
    assert_block_mentions(&cg, "--depth", "unlimited", 6);
}

#[test]
fn regression_find_help_explains_query_syntax() {
    let help = help_for("find");

    assert!(
        help.contains("|") && (help.contains("alternative") || help.contains("OR")),
        "find --help should explain the `name` / `<a>|<b>` query syntax; got:\n{}",
        help
    );

    let mentions_path_line = help.contains("path.rs:LINE")
        || help.contains("path:LINE")
        || help.contains("path/to/file.rs:LINE");
    assert!(
        mentions_path_line,
        "find --help should explicitly mention `path:LINE` (so users know it isn't supported here); got:\n{}",
        help
    );
    let redirects_elsewhere = help.contains("callers path") || help.contains("Read tool");
    assert!(
        redirects_elsewhere,
        "find --help should redirect users wanting path:LINE to `callers` or the Read tool; got:\n{}",
        help
    );
}

fn assert_block_mentions(help: &str, marker: &str, needle: &str, window: usize) {
    let lines: Vec<&str> = help.lines().collect();
    let marker_idx = lines
        .iter()
        .position(|l| l.contains(marker))
        .unwrap_or_else(|| panic!("marker `{}` not found in help:\n{}", marker, help));
    let end = (marker_idx + window).min(lines.len());
    let block = lines[marker_idx..end].join("\n");
    assert!(
        block.contains(needle),
        "expected `{}` within {} lines after the line containing `{}`; got block:\n{}\n--- full help ---\n{}",
        needle,
        window,
        marker,
        block,
        help
    );
}
