# Plattformen und optionale Indizierung

**Features:** F-006 Optionale DB, F-007 RAW-Import, F-010 CLI, GUI und WASM

## Inhaltsverzeichnis

- [Gemeinsame Grenze](#gemeinsame-grenze)
- [CLI](#cli)
- [Desktop-GUI](#desktop-gui)
- [WASM](#wasm)
- [Optionale zentrale Indizierung](#optionale-zentrale-indizierung)
- [Abnahme](#abnahme)

## Gemeinsame Grenze

`lumina-core` bleibt plattformneutral. RAW-Decoder, ONNX-Runtime,
Dateisystemzugriff, GPU und Parallelisierung werden über Adapter angebunden.
CLI und GUI verwenden dieselbe Rezept- und Renderlogik.

Der ONNX-Adapter muss sowohl automatische Modelle als auch interaktive
Segmentierungsmodelle mit Box-, Punkt- und Masken-Prompts unterstützen können.
Die konkrete Modellfähigkeit wird aus dem Modellmanifest gelesen und nicht aus
dem Modellnamen erraten.

## Mehrbildbearbeitung

Die GUI unterstützt auswahlbasierte Bearbeitungsbefehle ohne dauerhafte
Gruppenverknüpfung. Jede ausgewählte Datei erhält ein eigenes Sidecar und ein
eigenes Rezept. Globale Werte, Auto-Regeln, Source-Actions und Maskenabsichten
werden als Operation auf die Auswahl angewendet.

Eine AI-Matte wird pro Zielbild als `missing` oder `pending` geführt und kann in
der Idle-Queue berechnet werden. Die aktuelle aktive Vorschau blockiert diese
Hintergrundjobs nicht. Das Erzeugen einer Maske für ein Zielbild bleibt über
`--update-masks` beziehungsweise die GUI-Aktion explizit steuerbar.

Die langfristige Struktur sieht `lumina-sidecar` als verpflichtendes Modul und
`lumina-index` als optionalen, wiederaufbaubaren Adapter vor.

## CLI

Die CLI soll mindestens `import`, `inspect`, `develop`, `render`, `export`,
`batch`, `mask`, `reindex` und `validate` unterstützen. Einzel- und
Batchverarbeitung arbeiten ohne GUI und ohne zentrale DB.

Batchjobs benötigen Resume, Retry, Dry-Run, begrenzten Speicher, reproduzierbare
Exit-Codes und strukturierte Ausgabe. Optionen für virtuelle Kopie,
Masken-Neuberechnung und Render-Cache werden explizit angeboten.

## Desktop-GUI

Die GUI zeigt Datei-, Sidecar-, Offline-, Masken- und Konfliktstatus. Vorschau
und Histogramm gehören zu einem konkreten Renderstand. Veraltete parallele
Ergebnisse dürfen nicht als aktuell erscheinen.

Regler, Presets, Auto-Tone, virtuelle Kopien, Masken und Exporte ändern nur das
deklarative Rezept und schreiben anschließend das Sidecar.

Beim Verlassen eines Bildes wird standardmäßig nur die aktuelle Standard-
Vorschau je Quelle und virtueller Kopie gespeichert. Eine 1:1-Vorschau ist
optional, wird in `.lumina/settings.json` auf Ordnerebene gespeichert und von
übergeordneten Ordnern geerbt. `.lumina/` enthält ausschließlich löschbaren
Cache und Einstellungen, keine autoritativen Rezepte.

Für v1 ist egui/eframe festgelegt. Tauri ist keine v1-Abhängigkeit und kann in
einer späteren Architekturentscheidung erneut bewertet werden.

## WASM

WASM unterstützt zunächst den portablen Core und klar begrenzte Preview-
Szenarien. Browser-Dateiimport, Speicherlimits, Export und GPU werden getrennt
behandelt. Native RAW- und ONNX-Backends gelten nicht automatisch als
browserfähig.

Eine Capability-Matrix dokumentiert native CLI, Desktop und Browser getrennt.

## Optionale zentrale Indizierung

Die DB darf nur Pfade, Quellhashes, Metadaten, Sidecarstatus, Jobstatus,
Cacheverweise und Konflikte indizieren. Rezepte, virtuelle Kopien,
Maskenmetadaten und Maskenartefakte bleiben im Sidecar-Bundle.

Löschen und anschließender Reindex müssen alle Bearbeitungen aus den Sidecars
wiederherstellen können.

## Abnahme

- Sidecar-only-CLI funktioniert ohne DB.
- GUI und CLI erzeugen dasselbe Rezeptmodell.
- Der Index kann gelöscht und aus Sidecars neu erstellt werden.
- WASM-Build und native Capability-Grenzen sind dokumentiert.
