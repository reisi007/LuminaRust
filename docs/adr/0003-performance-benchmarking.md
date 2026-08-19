# 0003: Performance-Benchmarking

**Status:** akzeptiert  
**Feature-IDs:** F-074, F-075  
**Datum:** 2026-08-19  

## Kontext

LuminaRust ist multi-arch (nativ + wasm32) und benötigt eine messbare,
reproduzierbare Performance-Methodik. Es gibt bislang keinen Timing-Harness.
Die Features F-074 (Performance-Benchmarks) und F-075 (Speicherbudgets)
brauchen verlässliche Messwerte; CI läuft in einem pinned-LibRaw-Container.
Der benchmarkbare Code liegt arch-agnostisch in `lumina-core` – dieselben
Codepfade laufen nativ und im Browser.

## Entscheidung

1. **Criterion** ist das einzige native Timing-Harness, gebündelt im
   separaten Workspace-Crate `crates/lumina-bench` (`publish = false`).
   `lumina-raw` wird optional über das Feature `raw-bench` angebunden, damit
   das Standard-Harness leichtgewichtig bleibt und nicht für
   Nicht-Decode-Benchmarks LibRaw linken muss.
2. **Committete Stores:** `perf/baseline.json` (schema_version 1,
   environment, benchmarks) und `perf/budgets.json` (schema_version 1,
   budgets mit `gate`-Flag). Sie gehören versioniert zum Repository.
3. **Vergleichsskript:** `scripts/perf/compare.mjs` mit den Modi `report`
   (immer, Exit 0), `warn` (budgetierte Überschreitung = Warnung)
   und `gate` (nur `gate: true`-Benchmarks, Exit 1 bei Verletzung).
4. **WASM-Politik:** Die native Messung ist Proxy für alle Archs, weil die
   Core-Codepfade identisch sind. Browser-WASM erhält später nur grobe,
   nicht-gerichtete Smoke-Timings; es gibt keine feingranularen Thresholds in
   CI.

## Alternativen

1. **Divan:** Schlanker Harness, aber Baseline-Vergleich, Statistik und
   Store-Format müssten komplett selbst gebaut werden. Abgelehnt.
2. **Iai/Cachegrind:** Deterministisch (valgrind-basiert, kein
   Rauschen), aber valgrind-abhängig und auf macOS eingeschränkt verfügbar.
   Als spätere Ergänzung möglich, nicht als primäres Harness.
3. **`benches/` direkt in `lumina-core`:** Bringt eine native Dependency in
   die Nähe des wasm-geprüften Kerns und vermischt Harness- und
   Bibliotheks-Cargo.toml. Abgelehnt.
4. **hyperfine:** Gut für Prozess-E2E (CLI), nicht für In-Process-Messungen
   mit Statistik. Optional später für CLI-Level-Benchmarks.

## Begründung

- Criterion liefert Bootstrap-Statistik (Median, Konfidenzintervalle),
  `--save-baseline`/`--baseline` für Baseline-Vergleiche und
  maschinenlesbare `estimates.json` als Grundlage für `compare.mjs`.
- Ein separates natives Crate hält die WASM-Checks unberührt: `lumina-bench`
  wird nie für `wasm32` gebaut, der portable Kern bleibt ohne native
  Test-/Bench-Abhängigkeiten.
- Budgets plus Annotation (`gate`-Flag) statt überall harter Gates vermeidet
  flaky CI: Report und Warnung laufen immer, harte Gates erst nach
  Kalibrierung auf einem stabilen Subset.

## Konsequenzen

- Neuer Workspace-Member `crates/lumina-bench` (native-only, `publish =
  false`).
- `Cargo.lock` wächst um Criterion und seine Abhängigkeiten.
- F-074-N3 (Messungen), F-074-N4 (Baseline-Analyse, Optimierungs-TODOs) und
  F-074-N5 (Regression-Gate) bauen auf diesem Gerüst auf.
- Budget-/Baseline-Änderungen sind bewusste Entscheidungen und werden im
  selben Commit wie das verursachende Feature begründet.
- F-075 (Speicherbudgets) nutzt dieselbe Store- und Report-Philosophie.

## Verweise

- `feature/quality/performance-benchmarks.md` (normatives SOLL für F-074)
- `feature/decisions.md` (Abschnitt Performance-Benchmarking)
- `docs/adr/README.md`
