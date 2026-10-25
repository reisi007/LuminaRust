#!/usr/bin/env python3
"""Shared source-file population for Rust line-count gates and doc facts."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Iterator


RULE = "rust-file-line-ceiling"
POPULATION_GLOBS = ["crates/**/*.rs", "agent-harness/**/*.rs"]
TARGET_EXCLUDE_GLOB = "**/target/**"


def source_measurements(root: Path) -> Iterator[tuple[str, int]]:
    """Yield (repo-relative path, physical line count) for the source population."""
    root = root.resolve()
    paths: dict[str, Path] = {}
    for pattern in POPULATION_GLOBS:
        for path in root.glob(pattern):
            if not path.is_file():
                continue
            relative = path.relative_to(root)
            # Ignore directories named exactly `target`, regardless of Git's
            # ignore rules. A source file named `target.rs` remains in scope.
            if "target" in relative.parts[:-1]:
                continue
            paths[relative.as_posix()] = path

    for relative, path in sorted(paths.items()):
        yield relative, len(path.read_bytes().splitlines())


def effective_ratchet_config(config_path: Path) -> dict:
    """Load Ratchet config with the source-population exclusion applied."""
    config = json.loads(config_path.read_text())
    rule = config["rules"][RULE]
    files = rule.get("count", {}).get("file_metrics", {}).get("files")
    if files != POPULATION_GLOBS:
        raise ValueError(
            "file_metrics population must cover all .rs under crates/ and agent-harness/"
        )

    excludes = rule.get("exclude", [])
    if not isinstance(excludes, list) or any(not isinstance(item, str) for item in excludes):
        raise ValueError("Ratchet rule exclusions must be a list of path globs")
    if TARGET_EXCLUDE_GLOB not in excludes:
        rule["exclude"] = [*excludes, TARGET_EXCLUDE_GLOB]
    return config


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--jsonl", action="store_true", help="emit one JSON measurement per source file")
    mode.add_argument("--ratchet-config", type=Path, help="source Ratchet config to materialize")
    parser.add_argument("--output", type=Path, help="effective Ratchet config output path")
    args = parser.parse_args()

    if args.jsonl:
        for path, count in source_measurements(args.root):
            print(json.dumps({"path": path, "line_count": count}, ensure_ascii=False))
        return

    if args.output is None:
        parser.error("--output is required with --ratchet-config")
    effective = effective_ratchet_config(args.ratchet_config)
    args.output.write_text(json.dumps(effective, indent=2) + "\n")


if __name__ == "__main__":
    main()
