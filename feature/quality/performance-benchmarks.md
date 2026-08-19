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
ohne Messungen). Erste Messungen und Baseline folgen in F-074-N3;
Regression-Gate in F-074-N5.
