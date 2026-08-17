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

- [ ] **F-001** Produktumfang für die erste Version verbindlich festlegen:
  Katalogumfang, Import, Entwicklung, Export, virtuelle Kopien, Masken,
  Historie und Offline-Status.
- [ ] **F-002** Die Dokumente unter `feature/` als normative Featurequelle
  gegen die ursprüngliche SRS prüfen und widersprüchliche Anforderungen
  markieren oder korrigieren.
- [ ] **F-003** Sidecar-first-Entscheidung als Architekturentscheidung
  dokumentieren: Sidecar ist autoritativ, Datenbank bleibt optional und
  wiederaufbaubar.
- [ ] **F-004** Sidecar-Bundle, Dateinamensregeln, relative Artefaktpfade,
  Atomic Writes und Recovery-Verhalten festlegen.
- [ ] **F-005** Arbeitsfarbraum, Pipeline-Reihenfolge, Bit-Tiefen, Clipping,
  Transferfunktionen und Farbprofilstrategie normativ spezifizieren.
- [ ] **F-006** Native-, Desktop- und WASM-Capability-Matrix erstellen.
- [ ] **F-007** RAW-Backend, ONNX-Backend, GUI-Framework und Lizenzbedingungen
  bewerten und jeweils eine begründete Entscheidung treffen.
- [ ] **F-008** Workspace-Struktur um ein verpflichtendes
  `lumina-sidecar`-Modul und gegebenenfalls ein getrenntes optionales
  Index-Modul ergänzen.
- [ ] **F-009** ADR-Struktur und Entscheidungsworkflow für spätere Änderungen
  an Schema, Pipeline und Plattformgrenzen einrichten.

## Phase 1: Sidecar-Domain-Modell

- [ ] **F-011** Quellidentität mit Content-Hash, Dateigröße, Änderungszeit,
  Format, Orientierung und relevanten Decode-Parametern definieren.
- [ ] **F-012** Vollständiges Sidecar-Schema für eine einzelne RAW-Datei
  entwerfen und mit einem Beispiel dokumentieren.
- [ ] **F-014** Regeln für Standardkopie, Umbenennen, Sortieren, Duplizieren,
  Löschen und Wiederherstellen virtueller Kopien spezifizieren.
- [ ] **F-015** Rezeptmodell so definieren, dass jede virtuelle Kopie ein
  selbständig auswertbares, nicht-destruktives Rezept besitzt.
- [ ] **F-016** Rezept- und Pipelineversion getrennt modellieren.

## Phase 2: Rezept, virtuelle Kopien und Migrationen

- [ ] **F-018** Einen großen Sidecar-Roundtrip mit realistischem Datenumfang
  und Speicher-/Performancegrenzen ergänzen.
- [ ] **F-019** Sidecar-Migrationen, unbekannte Felder, inkompatible Versionen
  und Downgrade-Verhalten definieren.
- [ ] **F-020** Virtuelle-Kopien-Roundtrip-, Duplikations-, Umbenennungs- und
  unabhängige-Rezept-Tests implementieren.
- [ ] **F-021** Atomare Schreibvorgänge, temporäre Dateien, Crash-Recovery und
  parallele Schreibkonflikte implementieren und testen.
- [ ] **F-022** Dokumentieren und testen, dass XMP in v1 nicht unterstützt wird;
  einen späteren XMP-Adapter außerhalb des v1-Scopes planen.
- [ ] **F-023** Konfliktauflösung für extern veränderte Sidecars, verschobene
  Originale, fehlende Artefakte und beschädigte JSON-Dateien implementieren.

## Phase 3: Renderpipeline und Cache

- [ ] **F-024** Deklarative Pipeline mit stabiler Reihenfolge und expliziten
  Eingangs-/Ausgangsformaten spezifizieren.
- [ ] **F-025** Pipeline als gemeinsam verwendbare Core-API implementieren;
  GUI und CLI dürfen keine eigene Pixelpipeline enthalten.
- [ ] **F-026** `RenderKey` aus Quell-Hash, Decode-/Pipeline-Version,
  Rezept-Hash, Masken-Hashes, Farbprofil, Zielgröße und Ausgabeformat bilden.
- [ ] **F-027** Cache-Stufen für Decode, Preview, Histogramm, Masken und Export
  sowie deren Invalidierungsabhängigkeiten dokumentieren.
- [ ] **F-028** Cache mit atomaren Artefakten, Prüfsummen, Größenlimit,
  Bereinigung und Abbruchverhalten implementieren.
- [ ] **F-029** Sicherstellen, dass reine Crop-, UI- oder Ausgabeänderungen
  keine unnötige RAW-Dekodierung oder AI-Inferenz auslösen.
- [ ] **F-030** Stale-Result-Erkennung für parallele Preview- und Exportjobs
  implementieren.
- [ ] **F-031** Determinismusregeln zwischen CPU, GPU und optionalen Backends
  festlegen und durch Referenztests absichern.
- [ ] **F-084** Nicht-destruktive Source-Actions für Staubentfernung und
  spätere KI-Teil-Ersetzung vor Auto-Analyse und Maskenanwendung modellieren.
- [ ] **F-085** Source-Action-Operationen, ihre History-Schritte und ihre
  Auswirkung auf Auto-WB, Auto-Tone und Exposure Matching testen.
