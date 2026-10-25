#!/usr/bin/env python3
"""Focused executable contract tests for the 1000-line Ratchet migration."""

from __future__ import annotations

import json
import importlib.util
import os
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from rust_source_population import POPULATION_GLOBS, effective_ratchet_config


RATCHET = os.environ.get("RATCHET_BIN", "ratchet")
RULE = "rust-file-line-ceiling"
EXCEPTION = "crates/legacy.rs"


def run(args: list[str], cwd: Path, ok: bool = True, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(args, cwd=cwd, text=True, capture_output=True, env=env)
    if (result.returncode == 0) != ok:
        raise AssertionError(f"{args} returned {result.returncode}\n{result.stdout}\n{result.stderr}")
    return result


class CeilingContract(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        (self.root / "ratchet.config.json").write_text(json.dumps({
            "tiers": {"fast": {"floor": 1}},
            "rules": {RULE: {
                "class": "oversized source",
                "incident": "prevention: no unexcepted Rust file may exceed 1000 lines",
                "unit": "lines",
                "direction": "ceiling",
                "aggregate": "max",
                "count": {"file_metrics": {
                    "files": POPULATION_GLOBS,
                    "metric": "line_count",
                }},
                "scopes": [{"id": "legacy", "description": "fixture exception", "file": EXCEPTION}],
            }},
        }))
        self.source(EXCEPTION, 1001)
        self.source("crates/ordinary.rs", 10)
        (self.root / "ratchet.baseline.json").write_text(json.dumps({
            "version": 1,
            "rules": {RULE: {"value": 1000, "scopes": {"legacy": {"value": 1001}}}},
        }))
        (self.root / ".baseline.committed").write_bytes((self.root / "ratchet.baseline.json").read_bytes())
        fakebin = self.root / "fakebin"
        fakebin.mkdir()
        git = fakebin / "git"
        git.write_text("#!/bin/sh\ncase \"$1\" in\n  rev-parse) pwd ;;\n  show) cat .baseline.committed ;;\n  *) exit 1 ;;\nesac\n")
        git.chmod(git.stat().st_mode | stat.S_IXUSR)
        self.env = {**os.environ, "PATH": f"{fakebin}:{os.environ['PATH']}"}

    def tearDown(self) -> None:
        self.temp.cleanup()

    def source(self, path: str, lines: int) -> Path:
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text("x\n" * lines)
        return target

    def ratchet(self, ok: bool) -> subprocess.CompletedProcess[str]:
        effective = self.root / ".ratchet-effective-config.json"
        effective.write_text(json.dumps(effective_ratchet_config(self.root / "ratchet.config.json")))
        try:
            return run([
                RATCHET, "check", "--config", str(effective), "--no-config",
                "--rule", RULE, "--no-update-check",
            ], self.root, ok, self.env)
        finally:
            effective.unlink(missing_ok=True)

    def test_existing_exception_overrun_fails(self) -> None:
        self.source(EXCEPTION, 1002)
        self.ratchet(False)

    def test_populated_exact_exception_passes(self) -> None:
        self.ratchet(True)

    def test_unexcepted_oversize_fails(self) -> None:
        self.source("crates/new.rs", 1001)
        self.ratchet(False)
        (self.root / "crates/new.rs").unlink()
        self.source("agent-harness/new.rs", 1001)
        self.ratchet(False)

    def test_working_tree_baseline_raise_fails(self) -> None:
        baseline = self.root / "ratchet.baseline.json"
        data = json.loads(baseline.read_text())
        data["rules"][RULE]["scopes"]["legacy"]["value"] = 1002
        baseline.write_text(json.dumps(data))
        self.ratchet(False)

    def test_reasoned_scoped_increase_is_rejected_by_validator(self) -> None:
        self.source(EXCEPTION, 1002)
        baseline = self.root / "ratchet.baseline.json"
        data = json.loads(baseline.read_text())
        data["rules"][RULE]["scopes"]["legacy"].update({
            "value": 1002,
            "reason": "allow this increase",
        })
        baseline.write_text(json.dumps(data))

        # Ratchet permits an explicitly reasoned increase; this policy's
        # validator must reject that override even when value and source agree.
        self.ratchet(True)
        validator = Path(__file__).with_name("check_ratchet_exceptions.py")
        spec = importlib.util.spec_from_file_location("exception_validator", validator)
        module = importlib.util.module_from_spec(spec)
        assert spec.loader is not None
        spec.loader.exec_module(module)
        errors = module.validate(self.root)
        self.assertTrue(any("must not contain a reason" in error for error in errors), errors)

    def test_exception_at_or_below_limit_is_stale(self) -> None:
        self.source(EXCEPTION, 1000)
        validator = Path(__file__).with_name("check_ratchet_exceptions.py")
        data = json.loads((self.root / "ratchet.config.json").read_text())
        (self.root / "ratchet.config.json").write_text(json.dumps(data))
        spec = importlib.util.spec_from_file_location("exception_validator", validator)
        module = importlib.util.module_from_spec(spec)
        assert spec.loader is not None
        spec.loader.exec_module(module)
        self.assertTrue(any("stale exception" in error for error in module.validate(self.root)))

    def test_target_directories_are_excluded_but_target_named_sources_stay_in_scope(self) -> None:
        self.source("agent-harness/target/debug/generated.rs", 1200)
        self.source("crates/sample/target/debug/generated.rs", 1300)

        validator = Path(__file__).with_name("check_ratchet_exceptions.py")
        spec = importlib.util.spec_from_file_location("exception_validator", validator)
        module = importlib.util.module_from_spec(spec)
        assert spec.loader is not None
        spec.loader.exec_module(module)
        self.assertEqual(module.validate(self.root), [])
        self.ratchet(True)

        for path in ("crates/target.rs", "agent-harness/targeted.rs"):
            self.source(path, 1001)
            with self.subTest(path=path):
                errors = module.validate(self.root)
                self.assertTrue(
                    any(
                        f"missing exact-file exception for oversized Rust file: {path} (1001 lines)" in error
                        for error in errors
                    ),
                    errors,
                )
                self.ratchet(False)
            (self.root / path).unlink()

    def test_doc_population_uses_the_same_target_directory_filter(self) -> None:
        self.source("crates/target.rs", 1001)
        self.source("agent-harness/targeted.rs", 1002)
        self.source("crates/sample/target/generated.rs", 2000)
        self.source("agent-harness/target/generated.rs", 2001)

        helper = Path(__file__).with_name("rust_source_population.py")
        result = run([
            sys.executable, str(helper), "--root", str(self.root), "--jsonl",
        ], self.root)
        measured = {
            record["path"]: record["line_count"]
            for record in map(json.loads, result.stdout.splitlines())
        }
        self.assertEqual(measured["crates/target.rs"], 1001)
        self.assertEqual(measured["agent-harness/targeted.rs"], 1002)
        self.assertNotIn("crates/sample/target/generated.rs", measured)
        self.assertNotIn("agent-harness/target/generated.rs", measured)


if __name__ == "__main__":
    unittest.main(verbosity=2)
