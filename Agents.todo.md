# LuminaRust Umsetzungsplan

Dieser Plan ist eine lebende Arbeitsliste. Er wird während der Implementierung
fortgeschritten. Erledigte Aufgaben werden nach bestandener unabhängiger
Verifizierung und bestätigter Testabdeckung aus dieser Datei entfernt. Es gibt
keine dauerhafte Liste abgehakter Aufgaben.

## Stand (2026-08-19)

Offene Arbeit und Abgrenzung — verifiziert abgeschlossene Aufgaben sind aus
dieser Datei entfernt (siehe Git-Historie und Feature-Dokumente):

- **LIZ-ENTSCHEIDUNG** Projektlizenz ist **offen**: Projekt ist aktuell
  kommerziell (kein Open-Source-License) und die Lizenz wird **zur
  MVP-Erklärung** festgelegt. Alle 8 Workspace-Crates tragen bewusst **kein**
  `license`-Feld, keine `LICENSE`-Datei im Repo. Die provisorische MIT-Annahme
  (historisch) wurde aus der Git-Historie entfernt. Optionen zur MVP-Entscheidung:
  MIT / Apache-2.0 / Dual MIT+Apache-2.0 / MPL-2.0 / proprietär.
- **F-036-N1** verifiziert erledigt und entfernt (As-Shot-WB-Kontext,
  Commit folgt); F-042 baut darauf auf.
- **F-042** verifiziert erledigt und entfernt (gemeinsamer Render-Einstiegspunkt
  `render_frame` in lumina-core; Source-Actions- und Masken-Stufe in der
  dokumentierten Reihenfolge; CLI und GUI nutzen dieselbe Renderpipeline;
  Folgeaufgabe F-042-N1 ergänzt).
- **F-085** verifiziert erledigt und entfernt (behaviorale Tests: Source-Actions
  × Auto-WB/Auto-Tone/Matching, Schwellwert-Grenzfälle, Nicht-Destruktion,
  Determinismus, History-Reproduzierbarkeit, CLI-Interplay; 11 Tests).
- **F-072-N2** verifiziert erledigt und entfernt (wasm32-Check lumina-gui auf
  0 Fehler; CI läuft grün — RUSTFLAGS=-D warnings bewusst aus CI entfernt,
  weil Vendor-libraw-sys-Build-Script-Warnungen sonst `cargo check` brechen;
  strikter Gate bleibt Clippy `-D warnings`).
- **F-041** verifiziert erledigt und entfernt (Matching auf finalem sichtbarem
  Messbereich: post-Crop/Geometrie normativ, Masken-Gewicht = Produkt der
  Ebenen/u16::MAX, Fallback Delta 0.0 bei vollmaskiert; CLI/GUI verdrahtet;
  8 Core-Tests, CLI-e2e umgestellt).
- **F-043** verifiziert erledigt und entfernt (22 proptest-Properties für
  Auto-Tone/Matching inkl. Regression-Seeds, 7 Referenzbildtests mit
  programmatisch erzeugten Fixtures + Provenance-README; All-MAX-Short-Circuit
  für bit-exakte Identität maskierter Messung; 250 Tests gesamt grün).
- **F-042-N1** verifiziert erledigt und entfernt (Source-Actions-Persistenz:
  additives `source_actions`-Schema im Sidecar, zdata Repair-Region-Format mit
  `RecordKind`-Diskriminator, CLI `dust-removal`-Command, Lückenschluss
  `resolve_source_actions` in `process_selected`, e2e-Tests — unabhängig
  verifiziert 2026-08-19).
- **F-047** verifiziert erledigt und entfernt (lumina-onnx: SubjectInference-
  Adapter, StubBackend, F-080 ModelManifest/ModelCapabilities mit
  `deny_unknown_fields`, BiRefNet-Deskriptor, onnx-rt非默认, 26 Tests —
  unabhängig verifiziert 2026-08-19).
- **F-074-N3** verifiziert erledigt und entfernt (erste echte Benchmarks: 32
  deterministische Fixtures, Baseline-Erfassung, `compare.mjs`
  Bestandsvalidierung — unabhängig verifiziert 2026-08-19).
- **F-076** verifiziert erledigt und entfernt (Rezept-/Sidecar-/Pipeline-
  Migrationsstrategie: `docs/release-migration-strategy.md` — unabhängig
  verifiziert 2026-08-19).
