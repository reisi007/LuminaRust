# LuminaRust Umsetzungsplan

Dieser Plan ist eine lebende Arbeitsliste. Er wird während der Implementierung
fortgeschrieben. Erledigte Aufgaben werden nach bestandener unabhängiger
Verifizierung und bestätigter Testabdeckung aus dieser Datei entfernt — es gibt
keine dauerhafte Liste abgehakter Aufgaben. Verifiziert Erledigtes steht unten
als kompakte Kurzliste; die Details liegen in den Feature-Dokumenten und der
Git-Historie.

## Stand (2026-08-20)

Offene Arbeit und Abgrenzung bis MVP:

- **LIZ-ENTSCHEIDUNG (beantwortet 2026-08-20: interim proprietär, spätere
  Entscheidung offen; MVP-Release-Gate):** Projekteigentümer hat entschieden,
  die Wahl vorerst **nicht** zu treffen und den **proprietären/kommerziellen
  Status** beizubehalten (kein `license`-Feld in den 9 Crates, keine
  `LICENSE`-Datei). Sobald entschieden: MIT / Apache-2.0 / Dual
  MIT+Apache-2.0 / MPL-2.0 — dann `license`-Felder + Root-`LICENSE`
  ergänzen (F-073-R2). **Lensfun** (MVP-Teil seit F-098-N1, LGPL-3.0
  dynamisch gelinkt, DB CC-BY-SA-3.0) ist in `THIRD-PARTY-NOTICES.md`
  dokumentiert und gilt unabhängig von der späteren Wahl.
- **F-098-N1** Lensfun-Integration als **Pre-MVP**-Bestandteil (Phase 3):
  **verifiziert erledigt** (2026-08-20) — Crate `lumina-lensfun` + lumina-core
  `lensfun`-Feature + Pipeline-Integration mit graceful fallback + Tests
  (siehe Phase 3). Folgeaufgaben: **F-098-N2** (CLI-Verdrahtung,
  implementiert, Verifikation ausstehend), **F-098-N3** (CI-Container,
  implementiert, Verifikation ausstehend), **F-098-N4** (Lizenz-Doku,
  implementiert, Verifikation ausstehend).
- **F-082 / F-083** SAM-2-Segmentierungsadapter + Prompt-Roundtrip-Tests
  (Phase 6): **SAM 2 vom Eigentümer bestätigt** (2026-08-20); Start nach
  Abschluss der aktuellen Batch (F-098-N2…N4, F-074-A1…A4) mit der
  Apache-2.0-Lizenzprüfung der tatsächlichen Gewichtsquelle.
- **F-073** Fixtures-Lizenzierung/Audit (Phase 11): Audit-Dokumente
  existieren; **R1 geschlossen (2026-08-20)** — Autor via EXIF + Commit
  belegt, uneingeschränkte Nutzungs-/Distributionsgewährung für LuminaRust
  dokumentiert (`sample-data/raw/README.md`, §4/§8); **R2** = LIZ-
  Entscheidung (oben, offen).
- **F-078** Lizenz-/Modell-/Distributionsaudit (Phase 11): Audit-Dokumentation
  abgeschlossen (`THIRD-PARTY-NOTICES.md`, `fixtures-licensing.md`, inkl.
  **Lensfun-LGPL-3.0-Eintrag** seit 2026-08-20); Beleg
  LibRaw = einzige Pflicht-/Datenbank-Dependency, `ort`
  `=2.0.0-rc.13` optional hinter `onnx-rt`, ONNX Runtime MIT, BiRefNet
  Apache-2.0; **unabhängig verifiziert BESTANDEN (2026-08-20)** — Auflagen
  (NOTICES-Vollständigkeit 480/480, stale EN-R1-Texte, Crates-Zahl) direkt
  behoben. Offen bis Final-Release: R3/R4 (Bundle-Artefakte), R5 (ADR-0002-
  Dreifachlizenz), Lensfun-SPDX/DB-Lizenz-Detail, R2-LIZ.
- **F-072** CI-Gates: unabhängig verifiziert **bestanden** (2026-08-20) —
  verbleibender Aufwand nur Eintrag-Nachpflege (siehe Phase 11).

## Antworten des Projekteigentümers (2026-08-20, interaktiv abgefragt)

Alle vier Fragen wurden am 2026-08-20 beantwortet; die Ergebnisse sind in die
betroffenen Feature-Dokumente übernommen. Noch offene Folgearbeit ist unten
mit Datum markiert.

