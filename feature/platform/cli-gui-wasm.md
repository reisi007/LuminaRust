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

Der erste vertikale Raster-MVP stellt zusätzlich die direkt ausführbaren
Befehle `process` und `inspect` bereit. `process` verarbeitet aktuell PNG,
JPEG und WebP, liest optional ein `Preset`, lässt `--exposure` und `--contrast`
die Presetwerte überschreiben und schreibt Export sowie Sidecar jeweils atomar.
Diese beiden Einzeldatei-Writes bilden keine atomare Zwei-Dateien-Transaktion;
ein Abbruch zwischen ihnen kann daher einen bereits geschriebenen Export ohne
aktualisiertes Sidecar (oder umgekehrt) hinterlassen. `inspect` zeigt den
JSON-Status und die virtuellen Kopien ohne GUI. `inspect` zeigt auch Auto-Tone-
und Matching-Status. `process` akzeptiert `--auto-tone`,
`--match-total-exposure` und `--target-luminance 0..=1`; die Reihenfolge ist
Auto-Tone, Preset, CLI-Overrides, Masken später, Matching am finalen Rasterbild.

RAW ist ein verbindlicher MVP-Bestandteil. Der native LibRaw-Adapter unterstützt
CR2, CR3, NEF, ARW, DNG, ORF, RAF, RW2, CRW, PEF, SRW, 3FR, IIQ, RWL, MOS,
ERF, KDC und X3F einschließlich EXIF-Orientierung und überführt sie in denselben
Core-/CLI-/Desktop-Pfad. Browser/WASM unterstützt RAW nicht und meldet
`UnsupportedPlatform`; lokale LibRaw-/Dateisystemlogik wird dort nicht gebaut.
Der aktuelle Implementierungsstand enthält den Adapter, den gemeinsamen
CLI-/Desktop-Pfad und Fehler-/Capability-Tests. Ein echter Kamera-Golden-Test
bleibt bis zur Bereitstellung einer lizenzgeeigneten Fixture offen.

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

### Erster visueller User-Test

Das gemeinsame `lumina-gui`-Crate verwendet eframe nativ und als Trunk-WASM-
App. Die reproduzierbaren MVP-Befehle sind:

```bash
cargo run -p lumina-gui
cd crates/lumina-gui
trunk serve
trunk build --release
```

Die Oberfläche lädt PNG, JPEG und WebP sowie native RAW-Dateien per Pfad oder
Drag-and-drop. Browser/WASM bleibt RAW-frei und weist diese Capability klar aus. Preview, Exposure
(`-10..=10`) und Contrast (`-1..=1`) laufen über `lumina-core::ImageFrame` und
`lumina-sidecar::EditRecipe`. Native Sidecars werden neben dem Original
gespeichert; Browser-Dateispeichern ist im MVP noch nicht implementiert. ONNX,
Masken, Cache und Mehrbild-Synchronisierung bleiben ausdrücklich offen.

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
