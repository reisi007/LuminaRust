#!/usr/bin/env python3
"""Fail when Ratchet's exact-file exceptions do not match the live population."""

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CONFIG = "ratchet.config.json"
BASELINE = "ratchet.baseline.json"
RULE = "rust-file-line-ceiling"
CEILING = 1000
POPULATION = ["crates/**/*.rs", "agent-harness/**/*.rs"]


def validate(root: Path = ROOT) -> list[str]:
    config = json.loads((root / CONFIG).read_text())
    rule = config["rules"][RULE]
    errors: list[str] = []
    if rule.get("count", {}).get("file_metrics", {}).get("files") != POPULATION:
        errors.append("file_metrics population must cover all .rs under crates/ and agent-harness/")
    if rule.get("count", {}).get("file_metrics", {}).get("metric") != "line_count":
        errors.append("file_metrics metric must be line_count")
    if rule.get("aggregate") != "max" or rule.get("direction") != "ceiling":
        errors.append("rule must use aggregate=max and direction=ceiling")
    scopes = rule.get("scopes", [])
    configured: dict[str, str] = {}
    for scope in scopes:
        path = scope.get("file")
        if not path or "files" in scope or path in configured:
            errors.append(f"exception scope must name one unique exact file: {scope.get('id')}")
            continue
        configured[path] = scope.get("id", "")
        source = root / path
        if not source.is_file():
            errors.append(f"stale exception for missing file: {path}")
        elif line_count(source) <= CEILING:
            errors.append(f"stale exception at or below {CEILING} lines: {path}")
    actual = {}
    for folder in ("crates", "agent-harness"):
        base = root / folder
        if base.exists():
            actual.update({f.relative_to(root).as_posix(): line_count(f)
                           for f in base.rglob("*.rs") if line_count(f) > CEILING})
    for path in sorted(actual.keys() - configured.keys()):
        errors.append(f"missing exact-file exception for oversized Rust file: {path} ({actual[path]} lines)")
    for path in sorted(configured.keys() - actual.keys()):
        if (root / path).is_file():
            errors.append(f"stale exception at or below {CEILING} lines: {path}")
    baseline_path = root / BASELINE
    if baseline_path.exists():
        data = json.loads(baseline_path.read_text())["rules"][RULE]
        if data.get("value") != CEILING:
            errors.append(f"global baseline must be {CEILING}")
        values = data.get("scopes", {})
        if set(values) != set(configured.values()):
            errors.append("baseline exact-file scopes do not match configured exceptions")
        for path, scope_id in configured.items():
            if scope_id in values:
                scope_baseline = values[scope_id]
                if "reason" in scope_baseline:
                    errors.append(
                        f"exception baseline must not contain a reason (reasoned increases are forbidden): {path}"
                    )
                if scope_baseline.get("value") != actual.get(path):
                    errors.append(f"exception baseline must equal measured current line count: {path}")
    return errors


def line_count(path: Path) -> int:
    return len(path.read_bytes().splitlines())


if __name__ == "__main__":
    problems = validate()
    if problems:
        print("Ratchet exception validation failed:", file=sys.stderr)
        for problem in problems:
            print(f"- {problem}", file=sys.stderr)
        raise SystemExit(1)
    print("Ratchet 1000-line population and exact-file exceptions are current.")
