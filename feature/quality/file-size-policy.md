# SOLL — 1000-line Rust source ceiling (RATCHET-CLI-1K-MIGRATION-99)

**Feature-ID:** `RATCHET-CLI-1K-MIGRATION-99` · **Status:** enforced by Ratchet
v0.5.0 (`ratchet.config.json`, `ratchet.baseline.json`) and the focused
validator `scripts/check_ratchet_exceptions.py`.

The 1000-line maximum is an owner-mandated ceiling, not a measured target.
Physical line counts are measured independently for every `.rs` file under
`crates/` and `agent-harness/`. `scripts/file_size_baseline.txt` remains the
separate 500-line growth gate and is not changed by this policy.

## 1. Scope

The checked population is all `.rs` files below `crates/` and
`agent-harness/`; build artifacts under `target/` are outside the population.
Ratchet uses the `line_count` file metric with `aggregate: max` and a ceiling
direction. Exact exception scopes take precedence over the global fallback,
so one file's larger exception cannot mask another file's growth.

## 2. Population

<!-- BEGIN GENERATED: population -->
Population (`crates/` + `agent-harness/`, alle `.rs`-Dateien): **929** Dateien, davon
**94** ueber 500 Zeilen und **2** ueber 1000 Zeilen.
<!-- END GENERATED: population -->

## 3. Permanent ceiling and exceptions

The global fallback baseline is exactly 1000. A file without an exact
exception therefore fails above 1000; a new oversized file cannot inherit a
different file's exception. Each existing oversized file has its own exact
scope and independent baseline. Each exception is checked against its own
baseline; the validator forbids a `reason` field on scoped baseline records,
because Ratchet treats such a reason as authorization for an increase. With no
scoped reason, Ratchet rejects a working-tree increase relative to the
committed baseline.

An exception is required to be removed as soon as its file is at or below 1000
lines. Ratchet rejects an empty scope, but that does not detect a still-active
scope whose file remains present and has shrunk; therefore the gate also runs
`scripts/check_ratchet_exceptions.py`, which fails for a missing or <=1000-line
exception file. A newly approved exception must be an exact-file scope at its
measured current size, with its baseline initialized through Ratchet and
reviewed as an explicit exception.

<!-- BEGIN GENERATED: file-sizes -->
| Datei | Ist-Stand (Zeilen) | Ausnahme-Baseline | Eintrag `scripts/file_size_baseline.txt` |
| --- | --- | --- | --- |
| `crates/lumina-cli/src/main.rs` | 4572 | 4572 | 4572 |
| `crates/lumina-gui/src/lib.rs` | 10517 | 10517 | 10517 |
<!-- END GENERATED: file-sizes -->

## 4. Baseline direction

`direction: ceiling` means each current measurement must be no greater than its
own baseline. Scoped baseline records must not contain `reason`: Ratchet allows
a reasoned increase, so `scripts/check_ratchet_exceptions.py` rejects that
waiver and Ratchet's working-tree monotonicity check rejects an unreasoned
increase. Initial exception entries are owner-approved at the measured current
line count; later scoped values may only decrease. The global fallback is
independently pinned to exactly 1000 by the validator.

## 5. Relationship to the 500-line growth gate

The 500-line gate remains `scripts/check_file_sizes.sh`, with its existing
population under `crates/` and baseline `scripts/file_size_baseline.txt`.
It is independent and unchanged. A source file may be subject to both gates;
the 1000-line rule does not replace the 500-line growth rule.

## 6. Gate and focused validation

`sh scripts/check_file_ceiling.sh` checks Ratchet v0.5.0, validates active
exact exceptions, runs `scripts/test_ratchet_file_ceiling.py`, and executes
Ratchet rule `rust-file-line-ceiling`. The focused tests cover an existing
exception overrun, an unexcepted oversized file in either population, an
unreasoned working-tree baseline increase, a reasoned increase attempt that the
validator rejects, and a stale exception at or below 1000 lines.

## 7. Population facts

The generated population above comes from the filesystem. The exception table
is generated from exact scopes in `ratchet.config.json` and independent scope
values in `ratchet.baseline.json`; the drift gate checks both generated blocks.