1. **LIZ-Entscheidung — Projektlizenz:** **vorerst offen / proprietären
   Status beibehalten.** Keine `LICENSE`-Datei, keine `license`-Felder;
   interim ist das Projekt bewusst unlizenziert/kommerziell bis zur
   endgültigen Entscheidung (F-073-R2/F-078 bleiben offen). Lensfun-Pflichten
   (LGPL-3.0 dynamisch gelinkt → nur Sammelwerk-Hinweis; DB CC-BY-SA-3.0 →
   Attribution) sind in `THIRD-PARTY-NOTICES.md` dokumentiert und gelten
   unabhängig von der späteren Wahl.
2. **Fixtures-Lizenzgewährung (F-073-R1):** **GELÖST** — Eigentümer hat eine
   **uneingeschränkte Nutzungs- und Distributionsgewährung für das
   LuminaRust-Projekt** erteilt (2026-08-20); eingetragen im Provenienz-Block
   `sample-data/raw/README.md`, Status in `feature/quality/fixtures-licensing.md`
   §4/§8 aktualisiert. Verifikation der Doku im Rahmen der F-078-Abnahme.
3. **Interaktives Segmentierungsmodell (F-082):** **SAM 2 bestätigt**
   (2026-08-20). F-082/F-083 starten nach Abschluss der aktuellen
   Umsetzungs-Batch (F-098-N2/N3/N4, F-074-A1…A4) mit der Apache-2.0-
   Lizenzprüfung der tatsächlichen Gewichtsquelle und der ONNX-
   Einbindung; die Projektlizenz (Frage 1) blockiert die Implementierung
   nicht (Modell ist Apache-2.0, unabhängig von der Projektlizenz).
4. **Produktname (F-101-F1):** **später entscheiden** (kein MVP-Blocker) —
   bleibt als offener Punkt dokumentiert (`docs/naming-brainstorm.md`).

**Post-MVP (nicht MVP-blockierend):** F-019 (CLI `migrate_sidecar`), Phase 9
Index (F-064…F-067), WASM-Browser (F-069…F-071), MCP-Erweiterungen
(CLI-Tools als MCP-Tools, `lumina mcp`-Subcommand, Vision-Agent,
Namensfindung — siehe Phase 7), Lensfun-Ausbau (CA via Lensfun, automatische
Profil-Erkennung per EXIF, WASM-Port).

### Verifiziert erledigt (Kurzliste, neueste zuerst)

Details in Git-Historie und Feature-Dokumenten; „Lensfun bleibt Post-MVP" ist
durch F-098-N1 obsolet und bewusst entfernt.

- **F-098** Objektivkorrekturen (manuelles Modell, `apply_geometry`-Reihenfolge
  distortion → vignette → perspective → CA → crop; Presets; RenderKey) —
  unabhängig verifiziert 2026-08-20. Bekannte Testlücke (nicht blockierend):
  Preset-Koeffizienten und Grün-Referenz nicht pixel-explizit assertet.
- **F-098-N1** Lensfun-Integration (Pre-MVP): Crate `lumina-lensfun`
  (FFI + Safe Wrapper, 6 Native-Tests) + lumina-core `lensfun`-Feature +
  Pipeline-Integration mit byte-identischem Fallback — unabhängig verifiziert
  2026-08-20 (BESTANDEN). Folgeaufgaben F-098-N2…N4 offen (Phase 3).
- **F-075** Speicherbudgets + Abbruch großer RAW/Masken; **F-102** LibRaw-
  Version in Decode-/Render-Identität; **F-097** Vignette/Körnung;
  **F-050** Masken-Entscheidungsschicht — verifiziert 2026-08-20.
- **F-074-N1…N5** Performance-Methodik (N1 SOLL/ADR 0003, N2 Gerüst, N3 erste
  Benchmarks + Baseline, N4 Analyse/Hotspots, N5 compare.mjs report/warn/gate)
  — verifiziert 2026-08-19. Offene Hotspot-Tasks A1…A4, siehe Abschnitt unten.
- **F-101** MCP AI-Agent-Schnittstelle (`lumina-mcp`, 8 Tools inkl.
  `lumina_analyze`, `docs/skills/lumina.md`, SOLL
  `feature/platform/mcp-server.md`) — verifiziert 2026-08-19.
- **F-036-N1** As-Shot-WB-Kontext; **F-042** Render-Einstiegspunkt
  `render_frame`; **F-042-N1** Source-Actions-Persistenz + `dust-removal`;
  **F-041** Matching-Messbereich post-Crop; **F-043** 22 Property- + 7
  Referenzbildtests; **F-085** behaviorale Tests; **F-047/F-080** ONNX-Adapter +
  Modellfähigkeiten; **F-048/F-051/F-079/F-081** Masken-Laden/-Quellen;
  **F-076** Migrationsstrategie; **F-077** Backup-/Recovery-Tests;
  **F-072-N2** wasm32-Check lumina-gui; **F-100** GUI-Konventionen — verifiziert
  2026-08-17…19.

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
Umgebungskontext. Feature-Wachstum wird als bewusste, dokumentierte
Budget-Anpassung im selben Commit wie das Feature behandelt.

