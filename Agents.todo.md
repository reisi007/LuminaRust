# LuminaRust Umsetzungsplan

Dieser Plan ist eine lebende Arbeitsliste. Er wird während der Implementierung
fortgeschritten. Erledigte Aufgaben werden nach bestandener unabhängiger
Verifizierung und bestätigter Testabdeckung aus dieser Datei entfernt. Es gibt
keine dauerhafte Liste abgehakter Aufgaben.

## Stand (2026-08-18)

Offene Arbeit und Abgrenzung — verifiziert abgeschlossene Aufgaben sind aus
dieser Datei entfernt (siehe Git-Historie und Feature-Dokumente):

- **F-072-N2** neu ergänzt: vorbestehender wasm32-Fehler in `lumina-gui`
  (dokumentierte Baseline aus Verifikation F-072-N1).
- **F-036-N1** war in Vorbereitung, wurde auf Anweisung nicht delegiert; keine
  offenen Arbeitsbaum-Änderungen (main sauber).

Verbleibend bis MVP: F-036-N1, F-042, F-085, F-097 (Phase 3/4), F-041, F-043
(Phase 5), Phase 6 AI-Masken (F-047…F-083), Release-Gates (F-072, F-073…F-078,
F-072-N2). Post-MVP: F-019, Phase 9 (F-064…F-067), WASM-Browser (F-069, F-070).

## Inhaltsverzeichnis

- [Stand](#stand-2026-08-18)
- [Arbeitsregeln](#arbeitsregeln)
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

- [ ] **F-085** Source-Action-Operationen, ihre History-Schritte und ihre
  Auswirkung auf Auto-WB, Auto-Tone und Exposure Matching testen
  (Reihenfolge-Test verifiziert; behaviorale Tests folgen mit F-036/F-042).
- [ ] **F-097** (niedrige Priorität) Deterministische Vignettierung und Körnung
  mit RenderKey-abgeleitetem Seed umsetzen.

## Phase 4: RAW-Verarbeitung

Diese Phase ist ein verbindliches MVP-Gate. Der erste User-Test gilt erst als
produktseitig vollständig, wenn native RAW-Dekodierung, Orientierung und die
minimalen RAW-Golden-Tests vorhanden sind. **MVP-Grenze (2026-08-17):** Das MVP
umfasst CLI und native Desktop (inkl. RAW). Web/WASM-RAW ist aus dem MVP
geschoben (Post-MVP via `libraw-wasm`, Feature `wasm-js`), die Architektur wird
aber kompatibel gehalten (einheitlicher `decode_bytes`/`RawMetadata`-Vertrag,
`cfg(target_arch = "wasm32")`-Kapselung).

- [ ] **F-036-N1** As-Shot-Weißabgleich anbinden: `RawMetadata.camera_white_balance`
  (cam_mul `[f32;4]`) über eine Core-API an `apply_recipe` durchreichen (kein
  stiller Fallback; ohne Kontext bleibt Identitäts-Semantik).

## Phase 5: Auto-Tone und Exposure Matching

- [ ] **F-041** `Match Total Exposure` auf den finalen sichtbaren Messbereich
  nach Crop, Geometrie und aktiven Masken erweitern; Epsilon-, Clipping- und
  Fallback-Schutz sind im aktuellen Raster-Messbereich vorhanden.
- [ ] **F-042** Auto-WB, Source-Actions und lokale Masken in die dokumentierte
  Reihenfolge integrieren; die aktuelle Raster-Reihenfolge ist festgelegt.
- [ ] **F-043** Echte Property-Tests und Referenzbildtests für Auto-Tone und
  Exposure Matching ergänzen; deterministische Invariantentests sind vorhanden.

## Phase 6: Persistente AI-Masken

- [ ] **F-047** Austauschbaren ONNX-Inferenzadapter mit BiRefNet als erstem
  automatischen Subject-Modell integrieren, ohne den WASM-kompatiblen Core zu
  belasten.
- [ ] **F-048** Persistierte Masken bevorzugt laden; Neuberechnung nur bei
  fehlender, veralteter oder ausdrücklich erneuerter Maske durchführen.
- [ ] **F-049** Masken-Invertierung, Feathering, Blur und lokale Anpassungen
  nicht-destruktiv in der Pipeline anwenden.
- [ ] **F-050** Tests für fehlende Artefakte, Modellwechsel, Quelländerung,
  falsche Prüfsumme und erneute Inferenz schreiben.
- [ ] **F-051** Verhalten bei nicht verfügbarem Modell definieren: persistierte
  Maske weiterverwenden, Neuberechnung anbieten und Fehler sichtbar machen.
- [ ] **F-079** Promptfähige Maskenquellen für Box, Pinsel, Polygon, Ellipse und
  Verläufe in das Masken-DAG-Modell aufnehmen.
- [ ] **F-080** Modellfähigkeiten wie `box_prompt`, `point_prompt`,
  `mask_prompt`, `class_detection` und `instance_segmentation` im ONNX-
  Manifest und Adaptermodell abbilden.
- [ ] **F-081** Benutzergeführte Segmentierung für Rechteck- und Pinsel-Prompts
  spezifizieren; Prompt-Transformationen und Koordinatensysteme persistieren.
- [ ] **F-082** Einen ersten interaktiven Segmentierungsadapter, vorzugsweise
  SAM 2 nach Lizenz- und ONNX-Prüfung, auswählen und integrieren.
- [ ] **F-083** Prompt-Roundtrip-, Modellfähigkeits-, Re-Run- und
  nicht-unterstützter-Prompt-Tests ergänzen.

## Phase 7: CLI und Batch

(alle Punkte umgesetzt und verifiziert — 2026-08-17)

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
  Property- und CLI-Tests einrichten.
- [ ] **F-072-N2** Vorbestehenden wasm32-Fehler in lumina-gui beheben:
  `LuminaApp::new()` setzt `mask_name_input: String::new()` ungated, das
  Struct-Feld ist aber `#[cfg(not(target_arch = "wasm32"))]`-gated →
  `error[E0560]` unter `cargo check -p lumina-gui --target
  wasm32-unknown-unknown`. Entweder Konstruktor-Zeile gaten oder Feld für
  wasm32 bereitstellen (Verifikation F-072-N1, dokumentierte Baseline).
- [ ] **F-073** Kleine versionierte Referenzbilder, RAW-Fixtures und Modelle
  einschließlich Lizenzinformationen bereitstellen.
- [ ] **F-074** Benchmarks für Decode, Preview, Maskeninferenz, Cache-Hit und
  Batch-Export definieren.
- [ ] **F-075** Speicherbudgets und Abbruchverhalten für große RAWs und Masken
  messen und absichern.
- [ ] **F-076** Rezept-, Sidecar- und Pipeline-Migrationsstrategie für Releases
  dokumentieren.
- [ ] **F-077** Backup-, Recovery-, Sidecar-Konflikt- und Datenverlusttests als
  Release-Gate einrichten.
- [ ] **F-078** Lizenz-, Modell- und Distributionsprüfung vor dem ersten Release
  abschließen.

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
