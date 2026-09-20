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
  | 2 | CLI-Benutzungsfehler (unbekanntes Flag/falsche Argumente — clap **sowie** geprüfte Post-parse-Konflikte wie mutually exclusive Aktionen, `CliError::Usage`) |
  | 3 | Batch teilfehlerhaft: mindestens ein Item failed, Summary und Statusdateien sind dennoch vollständig geschrieben |

- **Konsistenz-Details:** Korrupte `.lumina.zdata`-Bundles melden sich bei
  Masken explizit als „unreadable or corrupt“ über denselben Warnungskanal
  wie fehlende/stale Masken (R2-CLI-05); `develop` weist Out-of-Range-Werte
  vorab mit erlaubter Range zurück (analog MCP, R2-CLI-09); `import`
  akzeptiert nur noch seine tatsächlichen Flags (`--input`, `--json`,
  `--migrate`) statt still ignorierteter Render-Flags (R2-CLI-10);
  Batch-Inputs werden per Datei-Identität dedupliziert (Unix `(dev, inode)`;
  R2-CLI-11).

### Metadaten-MVP (G-15, Slice 2: CLI)

Normativer CLI-SOLL für Keywords, Sammlungen, Smart-Sammlungen und
Stapelvergabe. Die Datentypen (`SidecarDocument::{keywords, collections}`,
`CollectionMembership`, `SmartCollectionDef`/`SmartRule` v1,
`BatchOp`/`apply_batch_op`) leben in `lumina-sidecar` (Slice 1) und werden
hier nur genutzt — keine Schemaänderung, kein Bump.

- `lumina keywords --input <bild> [--add <kw>...] [--remove <kw>...] [--json]`:
  listet die Quellebenen-Keywords des Sidecars. Mit `--add`/`--remove` werden
  die Operationen in Flag-Reihenfolge als `BatchOp::{AddKeyword,RemoveKeyword}`
  via `apply_batch_op` angewendet und bei Änderung atomar via `save_sidecar`
  gespeichert (unverändert → kein Rewrite). Fehlendes Sidecar und ungültige
  Keywords (leer,Whitespace, Steuerzeichen, > 128 Zeichen) scheitern laut;
  das Original wird nie verändert.
- `lumina collections --input <bild> [--add-to <id=name>...]
  [--remove-from <id>...] [--json]`: listet die statischen
  Mitgliedschaften (`{ id, name }`, Quellebene). Mutationen laufen als
  `BatchOp::{AddToCollection,RemoveFromCollection}` über denselben
  atomaren Pfad; `--add-to` trennt am ersten `=` (fehlendes `=` ist ein
  lauter Benutzungsfehler). Umbenennen (`id` bekannt, `name` neu) ist
  Stapel-Semantik pro Sidecar.
- `lumina batch-meta --input <bild|verzeichnis>
  (--op <json>|--op-file <pfad>) [--json]`: wendet genau einen `BatchOp`
  (JSON in der `BatchOp`-Serdeform, z. B.
  `{"op":"add_keyword","keyword":"portrait"}`) auf jedes gefundene Sidecar an
  — je Datei atomar (`save_sidecar`, nur bei `changed`), Datei-Iteration
  laut und isoliert: Ein defektes/fehlendes Sidecar markiert nur sein Item
  als `failed` (stderr-Zeile + `info!`-Log), die übrigen Items laufen weiter.
  Exit `0` bei vollem Erfolg, `3` bei Teilfehlern (analog `batch`), `1` bei
  hartem Fehler (ungültiges `--op`, unlesbares Eingabeverzeichnis).
- `lumina smart-collections --input <bild|verzeichnis> --catalog <pfad>
  [--json]`: wertet die portable Smart-Katalog-Datei gegen jedes Sidecar aus
  (`SmartCollectionDef::matches_any_copy`, deterministisch, nur
  Sidecar-Inputs). Defekte Sidecars/Regeln werden pro Item laut gemeldet,
  der Rest läuft weiter (Exit `3` bei Teilfehlern). Reine Leseoperation —
  kein Sidecar wird geschrieben.
- **Katalog-Datei (portabel, kein DB-Ersatz):** JSON
  `{"format":"lumina-smart-catalog","version":1,
  "collections":[SmartCollectionDef, ...]}`. `version` muss `1`
  (`SMART_COLLECTION_VERSION`) sein, jede Definition wird mit
  `validate_smart_collection_def` geprüft; Abweichungen scheitern laut.
  Die Datei enthält nur versionierte Regel-Daten (`id`/`name`/`rule`) —
  niemals absolute Pfade. Der Katalogpfad ist ein reines CLI-Argument und
  wird nie in Rezept- oder Sidecar-Daten persistiert.
- **Exit-Codes (Ergänzung zur Tabelle oben):** Einzelbefehle
  (`keywords`, `collections`) nutzen `0`/`1`; Mehrdatei-Befehle
  (`batch-meta`, `smart-collections`) melden Teilfehler mit `3`.

### HDR-/Panorama-Merge (G-13, Release 1.5: CLI, LRPAR-G13-MERGE-15)

Normativer CLI-SOLL für den Merge-Mehrbild-Vorlauf (Entscheid:
`decisions/LRPAR-G13-MERGE-15.md`). Beide Befehle rufen denselben
Merge-Einstiegspunkt auf wie die GUI-Aktionen `Cmd/Ctrl+H`/`Cmd/Ctrl+M`
(keine GUI-eigene Bildlogik). Quell-Sidecars werden nie verändert; das
Merge-Ergebnis ist ein neues lineares DNG plus eigenes Sidecar-Bundle.

- `lumina merge-hdr --input <a> --input <b> [--input <c>...]
  [--exposure-times <s,s,...>] [--isos <n,n,...>] [--f-numbers <f,f,...>]
  [--max-shift-px <n>] [--output <dng>] [--force] [--json]`: führt eine
  Belichtungsreihe (mindestens 2, höchstens 256 Quellen) zu einem linearen
  DNG zusammen (Translation-Ausgleich via `lumina-merge`, gewichtetes
  lineares Merge). Belichtungswerte kommen je Quelle aus den expliziten
  Listen (Reihenfolge = `--input`-Reihenfolge) oder — nur bei RAW-Quellen —
  aus deren EXIF (`shutter`/`iso`/`aperture`); fehlt beides, scheitert der
  Lauf laut als `unsupported` (kein Raten aus Pixeln). Eine Restverschiebung
  über der dokumentierten Schwelle (`HDR_SHIFT_WARN_PX`) meldet eine
  sichtbare Warnung (`aligned_with_residual`), bricht aber nicht ab.
- `lumina merge-pano --input <a> --input <b> [--input <c>...]
  [--max-shift-px <n>] [--blend-width-px <n>] [--output <dng>] [--force]
  [--json]`: richtet überlappende Einzelbilder aus (verkettete
  Translation+Rotation-light-Homographie, nur zylindrische Projektion) und
  verbindet sie mit Feder-Blend im Überlapp. Nicht überlappende oder nicht
  verkettbare Sätze scheitern laut als `unsupported` (kein Teil-Panorama).
  Belichtung fließt nicht in die Panorama-Pixel ein; je Quelle wird die
  EXIF-Belichtung — oder, falls keine vorhanden, der dokumentierte neutrale
  Default (0,01 s, ISO 100, f/8) — nur als Provenienz im Rezept gespeichert.
- **Ausgabe:** Standardmäßig `<Basis>-HDR.dng` / `<Basis>-Pano.dng` neben
  der Referenzquelle (erste `--input`; Namensregel `merge_dng_filename`),
  dazu `<basis>.dng.lumina.json` (via `sidecar_path_for`): volles
  Sidecar-Dokument (Standardkopie mit eigenem Rezept) plus
  `"type": "merge"`-Envelope auf Dokument-Ebene, `merge_recipe`
  (`merge_version` 1, Modus, Quellen mit BLAKE3-Hash/Decode-Kontext/
  Belichtung, Alignment, Status) und DNG-Artefaktverweis (relativer Pfad,
  Format `dng`, BLAKE3-Prüfsumme, Auflösung, Kanäle `rgb16`,
  Datenversion `1`). Quellenverweise sind relativ zum Sidecar-Bundle
  (gleiche Datei → Dateiname; Unterverzeichnis → `sub/datei`); Quellen
  außerhalb des Bundle-Verzeichnisses scheitern laut als `unsupported`
  (das Schema verbietet `..`-Segmente und absolute Pfade). DNG und Sidecar
  werden atomar geschrieben (Temp-Datei + Rename).