- [ ] **F-074-A4** PNG-Export-Encode-Durchsatz verbessern (Δ `batch` −
  `render_frame`) — ~56 % von `render_frame` / ~36 % von `batch`.
  *(Implementierung läuft, 2026-08-20.)*

Verifiziert erledigt und entfernt: F-074-N1 (Methodik-SOLL, ADR 0003),
F-074-N2 (Setup-Gerüst), F-074-N3 (erste echte Benchmarks: 32 deterministische
Fixtures, Baseline-Erfassung, `compare.mjs`-Bestandsvalidierung), F-074-N4
(Baseline-Analyse: Hotspot `apply_recipe` ~91 %, abgeleitete IDs A1…A4),
F-074-N5 (`scripts/perf/compare.mjs` report/warn/gate, optionaler
nicht-blockierender CI-`bench`-Job im `warn`-Modus), F-074-A1
(Adjustments-Kernel: per-Kanal-LUT-Fusion, −40 % auf dem Hotspot,
byte-identisch über 128 Trigger-Kombinationen, Commit `8dacd3d`),
F-074-A2 (Decode-Durchsatz: Kopie entfernt + RGBA-Fast-Path, −19,6 %/−18,5 %
vs. Baseline, pixel-identisch, Commit `a03c272`), F-074-A3 (Auto-Tone-Kernel:
Single-Pass-`mean_luminance` statt Voll-Sort, −95 % `match_total_exposure`,
−22 % `analyze_tone`/`suggest_auto_tone`, wert-identisch, Commit `2253de5`) —
jeweils unabhängig verifiziert (BESTANDEN).

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
(c) die v1→v2-Migration mit ihren Tests (aus F-089/F-090) bleibt als Muster
erhalten, aber **pre-MVP gibt es keinen Zwang, für jede Migration einen
eigenen Test zu schreiben** — die Regel „Tests für jede Migration" gilt ab dem
MVP. Konsequenz: Die Spec-Vorgabe „Altdateien mit flacher adjustments-Map
bleiben als schema_version: 1 gültig" (pipeline.md, Abschnitt
Bearbeitungsregler) ist als Produktanforderung bis zum MVP ausgesetzt; der
v1→v2-Migrationspfad mit Tests wird trotzdem umgesetzt.

- [ ] **F-019** (deferriert auf Post-MVP) CLI `migrate_sidecar`
  (crates/lumina-cli/src/main.rs) auf `lumina_sidecar::migrate_sidecar_file`
  umstellen (`.bak`-Backup + Lock); erst nach MVP relevant, da bis dahin keine
  Migrationen laufen. Verifikations-Hinweis: Library-Teil ist verifiziert.

## Phase 3: Renderpipeline und Cache

F-098-N1 (Lensfun-Integration als **Pre-MVP**-Bestandteil) ist verifiziert
erledigt und entfernt: neues natives Crate `crates/lumina-lensfun`
(Workspace-Member, nicht in wasm32-Checks; Feature `native` default off,
ohne native leere Lib; build.rs linkt System-liblensfun dynamisch via
pkg-config — LGPL-3.0, Datenbank CC-BY-SA, keine neuen Crates/bindgen/cc;
handgeschriebenes `extern "C"`-FFI-Subset; Safe Wrapper `LensfunDb::load_system`
+ `Corrector::for_camera` (None = kein Profil/Identität → graceful fallback) +
`geometry()`/`color_gain()`/`is_identity()`; 6 Native-Tests grün gegen die
reale Profil-DB). `lumina-core`: Feature `lensfun = [dep:lumina-lensfun,
lumina-lensfun/native]` default off (Default-Build + wasm32 bleiben grün);
cfg-gatedes `RenderContext.lensfun`-Feld; `apply_geometry`/`apply_lens` nutzen
per-Pixel Lensfun-Geometrie/Vignette bei nicht-identischem Corrector
(Priorität Lensfun > manuell > Identität), sonst byte-identisches manuelles
Modell (Test `lensfun_none_is_byte_identical_to_default_pipeline`); CA bleibt
manuell (MVP-Grenze). Alle RenderContext-Konstruktionsstellen cfg-korrekt
(cli/gui/mcp/bench deklarieren `lensfun`-Forwarding-Feature). Gates: fmt,
check --workspace, wasm32 core+gui, Clippy workspace CI-Config + Feature
`-D warnings`, Tests Default (204+7) + Feature (207+7) + lumina-lensfun
(6) + lumina-cli (13+8) — alle grün; **unabhängig verifiziert 2026-08-20**
(BESTANDEN; M1 per-pixel-FFI-Overhead ohne Benchmark, M3
crop-factor-Offset-Hack dokumentiert; nicht blockierend). SOLL:
pipeline.md F-098.

