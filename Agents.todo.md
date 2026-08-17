# LuminaRust Umsetzungsplan

Dieser Plan ist eine lebende Arbeitsliste. Er wird während der Implementierung
fortgeschrieben. Erledigte Aufgaben werden nach bestandener unabhängiger
Verifizierung und bestätigter Testabdeckung aus dieser Datei entfernt. Es gibt
keine dauerhafte Liste abgehakter Aufgaben.

## Inhaltsverzeichnis

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
- Der unabhängige Verifizierungs-Agent muss Korrektheit und Testabdeckung
  bestätigen, bevor die Aufgabe aus dieser Datei entfernt wird.
- Eine fehlgeschlagene Verifizierung lässt die Aufgabe offen und erzeugt eine
  konkrete Folgeaufgabe.

## Phase 0: Zielzustand und Architektur

(alle Punkte umgesetzt und verifiziert — 2026-08-17)

## Phase 1: Sidecar-Domain-Modell

- [ ] **F-014** Regeln für Standardkopie, Umbenennen, Sortieren, Duplizieren,
  Löschen und Wiederherstellen virtueller Kopien spezifizieren.

## Phase 2: Rezept, virtuelle Kopien und Migrationen

- [ ] **F-019** Sidecar-Migrationen, unbekannte Felder, inkompatible Versionen
  und Downgrade-Verhalten definieren.
- [ ] **F-021** Atomare Schreibvorgänge, temporäre Dateien, Crash-Recovery und
  parallele Schreibkonflikte implementieren und testen.
- [ ] **F-023** Konfliktauflösung für extern veränderte Sidecars, verschobene
  Originale, fehlende Artefakte und beschädigte JSON-Dateien implementieren.

## Phase 3: Renderpipeline und Cache

- [ ] **F-029** Sicherstellen, dass reine Crop-, UI- oder Ausgabeänderungen
  keine unnötige RAW-Dekodierung oder AI-Inferenz auslösen.
- [ ] **F-031** Determinismusregeln zwischen CPU, GPU und optionalen Backends
  festlegen und durch Referenztests absichern.
- [ ] **F-085** Source-Action-Operationen, ihre History-Schritte und ihre
  Auswirkung auf Auto-WB, Auto-Tone und Exposure Matching testen.
- [ ] **F-086-N1** Test für partielle Vererbung von `.lumina/settings.json`
  ergänzen: Kind-JSON mit nur einem gesetzten Feld erbt das andere Feld vom
  Eltern-JSON (Code in `cache/disk.rs` implementiert das bereits korrekt).
- [ ] **F-089** Gradationskurve mit Master- und getrennten RGB-Kanalkurven,
  monotoner Interpolation, Versionierung und Cache-Tests umsetzen.
- [ ] **F-090** HSL/Farbmischer mit acht sRGB-Kanälen, Nachbargewichtung und
  Roundtrip-/Wertebereichstests umsetzen.
- [ ] **F-091** Color Grading für Schatten, Mitteltöne, Lichter und Balance mit
  weichen Bereichsgewichten umsetzen.
- [ ] **F-092** Dynamik- und lineare Sättigungsregler einschließlich
  Schutzlogik für schwache/bereits gesättigte Farben umsetzen.
- [ ] **F-093** Vollwertige Crop-/Dreh-/Spiegelungsgeometrie, RenderKey-
  Invalidierung und finale F-041-Messdomäne umsetzen.
- [ ] **F-094** Presence-Regler für Texture, Clarity und Dehaze mit
  dokumentierten MVP-Heuristiken und Reihenfolgentests umsetzen.
- [ ] **F-095** Lightroom-artiges Unsharp-Mask-Schärfen mit Luminanz-Maskierung
  und skalierungsabhängigem Radius umsetzen.
- [ ] **F-096** Manuelle Luminanz-/Farbrauschreduzierung vor dem Schärfen
  umsetzen; KI-Denoise als spätere Option offenhalten.
- [ ] **F-098** Manuelle Objektivkorrektur mit Radialmodell, Vignette und
  R-/B-Kanal-CA umsetzen; Lensfun nur als Post-MVP prüfen.
- [ ] **F-099** Manuelle Upright-/Perspektivkorrektur als Homographie vor Crop
  umsetzen; automatische Linienanalyse als Post-MVP offenhalten.
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

- [ ] **F-036** Weißabgleich, lineare Datenrepräsentation, Belichtung,
  Kontrast, Highlights, Shadows, Whites und Blacks implementieren.
- [ ] **F-037** sRGB-, PNG-, JPEG- und WebP-Export mit Bit-Tiefe,
  Qualitätswerten, Profilen, Metadaten und Dithering definieren.

## Phase 5: Auto-Tone und Exposure Matching

- [ ] **F-039** Eine explizite Histogramm-Repräsentation ergänzen; die aktuelle
  RGBA8-/Rec.709-Messdomäne, Gewichtung und Perzentilinterpolation sind für den
  Raster-MVP festgelegt.
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

- [ ] **F-088** Idle-Queue mit Opt-out für fehlende AI-Masken sowie Warnungen und
  `--update-masks`-Verhalten vor dem Export implementieren.
- [ ] **F-061-N1** Roundtrip-Test für `set_mask_local_adjustment` ergänzen
  (Extras via `adjustment_*`-Keys in `MaskLayer.extras` persistieren und laden).

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
