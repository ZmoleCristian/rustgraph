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

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn unwrap_fixture() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub fn ok() {\n    let x = some.unwrap();\n    let y = other.unwrap();\n}\n",
    );
    dir
}

fn no_match_fixture() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub fn alpha() {}\npub fn beta() {}\n",
    );
    dir
}

#[test]
fn fix1_paren_warning_suppressed_when_matches_found() {
    let fixture = unwrap_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "grep", r"\.unwrap\(\)"]);
    assert!(
        out.status.success(),
        "grep should succeed; status={:?} stderr={}",
        out.status,
        stderr_of(&out)
    );

    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("unwrap()"),
        "expected literal `.unwrap()` matches in stdout; got:\n{stdout}"
    );
    assert!(
        stdout.contains("2 match"),
        "expected 2 matches in headline; got:\n{stdout}"
    );

    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("Rust regex uses bare"),
        "R25-W2: paren warning must NOT fire when matches are found (false-positive trap); got stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("note: pattern contains"),
        "R25-W2: paren note must NOT fire on successful match; got stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("\u{2501}\u{2501}"),
        "R25-W2: separator banner must not appear when matches present; got stderr:\n{stderr}"
    );
}

#[test]
fn fix1_paren_warning_fires_when_zero_matches() {
    let fixture = no_match_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "grep", r"\.\(broken\)"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));

    let stderr = stderr_of(&out);

    assert!(
        stderr.contains("\u{2501}\u{2501}\u{2501}\u{2501}"),
        "expected separator banner around \\(\\) warning; got stderr:\n{stderr}"
    );

    assert!(
        stderr.contains("note:") && stderr.contains("Rust regex uses bare"),
        "expected `note:` + `Rust regex uses bare (...)` text; got stderr:\n{stderr}"
    );

    assert!(
        stderr.contains("'\\.(broken)'"),
        "expected suggested `\\.(broken)` (only paren backslashes stripped) in stderr; got:\n{stderr}"
    );
}

#[test]
fn fix1_pipe_warning_still_fires_with_matches() {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        "// hits Foo|Bar marker text\npub fn ok() {}\n",
    );
    let base = dir.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "grep", r"Foo\|Bar"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));

    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("Foo|Bar"),
        "expected literal `Foo|Bar` to match; got stdout:\n{stdout}"
    );

    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("did you mean") && stderr.contains("Foo|Bar"),
        "R25-W2: `\\|` warning must keep firing even with matches (different defaults from `\\(\\)`); got stderr:\n{stderr}"
    );
}

#[test]
fn fix1_paren_warning_emitted_on_compile_fail() {
    let fixture = no_match_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "grep", "[invalid"]);
    assert!(
        !out.status.success(),
        "expected non-zero exit on invalid regex; status={:?}",
        out.status
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("invalid regex pattern"),
        "expected parse-error wrapper; got:\n{stderr}"
    );

    assert!(
        !stderr.contains("Rust regex uses bare"),
        "no paren escapes → no paren warning; got:\n{stderr}"
    );
}

#[test]
fn fix1_pipe_warning_keeps_loud_banner() {
    let dir = tempdir().expect("tempdir");
    write_file(&dir.path().join("src/lib.rs"), "pub fn alpha() {}\n");
    let base = dir.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "grep", r"a\|b"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("\u{2501}\u{2501}\u{2501}\u{2501}"),
        "expected R24-W3 LOUD banner around `\\|` warning; got stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("warning:") && stderr.contains("Rust regex"),
        "expected `warning:` + `Rust regex` tokens; got stderr:\n{stderr}"
    );
}

fn zorblax_sig_only_fixture() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub fn handler(s: &Zorblax) {}\npub fn other_helper() -> u32 { 0 }\n",
    );
    dir
}

fn no_zorblax_fixture() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub fn ok() {}\npub fn other() -> u32 { 0 }\n",
    );
    dir
}

#[test]
fn fix2_find_match_signature_returns_sig_hit_with_visibility_note() {
    let fixture = zorblax_sig_only_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "--match-signature", "find", "Zorblax"]);
    assert!(
        out.status.success(),
        "find --match-signature must succeed when sig tier finds hits; status={:?} stderr={}",
        out.status,
        stderr_of(&out)
    );

    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("handler") && stdout.contains("[sig]"),
        "expected sig hit `handler [sig]`; got stdout:\n{stdout}"
    );

    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("note:") && stderr.contains("0 name matches"),
        "expected `note: 0 name matches` visibility note; got stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("(--match-signature)"),
        "expected note to credit `(--match-signature)`; got stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("1 signature/path matches"),
        "expected `1 signature/path matches` count in note; got stderr:\n{stderr}"
    );
}

#[test]
fn fix2_find_match_signature_genuine_no_match_exits_one() {
    let fixture = no_zorblax_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "--match-signature", "find", "Zorblax"]);
    assert!(
        !out.status.success(),
        "expected exit 1 on genuine no-match (no name AND no sig/path hits); status={:?}",
        out.status
    );

    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("no match for query"),
        "expected `no match for query` error; got stderr:\n{stderr}"
    );

    assert!(
        !stderr.contains("0 name matches; showing"),
        "visibility note must NOT fire when 0 total hits; got stderr:\n{stderr}"
    );
}

#[test]
fn fix2_visibility_note_pins_all_sig_format() {
    let fixture = zorblax_sig_only_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "-p",
        &base,
        "--match-signature",
        "find",
        "Zorblax",
        "--func",
    ]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("note: 0 name matches; showing"),
        "all-sig note format pinned: `note: 0 name matches; showing N signature/path matches`; got:\n{stderr}"
    );
    assert!(
        stderr.contains("(--match-signature)"),
        "label `(--match-signature)` (R26-W2: dropped `fallback` word); got:\n{stderr}"
    );
}

#[test]
fn fix2_visibility_note_mixed_emits_additive_breakdown() {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub struct Zorblax;\npub fn handler(s: &Zorblax) {}\npub fn other() {}\n",
    );
    let base = dir.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "--match-signature", "find", "Zorblax"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));

    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("[name]") && stdout.contains("[sig]"),
        "expected both [name] (struct) and [sig] (fn) tags in mixed result set; got stdout:\n{stdout}"
    );

    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("1 name + 1 sig/path matches"),
        "R26-W2 additive: mixed name+sig must emit `1 name + 1 sig/path matches` note; got stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("(--match-signature)"),
        "R26-W2 additive note must credit --match-signature; got stderr:\n{stderr}"
    );

    assert!(
        !stderr.contains("0 name matches; showing"),
        "OLD-format `0 name matches; showing` must not fire when name hits exist; got stderr:\n{stderr}"
    );
}

#[test]
fn fix2_visibility_note_suppressed_in_default_mode() {
    let fixture = zorblax_sig_only_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "find", "Zorblax"]);
    assert!(
        !out.status.success(),
        "expected exit 1 without --match-signature (no name match); status={:?}",
        out.status
    );

    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("0 name matches; showing"),
        "visibility note keyed on --match-signature; default mode stays quiet; got stderr:\n{stderr}"
    );
}

#[test]
fn fix2_sig_tag_still_renders_alongside_visibility_note() {
    let fixture = zorblax_sig_only_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "--match-signature", "find", "Zorblax"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stdout = stdout_of(&out);
    let stderr = stderr_of(&out);
    assert!(
        stdout.contains("[sig]"),
        "R22-W1 [sig] tag must still render per-hit; got stdout:\n{stdout}"
    );
    assert!(
        stderr.contains("0 name matches; showing"),
        "R25-W2 visibility note must fire alongside; got stderr:\n{stderr}"
    );
}
