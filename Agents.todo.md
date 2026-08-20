# LuminaRust Umsetzungsplan

Dieser Plan ist eine lebende Arbeitsliste. Er wird während der Implementierung
fortgeschrieben. Erledigte Aufgaben werden nach bestandener unabhängiger
Verifizierung und bestätigter Testabdeckung aus dieser Datei entfernt — es gibt
keine dauerhafte Liste abgehakter Aufgaben. Verifiziert Erledigtes steht unten
als kompakte Kurzliste; die Details liegen in den Feature-Dokumenten und der
Git-Historie.

## Stand (2026-08-20)

Offene Arbeit und Abgrenzung bis MVP:

- **LIZ-ENTSCHEIDUNG (offen, MVP-Release-Gate):** Projektlizenz ist offen —
  Projekt ist aktuell kommerziell (kein Open-Source-License); alle 8
  Workspace-Crates tragen bewusst kein `license`-Feld, keine `LICENSE`-Datei
  im Repo. Optionen zur MVP-Entscheidung: MIT / Apache-2.0 /
  Dual MIT+Apache-2.0 / MPL-2.0 / proprietär. **Erweiterung durch Lensfun
  (MVP):** Lensfun ist seit 2026-08-20 MVP-Teil (F-098-N1) — LGPL-3.0,
  Datenbank CC-BY-SA, dynamisch gelinkt; die Lizenzentscheidung muss das
  abdecken (F-078, F-098-N1).
- **F-098-N1** Lensfun-Integration als **Pre-MVP**-Bestandteil (Phase 3, aktiv
  in Bearbeitung). SOLL in `feature/architecture/pipeline.md` F-098 aktualisiert
  (2026-08-20): native Capability, LGPL-3.0/dynamisch, CC-BY-SA-Datenbank,
  graceful fallback auf das manuelle Modell.
- **F-082 / F-083** SAM-2-Segmentierungsadapter + Prompt-Roundtrip-Tests
  (Phase 6, nach Lizenz-/ONNX-Prüfung).
- **F-073** Fixtures-Lizenzierung/Audit (Phase 11): Audit-Dokumente existieren
  (`feature/quality/fixtures-licensing.md`, `docs/fixtures-and-licensing.md`);
  R1-Blocker = Provenienz der committeten `.cr3`-Fixtures
  (`sample-data/raw/aircraft-{landscape,portrait}.cr3`), R2 = LIZ-Entscheidung
  (oben).
- **F-078** Lizenz-/Modell-/Distributionsaudit (Phase 11): Audit-Dokumentation
  abgeschlossen (`THIRD-PARTY-NOTICES.md`, `fixtures-licensing.md`), Beleg
  LibRaw = einzige Pflicht-/Datenbank-Dependency, `ort`
  `=2.0.0-rc.13` optional hinter `onnx-rt`, ONNX Runtime MIT, BiRefNet
  Apache-2.0; **unabhängige Verifizierung offen**; um den Lensfun-LGPL-Eintrag
  zu ergänzen (hält das MVP-Gate).
- **F-072** CI-Gates: unabhängig verifiziert **bestanden** (2026-08-20) —
  verbleibender Aufwand nur Eintrag-Nachpflege (siehe Phase 11).

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

- [ ] **F-074-A1** Hotspot-Optimierung: `apply_recipe_with_white_balance`-
  Adjustments-Kernel (WB + pro-Pixel-Regler) beschleunigen — ~91 % von
  `render_frame` (interaktiver Pfad).
- [ ] **F-074-A2** `decode/raw`-Durchsatz verbessern (LibRaw-Overhead,
  Decode/Upload-Überlappung, Decodepuffer-Cache) — ~3,0–3,4× `render_frame`.
- [ ] **F-074-A3** Auto-Tone-/Exposure-Match-Analyse-Kernel optimieren
  (geteilte Histogramm/Perzentil-Statistik) — ~64 % von `render_frame`.
- [ ] **F-074-A4** PNG-Export-Encode-Durchsatz verbessern (Δ `batch` −
  `render_frame`) — ~56 % von `render_frame` / ~36 % von `batch`.

Verifiziert erledigt und entfernt: F-074-N1 (Methodik-SOLL, ADR 0003),
F-074-N2 (Setup-Gerüst), F-074-N3 (erste echte Benchmarks: 32 deterministische
Fixtures, Baseline-Erfassung, `compare.mjs`-Bestandsvalidierung), F-074-N4
(Baseline-Analyse: Hotspot `apply_recipe` ~91 %, abgeleitete IDs A1…A4),
F-074-N5 (`scripts/perf/compare.mjs` report/warn/gate, optionaler
nicht-blockierender CI-`bench`-Job im `warn`-Modus) — jeweils unabhängig
verifiziert.

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

- [ ] **F-098-N1** Lensfun-Integration als **Pre-MVP**-Bestandteil (SOLL:
  pipeline.md F-098, aktualisiert 2026-08-20).
  Umfang:
  - Neues natives Crate `crates/lumina-lensfun` (Workspace-Member, **nicht** in
    den wasm32-Checks): FFI-Bindung an System-`liblensfun` via `pkg-config`
    (dynamisch gelinkt, **LGPL-3.0**, Datenbank CC-BY-SA) + Safe Wrapper
    (DB laden, Modifier für Kamera/Objektiv/Brennweite/Blende/Distanz finden,
    Geometrie-/Farb-Mapping).
  - `lumina-core`: optionales Feature `lensfun` (default **off**; Dependency
    auf lumina-lensfun nur bei Feature → nativer Default-Build und wasm32
    bleiben grün). `apply_geometry` nutzt den Modifier für Verzeichnung +
    Vignette, wenn vorhanden (Priorität: Lensfun > manuelle Koeffizienten >
    Identität; graceful fallback). CA bleibt im manuellen Modell
    (dokumentierte MVP-Grenze).
  - Verdrahtung CLI/RAW: `RawMetadata.camera_make`/`camera_model` reichen bis
    zum Render-Aufruf; Modifier nur aufbauen, wenn Feature an + Profil
    gefunden.
  - Tests: Fallback-Pfad (Feature off), Profil-gestützte Korrektur mit echter
    liblensfun (Feature on), Fehlerfälle; CI-Container (`ci-libraw-image.yml`)
    um `liblensfun` erweitern.
  Abnahme: Default-Build + wasm32 + gesamte Test-Suite grün; Feature-Build
  grün; unabhängige Verifizierung durch `general`-Agenten.

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

- [ ] **F-073** Fixtures-Lizenzierung/Audit (R1: .cr3-Provenienz offen; R2:
  LIZ-Entscheidung; Audit-Doku existiert).
- [ ] **F-078** Lizenz-/Modell-/Distributionsaudit abschließen: Belege in
  `THIRD-PARTY-NOTICES.md`/`fixtures-licensing.md` von unabhängigem Agenten
  verifizieren lassen; **Lensfun-LGPL-3.0-Eintrag ergänzen** (dynamisches
  Linken, CC-BY-SA-Datenbank, Quellangebot im Release-Bundle — F-098-N1);
  Pre-Release-Bundle: Lensfun-/LibRaw-Lizenztexte + Modell-Lizenzen
  (BiRefNet Apache-2.0, ONNX Runtime MIT) mitliefern (R3/R4).

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