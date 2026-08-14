use serde_json::Value;
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

#[test]
fn regression_help_text_does_not_apologize_for_in_flag() {
    let banned = ["--in alias dropped", "haiku", "R14 confused"];

    for sub in ["callers", "ensemble", "grep", "refs"] {
        let out = run_rustgraph(&[sub, "--help"]);

        assert!(
            out.status.success(),
            "{} --help failed: stderr={}",
            sub,
            String::from_utf8_lossy(&out.stderr)
        );
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        for needle in &banned {
            assert!(
                !combined.contains(needle),
                "subcommand `{}` --help contains apologetic meta-narrative substring `{}`. \
                 Fix: delete the apology from the help text (or unify the --in flag globally). \
                 Full help:\n{}",
                sub,
                needle,
                combined
            );
        }
    }
}

#[test]
fn regression_in_flag_means_files_filter_consistently() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/keep/a.rs"),
        "pub fn target() {}\n",
    );
    write_file(
        &fixture.path().join("src/drop/b.rs"),
        "pub fn target() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let grep_out = run_rustgraph(&["grep", "target", "--in", "keep", "-p", &base, "-j"]);
    assert!(
        grep_out.status.success(),
        "grep with --in failed: stderr={}",
        String::from_utf8_lossy(&grep_out.stderr)
    );
    let json: Value = serde_json::from_slice(&grep_out.stdout).unwrap_or_else(|e| {
        panic!(
            "grep --in did not produce JSON: {}\nstdout={}\nstderr={}",
            e,
            String::from_utf8_lossy(&grep_out.stdout),
            String::from_utf8_lossy(&grep_out.stderr)
        )
    });
    let total = json["total_matches"]
        .as_u64()
        .expect("total_matches present");
    assert_eq!(
        total, 1,
        "grep --in keep should return exactly 1 hit (in src/keep/a.rs); got {} — payload={}",
        total, json
    );
    let matches = json["matches"].as_array().expect("matches array");
    let path = matches[0]["file_path"].as_str().unwrap_or("");
    assert!(
        path.contains("keep"),
        "expected match path to contain `keep`, got `{}`",
        path
    );

    let callers_out = run_rustgraph(&["callers", "target", "--in", "keep", "-p", &base]);
    assert!(
        !callers_out.status.success(),
        "expected callers --in to be rejected (alias dropped in R15), but it succeeded. \
         stdout={}\nstderr={}",
        String::from_utf8_lossy(&callers_out.stdout),
        String::from_utf8_lossy(&callers_out.stderr)
    );
    let stderr = String::from_utf8_lossy(&callers_out.stderr);
    assert!(
        stderr.contains("unexpected argument '--in'")
            || stderr.contains("unexpected argument \"--in\""),
        "expected stderr to mention `unexpected argument '--in'` (current asymmetric state); got: {}",
        stderr
    );
}

#[test]
fn regression_find_o_flag_writes_only_to_file_not_stdout() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), "pub fn target() {}\n");
    let base = fixture.path().to_string_lossy().to_string();
    let out_file = fixture.path().join("find_out.txt");
    let out_file_str = out_file.to_string_lossy().to_string();

    let res = run_rustgraph(&["find", "target", "-p", &base, "-o", &out_file_str]);
    assert!(
        res.status.success(),
        "find -o failed: stderr={}",
        String::from_utf8_lossy(&res.stderr)
    );

    assert!(
        out_file.exists(),
        "expected -o file `{}` to exist after `find -o`",
        out_file.display()
    );
    let written = fs::read_to_string(&out_file).expect("read -o file");
    assert!(
        written.contains("target"),
        "expected -o file to contain `target` in the find result; got:\n{}",
        written
    );

    let stdout = String::from_utf8_lossy(&res.stdout);
    assert!(
        stdout.trim().is_empty(),
        "find -o should write ONLY to the file, but stdout was non-empty:\n---STDOUT---\n{}\n------------",
        stdout
    );
}

#[test]
fn regression_callers_and_refs_o_flag_writes_only_to_file() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn target() {}\nfn caller_of_target() { target(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let callers_out_file = fixture.path().join("callers_out.txt");
    let callers_out_file_str = callers_out_file.to_string_lossy().to_string();
    let callers_res = run_rustgraph(&[
        "callers",
        "target",
        "-p",
        &base,
        "-o",
        &callers_out_file_str,
    ]);
    assert!(
        callers_res.status.success(),
        "callers -o failed: stderr={}",
        String::from_utf8_lossy(&callers_res.stderr)
    );
    assert!(
        callers_out_file.exists(),
        "expected -o file `{}` to exist after `callers -o`",
        callers_out_file.display()
    );
    let callers_written = fs::read_to_string(&callers_out_file).expect("read callers -o file");
    assert!(
        !callers_written.is_empty(),
        "expected callers -o file to be non-empty"
    );
    let callers_stdout = String::from_utf8_lossy(&callers_res.stdout);
    assert!(
        callers_stdout.trim().is_empty(),
        "callers -o should write ONLY to the file, but stdout was non-empty:\n---STDOUT---\n{}\n------------",
        callers_stdout
    );

    let refs_out_file = fixture.path().join("refs_out.txt");
    let refs_out_file_str = refs_out_file.to_string_lossy().to_string();
    let refs_res = run_rustgraph(&["refs", "target", "-p", &base, "-o", &refs_out_file_str]);
    assert!(
        refs_res.status.success(),
        "refs -o failed: stderr={}",
        String::from_utf8_lossy(&refs_res.stderr)
    );
    assert!(
        refs_out_file.exists(),
        "expected -o file `{}` to exist after `refs -o`",
        refs_out_file.display()
    );
    let refs_written = fs::read_to_string(&refs_out_file).expect("read refs -o file");
    assert!(
        !refs_written.is_empty(),
        "expected refs -o file to be non-empty"
    );
    let refs_stdout = String::from_utf8_lossy(&refs_res.stdout);
    assert!(
        refs_stdout.trim().is_empty(),
        "refs -o should write ONLY to the file, but stdout was non-empty:\n---STDOUT---\n{}\n------------",
        refs_stdout
    );
}