- **F-080** verifiziert erledigt und entfernt (Modellfähigkeiten: 6 Flags im
  ONNX-Manifest, BiRefNet besitzt `subject_segmentation` als dokumentierte
  Erweiterung — Teil von F-047, unabhängig verifiziert 2026-08-19).
- **F-048** verifiziert erledigt und entfernt (intelligente Masken-Lade-
  entscheidung: `resolve_mask_planes` in lumina-core, `MaskInference`-Trait,
  `ModelManifest::to_model_identity`, CLI-Verdrahtung — unabhängig verifiziert
  2026-08-19).
- **F-051** verifiziert erledigt und entfernt (Verhalten bei nicht verfügbarem
  Modell: Cache-Fallback mit Warnung oder harter Fehler — integriert in F-048,
  unabhängig verifiziert 2026-08-19).
- **F-050** verifiziert erledigt und entfernt (umfassende Entscheidungsschicht-
  Tests für Masken-Invalidierung und Re-Inferenz in `mask_loader.rs`:
  fehlende Artefakte, Modellwechsel, Quelländerung, Decode-Kontext-Änderung,
  `Corrupt`-Status und Inferrenz-Fehlschlag ohne stillen Cache-Fallback;
  falsche Prüfsumme über die zdata-Ebene abgedeckt — unabhängig verifiziert
  2026-08-20).
- **F-097** verifiziert erledigt und entfernt (deterministische Vignettierung
   und Körnung als `recipe.effects.{vignette,grain}`; radiale Vignette und
   kanalgekoppeltes, seed-deterministisches Korn als letzte Adjustment-
   Unterstufe nach Schärfen; fließt über die handgeschriebene `EditRecipe`-
   Serialisierung automatisch in `recipe_hash`/`RenderKey`; Validierung der
   Wertebereiche, Serde-Roundtrip- und Math-Tests — unabhängig verifiziert
   2026-08-20).
- **F-077** verifiziert erledigt und entfernt (19 Backup-/Recovery-/Konflikt-/
  Datenverlusttests als Release-Gate in lumina-sidecar — unabhängig verifiziert
  2026-08-19).
- **F-079** verifiziert erledigt und entfernt (Promptfähige Maskenquellen im
  Masken-DAG-Modell: `MaskPrompt`-Enum Box/Brush/Polygon/Ellipse/Gradient mit
  `PromptTransform` (Teil der Maskenidentität) und `MaskDefinition.prompt`
  (additives Schema-v2-Feld); validiert; deterministischer, modellfreier
  geometrischer Rasterizer in `rasterize_prompt`; DAG wertet Prompt-Quellen
  aus (geladene Ebene zuerst, sonst geometrisch) — unabhängig verifiziert
  2026-08-20). **F-081 (Persistenz von Prompt-Transformationen und
  Koordinatensystemen) ist mit abgedeckt** (`PromptTransform` + bestehendes
  `coordinate_system` auf jeder Prompt-Variante).

Verbleibend bis MVP: Phase 6 AI-Masken
(F-082, F-083 — F-049, F-050, F-079, F-081 sind verifiziert erledigt),
F-101 MCP AI-Agent-Schnittstelle (Phase 7, verifiziert erledigt),
Release-Gates (F-072, F-073, F-078). F-097 (Vignette/Körnung), F-102
(LibRaw-Version in Decode-Identität) und F-075 (Speicherbudgets) sind
verifiziert erledigt.
F-098 (Objektivkorrekturen) ist verifiziert erledigt (bereits implementiert,
unabhängig verifiziert 2026-08-20).
Post-MVP: F-019, Phase 9 (F-064…F-067), WASM-Browser (F-069, F-070).

Der Block **Performance-Methodik (F-074)** ist hochpriorisiert vor den
restlichen MVP-Gates eingeplant (2026-08-19): Methodik-Dokumentation
(F-074-N1) und Setup-Gerüst ohne Messungen (F-074-N2) sind verifiziert
erledigt und aus dieser Datei entfernt; erste Benchmarks (F-074-N3),
Baseline-Analyse (F-074-N4) und Regression-Gate (F-074-N5) folgen in
dieser Reihenfolge.

