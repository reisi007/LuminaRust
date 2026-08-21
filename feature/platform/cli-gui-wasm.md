# Plattformen und optionale Indizierung

**Features:** F-006 Optionale DB, F-007 RAW-Import, F-010 CLI, GUI und WASM,
F-100 Lightroom-UI-Konventionen

## Inhaltsverzeichnis

- [Gemeinsame Grenze](#gemeinsame-grenze)
- [CLI](#cli)
- [Desktop-GUI](#desktop-gui)
- [UI-Konventionen (F-100)](#ui-konventionen-f-100)
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

> **Implementierungsstatus (F-103, 2026-08-21):** Die Desktop-GUI (egui/eframe,
> native Desktop als einzige MVP-GUI) erfüllt die F-100-Konventionen:
> Modul-Leiste Library/Develop/Export, acht kollabierbare Develop-Sektionen in
> normativer Reihenfolge, Navigator/Vorschau, Filmstreifen mit Thumbnails aus
> dem Preview-Cache (Hintergrundgenerierung via IdleQueue), LR-dark-Theme mit
> zentraler Palette und WCAG-Kontrasttests, i18n-Gerüst (englisch, 0 deutsche
> UI-Literale), Regler-Semantik (Doppelklick-Einzelreset, Alt-Scroll-Feinjustierung,
> -100..+100-Anzeige für -1..=1-Domänen), Before/After (`Y`), Auto-Tone-Button,
> WB-Pipette, interaktive Maskenwerkzeuge Pinsel/Verlauf/Radial mit Overlay und
> Sidecar-Persistenz (MaskPrompt Brush/Gradient/Ellipse gemäß F-079/F-081),
> Exportieren-Modul über die gemeinsame `lumina_core::export_image`-Logik —
> GUI-Export byte-identisch zum CLI-Export (getestet), Same-Path-Schutz,
> atomarer Artefakt-Write. Unabhängig verifiziert BESTANDEN (2026-08-21);
> App-State-Tests 43 grün. Offen: F-103-N6 (erster visueller User-Test),
> F-103-N7 (Presence-/Vibrance/Saturation-Regler), F-103-N8 (CLI-Doppelrender),
> F-103-N9 (kittest-Screenshot-Regressionen). Browser-Dateispeichern, ONNX,
> Masken-Inferenz, Cache-Synchronisierung und Mehrbild-Bearbeitung bleiben
> bewusst Post-MVP.

> **Export-Determinismus (GUI ↔ CLI, 2026-08-21):** Die Desktop-GUI erzeugt über
> das Export-Modul exakt denselben Bytestrom wie die CLI, weil beide den
> gemeinsamen Pfad `lumina_core::export_image` (Render + Encode) nutzen. JPEG mit
> fester Qualität wird über den `image`-Encoder als deterministisch behandelt
> (gleiche Eingabe → gleiche Bytes); PNG dient in den GUI-Export-Tests als
> primärer Byte-Vergleichsanker. Der Originalpfad wird beim Export nie
> überschrieben (nicht-destruktiver Export).

Für v1 ist egui/eframe festgelegt. Tauri ist keine v1-Abhängigkeit und kann in
einer späteren Architekturentscheidung erneut bewertet werden.

> **Produktentscheidung (2026-08-21, Projekteeigentümer):** Die **native
> Desktop-App ist die einzige MVP-GUI.** Der WASM-/Trunk-Pfad bleibt buildbar
> (CI-Check) und wird ausschließlich als dokumentierte Capability-Grenze
> geführt; es findet keine Funktionsentwicklung für WASM statt (Post-MVP,
> siehe WASM-Abschnitt). UI-Verifikation im MVP erfolgt nativ, z. B. über
> Headless-Snapshot-Tests (`egui_kittest`); Browser-basierte Screenshot-Harnesses
> (trunk serve + Playwright) sind Post-MVP-Optionen.

## UI-Konventionen (F-100)

Die Desktop-GUI folgt verbindlich den UI-Konventionen von **Lightroom Desktop**
als Referenz. Diese Vorgaben beschreiben die Bedien- und Anordnungssemantik,
nicht eine pixelgenaue Kopie der Adobe-Oberfläche. Abweichungen von den
folgenden Regeln benötigen eine dokumentierte Produktentscheidung.

> **Produktentscheidung (2026-08-21, Projekteeigentümer):** Die GUI-Oberfläche
> ist zum MVP **englischsprachig**; die deutschen Abschnittsnamen unten sind die
> deutsche Referenzübersetzung und werden erst mit einer späteren Lokalisierung
> aktiv. Die UI-Texte werden von Anfang an über ein i18n-Gerüst (zentrale
> Übersetzungstabelle, keine im Code verteilten Literalen) verdrahtet, sodass
> Deutsch als weitere Sprache ergänzt werden kann, ohne UI-Code anzufassen.
> Panelanordnung, Reihenfolge und Semantik folgen unverändert dieser Sektion.

### Anordnung und Panelstruktur

- Das Bearbeitungs- beziehungsweise Develop-Panel befindet sich auf der
  rechten Seite. Seine Sektionen sind kollabierbar und heißen in der deutschen
  UI **Grundtonung**, **Tonwertkurve**, **Farbe**, **Effekte**, **Details**,
  **Optik**, **Geometrie** und **Maskierung**. Die englischen Lightroom-
  Referenzbegriffe sind Basic, Tone Curve, Color, Effects, Detail, Optics,
  Geometry und Masking.
- Die Sektionen werden in dieser Reihenfolge angezeigt. Innerhalb der Sektionen
  wird die Bearbeitungsreihenfolge der F-089–F-099-Unterstufen sichtbar und
  verbindlich abgebildet: **globale Tonwerte** (Exposure, Contrast,
  Highlights, Shadows, Whites, Blacks) → **Kurve** (F-089 Tone Curve) →
  **HSL/Farbmischer** (F-090 HSL/Color Mixer) → **Color Grading** (F-091) →
  **Präsenz** (F-094 Texture, Clarity, Dehaze) → **Dynamik/Sättigung** (F-092
  Vibrance, Saturation) → **Schärfen** (F-095
  Sharpening) → **Rauschreduzierung** (F-096 Noise Reduction) →
  **Vignettierung/Körnung** (F-097 Vignette/Grain) →
  **Objektivkorrektur** (F-098 Lens Correction) → **Perspektive** (F-099
  Upright/Perspective) → **Crop/Zuschneiden** (F-093 Crop).
- Diese visuelle Reihenfolge ist eine UI-Konvention und ändert nicht die
  normative Renderreihenfolge der Pipeline, insbesondere die dort festgelegte
  Reihenfolge von Rauschreduzierung und Schärfen.
- Links nimmt Navigator und Vorschau den großen Arbeitsbereich ein. Am unteren
  Rand befindet sich ein Filmstreifen mit Miniaturen als
  Datei-Browser-Entsprechung. Diese beiden Bereiche sind beim Entwickeln
  vorhanden; der Filmstreifen darf nicht durch eine reine Dateiliste ersetzt
  werden.
- Oben befinden sich die Modul-Leiste mit den Lightroom-Entsprechungen
  **Bibliothek**, **Entwickeln** und **Exportieren** (Library, Develop,
  Export) sowie das Histogramm. Das Histogramm bezieht sich auf den konkret
  angezeigten Renderstand.

### Regler und Standardinteraktionen

- Jeder Bearbeitungsregler ist ein horizontaler Slider mit der Beschriftung
  links und dem aktuellen Wert rechts. Die Wertebereichsanzeige ist am Regler
  sichtbar.
- Ein Doppelklick auf die Beschriftung setzt ausschließlich diesen Regler auf
  seinen dokumentierten Standardwert zurück. Ein Doppelklick auf den Wert darf
  nicht stattdessen das gesamte Rezept zurücksetzen.
- Alt/Option-Scroll über einem Regler feinjustiert dessen Wert in kleineren
  Schritten. Die normale Scroll-/Drag-Interaktion bleibt für die grobe
  Einstellung erhalten.
- Die Anzeige verwendet die Lightroom-konventionelle Skala, sofern die
  jeweilige F-089–F-099-Spezifikation keine andere Domäne vorgibt. Für interne
  Werte in `-1..=1` wird beispielsweise `-100..+100` angezeigt (etwa bei
  Presence, HSL, Color Grading-Balance und Dynamik/Sättigung); Speicherung und
  Pipelinevalidierung verwenden weiterhin die normativen internen Werte.
- **Vorher/Nachher** (Before/After) ist als Umschaltaktion verfügbar und die
  Standard-Tastenkombination ist `Y`. Der **Auto**-Button (Auto Tone) befindet
  sich in der Sektion Grundtonung. Die Weißabgleich-Auswahl enthält eine
  Pipette (White Balance Eyedropper), die einen Punkt aus Navigator oder
  Vorschau übernimmt.

### Maskierung

Die Sektion **Maskierung** (Masking) enthält eine Liste der Masken und einen
Button **Neu**. Nach dem Anlegen stehen die Werkzeuge **Pinsel**, **Verlauf**
und **Radial** (Brush, Linear Gradient und Radial Gradient) zur Verfügung;
ihre Prompt-/Koordinatensemantik muss F-079 und F-081 entsprechen.

Die Auswahl einer Maske zeigt darunter im selben Panel deren lokale Regler,
mindestens **Belichtung**, **Kontrast** sowie die jeweils unterstützten lokalen
Tonwert-, Farb- und Präsenzregler. Lokale Regler erscheinen nicht in einem
separaten, kontextlosen Dialog: Ihre Zugehörigkeit zur ausgewählten Maske muss
im Panel sichtbar bleiben. Masken-Layer, Invertierung, Feathering, Blur und
lokale Anpassungen werden entsprechend der virtuellen Kopie im deklarativen
Rezept gespeichert.

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
