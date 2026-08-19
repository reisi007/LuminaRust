# LuminaRust Feature-SOLL

Diese Datei ist der Einstiegspunkt und Index für den gewünschten Produktzustand
von LuminaRust. Sie entspricht funktional einer `index.html` für die
Feature-Dokumentation: Erst hier orientieren, danach das relevante Dokument
öffnen.

## Inhaltsverzeichnis

- [Status](#status)
- [Leitbild](#leitbild)
- [Invarianten](#invarianten)
- [Dokumentationsstruktur](#dokumentationsstruktur)
- [Feature-Matrix](#feature-matrix)
- [Festgelegte Entscheidungen](#festgelegte-entscheidungen)
- [Arbeitsweise](#arbeitsweise)

## Status

- Status: **SOLL-Zustand, Version 0.1**
- Quelle: ursprüngliche SRS, erweitert um Sidecar-first-Persistenz und
  virtuelle Kopien
- Autorität: fachlicher Zielzustand für Implementierungs- und
  Verifizierungs-Agenten
- Codebestand: Raster-MVP in Arbeit — `lumina-sidecar`, `lumina-core`, `lumina-raw`,
  `lumina-cli` und `lumina-gui` vorhanden; Stand und offene Arbeit siehe
  `Agents.todo.md` (Stand 2026-08-18)

## Leitbild

LuminaRust soll ein nicht-destruktiver, Lightroom-ähnlicher RAW-Prozessor mit
Headless-CLI und optionaler Desktop-/Web-Oberfläche werden. Originale bleiben
unverändert. Bearbeitungen, virtuelle Kopien, Maskenreferenzen und relevante
Versionen werden portabel in Sidecars neben den Originalen gespeichert.

Eine zentrale Datenbank darf Suche, Index und Jobsteuerung beschleunigen, ist
aber weder Voraussetzung noch Quelle der Wahrheit. Ein Projekt muss aus seinen
Sidecars vollständig wiederherstellbar sein.

## Invarianten

- Originaldateien werden niemals verändert.
- Das Sidecar ist die autoritative Persistenz für Lumina-Bearbeitungen.
- Eine gültige persistierte AI-Maske wird wiederverwendet und nicht ungefragt
  neu berechnet.
- Virtuelle Kopien besitzen stabile IDs und eigene vollständige Rezepte.
- Veraltete oder fehlende Daten werden sichtbar gemeldet.
- Renderings und Exporte sind reproduzierbar aus Quelle, Rezept, Artefakten und
  Versionen ableitbar.
- Der optionale Index ist aus Sidecars vollständig neu aufbaubar.
- Plattformabhängige Implementierungen bleiben aus dem portablen Core heraus.

## Dokumentationsstruktur

### Architektur

- [`architecture/sidecar.md`](architecture/sidecar.md): Sidecar-Bundle,
  Manifest, Persistenz, Migration und Dateisicherheit
- [`architecture/pipeline.md`](architecture/pipeline.md): Renderpipeline,
  Versionen, Farbraum, Render-Keys, Cache-Invalidierung und
  Bearbeitungsregler F-089–F-099

### Produktfunktionen

- [`product/virtual-copies.md`](product/virtual-copies.md): virtuelle Kopien,
  Identität, Rezepttrennung, Standardkopie-Regeln (F-014) und geteilte Artefakte
- [`product/ai-masks.md`](product/ai-masks.md): persistierte AI-Masken,
  Modellidentität, Status und lokale Anpassungen
- [`product/export.md`](product/export.md): sRGB-, PNG-, JPEG- und WebP-Export,
  Bit-Tiefe, Qualität, Profile, Metadaten und Dithering (F-037)

### Plattformen

- [`platform/cli-gui-wasm.md`](platform/cli-gui-wasm.md): CLI, Desktop-GUI,
  verbindliche Lightroom-UI-Konventionen (F-100), WASM-Capabilities und
  optionale zentrale Indizierung
- [`platform/mcp-server.md`](platform/mcp-server.md): MCP AI-Agent-
  Schnittstelle für programmatischen Bildzugriff, Rezept-Bearbeitung und
  Schnellvorschau (F-101)

### Qualität

- [`quality/conflicts-and-acceptance.md`](quality/conflicts-and-acceptance.md):
  Konfliktmatrix, Abnahmeszenarien und Testanforderungen
- [performance-benchmarks.md](quality/performance-benchmarks.md):
  Performance-Methodik, Benchmark-Klassen, Baselines, Budgets und
  semi-automatische Regressionserkennung (F-074)
- [fixtures-licensing.md](quality/fixtures-licensing.md):
  Fixture-Inventar, Modell- und Abhängigkeitslizenzen, Distributionsaudit und
  Versionierungs-Policy (F-073, F-078)

## Feature-Matrix

| ID | Feature | Zieldokument | Risiko |
| --- | --- | --- | --- |
| F-001 | Sidecar-first | [Sidecar](architecture/sidecar.md) | hoch |
| F-002 | Virtuelle Kopien | [Virtual Copies](product/virtual-copies.md) | hoch |
| F-003 | Nicht-destruktive Entwicklung | [Pipeline](architecture/pipeline.md) | hoch |
| F-004 | Persistente AI-Masken | [AI-Masks](product/ai-masks.md) | hoch |
| F-005 | Cache und Render-Key | [Pipeline](architecture/pipeline.md) | mittel |
| F-006 | Optionale DB | [Plattformen](platform/cli-gui-wasm.md) | mittel |
| F-007 | RAW-Import | [Plattformen](platform/cli-gui-wasm.md) | hoch |
| F-008 | Auto-Tone und Exposure Matching | [Pipeline](architecture/pipeline.md) | mittel |
| F-036 | Weißabgleich und globale Tonwerte | [Pipeline](architecture/pipeline.md) | hoch |
| F-037 | Bildexport | [Export](product/export.md) | mittel |
| F-074 | Performance-Benchmarks | [Performance](quality/performance-benchmarks.md) | mittel |
| F-009 | Presets | [Virtual Copies](product/virtual-copies.md) | mittel |
| F-010 | CLI, GUI und WASM | [Plattformen](platform/cli-gui-wasm.md) | hoch |
| F-011 | Konflikt- und Releasequalität | [Qualität](quality/conflicts-and-acceptance.md) | hoch |
| F-012 | Benutzergeführte Segmentierung | [AI-Masks](product/ai-masks.md) | hoch |
| F-014 | Standardkopie-Regeln | [Virtual Copies](product/virtual-copies.md) | mittel |
| F-089 | Gradationskurve | [Pipeline](architecture/pipeline.md) | mittel |
| F-090 | HSL/Farbmischer | [Pipeline](architecture/pipeline.md) | mittel |
| F-091 | Color Grading | [Pipeline](architecture/pipeline.md) | mittel |
| F-092 | Dynamik und Sättigung | [Pipeline](architecture/pipeline.md) | mittel |
| F-093 | Zuschneiden und Drehen | [Pipeline](architecture/pipeline.md) | hoch |
| F-094 | Präsenz | [Pipeline](architecture/pipeline.md) | mittel |
| F-095 | Schärfen | [Pipeline](architecture/pipeline.md) | mittel |
| F-096 | Rauschreduzierung | [Pipeline](architecture/pipeline.md) | mittel |
| F-097 | Vignettierung und Körnung | [Pipeline](architecture/pipeline.md) | niedrig |
| F-098 | Objektivkorrekturen | [Pipeline](architecture/pipeline.md) | hoch |
| F-099 | Upright und Perspektive | [Pipeline](architecture/pipeline.md) | hoch |
| F-100 | Lightroom-UI-Konventionen | [Plattformen](platform/cli-gui-wasm.md) | mittel |
| F-101 | MCP AI-Agent-Schnittstelle | [MCP Server](platform/mcp-server.md) | mittel |

## Arbeitsweise

- Vor einer Implementierung muss das passende Zieldokument gelesen werden.
- Änderungen am Zielzustand werden zuerst im passenden Dokument und danach im
  Implementierungsplan eingetragen.
- Konflikte zwischen Dokumenten werden nicht durch stillschweigende
  Implementierung gelöst.
- Nach erfolgreicher Implementierung und unabhängiger Verifizierung wird der
  erreichte Zustand ergänzt.
- Offene Arbeit steht ausschließlich in `Agents.todo.md`.

## Festgelegte Entscheidungen

- Autoritative Arbeitsdateien pro Bild sind
  `<filename>.lumina.json` und `<filename>.lumina.zdata`.
- `.lumina.zdata` ist ein eigener, versionierter Container mit Zstd-
  komprimierten `uint16`-Maskenkacheln.
- Presets werden als einzelne `<name>.lumina-preset.json`-Dateien exportiert.
- XMP wird in v1 nicht unterstützt.
- Entwicklungshistorie ist persistent und kann für die ausgewählte virtuelle
  Kopie vollständig gelöscht werden.
- Maskenbibliotheken gehören zunächst zu virtuellen Kopien; Cross-Copy-
  Referenzen sind erlaubt und werden bei Löschung materialisiert.
- RAW wird zunächst über einen gekapselten LibRaw-Adapter gelesen.
- Der interne Arbeitsfarbraum ist im Raster-MVP sRGB-codiertes RGBA8; ein
  linearer ProPhoto-RGB-Arbeitsraum ist als Ziel reserviert (siehe
  `architecture/pipeline.md`).
- GUI-Technologie ist egui/eframe.
- **Pre-MVP-Schema-Entscheidung (2026-08-17):** Bis zum MVP ist das
  Sidecar-/Rezept-Schema bewusst nicht abwärtskompatibel — Altdateien müssen
  nicht lesbar bleiben. Die Migrations-Maschinerie bleibt dauerhaft im Code
  (v1→v2-Migration samt Tests aus F-089/F-090 als Muster); pre-MVP gibt es
  keinen Test-Zwang pro Migration — die Regel „Tests für jede Migration" gilt
  ab dem MVP.
- BiRefNet ist das erste automatische Subject-Modell; SAM 2 ist das erste
  interaktive Box-/Pinsel-Modell. Der ONNX-Adapter bleibt austauschbar.
- Es gibt in v1 keine zentrale Datenbank.
- Presets unterstützen absolute Werte sowie relative Exposure. Relative
  Exposure ist ohne aktiviertes Auto-Tone ungültig.
- Die optionale Reihenfolge lautet Source-Actions, Auto-WB/Auto-Tone, Preset,
  Masken, Matching.
- Die normativen Bearbeitungsregler F-089–F-099 sind in
  `architecture/pipeline.md` festgelegt. Der Raster-MVP verarbeitet sie im
  sRGB-codierten RGBA8-Arbeitsraum; Rezept- und Pipelineversion werden getrennt
  validiert.
- Die Adjustment-Unterstufen lauten im MVP: globale Tonwerte, Presence,
  Gradationskurve, HSL/Farbmischer, Dynamik/Sättigung, Color Grading,
  Rauschreduzierung und Schärfen. Geometrisch gilt:
  Objektivkorrektur → Perspektive → Crop → Rotation → Spiegelung.
- HSL verwendet acht getrennte Zentren: Rot, Orange, Gelb, Grün, Cyan, Blau,
  Violett und Magenta. Lensfun ist Post-MVP und erfordert Lizenz- sowie
  Capability-Prüfung gemäß F-078; KI-Denoise und automatische Upright-Analyse
  sind ebenfalls Post-MVP.
- Auto-WB, Auto-Tone und Auto-Exposure persistieren Ergebnis und
  Analysefingerprint.
- Der Raster-MVP misst sRGB-codierte RGBA8-RGB-Werte mit Rec.709-Gewichten,
  ignoriert Alpha und begrenzt Auto-/Matching-Exposure auf -10..=10 EV.
- Der schnelle Quell-Fingerprint wird nur bei kritischen Operationen durch
  einen vollständigen BLAKE3-Hash ergänzt.
- Output ist zunächst sRGB; weitere Profile werden im Modell vorbereitet.
- Migrationen erfolgen verzögert mit Bestätigung, Backup und atomarem Write;
  CLI benötigt dafür ein ausdrückliches Flag.
- Die GUI speichert standardmäßig Standard-Previews pro Quelle und virtueller
  Kopie. 1:1-Previews sind eine geerbte Ordneroption und standardmäßig aus.
- Auto-Synchronisation erfolgt als Auswahloperation ohne dauerhafte
  Gruppenverknüpfung. Masken werden als Absicht übertragen und pro Zielbild
  erzeugt.

Automatische Kategorien wie „Haare von Person 1“ oder „Haare aller Personen“
sind als spätere Instanz- und Teilsegmentierung vorgesehen. v1 konzentriert
sich bei benutzergeführter Segmentierung auf ein konkretes Objekt mit Box- oder
Pinsel-Prompt.
