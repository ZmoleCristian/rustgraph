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
    Command::new(bin)
        .args(full)
        .output()
        .expect("run rustgraph")
}

fn line_count(s: &str) -> usize {
    s.lines().count()
}

#[test]
fn regression_ensemble_default_view_is_summary_not_full() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub struct Request { pub id: u32, pub payload: String, pub kind: u8 }
pub struct Response { pub status: u32, pub body: String }
pub struct Session { pub user_id: u32, pub created_at: u64 }
pub struct Event { pub kind: u32, pub payload: String }

pub fn build_request(id: u32) -> Request {
    Request { id, payload: String::new(), kind: 0 }
}

pub fn build_session(user_id: u32) -> Session {
    Session { user_id, created_at: 0 }
}

pub fn fan_in(req: Request, sess: Session) -> Event {
    let _ = req.id; let _ = sess.user_id;
    Event { kind: 1, payload: req.payload }
}

pub fn fan_out(ev: Event) -> Response {
    Response { status: 200, body: ev.payload }
}

pub fn pipeline(id: u32) -> Response {
    let req = build_request(id);
    let sess = build_session(id);
    let ev = fan_in(req, sess);
    let resp = fan_out(ev);
    resp
}

pub fn caller_a() { let _ = pipeline(1); }
pub fn caller_b() { let _ = pipeline(2); }
pub fn caller_c() { let _ = pipeline(3); }
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();

    let default_out = run_rustgraph(&["--path", &base, "ensemble", "pipeline"]);
    assert!(
        default_out.status.success(),
        "default ensemble failed: {}",
        String::from_utf8_lossy(&default_out.stderr)
    );
    let default_text = String::from_utf8_lossy(&default_out.stdout);
    let default_lines = line_count(&default_text);

    let full_out = run_rustgraph(&["--path", &base, "ensemble", "pipeline", "--view", "full"]);
    assert!(
        full_out.status.success(),
        "full ensemble failed: {}",
        String::from_utf8_lossy(&full_out.stderr)
    );
    let full_text = String::from_utf8_lossy(&full_out.stdout);
    let full_lines = line_count(&full_text);

    assert!(
        default_lines < 100,
        "default ensemble view should be tight (< 100 lines), got {} lines:\n{}",
        default_lines,
        default_text
    );

    assert!(
        full_lines > default_lines,
        "full view must be larger than summary (default={} lines, full={} lines)",
        default_lines,
        full_lines
    );
    let delta = full_lines.saturating_sub(default_lines);
    assert!(
        delta >= 5,
        "full view should add a meaningful chunk over summary (>= 5 lines); got delta={} (default={}, full={})",
        delta,
        default_lines,
        full_lines
    );
}

#[test]
fn regression_impls_zero_implementors_distinguishes_from_unknown() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub trait MyTrait {
    fn behave(&self);
}

pub struct WithoutImpl;
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "impls", "MyTrait"]);
    assert!(
        out.status.success(),
        "impls failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let lower = combined.to_lowercase();
    assert!(
        lower.contains("0 implementors") || lower.contains("no types implement"),
        "expected '0 implementors'-style message for an unimplemented trait, got:\n{}",
        combined
    );
    assert!(
        !lower.contains("check the trait name spelling"),
        "spelling hint should be suppressed when trait IS referenced; got:\n{}",
        combined
    );
}

#[test]
fn regression_impls_unknown_trait_says_check_spelling() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub struct Foo;
pub fn helper() {}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "impls", "XyzTotallyMadeUp"]);
    assert!(
        out.status.success(),
        "impls failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let lower = combined.to_lowercase();
    assert!(
        lower.contains("spelling") || lower.contains("fully-qualified"),
        "expected spelling/fully-qualified hint for an unknown trait, got:\n{}",
        combined
    );
}

#[test]
fn regression_grep_invalid_regex_message_includes_hint() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), "pub fn x() {}\n");
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "grep", ".expect("]);
    assert!(
        !out.status.success(),
        "expected non-zero exit for invalid regex, got success"
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let lower = combined.to_lowercase();
    assert!(
        lower.contains("escape")
            || lower.contains("fixed-string")
            || lower.contains("regex metacharacter"),
        "expected hint about escaping / fixed-string / regex metacharacters, got:\n{}",
        combined
    );
}

fn dead_code_fixture(path: &Path) {
    write_file(
        &path.join("src/lib.rs"),
        r#"
pub mod submod;
pub use submod::reexported_helper;

pub fn used_publicly() { submod::called_internally(); }

pub fn dead_top_level() {}
"#,
    );
    write_file(
        &path.join("src/submod.rs"),
        r#"
pub fn reexported_helper() {}

pub fn called_internally() {}

pub fn dead_in_submod() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_runs() {
        let _ = called_internally();
    }
}
"#,
    );
}

#[test]
fn regression_dead_code_default_omits_skip_summary() {
    let fixture = tempdir().expect("tempdir");
    dead_code_fixture(fixture.path());
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "dead-code"]);
    assert!(
        out.status.success(),
        "dead-code failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("skipped as re-exported"),
        "default dead-code output should hide skip-reason summary; got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("skipped inside cfg(test)"),
        "default dead-code output should hide cfg(test) skip line; got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("skipped as runtime entrypoints"),
        "default dead-code output should hide runtime entrypoints skip line; got:\n{}",
        stdout
    );

    assert!(
        stdout.contains("Summary:"),
        "default output should still emit a Summary line; got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("candidates"),
        "default output should still emit candidate count; got:\n{}",
        stdout
    );
}

#[test]
fn regression_dead_code_verbose_includes_skip_summary() {
    let fixture = tempdir().expect("tempdir");
    dead_code_fixture(fixture.path());
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "dead-code", "--verbose"]);
    assert!(
        out.status.success(),
        "dead-code --verbose failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("skipped as re-exported"),
        "--verbose should re-enable skip-reason summary; got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("skipped inside cfg(test)"),
        "--verbose should re-enable cfg(test) skip line; got:\n{}",
        stdout
    );
}

#[test]
fn regression_refs_by_function_tags_test_fns() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn something() -> u32 { 42 }

pub fn prod_caller() -> u32 {
    something()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thing() {
        let _ = something();
    }
}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "refs", "something", "--by-function"]);
    assert!(
        out.status.success(),
        "refs --by-function failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);

    let mut saw_test_tagged = false;
    let mut saw_prod_untagged = false;
    for line in stdout.lines() {
        if line.contains("test_thing") && line.contains("[test]") {
            saw_test_tagged = true;
        }
        if line.contains("prod_caller") && !line.contains("[test]") {
            saw_prod_untagged = true;
        }
    }
    assert!(
        saw_test_tagged,
        "expected `test_thing [test]` in by-function rollup; got:\n{}",
        stdout
    );
    assert!(
        saw_prod_untagged,
        "expected `prod_caller` rollup WITHOUT [test] tag; got:\n{}",
        stdout
    );
}
