#!/usr/bin/env python3
"""Evaluate `rustgraph stringly` over a directory containing many Rust projects.

The harness is intentionally dependency-free. It discovers outermost Cargo
roots (so workspace members are not scanned twice), runs independent rustgraph
processes with bounded concurrency/timeouts, and writes one reproducible JSON
report containing both aggregate metrics and the original findings.
"""

from __future__ import annotations

import argparse
import collections
import concurrent.futures
import dataclasses
import json
import os
import re
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path
from typing import Any, Iterable


EXCLUDED_DIRECTORIES = {
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
}


@dataclasses.dataclass(frozen=True)
class Project:
    root: Path
    rust_files: int


@dataclasses.dataclass(frozen=True)
class RunConfig:
    binary: Path
    corpus_root: Path
    min_confidence: float
    exclude_tests: bool
    include_suppressed: bool
    timeout_seconds: float


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("corpus", type=Path, help="Directory containing Rust projects")
    parser.add_argument(
        "--rustgraph",
        type=Path,
        default=Path("target/release/rustgraph"),
        help="rustgraph binary (default: target/release/rustgraph)",
    )
    parser.add_argument(
        "--min-confidence",
        type=float,
        default=0.70,
        help="Detector threshold; 0.70 captures every current signal",
    )
    parser.add_argument(
        "--workers",
        type=int,
        default=min(4, os.cpu_count() or 1),
        help="Concurrent rustgraph processes (default: min(4, CPU count))",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=120.0,
        help="Per-project timeout in seconds (default: 120)",
    )
    parser.add_argument(
        "--exclude-tests",
        action="store_true",
        help="Exclude test declarations from detector output",
    )
    parser.add_argument(
        "--include-suppressed",
        action="store_true",
        help="Include normally withheld findings so their caveats can be audited",
    )
    parser.add_argument(
        "--all-manifests",
        action="store_true",
        help="Scan every Cargo.toml instead of only outermost roots",
    )
    parser.add_argument(
        "--match",
        type=re.compile,
        help="Only scan project roots matching this regular expression",
    )
    parser.add_argument(
        "--sample",
        type=int,
        help="Evenly sample N projects by Rust-file count",
    )
    parser.add_argument("--output", type=Path, help="Write the complete JSON report here")
    args = parser.parse_args()
    if not 0.0 <= args.min_confidence <= 1.0:
        parser.error("--min-confidence must be in [0,1]")
    if args.workers < 1:
        parser.error("--workers must be at least 1")
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    if args.sample is not None and args.sample < 1:
        parser.error("--sample must be at least 1")
    return args


def walk_files(root: Path, wanted_name: str | None = None) -> Iterable[Path]:
    for directory, child_directories, files in os.walk(root, followlinks=False):
        child_directories[:] = [
            name for name in child_directories if name not in EXCLUDED_DIRECTORIES
        ]
        for name in files:
            if wanted_name is None or name == wanted_name:
                yield Path(directory, name)


def discover_roots(corpus_root: Path, all_manifests: bool) -> list[Path]:
    manifests = sorted(
        walk_files(corpus_root, "Cargo.toml"),
        key=lambda path: (len(path.parts), str(path)),
    )
    if all_manifests:
        return [manifest.parent for manifest in manifests]

    accepted: set[Path] = set()
    roots: list[Path] = []
    for manifest in manifests:
        root = manifest.parent
        if any(parent in accepted for parent in root.parents):
            continue
        accepted.add(root)
        roots.append(root)
    return roots


def count_rust_files(root: Path) -> int:
    return sum(1 for path in walk_files(root) if path.suffix == ".rs")


