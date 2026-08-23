# Performance-Benchmarks

**Feature:** F-074 Performance-Benchmarks

## Inhaltsverzeichnis

- [Ziel und Geltungsbereich](#ziel-und-geltungsbereich)
- [Benchmark-Klassen](#benchmark-klassen)
- [Fixtures und Daten](#fixtures-und-daten)
- [Messmethodik](#messmethodik)
- [Umgebung und Rauschen](#umgebung-und-rauschen)
- [Baseline- und Budget-Stores](#baseline--und-budget-stores)
- [Vergleichsskript und Regressionspolitik](#vergleichsskript-und-regressionspolitik)
- [Optimierungs-TODO-Ableitung](#optimierungs-todo-ableitung)
- [Bewusste Nichtziele](#bewusste-nichtziele)
- [Status](#status)

## Ziel und Geltungsbereich

Performance ist in LuminaRust eine messbare, reproduzierbare Methodik und
kein Stimmungsbild. Sie liefert die Grundlage für priorisierte
Optimierungs-TODOs und ermöglicht eine semi-automatische
Regressionserkennung: Performanceverschlechterungen werden erkannt und
verhindert – außer das Team nutzt bewusst mehr Features.

Die Methodik steht in engem Bezug zu zwei benachbarten Features:

- **F-075 (Speicherbudgets):** F-074 misst Laufzeit, F-075 definiert
  Speicherbudgets. Beide teilen sich die Store- und Report-Philosophie und
  werden getrennt behandelt.
- **F-043 (Korrektheits-Golden-Tests):** Golden-Image-Tests sind das
  Korrektheits-Gegenstück zu Optimierungen. Eine Optimierung darf die
  Ausgabequalität nicht ändern; jeder Performance-Fund aus F-074 wird gegen
  die Golden-Toleranzen aus F-043 verifiziert.

Benchmarks sind keine Abnahme im Sinne des Definition-of-Done-Prozesses,
solange sie nicht in einen kalibrierten Gate (F-074-N5) münden. Sie sind
Werkzeuge zur Entscheidungsfindung und Regressionsfrüherkennung.

## Benchmark-Klassen

Die Klassen definieren, *was* gemessen wird. Die konkrete Umsetzung erfolgt
in F-074-N3; in diesem Stand (F-074-N1/N2) existieren noch keine Messungen.

### Core/Pipeline (lumina-core)

- **Komplettes `render_frame`:** Source-Actions, Adjustments inklusive
  Weißabgleich, Masken und Geometrie als End-to-End-Pipeline-Lauf.
- **Einzelstufen:**
  - `apply_recipe_with_white_balance` (Adjustments inklusive WB),
  - `apply_geometry` (Geometrie-Stufe),
  - `MaskGraph`-Auswertung und Plane-Resampling (Maskenanwendung).
- **Auto-Tone und Matching:** `analyze_tone`, `suggest_auto_tone` und
  `match_total_exposure`.
- **Histogramm:** `LuminanceHistogram` (Aufbau und Aggregation).
- **Cache-Hit/Miss:** `FolderCache` – Hit-Pfad (Render-Key vorhanden) und
  Miss-Pfad (Render-Key fehlt, Neuberechnung) getrennt messen.

### Decode (lumina-raw)

- RAW-Decode der committeten Fixtures `sample-data/raw/aircraft-landscape.cr3`
  und `sample-data/raw/aircraft-portrait.cr3`.
- Die Decode-Benchmarks sind über `LUMINA_RAW_FIXTURE` env-gated – analog zu
  den bestehenden RAW-Tests in `conflicts-and-acceptance.md`. Ohne die Variable
  werden sie übersprungen.
- Sie gehören nicht in das schnelle CI-Set, sondern in einen eigenen,
  optional aktivierten Lauf (natives Harness, Feature `raw-bench`).

### Batch/Ende-zu-Ende

- `render_frame` + `encode_with_options`-Wiederholungen simulieren den
  Batch-Export: mehrfaches Rendern und Kodieren desselben deterministischen
  Inputs.
- Optional ist später ein CLI-Level-Benchmark mit hyperfine vorgesehen
  (Prozessstart, Datei-I/O, kompletter Exportlauf); er ist kein Ersatz für die
  Criterion-Messungen, sondern ergänzt sie um den Prozess-Overhead.

## Fixtures und Daten

- **Synthetische Bilder:** Deterministische synthetische Bilder werden im
  Bench-Code erzeugt und nicht committet. Feste Auflösungsstufen sind
  512 / 1024 / 2048 (quadratisch oder dokumentierte Seitenverhältnisse). Der
  Zufalls-Seed ist fest im Bench-Code dokumentiert und wird niemals geändert,
  ohne die Baseline neu zu erfassen.
- **RAW:** Ausschließlich die zwei vorhandenen CR3-Fixtures
  (`aircraft-landscape.cr3`, `aircraft-portrait.cr3`). Sie sind lizenzkonform
  dokumentiert (F-073) und bereits in `sample-data/raw/` committet.
- **Keine Netzwerk-Downloads:** Benchmarks laden weder Modelle noch Bilder
  noch sonstige Daten aus dem Netz. Alle Inputs entstehen lokal und
  deterministisch.

## Messmethodik

- **Harness:** Criterion (natives Harness im separaten Workspace-Crate
  `crates/lumina-bench`, siehe ADR 0003). Criterion liefert Bootstrap-Statistik
  und maschinenlesbare `estimates.json`.
- **Laufparameter:** Warmup und Sample-Budget werden pro Benchmark explizit
  gesetzt; Criterion-eigene Outlier-Handling-Mechanismen bleiben aktiv und
  werden nicht abgeschaltet.
- **Ausgabe:** Berichtet werden Median und P95 (Nanosekunden), nicht der
  Mittelwert. Der Median ist robust gegen Ausreißer, P95 zeigt die
  Schwanzlast.
- **Determinismus:** Alle Inputs sind deterministisch (fester Seed,
  synthetische Bilder, committete Fixtures). Eine Messung ist nur dann
  vergleichbar, wenn der Input identisch ist.
- **Profil:** Gemessen wird ausschließlich im Release-Profil
  (`cargo bench` baut Release). Debug-Messungen sind wertlos und werden nicht
  erfasst.
- **Benchmark-ID:** Jede Messung trägt eine stabile ID nach dem Schema
  `<klasse>/<operation>__<fixture>` (siehe `bench/README.md`). IDs werden
  einmal vergeben, niemals still umbenannt und ausschließlich in den Stores
  (`perf/baseline.json`, `perf/budgets.json`) registriert.

## Umgebung und Rauschen

- Der Hardware-/Toolchain-Kontext jeder Baseline-Erfassung wird in
  `perf/baseline.json` unter `environment` festgehalten: Host, OS, Arch,
  rustc-Version, LibRaw-Version und Erfassungszeitpunkt (recorded_at).
- **Keine absoluten Vergleiche über Maschinen hinweg.** Eine Baseline ist nur
  innerhalb derselben Umgebung aussagekräftig. Fremde Hardware oder geänderte
  Toolchains erzeugen eine neue Baseline, keinen Vergleich mit der alten.
- **CI-Runner-Rauschen:** CI-Runner sind verrauscht (Co-Tenancy, DVFS,
  Thermal-Throttling). Messungen dort sind nur als Report-Artefakt und
  grobe Indikation gedacht, nicht als präzise Zahlen.
- **Budget-Toleranzen:** Budgets besitzen eine Toleranz (`tolerance_ratio`),
  die Messrauschen absorbiert. Überschreitungen innerhalb der Toleranz sind
  keine Verletzungen.

## Baseline- und Budget-Stores

Beide Stores sind committet und versioniert – sie gehören zum Repository und
werden mit ihm gepflegt.

### `perf/baseline.json`

```json
{
  "schema_version": 1,
  "environment": null,
  "benchmarks": [
    { "id": "core/render_frame__2048", "median_ns": 0, "p95_ns": 0, "unit": "ns" }
  ]
}
```

- `schema_version`: 1.
- `environment`: null oder Objekt mit `host`, `os`, `arch`, `rustc`,
  `libraw`, `recorded_at`. Wird bei der ersten Baseline-Erfassung (F-074-N3)
  gefüllt.
- `benchmarks`: ein Eintrag pro Benchmark-ID mit Median und P95 in
  Nanosekunden. `unit` ist immer `"ns"`.

### `perf/budgets.json`

```json
{
  "schema_version": 1,
  "budgets": [
    {
      "id": "core/render_frame__2048",
      "budget_ns": 0,
      "tolerance_ratio": 1.2,
      "gate": true,
      "note": "Begründung, siehe Feature-Dokument"
    }
  ]
}
```

- `schema_version`: 1.
- `budgets`: ein Eintrag pro Benchmark-ID mit Budget in Nanosekunden,
  Toleranzfaktor, Gate-Flag und begründender Notiz.
- `gate: true` markiert stabile Benchmarks, die in den harten Gate einfließen
  (frühestens nach Kalibrierung in F-074-N5).

### Budget-Anpassungsregel

Eine Budget-Anpassung ist eine **bewusste Entscheidung** und geschieht im
**selben Commit wie das verursachende Feature** (Feature-Wachstum). Die
Begründung steht im Notizfeld des Budget-Eintrags und im betroffenen
Feature-Dokument. Budgets werden niemals stillschweigend erhöht, nur damit
CI grün bleibt.

## Vergleichsskript und Regressionspolitik

Das Vergleichsskript `scripts/perf/compare.mjs` wertet Criterion-Ergebnisse
gegen Baseline und Budgets aus. Es kennt drei Modi:

| Modus | Verhalten | Exit-Code |
| --- | --- | --- |
| `report` | Immer ausführen; erzeugt das Report-Artefakt | 0 |
| `warn` | Budgetierte Überschreitung erzeugt eine Warnung | 0 |
| `gate` | Nur Benchmarks mit `gate: true`; Verletzung bricht ab | 1 bei Verletzung |

- **Fehlende Baseline wird gemeldet, kein stiller Fallback.** Wenn kein
  Baseline-Eintrag zur gemessenen ID existiert, erscheint ein ausdrücklicher
  Hinweis; das Skript erfindet keine Vergleichswerte.
- **CI:** CI erzeugt ein Report-Artefakt (`/perf/results/`, gitignored).
  Harte Gates laufen erst nach Kalibrierung (F-074-N5) und nur auf dem
  markierten `gate: true`-Subset. Verrauschte CI-Runner sind niemals der
  alleinige harte Gate.
- **Exit-Codes:** 0 = ok, 1 = Verletzung, 2 = Schema-/Nutzungsfehler.

## Optimierungs-TODO-Ableitung

Performance-Funde werden nach einem festen Prozess in umsetzbare Arbeit
überführt (F-074-N4):

1. **Baseline erfassen:** Messwerte für die definierten Klassen liegen in
   `perf/baseline.json` vor.
2. **Anteil je Pipeline-Stufe berechnen:** Einzelstufen-Messungen werden ins
   Verhältnis zum kompletten `render_frame` gesetzt; so entsteht eine
   Kostenverteilung der Pipeline.
3. **Priorisierte Performance-Feature-IDs ableiten:** Für jeden Hotspot wird
   eine Performance-Feature-ID vergeben und mit dem gemessenen Anteil als neue
   offene Aufgabe in `Agents.todo.md` eingetragen.

**Hotspot-Definition:** Ein Hotspot ist die Pipeline-Stufe mit dem höchsten
gemessenen Anteil am Gesamtlauf, beziehungsweise jede Stufe im interaktiven
Pfad (Vorschau, Regler-Interaktion), deren Anteil die
Interaktivitätsschwelle überschreitet.

Optimierungen werden anschließend gegen F-043-Golden-Toleranzen verifiziert
(Korrektheit) und mit F-075-Speicherbudgets abgeglichen (kein Speicher-für-
Zeit-Tausch ohne Entscheidung).

## Bewusste Nichtziele

- keine feingranularen WASM-Thresholds in CI – die native Criterion-Messung
  ist Proxy für alle Archs (identische Core-Codepfade);
- keine absoluten Laufzeitziele als Abnahme ohne Umgebungskontext;
- kein Benchmark auf verrauschten CI-Runnern als alleiniger harter Gate;
- **in diesem Stand keine Messungen:** F-074-N1/N2 dokumentieren nur die
  Methodik und stellen das Setup-Gerüst bereit. Benchmark-Tests, Timing-Code
  und Instrumentierung folgen ausdrücklich erst in F-074-N3.

## Status

Status (2026-08-19): F-074-N1 umgesetzt und unabhängig verifiziert (Methodik
dokumentiert), F-074-N2 umgesetzt und unabhängig verifiziert (Setup-Gerüst
ohne Messungen). F-074-N3 umgesetzt: erste echte Benchmarks für alle
definierten Klassen existieren und eine Baseline ist in `perf/baseline.json`
erfasst (Umgebung dokumentiert). F-074-N4 umgesetzt: Baseline-Analyse liefert
die Kostenverteilung je Pipeline-Stufe und daraus abgeleitete,
priorisierte Performance-Feature-IDs (siehe unten). F-074-N5 umgesetzt:
`scripts/perf/compare.mjs` implementiert die drei Modi (report/warn/gate) mit
korrekten Exit-Codes, ein kalibriertes `gate: true`-Subset (30 deterministische
Core-/Batch-Benchmarks) ist in `perf/budgets.json` registriert, und ein
optionaler, nicht-blockierender `bench`-Job erzeugt ein Report-Artefakt und
läuft CI-seitig nur im `warn`-Modus.

### F-074-N3 — umgesetzte Benchmarks

Implementiert in `crates/lumina-bench` (Criterion-Harness, `harness = false`,
ADR 0003). Synthetische Fixtures werden deterministisch in
`crates/lumina-bench/bench/common/mod.rs` erzeugt (fester Seed `0x5EED`,
Größen 512 / 1024 / 2048). Kein Netzwerk-Zugriff in keinem Benchmark.

- **Core/Pipeline** (`bench/core.rs`, Gruppe `core`): `render_frame` (inkl.
  Masken-Resampling über eine gültige `MaskContext`),
  `apply_recipe_with_white_balance`, `mask_graph_eval` (MaskGraph-Auswertung),
  `analyze_tone`, `suggest_auto_tone`, `match_total_exposure`, `histogram`
  (LuminanceHistogram-Aufbau + Median-Aggregation) sowie `cache_hit` und
  `cache_miss` (FolderCache) — jeweils für 512 / 1024 / 2048.
- **Decode** (`bench/decode.rs`, Gruppe `decode`): `decode/raw__aircraft-landscape`
  und `decode/raw__aircraft-portrait` via `lumina_raw::decode_bytes`.
  Env-gated über `LUMINA_RAW_FIXTURE` und das Feature `raw-bench`
  (`required-features` am `[[bench]]`-Target), damit das Standard-Harness
  LibRaw nicht linken muss. Ohne die Variable werden die Benchmarks sauber
  übersprungen (Hinweis auf stderr, kein Panic, kein Netzwerk).
- **Batch/End-to-End** (`bench/batch.rs`, Gruppe `batch`):
  `render_export_png__<512|1024|2048>` = `render_frame` + `encode_with_options`
  (PNG) auf demselben deterministischen Input.

Alle IDs folgen dem Schema `<klasse>/<operation>__<fixture>` und sind in beiden
Stores (`perf/baseline.json`, `perf/budgets.json`) registriert. Gemessene
Median-/P95-Werte (Nanosekunden) stehen in `perf/baseline.json`; Budgets sind
mit dem 2-fachen Median und `tolerance_ratio` 1.2 angelegt. Das harte
Regression-Gate (F-074-N5) ist kalibriert: 30 deterministische Core-/Batch-
Benchmarks tragen `gate: true`, die beiden env-gated `decode/raw__*`-Benchmarks
bleiben `gate: false` (siehe F-074-N5). Hinweis: Der harte `gate`-Modus vergleicht
`median_ns` der Messung gegen `baseline.median_ns × tolerance_ratio`; das Feld
`budget_ns` in `perf/budgets.json` ist aktuell informativ (Kalibrierwert ≈ 2× der
Baseline) und kein eigenständig erzwungener Schwellwert.

### F-074-N4 — Baseline-Analyse und abgeleitete Performance-TODOs

Kostenverteilung je Pipeline-Stufe, bezogen auf `core/render_frame` derselben
Auflösung (Werte @2048, Quelle `perf/baseline.json`). Die Anteile sind über
512/1024/2048 weitgehend stabil, mit einer Ausnahme: die Auto-Tone-/Exposure-
Match-Stufen rampen mit der Auflösung (≈47 % → 59 % → 64 %), die @2048-Werte
untermauern die A3-Motivation:

| Pipeline-Stufe | @2048 (ns) | Anteil an `render_frame` | Anteil an `batch/render_export_png` |
| --- | ---: | ---: | ---: |
| `core/render_frame` (Gesamt) | 106 665 042 | 100 % | 64,0 % |
| `apply_recipe_with_white_balance` | 97 447 354 | **91,4 %** | 58,4 % |
| `analyze_tone` | 68 983 542 | 64,7 % | — |
| `suggest_auto_tone` | 68 699 605 | 64,4 % | — |
| `match_total_exposure` | 68 712 188 | 64,4 % | — |
| `histogram` | 2 762 231 | 2,6 % | — |
| `mask_graph_eval` | 105 771 | 0,1 % | — |
| `cache_hit` / `cache_miss` | 8 / 6 | ~0 % | — |
| PNG-Encode (Δ `batch` − `render_frame`) | 60 105 979 | 56,4 % | 36,0 % |
| `decode/raw` (eigener Hot-Path, env-gated) | 321 892 250 (landscape) / 360 537 833 (portrait) | **3,0× / 3,4×** `render_frame` | — |

**Hotspot-Schlussfolgerung:** `apply_recipe_with_white_balance` dominiert den
interaktiven Render-Pfad mit **~91 %** von `render_frame` (und ~58 % des
Batch-Exports) über alle Auflösungen hinweg – das ist der klare Regler-/Vorschau-
Hotspot. Der RAW-Decode ist mit **~3,0–3,4× `render_frame`** (bei 2048; bei
kleinen Auflösungen durch den fixen Decode-Overhead noch deutlich höher, z. B.
~45× bei 512) ein eigener, env-gated Hot-Path für die Erst-Öffnen-Latenz.
Auto-Tone/Exposure-Match (`analyze_tone`, `suggest_auto_tone`,
`match_total_exposure`) kosten je ~64 % von `render_frame` und bilden den
Auto-Tone-Interaktionspfad. Der PNG-Encode-Anteil am Batch-Export liegt bei
~36 % (Δ zum reinen `render_frame`).

**Priorisierte Performance-Feature-IDs** (als neue offene Aufgaben in
`Agents.todo.md` einzutragen – Kopplung F-043 Korrektheit, F-075 Speicherbudgets):

- **F-074-A1** — `apply_recipe_with_white_balance` Adjustments-Kernel
  optimieren (WB + Regler pro Pixel). Motivierung: **91,4 %** von `render_frame`
  (und ~58 % von `batch/render_export_png`) @2048 – dominanter interaktiver
  Pfad.
- **F-074-A2** — `decode/raw` Decode-Durchsatz (LibRaw-Overhead, Decode/Upload
  überlappen, dekodierte Buffer cachen). Motivierung: **~3,0–3,4× `render_frame`**
  @2048, eigener env-gated Hot-Path für Erst-Öffnen-Latenz.
- **F-074-A3** — Auto-Tone/Exposure-Match Analyse-Kernel (`analyze_tone`,
  `suggest_auto_tone`, `match_total_exposure`) optimieren (geteilte
  Histogramm/Perzentil-Statistik). Motivierung: je **~64 %** von `render_frame`
  @2048 – Auto-Tone-Button- und Exposure-Match-Interaktionspfad.
- **F-074-A4** — PNG-Export-Encode-Durchsatz (`batch/render_export_png` −
  `render_frame`) optimieren (Codec/Stufen, ggf. schnellere Kodierung).
  Motivierung: Encode-Δ ≈ 60,1 Mio. ns ≈ **56 %** von `render_frame` /
  **36 %** von `batch` @2048 – dominanter Batch-Export-Kostenanteil.

### F-074-N5 — Regression-Gate

`scripts/perf/compare.mjs` (ADR 0003, Modi report/warn/gate) ist vollständig
implementiert und mit den drei Exit-Codes 0 (ok) / 1 (Verletzung) / 2
(Schema-/Nutzungsfehler) ausgestattet. Die Vergleichsschwelle ist
`gemessener Median > Baseline.median_ns * tolerance_ratio`; fehlt ein
Baseline-Eintrag, wird dies ausdrücklich gemeldet – **kein stiller Fallback**,
es werden keine Vergleichswerte erfunden. Pro Modus:

- **report:** immer ausführen, druckt die Tabellenzeile (ID, gemessener Median,
  Baseline-Median, Delta, Status) für alle entdeckten Benchmarks und schreibt
  das Report-Artefakt nach `perf/results/perf-report.md`. Exit 0.
- **warn:** wie report, zusätzlich WARNUNG bei budgetierter Überschreitung
  (nur IDs mit Budget-Eintrag). Exit 0.
- **gate:** prüft ausschließlich `gate: true`-Budgets; jede Überschreitung oder
  fehlende Baseline ist eine Verletzung → Exit 1. Exit 0, wenn alle bestehen.

**Kalibriertes Gate-Subset (`perf/budgets.json`):** 30 deterministische
`core/*`- und `batch/*`-Benchmarks tragen `gate: true` (kalibriert
2026-08-19 gegen die erfasste Baseline; `tolerance_ratio` 1.2 absorbiert
Same-Machine-Rauschen). Die beiden `decode/raw__*`-Benchmarks bleiben
`gate: false` (env-gated, maschinen-/runner-abhängig, nicht im schnellen
CI-Set). Budgets wurden **bewusst nicht still erhöht** – es wurden nur
Gate-Flags und begründende `note`-Felder gesetzt.

**CI (`bench`-Job, `.github/workflows/ci.yml`):** optionaler, nicht-blockierender
Job (`needs: detect`, `if: has_cargo == 'true'`), der `cargo bench -p lumina-bench`
(Release, bescheidene Stichprobe/Warm-up) ausführt, dann
`node scripts/perf/compare.mjs --mode warn --report-dir perf/results` und das
Report-Artefakt hochlädt (`perf/results/`, gitignored). Das harte Gate läuft
**bewusst nicht in CI** (CI-Runner-Rauschen-Regel); CI nutzt nur report/warn.
Die bestehenden `rust`/`wasm`/`docs`-Jobs bleiben unverändert. Feature-Wachstum
wird als bewusste Budget-Anpassung im selben Commit wie das Feature behandelt
(Begründung im `note`-Feld und im betroffenen Feature-Dokument).

### GPU-Bench (F-074-N6 Entwurf)

Erweitert die F-074-Suite, damit **beide** Render-Pfade — CPU
(`lumina-core`) und GPU (`lumina-gpu` / `GpuContext::render_with_gpu`) —
gemessen werden. Implementiert in `crates/lumina-bench/bench/gpu.rs`
(Criterion-Gruppe `gpu`, `harness = false`), abhängig vom `gpu`-Feature des
`lumina-bench`-Crates (Standard-Feature, zieht `lumina-gpu` inkl. dessen
Default-`gpu`-Feature = wgpu/Metal nach).

- **`gpu/render_with_gpu__{512,1024,2048}`** — vollständiger GPU-Render auf
  exakt denselben synthetischen Frames/Recipes wie `core.rs`
  (`bench/common/mod.rs`, Seed `0x5EED`).
- **`gpu/update_uniforms__recipe`** — Microbench des Uniform-Uploads
  (`queue.write_buffer`-Pfad, fixe 2048-Recipe), isoliert vom Render-Pass.
- **`gpu/cpu_vs_gpu__cpu__2048` / `gpu/cpu_vs_gpu__gpu__2048`** —
  End-to-End-Vergleich @2048, beide Pfade back-to-back im selben Report
  (CPU-Pfad = `ImageFrame::apply_recipe`, identisch zur GPU-Fallback-Mathematik),
  damit das Verhältnis direkt ablesbar ist.

**Adapter-Gating:** Der GPU-Kontext wird einmal pro Gruppe erzeugt
(`GpuContext::new()`). Ist kein Adapter gebunden (z. B. headless CI ohne
Metal/Vulkan, oder `gpu`-Feature aus), überspringt die gesamte Gruppe sauber mit
der Meldung `GPU adapter unavailable - skipped equivalence check` — kein Panic,
kein Netzwerk, keine erfundene Zahl. Liegt ein Metal-Adapter vor, läuft der echte
Shader-Pfad; andernfalls misst `render_with_gpu` transparent den CPU-Fallback.

**Budgets (Stand 2026-08-22):** Alle sechs GPU-IDs sind in `perf/budgets.json`
mit `gate: false` registriert (report-only) — neue Benchmarks starten per
F-074-Methodik im Report-Modus, bis der GPU-Pfad stabilisiert und unabhängig
kalibriert ist. `budget_ns` ≈ 2× Median, `tolerance_ratio` 1.2. Eine Baseline
(`perf/baseline.json`) für die GPU-IDs ist **noch nicht** erfasst; `compare.mjs`
meldet im `report`/`warn`-Modus korrekt „KEINE BASELINE" (kein stiller Fallback).
Erfasste Mediane (dieser M5-Pro-Lauf, ns):

| Benchmark-ID | Median (ns) | Verhältnis GPU/CPU @2048 |
| --- | ---: | ---: |
| `gpu/render_with_gpu__512` | 1 527 623 | — |
| `gpu/render_with_gpu__1024` | 1 971 147 | — |
| `gpu/render_with_gpu__2048` | 5 057 014 | — |
| `gpu/update_uniforms__recipe` | 5 324 | — |
| `gpu/cpu_vs_gpu__cpu__2048` | 59 425 792 | — |
| `gpu/cpu_vs_gpu__gpu__2048` | 5 383 811 | **~11,0× schneller** als CPU |

Hinweis: Die GPU-Pipeline liest aktuell via `map_async` in einen CPU-Puffer zurück
(siehe `TODO(PERF)` in `lumina-gpu/src/lib.rs`); dieser Copy ist in den
GPU-Zahlen enthalten. Sobald der GPU-Pfad final ist, wird `gate: true` nach
unabhängiger Kalibrierung aktiviert und die Baseline ergänzt.

## F-075 Speicherbudgets und Abbruchverhalten

**Ziel:** Große RAW-Dekodierungen und Masken-Allokationen dürfen LuminaRust
nicht in einen OOM/Absturz zwingen. Vor der Allokation großer Puffer wird
gegen ein konfigurierbares Speicherbudget geprüft; bei Überschreitung gibt es
einen **klaren Fehler (Abbruch)**, kein stillschweigendes OOM.

**Umsetzung (`crates/lumina-core/src/memory.rs`, plattformneutral):**
`MemoryBudget { max_raw_pixels, max_mask_pixels, max_alloc_bytes }` mit
begründeten Defaults (200 MP RAW, 100 MP Masken, ~2,4 GiB Single-Allocation-
Cap). Konfigurierbar über `LUMINA_MAX_RAW_PIXELS` / `LUMINA_MAX_MASK_PIXELS` /
`LUMINA_MAX_ALLOC_BYTES` via `MemoryBudget::from_env()` (Default-Fallback bei
Fehlen/Ungültig). `check_decode(width, height, channels, bytes_per_channel)`
und `check_mask(width, height)` berechnen die benötigten Bytes und liefern bei
Überschreitung einen `MemoryBudgetError` (Overflow / RawPixelsExceeded /
MaskPixelsExceeded / AllocExceeded).

**Verdrahtung:**
- `lumina-raw` (nativer Decode-Pfad): nach `libraw_unpack` wird via
  `libraw_adjust_sizes_info_only` die Ausgabegeometrie ermittelt und vor dem
  speicherintensiven `libraw_dcraw_process` gegen `check_decode` geprüft
  (Kanäle = 4 für den finalen RGBA-Frame, bewusst konservativ); bei
  Verstoß → `RawError::MemoryBudgetExceeded`.
- `lumina-core` (`rasterize_prompt`): vor Allokation der u16-Masken-Matte wird
  `check_mask` geprüft; bei Verstoß → `MaskError::MemoryBudgetExceeded`.

**Messung:** Die Check-Funktionen liefern die benötigten Bytes zurück
(`required`), sodass Caller die tatsächliche Allokationsgröße berichten können;
eine Allocator-Instrumentierung ist nicht nötig.

**Bekannte Grenzen:** Die Prüfung vor `dcraw_process` nutzt die von
`adjust_sizes_info_only` ermittelte Ausgabegeometrie als Schätzgrundlage; sie
ist konservativ sicher (lehnt eher ab, als OOM zuzulassen). Defaults sind
Großschätzungen für Desktop-Nutzung, keine harten Obergrenzen für
Mobile/Eingebettet.

### Bekannte Grenzen / Limitationen (F-074-N3 / N5)

- **Decode-Gating:** Die Decode-Benchmarks hängen von LibRaw und den
  committeten CR3-Fixtures ab. Sie sind bewusst nicht im schnellen
  Standard-Lauf enthalten; für eine Baseline-Erfassung ist
  `LUMINA_RAW_FIXTURE=sample-data/raw cargo bench -p lumina-bench --features raw-bench`
  nötig. Falls die Erfassung auf einer Maschine entfällt, sind die
  Decode-IDs in `perf/baseline.json` mit `median_ns`/`p95_ns = 0`
  registriert („pending") und werden in F-074-N5 nachgereicht.
- **Umgebungsabhängigkeit:** Baseline-Werte sind nur innerhalb derselben
  Toolchain/Hardware aussagekräftig (siehe „Umgebung und Rauschen"); ein
  Wechsel erfordert eine neue Baseline-Erfassung.
- Die Budgets waren vorläufige 2×-Schätzer; das harte Regression-Gate
  (F-074-N5) ist nun kalibriert: 30 deterministische `core/*`/`batch/*`-
  Benchmarks tragen `gate: true`, die `decode/raw__*`-Benchmarks bleiben
  `gate: false` (siehe F-074-N5 oben).