- [ ] **F-086** Ordner-Cache unter `.lumina/` mit geerbter `settings.json`,
  Standardvorschau, optionaler 1:1-Vorschau und sofortigem Prune verwaister
  Einträge implementieren.

## Phase 4: RAW-Verarbeitung

Diese Phase ist ein verbindliches MVP-Gate. Der erste User-Test gilt erst als
produktseitig vollständig, wenn native RAW-Decodierung, Orientierung und die
minimalen RAW-Golden-Tests vorhanden sind. WASM bleibt für RAW ausdrücklich
außerhalb des Scopes.

- [ ] **F-032** Unterstützte RAW-Formate, Kamera-Fixtures, Metadatenfelder und
  Fehlerverhalten definieren.
- [ ] **F-033** RAW-Backend integrieren und von `lumina-core` entkoppeln.
- [ ] **F-034** EXIF, Orientierung, Kamera-Farbmatrix und relevante Profile
  extrahieren und persistierbar machen.
- [ ] **F-035** Demosaicing-Strategie mit Qualitäts-, Speicher- und
  Performancekriterien implementieren.
- [ ] **F-036** Weißabgleich, lineare Datenrepräsentation, Belichtung,
  Kontrast, Highlights, Shadows, Whites und Blacks implementieren.
- [ ] **F-037** sRGB-, PNG-, JPEG- und WebP-Export mit Bit-Tiefe,
  Qualitätswerten, Profilen, Metadaten und Dithering definieren.
- [ ] **F-038** RAW-Golden-Tests, Orientation-Tests und fehlerhafte/teilweise
  lesbare Datei-Tests ergänzen.

## Phase 5: Auto-Tone und Exposure Matching

- [ ] **F-039** Messdomäne für Luminanz, Gewichtung, Histogramm und Perzentile
  eindeutig festlegen.
- [ ] **F-040** Auto-Tone-Analyse und Begrenzungen für EV, Kontrast,
  Highlights und Shadows implementieren.
- [ ] **F-041** `Match Total Exposure` mit Zielbereich, Schutz vor Division durch
  null, Clipping-Grenzen und Fallbacks implementieren.
- [ ] **F-042** Optionale Reihenfolge von Auto-WB/Auto-Tone, Preset, lokalen
  Masken und Match Total Exposure als reproduzierbare Regel implementieren.
- [ ] **F-043** Mathematische Unit-Tests, Property-Tests und Referenzbildtests
  für Auto-Tone und Exposure Matching schreiben.

## Phase 6: Persistente AI-Masken

- [ ] **F-044** Union, Intersect, Subtract und Invert als auswertbare Operatoren
  auf dem bereits validierten Masken-DAG implementieren.
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

- [ ] **F-052** CLI-Befehle für `import`, `inspect`, `develop`, `render`,
  `export`, `batch`, `mask`, `reindex` und `validate` definieren.
- [ ] **F-053** Einzelbildverarbeitung mit Sidecar-Laden, virtuelle-Kopie-
  Auswahl, Render-Key und Export implementieren.
- [ ] **F-054** Batch-Verarbeitung mit Rayon, begrenztem Speicher, Resume,
  Retry, Dry-Run und reproduzierbaren Exit-Codes implementieren.
- [ ] **F-055** CLI-Optionen für `--update-masks`, `--migrate`,
  `--force-render`, `--virtual-copy`, `--format`, `--quality` und
  strukturierte Ausgabe testen.
- [ ] **F-056** CLI-End-to-End-Tests für Sidecar-only-Betrieb ohne zentrale DB
  ergänzen.

## Phase 8: Desktop-GUI

- [ ] **F-057** egui/eframe-GUI-Grundgerüst und gemeinsame Jobsteuerung
  implementieren.
- [ ] **F-058** Datei-Browser mit Sidecar-Status, Offline-Status,
  Konfliktstatus und virtuellen Kopien implementieren.
- [ ] **F-059** Vorschau und Histogramm an einen konkreten Renderstand koppeln;
  veraltete Ergebnisse dürfen nicht sichtbar als aktuell erscheinen.
- [ ] **F-060** Regler, Auto-Tone, Exposure Matching und Preset-Anwendung als
  nicht-destruktive Rezeptänderungen implementieren.
- [ ] **F-061** Maskenwerkzeuge für Auswahl, Benennung, Invertierung,
  Feathering, lokale Anpassungen, Speichern und Neuberechnung implementieren.
- [ ] **F-062** Preset-Creator mit ausgewählten Feldern, Validierung,
  Versionierung und virtueller-Kopie-Anwendung implementieren.
- [ ] **F-063** GUI-Tests für Sidecar-Schreiben, Wiederöffnen, Kopien und
  fehlende Maskenmodelle ergänzen.
- [ ] **F-087** Auswahlbasierte Mehrbildänderungen ohne dauerhafte
  Gruppenverknüpfung implementieren; Maskenabsichten werden pro Zielbild als
  `missing`/`pending` geführt.
- [ ] **F-088** Idle-Queue mit Opt-out für fehlende AI-Masken sowie Warnungen und
  `--update-masks`-Verhalten vor dem Export implementieren.
- [ ] **F-089** Preset-Dateien mit explizit auswählbaren Feldern, absoluter und
  relativer Semantik sowie einem neuen History-Schritt beim Anwenden
  implementieren.

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

- [ ] **F-068** WASM-kompatiblen Core-Build in CI einrichten.
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
