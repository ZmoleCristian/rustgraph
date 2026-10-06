#!/usr/bin/env python3
"""Collect LLVM coverage and render a production-only test map from the same source.

python3 tools/test_coverage.py -p ../crate -o target/coverage -- --test accounting
Requires cargo/rustc, matching llvm-profdata + llvm-cov, Graphviz, and built rustgraph.
Cargo flags after -- select the tests; CARGO_HOME and other Cargo environment are inherited.
"""

import argparse
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile


def run(args, **kwargs):
    return subprocess.run([str(arg) for arg in args], check=True, **kwargs)


def snapshot(root, output):
    # Match source discovery's ignore rules and avoid generated build/dependency trees.
    command = ["rg", "--files", "-g", "*.rs", "-g", "Cargo.toml", "-g", "Cargo.lock",
               "-g", "!target/**", "-g", "!**/.cargo-home/**"]
    if output.is_relative_to(root):
        command.extend(["-g", f"!{output.relative_to(root)}/**"])
    result = run([*command, "."],
                 cwd=root, capture_output=True, text=True)
    return {str((root / name).resolve()): (root / name).read_text()
            for name in sorted(result.stdout.splitlines())}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("-p", "--path", type=Path, required=True)
    parser.add_argument("-o", "--output-dir", type=Path, required=True)
    parser.add_argument("--rustgraph", type=Path, default=Path(__file__).resolve().parents[1] / "target/debug/rustgraph")
    parser.add_argument("cargo_args", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    root, out = args.path.resolve(), args.output_dir.resolve()
    extra = args.cargo_args
    if extra and extra[0] == "--":
        extra = extra[1:]
    if root == out:
        parser.error("use a separate output directory")
    # These modes do not execute tests and would produce an empty/misleading report.
    if any(arg in {"--no-run", "--list", "--message-format", "--target-dir"} or arg.startswith(("--message-format=", "--target-dir=")) for arg in extra):
        parser.error("pass test selectors, not --no-run/--list/--message-format/--target-dir")
    for tool in ["cargo", "rustc", "llvm-profdata", "llvm-cov", "dot", "rg"]:
        if not shutil.which(tool):
            parser.error(f"missing tool: {tool}")
    rust_version = run(["rustc", "-vV"], capture_output=True, text=True).stdout
    llvm_version = next(line.split(": ", 1)[1] for line in rust_version.splitlines() if line.startswith("LLVM version:"))
    for tool in ["llvm-cov", "llvm-profdata"]:
        version = run([tool, "--version"], capture_output=True, text=True).stdout
        if f"LLVM version {llvm_version}" not in version:
            parser.error(f"{tool} must match rustc's LLVM {llvm_version}")
    before = snapshot(root, out)
    out.mkdir(parents=True, exist_ok=True)
    # Both build artifacts and raw counters are fresh; previous runs cannot contaminate counts.
    session = Path(tempfile.mkdtemp(prefix="run-", dir=out))
    profiles = session / "profiles"
    profiles.mkdir()
    build_profiles = session / "build-profiles"
    build_profiles.mkdir()
    env = os.environ.copy()
    flags = env.get("CARGO_ENCODED_RUSTFLAGS")
    flags = flags.split("\x1f") if flags else shlex.split(env.get("RUSTFLAGS", ""))
    env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join([*flags, "-C", "instrument-coverage"])
    env.pop("RUSTFLAGS", None)
    env["CARGO_TARGET_DIR"] = str(session / "build")
    env["LLVM_PROFILE_FILE"] = str(build_profiles / "%p-%m.profraw")
    artifacts = session / "artifacts.jsonl"
    with artifacts.open("w") as output:
        run(["cargo", "test", "--locked", "--no-run", "--message-format=json", *extra], cwd=root, env=env, stdout=output)
    # Build-script execution is not test execution. Only merge profiles from this phase.
    env["LLVM_PROFILE_FILE"] = str(profiles / "%p-%m.profraw")
    with (session / "tests.log").open("w") as output:
        run(["cargo", "test", "--locked", *extra], cwd=root, env=env, stdout=output)
    after = snapshot(root, out)
    if before != after:
        raise RuntimeError("source files changed during compilation/tests; rerun on a stable source tree")
    executables = set()
    for line in artifacts.read_text().splitlines():
        try:
            item = json.loads(line)
        except json.JSONDecodeError:
            continue
        if item.get("reason") == "compiler-artifact" and item.get("executable"):
            executables.add(item["executable"])
    raw = sorted(profiles.glob("*.profraw"))
    if not raw or not executables:
        raise RuntimeError("no instrumented executables/profiles produced")
    profdata = session / "coverage.profdata"
    run(["llvm-profdata", "merge", "-sparse", *raw, "-o", profdata])
    objects = sorted(executables)
    command = ["llvm-cov", "export", objects[0], "-instr-profile", profdata]
    for obj in objects[1:]:
        command.extend(["-object", obj])
    export = session / "llvm.json"
    with export.open("w") as output:
        run(command, stdout=output)
    Path(str(export) + ".sources.json").write_text(json.dumps({"format": "rustgraph.coverage.sources.v1", "files": before}))
    run([args.rustgraph.resolve(), "-p", root, "test-coverage", "--llvm-coverage", export,
         "--dot", out / "coverage.dot", "-j", "-o", out / "coverage.json"])
    for fmt in ["png", "svg"]:
        run(["dot", f"-T{fmt}", out / "coverage.dot", "-o", out / f"coverage.{fmt}"])
    print(out / "coverage.png")


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        sys.exit(str(error))
