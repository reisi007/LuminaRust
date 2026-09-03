# Plattformen und optionale Indizierung

**Features:** F-006 Optionale DB, F-007 RAW-Import, F-010 CLI und GUI,
F-100 Lightroom-UI-Konventionen

> **WASM gestrichen (2026-09-04, Eigentümer-Entscheidung):** alle WASM-/Browser-
> Ziele (F-069…F-071), `cfg(target_arch = "wasm32")`-Pfade, `wasm-bindgen`-/`trunk`-
> Artefakte und der WASM-CI-Job werden ausgebaut. Dieses Dokument beschreibt nur
> noch native CLI + Desktop-GUI. Historische WASM-Abschnitte unten sind als
> ENTFERNT markiert und nicht normativ.

## Inhaltsverzeichnis

- [Gemeinsame Grenze](#gemeinsame-grenze)
- [CLI](#cli)
- [Desktop-GUI](#desktop-gui)
- [UI-Konventionen (F-100)](#ui-konventionen-f-100)
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
`--highlights` und `--shadows` die Presetwerte überschreiben. Der Export wird
zunaechst in eine Staging-Temp-Datei geschrieben, dann das Sidecar committet;
erst danach wird der Export atomar an seinen Zielpfad umbenannt. Scheitert das
Sidecar-Schreiben, wird die Staging-Datei verworfen — Exit 1 OHNE erzeugten
Export, das Sidecar bleibt byte-identisch (kein stiller Fallback). `inspect` zeigt den
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

**MVP-Grenze (Stand 2026-09-04):** Das MVP umfasst **CLI und native Desktop**
(auch RAW). **WASM/Browser ist ersatzlos gestrichen** (Eigentümer-Entscheidung
2026-09-04) und wird nicht umgesetzt. `lumina-raw` kapselt
den LibRaw-Zugriff hinter einem einheitlichen `decode_bytes`/`RawMetadata`-
Vertrag; der native LibRaw-Adapter bleibt
Default für CLI/Desktop.

Die CLI soll mindestens `import`, `inspect`, `develop`, `render`, `export`,
`batch`, `mask`, `reindex` und `validate` unterstützen. Einzel- und
Batchverarbeitung arbeiten ohne GUI und ohne zentrale DB.

Batchjobs benötigen Resume, Retry, Dry-Run, begrenzten Speicher, reproduzierbare
Exit-Codes und strukturierte Ausgabe. Optionen für virtuelle Kopie,
Masken-Neuberechnung und Render-Cache werden explizit angeboten.

**Umgesetzter Stand (Review-Batch 2026-08-25, verifiziert):**
- **Masken-Policy:** Alle Renderkommandos (`render`, `export`, `batch`,
  `develop`/`process`) akzeptieren `--mask-policy warn|strict` (Default
  `warn`). `warn` = Warn-and-continue bei fehlenden/stalen Masken (überall
  konsistent, inklusive `export --update-masks`, das den Request an den
  eigenen Render durchreicht statt vorab abzubrechen); `strict` bricht laut
  ab. Damit ist `MaskPolicy::Strict` erstmals wirklich erreichbar.
- **Einmalige Masken-Flags:** `update_masks`/`force_render` werden nach dem
  Konsum aus dem persistierten Rezept entfernt — keine permanente Re-Inferenz
  trotz gültiger Maske (Persistenz-Invariante).
- **Persistenter Masken-Tile-Key:** zdata-Tiles werden unter der Composite-ID
  `<copy_id>/<mask_id>` gespeichert/gelesen (nicht mehr nur `mask.id`);
  Kopien mit gleichen Masken-IDs teilen keine Matte mehr. Die GUI nutzt
  dieselbe Konvention. Legacy-Plain-ID-Tiles werden verworfen (Pre-MVP-
  Schemaentscheid).
- **Schutzmechanismen:** Overwrite-Guards decken `<input>.lumina.json`/
  `.lumina.zdata` (auch noch nicht existierende Ziele) und Hardlinks via
  `(dev, inode)` ab; Batch lehnt namensbasierte Zielkollisionen vorab ab;
  `reindex` beendet mit Exit ≠ 0 bei korrupten Sidecars; `import` prüft
  Content-Hash gegen ein bestehendes Sidecar; Verzeichnis-Walks sind
  symlink-/loopsicher.
- **ONNX-Runtime-Einbindung (F-082-FOLLOWUP-Rest):** Mit dem CLI-Feature
  `onnx-rt` (forwarded zu `lumina-onnx/onnx-rt`) fragt die CLI die echte
  ONNX-Engine über `lumina_onnx::resolve::try_load_onnx_engine` an, sobald
  ein Renderlauf Re-Inferenz brauchen kann (aktive Kopie trägt `mask_layers`);
  das `.onnx`-Artefakt wird über `LUMINA_MODEL_PATH` konfiguriert. Ein
  fehlendes/stale/unbenanntes Artefakt ist ein harter Fehler (`MissingModel` /
  `ModelArtifactStale` / `InferenceFailed`) — nie ein stiller Fallback auf den
  deterministischen `StubBackend`, der ohne `onnx-rt` der unveränderte Default
  bleibt. Die onnx-rt-CLI-Suite läuft dagegen mit einem bei Testzeit
  generierten, BiRefNet-kompatiblen Crafted-ONNX-Modell grün (keine
  Downloads, keine committeten Gewichte).

**Umgesetzter Stand (Review-R2-CLI-Fixes, 2026-08-26):**

- **RAW-Erkennung single-source (R2-CLI-01):** Die 18 RAW-Extensions aus der
  Formatliste oben liegen genau einmal vor — als
  `lumina_raw::RAW_EXTENSIONS` / `lumina_raw::is_raw_extension`. Sowohl die
  Decode-Route (`is_raw_path`) als auch die Batch-Kollektion
  (`has_image_extension`) referenzieren dieselbe Liste; `lumina batch`
  findet damit alle unterstützten Formate, nicht mehr nur eine Teilmenge.
- **`inspect --json` (R2-CLI-03/-04):** Der SOLL-Satz „`inspect` zeigt den
  JSON-Status" ist mit einem expliziten `--json`-Flag umgesetzt: eine
  maschinenlesbare JSON-Ausgabe mit RAW-Metadaten (Maße, Orientierung,
  Kamera/Lens/EXIF-Feldern), Sidecar-Status (`valid`/`missing`/`invalid`,
  inklusive Quelle) und allen virtuellen Kopien mit Auto-Tone-, Matching-
  und Target-Luminance-Stand. Freitext bleibt das Default-Ausgabeformat.
  Der RAW-Zweig nutzt die Metadata-only-API `lumina_raw::read_metadata`
  statt eines Voll-Decodes. Ehrliche Grenze: LibRaw verlangt `unpack()`
  vor `adjust_sizes_info_only`, daher läuft die Entropie-Dekodierung
  weiterhin; Demosaic, Farbumsetzung, Memory-Image und Promotion werden
  übersprungen (keine Pixel-Allokation).
- **Batch-Ausgabe (R2-CLI-06):** Jedes Batch-Item meldet seinen Abschluss
  als Progresszeile auf stderr (`[batch i/n] <name> ok|failed|dry-run|
  skipped`). Die Item-Einträge der JSON-Summary weisen `mask_warnings`
  aus wie render/export; stderr trägt niemals JSON-Payload.
- **Exit-Codes (R2-CLI-07):** Reproduzierbar und dokumentiert:

  | Code | Bedeutung |
  | ---- | --------- |
  | 0 | Erfolg (auch „Batch vollständig erfolgreich“) |
  | 1 | Laufzeitfehler eines Befehls (Decode-, Sidecar-, I/O-, Validierungsfehler) |
  | 2 | CLI-Benutzungsfehler (unbekanntes Flag/falsche Argumente, clap) |
  | 3 | Batch teilfehlerhaft: mindestens ein Item failed, Summary und Statusdateien sind dennoch vollständig geschrieben |

- **Konsistenz-Details:** Korrupte `.lumina.zdata`-Bundles melden sich bei
  Masken explizit als „unreadable or corrupt“ über denselben Warnungskanal
  wie fehlende/stale Masken (R2-CLI-05); `develop` weist Out-of-Range-Werte
  vorab mit erlaubter Range zurück (analog MCP, R2-CLI-09); `import`
  akzeptiert nur noch seine tatsächlichen Flags (`--input`, `--json`,
  `--migrate`) statt still ignorierteter Render-Flags (R2-CLI-10);
  Batch-Inputs werden per Datei-Identität dedupliziert (Unix `(dev, inode)`;
  R2-CLI-11).

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
> (`DiskFolderCache`); siehe feature/architecture/pipeline.md.

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
> App-State-Tests 43 grün. F-103-N7 (Presence-Regler Texture/Clarity/Dehaze
> F-094 und Vibrance/Saturation F-092 in der Color-Sektion in F-100-Reihenfolge
> Color Grading → Presence → Vibrance/Saturation, über bestehenden
> set_adjustment-/Rezeptpfad; Pipeline-Stufen vorhanden) implementiert
> (2026-08-21). Offen: F-103-N6 (erster visueller User-Test), F-103-N8
> (CLI-Doppelrender). F-103-N9 (kittest-Screenshot-Regressionen) ist
> umgesetzt (2026-08-21, siehe unten).
> Browser-Dateispeichern, ONNX,
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

> **Produktentscheidung (2026-09-04, Projekteeigentümer):** Die **native
> Desktop-App ist die einzige GUI. WASM/Browser ist ersatzlos gestrichen.**
> UI-Verifikation erfolgt nativ, z. B. über
> Headless-Snapshot-Tests (`egui_kittest`).

> **F-103-N9 — UI-Snapshot-Regressionen (`egui_kittest`, 2026-08-21):** Die
> Integrationstests unter `crates/lumina-gui/tests/kittest_snapshots.rs` rendern
> die GUI headless über den wgpu-Backend und vergleichen den Frame gegen
> committete Goldens unter `crates/lumina-gui/tests/snapshots/`. Die Tests sind
> standardmäßig `#[ignore]` (headless GPU nötig), damit CI ohne GPU grün bleibt.
> Goldens erzeugen/aktualisieren:
> `UPDATE_SNAPSHOTS=true cargo test -p lumina-gui --test kittest_snapshots -- --ignored`
> Ein **roter Snapshot** bedeutet, dass der gerenderte Frame vom committeten
> Golden abweicht — meist eine beabsichtigte UI-Änderung (Golden via
> `UPDATE_SNAPSHOTS=true` refreshen) oder eine Regression (Diff unter
> `tests/snapshots/<name>.diff.png` prüfen). Pro Zustand ein eigener Test:
> `library_empty`, `library_with_image`, `develop_basic`,
> `develop_sections_expanded`, `export_module`. Ein Masken-Werkzeug-Zustand
> wurde bewusst weggelassen: ein aussagekräftiges Overlay braucht ein geladenes
> Bild plus einen committierten Brush-Stroke, was `save_sidecar` (Disk-Schreiben)
> triggert — ungeeignet für einen headless-regressionstest ohne
> Filesystem-Seiteneffekte; ein nur „armierter“ (ohne Bild) Tool-Zustand ist
> visuell nicht vom Develop-Grundzustand zu unterscheiden.

> **Test-Ist-Stand (2026-09-03, LR-PARITY-01 Wellen 1–3 + SPOT-Fixes, verifiziert
> BESTANDEN, HEAD 711fe09):** `lumina-gui` 185p, `lumina-core` 328p,
> `lumina-sidecar` 101p (`--lib`, 139p mit `zdata`-Feature). Die 5 kittest-Goldens
> bleiben `#[ignore]` (headless GPU noetig); ein Rebaseline nach den Wellen 2/3
> steht aus (siehe `Agents.todo.md` GUI-KIT-01-REFRESH, `UPDATE_SNAPSHOTS=true`
> nur bei beabsichtigtem Diff auf einer GPU-Maschine).

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
  UI **Grundtonung**, **Tonwertkurve**, **Farbe**, **Details**, **Effekte**,
  **Optik**, **Geometrie** und **Maskierung**. Die englischen Lightroom-
  Referenzbegriffe sind Basic, Tone Curve, Color, Detail, Effects, Optics,
  Geometry und Masking.

> **Produktentscheidung (2026-08-25, Projekteeigentümer — F-103-N10):**
> Die Sektionsreihenfolge folgt der Lightroom-Classic-Panelfolge
> (Basic → Tone Curve → HSL/Color → Color Grading → **Detail → Effects** …):
> **„Details“ (Schärfen, Rauschreduzierung) steht vor „Effekte“
> (Vignettierung/Körnung).** Zuvor war in diesem Abschnitt „Effekte“ vor
> „Details“ notiert (und so auch gerendert); SOLL und GUI sind mit dieser
> Entscheidung gleichgezogen (Umsetzung 2026-08-26). Die Kollaps-Zustände
> bleiben davon unberührt, da egui sie an den Sektionslabels speichert, nicht
> an der Position.

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
  werden. Der Filmstreifen ist in allen drei Modulen sichtbar (Library,
  Develop, Export).
- Mehrfachauswahl im Filmstreifen (Click = Auswahl, Cmd/Ctrl-Click = Toggle,
  Shift-Click = Bereich): **Sync Settings** wendet Rezept der aktiven Kopie
  auf alle ausgewählten Bilder an (je eigenes Sidecar, CAS, Fehler einzeln
  laut); **Match Total Exposures** gleicht Belichtung über die Auswahl an
  (Core-`match_total_exposure` je Bild gegen Auswahl-Median). Beide Aktionen
  loggen `info!` je Bild und bumpen `preview_generation`.
- Globale Develop-Aktionen (Save Recipe/Sidecar, Reset, Render/Apply, Match
  Total Exposure) stehen als fixierter Footer unterhalb der ScrollArea; die
  Sektionsreihenfolge darüber bleibt unverändert.
- **Ist-Stand 2026-09-03:** Preview-Noise (Neighbor-Stand-in ehrlich als Draft verbucht), Toast-Overlay (4 s + ✕), Optics-Profilstatus + stellbare Slider, Custom-Gate, Single-Source-Auswahl, Nav-Hybrid-Overview, Histogramm-Full-Frame umgesetzt + verifiziert BESTANDEN (259p lib, kittest 11/11, Vision 6/6); rechtes Panel-Thumb bewusst entfernt (nicht in F-100 normiert, zeigte ROI-Crop als Vollbild).
- Oben befinden sich die Modul-Leiste mit den Lightroom-Entsprechungen
  **Bibliothek**, **Entwickeln** und **Exportieren** (Library, Develop,
  Export).
- **Startverhalten (F-100, User-Vorgabe 2026-09-04):**
  - Enthält das geöffnete Verzeichnis mindestens ein unterstütztes Bild und
    ist nichts geladen/ausgewählt, wird das erste Bild (Grid-Sortierung)
    automatisch selektiert **und** geladen — die Auswahl steht danach nie
    leer, solange Bilder existieren. Das gilt für alle unterstützten Formate
    (nicht nur RAW); der bestehende RAW-only-Auto-Load ist die Untergrenze.
  - Das Startmodul ist Develop (Default). Quelle der Wahrheit für einen
    deterministischen Start sind CLI-Flags: `--module
    library|develop|export` und `--fullscreen` (Lights-Out-Arbeitsansicht,
    kein OS-Vollbild). Eine Persistenz der letzten Sitzung (Modul/Ansicht)
    gibt es in v1 bewusst nicht (Sidecar-first, keine zentrale Session-DB).
  - `Modulwechsel mutieren niemals Rezept oder Sidecar` (bleibt).
- **Ist-Stand 2026-09-04:** Auto-Select (erstes Bild alle Formate, Selektion nie
  leer), `--module`/`--fullscreen`-Flags umgesetzt + verifiziert BESTANDEN
  (281p lib, 7p bins, kittest 11/11, Vision Golden-BESTANDEN); Folgearbeit:
  `.lumina/`-Scan-Ausschluss (Befund B3, hoch).
- Das Histogramm ist eine echte Grafik (gefüllte 256-Bin-Luminanzkurve per
  Painter, P01/P99 als schmale Marker, Mean/Median-Text) in einer eigenen
  einklappbaren Sektion (Default offen) und wird immer aus dem **gesamten
  Bild** (Full-Render, nie nur sichtbarer Viewport/ROI-Ausschnitt) berechnet;
  Draft-/Veraltet-Zustände bleiben sichtbar markiert.
- Der Navigator zeigt das Gesamtbild mit einem Viewport-Rechteck (= aktuell
  sichtbarer Develop-Arbeitsbereich); Draggen des Rechtecks pannt den
  sichtbaren Bereich. Das Navigator-Panel ist einklappbar.
- Zoomstufen: **Fit (Default)**, 25 %, 50 %, 75 %, 100 % (1:1), 200 %,
  Fit-Breite. Das Mausrad zoomt nur mit Modifier (sonst Scroll/Pan) — ohne
  Modifier entsteht nie ein Zoom. `Custom` ist die gepinnte Ansicht (Zoom **und**
  Pan): Pannen (Wheel ohne Modifier im Zoom, Drag, Navigator-Rechteck) pinnt
  `Custom`, tastet den Zoomfaktor aber nie an. Die Zoom-Anzeige nennt die
  nominale Stufe (Fit/25/50/75/100/200 %, Fit-Breite); die effektive
  On-Screen-Skala ist höchstens Tooltip.
- Slider-Commits speichern: Nach Debounce-Ende wird bei erfolgreichem Render
  das Sidecar geschrieben und per INFO-Log + Status bestätigt („Sidecar
  saved"); Fehler sind laut, nie still.
- **Ist-Stand 2026-09-04:** Fit neutralisiert Pan und zeigt Vollbild (stale
  Crop-Textur wird pan-neutral ersetzt, GUI-FIT-1); Draft- und Full-Placement
  sind geometrisch identisch (kein Springen, GUI-DRAFT-JUMP-1); Auto-Tone
  schreibt 6 Regler + Spiegel mit selektivem Stale-Clear (AUTO-TONE-2).

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

### Tastaturkürzel (F-100, LR-01/LR-09/LR-10, Welle 2, Welle 3)

Alle Kürzel werden ignoriert, solange ein Widget Tastatureingaben erwartet
(z. B. ein fokussiertes Textfeld), damit sie keinen eingegebenen Text
kapern. Modulwechsel mutieren niemals Rezept oder Sidecar.

| Taste | Aktion | Anmerkung |
| --- | --- | --- |
| `G` / `D` / `E` | Bibliothek / Entwickeln / Lupe (Alias für Bibliothek) | gebunden |
| `1`–`5` | Sternebewertung der aktiven virtuellen Kopie setzen | LR-01; ersetzt die frühere Zoom-Belegung von `Num1`/`Num2` (1:1/2:1 bleiben über die Vorschau-Werkzeugleiste erreichbar) |
| `0` | Bewertung zurücksetzen (unbewertet) | LR-01; nur mit geladener virtueller Kopie, sonst Zoom-Fit wie bisher |
| `P` / `X` / `U` | Pick / Reject / Unflag der aktiven Kopie | LR-01 |
| `K` / `M` / `Shift+M` | Maskenwerkzeug Pinsel / Verlauf / Radial scharfschalten | LR-10; `Esc` entschärft; bei aktiver Rezept-Geometrie laut verweigert |
| `Q` | Spot-Heal-Werkzeug umschalten | bereits gebunden |
| `Cmd/Ctrl+'` | Aktive virtuelle Kopie duplizieren und auswählen | LR-09; vorheriges `save_sidecar` sichert ungespeicherte Edits, damit das Duplikat den aktuellen Stand erbt |
| `Cmd/Ctrl+Shift+C` / `Cmd/Ctrl+Shift+V` | Einstellungen kopieren / einfügen (aktive virtuelle Kopie) | LR-09 Welle 2; sitzungsweiter Clipboard (nicht persistiert), Einfügen über Save/Render-Pfad mit `preview_generation`-Bump |
| `6`–`9` | Farb-Label 1–4 (Rot/Gelb/Grün/Blau) der aktiven Kopie | Welle 2; `extras["color_label"]`, kein Schema-Change; `0` = kein Label |
| `V` | Schwarz-Weiß-Behandlung umschalten | Welle 2; rezeptbasiert (`saturation`/`vibrance` −1, Vorwerte in `extras["bw_stash"]`), erneutes `V` stellt exakt wieder her |
| `J` | Clipping-Warnungen umschalten | Welle 2; reines Anzeige-Badge aus Preview-Pixeln, nie Rezept |
| `L` | Lights-Out (Seitenpanels + Filmstreifen aus) | Welle 2; Header/Modulleiste bleiben, nie Rezept |
| `R` | Crop-Modus-Badge umschalten | Welle 2; reine Anzeige, Edits in Geometrie-Crop |
| `Tab` | Seitenpanels ein-/ausblenden (Filmstreifen bleibt) | Welle 2; nie Rezept |
| `Shift+Tab` | Alle Panels ein-/ausblenden (Seitenpanels + Navigator + Filmstreifen) | G-11; nie Rezept, keine Kollision (kein anderer `Shift+Tab`-Pfad) |
| `Y` | Vorher/Nachher | gebunden |
| `Shift+Y` | Split-Vorher/Nachher-Markierung (Vollbild-Before-Proxy über `before_after`; Side-by-Side-Render ist Folgearbeit) | Welle 3; nie Rezept |
| `C` | Compare (Vorher-Bild über `before_after`, erneutes `C` verlässt) | Welle 3, LR-20 light; nie Rezept |
| `N` | Survey (Sprung ins Bibliotheks-Raster, erneutes `N` verlässt den Modus) | Welle 3, LR-20 light; nie Rezept |
| `\` | Library-Filterleiste + Quick Develop (Textfilter über gescannte Metadaten: Name, `rating:0-5`, `flag:pick/reject`, `label:Farbe`; Quick Develop `exposure/contrast/highlights/shadows` über Save/Render-Pfad) | Welle 3, LR-13 light; kein Index |
| `Cmd/Ctrl+G` | Stapel-Gruppen-Proxy der aktiven Kopie (`extras["stack_group"]`, erneutes Drücken gruppiert aus) | Welle 3, LR-17 light; kein Schema-Change |
| `Cmd/Ctrl+Alt+S` | Schnappschuss (benannter History-Freeze `Snapshot <n>`, Wiederherstellen über History-Pfad) | Welle 3, LR-12 light; kein Schema-Change |
| `Cmd/Ctrl+Shift+I` / `Cmd/Ctrl+Shift+E` | Bibliothek (Import) / Exportieren anspringen (reiner Modulwechsel, Dialoge bleiben manuell) | Welle 3, LR-13 light; nie Rezept |
| `F` | Vollbild-Vorschau (versteckt dieselbe Chrome wie Lights-Out, setzt beim Einschalten Zoom auf Fit) | Welle 3; nie Rezept |
| `Num0`, `+` / `-` | Zoom Fit (ohne Dokument) / Zoomstufen | gebunden (`Num0` mit Dokument = Bewertung 0, LR-01) |

Die Bibliotheks-Rasteransicht zeigt je Datei ein Bewertungs-Badge (Sterne der
Standardkopie plus Pick-/Reject-Markierung); Details stehen im Hover-Text.
Die Filterleiste (`\`, Welle 3) filtert das Raster über die bereits
gescannten Metadaten (Dateiname, `rating:`, `flag:`, `label:` — kein Index);
Quick Develop setzt Grundtonung (`exposure/contrast/highlights/shadows`) auf
der aktiven Kopie über den normalen Save/Render-Pfad.
- Das Bibliotheks-Raster zeigt Bilder des gewählten Ordners **einschließlich
  Unterordner** (rekursiv, symlink-/loop-sicher, Tiefe begrenzt analog
  `FOLDER_SCAN_DEPTH`); jede Zelle trägt den relativen Unterordner als
  Pfad-Badge; der Ordnerbaum bleibt als flache Pro-Ordner-Navigation erhalten
  (Klick = dieser Ordner flach listen bleibt möglich).
- **`.lumina/`-Ausschluss (F-100 Library, GUI-LIBRARY-LUMINA-DIR-1):** Der
  Library-Scan (flach wie rekursiv, alle Ebenen) steigt niemals in
  Verzeichnisse mit dem exakten Namen `.lumina` ab und listet keine Dateien
  darunter — `.lumina/` enthält ausschließlich löschbaren Cache
  (z. B. `.lumina/previews/*.preview.webp`) und Einstellungen, die ohne
  Datenverlust gelöscht werden können und daher nie als Bilder im Grid,
  Sync/Match-fähig oder als Sidecar-Ziel erscheinen dürfen.
- **Ist-Stand 2026-09-03:** umgesetzt + verifiziert BESTANDEN (236p lib, kittest 11/11 inkl. `library_subfolder_badges`-Golden, Vision: Badges korrekt zugeordnet; Kontrast-Nacharbeit s. GUI-LIBRARY-BADGE-CONTRAST-1).

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

### Tool-Overlays, Edit-Pins, Solo-Mode, Shift+Tab (G-11, LRPAR-G11-OVERLAYS)

Overlay-/Panel-Comfort nach Lightroom-Vorbild, GUI-contained (kein CLI-Anteil).
Alle vier Bausteine sind reiner Session-Display-State und werden **nicht** ins
Sidecar persistiert (wie `Tab`/`L`/`F`/`J`/`R`): Das Sidecar bleibt portabel,
ein Reload stellt die Defaults wieder her, das Rezept wird nie berührt.

- **Tool-Overlay-Modi** (`OverlayMode`, global für Masken- und Retusche-
  Werkzeuge — bewusst ein Schalter statt pro Werkzeug, damit der Zustand
  vorhersehbar bleibt): `Always` malt das Matte-Overlay, sobald ein Prompt
  existiert (Live-Drag oder gespeicherter Prompt der selektierten Maske) —
  das ist das bisherige Verhalten und daher der Default; `Auto` malt nur,
  solange ein Masken- (`K`/`M`/`Shift+M`) oder Spot-Heal-Werkzeug (`Q`)
  armiert ist oder ein Drag läuft; `Never` malt nie. Umschalter in der
  Masking-Sektion, Statuszeile + `info!`-Log.
- **Edit-Pin-Sichtbarkeit** (`PinVisibility`, global, Default `Auto`):
  `Always` zeigt alle Pins ohne armiertes Werkzeug, `Never` zeigt keine,
  `Auto` zeigt Pins nur bei armiertem Masken-/Spot-Werkzeug. Ein Pin steht
  für jede Maske der aktiven Kopie mit ableitbarem Anker (Box: Rechteck-
  Mitte; Brush: erster Mark; Polygon: erster Vertex; Ellipse: Zentrum;
  Gradient: Mittelpunkt der Verlaufsstrecke aus `angle_deg`/`start`/`end`
  um die Bildmitte, auf `0..=1` geclampt; Masken ohne Prompt/Geometrie
  erhalten bewusst keinen Pin statt einer erfundenen Position) plus jeden
  Spot-Heal (`center_x`/`center_y` aus `spot_removals`). Pins sind
  Painter-Content (für AccessKit unsichtbar) — daher hält das Modell
  zusätzlich den testbaren Getter `visible_edit_pins()` (Anzahl/Anker/
  Selektion) vor; der Painter malt dieselbe Liste.
- **Solo-Mode** (Checkbox in der Masking-Sektion, Default aus): Ist er an,
  schließt das Öffnen einer der acht Develop-Sektionen (Basic, Tone Curve,
  Color, Detail, Effects, Optics, Geometry, Masking) die anderen sieben;
  das Einschalten bei mehreren offenen Sektionen behält deterministisch die
  erste (niedrigster Index) und schließt den Rest. Die Öffnungszustände
  (`section_open[8]`) sind expliziter App-State (kein egui-implizites
  `ui.collapsing`-Gedächtnis), damit Solo headless testbar bleibt.
- **`Shift+Tab`** schaltet `all_panels_hidden`: Seitenpanels, Navigator-Rail
  und Filmstreifen werden ausgeblendet (Header/Modulleiste + Vorschau
  bleiben, damit Status und Fehler sichtbar sind). `Tab` allein behält
  bewusst den Filmstreifen. Mapping als reine Funktion
  `all_panels_toggle_for_key(Taste, Shift)` mit Mapping-Test; keine
  Kollision mit Bestand (`Shift+M` Radial, `Shift+Y` Split, `Shift+C/V/I/E`
  Clipboard/Import/Export nutzen andere Tasten; `Shift+Tab` fiel bisher in
  den `Tab`-Zweig und ist jetzt disambiguiert).

## WASM — ENTFERNT (2026-09-04)

WASM/Browser ist ersatzlos gestrichen (Eigentümer-Entscheidung). F-069…F-071
entfallen; `cfg(target_arch = "wasm32")`-Pfade, `wasm-bindgen`-/`trunk`-Artefakte
und der WASM-CI-Job werden ausgebaut (Tasks WASM-REMOVE-GUI/-ONNX/-REST).
Frühere Inhalte dieses Abschnitts leben in der Git-Historie weiter.

### Capability-Matrix Native — `wgpu`-Renderer mit Shared Device (GUI-WGPU-PRESENT-1, 2026-08-26)

Der native Desktop nutzt `eframe` mit dem **wgpu**-Renderer;
`lumina_gpu::GpuContext::from_parts` übernimmt Renderer-`Instance`/`Adapter`/
`Device`/`Queue` (`attach_wgpu_render_state`). Alle VRAM-Texturen und das
Present-Target liegen auf dem **dieselben** Device, das die Swapchain bedient —
der frühere glow/wgpu-Dual-Backend-Konflikt ist aufgelöst.

| Capability | CLI (nativ) | GUI Desktop — `wgpu` present |
|------------|-------------|------------------------------|
| `lumina-core` (CPU-Referenz) | ✅ verfügbar | ✅ Fallback (Before/After, ROI-Zoom, nicht-GPU-unterstützte Rezepte) |
| `lumina-gpu` (`gpu` feature, `wgpu`/`bytemuck`/`pollster`) | ✅ optional (`--features gpu`) | ✅ default on, shared device via `from_parts` |
| Decode/Demosaic (`lumina-raw` LibRaw) | ✅ | ✅ (Worker-Thread) |
| Color/Tone GPU Shader (`lumina-gpu::shaders`) | ✅ `render_with_gpu` / `render_to_vram` | ✅ VRAM-resident (`render_to_vram`, Uniform → UBO) |
| SourceAction GPU Stage (`SOURCE_ACTION_STAGE_SRC`) | ✅ bei gebundenen Artefakten (`set_source_action_artifacts`), sonst CPU-Route | ✅ Drag-Pfad compositiert vor Tone; Present-Gate hält nicht unterstützte Rezepte auf CPU |
| Masken‑Brush + evaluierte Ebenen im VRAM (`R16Uint`) | — | ✅ persistente `Vec<u16>` Plane → dirty 512² Tiles (`upload_mask_tile`) + evaluierte Planes nach Full-Render (`combine_mask_planes` → `upload_mask_plane`, byte-exakt) |
| Overlay‑Composite + Present | — | ✅ readback-frei: `copy_vram_to_texture(present_target)` → `register_native_texture` → `painter().image`; CPU-Upload als Fallback erhalten | — |
| `VramState` Management | — | LRU-Pool dimensionsschlüsselt (`LUMINA_GPU_VRAM_POOL_ENTRIES`=4, `LUMINA_GPU_VRAM_BUDGET_MB`=1024); 512² `TiledCache`/`DraftPyramid` bleibt M2 | — |
| Fehlerreporting | `warn!`/CPU-Route-Logs einmal pro Grundmenge | `warn!` bei Tile-/Plane-/Overlay-Fehlern; Init-Fehler via `log_gpu_init_failure` (getestet) | — |

**Hinweis:** Der On-Screen-Present ist headless nicht automatisiert testbar
(eframe-Laufzeit nötig); die Pixel-Gleichheit der Stufen ist gegen die
CPU-Referenz getestet (`tests/golden.rs`, `tests/stages.rs`), der UI-Renderer-
Wechsel durch unverändert grüne kittest-Snapshots abgesichert. Der nächste
manuelle GUI-Test (Block C) verifiziert den Present-Pfad visuell.

Eine ausführliche GPU‑DAG‑ und Present‑Diskussion steht in `docs/gpu-bootstrap.md`
§ *Dual‑Backend Native: resolved — `eframe` wgpu renderer + shared device*.

### Erster visueller User-Test

Das `lumina-gui`-Crate ist eine native eframe-Desktop-App. Der reproduzierbare
MVP-Befehl ist:

```bash
cargo run -p lumina-gui
```

Die Oberfläche lädt PNG, JPEG und WebP sowie native RAW-Dateien per Pfad oder
Drag-and-drop. Preview, Exposure
(`-10..=10`) und Contrast (`-1..=1`) laufen über `lumina-core::ImageFrame` und
`lumina-sidecar::EditRecipe`. Native Sidecars werden neben dem Original
gespeichert. ONNX,
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
- Native Capability-Grenzen sind dokumentiert.
