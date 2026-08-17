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
- Codebestand: derzeit noch nicht initialisiert

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
  Versionen, Farbraum, Render-Keys und Cache-Invalidierung

### Produktfunktionen

- [`product/virtual-copies.md`](product/virtual-copies.md): virtuelle Kopien,
  Identität, Rezepttrennung und geteilte Artefakte
- [`product/ai-masks.md`](product/ai-masks.md): persistierte AI-Masken,
  Modellidentität, Status und lokale Anpassungen

### Plattformen

- [`platform/cli-gui-wasm.md`](platform/cli-gui-wasm.md): CLI, Desktop-GUI,
  WASM-Capabilities und optionale zentrale Indizierung

### Qualität

- [`quality/conflicts-and-acceptance.md`](quality/conflicts-and-acceptance.md):
  Konfliktmatrix, Abnahmeszenarien und Testanforderungen

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
| F-009 | Presets | [Virtual Copies](product/virtual-copies.md) | mittel |
| F-010 | CLI, GUI und WASM | [Plattformen](platform/cli-gui-wasm.md) | hoch |
| F-011 | Konflikt- und Releasequalität | [Qualität](quality/conflicts-and-acceptance.md) | hoch |
| F-012 | Benutzergeführte Segmentierung | [AI-Masks](product/ai-masks.md) | hoch |

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
- BiRefNet ist das erste automatische Subject-Modell; SAM 2 ist das erste
  interaktive Box-/Pinsel-Modell. Der ONNX-Adapter bleibt austauschbar.
- Es gibt in v1 keine zentrale Datenbank.
- Presets unterstützen absolute Werte sowie relative Exposure. Relative
  Exposure ist ohne aktiviertes Auto-Tone ungültig.
- Die optionale Reihenfolge lautet Source-Actions, Auto-WB/Auto-Tone, Preset,
  Masken, Matching.
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