- **Status/Exit-Codes (Ergänzung zur Tabelle oben):** `0` Erfolg (auch
  „Bundle bereits aktuell" ohne Rewrite); `1` mit klarem stderr-Präfix bei
  `merge missing:` (Quelle/DNG/Sidecar fehlt), `merge stale:` (bestehendes
  Bundle, aber Quell-Hash, Decode-Kontext oder DNG-Prüfsumme weichen vom
  gespeicherten Digest ab — nur mit `--force` neu erzeugen, nie still) und
  `merge unsupported:` (Decode-, Geometrie-, Belichtungs-, Dimensions- oder
  Writer-Grenze); `2` bei clap-Benutzungsfehlern.

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
> (2026-08-21). Offen: F-103-N6 (visueller User-Test, Runde 1 am 2026-09-17
> durchgeführt mit Befunden GUI-CLICK-ALL-17/GUI-ROUTING-N6/GUI-INSTRDBG-17).
> F-103-N8 (CLI-Doppelrender) und F-103-N9 (kittest-Regressionen) sind
> umgesetzt. Historisch (2026-08-21, überholt): Browser-Dateispeichern bleibt
> Post-MVP; ONNX/Masken-Inferenz/Mehrbild-Bearbeitung sind seit F-082 bzw.
> Sync/Match/Previous implementiert (s. Feature-Docs).

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
> steht aus (`UPDATE_SNAPSHOTS=true`
> nur bei beabsichtigtem Diff auf einer GPU-Maschine).

## UI-Konventionen (F-100)

Die Desktop-GUI folgt verbindlich den UI-Konventionen von **Lightroom Classic**
als Referenz (User-Vorgabe 2026-09-17: kein reines 1:1, sondern leicht
modernerer Feinschliff — jeder Lightroom-Nutzer fühlt sich sofort zu Hause;
Details: `platform/lightroom-ux-parity.md`). Diese Vorgaben beschreiben die Bedien- und Anordnungssemantik,
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
  Highlights, Shadows, Whites, Blacks) → **Kurve** (F-089 Tone Curve,
  parametrisch + Punkte je Kanal Master/R/G/B) →
  **HSL/Farbmischer** (F-090 HSL/Color Mixer) → **Point Color** (F-090b,
  gezielte Farbauswahl) → **Color Grading** (F-091, inkl. Luminance je
  Bereich + Blending) →
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
- **Fehlermeldungen und Toasts (F-100, User-Entscheid 2026-09-13):**
  Info-Toasts (4 s + ✕-Dismiss) für abgeschlossene Aktionen; explizite
  User-Aktionen mit Fehler öffnen zusätzlich einen modalen Fehler-Dialog
  (Titel + Nachricht + Close) und loggen `error!`; Hintergrund-Fehler
  (z. B. Decode) bleiben Banner + `error!`-Log und öffnen nie einen Dialog
  (sonst würde jede Library-Sentinel den Dialog auslösen). Fehler-Toasts als
  eigene Klasse gibt es nicht — der Dialog ersetzt sie.
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
- **Klickbarkeit (F-100, User-Vorgabe 2026-09-17, F-103-N6-Befund):** Jedes
  sichtbare Feature besitzt einen **klickbaren Button** (Werkzeugleiste, Panel
  oder Sektion) — Tastaturkürzel sind nur Alias, nie der einzige Zugang.
  Befund: Crop-Modus war nur per `R` erreichbar (kein Button) und im manuellen
  Test nicht auffindbar.
  **Ist-Stand 2026-09-17 (GUI-CLICK-ALL-17, verifiziert BESTANDEN):** Alle Kürzel ohne vorherigen Button haben jetzt einen
  klickbaren Button mit derselben Aktion (Button und Kürzel rufen dieselbe
  Methode auf), einem Kürzel-Tooltip (`Str::ShortcutHint`) und `info!`-Log
  (DoD §4; die betroffenen View-Toggles loggten vorher `trace!` und wurden in
  diesem Slice auf `info!` angehoben — Ausnahme: reine Modul-/Ansichtswechsel
  (`set_module`/`set_library_view`) bleiben per G-09 bewusst `trace!` (reiner
  Session-Display-State)):
  - **Vorschau-Werkzeugleiste** (`LuminaApp::draw_view_toolbar`, sichtbar wie die
    Zoom-Controls): Crop (`R`), Clipping (`J`), Split (`Shift+Y`), Lights Out
    (`L`), Panels (`Tab`), All Panels (`Shift+Tab`), Fullscreen (`F`). Die
    B&W-Behandlung (`V`) hat den vorhandenen Treatment-Button in der
    Basic-Sektion.
  - **Library-Raster-Werkzeugleiste:** Filter-Drawer (`\`, Label `Str::FilterBar`).
  - **History-Sektion:** Duplicate Copy (`Cmd/Ctrl+'`), Copy Settings
    (`Cmd/Ctrl+Shift+C`), Paste Settings (`Cmd/Ctrl+Shift+V`), Snapshot
    (`Cmd/Ctrl+Alt+S`), Stack/Unstack (`Cmd/Ctrl+G`).
  Audit-Tests (headless): `f100_view_toolbar_paints_every_display_toggle_button`
  (malt), die Klick-/Toggle-Tests je neuem Button
  (`f100_crop_button_toggles_crop_mode_and_badge`,
  `f100_lights_out_button_toggles_and_stays_reachable`,
  `f100_clipping_button_toggles_overlay`,
  `f100_split_button_toggles_and_holds_before`,
  `f100_panels_button_toggles_side_panels`,
  `f100_all_panels_button_toggles_all_panels`,
  `f100_fullscreen_button_toggles_and_settles_fit`,
  `f100_filter_button_toggles_drawer`,
  `f100_history_duplicate_copy_button_duplicates`,
  `f100_history_copy_and_paste_buttons_roundtrip`,
  `f100_history_snapshot_button_freezes`,
  `f100_history_stack_button_toggles_group` — jeder Klick schaltet
  nachweisbar), `f100_keyboard_only_actions_have_buttons` (malt
  Filter/History/Split/Fullscreen/All-Panels),
  `f100_panel_and_view_toggle_labels_are_exhaustive_and_distinct` und der
  zentrale Shortcut→Button-Audit
  `f100_shortcut_audit_every_action_has_a_button`: er mappt **jede**
  `GuiAction` per erschöpfendem Match (ohne `_`-Arm) auf ihre Button-Oberfläche
  und ihren Button-Text und prüft, dass alle 34 Actions tatsächlich gemalt
  werden. `f100_shortcut_enum_variants_have_buttons` deckt zusätzlich die
  übrigen kürzel-tragenden Enums erschöpfend ab (`Module`, `LibraryView`,
  `CompareMode` für `C`/`N`, `MaskTool`, `Flag`). Ein künftiges Kürzel ohne
  Button kompiliert damit nicht mehr (neue `GuiAction`/Enum-Variante erzwingt
  den Match-Arm) bzw. fällt im Audit durch. Die
  kittest-Goldens wurden rebaselined (neue Werkzeugleisten-Zeile + Filter-Button).
- **Debug-Instrumentierung (GUI-INSTRDBG-17, User-Vorgabe 2026-09-17,
  verifiziert BESTANDEN):** Jede instrumentierte
  GUI-Aktion loggt in Debug-Builds genau eine Zeile
  `action=<name> duration_ms=<n> gpu_route=<present|cpu-fallback|n/a>`.
  Zentraler RAII-Helfer `GuiActionTimer` + Makro `instrument_gui_action!` +
  `GuiAction`-Namenstabelle; der GPU-Routenwert kommt aus dem vorhandenen
  `gpu_route_fallback`-State (present = Kontext gebunden ohne Fallback).
  Verschachtelte instrumentierte Aufrufe sind Teil der äußeren Aktion und
  erzeugen keine zweite Zeile. In Release-Builds sind Timer, Log-Aufruf und
  Test-Capture per `#[cfg(debug_assertions)]` wegkompiliert (Nachweis:
  `cargo clippy -p lumina-gui --release --all-targets -- -D warnings` sowie
  `cargo test -p lumina-gui --release --lib -- instrdbg_`, wo nur der
  Release-Passthrough-Test existiert). Instrumentiert ist die F-100-Aktionsfläche
  (alle Shortcut-Aktionen, die in diesem Slice ergänzten Buttons sowie
  Save/Reset/Render/Export); `set_mask_tool` und `set_spot_tool` sind bereits
  als eigene `GuiAction`s instrumentiert.
  **GUI-INSTRDBG-17b (implementiert, Verifizierung offen):** Über die 34
  Ausgangs-Aktionen hinaus sind 32 weitere user-sichtbare Schaltflächen aus
  Library-Compare, Geometrie, Masken-Layer und Metadaten instrumentiert
  (insgesamt 66 `GuiAction`s) — `toggle_compare_mode` (`C`/`N`),
  `clear_crop`/`set_crop_aspect`/`rotate_step`/`set_geometry_mirror`/
  `analyze_upright`/`set_upright_enabled`/`clear_upright`,
  `create_mask`/`select_mask`/`set_mask_visible`/`set_show_mask_overlay`/
  `set_overlay_color`/`create_ai_mask`/`create_luminance_range_mask`/
  `create_color_range_mask`/`combine_masks`/`duplicate_mask`/`set_overlay_mode`/
  `set_pin_visibility`/`set_solo_mode`/`set_mask_inverted`/
  `offer_mask_recalculation` sowie
  `add_keyword`/`remove_keyword`/`commit_metadata_draft`/
  `clear_metadata_draft`/`copy_metadata_draft`/`paste_metadata_draft`/
  `clear_metadata_history`/`apply_meta_preset`/`sync_metadata`. Der
  Instrumentierungs-Kern (Namenstabelle, RAII-Timer, Makro) ist dazu in
  `crates/lumina-gui/src/gui_action.rs` extrahiert (File-Size-Ratchet), die
  Rest-Sektions-Log-Tests liegen in `crates/lumina-gui/src/tests/instrdbg.rs`
  (der Audit liegt in `lib.rs`: `f100_action_button` ohne `_`-Arm).
  **GUI-INSTRDBG-17b-REST (implementiert, Verifizierung offen):** Über die 66
  `GuiAction`s aus Teil 1 hinaus sind 17 weitere user-sichtbare Schaltflächen
  aus Spot-Extras, Detail/Rote Augen, Optics, Tone Curve und Presets
  instrumentiert (insgesamt 83 `GuiAction`s) — `set_spot_mode`,
  `clear_spot_visualize`, `detect_spot_candidates`, `apply_detected_spots`,
  `regenerate_spot_variant`, `clear_spot_heals`, `detect_red_eye_candidates`,
  `apply_detected_red_eyes`, `remove_red_eye_region`, `clear_red_eye`,
  `set_lens_profile`, `clear_lens_profile`, `set_lens_blur_bokeh`,
  `add_curve_point`, `remove_curve_point`, `apply_preset`, `save_preset_file`.
  Buttons, deren Handler einen Teil-Schritt (`detect_*` in
  `apply_detected_*`) oder einen mit Slidern geteilten Commit
  (`set_spot_visualize`) aufrufen, laufen über eine eigene instrumentierte
  Button-Methode; verschachtelte Instrumentierung unterdrückt der Tiefen-Guard
  (genau eine Zeile). Klick-Tests je Button liegen in
  `crates/lumina-gui/src/tests/instrdbg_rest.rs` (Button → instrumentierte
  Methode → genau eine Logzeile) plus No-op-Log-Regressionstest; der Audit
  prüft die neuen Oberflächen `Detail`, `Optics`, `Tone Curve` und `Presets`
  mit. **GUI-INSTRDBG-17c (Rework 2026-09-17, Verifizierung offen):** Über die
  83 `GuiAction`s aus 17b-REST hinaus sind die user-sichtbaren Schaltflächen
  der Sektions-/Dialog- und Filmstreifen-Fläche instrumentiert (insgesamt 101
  `GuiAction`s) — `set_spot_distraction` (die vier Distraction-Checkboxen),
  `set_red_eye_pick_mode` (Red-Eye-Region-Pick-Toggle), `reload_preset_entries`
  (Presets-Refresh), `arm_wb_eyedropper` (Weißabgleich-Eyedropper),
  `add_point_color`/`remove_point_color`, `set_expand_canvas` (Apply Frame/Set
  default), `generate_generative_canvas` sowie die im 17c-Rework nachgezogenen
  rezept-mutierenden Controls `set_expand_beyond_image`/
  `set_auto_fill_transparent` (generative Canvas-Checkboxen),
  `restore_section_previous`/`reset_section` (die von allen acht
  Develop-Panels geteilte Previous/Reset-Zeile), `set_lens_blur_enabled`
  (Optics „Enable lens blur") sowie die drei Filmstreifen-Auswahlbuttons
  `sync_settings_to_selection` (Sync Settings), `match_exposures_of_selection`
  (Match Total Exposures) und `apply_previous_to_selection` (Previous Image,
  Rework F-1) sowie die KI-Denoise-Enable-Checkbox `set_denoise_enabled`
  (Detail-Sektion, Rework F-A: das Control ist bereits sichtbar, obwohl das
  Denoise-Feature erst Release 2.0 ist, und wird daher instrumentiert statt
  ausgenommen) sowie der People-View-Button „Use as mask" `create_face_mask`
  (Rework-Rest GUI-INSTRDBG-17c: legt über `push_mask_definition` eine
  Maskendefinition in der aktiven virtuellen Kopie an und schreibt das
  Sidecar). Jede dieser Methoden trägt
  `instrument_gui_action!` als erste Anweisung; `start_merge` nutzt als einzige
  Stelle den identischen Debug-Pfad explizit, und beide Merge-Buttons
  (HDR/Panorama) laufen durch denselben instrumentierten Handler — kein
  zweiter, ungetracter Pfad. Klick-Tests je Button liegen in
  `crates/lumina-gui/src/tests/instrdbg_last.rs` (17c) und
  `crates/lumina-gui/src/tests/instrdbg_rework.rs` (Rework: generative
  Checkboxen, Previous/Reset aller acht Panels, Lens-Blur-Enable, beide
  Merge-Buttons, die drei Filmstreifen-Buttons, die KI-Denoise-Enable-Checkbox
  (F-A)) plus No-op-Log-Regressionstests; der Audit prüft die neuen
  Oberflächen `Color` (Point Color), `Generative` und `Filmstrip` mit, die Klick-Tests der
  ersten Slices in `instrdbg_rest.rs` und die Kern-Tests (Format,
  Nested-Suppression, Namenstabelle, Release-Passthrough) in
  `crates/lumina-gui/src/tests/instrdbg_core.rs` (aus `gui_action.rs`
  extrahiert, File-Size-Ratchet). Die beiden WB-Abbruch-Affordanzen
  (`WbEyedropperActive`/`Cancel` setzen nur `wb_pick_mode = false`) bleiben wie
  `disarm_preview_pickers`/`Esc` bewusster reiner Session-State ohne eigene
  `GuiAction`. Tone-Curve-Kanalwahl ist ebenfalls reiner Session-State
  (keine Rezept-Aktion) und bleibt bewusst uninstrumentiert. Die drei
  Filmstreifen-Handler und ihre CAS-Sidecar-Helfer sind für den File-Size-
  Ratchet nach `crates/lumina-gui/src/selection_actions.rs` extrahiert; dort
  steht `instrument_gui_action!` als erste Anweisung, `lib.rs` wächst nicht.
  Analog ist das Detail-Denoise-Panel nach
  `crates/lumina-gui/src/denoise_panel.rs` extrahiert (Ratchet); die
  Instrumentierung steht in `denoise_gui.rs` als erste Anweisung in
  `set_denoise_enabled`.
  **H-1-Schlussbeleg Rework F-1 + F-A + Rest (2026-09-17, eigene Zählung):**
  Im Scope der 17b/17c-Rework-Flächen bleibt kein user-sichtbarer, das
  Edit-Rezept oder Masken mutierender `ui.button`/`ui.checkbox` ohne
  `GuiAction`.
  Vollständige Klassenprüfung: alle 102
  `GuiAction`s sind über den Audit (`f100_action_button`, ohne `_`-Arm)
  einer gezeichneten Oberfläche zugeordnet, und jede instrumentierte Methode
  trägt das Makro als erste Anweisung (Klick-Tests je Button/Checkbox).
  Verbleibende, bewusst nicht instrumentierte Beobachtungen außerhalb der
  Button-/Checkbox-Klasse:
  (a) `F-2` — die Lens-Profil-Auswahl ist eine Radio-/Listenfläche, kein
  `ui.button` (der instrumentierte Handler `set_lens_profile` wird beim Klick
  auf einen Eintrag gerufen); (b) das Klicken einer History-Listenzeile
  (`restore_history`, `selectable_label`) setzt das Rezept auf den
  History-Stand zurück und ist ebenfalls keine Schaltfläche (Session-nahe
  Auswahl-/Restore-Liste, nicht instrumentiert); (c) Batch-/Sammlungs-/
  Smart-Collection-Aktionen (`apply_metadata_batch`, `add_to_collection`,
  `create_smart_collection`, …) sowie die Face-Cluster-Operationen
  (Confirm/Split/Merge) und das Culling-Adopt mutieren
  Metadaten/Katalog-/Dokumentdaten, nicht das `EditRecipe`; (d) die
  Entwicklungs-Profil-Auswahl (`set_profile`) ist eine `ComboBox`/
  `selectable_value`-Liste, kein Button; (e) die Preview-Klick-Edits
  (`set_white_balance_from_point` nach instrumentiertem
  `arm_wb_eyedropper`, `add_red_eye_region` nach instrumentiertem
  `set_red_eye_pick_mode`) sind Canvas-Klicks, keine Schaltflächen;
  (f) Slider-Klasse: alle `set_*_value`-Setter (inkl. der KI-Denoise-Slider
  `set_denoise_strength`/`set_denoise_preserve_detail`) und die
  Shift-Sliderlabel-Geste `apply_auto_endpoint`; (g) reiner Session-State:
  `set_denoise_policy`, Brush-Eraser, Metadaten-Draft-Auswahl, Export-Optionen,
  Reset-Sliders-Präferenz und Masken-Preview.
  **People-View-Rest geschlossen (Rework-Rest GUI-INSTRDBG-17c, 2026-09-17):**
  Der People-View-Button „Use as mask" (`create_face_mask`,
  `crates/lumina-gui/src/face_gui.rs`) ist jetzt eine `GuiAction`
  (`create_face_mask`) mit `instrument_gui_action!` als erster Anweisung,
  eigener Audit-Oberfläche `People` (`f100_action_button`, ohne `_`-Arm) und
  Klick-Test (`crates/lumina-gui/src/tests/instrdbg_face.rs`: Button → genau
  eine Logzeile → Maskendefinition im Rezept und im Sidecar). Das People-Panel
  ist für den File-Size-Ratchet nach
  `crates/lumina-gui/src/people_panel.rs` extrahiert (`face_gui.rs`
  1095→899); die H-1-Aussage „kein user-sichtbarer Edit-Rezept-/Masken-
  mutierender Button/Checkbox ohne `GuiAction`" ist damit vollständig belegt.
  In Release-Builds
  ist die Instrumentierung wegkompiliert: `strings target/release/lumina-gui`
  enthält weder `action=`/`duration_ms=`/`gpu_route=` noch
  instrumentierungs-eigene Aktionsnamen. Einzelne Aktionsnamen erscheinen in
  Release-Strings weiterhin, weil sie in (nicht wegkompilierten)
  `info!`/`warn!`-Interaktionszeilen stehen — etwa
  `set_auto_fill_transparent`, `set_lens_blur_enabled`,
  `restore_section_previous`, `reset_section`, `reload_preset_entries` sowie
  nach Rework F-1 auch `set_expand_beyond_image`/`set_expand_canvas`
  (Verifikationsbefund F-4: die beiden fehlenden `info!`-Interaktionszeilen
  wurden nach DoD §4 ergänzt). Das ist keine Instrumentierung
  und kein Widerspruch zur Wegkompilierung (Nachweis 2026-09-17).
- **Kittest-Gate-Befund (GUI-INSTRDBG-17c-Rework, 2026-09-17):**
  `generative_expand_panel_ready` ist load-abhängig flaky. In Isolation
  (8/8) und bei Standard-Parallelität (8/8) grün, bei hoher Parallelität
  (`--test-threads=64`) reproduzierbar rot (8/8; Standardlauf 1/3). Der Diff
  liegt im asynchron gerenderten Generative-Preview (bei geringer Last nur im
  zeitbasierten `Preview ready`-Overlay-Toast), nicht in Panel-Layout oder
  Text. Die i18n-Rebaseline `8de5585` ist vollständig (das Golden-Update ist
  im Commit), und die 17c-Instrumentierung ist debug-only und damit
  pixelneutral (kein UI-Draw-Code geändert; Release-`strings`-Nachweis).
  Daher kein Rebaseline; eine Stabilisierung (Render-Synchronisation vor dem
  Snapshot) ist ein eigener Folge-Task. Die `kittest_parity`-Goldens
  `parity_paths_*` wurden am 2026-09-19 als `GUI-PARITY-GOLDENS-18` rebaselined
  (verifiziert BESTANDEN, 4/4 grün; Diffs ausschließlich UX-LOOK-Deltas seit
  6b4105e — Toolbar, linke Rail, Footer, History-Labels; numerische Parität
  unverändert: neutral/detail/lensfun maxAbsDiff 0, tinted 1/90.96 dB).
- **Routing-Badge-Befund (GUI-ROUTING-N6, F-103-N6-Runde 1, 2026-09-17):**
  Der gelbe Badge im manuellen Test (`Render routed to CPU: …`) wurde
  reproduziert. Für die committeten RAW-Fixtures mit einem harmlosen
  Basis-Rezept (Exposure/Contrast, Zoom) war der Grund
  `lens_correction (Lensfun corrector)`. Ursache war keine fehlende GPU-Stufe,
  sondern eine **lose Lensfun-Profil-Suche**: Für das nicht in der Datenbank
  vorhandene Paar `Canon EOS R1` + `RF200-800mm F6.3-9 IS USM` hat lensfun ein
  fremdes Objektiv (`RF 24-240mm F4-6.3 IS USM`) bzw. eine falsche Kamera
  (`EOS R`) geliefert. `Corrector::for_camera` sucht jetzt **strikt** (kein
  `LF_SEARCH_LOOSE`); ohne echten DB-Eintrag greift der manuelle Pfad
  („nie ein geratenes Profil"). Damit ist der Badge für die Fixtures weg und
  eine reale Fehlkorrektur behoben. Ein **korrekt gematchter** Lensfun-Corrector
  läuft seit **GPU-LENSFUN-PARITY-1 (2026-09-18)** auf der GPU: Der CPU-Aufbau
  erzeugt eine `LensfunMap` (Warp-/Gain-Map), die der `lumina-gpu`-Resample-Pass
  abtastet (`GpuContext::set_lensfun_map`), mit Oracle-Parität (`maxAbsDiff ==
  0`). Ein GPU-Badge für den Corrector entfällt damit; verweigert/CPU-geroutet
  bleibt nur der Distortion-Fall ohne expliziten Crop (datenabhängiger
  Default-Crop, `feature/architecture/pipeline.md` § Implementierungsstatus
  GPU-Pfad). **GUI-Wiring abgeschlossen (2026-09-18):** Der Present-Pfad baut die
  Map pro Quelle/Dimensionen (`LensfunMap::from_corrector`, gecacht am
  `CachedLensCorrector`) und bindet sie vor jedem `render_to_vram`
  (`crates/lumina-gui/src/lensfun_gpu.rs`, Debug-Diagnostik `kept on CPU:`);
  Headless-Belege: `gpu_audit_lensfun_corrector_presents_gpu_without_badge`
  (Metal, kein Badge; Negativ-Distortion-Fall bleibt laut CPU) und die
  gedrehte `kittest_parity`-Zelle
  `lensfun_corrector_cell_presents_gpu_without_badge` (CPU↔GPU `maxAbsDiff=0`).
  Die Zelle legt zwei Goldens ab (`parity_paths_lensfun_corrector_{cpu,gpu}`);
  weil der VRAM-Present nur die geteilte Preview-Textur speist und die UI nicht
  ändert, ist der GPU-Snapshot **byte-identisch** zum CPU-Snapshot — der
  eigentliche GPU-Pfad ist über `assert_path_parity` (`maxAbsDiff=0`) und die
  `gpu_present_frame_size`-Present-Prüfung gepinnt, nicht über den Snapshot.
- **GUI-GPU-AUDIT-17 (Release 1.0, User-Vorgabe 2026-09-17, F-103-N6):**
  Automatisierter headless Routing-Audit über **alle** 102 `GuiAction`s
  (Quelle der Aktionsliste: `ALL_GUI_ACTIONS`). Der Audit lädt eine
  deterministische synthetische Quelle in eine `LuminaApp` mit **echtem**
  GPU-Kontext (Standalone-Metal-Adapter über `attach_wgpu_render_state`),
  fährt je Aktion den realen Handler, führt danach den echten
  Draft-Hot-Path (`render_draft_tick`: füllt `vram_fresh`/
  `vram_render_refusal`) und den Present-Gate (`update_texture`) aus und
  prüft je Aktion `gpu_routing_fallback_badge()`.
  - **Testanker:** `crates/lumina-gui/src/tests/gpu_audit.rs` (Audit +
    adapter-unabhängige Vollständigkeitsprüfung) und
    `crates/lumina-gui/src/tests/gpu_audit_actions.rs` (erschöpfender
    `GuiAction`-Dispatch ohne `_`-Arm — eine neue Aktion kompiliert nur mit
    Audit-Arm).
  - **Kommandos:** `cargo test -p lumina-gui` (Vollständigkeitstest
    `gpu_audit_exception_table_is_complete_without_gpu`, läuft ohne GPU) und
    `cargo test -p lumina-gui --lib gpu_audit -- --ignored` (Metal-Audit;
    ohne Adapter SKIP statt Rot, wie `kittest_*`; CI-Gap kein Metal in CI).
  - **Ergebnis (lokal, Metal, 2026-09-18; Stand nach GPU-LENSFUN-PARITY-1):** 101
    Aktionen gefahren, 10 dokumentierte CPU-Ausnahmen (die vier
    `GuiAction`-Klassen unten, `default content crop` zählt drei Aktionen),
    keine undokumentierte CPU-Route; der adapter-unabhängige Test pinnt die
    Ausnahmetabelle gegen die dokumentierten Grundklassen. Der Present-Pfad
    selbst bleibt über `kittest_parity` abgedeckt. Mit **LRPAR-G09-SORT-09**
    (2026-09-20) ist die Oberfläche auf **102** Aktionen gewachsen:
    `set_library_sort` ist display-only (Sortier-/Anzeigezustand, kein
    Render-Key, keine Bildstufe) und daher keine CPU-Ausnahme — der
    Metal-Lauf über 101 Render-Aktionen bleibt unberührt.
  - **Dokumentierte CPU-Ausnahmen (explizite Liste, alle laut sichtbar per
    Badge):**

    | Aktion(en) | Badge-Grund | Normative Quelle |
    | --- | --- | --- |
    | `set_lens_profile`, `analyze_upright`, `set_upright_enabled` | `geometry (default content crop)` | CROP-MAXRECT-1 / GPU-MAXRECT-WELLE (`architecture/pipeline.md` § GPU-Pfad): Lens-/Perspektiv-Korrektur ohne explizites `geometry.crop` aktiviert den datenabhängigen MaxRect-Crop. |
    | `set_crop_aspect`, `rotate_step` | `geometry (dimension-changing output…)` | GUI-LENSFUN-GATE-3 F1 (`architecture/pipeline.md` § GPU-Pfad): die readback-freie VRAM-Present-Textur ist quellgroß; ein dimensionsänderndes Rezept wird laut verweigert und exakt auf der CPU präsentiert (Export/Readback bleibt GPU-fähig). |
    | `set_expand_canvas`, `generate_generative_canvas`, `set_expand_beyond_image`, `set_auto_fill_transparent` | `generative_edit (…)` | GEN-ONNX-1 Welle 2b: der readback-freie VRAM-Present ist artifact-blind; Preview/Export laufen per Design über den artifact-aware CPU-Pfad (kein VRAM-Injektionspunkt ohne Readback). |
    | `set_denoise_enabled` | `denoise_ai (not GPU-wired)` | LRPAR-G14-DENOISE-IMPL-20: additive KI-Denoise-Stufe (Release 2.0) hat noch keinen WGSL-Pass; aktive Stufe routet laut auf CPU. |

    **Kein Lensfun-Corrector-Eintrag mehr (GPU-LENSFUN-PARITY-1 GUI-Wiring,
    2026-09-18):** Ein strikt gematchter Correcter ist eine gebundene
    `LensfunMap` und läuft auf GPU (`crates/lumina-gui/src/lensfun_gpu.rs`);
    einzige verbleibende Corrector-CPU-Route ist der Distortion-Fall ohne
    expliziten Crop (`lensfun_map.default_content_crop`) sowie eine nicht
    bindbare/dimensionsfremde Map (`lensfun_map.dimensions`), beide über
    `classify_vram_refusal` als Badge benannt. Headless-Beleg:
    `gpu_audit_lensfun_corrector_presents_gpu_without_badge` (Assertion
    `vram_fresh` + Badge-Abwesenheit; Negativ-Distortion-Fall bleibt laut),
    plus die gedrehte `kittest_parity`-Zelle. Der Audit-Zähler der
    `GuiAction`-Ausnahmen bleibt **10** (der Corrector war nie eine
    `GuiAction`).

  - **Timing-Tabelle (report-only, unkalibriert; kein hartes Gate):** eine
    repräsentative lokale Metal-Messung (Debug, 64×48-Quelle,
    Handler-Wanduhrzeit in Mikrosekunden; die absolute Zahl schwankt mit
    Maschine/Last und wird bewusst nicht gegated). Gesamt:
    **235317 µs** über 101 Aktionen (Summe der Handler-Zeiten, inkl. der
    CPU-Vollrenders/Sidecar-Writes der betroffenen Handler).

    | Aktion | Handler (µs) | Route |
    | --- | ---: | --- |
    | `toggle_before_after` | 10 | present |
    | `toggle_split_view` | 2 | present |
    | `toggle_crop_mode` | 6 | present |
    | `toggle_clipping` | 1 | present |
    | `toggle_softproof` | 1 | present |
    | `toggle_original_histogram` | 4 | present |
    | `toggle_lights_out` | 1 | present |
    | `toggle_panels_hidden` | 0 | present |
    | `toggle_all_panels_hidden` | 1 | present |
    | `toggle_fullscreen` | 3 | present |
    | `toggle_filter_bar` | 0 | present |
    | `toggle_black_white` | 12155 | present |
    | `toggle_stack_group` | 8312 | present |
    | `create_snapshot` | 7009 | present |
    | `duplicate_copy` | 14316 | present |
    | `copy_settings` | 1 | present |
    | `paste_settings` | 7027 | present |
    | `set_rating` | 7101 | present |
    | `set_flag` | 7083 | present |
    | `set_color_label` | 6942 | present |
    | `set_mask_tool` | 5 | present |
    | `set_spot_tool` | 1 | present |
    | `set_treatment` | 101 | present |
    | `set_module` | 0 | present |
    | `set_library_view` | 1 | present |
    | `set_zoom_mode` | 18 | present |
    | `regenerate_stale` | 1 | present |
    | `match_total_exposure` | 7115 | present |
    | `auto_tone` | 6957 | present |
    | `save_recipe` | 7024 | present |
    | `reset` | 86 | present |
    | `render` | 86 | present |
    | `export` | 5216 | present |
    | `start_merge` | 5 | present |
    | `toggle_compare_mode` | 1 | present |
    | `clear_crop` | 1 | present |
    | `set_crop_aspect` | 1 | geometry (dimension-changing output) |
    | `rotate_step` | 1 | geometry (dimension-changing output) |
    | `set_geometry_mirror` | 1 | present |
    | `analyze_upright` | 12755 | geometry (default content crop) |
    | `set_upright_enabled` | 12279 | geometry (default content crop) |
    | `clear_upright` | 3 | present |
    | `create_mask` | 172 | present |
    | `select_mask` | 175 | present |
    | `set_mask_visible` | 7293 | present |
    | `set_show_mask_overlay` | 1 | present |
    | `set_overlay_color` | 0 | present |
    | `create_ai_mask` | 7352 | present |
    | `create_luminance_range_mask` | 6993 | present |
    | `create_color_range_mask` | 6958 | present |
    | `combine_masks` | 6920 | present |
    | `duplicate_mask` | 6973 | present |
    | `set_overlay_mode` | 2 | present |
    | `set_pin_visibility` | 1 | present |
    | `set_solo_mode` | 5 | present |
    | `set_mask_inverted` | 190 | present |
    | `offer_mask_recalculation` | 200 | present |
    | `add_keyword` | 6938 | present |
    | `remove_keyword` | 14062 | present |
    | `commit_metadata_draft` | 111 | present |
    | `clear_metadata_draft` | 104 | present |
    | `copy_metadata_draft` | 111 | present |
    | `paste_metadata_draft` | 107 | present |
    | `clear_metadata_history` | 101 | present |
    | `apply_meta_preset` | 30 | present |
    | `sync_metadata` | 72 | present |
    | `set_spot_mode` | 0 | present |
    | `clear_spot_visualize` | 1 | present |
    | `detect_spot_candidates` | 66 | present |
    | `apply_detected_spots` | 9760 | present |
    | `regenerate_spot_variant` | 2 | present |
    | `clear_spot_heals` | 6857 | present |
    | `detect_red_eye_candidates` | 111 | present |
    | `apply_detected_red_eyes` | 104 | present |
    | `remove_red_eye_region` | 5 | present |
    | `clear_red_eye` | 5 | present |
    | `set_lens_profile` | 2 | geometry (default content crop) |
    | `clear_lens_profile` | 4 | present |
    | `set_lens_blur_bokeh` | 4 | present |
    | `add_curve_point` | 12 | present |
    | `remove_curve_point` | 11 | present |
    | `apply_preset` | 160 | present |
    | `save_preset_file` | 4400 | present |
    | `set_spot_distraction` | 3 | present |
    | `set_red_eye_pick_mode` | 3 | present |
    | `reload_preset_entries` | 104 | present |
    | `arm_wb_eyedropper` | 1 | present |
    | `add_point_color` | 2 | present |
    | `remove_point_color` | 10 | present |
    | `set_expand_canvas` | 202 | generative_edit |
    | `generate_generative_canvas` | 12401 | generative_edit |
    | `set_expand_beyond_image` | 204 | generative_edit |
    | `set_auto_fill_transparent` | 181 | generative_edit |
    | `restore_section_previous` | 7098 | present |
    | `reset_section` | 7084 | present |
    | `set_lens_blur_enabled` | 1 | present |
    | `sync_settings_to_selection` | 6 | present |
    | `match_exposures_of_selection` | 2 | present |
    | `apply_previous_to_selection` | 1 | present |
    | `set_denoise_enabled` | 4 | denoise_ai (not GPU-wired) |
    | `create_face_mask` | 2 | present |

  - **Offen (nicht Teil dieses Implementierungsauftrags):** der **manuelle
    Runde-2-Beleg** der Testfahrt mit Debug-Log (`RUST_LOG=trace`, Zeilen
    `action=<name> duration_ms=<n> gpu_route=<present|cpu-fallback|n/a>`) steht
    aus; der Audit ersetzt ihn nicht (DoD §6 verlangt Log-Ausschnitt). Die
    Automatik belegt die Route über den Present-Gate-State, die manuelle Fahrt
    muss denselben Zustand über die Debug-Instrumentierung (GUI-INSTRDBG-17)
    bestätigen.
- **Ist-Stand 2026-09-04:** Auto-Select (erstes Bild alle Formate, Selektion nie
  leer), `--module`/`--fullscreen`-Flags umgesetzt + verifiziert BESTANDEN
  (281p lib, 7p bins, kittest 11/11, Vision Golden-BESTANDEN); Folgearbeit:
  `.lumina/`-Scan-Ausschluss (Befund B3, hoch).
- Das Histogramm ist eine echte Grafik (gefüllte 256-Bin-Luminanzkurve per
  Painter, P01/P99 als schmale Marker, Mean/Median-Text) in einer eigenen
  einklappbaren Sektion (Default offen) und wird immer aus dem **gesamten
  Bild** (Full-Render, nie nur sichtbarer Viewport/ROI-Ausschnitt) berechnet;
  Draft-/Veraltet-Zustände bleiben sichtbar markiert.
- **„Original Photo“-Vergleich (G-10, LRPAR-G10-VIEWER):** Das Histogramm-Panel
  trägt einen Umschalter „Show original“ (bearbeitet vs. unbearbeitet).
  Datenquellen-Entscheid: **Original-Decode** — die Anzeige nutzt den
  unbearbeiteten Decode (`LuminaApp.original`), **kein** Rezept-Reset. Ein
  Reset-Render wäre ein zweiter, teurer Pipeline-Lauf und würde
  „unbearbeitet“ mit „Default-Rezept“ verwechseln (Demosaic/Decode bleiben
  auch beim Reset aktiv); der Original-Decode ist das wahre „unbearbeitet“.
  Es gibt **keinen zweiten Analysepfad**: Die Original-Messung nutzt dieselbe
  `analyze_tone`/`LuminanceHistogram`-Messung wie der Before/After-Pfad
  (`Y`). Die Histogramm-Full-Render-Regel gilt unverändert — der
  Original-Frame liegt in Vollauflösung vor, nie als Viewport-Ausschnitt.
  Bei aktivem Original zeigt das Panel zusätzlich das Delta (Δ Mean +
  normierte L1-Distanz der 256 Bins, echte Analysewerte). Der Umschalter ist
  reiner Session-Display-State (nie Rezept/Sidecar) und loggt `info!`.
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

### Ruckel-Attribution (`GUI-JANKLOG-19`, IMPLEMENTIERT 2026-09-19)

> **Status: IMPLEMENTIERT (verifiziert BESTANDEN 2026-09-19).** Diese
> Untersektion war der SOLL-Entscheid zu `GUI-JANKLOG-19` (Release 1.0) und
> ist seit 2026-09-19 umgesetzt: Slice **`jank_log.rs`**
> (`crates/lumina-gui/src/jank_log.rs`, ≤ 500 Zeilen, kein Baseline-Eintrag),
> Build-Opt-in per Cargo-Feature **`janklog`** (nicht default), nur
> Debug-Builds (`debug_assertions`), Release still per Konstruktion.
> Schwelle Default **8,3 ms** (120-Hz-Budget), Override `LUMINA_JANK_MS`
> (`0` = aus mit einmaligem `info!`, unparsbar = `warn!` + Default).
> Andockstellen: `dirty.rs` (Dirty-Key), `render_tick.rs`/`render_entry.rs`
> (Teil-Dauern), `present.rs` (Route/Badge), `gui_action.rs` (Makro-Scope),
> `lib.rs` (nur `mod`-Zeile). Tests: `jank_log::tests::*` — Slow-Render und
> Slow-Action erzeugen genau eine attribuierte Zeile, Normalbetrieb bleibt
> still (Stille-Test), U6-Lautheit per Regressionstest gepinnt (Re-Verifizierung BESTANDEN
> 2026-09-19). Einzige bekannte Grenze: Route/Badge wird beim Scope-Eintritt gelesen (Stand des zuletzt gemalten Frames — Verifizierungsbefund 3,
> niedrig, dokumentiert). U6/U7-Reste (Verhältnis zu `LUMINA_PERF_LOG`)
> bleiben Default bis Freigabe.

**Ziel.** Standardbetrieb bleibt im Log still (kein Per-Frame-Spam); nur
nachweislich langsame Aktionen/Render erzeugen **genau eine** attribuierte,
greppbare Zeile, aus der die Kette Aktion → Rezept-Änderung →
Renderpfad/Route → Teil-Dauern ablesbar ist. Kein stiller Fallback: eine
unlesbare Schwelle/Umgebungsvariable wird laut gemeldet, nie stillschweigend
deaktiviert. Reine Diagnose — keine Rezept-/Sidecar-/Pixel-Auswirkung.

**Bestand (Abdeckungsbefund 2026-09-18, kein Doppelbau).**

- `GuiActionTimer`/`instrument_gui_action!` (`gui_action.rs`) loggt in
  Debug-Builds **unbedingt jede** Aktion (`action=… duration_ms=… gpu_route=…`),
  ohne Schwelle; in Release ist alles wegkompiliert.
- `mark_recipe_dirty(key, value)` kennt den Dirty-Key, gibt ihn aber an kein
  Log weiter; `mark_dirty()` kennt nur den Invalidierungszustand.
- `render_draft_tick` misst `gpu_ms`/`cpu_draft_ms`/`analyse_ms` in
  `DragTickTimings`, loggt sie aber auf einer separaten `trace!`-Zeile ohne
  Bezug zur auslösenden Aktion.
- `LUMINA_PERF frame=…` loggt in beiden Profilen, aber nur bei
  `LUMINA_PERF_LOG=1`, und ist Per-Frame-/Scroll-Diagnose, keine
  Aktions-Attribution.
- Route/Badge liegen als `gpu_route_label()` bzw. `routing_fallback_reason()`
  vor, sind aber nicht mit der Aktionsdauer verknüpft.

**1. Langsam-Schwelle.**

| Parameter | Default-Vorschlag | Konfiguration | Begründung |
| --- | --- | --- | --- |
| Langsam-Schwelle | **16,7 ms** | `LUMINA_JANK_MS` (ganzzahlige Millisekunden) | entspricht genau einem 60-Hz-Frame-Budget und der bestehenden `slow_frame`-Definition (`LUMINA_PERF` zieht dieselbe 16,7-ms-Grenze) |

- **Belegte Kalibrierung:** Die committete Handler-Messung der GPU-AUDIT-17-
  Timing-Tabelle (Debug, 64×48-Quelle) hat als größten Einzelwert `duplicate_copy`
  mit 14,3 ms; alle 101 Aktionen liegen ≤ 16,7 ms. Der dokumentierte
  Normal-/Auditbetrieb bleibt damit unter dem Default **still**, während ein
  echter Slow-Render (simulierte Verzögerung) sicher auslöst.
- **Wo konfigurierbar:** einmalig beim Start gelesen (kein Per-Frame-Parsen),
  reine Laufzeit-Diagnose, **nicht** im Sidecar/Rezept persistiert. Ein
  `LUMINA_JANK_MS`-Wert `0` deaktiviert die Zeile ausdrücklich (mit einmaligem
  erkennbarem Hinweis), ein unparsbarer Wert erzeugt `warn!` + Default 16,7 ms —
  kein stiller Fallback.
- **Bewusst offen:** ob Aktionen und Render-Ticks dieselbe Schwelle teilen oder
  je `kind` eine eigene bekommen (Frage U1).

**2. Log-Level Debug vs. Release (Runde 2 fährt Release).**

Ausgangslage: GUI-INSTRDBG ist `#[cfg(debug_assertions)]` — in Release sind
Timer, Log und Format-Strings vollständig wegkompiliert. Der manuelle
Runde-2-Beleg läuft jedoch im Release-Build. Entscheidungsvorlage:

- **Option A — Debug-only (Status quo):** Zero-Overhead in Release, aber Runde 2
  sieht nichts; Diagnose nur im Debug-Build. Trade-off: erfüllt das
  Zero-Overhead-Prinzip, verfehlt den Runde-2-Diagnosebedarf.
- **Option B — Empfehlung:** in **beiden** Profilen instrumentiert, Emission
  hinter Laufzeit-Opt-in `LUMINA_JANK_LOG=1` (Default aus in Release, an in
  Debug). Ist es aus, prüft der Trigger nur ein gecachtes
  `AtomicBool`/`OnceLock` — kein `Instant::now()` auf Idle-Frames, keine
  Allokation → praktisch zero overhead im Normalbetrieb. Ist es an, wird die
  Zeile als `warn!` emittiert (sichtbar bei Default-`RUST_LOG=info`,
  unmissverständlich), damit Runde 2 ohne Log-Level-Tuning funktioniert.
  Trade-off: Code und Format-Strings bleiben im Release-Binary (etwas größere
  Binärdatei), Messpunkt bleibt minimal. Tests nutzen denselben thread-lokalen
  Capture-Seam wie INSTRDBG statt des Env-Vars.
- **Option C — immer im Code, nur `debug!`, `RUST_LOG` filtert:** gleiches
  Laufzeitverhalten wie B ohne separaten Env-Schalter, aber Runde 2 müsste
  `RUST_LOG=debug` setzen — dann überschwemmen alle übrigen `debug!`-Zeilen die
  greppbare Jank-Zeile. Trade-off: kein zusätzlicher Schalter, dafür schlechtere
  Greppbarkeit in Runde 2.

**Empfehlung: Option B** — Zero-Overhead im Normalbetrieb (Default aus) und
zugleich Release-Diagnose per ausdrücklichem Opt-in. Der genaue Level
(`warn!` vs. `info!` vs. `debug!`) bleibt Freigabefrage U2.

**3. Trigger-Format (greppbar).**

Genau **eine** Zeile pro langsamem Vorgang, mit stabilem Präfix `LUMINA_JANK`:

```text
LUMINA_JANK kind=<action|render> action=<name|-> recipe_key=<key|-> route=<present|cpu-fallback|n/a> badge_reason="<reason|->" total_ms=<n> gpu_ms=<n> cpu_draft_ms=<n> analyse_ms=<n>
```

Feld-Regeln (Reihenfolge fix, Werte whitespace-frei außer dem gequoteten
`badge_reason`, keine freien Texte):

- `kind` — `action` (GuiAction-Scope) oder `render` (Draft-/Full-Render-Tick
  außerhalb eines Aktions-Scopes).
- `action` — `GuiAction::name()` des jüngsten instrumentierten Nutzer-Aufrufs
  (`-`, wenn keiner, z. B. beim Debounce-Render).
- `recipe_key` — Dirty-Key aus `mark_recipe_dirty`/`set_adjustment`/
  `set_presence` (z. B. `exposure`, `presence.clarity`); `-` bei reiner
  View-/Session-Änderung.
- `route` — `gpu_route_label()` (`present|cpu-fallback|n/a`), unverändert
  übernommen.
- `badge_reason` — `routing_fallback_reason()`, in Anführungszeichen (der Grund
  enthält Leerzeichen), `-` wenn keine CPU-Route.
- `total_ms` — Gesamtdauer der äußeren Aktion bzw. des Render-Ticks.
- `gpu_ms`, `cpu_draft_ms`, `analyse_ms` — Teil-Dauern aus `DragTickTimings`
  (`-`, wo nicht zutreffend).
- Emission **genau einmal** am Ende des äußersten Aktions-Scopes (RAII-Drop
  analog `GuiActionTimer`); verschachtelte Messpunkte (Aktion → Render-Tick)
  erzeugen **keine** zweite Zeile, sondern füllen dieselbe Zeile. Damit ist die
  Abnahme „genau eine attribuierte Zeile“ strukturell erfüllt.
- Kein Datum/Zeitstempel im Format duplizieren; der bestehende Logger stellt
  `[WARN] <target>:` voran.

Damit sind `grep 'LUMINA_JANK'`, `grep 'LUMINA_JANK.*kind=render'` und die
Feld-Greps (`action=`, `recipe_key=`, `route=`) stabil. Einzeiler ist die
Empfehlung; ein Zwei-Zeilen-Fallback mit `seq=<n>` bleibt Freigabefrage U4.

**4. Andockstellen (kein Bau in diesem Schritt).**

- `crates/lumina-gui/src/jank_log.rs` (**neu, S1.5**): einmalige
  Schwellen-/Env-Auswertung, reine Format-Funktion (testbar), Emission,
  thread-lokaler Capture-Seam für Tests — einzige Quelle des Formats.
- `dirty.rs` (W1 S1.3): liefert den jüngsten Dirty-Key an den Jank-Record
  (verhaltensneutrale Beobachtung; Invalidierungsinvariante und das
  `set_adjustment`-Duplikat bleiben unangetastet).
- `render_tick.rs` (W1 S1.1) / `present.rs` (W1 S1.4a): `gpu_ms`,
  `cpu_draft_ms`, `analyse_ms` aus `DragTickTimings` bzw. Present-Ergebnis und
  Badge-Grund fließen in denselben Record; `gpu_routing.rs` (S1.4b) liefert die
  Route.
- `gui_action.rs`: der bestehende `GuiActionTimer` bleibt Debug-Format; der
  Jank-Record hängt sich an denselben äußersten Scope (kein zweiter Timer).
- `logger.rs`: `RUST_LOG`-/`LUMINA_JANK_LOG`-Auswertung, Default-Level `info`.
- `LUMINA_PERF`-Frame-Pfad (`lib.rs`): liefert weiter `slow_frame` und bleibt
  unverändert; JANKLOG ist Aktions-Attribution, keine zweite Per-Frame-Quelle.

**5. Test-/Abnahmeanker (Stille-Test, DoD).**

- Headless-Test (neue Testdatei strikt ≤ 500, `cfg(debug_assertions)` plus
  Capture-Seam): Normalbetrieb (Idle-Frames und Aktionen unter der Schwelle)
  erzeugt **null** `LUMINA_JANK`-Zeilen; ein simulierter Slow-Render
  (künstliche Überschreitung) erzeugt **genau eine** Zeile mit vollständiger
  Kette.
- Fehlerfall-Anchor: unparsbarer `LUMINA_JANK_MS` → `warn!` + Default, nie still
  deaktiviert (Regressionstest gegen den stillen Fallback).
- `cargo test -p lumina-gui` grün ohne GPU; keine Pixel-Änderung (kein
  Draw-Code), optional kittest-Byte-Identität als Nachweis.

**6. Abgrenzung / Nicht-Ziel.**

- Keine Sidecar-/Rezept-Felder, keine Persistenz, keine Migration
  (Diagnose only, Sidecar bleibt alleinige Quelle für Bearbeitungen).
- Kein Per-Frame-Log im Normalbetrieb; `LUMINA_PERF` bleibt separat.
- Kein Ersatz für den manuellen Runde-2-Log-Beleg (DoD §6), sondern dessen
  Release-taugliche Grundlage.
- Keine Änderung an Renderpfad, Pixeln oder GPU-Routing.

**7. Offene Freigabefragen an den User.**

- **U1 Schwelle:** Default 16,7 ms (Vorschlag) oder wahrnehmungsnäher 33/50 ms?
  Eigene Schwelle je `kind` (action vs. render)?
- **U2 Release-Sichtbarkeit:** Option B (Empfehlung) oder A/C? Wenn B: Level
  `warn!` (Vorschlag), `info!` oder `debug!`? Env-Namen `LUMINA_JANK_LOG` /
  `LUMINA_JANK_MS` in Ordnung?
- **U3 Formatdetails:** Einzeiler wie spezifiziert? `badge_reason` gequotet
  (Vorschlag) oder whitespace-frei sluggen? Zusätzliche Felder gewünscht
  (`frame=`, `seq=`, `source=`)?
- **U4 Ein Zeile vs. Sequenz:** Einzeiler (Vorschlag) oder zwei korrelierte
  Zeilen?
- **U5 Debug-Default:** JANKLOG in Debug standardmäßig an (wie INSTRDBG) oder
  ebenfalls Opt-in?
- **U6 Deaktivierung:** Semantik von `LUMINA_JANK_MS=0` (Vorschlag: aus, mit
  einmaligem erkennbarem Log) und Verhältnis zu `LUMINA_PERF_LOG`.
- **U7 Geltung:** nur GUI oder auch CLI-Render (globale Diagnose)?

**Freigabe 2026-09-18 (User-Entscheide, verbindlich für S1.5):**
- **U1 Schwelle: 8,3 ms (120-Hz-Budget).** Kein 144-Hz-Display: MacBook Pro =
  ProMotion adaptiv bis 120 Hz (Apple-Specs) → Frame-Budget 8,33 ms. Eine
  Schwelle je `kind` bleibt möglich, Default einheitlich 8,3 ms.
- **U2 Release: nur Debug.** Release-Builds bleiben still (Zero-Overhead-Prinzip);
  kein Release-Opt-in. Runde-2-Diagnose läuft in Debug-Builds.
- **U3/U4 Format: Einzeiler** wie spezifiziert (`badge_reason` gequotet, keine
  Zusatzfelder vorerst).
- **U5 Debug: Build-Opt-in (User-Idee).** JANKLOG ist in Debug-Builds nicht
  standardmäßig an, sondern per Cargo-Feature zuschaltbar (kein Env-Opt-in,
  kein Immer-an). Feature-Name bei S1.5-Umsetzung festlegen.
- U6/U7 bleiben offen (Default-Vorschläge des Entwurfs gelten bis zur Freigabe).

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

### Generative/AI-Neuberechnung pro Modul (F-100, User-Vorgabe 2026-09-16)

- Jede ableitbare AI-/Analysegröße (Denoise-Artefakt, Face-Analyse,
  Culling-Vorschlag, Merge-DNG, AI-Masken) ist **einzeln neu generierbar**:
  Jedes Modul besitzt eine eigene explizite Generieren-Aktion (Button/Command
  pro Modul, z. B. pro Sektion bzw. `denoise`/`face`/`cull`/`merge-hdr`/
  `merge-pano`), die nur diese Größe neu erzeugt und alle anderen
  persistierten Artefakte unverändert lässt.
- **Default ist „alle neu generieren":** Die Sammelaktion (bzw. der Default
  ohne Modulauswahl) erzeugt alle veralteten/fehlenden Größen neu; die
  Einzelauswahl schränkt explizit ein. Kein Modul wird je implizit oder
  automatisch neu berechnet — jede Neuberechnung ist eine ausdrückliche
  Aktion (kein stiller Fallback, keine Auto-Neuberechnung als einzige Option).
- **Umfang 1.0 / Erweiterung:** In 1.0 gilt die Konvention für alle dort
  vorhandenen Module; später hinzukommende Module (Denoise, Face, Culling,
  Merge) hängen sich in dieselbe Konvention (eigene Aktion + Sammel-Default).

#### 1.0-Inventar und konkrete Aktionsfläche (GUI-GEN-GRANULAR-10)

- **1.0-Module (Ist-Stand 2026-09-17, verifiziert am Code):** ableitbare,
  persistierte Analysegrößen sind `masks` (AI-Masken-Inferenz), `auto-tone`
  (Auto-Tone: sechs Regler + `analysis_fingerprint`, enthält „Auto-Exposure“
  als `auto_exposure`) und `matching` (F-008 Exposure Matching /
  `matched_exposure`). **Auto-WB** ist in `architecture/pipeline.md` normativ
  in der optionalen Reihenfolge genannt, aber in 1.0 **nicht implementiert**
  (es gibt nur den As-Shot-WB-Kontext aus RAW-Metadaten, keine persistierte
  Auto-WB-Analyse); es erhält daher noch keine eigene Aktion und hängt sich
  beim Implementieren in dieselbe Konvention. Nicht Teil dieses Slices sind
  die späteren Module laut Releaseplan: Merge (1.5), KI-Denoise (2.0), Face
  (2.0), Culling (2.5) sowie GenerativeEdit/Spot-generativ (Post-MVP).
- **CLI (eine Sammel- und Modulaktion):**
  `lumina regenerate --input <DATEI> [--virtual-copy <ID>] [--json]
  [--module masks|auto-tone|matching]…`.
  Ohne `--module` ist es die **Sammelaktion** und erzeugt **nur veraltete oder
  fehlende** Größen neu (die drei Module werden unabhängig geprüft). Ein
  explizites `--module` **erzwingt** genau dieses Modul (auch wenn der aktuelle
  Wert frisch aussieht) und lässt alle anderen Größen unangetastet. Die
  Aktion ist idempotent und schreibt nur bei tatsächlicher Änderung atomar.
  Die Auto-Tone-Frischeprüfung verlangt den **vollen** AUTO-TONE-2-Vertrag
  (sechs Adjustments + sechs Spiegel + Fingerprint). **Offen (bewusst nicht in
  diesem Slice):** `process --auto-tone` persistiert weiterhin nur
  `exposure`/`contrast` (`process`-Artefakt); `regenerate` erkennt diesen
  unvollständigen Stand deshalb als stale und regeneriert ihn. Eine
  Vereinheitlichung von `process` ist ein eigener Folgeentscheid.
- **GUI:** Die Modulaktionen sind die vorhandenen Buttons `Auto`
  (Grundtonung, `auto_tone`), `Match Exposure` (Footer, `match_total_exposure`)
  und `Recalculation` je Maske. Die fehlende **Sammelaktion** ist der Button
  „Regenerate stale/missing“ im Develop-Footer: er führt genau die
  Modulaktionen für alle veralteten/fehlenden Größen aus und überspringt
  frische (idempotent, nie implizit).
- **Kein impliziter Re-Compute:** `regenerate` (CLI) bzw. die Sammelaktion
  (GUI) ist der einzige *Sammel*-Pfad. Die bestehende Render-Zeit-Auflösung
  der Maske (F-048/F-051: veraltete/fehlende Maske wird bei aktivem Modell
  re-inferiert) bleibt funktional, wird aber jetzt **laut** gemeldet: die
  Entscheidungsschicht hängt für jede *nicht ausdrücklich angeforderte*
  Re-Inferenz (`refresh == false` **und** kein persistierter `Pending`-Marker)
  eine Warnung an `MaskLoadResult.warnings`, die die CLI als
  `warning: …`/`mask_warnings` ausgibt. Eine explizite Refresh-Anforderung
  bleibt bewusst leise — sie ist die gewollte, sichtbare Aktion. **Drei
  Ausprägungen expliziter Masken-Anforderung sind zu unterscheiden:**
  `develop`/`export`/`batch --update-masks` setzen den kopweiten One-Shot-
  Schalter `recipe.options["update_masks"] = "true"` (der Render konsumiert und
  entfernt ihn wieder; er übersteuert den Persisted-Valid-Fastpath für **alle**
  erreichbaren Quellmasken). `mask --update-masks` und `regenerate --module
  masks` markieren die betroffenen Masken als `Pending` **und** armen denselben
  kopweiten Schalter, damit der Render die Re-Inferenz als ausdrücklich
  angefordert erkennt und nicht als implizit meldet. Die **Sammelaktion** (kein
  `--module`) armiert den kopweiten Schalter dagegen **nicht**: sie markiert
  ausschließlich die veralteten/fehlenden Quellmasken als `Pending`; deren
  persistierter `Pending`-Marker gilt der Entscheidungsschicht als
  ausdrückliche Anforderung (leise Re-Inferenz nur dieser Masken), während
  frische `Valid`-Masken unberührt auf dem Persisted-Valid-Fastpath bleiben.
  Ohne diese Trennung würde die Sammelaktion über den kopweiten Schalter auch
  frische Masken re-inferieren und damit dem SOLL „nur veraltete oder fehlende"
  widersprechen. Auto-Tone/Auto-Exposure/Matching werden nie als
  Render-Nebeneffekt neu berechnet.
  **Offene SOLL-Spannung (Entscheid nötig):** Die reine F-100-Lesart („nie
  implizit") würde die F-048-Render-Zeit-Re-Inferenz ganz verbieten und nur
  noch bei `refresh == true` erlauben. Dieser Slice macht sie stattdessen
  laut (User-Vorgabe „ggf. laut machen oder entfernen", 2026-09-17). Eine
  strikte Variante ist eine bewusste F-048-Revision (Render verweigert statt
  zu re-inferieren) und bleibt eine eigene Entscheidung, keine stille
  Nebenänderung.
- **GUI-Sammelstatus:** Die Sammelaktion gibt **genau eine** zusammengefasste
  Statusmeldung aus; die Modulliste ist dedupliziert (ein Modul erscheint
  höchstens einmal), auch wenn mehrere Masken markiert wurden.
- **Masken-Artefakt-Persistenz (bewusste Grenze):** Die `.lumina.zdata`-Persistenz
  re-inferierter Matten ist die dokumentierte offene F-082-Grenze
  (`feature/product/ai-masks.md`). `regenerate --module masks` fordert daher
  eine ausdrückliche Refresh-Resolution an (Status `Pending` + armer
  One-Shot-Schalter, s. o.); der nächste Render
  konsumiert sie und verweigert ohne Engine laut. Es wird **kein**
  Stub-Ergebnis als gültiges Artefakt persistiert. Sobald F-082 geschlossen
  ist, wird die Matte direkt persistiert, ohne die Aktionsfläche zu ändern.
- **Byte-Identität (bekannte Grenze, 2026-09-17):** „Unangetastet" gilt
  strukturell (kein Modul schreibt fremde Felder), nicht streng byteweise:
  der serde_json-Load/Save-Roundtrip kann fremde Float-Felder um 1 ULP
  verschieben (ohne `float_roundtrip`, vorbestehend). Exakter Byte-Roundtrip
  ist ein eigener Folgeentscheid.
- **Stand 2026-09-17 (GUI-GEN-GRANULAR-10, Verifizierung BESTANDEN):**
  `lumina regenerate` (CLI, je Modul + Sammel-Default), GUI-Sammel-Button +
  Modulaktionen, laute Render-Re-Inferenz, `info!`-Logs; 9 CLI-E2E + GUI-/Core-
  Tests grün. Offen: F-048-Revision, `process`-Vereinheitlichung, Byte-Roundtrip
  (s. o.), Auto-WB-Aktion bei Implementierung.
- **Abgrenzung: Rote-Augen-Erkennung (LRPAR-G14-REDEYE-AUTO-15, 2.0)** ist
  bewusst **kein** `regenerate`-Modul. `regenerate` regeneriert veraltete oder
  fehlende *aktivierte* analysierbare Größen; die rote-Augen-Erkennung ist eine
  einmalige, explizite Analyse, die sonst im Sammel-Default Regionen ohne
  Nutzeraktion vorbefüllen würde (stille Vorbefüllung). Sie folgt daher dem
  Muster `upright --analyze` bzw. `spot --detect-objects`/`--detect-apply`:
  CLI `lumina red-eye --detect` (listen) / `--detect-apply` (persistieren),
  GUI „Detect pupils" / „Apply detected". Details und Schwellen:
  `architecture/pipeline.md` § G-14.

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
| `Shift`+Doppelklick auf `Whites`/`Blacks`-Label | Auto-Weißpunkt / Auto-Schwarzpunkt (nur dieses Feld aus `suggest_auto_tone`, kein Zweit-Algorithmus) | G-16; `Shift`+Doppelklick auf anderen Labels = normaler Einzel-Reset; ohne `Shift` = normaler Einzel-Reset |
| `Alt`+Regler (Track-Drag/Scroll an Ton-Reglern) | Maskierungsvorschau: Clipping-Badge (`J`-Pfad) solange `Alt` gehalten | G-16; Scope: `exposure`/`contrast`/`highlights`/`shadows`/`whites`/`blacks`; Label-`Alt`-Klick bleibt Einzel-Reset, `Alt`-Scroll-Feinjustierung bleibt |
| `S` | Softproof-Vorschau umschalten (reines Anzeige-Badge, nie Rezept) | G-16 + G-10: klickbarer Schalter zusätzlich im Histogramm-Panel (gleicher `info!`-Pfad); Scope-Entscheid (G-10, ehrlich): MVP ist nur Toggle+Badge — eine echte Druck-/Gamut-Simulation braucht einen Pipeline-Anker in `lumina-core` (Output-Profil-/Gamut-Stufe um `output.profile`, s. `crates/lumina-core/src/pipeline.rs`) und bleibt ein eigener Folge-Slice mit Crate-übergreifendem Schema-/Render-Entscheid, kein GUI-Workaround |
| `Cmd/Ctrl+H` / `Cmd/Ctrl+M` | HDR-Merge (`merge-hdr`) / Panorama-Merge (`merge-pano`) starten (Jobsteuerung via Poll, Status sichtbar) | G-13, MERGE-GUI; kein `K`-Konflikt (`M` allein bleibt Maske) |

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

### Library-Parität G-09 (LRPAR-G09-LIB, Release 1.0)

Normative GUI-/CLI-Fläche für `.goal/Goal.md` G-09 (Grid/Loupe/Compare/
Survey-Vollparität + Katalog-/Ordner-Verwaltung). Assisted/KI-Culling ist
explizit **nicht** Teil dieses Slices (2.5, LRPAR-G09-CULL-IMPL-25).
Bestehende Library-/Metadata-Pfade (Filter, Badges, `apply_batch_op`,
`save_sidecar`/`load_sidecar`, Smart-Katalog) werden wiederverwendet — kein
Zweit-Mechanismus.

- **Ansichten:** Das Bibliotheks-Modul kennt fünf Ansichten (`LibraryView`):
  `Grid` (Miniatur-Raster, Default), `Loupe` (Einzelbild groß: aktive
  Auswahl), `Compare` (Vorher/Nachher-Vergleich des aktiven Bildes über den
  bestehenden `before_after`-Pfad), `Survey` (Mehrbild-Vergleich der
  Filmstreifen-Auswahl, größere Zellen; bei < 2 ausgewählten Bildern das
  gefilterte Raster). Alle Ansichten teilen dieselbe Auswahl-Buchhaltung
  (`filmstrip_selection`, Pfad-schlüssel, nie Index) und dieselben Filter
  (`\`-Leiste + aktive Sammlung). Der Ansichtswechsel ist reiner
  Session-Display-State (nie Rezept/Sidecar) und loggt `trace!`.
- **Shortcuts:** `G` → Bibliothek/Grid, `E` → Bibliothek/Loupe (Alias-Doku
  aus `module_for_key` bleibt: kein separates Loupe-Modul),
  `C` → Compare (setzt `before_after`, erneutes `C` verlässt),
  `N` → Survey (Sprung ins Bibliotheks-Raster, erneutes `N` verlässt den
  Modus). Alle vier werden wie alle F-100-Kürzel ignoriert, solange ein
  Widget Tastatureingaben erwartet. `Cmd/Ctrl+Shift+I` (Bibliothek/Import)
  und `Cmd/Ctrl+Shift+E` (Exportieren) bleiben reine Modulwechsel.
- **People-Ansicht (G-12, FACE S5, 2026-09-16):** fünfte `LibraryView`-Ansicht
  `People` (Cluster/Personen aus der persistierten `FaceAnalysis`, Namen
  vergeben, confirm/split/merge als reine Daten-Ops, `person:`-Filter,
  Status-Warnungen stale/missing). Kein Karten-Modul/GPS (nie Ziel).
- **Assisted-Culling-Sektion (G-09, CULL Stufe 1, 2026-09-16):**
  Library-Badges (keep/review/reject/none/stale via `CullingReadState`) +
  `cull:`-Filter + explizite Übernahme-Aktion (schreibt nur
  `document.culling`, nie Rating/Flag/Label/Rezept; kein Auto-Rating).
- **Merge-Sektion (G-13, MERGE-GUI, 2026-09-16; F6-Dedup 2026-09-17):**
  `merge-hdr`-/`merge-pano`-Aktionen, Jobsteuerung via Poll, DNG-Artefaktstatus
  (`ok`/`stale`/`missing`/`unsupported`/`none`), Envelope-Konflikte laut +
  non-destruktiv. Seit MERGE-IMPL-15 ruft die GUI **denselben** gemeinsamen Pfad
  `lumina_merge::bundle::run_merge` wie die CLI auf (keine GUI-eigene
  Merge-Schrittfolge; nur Decode-Adapter + Exposure-Policy bleiben frontend-
  spezifisch), Paritätsanker via `lumina-cli/tests/merge_parity.rs`. Shortcuts
  `Cmd/Ctrl+H` (HDR-Merge) / `Cmd/Ctrl+M` (Panorama-Merge).
- **AI-Denoise-Panel (G-14, DENOISE-GUI, 2026-09-16):** Detail-Sektion
  (enabled/strength/preserve_detail, Modell-Identität lesbar), Status-Badge
  (`ready`/`stale`/`missing`/`corrupt`/`unavailable`/`inactive`), Strict
  bricht laut ab, Warn rendert mit sichtbarem Badge.
- **Auswahl-Semantik:** Einfachklick = Auswahl (ohne Öffnen),
  `Cmd/Ctrl`-Klick = Toggle, `Shift`-Klick = Bereich ab Anker. Öffnen:
  Doppelklick im Grid = Laden + Wechsel zu Develop, Doppelklick in Survey
  und `Enter` (alle Library-Ansichten) = Laden + Wechsel zu Loupe.
  Löschen oder
  Verschieben eines Bildes stabilisiert die Auswahl auf den Nachfolger an
  der entfernten Rasterposition (Bestand: `stabilize_selection`).
- **Tastatur-Navigation (Bibliothek, ohne Textfokus):** `Pfeil links/rechts`
  = ±1 im gefilterten Raster (Auswahl + Anker folgen, ohne Öffnen),
  `Pfeil hoch/runter` = ±eine Rasterzeile, `Home`/`End` = erstes/letztes
  Bild, `Enter` = Öffnen des aktiven Bildes, `Esc` = zurück zu Grid. Reine
  Index-Arithmetik (`library_move_index`, Clamp, nie Wrap) als pure
  Funktion, headless getestet.
- **Katalog-/Ordner-Verwaltung (ordentlich, mit Sidecar-Begleitung):**
  `create_folder` (laut bei existierendem Ziel), `rename_folder`
  (Verzeichnis-`rename`; Sidecars liegen neben den Quellen und ziehen
  automatisch mit), `move_image_to_folder` (Bild **plus**
  `<name>.lumina.json` **plus** `<name>.lumina.zdata`, sofern vorhanden;
  Ziel-Überschreiben wird laut verweigert, kein stiller Datenverlust),
  `delete_image_with_sidecars` (Bild + beide Begleiter; fehlende Begleiter
  sind kein Fehler), `delete_empty_folder` (nur leere Verzeichnisse, laut
  sonst). Jede Aktion loggt `info!` je Pfad, meldet Fehler laut (Status +
  `GuiError`, nie still) und listet danach das Verzeichnis neu.
  Volumen-übergreifende Moves (z. B. internes → externes Laufwerk) nutzen
  Copy+Remove als Fallback, wenn `rename` mit `EXDEV` scheitert. Wird das
  **geladene** Bild verschoben, flusht ein offener Edit zuerst ans alte
  Sidecar und die Session zeigt danach auf das verschobene Bundle (Pfad +
  Sidecar-Revision + Verzeichnis); wird es gelöscht, werden offener Edit,
  Pfad und Revision verworfen, sodass kein späterer Save ein verwaistes
  Sidecar am alten Ort erzeugen kann. Das Verschieben auf Benutzerwunsch
  ist eine ausdrückliche Datei-Operation und keine Pipeline-Überschreibung:
  Die Nicht-Destruktivitäts-Regel (kein Render/Export ersetzt je ein
  Original) bleibt unberührt. Persistierte Daten enthalten nie absolute
  Pfade (Sidecar-Relativitäts-Regel).
- **CLI:** `lumina relocate --from <bild> --to <bild> [--json]` verschiebt
  ein Bild **mit** seinen Sidecar-Begleitern (`.lumina.json`,
  `.lumina.zdata`, sofern vorhanden) an den Zielpfad. Verweigert laut ein
  existierendes Ziel und eine fehlende Quelle (Exit 1, kein Halb-Zustand:
  Begleiter werden erst nach erfolgreichem Bild-Move versetzt; ein
  fehlgeschlagener Begleiter-Move meldet laut, das Bild liegt dann bereits
  am Ziel — kein stiller Verlust; in diesem Halb-Zustand wird die
  Verzeichnisliste nicht neu aufgebaut). Volumen-übergreifende Moves nutzen
  Copy+Remove als Fallback bei `EXDEV`. Nach dem Move verifiziert `inspect`
  den Roundtrip (`valid`). Exit-Codes wie Bestand: `0` Erfolg, `1`
  Laufzeitfehler, `2` Benutzungsfehler (clap).
- **Status (LRPAR-G09-LIB):** umgesetzt — `LibraryView` (Grid/Loupe/
  Compare/Survey) mit `G`/`E`/`C`/`N`-Bindung, Auswahl-Semantik +
  Tastatur-Navigation (Arrows/Home/End/Enter/Esc), Ordner-Operationen mit
  Sidecar-Begleitung + headless E2E-Tests (Setter → Datei → Reload),
  CLI-`relocate` mit Exit-Codes; kein KI-Culling (2.5).

### Library-Sortierung (LRPAR-G09-SORT-09, Release 1.0)

Normative Sortier-/Custom-Order-Fläche der Library (Grid + Filmstrip). Sie baut
auf der bestehenden Display-Order (`raw_entry_indices`/`filtered_library_order`)
und der Stapel-Logik (LRPAR-G15-STACK-15) auf — kein Zweit-Mechanismus. Die
Sortierung ist reine Anzeige-Reihenfolge: sie verändert **nie** Rezept, Sidecar
oder Original und erzeugt keine Kopien.

- **Modi (abschließend, User-Entscheid 2026-09-19):** genau drei —
  `Name` (Dateiname, lexikografisch, der Default), `Aufnahmedatum (EXIF)`
  (aufsteigend nach `RawMetadata.timestamp`; Einträge ohne Zeitstempel
  sortieren deterministisch **nach** allen mit Zeitstempel, Tie-Break
  Dateiname) und `Custom` (manuelle Reihenfolge). Andere Modi gibt es nicht.
- **Wirkung:** Die gewählte Reihenfolge gilt für **Grid und Filmstrip** (beide
  lesen dieselbe Display-Order), ebenso für die Library-Navigation
  (Pfeiltasten/Home/End) und die Loupe-/Survey-Reihenfolge. Sie ist reiner
  Session-/Ordner-Anzeigezustand und wird **nicht** in Rezept oder Sidecar
  geschrieben.
- **Custom-Order liegt in einer Ordner-Datei.** Die Datei liegt direkt im
  jeweils gelisteten Ordner und heißt `lumina-sort.json`
  (`format = "lumina-folder-sort"`, `version = 1`). Sie ist portabel: die
  Reihenfolge wird als Liste **relativer Namen** (Dateiname bzw.
  `unterordner/dateiname`, `/`-getrennt, relativ zum gelisteten Ordner)
  gespeichert — nie absolute Pfade, nie `.`/`..`, nie Array-Indizes. Die
  Reihenfolge ist damit stabil gegen Umsortieren, Umbenennen einzelner
  Einträge und das Verschieben des ganzen Ordner-Bundles.
- **Persistenz-Semantik:** Die Datei trägt `mode` **und** `order`. Ein
  Sortierwechsel oder eine Drag-&-Drop-Umsortierung schreibt sie atomar
  (NamedTempFile + persist, gleiche Semantik wie Sidecar-Schreibvorgänge;
  keine temporären Reste als gültig). Fehlt die Datei, gilt `mode = name` und
  eine leere Order. Beim erneuten Listing wird der Ordner-Zustand wieder
  hergestellt — die Custom-Order und der gewählte Modus überleben den Reload
  sichtbar. Eine beschädigte Datei, eine unbekannte `format`-Kennung, eine
  höhere `version` oder ein verbotener Pfad in `order` werden **laut**
  abgelehnt (`error!` + sichtbarer Status) und auf `name`/leer
  zurückgefallen — kein stilles Ignorieren.
- **Interaktion mit Stapeln (LRPAR-G15-STACK-15):** Sortiert wird über die
  sichtbaren Einträge, ein zugeklappter Stapel zählt als **eine** Einheit an
  der Position seines Deckbilds. Die Custom-Order hält alle Mitglieder als
  zusammenhängenden Block, sodass ein zugeklappter Stapel durch jede
  Sortierung und jeden Reload intakt bleibt (nie halb getrennt). Eine
  Drag-&-Drop-Umsortierung, die ein Stapelmitglied greift, verschiebt den
  ganzen Stapel als Einheit.
- **Drag-&-Drop:** Im Grid lässt sich eine Zelle per Drag-&-Drop vor eine
  andere ziehen. Die Umsortierung wechselt automatisch auf `Custom`,
  materialisiert dabei die aktuelle Anzeige-Reihenfolge (Name-/Datum-Sort
  wird zur Custom-Basis), schreibt die Ordner-Datei atomar und loggt `info!`.
- **Sichtbare Bedienung:** Klickbare Buttons im Library-Drawer (`\`) wählen
  die drei Modi (jeder Button bleibt immer erreichbar, F-100-Klickbarkeit).
  Ein Tastatur-Kürzel ist nicht erforderlich; ein späteres Kürzel wäre nur ein
  Alias auf denselben `set_library_sort`-Pfad. Der Drawer ist der etablierte
  Library-Listen-Bedienort (Filter/Quick Develop/Metadaten) und bleibt wie
  diese standardmäßig zugeklappt, damit die Default-Goldens pixel-identisch
  bleiben.
- **Lautheit:** Jeder Sortierwechsel und jede Drag-&-Drop-Umsortierung loggt
  mindestens `info!` (DoD §4). Fehler beim Schreiben/Validieren der
  Ordner-Datei werden über `show_error` sichtbar gemeldet; es gibt keinen
  stillen Fallback.
- **Tests/Abnahme (headless, `cargo test -p lumina-gui` ohne GPU):** je Modus
  ein Mapping-Test; Custom-Order über Reload (Datei → neue App → Reihenfolge
  wiederhergestellt); Drag-&-Drop im Grid real per Pointer-Event → Modus
  `Custom`, Ordner-Datei geschrieben, Reihenfolge geändert; Stapel als
  Einheit bleibt bei Sortierung/Zuklappen intakt; beschädigte Ordner-Datei
  wird laut abgelehnt; Grid **und** Filmstrip zeigen dieselbe Reihenfolge.
  `cargo fmt --check`, `cargo clippy -p lumina-gui --all-targets -- -D warnings`.

### Power-Shortcuts Rest (G-16, LRPAR-G16-POWER)

GUI-contained, keine Kollision mit Bestand (vollständiger Kollisionscheck gegen
die F-100-Tabelle oben: `S` war bisher nur als `Cmd/Ctrl+Alt+S`-Schnappschuss
gebunden, plain-`S` war frei; `Shift`+Doppelklick und `Alt`+Track-Drag waren
unbelegt — Label-`Alt`-Klick = Einzel-Reset und `Alt`-Scroll-Feinjustierung
bleiben daneben unverändert bestehen).

- **Auto-Weiß-/Schwarzpunkt (`Shift`+Doppelklick):** Scope-Entscheid
  (Lightroom-konform): `Shift`+Doppelklick auf das **Whites**-Label setzt den
  Auto-Weißpunkt, auf das **Blacks**-Label den Auto-Schwarzpunkt — jeweils nur
  dieses eine Feld aus dem bestehenden `suggest_auto_tone`-Pfad (derselbe
  Auto-Tone-Algorithmus und -Fingerprint wie der Auto-Button, kein
  Zweit-Algorithmus; Persistenz über den normalen Save/Render-Pfad inkl.
  Auto-Spiegel). `Shift`+Doppelklick auf allen anderen Labels fällt auf den
  normalen Einzel-Reset zurück; ohne `Shift` ändert sich nichts.
- **Maskierungsvorschau (`Alt`+Regler):** Scope-Entscheid: gilt für die sechs
  Basic-Ton-Regler (`exposure`, `contrast`, `highlights`, `shadows`, `whites`,
  `blacks`). Solange `Alt` während einer Track-/Scroll-Wertänderung gehalten
  wird, zeigt die Vorschau-Kopfzeile das Clipping-Badge über den bestehenden
  `J`-Clipping-Pfad (reiner Session-Display-State, nie Rezept/Sidecar; der
  G-11-Overlay-Gate bleibt daneben unverändert). Disambiguierung: `Alt`-Klick
  auf das **Label** bleibt Einzel-Reset, `Alt`-Scroll-Feinjustierung bleibt
  Feinjustierung — die Vorschau ist additiv.
- **`S`-Belegung (G-10-Verträglichkeit):** `S` war in keiner Welle gebunden;
  LRPAR-G10-VIEWER beansprucht `S` für Softproof (noch offen). Entscheid: `S`
  schaltet **jetzt** eine display-only Softproof-Vorschau (Anzeige-Badge, nie
  Rezept/Sidecar) und reserviert damit die G-10-Bindung, statt sie zu
  verbauen. Die volle Druck-/Farbraumsimulation (Profilwahl, Gamut-Warnung)
  bleibt ausdrückliche Folgearbeit in LRPAR-G10-VIEWER.
- **G-10-Softproof-Ausbau (LRPAR-G10-VIEWER, umgesetzt):** Auf G-16-Basis —
  derselbe `toggle_softproof_preview`-Pfad (`info!`, Status, nie
  Rezept/Sidecar), zusätzlich mausbedienbar als Schalter im Histogramm-Panel.
  Kollisionscheck: plain-`S` bleibt frei von allen anderen Bindungen
  (`Cmd/Ctrl+Alt+S`-Schnappschuss disambiguiert per Modifier, Textfeld-Fokus
  via `egui_wants_keyboard_input`-Gate wie alle anderen Kürzel). Die
  Rezept-/Sidecar-Unberührtheit ist per Test assertiert (Rezept-JSON +
  Sidecar-Bytes identisch, Render-Pixel unverändert). Echte Gamut-Simulation
  bleibt Folge-Slice (s. `S`-Zeile der Kürzel-Tabelle).
- **Test-Hinweis:** Shift-Doppelklick-/Alt-Drag-Verdrahtung ist dünn und
  review-geprüft; Fein-Verhalten fährt im manuellen F-103-N6 nach.

### Previous-Übernahme (G-08, LRPAR-G08-PREVIOUS)

Normative GUI-/CLI-Fläche für `.goal/Goal.md` G-08 (Ein-Klick-Vorbild-
Übernahme, zusätzlich zu Sync/Match, klar davon abgegrenzt):

- **Semantik:** Voll-Rezept-Copy vom Vorbild auf Zielbild(er) — exakt der
  Sync-Mechanismus (`copy.recipe = Rezept-Clone`, keine Subset-Auswahl, kein
  Zweit-Mechanismus): je Zielbild eigenes Sidecar (CAS), genau ein sichtbarer
  History-Eintrag (`previous`, GUI zusätzlich `previous-{n}`-Zähler bei
  Mehrfachzielen) je Zielbild, Fehler pro Zielbild isoliert laut
  (`error!` + Report-Eintrag, Rest läuft weiter), `info!`-Log je Bild,
  `preview_generation`-Bump je angewandtem Bild, kein stiller Fallback,
  keine Original-Mutation, keine absoluten Pfade in persistenten Daten
  (History-`extras` tragen höchstens den Dateinamen, nie Pfade).
- **Vorbild-Quelle:** Das zuletzt bearbeitete Bild. GUI: Sitzungs-Referenz
  (`previous_reference`, session-only, nie persistiert) — beim erfolgreichen
  Bildwechsel wird das abgelöste Bild (Pfad + Rezept-Snapshot) als Referenz
  festgehalten; ein explizites Vorbild wird durch Öffnen gewählt (Vorbild
  öffnen, dann Ziel öffnen). CLI: explizit per `--from` (keine Sitzung).
  Ohne Referenz scheitert die Aktion laut (kein No-op, keine Defaults).
- **Ziele:** GUI wendet auf die Filmstreifen-Auswahl an (wie Sync); bei
  leerer Auswahl auf das aktuell geladene Bild (Lightroom-„Previous auf
  aktives Foto“); ohne geladenes Bild lauter No-op. Das aktuell geladene
  Zielbild übernimmt die Referenz zusätzlich In-Memory (Rezept + Dokument +
  Baseline + Re-Render), damit Vorschau und Sidecar konsistent bleiben —
  Sync lädt die aktive Vorschau bewusst nicht neu (Massenoperation), Previous
  schon (Ein-Klick auf das sichtbare Bild). CLI wendet auf jedes `--to`-Ziel
  an (existierende Sidecars; fehlende scheitern laut pro Ziel).
- **GUI:** `Previous`-Button in der Filmstreifen-Aktionszeile neben
  Sync/Match (gleiche Sichtbarkeits-Garantie, headless getestet). Label
  `Previous Image` (eigener i18n-Key `PreviousImage`, bewusst nicht
  `Previous` wie das bildlokale G-01-Panel-Undo).
- **CLI:** `lumina previous --from <bild> --to <bild...> [--from-copy ID]
  [--to-copy ID] [--json]`: lädt das Referenz-Rezept aus dem From-Sidecar
  (hart laut bei fehlendem/ungültigem Sidecar oder unbekannter Kopie, Exit
  1, kein Ziel wird angerührt), Roundtrip über
  `load_sidecar`/`save_sidecar` (validiert, atomar). Exit `0` bei vollem
  Erfolg, `3` bei Teilfehlern (analog `batch`/`batch-meta`), `1` bei hartem
  Fehler (keine Referenz, keine Ziele).
- **Abgrenzung:** G-01-Panel-Previous (LRPAR-G01-BASIC) ist bildlokal
  (Sektions-Undo auf den letzten gespeicherten Stand desselben Bildes) —
  G-08-Previous ist bildübergreifend (Vorbild-Rezept → Zielbild(er)).
  G-04-Follow-up (Spot/Remove-Varianten, Visualize, Distraction) besitzt
  eigene Tool-Semantik; Previous kopiert deren Rezept-Anteile wie jeden
  anderen Rezept-Anteil mit (kein separates Previous pro Tool).
- **Status (LRPAR-G08-PREVIOUS):** umgesetzt — SOLL (dieser Abschnitt),
  CLI-Befehl, GUI-Referenz + Button + In-Memory-Übernahme + headless
  E2E-Tests (Vorbild → Ziel-Datei → Reload, History-Schritt je Zielbild).

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

### Maskierungs-Parität G-03 (LRPAR-G03-MASK, Release 1.0)

Normative GUI-/CLI-Fläche für `.goal/Goal.md` G-03 (Details und
Auswertungsregeln: `feature/product/ai-masks.md` § „G-03
Maskierungs-Parität“):

- **Maskenliste mit Auge:** Jede Maske der aktiven Kopie erscheint mit
  Sichtbarkeits-Auge (`MaskLayer.visible`, persistiert pro virtueller
  Kopie); der selektierte Eintrag zeigt Status (`valid`/`stale`/`missing`/
  `corrupt`/`pending`) und Fehlertext. Unsichtbare Layer wirken nicht auf
  den Render (explizite Nutzerwahl, keine Warnung).
- **Neu-Menü:** `Subject`/`Sky`/`Background`/`Objects`/`People` (+ optionale
  Teile `face`/`hair`/`eyes`/`pupil`/`sclera`/`lips`/`teeth`/`skin`/`body`)
  als AI-Auswahl (`AiSelect`, Status `pending` bis inferiert, fehlendes
  Modell laut sichtbar); `Color Range` / `Luminance Range` als
  deterministische Rezept-Stufen (sofort `valid`, kein Modell nötig).
- **Kombinatorik:** `Add` (`union`), `Subtract` (`subtract`, Basis zuerst),
  `Invert` (`invert`, genau eine Referenz), `Duplicate` (neue stabile ID).
  Zyklen/falsche Stelligkeit werden laut verweigert (kein stiller Fallback).
- **Show + Color Overlay:** `Show`-Checkbox (Default an) plus Farbwahl
  (Default Rot) als Session-Display-State (nie Rezept/Sidecar, `info!`-Log),
  UND-verknüpft mit dem G-11-`OverlayMode`.
- **CLI:** `lumina mask --list` (Status je Kopie), `--add-ai-select`,
  `--add-color-range`, `--add-luminance-range`, `--combine`,
  `--duplicate`, `--show-layer`/`--hide-layer`, `--attach-layer`; Roundtrip über
  `save_sidecar`/`load_sidecar`, laute Fehler (Exit 1 Benutzungs-/Laufzeit-
  fehler wie Bestand, kein stiller Fallback, keine absoluten Pfade).

### Remove-Parität G-04 (LRPAR-G04-REMOVE, Release 1.0)

Normative GUI-/CLI-Fläche für `.goal/Goal.md` G-04 (Details, Feldsemantik und
Stufenregeln: `feature/product/spot-removal.md` § „G-04 Remove-Parität“):

- **Visualize-Slider:** Schwellwert-Slider im Spot-Panel
  (`spot_visualize_threshold`, `0..=1`), rezept-persistiert (Reload stellt
  ihn wieder her); deterministische Kandidaten-Tönung, kein Modell. Das
  Overlay malt ausschließlich die GUI-Vorschau (`render_from`-Post-Prozess,
  UND-verknüpft mit dem G-11-`OverlayMode`); Render/Export/CLI bleiben
  ungetönt.
- **Tool-Overlay-Modi:** derselbe G-11-`OverlayMode`-Umschalter
  (`Always`/`Auto`/`Never`) gilt für das Spot-Werkzeug (`Q`) **und** das
  Visualize-Overlay — reiner Session-Display-State (nie Sidecar, Reload →
  `Always`), `info!`-Log. Die CLI kennt kein Overlay-Flag und rendert
  immer ohne Overlay.
- **Detect-Objects:** Button im Spot-Panel + CLI `lumina spot
  --detect-objects` (Heuristik Stufe 1, ohne Modell, ONNX nur hinter
  F-078-Gate, Status laut); `--detect-apply` übernimmt explizit. Ohne
  `--detect-threshold` gilt der Rezept-Schwellwert
  (`spot_visualize_threshold` wenn gesetzt, sonst `0.5`); die GUI nutzt
  denselben Default (`spot_detect_effective_threshold()`).
- **Distraction Removal:** Schalter `Reflections`/`People`/`Dust` + `Auto`
  (alle Default aus, Rezept-persistiert); `Auto` listet nur, wendet nie
  still an. `Reflections`/`People` ohne Modell → sichtbarer
  `NeedsModel`-Status. `--set-distraction k=v,...` **mergiert** in die
  gespeicherten Schalter (ungenannte bleiben, Abschalten per `k=false`;
  Entscheid: Merge, konsistent zum GUI-Einzeltoggle — siehe
  `feature/product/spot-removal.md` § „G-04 Remove-Parität“).
- **Generativ-Varianten:** Prompt-/Seed-/Varianten-Steuerung im Spot-Panel +
  CLI `lumina spot --regenerate-variant --spot-id <id> --variant <n>`;
  deterministisch (`variant_seed`), persistiert, `info!`-Log.
- **CLI:** `lumina spot --list` (Spots + Einstellungen je Kopie),
  `--add-heuristic`, `--clear`, `--set-visualize-threshold`,
  `--set-distraction k=v,...`, `--detect-objects/--detect-apply`,
  `--regenerate-variant`; Roundtrip über `save_sidecar`/`load_sidecar`,
  laute Fehler (Exit 1 Benutzungs-/Laufzeitfehler wie Bestand, kein stiller
  Fallback, keine absoluten Pfade). `--clear` entfernt alle Spots und
  widerspricht `--add-heuristic`/`--detect-apply`/`--regenerate-variant`
  (lauter Fehler, kein stilles Verwerfen des Adders).

### Lens Blur G-05 (LRPAR-G05-LENSBLUR, Release 1.0)

Normative GUI-/CLI-Fläche für `.goal/Goal.md` G-05 (Feldsemantik,
Tiefenquellen-Entscheid und Stufenregeln: `feature/architecture/pipeline.md`
§ „G-05 Lens Blur“). Die Sektion liegt GUI-seitig adjazent zu Optics (eigene
kollabierbare Untergruppe „Lens Blur“, kein zweiter Renderpfad):

- **Fokus-Rahmen:** normiertes Rechteck (`x`/`y`/`width`/`height`), als
  Overlay-Rechteck in der Vorschau sichtbar (reiner Session-Display-State
  für das Malen, Werte rezept-persistiert, deterministisch); ungültige
  Rechtecke werden laut verweigert.
- **Focal Range:** Slider `Near`/`Far` (`0..=1`, `near <= far`);
  **Blur Amount:** Slider `0..=1` (0 = Identität, kein Blur-Pass);
  **Bokeh:** Auswahl `Round`/`Elliptical`/`Hexagonal` (deterministisch
  renderbar, Golden/PSNR-Gates).
- **Tiefenstatus:** sichtbar (`Off` / `Heuristic active` / `Missing depth
  artifact`); fehlendes referenziertes Artefakt bricht Render/Export laut
  ab (Exit 1), nie stilles Heuristik-Rendering.
- **External-Depth-Bindung (Entscheid 2026-09-16, DEPTH-PLUMBING-1):** Die
  Laufzeit-Bindung einer externen Tiefenkarte (Dateiformat + Loader) ist
  bewusst **Post-MVP** (Details/Validierung:
  `feature/architecture/pipeline.md` § „External-Depth-Bindung“). In v1
  persistieren `--set-depth-artifact` / `--clear-depth-artifact` ausschließlich
  die portable Referenz; **kein** CLI-/GUI-Caller lädt eine Ebene, ein Rezept
  mit gesetzter Referenz bricht im Render/Export laut ab (Exit 1). Die
  Fokus-Rechteck-Heuristik bleibt die einzige renderbare Tiefenquelle.
- **CLI:** `lumina lens-blur --list` (Werte + Status je Kopie),
  `--enable/--disable`, `--set-amount`, `--set-focal-near/--far`,
  `--set-bokeh round|elliptical|hexagonal`, `--set-focus-rect
  x,y,w,h`, `--set-depth-artifact PATH:SHA256`, `--clear-depth-artifact`,
  `--clear`; Roundtrip über `save_sidecar`/`load_sidecar`, laute Fehler
  (Exit 1 Benutzungs-/Laufzeitfehler wie Bestand, kein stiller Fallback,
  keine absoluten Pfade). `--clear` entfernt die ganze Stufe und
  widerspricht jedem anderen Mutations-Flag (lauter Fehler statt stillem
  Kurzschluss).
- **Status (LRPAR-G05-LENSBLUR):** umgesetzt — CLI-Befehl, Optics-Panel
  (Lens-Blur-Untergruppe mit Statuszeile), Fokus-Overlay im Preview und
  headless E2E-Tests (Setter → Datei → Reload); fehlendes Tiefenartefakt
  bricht Render/Export mit Exit 1 bzw. sichtbarem GUI-Fehler ab.

### Geometrie-Parität G-06 (LRPAR-G06-GEO, Release 1.0)

Normative GUI-/CLI-Fläche für `.goal/Goal.md` G-06 (Feldsemantik,
Stufenreihenfolge, Straighten-Alias, Lensfun-Vollausbau:
`feature/architecture/pipeline.md` §§ F-093, F-098, F-099). Auto-Upright ist
explizit **nicht** Teil dieses Slices (1.5, LRPAR-G06-UPRIGHT-15).

- **Crop:** Aspect-Preset-Auswahl (`original`, `1:1`, `4:5`, `5:4`, `3:2`,
  `2:3`, `4:3`, `3:4`, `16:9`, `9:16`) plus freie Rechteck-Felder
  (`x`/`y`/`width`/`height`, normiert `0..=1`) plus Löschen-Button; ungültige
  Rechtecke werden laut verweigert (Status + kein Save), nie still geclippt.
  Das aktive Rechteck erscheint als Overlay-Rechteck in der Vorschau (reiner
  Session-Display-State, UND-verknüpft mit dem G-11-`OverlayMode`).
- **Straighten:** Slider `-180..=180` (Alias auf `geometry.rotation_degrees`,
  gleiche Validierung/Renderwirkung wie Rotation) plus ±90°-Buttons; der
  Crop-Modus-Badge (`R`) bleibt reine Anzeige, Edits laufen über diese
  Controls.
- **Spiegelung:** Checkboxen horizontal/vertikal über den normalen
  Save/Render-Commit.
- **Objektivkorrektur manuell:** Profil-Dropdown aus der Core-Whitelist
  (`wide-light`/`tele-light`/`standard-neutral`, Freitext laut verweigert)
  plus Koeffizienten-Slider (`distortion_k1..k3`, `vignette_c0..c2`,
  `ca_red`/`ca_blue` in den F-098-Domänen).
- **Lensfun-Status:** sichtbare Statuszeile (Profil gefunden mit
  Distortion/Vignetting/TCA-Markierung vs. fehlt + Grund); kein stiller
  Identitäts-Render. Die manuellen Crop-/Perspektiv-Controls sind **immer**
  verfügbar (kein Lensfun-Feature-Gate — F-093/F-099 sind reine Core-Modelle).
- **Perspektive manuell:** sieben Slider (`vertical`/`horizontal`/`rotation`
  `-1..=1`, `scale`/`aspect_ratio` `0.1..=10`, `shift_x`/`shift_y` `-1..=1`)
  über den normalen Save/Render-Commit.
- **History-Regel:** Jeder Geometrie-Schritt ist genau ein sichtbarer
  History-Eintrag — CLI: ein Eintrag pro mutierendem `geometry`-Aufruf;
  GUI: pro gespeichertem Geometrie-Commit ein Eintrag (Slider-Drags
  koaleszieren zu einem Eintrag je Commit, diskrete Aktionen je einer).
- **CLI:** `lumina geometry --list` (Crop/Lens/Perspektive je Kopie),
  `--set-crop-aspect PRESET`, `--set-crop-free x,y,w,h`, `--clear-crop`,
  `--set-rotation DEG`, `--straighten DEG` (Alias), `--set-mirror
  h|v|hv|none`, `--set-lens-profile NAME`, `--set-lens FELD WERT`,
  `--clear-lens`, `--set-perspective FELD WERT`, `--clear-perspective`,
  `--clear-geometry`, `--lensfun-status` (EXIF→Profil-Auflösung, laut mit
  Grund bei Fehlschlag); Roundtrip über `save_sidecar`/`load_sidecar`, laute
  Fehler (Exit 1 Benutzungs-/Laufzeitfehler wie Bestand, kein stiller
  Fallback, keine absoluten Pfade). `--list` ist die reine Lese-Ansicht
  (Default ohne Mutations-Flag) und widerspricht jedem Mutations-Flag
  (lauter Fehler statt stillem Ignorieren).
- **Status (LRPAR-G06-GEO):** umgesetzt — Schema (bestand, additiv v2),
  Core-Stufen (keine zweite Pipeline, Reihenfolge
  Lens→Perspektive→Crop→Rotation→Spiegelung), TCA via Lensfun, CLI-Befehl,
  GUI-Sektion (Crop/Straighten/Aspect/Spiegelung/Lens/Perspektive +
  Lensfun-Status + Crop-Overlay) + headless E2E-Tests (Setter → Commit →
  Datei → Reload, Preview-Änderung); GPU routet aktive Geometrie laut auf
  CPU (Bestand).

### Auto-Upright G-06 (LRPAR-G06-UPRIGHT-15, Release 1.5)

Normative GUI-/CLI-Fläche für die automatische Upright-Analyse
(Feldsemantik, Algorithmus `upright-lines-v1`, Fingerprint/Veraltung,
Einordnung vor der Perspektive: `feature/architecture/pipeline.md` § F-099).

- **Schema/Persistenz:** `recipe.upright` (additiv, top-level; `version`,
  `enabled`, `analysis` mit `fingerprint`/`vertical`/`horizontal`/`rotation`/
  `line_count`/`confidence`). `enabled` ohne `analysis` wird laut abgelehnt.
- **GUI (Geometry-Sektion):** Statuszeile `Upright analysis: fresh|stale|none`,
  Button **Analyze** (läuft die deterministische Analyse auf dem geladenen
  Quellbild und persistiert sie fingerprint-gebunden), Checkbox **Apply
  analysis** (schaltet die effektive Perspektive ein/aus; das manuelle
  Perspektiv-Modell bleibt persistiert und greift bei Deaktivierung wieder),
  Button **Clear upright**. Die Analyse selbst ist nicht Teil des Deferred
  Sliders-Saves — sie schreibt über den normalen Save/Debounce-Commit.
- **CLI:** `lumina upright --input PFAD [--list] [--analyze] [--enable]
  [--disable] [--clear]`. `--list` ist read-only (Default) und widerspricht
  jedem Mutations-Flag laut; ein veralteter Fingerprint wird als `stale`
  gemeldet, nie still neu berechnet. Ein Aufruf = genau ein History-Eintrag.
- **Sichtbarkeit/Determinismus:** kein stiller Fallback — eine fehlende
  Analyse blockiert `--enable`/„Apply"; die Korrektur ist die bestehende
  F-099-Homographie (GPU-paritätisch bei explizitem Crop, sonst lautes
  CPU-Routing über den Default-Content-Crop-Grund).

### Rote-Augen-Parität G-14 (LRPAR-G14-REDEYE-15, Release 1.5)

Normative GUI-/CLI-Fläche für die Rote-Augen-Korrektur (Formel, Validierung,
Platzierung nach Schärfen: `feature/architecture/pipeline.md` § G-14).

- **Erkennung = explizites Markieren (Entscheid 2026-09-16):** In diesem
  Release gibt es **keine** automatische Pupillenerkennung. GUI und CLI
  markieren Regionen explizit und persistieren sie als
  `recipe.adjustments.red_eye.regions` mit stabilen ids; die Korrektur ist die
  deterministische, modellfreie G-14-Formel (GPU-paritätisch).
- **GUI (Detail-Sektion):** Toggle **Mark region** bewaffnet den
  Vorschau-Picker (Klick markiert ein Pupillenzentrum in normierten
  Quellkoordinaten; der Picker bleibt für mehrere Markierungen aktiv).
  Die WB-Pipette und der Red-Eye-Picker sind gegenseitig exklusiv; ein
  Geometrie-Schritt, der die Quell-Zuordnung blockiert, verweigert den Klick
  sichtbar. Pro Region: id-Label, Slider **Radius** (`0.001..=1`),
  **Desaturate**/**Darken** (`0..=1`), **Remove**; globaler **Clear all**.
- **CLI:** `lumina red-eye --input PFAD [--list] [--set
  ID:x,y,radius,desaturate,darken]… [--remove ID]… [--clear]`. Specs werden
  laut validiert (Bereiche, ≥1 Region, ≤32); Re-Markieren derselben id
  ersetzt sie stabil; `--remove` einer unbekannten id ist laut und schreibt
  nichts; ein Aufruf = genau ein History-Eintrag.
- **Platzierung/Abnahme:** nach Schärfen und vor Effekten/Masken/Crop;
  Golden-Gates: Lokalität (Pixel außerhalb/nicht-rot unverändert), Monotonie,
  Determinismus, Alpha-Erhalt, JSON-Roundtrip, Validierungsablehnung — plus
  headless GUI-E2E (Markieren/Ändern → Commit → Datei → Reload).

### Color-Parität G-02 (LRPAR-G02-COLOR, Release 1.0)

Normative GUI-/CLI-Fläche für `.goal/Goal.md` G-02 (Feldsemantik,
Stufenregeln, Interpolations- und Gewichtungsformeln:
`feature/architecture/pipeline.md` §§ F-089, F-090, F-090b, F-091):

- **Tone Curve je Kanal:** Kanalwahl `Master`/`Red`/`Green`/`Blue` in der
  Tone-Curve-Sektion. Parametrisch = vier Regions-Slider
  (Shadows/Darks/Lights/Highlights, `-1..=1`) je Kanal, persistiert als
  4-Punkte-Kurve des Kanals (gleiche Abbildung wie Master-Bestand); Punkte =
  freie Stützpunktliste (`2..=32`, Endpunkte `(0,0)`/`(1,1)` Pflicht) je
  Kanal, ersetzt die 4-Punkte-Liste (Last-Write-Wins je Kanal). Ungültige
  Punkte/Regionen werden laut verweigert (Status + kein Save), nie still
  normalisiert. Interpolation (monotone kubische Hermite) und Clipping
  (`0..=1`) wie F-089 dokumentiert.
- **Point Color:** Gruppe in der Color-Sektion (nach HSL-Mixer, vor Color
  Grading): Eintragsliste mit stabilen IDs (`pc-<n>`), je Eintrag Slider
  `Hue Center` (`0..=360`), `Range` (`0..=180`), `Hue Shift`/`Sat
  Shift`/`Lum Shift` (je `-1..=1`) plus Entfernen-Button und
  Hinzufügen-Button. Rezept-persistiert, deterministisch, Reload stellt die
  Liste exakt wieder her.
- **Color-Grading-Feinschliff:** je Bereich (`Shadows`/`Midtones`/
  `Highlights`) zusätzlich `Luminance`-Slider (`-1..=1`) plus globaler
  `Blending`-Slider (`0..=1`, Default `0.5` = bisheriges Verhalten);
  Commit-Pfad und Persistenz wie die bestehenden Grading-Slider.
- **Previous/Reset:** Tone-Curve-Sektion umfasst Master + alle Kanalkurven;
  Color-Sektion umfasst HSL + Point Color + Grading (+ Presence +
  Vibrance/Saturation wie bisher).
- **CLI:** `lumina color --list` (Kurven/HSL/Point-Color/Grading je Kopie),
  `--set-curve-param KANAL S,D,L,H`, `--set-curve-points KANAL
  'i,o;i,o;…'`, `--clear-curves [KANAL]`, `--set-hsl KANAL FELD WERT`,
  `--clear-hsl`, `--add-point-color` (+ `--hue-center/--hue-range/
  --hue-shift/--sat-shift/--lum-shift`), `--set-point-color ID FELD WERT`,
  `--remove-point-color ID`, `--clear-point-color`, `--set-grading BEREICH
  FELD WERT` (Felder `hue_degrees|saturation|luminance`),
  `--set-grading-balance V`, `--set-grading-blending V`,
  `--set-vibrance V`, `--set-saturation V`; Roundtrip über
  `save_sidecar`/`load_sidecar`, laute Fehler (Exit 1
  Benutzungs-/Laufzeitfehler wie Bestand, kein stiller Fallback, keine
  absoluten Pfade).
- **Status (LRPAR-G02-COLOR):** umgesetzt — Schema (`point_color`,
  `luminance`/`blending`), Core-Stufen (keine zweite Pipeline),
  CLI-Befehl, GUI-Sektionen (Kanalwahl, Punkteditor, Point-Color-Gruppe,
  Luminance/Blending-Slider) + headless E2E-Tests (Setter → Commit →
  Datei → Reload, Preview-Änderung); GPU routet aktives `point_color`
  (nicht-neutral) laut auf CPU.

### Develop-Basis G-01 (LRPAR-G01-BASIC, Release 1.0)

Normative GUI-/CLI-Fläche für `.goal/Goal.md` G-01 (Feldsemantik,
Stufenregeln, Messpfade: `feature/architecture/pipeline.md` § „G-01
Develop-Basis“):

- **Treatment:** Basic-Kopfzeile mit Auswahl `Color` / `Black & White`
  (derselbe `apply_treatment`-Pfad wie `V`, persistiert, `info!`-Log).
  `V` bleibt das Tastatur-Alias (Stash-Semantik unverändert).
- **Profile:** Basic-Dropdown aus der normativen Whitelist (`default |
  neutral | vivid | portrait | landscape | monochrome`, absent =
  `default`); nur Listenauswahl, Freitext wird laut verweigert. MVP-Grenze:
  Auswahlabsicht ohne eigene Renderwirkung (s. Pipeline-Doc).
- **Original-Photo-Vergleich:** Histogramm-Sektion wie bisher (G-10-Schalter
  „Show original“); G-01-Anteil ist die deterministische Referenzseite
  (`histogram_compare_data()` Original + Edit, Delta-Zeile Δ Mean + L1 bei
  aktivem Vergleich). Reiner Session-Display-State, `info!`-Log.
- **Reset Sliders Automatically:** Checkbox im Develop-Footer (Default aus),
  persistiert ordner-vererbt in `.lumina/settings.json`
  (`reset_sliders_automatically`); AN = Bildwechsel verwirft anstehende
  Commits, AUS = Bildwechsel flusht sie (kein Edit-Verlust). `info!`-Log.
- **Previous/Reset je Panel:** Jede Develop-Sektion trägt Sektions-Buttons
  `Previous` (Baseline wiederherstellen) und `Reset` (Sektions-Defaults)
  über den normalen Save/Render-Commit (History, `info!`-Log). Kein
  Mehrbild-Previous (LRPAR-G08-PREVIOUS).
- **CLI:** `lumina develop --treatment color|bw --profile NAME`
  (Vorab-Validierung, Exit 1 bei unbekanntem Wert, kein Halb-Apply);
  `lumina inspect [--json]` zeigt Treatment/Profil je Kopie
  (absent = `color`/`default`). Roundtrip über
  `save_sidecar`/`load_sidecar`, keine absoluten Pfade.

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

#### F-103-N6 Runde 2 Runbook (manueller Test, 2026-09-19)

**Arbeitsteilung (User-Regel):** Der User fährt die GUI, der Build-Agent
verwaltet und analysiert das Terminal-Log. Kein Befund ohne Log-Stelle.

- **Build (mit Crash-Fix ≥ `460cefd`):** `cargo build --release -p lumina-gui`
- **Start (durch den Build-Agenten, Log in Datei):**
  `RUST_LOG=trace nohup ./target/release/lumina-gui > /tmp/lumina_r2_trace.log 2>&1 < /dev/null & disown`
  Nur EINE Instanz gleichzeitig (keine zweite aus dem Terminal daneben).
- **Checkliste:** RAW per Pfad + Drag & Drop; Preview + Exposure/Contrast
  (Renderstand ändert sich); Sidecar-Write + Neustart-Restore; linke Rail;
  Icon-Leiste (jeder Button); Kurvengrafik (Klick/Drag/Doppelklick pro Kanal);
  Crop (Handles, Enter = Commit, Esc = Verwerfen); Library↔Develop-Wechsel;
  Loupe/Compare/Survey; 04a-Zahlen für R2-GUIMOD (gefühlte Draft-Latenz,
  Modulwechsel-Dauer).
- **Melden pro Befund:** Was + Wann („jetzt beim X geruckelt/verschwunden"),
  ggf. Screenshot; Crash-Dialog-Text bzw. Feststellung „still verschwunden".
  Der Build-Agent zieht die Log-Stelle, analysiert und verankert Fixes mit
  Tests — Befunde landen hier unten als R2-*-Einträge, nicht als Todo-Tasks.

#### F-103-N6 Runde 2 Befunde (2026-09-19, Release-Build, noch offen)

- **R2-CRASH-1 (kritisch, BEHOBEN 2026-09-19, verifiziert BESTANDEN):**
  SIGABRT — Panic im Logger auf dem Fehler-Anzeigepfad (`show_error` →
  `error!` → `eprint`-Panic → Panic-Hook-`eprintln`-Panic → Abort). Fix:
  `StderrLogger`/`Panic-Hook` panicken nie mehr (writeln + catch_unwind +
  Poison-Recovery + Zähler), Draft-Tick-Fehler dedupliziert (1×`error!` +
  1×Repeat-`warn!`). Auslöser: `set_tone_curve_channel_region` schrieb ohne
  Validierung Kurven mit Endpunkt ≠ (0,0) (positives `shadows`) → jeder Render
  schlug fehl → Per-Frame-Fehlerpfad; schreibt jetzt validiert + laut.
- **R2-JANK-1 (hoch, BEHOBEN 2026-09-19, verifiziert BESTANDEN):** Draft-Preview machte die
  Sidebar rucklig — synchroner ungedrosselter CPU-Draft-Render (~16 ms, mit
  Denoise ~120 ms) im UI-Frame (H1) + GPU/CPU-Doppelarbeit (H2) + redundanter
  Textur-Upload (H3). Fixes: F1 Frame-Budget-Drossel (max 1 Draft/16 ms,
  Stale-Badge sichtbar), F3 CPU-Upload-Skip bei aktivem GPU-Present
  (Navigator-Handle erhalten), F4 Analyse-Kadenz (150 ms, Retained-Snapshot +
  „pending"-Label, Full-Render bei Loslassen). F2 Worker-Offload begründet
  zurückgestellt (Architekturwechsel, Aufwand groß — eigener Task, kein
  1.0-Blocker). Bewusste Trade-offs: Navigator kann unter GPU-Present-Drag
  transient stale sein (Haupt-Preview korrekt); Pending-Label als Konstante
  statt `Str` (Ratchet-Ceiling + Namen-Vorbehalt).
- **R2-MODSWITCH-1 (mittel, BEHOBEN 2026-09-19, verifiziert BESTANDEN):**
  Library↔Develop-Wechsel langsam — synchrone Thumbnail-Disk-Probes +
  Preview-Decodes auf dem UI-Thread. Fix F7: metadata-only
  `PreviewIndexCache` (1 Build/Ordner, memoisiert, sichtbare Invalidierung) +
  Decode im Worker-Pool (`thumb_worker`) + Full-Render-Deferral um 1 Frame bei
  Modulwechsel (Stale sichtbar, Debounce/Drossel unverändert). F8
  (`list_directory`-Async-Scan) bleibt Folge-Task.
- **R2-CLAMP-1 (erledigt ohne Fix, 2026-09-19):** Untracked `gui.log` (23.08.) zeigte eine
  Render-Pfad-Panic (`f32::clamp`: min > max/NaN beim debounced Full
  Render, FitWidth-Rundung). Recherche-Beleg: Panic stammt aus einem Build 7
  Minuten VOR dem Fix-Commit `bbb0cba` (2026-08-23, „order-independent
  bounds", Guard per `swap`); Guard im aktuellen Code vorhanden
  (`preview_draws.rs:259-278`), kein zweiter variabler-Clamp-Kandidat im
  Render-/Present-Pfad. Regressionstest `preview_center_clamp_swaps_inverted_bounds_without_panic`
  (verifiziert BESTANDEN 2026-09-19, Mutations-Beweis: ohne Swap panickt er wie Alt-Log).

#### F-103-N6 Runde 3 Befunde (2026-09-19, Release-Build `236a7e7`, 602k-Zeilen-Trace, Analyse per Subagent)

- **R3-SWITCH-1 (teilweise vermessen 2026-09-19, dritter Run, instrumentiert):**
  Erster Library-Wechsel: First-Paint 2886.8 ms (kalt); alle späteren Wechsel:
  First-Paint 0.2–24.5 ms (Develop 24.5/0.5/0.4 ms, Library 0.2 ms). Thumbs je
  4.2 ms, PreviewIndex-Build 0.1 ms (2 Einträge), Decode 283.1 ms (6032×4024),
  Full-Render 85.2 ms ohne / 2117.8 ms mit Denoise-Fallback. F7-Deferral
  feuerte 0×. Verdacht verengt: nicht der Wechsel-Frame, sondern Auswahlkette
  + Full-Res-Upload (→ R3-RENDER-SIZE-1).
- **R3-GRIDSEL-1 (BEHOBEN 2026-09-19, verifiziert BESTANDEN):** Grid-Highlight folgt der Selektion
  nicht — `library_grid.rs:206` malte aus `self.path` (geladenes Bild), nicht
  aus `filmstrip_selection`. Grid zeigt Landscape (geladen), Filmstrip darunter
  korrekt Portrait (selektiert). Fix: `selected` aus `filmstrip_selection` prüfen.
- **R3-OPEN-1 (ENTSCHIEDEN 2026-09-19, User: Lightroom-mäßig):** Bei genau
  einer Selektion öffnet der Develop-Wechsel das Bild (LR-Verhalten);
  Doppelklick bleibt. Kaltstart-Warmup (R3-WARMUP-1) läuft im Hintergrund.
- **R3-WARMUP-1 (ENTSCHIEDEN 2026-09-19, User):** Kaltstart-Arbeit (Ordner-
  Index, Thumbnails, Decode + Full-Render des ersten Bildes) läuft direkt
  nach App-Start im Hintergrund (Worker/Idle), damit der erste Library-
  Wechsel kein 2.9-s-Loch mehr hat. Sichtbarer Fortschritt statt Stillstand,
  kein stiller Zustand.
- **R3-DENOISE-2 (offen, 2026-09-19):** Gelbes Badge „Render routed to CPU …
  [denoise_ai (not GPU-wired)]" ist per Design laut (Gewichte weiter pending),
  hat aber KEINE Log-Zeile: Badge stammt aus dem Rezept-Gate
  (`gpu_unsupported_stage_reasons`), nur Present-Refusals loggen. Lücke für
  R3-LOG-1: Gate-Routing braucht 1× `warn!` beim Auftreten.
- **R3-DRAFT-1 (offen, gemessen):** 87 Drag-Ticks, alle zu langsam: ~55–60 ms/
  Frame ohne Crop (cpu-Median 26.27 ms + gpu-Median 25.70 ms), ~70–90 ms mit
  aktivem Crop (cpu-Median 35.53 ms, +35 %). F1-Drossel feuerte 0× (Events
  langsamer als 16-ms-Budget — jeder Tick rendert voll), Monster-Top: 42.90 ms
  CPU-Draft. F4-Kadenz wirkt (52× gedrosselt, Analyse-Median 0.95 ms — Analyse
  ist nicht das Problem). Basis-Cache gesund (95× HIT / 3× MISS).
- **R3-ROUTING-1 (offen, gemessen):** Mit aktivem Crop (dimension-changing)
  fällt JEDER Kurven-Tick auf CPU zurück (38 Ticks ↔ 38× `render_to_vram
  failed`-Warnung, gpu bis 47.37 ms vergeudet) + gelbes „Render routed to
  CPU"-Badge. Das verletzt die GPU-Default-Regel (User-Entscheid: „routed to
  CPU" = Fail mit Fix-Pflicht) — VRAM-Geometrie-Pfad oder Draft-ROI ohne Crop
  fehlt. Sofortmaßnahme unabhängig davon: Pro-Tick-`warn!` bei bekanntem
  persistentem Refusal drosseln (1× beim Wechsel + `trace!` pro Tick, Vorbild
  Generative-Pfad) — 38 identische Warnungen pro Session sind Log-Spam.
- **R3-RENDER-SIZE-1 (offen, User-Frage 2026-09-19):** Weder Draft noch Full
  rendern auf Anzeigegröße — Draft feste 1280-px-Kante (`lib.rs:2899`),
  Full volle Quell-Auflösung (24 MP ≈ 96 MB RGBA, `render_entry.rs:24-84`).
  Anzeigegröße wäre sinnvoll (Vorschlag: Full/Draft auf Viewport-Auflösung +
  Device-Pixel-Ratio begrenzen, Full-Res nur für Export/1:1-Loupe).
- **R3-DENOISE-1 (bekannt, verstärkt):** `pending-integration`-Fallback (1×,
  funktioniert, Portrait ok) vs. harter Neighbor-Fail (Landscape, doppelt
  geloggt = ein Ereignis auf zwei Ebenen) im selben Run — Fallback- und
  Fail-Pfad sind je nach Bild/Pfad inkonsistent. Doppel-`warn!` zusammenführen
  (1 Zeile/Ereignis).
- **R3-LOG-1 (BEHOBEN 2026-09-19, verifiziert BESTANDEN):** Delta-Traces für
  Switch-Event + First-Paint, Decode (ms + Auflösung), PreviewIndex-Build,
  Thumbnail-Roundtrip, Full-Render (ms + Dims), Upload/Skip (Bytes),
  F7-/F1-Feuer-Zähler, Refusal-Wechsel (Stage, 1× `warn!` + `trace!`/Tick),
  Denoise-Einzeiler. Alle 7 Switch-Entry-Points über `set_module`. **Alle
  Trace-Meldungen tragen einen zentralen ISO-8601-UTC-Stempel**
  (`logger.rs`, std-only).
- **R3-CONFLICT-1 (ungeprüft):** 6 Sidecar-Saves, 0 Rebases/Konflikte —
  Single-Instance-Run; die Rebase-Logik ist per Log unbestätigt (nur per Test).

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
