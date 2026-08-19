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

## Hinzufügen-Checkliste (ab F-074-N3 implementiert)

Neue Benchmarks werden so angelegt:

1. Neues `[[bench]]`-Target in `crates/lumina-bench/Cargo.toml` mit
   `harness = false` (Raw-Decode-Benchmarks zusätzlich mit
   `required-features = ["raw-bench"]`).
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

Stand 2026-08-19 (F-074-N3 umgesetzt): Es existieren Benchmarks für die
definierten Klassen Core/Pipeline, Decode (env-gated) und Batch/End-to-End.
Alle synthetischen Fixtures werden deterministisch mit dem festen Seed
`0x5EED` in `bench/common/mod.rs` erzeugt (Größen 512 / 1024 / 2048). Die
RAW-Decode-Benchmarks sind über `LUMINA_RAW_FIXTURE` und das Feature
`raw-bench` gegated.

Registrierte Benchmark-IDs (jede in `perf/baseline.json` und
`perf/budgets.json`):

| Klasse | IDs |
| --- | --- |
| Core/Pipeline | `core/render_frame__<512\|1024\|2048>`, `core/apply_recipe_with_white_balance__<512\|1024\|2048>`, `core/mask_graph_eval__<512\|1024\|2048>`, `core/analyze_tone__<512\|1024\|2048>`, `core/suggest_auto_tone__<512\|1024\|2048>`, `core/match_total_exposure__<512\|1024\|2048>`, `core/histogram__<512\|1024\|2048>`, `core/cache_hit__<512\|1024\|2048>`, `core/cache_miss__<512\|1024\|2048>` |
| Decode | `decode/raw__aircraft-landscape`, `decode/raw__aircraft-portrait` (env-gated) |
| Batch/End-to-End | `batch/render_export_png__<512\|1024\|2048>` |

Die tatsächlich gemessenen Mediane/P95 stehen in `perf/baseline.json`
(Erfassung 2026-08-19, Umgebung eintragen). Budgets sind mit dem 2-fachen
Median und `tolerance_ratio` 1.2 angelegt; `gate` ist `true` für 30
Core/Batch-Benchmarks und `false` für 2 Decode-Benchmarks (F-074-N5
kalibriert).
