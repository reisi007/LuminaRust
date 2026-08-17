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
JPEG und WebP, liest optional ein `Preset`, lässt `--exposure`, `--contrast`,
`--highlights` und `--shadows` die Presetwerte überschreiben und schreibt Export
sowie Sidecar jeweils atomar.
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
Core-/CLI-/Desktop-Pfad. Der aktuelle Implementierungsstand enthält den Adapter, den gemeinsamen
CLI-/Desktop-Pfad und Fehler-/Capability-Tests. Ein echter Kamera-Golden-Test
bleibt bis zur Bereitstellung einer lizenzgeeigneten Fixture offen.

**MVP-Grenze (Stand 2026-08-17):** Das MVP umfasst **CLI und native Desktop**
(auch RAW). Der **Web/WASM-Teil ist aus dem MVP geschoben** und wird später
umgesetzt. Die Architektur bleibt aber bewusst kompatibel: `lumina-raw` kapselt
den LibRaw-Zugriff hinter einem einheitlichen `decode_bytes`/`RawMetadata`-
Vertrag, sodass ein späteres WASM-Backend (`libraw-wasm`, Emscripten/npm, Feature
`wasm-js`) ohne API-Änderung andocken kann. WASM-spezifische Pfade sind bereits
per `cfg(target_arch = "wasm32")` gekapselt; der native LibRaw-Adapter bleibt
Default für CLI/Desktop.

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

> **Implementierungsstatus (F-086, 2026-08-17):** Umgesetzt und unabhängig
> verifiziert. Die `.lumina/`-Disk-Schicht (settings.json inkl.
> Eltern-Vererbung, Preview-Ablage, Prune) liegt in `lumina-core`
> (`DiskFolderCache`), WASM-gekapselt; siehe feature/architecture/pipeline.md.

Für v1 ist egui/eframe festgelegt. Tauri ist keine v1-Abhängigkeit und kann in
einer späteren Architekturentscheidung erneut bewertet werden.

## WASM

WASM unterstützt zunächst den portablen Core und klar begrenzte Preview-
Szenarien. Browser-Dateiimport, Speicherlimits, Export und GPU werden getrennt
behandelt. Native RAW- und ONNX-Backends gelten nicht automatisch als
browserfähig.

**WASM-RAW-Backend (post-MVP, vorbereitet):** Für die spätere Browser-Anbindung
ist `libraw-wasm` (Emscripten/npm) vorgesehen. Die Rust-Seite würde die JS-
`LibRaw`-Klasse als `wasm-bindgen`-Extern deklarieren und `open`/`metadata`/
`imageData` in `lumina-raw::decode_bytes` bzw. `RawMetadata` übersetzen. Das
Backend ist hinter dem Feature `wasm-js` gekapselt und nur für
`cfg(target_arch = "wasm32")` aktiv; der native Pfad bleibt Default für CLI/
Desktop. Ein unabhängiger Verifizierungs-Agent prüft später, dass derselbe
`decode_bytes`-Vertrag (Orientierung, Metadaten, 8/16-bit) in beiden Backends
gilt. Im MVP ist WASM-RAW ausgeschaltet (`UnsupportedPlatform`).

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
Drag-and-drop. Browser/WASM bleibt im MVP RAW-frei und weist diese Capability
klar aus (Post-MVP: `libraw-wasm`-Backend, Feature `wasm-js`). Preview, Exposure
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