Folgeaufgaben (offen, einzeln delegierbar):
- [ ] **F-098-N2** (S6) Applikative CLI-Verdrahtung: in `lumina-cli` den
  Corrector aus `RawMetadata.camera_make`/`camera_model` (+ Brennweite/Blende
  sofern vorhanden) aufbauen und als `Some` in `RenderContext.lensfun` reichen
  (Feature `lensfun` an); strikter Fallback ohne EXIF-Angaben; Smoke-Test,
  dass ein reales Profil den Export verändert.
- [ ] **F-098-N3** (S7) CI: `liblensfun-dev` in den pinned Container
  (`ci-libraw-image.yml`) aufnehmen und Feature-Tests als separaten,
  pro-Crate-Schritt mit synchronisierten Features führen (Hinweis M2:
  nicht workspace-weit mit gemischten Features bauen);
  `cargo test -p lumina-lensfun --features native -p lumina-core --features lensfun`.
- [ ] **F-098-N4** (S8) Lizenz-/Distributions-Doku: Lensfun-Eintrag in
  `THIRD-PARTY-NOTICES.md` + `feature/quality/fixtures-licensing.md`
  (LGPL-3.0, DB CC-BY-SA, dynamisch gelinkt, Quellangebot im Release-Bundle);
  zählt zur F-078-Abnahme.

## Phase 4: RAW-Verarbeitung

Diese Phase ist ein verbindliches MVP-Gate. Der erste User-Test gilt erst als
produktseitig vollständig, wenn native RAW-Dekodierung, Orientierung und die
minimalen RAW-Golden-Tests vorhanden sind. **MVP-Grenze (2026-08-17):** Das MVP
umfasst CLI und native Desktop (inkl. RAW). Web/WASM-RAW ist aus dem MVP
geschoben (Post-MVP via `libraw-wasm`, Feature `wasm-js`), die Architektur wird
aber kompatibel gehalten (einheitlicher `decode_bytes`/`RawMetadata`-Vertrag,
`cfg(target_arch = "wasm32")`-Kapselung).

Verifiziert erledigt und entfernt:
- **F-036-N1** As-Shot-WB-Kontext `apply_recipe_with_white_balance`;
  CLI/GUI reichen `RawMetadata.camera_white_balance` durch (pipeline.md F-036).
- **F-102** LibRaw-Version in Decode-/Render-Identität
  (`DecodeFingerprint.version`/`RenderKey.decode_version` tragen bei Decoder
  `"libraw"` die gelinkte LibRaw-Version; `libraw_version()` in lumina-raw;
  verhindert stillschweigendes Cache-/Masken-Reuse bei LibRaw-Upgrade — CR3-
  Dimensionen ändern sich zwischen 0.21.x und 0.22.x). Bekannte Grenze:
  `libraw_version()` liefert das Build-Suffix (`"0.22.2-Release"`); optional
  später auf das numerische Tripel normalisieren.
- **F-098** Objektivkorrekturen (manuelles Modell): `LensCorrection` in
  `lumina-sidecar`, `validate_lens`/`apply_lens`/`apply_ca` in `lumina-core`,
  integriert in `apply_geometry`; Presets wide-light/tele-light/standard-
  neutral; `mask_recipe.lens_correction = None` schließt Geometrie aus dem
  Masken-Hash aus, `recipe_hash` invalidiert den RenderKey. **Lensfun-Erweiterung
  als MVP läuft unter F-098-N1 in Phase 3** (nicht mehr Post-MVP).

## Phase 5: Auto-Tone und Exposure Matching

(abgeschlossen: F-042 Render-Einstiegspunkt, F-041 Matching-Messbereich,
F-043 Property-/Referenzbildtests — jeweils verifiziert erledigt und entfernt)

## Phase 6: Persistente AI-Masken

- [ ] **F-082** Einen ersten interaktiven Segmentierungsadapter, vorzugsweise
  SAM 2 nach Lizenz- und ONNX-Prüfung, auswählen und integrieren.
- [ ] **F-083** Prompt-Roundtrip-, Modellfähigkeits-, Re-Run- und
  nicht-unterstützter-Prompt-Tests ergänzen.