## Inhaltsverzeichnis

- [Stand](#stand-2026-08-19)
- [Arbeitsregeln](#arbeitsregeln)
- [Performance-Methodik (hohe Priorität)](#performance-methodik-hohe-priorität)
- [Phase 0: Zielzustand und Architektur](#phase-0-zielzustand-und-architektur)
- [Phase 1: Sidecar-Domain-Modell](#phase-1-sidecar-domain-modell)
- [Phase 2: Rezept, virtuelle Kopien und Migrationen](#phase-2-rezept-virtuelle-kopien-und-migrationen)
- [Phase 3: Renderpipeline und Cache](#phase-3-renderpipeline-und-cache)
- [Phase 4: RAW-Verarbeitung](#phase-4-raw-verarbeitung)
- [Phase 5: Auto-Tone und Exposure Matching](#phase-5-auto-tone-und-exposure-matching)
- [Phase 6: Persistente AI-Masken](#phase-6-persistente-ai-masken)
- [Phase 7: CLI und Batch](#phase-7-cli-und-batch)
- [Phase 8: Desktop-GUI](#phase-8-desktop-gui)
- [Phase 9: Optionale zentrale Indizierung](#phase-9-optionale-zentrale-indizierung)
- [Phase 10: WASM und Plattformen](#phase-10-wasm-und-plattformen)
- [Phase 11: Qualität, Performance und Release](#phase-11-qualität-performance-und-release)
- [Abnahmekriterien](#abnahmekriterien)
- [Festgelegte Produktentscheidungen](#festgelegte-produktentscheidungen)

## Arbeitsregeln

- Vor jeder Umsetzung `Agents.md`, `feature/README.md` und das betroffene
  Feature-Dokument lesen.
- Wenn Code und SOLL-Zustand widersprechen, zuerst den Zielzustand klären und
  dokumentieren.
- Jede Aufgabe erhält bei Delegation eine Feature-ID, einen klaren Umfang und
  Abnahmekriterien.
- Der Build-Agent delegiert die Implementierung und anschließend die Prüfung an
  unterschiedliche Subagenten.
- Implementierungs-Agenten werden als `general`-Agenten delegiert (nicht als
  `build`-Agenten); Verifikation läuft immer über einen davon unabhängigen
  `general`-Agenten.
- Der unabhängige Verifizierungs-Agent muss Korrektheit und Testabdeckung
  bestätigen, bevor die Aufgabe aus dieser Datei entfernt wird.
- Eine fehlgeschlagene Verifizierung lässt die Aufgabe offen und erzeugt eine
  konkrete Folgeaufgabe.

## Performance-Methodik (hohe Priorität)

Gegenstand ist F-074: Performance-Tests als messbare, reproduzierbare Methodik
mit Baselines, Budgets und semi-automatischer Regressionserkennung
(„erkennen und verhindern – außer wir nutzen bewusst mehr Features“).
Normativ: `feature/quality/performance-benchmarks.md`, Entscheidung in
`docs/adr/0003-performance-benchmarking.md`. Bewusste Nicht-Ziele in diesem
Block: keine feingranularen WASM-Thresholds in CI (native Criterion-Messung
ist Proxy für alle Archs), keine absoluten Laufzeitziele ohne
Umgebungskontext, keine Benchmark-Tests/Timing vor F-074-N3.
Feature-Wachstum wird als bewusste, dokumentierte Budget-Anpassung im selben
Commit wie das Feature behandelt.

- [ ] **F-074-N3** Erste echte Benchmarks für die definierten Klassen
  (Core/Pipeline, Decode, Cache-Hit, Batch-Export) mit deterministischen
  synthetischen Fixtures und RAW-Gating über `LUMINA_RAW_FIXTURE`;
  Baseline erfassen (`perf/baseline.json`). (Implementiert; unabhängige
  Verifizierung ausstehend — blockiert bis `lumina-sidecar` wieder kompiliert.)
- [ ] **F-074-A1** Hotspot-Optimierung: `apply_recipe_with_white_balance`-
  Adjustments-Kernel (WB + pro-Pixel-Regler) beschleunigen — ~91 % von
  `render_frame` (interaktiver Pfad).
- [ ] **F-074-A2** `decode/raw`-Durchsatz verbessern (LibRaw-Overhead,
  Decode/Upload-Überlappung, Decodepuffer-Cache) — ~3,0–3,4× `render_frame`.
- [ ] **F-074-A3** Auto-Tone-/Exposure-Match-Analyse-Kernel optimieren
  (geteilte Histogramm/Perzentil-Statistik) — ~64 % von `render_frame`.
- [ ] **F-074-A4** PNG-Export-Encode-Durchsatz verbessern (Δ `batch` −
  `render_frame`) — ~56 % von `render_frame` / ~36 % von `batch`.

Verifiziert erledigt und entfernt: **F-074-N1** (Methodik-SOLL-Dokument,
ADR 0003, README-Matrix/-Link, decisions.md, ADR-Index — unabhängig
verifiziert 2026-08-19) und **F-074-N2** (Setup-Gerüst ohne Messungen:
`lumina-bench`-Workspace-Member, Konventionen, leere Baseline-/Budget-Stores,
`compare.mjs`-Gerüst, CI-Kompilierbarkeits-Check — unabhängig verifiziert
2026-08-19, keine Benchmarks/Timing). **F-074-N3** (erste echte Benchmarks,
32 deterministische Fixtures, Baseline-Erfassung, `compare.mjs`
Bestandsvalidierung — unabhängig verifiziert 2026-08-19). **F-074-N4**
(Baseline-Analyse: Kostenverteilung je Pipeline-Stufe, Hotspot
`apply_recipe` ~91 %, abgeleitete Performance-IDs F-074-A1…A4 — unabhängig
verifiziert 2026-08-19) und **F-074-N5** (`scripts/perf/compare.mjs`
report/warn/gate mit korrekten Exit-Codes, 30 deterministische `gate:
true`-Benchmarks, optionaler nicht-blockierender CI-`bench`-Job im `warn`-Modus
— unabhängig verifiziert 2026-08-19). Offene Hotspot-Tasks: A1…A4.

## Phase 0: Zielzustand und Architektur

(alle Punkte umgesetzt und verifiziert — 2026-08-17)

## Phase 1: Sidecar-Domain-Modell

(alle Punkte umgesetzt und verifiziert — 2026-08-17)

## Phase 2: Rezept, virtuelle Kopien und Migrationen

**Produktentscheidung (2026-08-17, Pre-MVP, präzisiert):** Wir befinden uns in
der Pre-MVP-Phase — das Sidecar-Schema wird bei Bedarf **bewusst nicht
abwärtskompatibel** abgeändert. Es gilt: (a) Schemaänderungen sind bis zum MVP
Breaking Changes, Altdateien müssen nicht lesbar bleiben; (b) die
Migrations-Maschinerie (`migrate_sidecar_file`, `.bak`-Backup, `migrate_json`)
bleibt dauerhaft im Code und wird ab dem MVP für Release-Migrationen genutzt;
(c) die v1→v2-Migration mit ihren Tests (aus F-089/F-090) bleibt als
Muster erhalten, aber **pre-MVP gibt es keinen Zwang, für jede Migration einen
eigenen Test zu schreiben** — die Regel „Tests für jede Migration" gilt ab dem
MVP.
Konsequenz: Die Spec-Vorgabe „Altdateien mit flacher adjustments-Map bleiben
als schema_version: 1 gültig" (pipeline.md, Abschnitt Bearbeitungsregler) ist
als Produktanforderung bis zum MVP ausgesetzt; der v1→v2-Migrationspfad mit
Tests wird trotzdem umgesetzt.

- [ ] **F-019** (deferriert auf Post-MVP) CLI `migrate_sidecar`
  (crates/lumina-cli/src/main.rs ~Z. 560) auf `lumina_sidecar::migrate_sidecar_file`
  umstellen (`.bak`-Backup + Lock); erst nach MVP relevant, da bis dahin keine
  Migrationen laufen. Verifikations-Hinweis: Library-Teil ist verifiziert.

## Phase 3: Renderpipeline und Cache

## Phase 4: RAW-Verarbeitung

Diese Phase ist ein verbindliches MVP-Gate. Der erste User-Test gilt erst als
produktseitig vollständig, wenn native RAW-Dekodierung, Orientierung und die
minimalen RAW-Golden-Tests vorhanden sind. **MVP-Grenze (2026-08-17):** Das MVP
umfasst CLI und native Desktop (inkl. RAW). Web/WASM-RAW ist aus dem MVP
geschoben (Post-MVP via `libraw-wasm`, Feature `wasm-js`), die Architektur wird
aber kompatibel gehalten (einheitlicher `decode_bytes`/`RawMetadata`-Vertrag,
`cfg(target_arch = "wasm32")`-Kapselung).

F-036-N1 ist verifiziert erledigt und entfernt (As-Shot-WB-Kontext
`apply_recipe_with_white_balance` in lumina-core; CLI/GUI reichen
`RawMetadata.camera_white_balance` durch; Status in pipeline.md F-036).

F-102 ist verifiziert erledigt und entfernt (LibRaw-Version in der Decode-/
Render-Identität: `DecodeFingerprint.version` bzw. `RenderKey.decode_version`
trägt bei Decoder `"libraw"` die gelinkte LibRaw-Version statt
`CARGO_PKG_VERSION`; Nicht-RAW nutzt weiter die App-Version;
`lumina_raw::libraw_version()` / `libraw_decode_version()` in
`crates/lumina-raw`; verhindert stillschweigendes Cache-/Masken-Reuse bei
LibRaw-Upgrade — CR3-Dimensionen ändern sich zwischen 0.21.x und 0.22.x.
Bekannte Grenze: `libraw_version()` liefert das Build-Suffix, z.B.
`"0.22.2-Release"`; ein reiner Formatwechsel (Release↔Debug) invalidiert
unnötig — optional später auf das numerische Tripel normalisieren. Status in
pipeline.md „Farbprofilstrategie"). **Hinweis:** Die ID F-098 war in diesem
Plan zeitweise mit dieser Aufgabe belegt; F-098 ist die normative ID für
**Objektivkorrekturen** (siehe unten) und wurde dafür zurückgegeben.

F-098 ist verifiziert erledigt und entfernt (Objektivkorrekturen bereits
implementiert: `LensCorrection` in `lumina-sidecar`, `validate_lens` /
`apply_lens` (radiales Verzeichnungspolynom + Vignette) / `apply_ca`
(R/B-Kanal-Skalierung) in `lumina-core`, integriert in `apply_geometry`
(distortion → vignette → perspective → CA → crop); benannte Presets
wide-light/tele-light/standard-neutral; `mask_recipe.lens_correction = None`
schließt Geometrie aus dem Masken-Hash aus, `recipe_hash` invalidiert den
RenderKey. Status in pipeline.md F-098 — unabhängig verifiziert 2026-08-20.
Bekannte Testlücken (nicht blockierend): Preset-Koeffizienten und
Grün-Referenz nicht pixel-explizit assertet; Lensfun bleibt Post-MVP (F-078).

## Phase 5: Auto-Tone und Exposure Matching

F-042 ist verifiziert erledigt und entfernt (Render-Einstiegspunkt
`render_frame` in lumina-core; Source-Actions- und Masken-Stufe in der
dokumentierten Reihenfolge; CLI/GUI nutzen dieselbe Renderpipeline; Status in
pipeline.md F-042; Folgeaufgabe F-042-N1 in Phase 3). F-041 ist verifiziert
erledigt und entfernt (Matching-Messbereich nach Crop/Geometrie/aktiven
Masken; Status in pipeline.md „Exposure Matching" F-041). F-043 ist
verifiziert erledigt und entfernt (Property- und Referenzbildtests; Status in
pipeline.md „Exposure Matching"). **Phase 5 ist damit vollständig abgeschlossen.**

## Phase 6: Persistente AI-Masken

- [ ] **F-082** Einen ersten interaktiven Segmentierungsadapter, vorzugsweise
  SAM 2 nach Lizenz- und ONNX-Prüfung, auswählen und integrieren.
- [ ] **F-083** Prompt-Roundtrip-, Modellfähigkeits-, Re-Run- und
  nicht-unterstützter-Prompt-Tests ergänzen.

## Phase 7: CLI und Batch

(alle CLI/Batch-Punkte umgesetzt und verifiziert — 2026-08-17)

- [x] **F-101** MCP AI-Agent-Schnittstelle: umgesetzt und verifiziert
  (`lumina-mcp` Crate, 8 Tools inkl. `lumina_analyze`, Agent-Skill
  `docs/skills/lumina.md`; SOLL in `feature/platform/mcp-server.md`).
  **Damit ist der letzte vor MVP offene Punkt geschlossen.**
  Erweiterte CLI-Tools (`import`/`batch`/`reindex`/`dust_removal`) und
  `lumina mcp` als CLI-Subcommand sind bewusst als Folgeauftrag offen.

  **Ziel:** Ein AI-Agent (z. B. Claude, Codex, lokales LLM mit
  MCP-Client) soll Bilddateien laden, Rezeptparameter ändern, Sidecars
  speichern und eine schnelle Vorschau erzeugen können — alles über
  standardisierte MCP-Tools, ohne GUI und ohne manuelle CLI-Aufrufe.

  **MCP-Transport:** stdio (stdin/stdout), lauffähig als eigenständiger
  Prozess oder als Sidecar-Dependency von `lumina-cli`. Der Server
  implementiert MCP-Protokollversion `2024-11-05` (oder aktuelle
  Stable-Version). Kein HTTP/WebSocket im MVP — stdio reicht für
  Agent-in-Terminal-Szenarien.

  **Tool-Set (MVP):**

  | Tool | Input | Output | Beschreibung |
  | --- | --- | --- | --- |
  | `lumina_load` | `{ path: string }` | `{ image_id, width, height, format, virtual_copies, sidecar_status }` | Lädt ein Bild (RAW, PNG, JPEG, WebP) und gibt dessen Metadaten zurück. Erzeugt oder erkennt Sidecar. |
  | `lumina_edit` | `{ image_id, virtual_copy?, adjustments: { exposure?, contrast?, highlights?, shadows?, whites?, blacks?, wb_temperature?, wb_tint? } }` | `{ ok: true, recipe_hash }` | Setzt globale Tonwert-Regler im Rezept. Write-through auf Sidecar. |
  | `lumina_get_recipe` | `{ image_id, virtual_copy? }` | `{ recipe: EditRecipe, recipe_hash }` | Liest das aktuelle Rezept einer virtuellen Kopie. |
  | `lumina_save` | `{ image_id, output_path, format: "png"\|"jpeg"\|"webp", quality?: 1..=100 }` | `{ ok: true, bytes_written, path }` | Rendert und exportiert das Bild in das angegebene Format. |
  | `lumina_preview` | `{ image_id, virtual_copy?, max_width?: u32 }` | `{ ok: true, preview_path, width, height, size_bytes }` | Erzeugt eine schnelle, verkleinerte Vorschau (Default max. 1024px breit) als PNG im temporären Verzeichnis. Dient als visueller Feedback-Loop für den Agenten. |
  | `lumina_list_virtual_copies` | `{ image_id }` | `{ copies: [{ id, name, recipe_hash }] }` | Listet alle virtuellen Kopien eines Bildes. |
  | `lumina_inspect` | `{ image_id }` | `{ source_path, sidecar_path, recipe_version, pipeline_version, virtual_copies, ai_masks }` | Zeigt den vollständigen Zustand eines geladenen Bildes. |

  **Schnellvorschau (`lumina_preview`) — Schlüssel-Design:**

  - Die Vorschau ist eine niedrige Auflösung (Default max. 1024px
    Breite, konfigurierbar via `max_width`), die ohne Cache-Eintrag
    erzeugt wird — bewusst ein Fluchtweg, kein Cache-Replace.
  - Rendering nutzt den bestehenden `render_frame`-Einstiegspunkt mit
    reduzierter Ausgabegröße (Resampling nach Pipeline, nicht als
    separate Stufe).
  - Die Vorschau wird als Datei im temporären System-Verzeichnis
    geschrieben (z. B. `$TMPDIR/lumina-previews/`), mit `image_id` als
    Dateinamen, und beim nächsten `lumina_preview`-Aufruf überschrieben.
    Alternativ kann ein `preview_dir` im MCP-Server-Config gesetzt
    werden.
  - Der Agent kann die Vorschau als Datei lesen (Pfad im Output) und
    sie z. B. über ein Vision-Modell analysieren lassen.
  - Determinismus: Gleicher Rezeptstand + gleiche Quelle = gleiche
    Vorschau-Bytes. Die Vorschau ist kein eigenständiges
    Cache-Kontingent — sie wird bei jedem Aufruf frisch erzeugt.

  **Architekturgrenzen:**

  - Der MCP-Server darf keine eigene Bildverarbeitungslogik enthalten.
    Alle Renderoperationen laufen über `render_frame` aus `lumina-core`.
  - Der Server ist ein Opener/Wrapper um die bestehende `lumina-cli`-
    Logik, kein zweites Backend.
  - Sidecar-Schreiboperationen nutzen dieselben atomaren Write-Pfade
    wie CLI und GUI.
  - Kein eigenständiger Render-Cache für den MCP-Pfad — Vorschauen
    sind explizit cache-frei; volle Exporte nutzen den gemeinsamen
    Cache.
  - Der Server ist single-image-scoped: `lumina_load` lädt ein Bild,
    alle weiteren Operationen beziehen sich darauf. Wechsel erfordert
    ein erneutes `lumina_load`. (Multi-Image ist Post-MVP.)
  - Keine ONNX-/Masken-Inferenz über den MCP-Server im MVP — Masken
    werden nur gelesen/angezeigt, nicht berechnet.

  **Nicht-Ziele (Pre-MVP):**

  - Kein HTTP/WebSocket-Transport.
  - Keine Multi-Image-Parallelverarbeitung.
  - Keine AI-Masken-Inferenz über MCP.
  - Keine Preset-Verwaltung über MCP.
  - Keine Batch-Befehle.
  - Keine Authentifizierung (lokaler Prozess).

  **Abhängigkeiten:**

  - `render_frame` (F-042) — muß vorhanden und stabil sein.
  - `EditRecipe` und Sidecar-Serialisierung — muß funktional sein.
  - `ExportOptions` und `ImageFrame::encode` (F-037) — muß für
    `lumina_save` funktionieren.
  - `lumina-cli` als Referenz-Orchestrierung als Vorbild; der
    MCP-Server ist eigenständiges Crate `lumina-mcp`.

  **Test-Strategie (Pre-MVP — Plan):**

  - Unit-Tests für Tool-Dispatch und JSON-Schema-Validierung.
  - Integrationstest: `lumina_load` → `lumina_edit` → `lumina_preview`
    → `lumina_save` als Roundtrip.
  - Determinismus-Test: Zwei `lumina_preview`-Aufrufe mit gleichem
    Rezept liefern identische Bytes.
  - Fehlerpfadtests: ungültiger Pfad, fehlender Sidecar, falsches
    Format, unzulässige Adjustment-Werte.
  - MCP-Protokoll-Compliance: Tool-Liste, Schema-Validierung, Error-
    Response-Format.

  **Feature-Dokument (SOLL):** `feature/platform/mcp-server.md`
  (anzulegen vor Implementierungsbeginn).

  **Erweiterter MVP-Scope (2026-08-19):**

  - **Volle CLI-Abdeckung:** Jeder `lumina`-CLI-Befehl wird ein MCP-Tool
    (`lumina_import`, `lumina_batch`, `lumina_reindex`,
    `lumina_dust_removal` u.a.). Die ursprünglichen 7 Tools sind die
    Mindestausstattung.
  - **Vision-fähiger Agent:** `lumina_preview` liefert den Pfad zur
    gerenderten Vorschau; ein vision-fähiger Agent kann das Bild direkt
    analysieren. Zusätzlich: `lumina_analyze` liefert strukturierte
    Bilddaten (Histogramm, Farbstatistiken) als JSON für Agents ohne
    Vision-Fähigkeit.
  - **Agent-Skill:** Ein OpenCode-Skill (`lumina.md`), der AI-Agenten
    beibringt, wie sie mit LuminaRust arbeiten (Sidecar-Philosophie,
    MCP-Tool-Referenz, Workflows, Best Practices).
  - **Namensfindung:** Finale Produktname vor MVP-Release festlegen.
    Brainstorm: `docs/naming-brainstorm.md`.

  **Status:** Plan und Dokumentation only. Keine Implementierung bis
  zum SOLL-Review.

## Phase 8: Desktop-GUI

(UI-Konventionen F-100 sind spezifiziert, verifiziert und für jede GUI-Arbeit
verbindlich — normativ in feature/platform/cli-gui-wasm.md)

## Phase 9: Optionale zentrale Indizierung

- [ ] **F-064** Minimalen, vollständig wiederaufbaubaren Indexumfang festlegen:
  Pfad, Quellhash, Metadaten, Sidecarstatus, Jobstatus und Cacheverweise.
- [ ] **F-065** SQLite-Index als optionalen Adapter implementieren, ohne
  Rezeptdaten nur dort zu speichern.
- [ ] **F-066** Rebuild aus Sidecars, Aktualisierung, Locking und beschädigte
  DB behandeln.
- [ ] **F-067** Nachweisen, dass Löschen der DB keine Bearbeitungsdaten,
  virtuellen Kopien oder Masken zerstört.

## Phase 10: WASM und Plattformen

- [ ] **F-069** Browser-Dateiimport, temporären Speicher und Exportmodell
  definieren.
- [ ] **F-070** ONNX im Browser als optionale Fähigkeit mit klarer
  Capability-Anzeige behandeln.
- [ ] **F-071** native, Desktop- und Browser-Limits für Bildgröße, Speicher,
  Threads und GPU dokumentieren.

## Phase 11: Qualität, Performance und Release

- [ ] **F-072** CI für Formatierung, Clippy, Unit-, Integrations-, Golden-,
  Property- und CLI-Tests einrichten. (CI existiert und läuft grün: fmt,
  check, test, zdata-Tests, Clippy `-D warnings`, wasm32-Checks für
  lumina-core und lumina-gui mit 0 Fehlern. `RUSTFLAGS="-D warnings"` ist
  bewusst NICHT gesetzt — Vendor-`libraw-sys`-Warnungen würden sonst
  `cargo check`/`cargo test` brechen; der strikte Gate ist Clippy. Offen:
  Golden-/Property-Tests folgen mit F-043/F-073.)
- [ ] **F-074** In den hochpriorisierten Block „Performance-Methodik“
  aufgesplittet (F-074-N1…F-074-N5), siehe oben. Benchmarks für Decode,
  Preview, Maskeninferenz, Cache-Hit und Batch-Export werden dort definiert
  und umgesetzt.
F-075 ist verifiziert erledigt und entfernt (`MemoryBudget` + `check_decode`/
`check_mask` in `crates/lumina-core/src/memory.rs`; Verdrahtung in lumina-raw
(nativ, vor `dcraw_process`) und `rasterize_prompt` (vor Matten-Allokation);
SOLL in `feature/quality/performance-benchmarks.md` F-075 — unabhängig
verifiziert 2026-08-20). Bekannte Grenzen: `MemoryBudgetError::Overflow`-Pfad
ungetestet (praktisch unerreichbar); `from_env`-Test mutiert Env unsynchronisiert
(latente Flakiness in parallelen Tests).

## Abnahmekriterien

Die erste produktiv nutzbare Version muss mindestens Folgendes erfüllen:

- Ein RAW kann ohne zentrale Datenbank importiert, bearbeitet und exportiert
  werden.
- Nach dem Neustart werden Bearbeitungsrezept und virtuelle Kopien ausschließlich
  aus dem Sidecar wiederhergestellt.
- Zwei virtuelle Kopien desselben Originals können unterschiedliche Rezepte,
  Masken-Layer und Exporte besitzen.
- Eine gültige persistierte AI-Maske wird wiederverwendet und nicht ungefragt
  neu berechnet.
- Änderungen an Quelle, Modell, Decode-Kontext oder Maskenartefakt werden als
  veraltet erkannt.
- Vorschauen und Exporte sind über einen reproduzierbaren Render-Key cachebar.
- Das Löschen eines optionalen zentralen Indexes zerstört keine Bearbeitung.
- Originaldateien bleiben byteweise unverändert.
- Sidecar-, Migration-, Cache-, Masken- und virtuelle-Kopien-Tests sind durch
  einen unabhängigen Verifizierungs-Agenten bestätigt.

## Festgelegte Produktentscheidungen

Die fachlichen Entscheidungen sind in `feature/README.md` und den verlinkten
SOLL-Dokumenten festgeschrieben. Neue offene Punkte werden als konkrete
Implementierungsaufgaben mit Feature-ID ergänzt, nicht als unpriorisierte
Entscheidungsliste gesammelt.
