# Benchmark-Konventionen

Diese Datei beschreibt die Konventionen für Benchmarks im Workspace-Crate
`crates/lumina-bench`. Sie enthält bewusst **keinen Benchmark-Code** – die
ersten Benchmarks folgen in F-074-N3.

## Zweck und Verweise

- Normatives SOLL: [`feature/quality/performance-benchmarks.md`](../../feature/quality/performance-benchmarks.md) (F-074)
- Architekturentscheidung: [`docs/adr/0003-performance-benchmarking.md`](../../docs/adr/0003-performance-benchmarking.md) (ADR 0003)

`lumina-bench` ist das einzige native Timing-Harness (Criterion). Die native
Messung ist Proxy für alle Konfigurationen.

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
| `merge/hdr_weighted__2048` | gewichteter linearer HDR-Merge (`lumina-merge`) |
| `merge/pano_blend__2048` | Panorama-Blend über die volle 3×3-Matrix |
| `merge/encode_dng__2048` | linearer 16-Bit-DNG-Writer |
| `merge/align_hdr__512` | HDR-Translations-Suche (`max_shift_px = 8`) |
| `denoise/blend__2048` | `strength`/`preserve_detail`-Blend-Kernel |
| `denoise/assemble_tiles__2048` | nahtlose Kachel-Assembly (512/32-Overlap) |
| `denoise/render_ready__2048` | `render_frame_with_denoise` mit `ready`-Artefakt |
| `denoise/status_resolve__ready` | §6-Statusklassifikation |
| `cull/analyze__2048` | vollständige Stufe-1-Heuristik |
| `cull/noise_sigma__2048` | Immerkaer-Rauschkernel (dokumentierter Hotspot) |
| `cull/similarity_signature__2048` | dHash/Histogramm-Signatur |
| `cull/analyze_selection__4x512` | explizite 4-Bild-Auswahl inkl. Gruppierung |
| `cull/status_evaluate__valid` | Identitäts-/Statusklassifikation |

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

Stand 2026-09-17 (F-074-N3 + Re-Baseline MERGE-IMPL-15 + F-074-N8): Es
existieren Benchmarks für die definierten Klassen Core/Pipeline, Decode
(env-gated), Batch/End-to-End, GPU (F-074-N6), Merge (`bench/merge.rs`,
F-074-N7) sowie Denoise (`bench/denoise.rs`) und Culling (`bench/cull.rs`,
beide F-074-N8). Alle synthetischen Fixtures werden deterministisch mit dem
festen Seed `0x5EED` in `bench/common/mod.rs` erzeugt (Größen 512 / 1024 /
2048). Die RAW-Decode-Benchmarks sind über `LUMINA_RAW_FIXTURE` und das
Feature `raw-bench` gegated. Die Denoise-Klasse ist kein Modell-Benchmark:
Die ONNX-Gewichte sind `pending-integration`, gemessen wird ausschließlich
der deterministische Fixture-Pfad.

Registrierte Benchmark-IDs (jede in `perf/baseline.json` und
`perf/budgets.json`):

| Klasse | IDs |
| --- | --- |
| Core/Pipeline | `core/render_frame__<512\|1024\|2048>`, `core/apply_recipe_with_white_balance__<512\|1024\|2048>`, `core/mask_graph_eval__<512\|1024\|2048>`, `core/analyze_tone__<512\|1024\|2048>`, `core/suggest_auto_tone__<512\|1024\|2048>`, `core/match_total_exposure__<512\|1024\|2048>`, `core/histogram__<512\|1024\|2048>`, `core/cache_hit__<512\|1024\|2048>`, `core/cache_miss__<512\|1024\|2048>` |
| Decode | `decode/raw__aircraft-landscape`, `decode/raw__aircraft-portrait` (env-gated) |
| Batch/End-to-End | `batch/render_export_png__<512\|1024\|2048>` |
| GPU (F-074-N6) | `gpu/render_with_gpu__<512\|1024\|2048>`, `gpu/update_uniforms__recipe`, `gpu/cpu_vs_gpu__{cpu,gpu}__2048` |
| Merge (F-074-N7) | `merge/hdr_weighted__<512\|1024\|2048>`, `merge/pano_blend__<512\|1024\|2048>`, `merge/encode_dng__<512\|1024\|2048>`, `merge/align_hdr__512` |
| Denoise (F-074-N8) | `denoise/blend__<512\|1024\|2048>`, `denoise/assemble_tiles__<512\|1024\|2048>`, `denoise/render_ready__<512\|1024\|2048>`, `denoise/status_resolve__ready` |
| Culling (F-074-N8) | `cull/analyze__<512\|1024\|2048>`, `cull/noise_sigma__<512\|1024\|2048>`, `cull/similarity_signature__<512\|1024\|2048>`, `cull/analyze_selection__4x512`, `cull/status_evaluate__valid` |

Die tatsächlich gemessenen Mediane/P95 stehen in `perf/baseline.json`. Die
Erfassung vom 2026-09-17 (Umgebung: rustc 1.98.0, gleiche Maschine) hat alle
messbaren Core-/Batch-/GPU-IDs neu erfasst (die alte Baseline war seit den
F-074-A1/A3-/GPU-Optimierungen veraltet) und die neue Merge-Klasse ergänzt;
die env-gated `decode/raw__*`-IDs bleiben unangetastet. F-074-N8 hat die 21
Denoise-/Culling-IDs (Erfassung 2026-09-17) ergänzt. Budgets sind mit dem
2-fachen Median und `tolerance_ratio` 1.2 angelegt. `gate` ist `true` für
Core-/Batch-/GPU-Benchmarks und `false` für die 2 Decode-, die 10
Merge- sowie die 21 Denoise-/Culling-Benchmarks (neue Klassen starten
report-only, bis sie unabhängig kalibriert sind).
