

use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::tempdir;

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(path, content).expect("write fixture file");
}

fn run_rustgraph(args: &[&str]) -> Output {
    let bin = env!("CARGO_BIN_EXE_rustgraph");
    let mut full: Vec<&str> = vec!["--absolute-paths", "--no-auto-path"];
    full.extend_from_slice(args);
    Command::new(bin).args(full).output().expect("run rustgraph")
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn small_fixture() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        r#"
pub fn r20_callee_fn(x: u32) -> u32 { x + 1 }

pub fn r20_target_fn(x: u32) -> u32 {
    let y = r20_callee_fn(x);
    y + 2
}

pub fn r20_caller_fn() {
    let _ = r20_target_fn(99);
}
"#,
    );
    dir
}


#[test]
fn regression_ensemble_summary_default_shows_truncation_footer() {
    let fixture = small_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "ensemble", "r20_target_fn"]);
    assert!(
        out.status.success(),
        "default ensemble failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    let lower = stdout.to_lowercase();
    assert!(
        lower.contains("summary view") || lower.contains("sections hidden"),
        "default Summary view should emit the truncation marker; got:\n{}",
        stdout
    );
}


#[test]
fn regression_ensemble_view_full_suppresses_truncation_footer() {
    let fixture = small_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "-p", &base, "ensemble", "r20_target_fn", "--view", "full",
    ]);
    assert!(
        out.status.success(),
        "--view full ensemble failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        !stdout.contains("summary view"),
        "expected NO 'summary view' marker under --view full; got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("sections hidden"),
        "expected NO 'sections hidden' text under --view full; got:\n{}",
        stdout
    );
}


#[test]
fn regression_ensemble_section_override_suppresses_truncation_footer() {
    let fixture = small_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "-p", &base, "ensemble", "r20_target_fn",
        "--section", "dataflow",
        "--section", "neighborhood",
    ]);
    assert!(
        out.status.success(),
        "ensemble with --section failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        !stdout.contains("sections hidden"),
        "expected NO 'sections hidden' text when --section is in effect; got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("summary view"),
        "expected NO 'summary view' label when --section is in effect; got:\n{}",
        stdout
    );

    let lower = stdout.to_lowercase();
    assert!(
        lower.contains("showing:") && lower.contains("dataflow") && lower.contains("neighborhood"),
        "expected `[showing: dataflow, neighborhood]` hint at top under --section; got:\n{}",
        stdout
    );
}


#[test]
fn regression_ensemble_footer_only_references_documented_flags() {
    let fixture = small_fixture();
    let base = fixture.path().to_string_lossy().to_string();


    let help = run_rustgraph(&["ensemble", "--help"]);
    assert!(
        help.status.success(),
        "ensemble --help failed: {}",
        stderr_of(&help)
    );
    let help_text = stdout_of(&help);


    let run = run_rustgraph(&["-p", &base, "ensemble", "r20_target_fn"]);
    assert!(
        run.status.success(),
        "default ensemble failed: {}",
        stderr_of(&run)
    );
    let run_text = stdout_of(&run);


    let footer_start = run_text
        .find("[summary view")
        .expect("expected the summary-view footer block in default output");
    let footer_end = run_text[footer_start..]
        .find(']')
        .expect("expected the footer block to be terminated by `]`");
    let footer = &run_text[footer_start..footer_start + footer_end + 1];


    let flag_tokens: Vec<&str> = footer
        .split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_'))
        .filter(|t| t.starts_with("--") && t.len() > 2)
        .collect();
    assert!(
        !flag_tokens.is_empty(),
        "expected at least one `--flag` token in the footer; got footer:\n{}",
        footer
    );

    for tok in &flag_tokens {


        let needle_with_space = format!("{} ", tok);
        let needle_with_newline = format!("{}\n", tok);
        let needle_with_eq = format!("{}=", tok);
        let found = help_text.contains(&needle_with_space)
            || help_text.contains(&needle_with_newline)
            || help_text.contains(&needle_with_eq);
        assert!(
            found,
            "footer references `{}` which is NOT documented in `ensemble --help`. Footer:\n{}\n--- help output (truncated) ---\n{}",
            tok,
            footer,
            &help_text[..help_text.len().min(2000)]
        );
    }
}
