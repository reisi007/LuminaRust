# Benchmark-Konventionen

Diese Datei beschreibt die Konventionen für Benchmarks im Workspace-Crate
`crates/lumina-bench`. Sie enthält bewusst **keinen Benchmark-Code** – die
ersten Benchmarks folgen in F-074-N3.

## Zweck und Verweise

- Normatives SOLL: [`feature/quality/performance-benchmarks.md`](../../feature/quality/performance-benchmarks.md) (F-074)
- Architekturentscheidung: [`docs/adr/0003-performance-benchmarking.md`](../../docs/adr/0003-performance-benchmarking.md) (ADR 0003)

`lumina-bench` ist das einzige native Timing-Harness (Criterion) und wird nie
für `wasm32` gebaut. Die native Messung ist Proxy für alle Archs.

## ID-Schema

Jede Benchmark besitzt eine stabile ID nach dem Schema

```
<klasse>/<operation>__<fixture>
```

Beispiele:

| ID | Bedeutung |
| --- | --- |
| `core/render_frame__2048` | komplettes `render_frame` auf 2048er-Fixture |
| `core/apply_recipe_with_white_balance__2048` | Adjustments inkl. WB |
| `core/mask_graph_eval__2048` | `MaskGraph`-Auswertung und Plane-Resampling |
| `core/cache_hit__2048` | `FolderCache`-Hit-Pfad |
| `decode/raw__aircraft-landscape` | RAW-Decode Fixture `aircraft-landscape.cr3` |
| `batch/render_export_png__2048` | `render_frame` + `encode_with_options` |

Regeln:

- IDs werden **einmal vergeben** und niemals umbenannt, ohne die Baseline neu
  zu erfassen (`perf/baseline.json`).
- Jede ID ist ausschließlich in den Stores registriert
  (`perf/baseline.json`, `perf/budgets.json`); keine ID existiert nur im Code.

## Fixture-Regeln

- **Synthetisch + deterministisch:** Bilder werden im Bench-Code erzeugt,
  nicht committet. Feste Auflösungsstufen: 512 / 1024 / 2048. Der
  Zufalls-Seed ist fest im Bench-Code dokumentiert.
- **RAW nur env-gated:** Decode-Benchmarks laufen nur mit gesetzter
  `LUMINA_RAW_FIXTURE` (Pfad auf `sample-data/raw/`, z. B.
  `aircraft-landscape.cr3` bzw. `aircraft-portrait.cr3`) und gehören nicht in
  das schnelle CI-Set.
- **Kein Netzwerk:** Kein Benchmark lädt Daten aus dem Netz.

## Hinzufügen-Checkliste (ab F-074-N3)

Neue Benchmarks werden so angelegt:

1. Neues `[[bench]]`-Target in `crates/lumina-bench/Cargo.toml` mit
   `harness = false`.
2. Benchmark-Funktion als Criterion-Group implementieren
   (`criterion_group!`/`criterion_main!` bzw. `Criterion::bench_function`).
3. Benchmark-ID gemäß ID-Schema vergeben und in `perf/budgets.json`
   registrieren (mit `gate`-Flag und begründender Notiz).
4. Baseline nachziehen: Messung erfassen und in `perf/baseline.json`
   eintragen (`environment` inklusive `recorded_at` ausfüllen).

## Umgebungshinweise

- `cargo bench` baut das Release-Profil; Debug-Messungen werden nicht erfasst.
- CPU-DVFS, Thermal-Throttling und Hypervisoren verrauschen Messungen.
  Messungen auf fremder Hardware oder CI-Runnern sind keine Basis für
  absolute Vergleiche.
- Nützliche Criterion-Flags:
  - `--sample-size <n>` und `--warm-up-time <s>` steuern Stichprobenumfang und
    Aufwärmzeit pro Benchmark.
  - `--save-baseline <name>` / `--baseline <name>` erfassen beziehungsweise
    vergleichen Baseline-Läufe.

## Stand

Stand 2026-08-19: Es existieren **noch keine Benchmarks** (F-074-N3
ausstehend). Diese Datei beschreibt nur die Konventionen für die kommenden
Benchmarks.