## Phase 7: CLI und Batch

(CLI/Batch-Punkte umgesetzt und verifiziert — 2026-08-17; F-101 MCP-Schnittstelle
verifiziert erledigt: `lumina-mcp`-Crate, 8 Tools inkl. `lumina_analyze`,
Agent-Skill `docs/skills/lumina.md`, SOLL `feature/platform/mcp-server.md` —
2026-08-19)

- [ ] **F-101-F1** Erweiterter MCP-Scope: volle CLI-Abdeckung als MCP-Tools
  (`lumina_import`, `lumina_batch`, `lumina_reindex`, `lumina_dust_removal`
  u. a.), `lumina mcp` als CLI-Subcommand, Vision-fähiger Agent
  (strukturierte `lumina_analyze`-Daten für Agents ohne Vision), finale
  Produktnamen-Entscheidung (`docs/naming-brainstorm.md`). Konzepte stehen im
  SOLL `feature/platform/mcp-server.md` (Abschnitt „Erweiterter MVP-Scope").

## Phase 8: Desktop-GUI

(UI-Konventionen F-100 sind spezifiziert, verifiziert und für jede GUI-Arbeit
verbindlich — normativ in `feature/platform/cli-gui-wasm.md`)

## Phase 9: Optionale zentrale Indizierung (Post-MVP)

- [ ] **F-064** Minimalen, vollständig wiederaufbaubaren Indexumfang festlegen:
  Pfad, Quellhash, Metadaten, Sidecarstatus, Jobstatus und Cacheverweise.
- [ ] **F-065** SQLite-Index als optionalen Adapter implementieren, ohne
  Rezeptdaten nur dort zu speichern.
- [ ] **F-066** Rebuild aus Sidecars, Aktualisierung, Locking und beschädigte
  DB behandeln.
- [ ] **F-067** Nachweisen, dass Löschen der DB keine Bearbeitungsdaten,
  virtuellen Kopien oder Masken zerstört.

## Phase 10: WASM und Plattformen (Post-MVP)

- [ ] **F-069** Browser-Dateiimport, temporären Speicher und Exportmodell
  definieren.
- [ ] **F-070** ONNX im Browser als optionale Fähigkeit mit klarer
  Capability-Anzeige behandeln.
- [ ] **F-071** native, Desktop- und Browser-Limits für Bildgröße, Speicher,
  Threads und GPU dokumentieren.

## Phase 11: Qualität, Performance und Release

**F-072** ist verifiziert erledigt und entfernt (CI existiert und läuft grün:
fmt, `cargo check --workspace --all-targets`, `cargo test --workspace
--all-targets` inkl. Golden-/Property-Tests (F-043), native zdata-Tests,
Clippy `-D warnings` (0 Fehler), wasm32-Checks lumina-core + lumina-gui,
Docs-Check — unabhängig verifiziert 2026-08-20). Bewusste Ausnahme: das
`onnx-rt`-Feature ist vom Clippy-Lauf ausgeschlossen (zieht `ort` →
`native-tls`/`openssl-sys`, im gepinnten CI-Container ohne libssl-dev nicht
baubar); der Pfad wird lokal gelintet. Bekannte benigne Warnung: `load_mask_planes`
dead-code unter wasm32 (optional stilllegen).

- [x] **F-073** Fixtures-Lizenzierung/Audit: **R1 geschlossen (2026-08-20)** —
  Autor via EXIF + Commit belegt, uneingeschränkte Nutzungs-/Distributions-
  gewährung für LuminaRust dokumentiert (`sample-data/raw/README.md`); R2
  (LIZ) offen — interim proprietär (Eigentümer-Antwort 2026-08-20); Audit-Doku
  existiert.
- [x] **F-078** Lizenz-/Modell-/Distributionsaudit: Belege in
  `THIRD-PARTY-NOTICES.md`/`fixtures-licensing.md` **unabhängig verifiziert
  (BESTANDEN, 2026-08-20)**; **Lensfun-LGPL-3.0-Eintrag ergänzt** (dynamisches
  Linken, CC-BY-SA-Datenbank, Quellangebot im Release-Bundle — F-098-N1);
  Pre-Release-Artefakte verbleiben offen (R3/R4): Lensfun-/LibRaw-Lizenztexte
  + Quellangebot + Modell-Lizenzen (BiRefNet Apache-2.0, ONNX Runtime MIT,
  SAM 2 bei F-082 zu verifizieren) im Release-Bundle mitliefern; R5
  (ADR-0002-Dreifachlizenz) und Lensfun-SPDX-/DB-Lizenz-Detail vor Final-
  Release klären.

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