def evenly_sample(projects: list[Project], sample_size: int | None) -> list[Project]:
    if sample_size is None or sample_size >= len(projects):
        return projects
    ordered = sorted(projects, key=lambda project: (project.rust_files, str(project.root)))
    if sample_size == 1:
        return [ordered[len(ordered) // 2]]
    indices = {
        round(position * (len(ordered) - 1) / (sample_size - 1))
        for position in range(sample_size)
    }
    return [ordered[index] for index in sorted(indices)]


def relative_root(project: Project, corpus_root: Path) -> str:
    try:
        return project.root.relative_to(corpus_root).as_posix()
    except ValueError:
        return project.root.as_posix()


def run_project(project: Project, config: RunConfig) -> dict[str, Any]:
    command = [
        str(config.binary),
        "--no-auto-path",
        "--path",
        str(project.root),
    ]
    if config.exclude_tests:
        command.append("--exclude-tests")
    command.extend(
        [
            "--json",
            "stringly",
            "--min-confidence",
            str(config.min_confidence),
            "--max-results",
            "0",
        ]
    )
    if config.include_suppressed:
        command.append("--include-suppressed")
    started = time.monotonic()
    base = {
        "root": relative_root(project, config.corpus_root),
        "rust_files": project.rust_files,
    }
    try:
        completed = subprocess.run(
            command,
            check=False,
            capture_output=True,
            text=True,
            timeout=config.timeout_seconds,
        )
    except subprocess.TimeoutExpired as error:
        return {
            **base,
            "status": "timeout",
            "elapsed_ms": round((time.monotonic() - started) * 1000),
            "error": f"timed out after {config.timeout_seconds:g}s",
            "stderr": (error.stderr or "")[-4000:],
        }
    except OSError as error:
        return {
            **base,
            "status": "spawn-error",
            "elapsed_ms": round((time.monotonic() - started) * 1000),
            "error": str(error),
        }

    elapsed_ms = round((time.monotonic() - started) * 1000)
    if completed.returncode != 0:
        return {
            **base,
            "status": "command-error",
            "elapsed_ms": elapsed_ms,
            "exit_code": completed.returncode,
            "stderr": completed.stderr[-4000:],
        }
    try:
        payload = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        return {
            **base,
            "status": "invalid-json",
            "elapsed_ms": elapsed_ms,
            "error": str(error),
            "stdout": completed.stdout[-4000:],
            "stderr": completed.stderr[-4000:],
        }
    return {
        **base,
        "status": "ok",
        "elapsed_ms": elapsed_ms,
        "scanned_string_sites": payload.get("scanned_string_sites", 0),
        "suppressed_findings": payload.get("suppressed_findings", 0),
        "suppression_counts": payload.get("suppression_counts", {}),
        "total_findings": payload.get("total_findings", 0),
        "origin_counts": payload.get("origin_counts", {}),
        "findings": payload.get("findings", []),
        "stderr": completed.stderr[-4000:] if completed.stderr else "",
    }


def confidence_bucket(confidence: float) -> str:
    if confidence >= 0.95:
        return "0.95-1.00"
    if confidence >= 0.85:
        return "0.85-0.95"
    if confidence >= 0.80:
        return "0.80-0.85"
    if confidence >= 0.70:
        return "0.70-0.80"
    return "below-0.70"


def summarize(results: list[dict[str, Any]], elapsed_ms: int) -> dict[str, Any]:
    statuses: collections.Counter[str] = collections.Counter()
    origins: collections.Counter[str] = collections.Counter()
    confidence: collections.Counter[str] = collections.Counter()
    evidence: collections.Counter[str] = collections.Counter()
    caveats: collections.Counter[str] = collections.Counter()
    suppression_reasons: collections.Counter[str] = collections.Counter()
    site_kinds: collections.Counter[str] = collections.Counter()
    suggestions: collections.Counter[str] = collections.Counter()
    total_sites = 0
    total_findings = 0
    total_suppressed = 0
    strong_findings = 0
    projects_with_findings = 0
    projects_with_actionable_findings = 0
    actionable_findings = 0
    withheld_findings_in_payload = 0
    literal_findings = 0
    repeated_literal_findings = 0
    one_literal_per_observation = 0
    literal_observations = 0
    distinct_literals = 0
    project_timings: list[tuple[int, str, int, int]] = []

    for result in results:
        statuses[result["status"]] += 1
        if result["status"] != "ok":
            continue
        sites = int(result.get("scanned_string_sites", 0))
        findings = result.get("findings", [])
        total_sites += sites
        total_suppressed += int(result.get("suppressed_findings", 0))
        suppression_reasons.update(result.get("suppression_counts", {}))
        total_findings += len(findings)
        projects_with_findings += bool(findings)
        project_has_actionable = False
        project_timings.append(
            (result["elapsed_ms"], result["root"], result["rust_files"], sites)
        )
        for finding in findings:
            withheld = any(
                bool(item.get("suppresses_by_default"))
                for item in finding.get("caveats", [])
            )
            withheld_findings_in_payload += withheld
            actionable_findings += not withheld
            project_has_actionable |= not withheld
            score = float(finding.get("confidence", 0.0))
            strong_findings += score >= 0.85
            confidence[confidence_bucket(score)] += 1
            suggestion = finding.get("suggestion", {})
            origins[str(suggestion.get("origin", "unknown"))] += 1
            suggestions[str(suggestion.get("canonical_path", "unknown"))] += 1
            site_kinds[str(finding.get("site_kind", "unknown"))] += 1
            for item in finding.get("evidence", []):
                evidence[str(item.get("kind", "unknown"))] += 1
            for item in finding.get("caveats", []):
                caveats[str(item.get("kind", "unknown"))] += 1
            stats = finding.get("literal_stats")
            if isinstance(stats, dict):
                literal_findings += 1
                observations = int(stats.get("observations", 0))
                distinct = int(stats.get("distinct", 0))
                literal_observations += observations
                distinct_literals += distinct
                repeated_literal_findings += observations > distinct
                one_literal_per_observation += observations == distinct
        projects_with_actionable_findings += project_has_actionable

    project_timings.sort(reverse=True)
    return {
        "projects": len(results),
        "statuses": dict(sorted(statuses.items())),
        "projects_with_findings": projects_with_findings,
        "projects_with_actionable_findings": projects_with_actionable_findings,
        "rust_files": sum(result.get("rust_files", 0) for result in results),
        "scanned_string_sites": total_sites,
        "findings": total_findings,
        "actionable_findings": actionable_findings,
        "withheld_findings_in_payload": withheld_findings_in_payload,
        "suppressed_findings": total_suppressed,
        "strong_findings_at_0.85": strong_findings,
        "findings_per_1000_sites": (
            round(total_findings * 1000 / total_sites, 2) if total_sites else 0.0
        ),
        "elapsed_ms": elapsed_ms,
        "origins": dict(sorted(origins.items())),
        "confidence_buckets": dict(sorted(confidence.items())),
        "site_kinds": dict(sorted(site_kinds.items())),
        "evidence_kinds": dict(sorted(evidence.items())),
        "caveat_kinds": dict(sorted(caveats.items())),
        "suppression_reasons": dict(sorted(suppression_reasons.items())),
        "literal_evidence": {
            "findings": literal_findings,
            "repeated_findings": repeated_literal_findings,
            "one_literal_per_observation": one_literal_per_observation,
            "distinct_literals": distinct_literals,
            "observations": literal_observations,
        },
        "top_suggestions": suggestions.most_common(30),
        "slowest_projects": [
            {
                "root": root,
                "elapsed_ms": duration,
                "rust_files": rust_files,
                "scanned_string_sites": sites,
            }
            for duration, root, rust_files, sites in project_timings[:20]
        ],
    }


def atomic_write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{path.name}.", suffix=".tmp", dir=path.parent
    )
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            json.dump(payload, handle, ensure_ascii=False, indent=2, sort_keys=True)
            handle.write("\n")
        os.replace(temporary_name, path)
    except BaseException:
        try:
            os.unlink(temporary_name)
        except FileNotFoundError:
            pass
        raise


def main() -> int:
    args = parse_args()
    corpus_root = args.corpus.expanduser().resolve()
    binary = args.rustgraph.expanduser().resolve()
    if not corpus_root.is_dir():
        print(f"error: corpus directory does not exist: {corpus_root}", file=sys.stderr)
        return 2
    if not binary.is_file():
        print(f"error: rustgraph binary does not exist: {binary}", file=sys.stderr)
        return 2

    roots = discover_roots(corpus_root, args.all_manifests)
    if args.match is not None:
        roots = [root for root in roots if args.match.search(str(root))]
    projects = [Project(root, count_rust_files(root)) for root in roots]
    projects = evenly_sample(projects, args.sample)
    projects.sort(key=lambda project: str(project.root))
    print(
        f"discovered {len(projects)} project(s), "
        f"{sum(project.rust_files for project in projects)} Rust file(s)",
        file=sys.stderr,
        flush=True,
    )

    config = RunConfig(
        binary=binary,
        corpus_root=corpus_root,
        min_confidence=args.min_confidence,
        exclude_tests=args.exclude_tests,
        include_suppressed=args.include_suppressed,
        timeout_seconds=args.timeout,
    )
    started = time.monotonic()
    completed_count = 0
    progress_lock = threading.Lock()
    results: list[dict[str, Any]] = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as executor:
        future_to_project = {
            executor.submit(run_project, project, config): project for project in projects
        }
        for future in concurrent.futures.as_completed(future_to_project):
            result = future.result()
            results.append(result)
            with progress_lock:
                completed_count += 1
                print(
                    f"[{completed_count:>3}/{len(projects)}] "
                    f"{result['status']:<13} {result['elapsed_ms']:>7} ms "
                    f"{result['root']} ({result.get('total_findings', 0)} findings)",
                    file=sys.stderr,
                    flush=True,
                )

    elapsed_ms = round((time.monotonic() - started) * 1000)
    results.sort(key=lambda result: result["root"])
    summary = summarize(results, elapsed_ms)
    report = {
        "schema_version": 3,
        "configuration": {
            "corpus": str(corpus_root),
            "rustgraph": str(binary),
            "min_confidence": args.min_confidence,
            "exclude_tests": args.exclude_tests,
            "include_suppressed": args.include_suppressed,
            "all_manifests": args.all_manifests,
            "workers": args.workers,
            "timeout_seconds": args.timeout,
        },
        "summary": summary,
        "projects": results,
    }
    if args.output is not None:
        output = args.output.expanduser().resolve()
        atomic_write_json(output, report)
        print(f"wrote {output}", file=sys.stderr)
    print(json.dumps(summary, ensure_ascii=False, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
