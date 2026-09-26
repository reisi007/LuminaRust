# Golden-Fixtures — Fixture-Vertrag und Golden-Inventar (GOLDEN-FIXT-31)

> **Feature-ID:** `GOLDEN-FIXT-31` · **Inventar-Version:** `1` ·
> Stand 2026-09-25 · Release 1.0
>
> Dieses Dokument ist die normative Grundlage für alle
> `egui_kittest`-Goldens in `crates/lumina-gui/tests/snapshots/`. Es
> klassifiziert jede committete Golden-Datei und definiert, welche Fixture
> für welchen Golden zulässig ist.

## 1. Problem und Ziel

Die bis 2026-09-25 committeten RAW-Fixtures waren **synthetisch**: 11 `.arw`-Dateien
mit je 18 Byte wörtlichem Text `lumina-raw-fixture`
(`library_views/`, `library_badges/`, `library_rated/`, `library_stack/images/`).
LibRaw lehnt diese Bytes ab. Folge:

- Das committed Golden `library_compare.png` (und `library_loupe.png`,
  `library_survey.png`, `library_rated_badges.png`, `library_subfolder_badges.png`,
  `library_stack_membership.png`) **nahm den roten Banner
  „LibRaw opening input failed (-100009)" samt Farbklotz-Platzhaltern als
  Soll-Inhalt auf**.
- Zusätzlich war der Preview-Source der Develop-Goldens
  (`LuminaApp::sample_image_png()`) ein **4×3-Pixel**-Synthetik-PNG, das auf die
  Panelfläche hochskaliert wird.

Ein Bild, das einen Decode-Fehler zeigt, kann eine Bildpipeline-Regression
**prinzipiell nicht** aufdecken. Ziel dieses Vertrags ist, dass jede als
Render-Invariante klassifizierte Golden-Datei echten Bildinhalt aus einer echten
Decodierung trägt.

## 2. Fixture-Klassen (normativ)

| Klasse | Bedeutung | Zulässige Bytes | Wird committet? |
| --- | --- | --- | --- |
| **R1 — echtes RAW-Fixture** | Die zwei lizenzierten Canon EOS R1-CR3s `sample-data/raw/aircraft-landscape.cr3` (6032×4024, Orientierung 1) und `aircraft-portrait.cr3` (4024×6032 nach Orientierung 8). Provenienz/Lizenz: `sample-data/raw/README.md` §4/§8. | Original-Bytes der CR3 | **Nein** — die Dateien liegen genau einmal in `sample-data/raw/`; ein Test stellt eine byte-identische Kopie in sein eigenes, relatives Fixture-Verzeichnis (`tests/fixtures/…`, gitignoriert über `tests/fixtures/.gitignore`) |
| **S1 — Layout-Sentinel** | Nicht-Bild-Bytes mit gültiger RAW-Endung, erzeugt **zur Laufzeit in einem `tempfile::TempDir`**. Ausschließlich dort zulässig, wo die Assertion eine *Geometrie* oder *Anzahl* ist, die unabhängig vom Decode-Erfolg gelten muss (z. B. Filmstreifen-Geometrie mit 20 Zellen). | Klar markierter Platzhaltertext (kein `lumina-raw-fixture`) | **Nein** |
| **S2 — Smoke-/Layout-Raster** | Kleine synthetische PNGs: `LuminaApp::sample_image_png()` (4×3), `tests/fixtures/generative/*.png`, eingebettete 2×1-JPEG-Metadatenfixtures. | beliebig, deterministisch | Rasterja: ja (Screenshots der Smoke-Goldens) |

### Quellwahl je Golden (User-Entscheid 2026-09-26, verbindlich)

- **Wo Bildinhalt zählt, ein echtes CR3** — und nichts anderes. Ein
  herunterskaliertes CR3-Derivat ist **kein RAW**: die volle demosaizierte
  Auflösung *ist* das, was die Datei zur RAW macht. Ein verkleinertes Derivat
  ist ein gerendertes Bild und kann keine RAW-Aussage tragen. **Ein
  abgespecktes CR3 ist als RAW-Fixture ausgeschlossen** — es wäre ein
  vorgetäuschter Beweis, und zwar genau in den Goldens, die allein
  Bildqualität belegen sollen. Damit ist die previously offene Alternative
  „herunterskaliertes Derivat als committete Fixture" **verworfen**.
- **Für alles andere ein synthetisches PNG.** Layout, Bedienung, Badges,
  Auswahl, Reihenfolge, Scroll, Empty-State, Fehlerbanner, Sichtbarkeit —
  all das ist Chrome, und Chrome braucht keinen RAW-Decode.
- **Die 18-Byte-`.arw`-Sentinel sind damit vollständig abgeschafft.** Sie waren
  weder RAW noch PNG: kein Decode, kein Raster, nur ein Platzhaltertext unter
  einer RAW-Endung. Sie wurden als Nächstes zu `S1` umgewidmet und sind nun
  durch synthetische PNGs ersetzt.
- **Folge für die Laufzeit:** die Decode-Kosten der echten CR3 konzentrieren
  sich auf die R1-Goldens, die sie wirklich brauchen. Die Suite lief am
  2026-09-26 mit 301 s (43 passed / 13 failed) — das ist **kein** Hash-Problem.
  `THUMB-HASH-PERF-35` hat das Hashen pro Frame beseitigt (211,6 ms → 0,004 ms
  warm), die Laufzeit blieb unverändert, weil sie am LibRaw-Decode eines
  24-MP-CR3 im Debug-Build hängt (~0,69 s je Decode). Wer die Suite verkürzen
  will, stuft Goldens von R1 auf S2 herab, **wenn die Aussage das zulässt** —
  niemals durch Verkleinerung der Quelle.

### Normative Regeln

1. **R1 ist die einzige RAW-Quelle, und sie ist unverkleinert.** Eine Fixture,
   die Bildinhalt beansprucht, enthält die Original-Bytes der beiden
   lizenzierten CR3. Es gibt kein zweites RAW-Format, keine Kopie der
   12-MB-Dateien im Testbaum und **kein herunterskaliertes Derivat**.
2. **Kein Vorkalibrieren des Preview-Caches.** Eine Fixture, die einen
   `Standard`-Preview in den `.lumina`-Cache schreibt, würde einen Cache-Hit
   erzeugen und einen defekten Decode grün halten. Die Library-/Filmstreifen-
   Zellen bekommen ihre Pixel deshalb aus dem **Produktions-Thumbnail-Worker**,
   der die reale CR3 dekodiert. `prepare_folder_cache` legt nur das (gitignorierte)
   Verzeichnis an, um die Ordnerbaum-Zeilen zwischen kaltem und warmem Lauf
   stabil zu halten.
3. **S1 ist niemals ein Pixel-Nachweis.** Ein S1-Sentinel darf nie als Beleg
   für Bildqualität, Decode oder Rendern zitiert werden.
4. **S2 ist ein Smoke-/Layout-Beleg.** `sample_image_png()` ist ein
   **4×3-Pixel**-PNG. Jede Develop-Sektion, jeder Overlay-Golden und jedes
   Histogramm, das darauf aufbaut, ist eine Chrome-/Layout-Invariante: er
   sichert Regler, Panel, Scrollposition und Overlay-Geometrie, **nicht**
   fotografische Bildqualität.
5. **Ein Decode-Fehler ist nie Soll-Inhalt.** Kein Golden darf den Zustand
   „RAW-Dekodierung schlug fehl" als beabsichtigten Inhalt aufnehmen. Der
   passende Wächter ist `assert_no_raw_decode_failure` (kein `LuminaApp::error`,
   kein gerendertes `LibRaw`-Label, keine `LibRaw`-Statuszeile).
6. **Fremde Änderungen am Ordnerbaum sind Kopplungen.** Der Ordnerbaum zählt
   RAW-Dateien tiefenbegrenzt (`FOLDER_SCAN_DEPTH`). Ein Golden, das den
   Standard-Arbeitsordner rendert (z. B. `library_people_empty`), hängt an dieser
   Zahl; jede Fixture-Umbenennung ist dort eine sichtbare Änderung.

## 3. Golden-Klassifikation (normativ)

Jede committete Golden-Datei in `crates/lumina-gui/tests/snapshots/` ist genau
einer Klasse zugeordnet:

- **R — Render-Invariante.** Die Pixel enthalten echten Bildinhalt aus einer
  echten Bildpipeline. Nur diese Klasse darf als Beleg für eine
  Bildpipeline-Regression (Decode, Demosaicing, Farb, Geometrie, Renderstufe)
  zitiert werden.
- **C — Chrome-/Layout-Invariante.** Der Golden sichert ausschließlich
  UI-Chrome und Layout (Smoke, Panel, Scrollposition, Empty State, Dialog, Toast,
  über dem Preview gezeichnete Overlays). **Eine Chrome-Invariante darf niemals
  als Beleg für eine Bildpipeline-Regression zitiert werden** (siehe
  `Agents.md` „Keine stillen Fallbacks“ und die DoD-Prüfung auf
  tatsächliche Testabdeckung).

Die Spalte **Nachweis-Scope** sagt zusätzlich, *worauf* eine Render-Invariante
ihre Aussage stützt. Das ist nötig, weil eine Render-Invariante auf einem
*synthetischen* Szenario (CPU/GPU-Parity) gültige Pipeline-Aussagen trägt, aber
**keine** Aussage über einen echten RAW-Decode.

## 4. Inventar (Version 1)

**66** committete Golden-Dateien (gezählt mit
`git ls-files 'crates/lumina-gui/tests/snapshots/*.png'`; dieselbe Zahl meldet
`golden_ref.sh check` als `goldens.count`). Die in `Agents.todo.md` genannte Zahl
„56" ist die Anzahl der `#[ignore]`-Tests im Binary `kittest_snapshots`; davon
erzeugen 46 ein Golden, 10 sind reine Zustands-/Interaktions-Tests ohne Golden.
Die restlichen **20** Goldens gehören zu den anderen kittest-Binaries und zum
Lib-Test (46 + 20 = 66).

**Bilanz: 14 Render-Invarianten (Klasse R), 52 Chrome-/Layout-Invarianten
(Klasse C)** — 66 Zeilen, ausgezählt aus der Tabelle dieses Abschnitts.
(Zahlenkorrektur 2026-09-26: die Fassung vor der Zeilen-Ergänzung nannte 62 /
16 / 48 und war nach der Ergänzung um genau die vier neuen Goldens zu niedrig.)

### 4.1 `kittest_snapshots` (46 Goldens, Binary `tests/kittest_snapshots.rs`)

| # | Golden | Klasse | Fixture | Nachweis-Scope |
| --- | --- | --- | --- | --- |
| 1 | `develop_basic.png` | C | S2 | Regler-/Panel-Layout (4×3-Sample) |
| 2 | `develop_empty.png` | C | – | Empty State ohne Bild |
| 3 | `develop_history_no_sidecar.png` | C | S2 | History-Panel ohne Sidecar |
| 4 | `develop_overlay_crop.png` | C | S2 | Crop-Rechteck (Painter-Primitive) über 4×3-Textur |
| 5 | `develop_overlay_lens_blur.png` | C | S2 | Lens-Blur-Fokusrechteck (Primitive) |
| 6 | `develop_overlay_mask.png` | C | S2 | Masken-Matte aus dem 4×3-Alpha, nicht aus einem Decode |
| 7 | `develop_overlay_pins.png` | C | S2 | Edit-Pins (Primitive) |
| 8 | `develop_section_color.png` | C | S2 | HSL-/Color-Mixer-Layout |
| 9 | `develop_section_denoise.png` | C | S2 | Detail-Sektion + `unavailable`-Badge |
| 10 | `develop_section_detail.png` | C | S2 | Detail-Regler |
| 11 | `develop_section_effects.png` | C | S2 | Effects-Regler |
| 12 | `develop_section_geometry.png` | C | S2 | Geometry-Regler |
| 13 | `develop_section_history.png` | C | S2 | History-Zeile (Seed-Zustand) |
| 14 | `develop_section_masking.png` | C | S2 | Masking-Panel (Seed-Zustand) |
| 15 | `develop_section_optics.png` | C | S2 | Optics-Panel |
| 16 | `develop_section_optics2.png` | C | S2 | Optics-Panel, Lens-Blur-Subgruppe |
| 17 | `develop_section_presets.png` | C | S2 | Presets-Panel |
| 18 | `develop_section_rating.png` | C | S2 | Rating-/Flag-/Label-Zeilen |
| 19 | `develop_section_tone_curve.png` | C | S2 | Tone-Curve-Grafik |
| 20 | `develop_sections_expanded.png` | C | S2 | Sektions-Zustände |
| 21 | `draw_meta_preset_dialog.png` | C | S2 | Preset-Dialog |
| 22 | `error_dialog.png` | C | S2 | Fehlerdialog (lauter Pfad) |
| 23 | `export_module.png` | C | S2 | Export-Panel |
| 24 | `export_module_jpeg.png` | C | S2 | Export-Panel, JPEG gewählt |
| 25 | `filmstrip_twenty_dummies.png` | C | S1 + S2 | Filmstreifen-Geometrie (20 Zellen), 4×3-Preview |
| 26 | `generative_auto_fill_panel.png` | C | S2 | Auto-Fill-Panelzustand |
| 27 | `generative_expand_panel_off.png` | C | S2 | Generative-Panel, inaktiv |
| 28 | `generative_expand_panel_ready.png` | C | S2 | Generative-Panel, `valid` |
| 29 | `histogram_graphic.png` | C | S2 | Histogramm-**Grafik**; die Verteilung stammt aus 12 Sample-Pixeln |
| 30 | `library_compare.png` | **R** | **R1** | echte CR3-Thumbnails in Before/After + Filmstreifen |
| 31 | `library_empty.png` | C | – | Grid-Empty-State |
| 32 | `library_loupe.png` | **R** | **R1** | echte CR3-Thumbnails im Loupe |
| 33 | `library_meta_embedded.png` | C | S2 (2×1 JPEG) | Embedded-Metadaten-Panel |
| 34 | `library_meta_history.png` | C | S2 | Metadaten-History |
| 35 | `library_meta_preset.png` | C | S2 | Meta-Preset-Panel |
| 36 | `library_meta_sync.png` | C | S2 | Sync-to-selection-Zeilen |
| 37 | `library_metadata.png` | C | S2 | Draft-Editor |
| 38 | `library_metadata_copy_paste.png` | C | S2 | Copy/Paste-Zeile |
| 39 | `library_people_empty.png` | C | – | People-View; **kopplung** an den RAW-Zähler des Ordnerbaums |
| 40 | `library_rated_badges.png` | **R** | **R1** | echte CR3-Thumbnails + Rating-/Flag-/Label-Badges |
| 41 | `library_subfolder_badges.png` | **R** | **R1** | echte CR3-Thumbnails + relative Pfad-Badges |
| 42 | `library_survey.png` | **R** | **R1** | drei echte CR3-Thumbnails (Multi-Selection) |
| 43 | `library_with_image.png` | C | S2 | Grid mit leerem Center (Empty State) |
| 44 | `navigator_closed.png` | C | S2 | Navigator zu |
| 45 | `navigator_viewport.png` | C | S2 | Navigator-Viewport-Rechteck |
| 46 | `toast_info.png` | C | S2 | Toast-Overlay |

### 4.2 Weitere kittest-Binaries (20 Goldens)

| # | Golden | Klasse | Fixture | Nachweis-Scope | Owner |
| --- | --- | --- | --- | --- | --- |
| 47 | `develop_overlay_crop_interactive.png` | C | S2 | Crop-Handle-Interaktion | `kittest_crop_overlay` |
| 48 | `develop_overlay_crop_rotation.png` | C | S2 | Crop + Rotation | `kittest_crop_overlay` |
| 49 | `library_stack_membership.png` | **R** | **R1** | echte CR3-Thumbnails + amber Stapel-Zeichen | `kittest_library_stack` |
| 50 | `mask_view_visibility.png` | C | S2 | Overlay-Sichtbarkeit | `kittest_mask_visibility` |
| 51 | `develop_spot_selected.png` | C | S2 | Spot-Auswahl | `kittest_spot_tool` |
| 52 | `develop_spot_tool_options.png` | C | S2 | Spot-Optionen | `kittest_spot_tool` |
| 53 | `develop_spot_tool_options_expanded.png` | C | S2 | Spot-Optionen, aufgeklappt | `kittest_spot_tool` |
| 54 | `mask_management_controls.png` | C | S2 | Maskenverwaltung | Lib-Test `src/tests/brush_management.rs` |
| 55 | `parity_paths_neutral_cpu.png` | **R** | S2-Szenario | Pipeline-Ausgabe CPU (synthetische Szene) | `kittest_parity` |
| 56 | `parity_paths_neutral_gpu.png` | **R** | S2-Szenario | Pipeline-Ausgabe GPU (synthetische Szene) | `kittest_parity` |
| 57 | `parity_paths_tinted_cpu.png` | **R** | S2-Szenario | getönte Szene, CPU | `kittest_parity` |
| 58 | `parity_paths_tinted_gpu.png` | **R** | S2-Szenario | getönte Szene, GPU | `kittest_parity` |
| 59 | `parity_paths_detail_cpu.png` | **R** | S2-Szenario | Detail-Stufe, CPU | `kittest_parity` |
| 60 | `parity_paths_detail_gpu.png` | **R** | S2-Szenario | Detail-Stufe, GPU | `kittest_parity` |
| 61 | `parity_paths_lensfun_corrector_cpu.png` | **R** | S2-Szenario | Lensfun-Korrektur, CPU | `kittest_parity` |
| 62 | `parity_paths_lensfun_corrector_gpu.png` | **R** | S2-Szenario | Lensfun-Korrektur, GPU | `kittest_parity` |
| 63 | `mask_local_tone_curve.png` | C | S2 | Mask-Local-Kurvenblock: Kanalreihe, Graph, Reset | `kittest_mask_local` |
| 64 | `mask_local_color.png` | C | S2 | Mask-Local-Color: HSL-Bänder, Vibrance-Paar, Grading | `kittest_mask_local` |
| 65 | `mask_local_presence.png` | C | S2 | Mask-Local-Presence: Texture/Clarity/Dehaze | `kittest_mask_local` |
| 66 | `mask_local_detail.png` | C | S2 | Mask-Local-Detail: Sharpening, Noise Reduction, Resets | `kittest_mask_local` |

**Zeilen 63–66 ergänzt 2026-09-26** (Verifikationsbefund `GUI-INT-MASKLOCAL-38`/M3).
Die vier Mask-Local-Goldens waren seit `2e9827f` committet, aber in dieser
Tabelle nie geführt — bei genau der Aufgabe, deren Abnahmekriterium (c) „die
Goldens sind als R1/S1/S2 klassifiziert" verlangt. Ihre Klassifikation stand
bisher nur in Doc-Kommentaren der Testdateien („Class C / S2"), die
`golden_ref.sh` nicht liest. **Sichtbarkeitsangaben je Golden** (welche Zeilen
über dem Fold liegen) stehen in `feature/product/ai-masks.md` §6.3 und im
Doc-Kommentar von `tests/kittest_mask_local.rs`; beide sind aus den
committeten PNGs gelesen, nicht geschätzt. `golden_ref.sh check` vergleicht
Digests, nicht diese Tabelle — eine fehlende Zeile fällt dort nicht auf.

### 4.3 Testseitig erzwungene Wächter

- `settle_thumbnails` (in `tests/kittest_fixtures_support/`): wartet, bis der
  Produktions-Worker für **jede** gestagete Datei einen `Standard`-Preview mit
  **exakt passendem Quell-Hash** geschrieben hat, und schlägt laut fehl, wenn das
  nicht geschieht. Ein Platzhalter kann so nicht gemalt werden.
- `assert_thumbnail_is_real`: prüft die heruntergerechnete Vorschau gegen die
  exakte Produktionsgeometrie (6032×4024 → 200×133, 4024×6032 → 133×200) **und**
  auf ≥ 400 verschiedene RGB-Tripel. Die entfernte synthetische Farbrampe
  erreicht höchstens ~192.
- `assert_no_raw_decode_failure`: kein `LuminaApp::error`, kein gerendertes
  `LibRaw`-Label, keine `LibRaw`-Statuszeile.

## 5. Bekannte Grenzen (Stand Inventar-Version 1)

1. **Die Develop-Sektionen haben keine Render-Invariante.** Sie laufen alle auf
   `sample_image_png()` (4×3). Ein Tonwert-, Schärfe- oder Geometrie-Regress der
   Develop-Pipeline ist heute **nicht** golden-abgedeckt; das ist eine Lücke, keine
   Klassifikation. Auflösung: entweder ein Real-RAW-Develop-Golden (Kosten siehe
   §6) oder eine nicht-golden Testabdeckung auf Pixelebene in `lumina-core`.
2. **`histogram_graphic.png` sichert die Grafik, keine Histogrammdaten.** Die
   Verteilung stammt aus 12 Pixeln.
3. **Die Parity-Goldens sind Render-Invarianten auf synthetischer Szene.** Sie
   belegen CPU/GPU-Parität der Pipeline, nicht den RAW-Decode.
4. **`library_people_empty.png` hängt an einem Ordnerbaum-Zähler.** Eine spätere
   Fixture-Umbenennung ist dort eine sichtbare Änderung (kein stiller Drift).
5. **Die Goldens sind vor dieser Änderung nicht neu geschrieben.** Die
   Baseline-Erneuerung gehört `GOLDEN-BASELINE-32`; bis dahin sind die fünf
   Library-Render-Invarianten und die von der Ordnerbaum-/Badge-Textänderung
   betroffenen Chrome-Invarianten planmäßig rot.

## 6. Offene Messfrage: Laufzeit der R1-Fixtures

`cargo test -p lumina-gui --test kittest_snapshots -- --ignored` (macOS,
Apple M5 Pro, Debug-Profil, LibRaw 0.22.2):

| | vorher (Sentinel-Bytes) | nachher (echte CR3) |
| --- | --- | --- |
| Testzeit | **2,29 s** | **300,97 s** |
| Wall (inkl. Cargo) | **2,46 s** | **301,85 s** |
| CPU (`user`) | 20,97 s | 2.147 s |
| Ergebnis | 44 bestanden / 12 rot | 43 bestanden / 13 rot |
| Einzeltest Library-Fixture | ~0,1 s | **~17 s** (`library_loupe`, seriell) |

Das ist eine **~123-fache Wandzeit- und ~102-fache CPU-Verdopplung** und damit
**nicht akzeptabel**. Einzelmessungen, die die Ursache belegen:

- `lumina_raw::decode_bytes` auf einer CR3: **~0,69 s** (Debug-Profil).
- BLAKE3 über 12 MB: **~0,105 s**.
- **Ein UI-Frame über ein Drei-Zellen-Fixture kostet ~0,79 s** (gemessen:
  6 Frames in 4,76 s, 5 Frames in 4,01 s).

Die Ursache ist *nicht* primär der Decode, sondern ein Produkt-Hotspot:
`FilmstripManager::refresh_source` (aufgerufen aus `ensure_thumbnail` für jede
sichtbare Zelle **in jedem Frame**, *vor* den billigen Early-Outs) berechnet über
`sidecar_bundle_identity` → `FileContentIdentity::from_path` einen vollständigen
BLAKE3 der Quelldatei **auf dem UI-Thread**. Mit 18-Byte-Sentinels war das
kostenlos; mit realistischen RAWs dominiert es die Laufzeit. Dasselbe Bild liefert
der Gegenbeweis: `settle_thumbnails` wartet bewusst **ohne** UI-Frames (der
Worker-Pool läuft in eigenen Threads) — dadurch sank die Einzeltestzeit von
23 s auf 17 s, **ohne dass sich ein einziges gerendertes Pixel änderte**.

**Status: GELÖST als `THUMB-HASH-PERF-35`** (umgesetzt, unabhängig verifiziert
2026-09-26, Commit `767c756` + `b5c5253`). Die Empfehlung (a) wurde umgesetzt —
**aber nicht mit dem ursprünglich vorgeschlagenen Cache-Schlüssel.**

- **(a) Produktfix — UMGESETZT als `THUMB-HASH-PERF-35`.** Quellidentität wird
  pro **`(Pfad, mtime, ctime, len)`** gecacht, nicht pro `(Pfad, mtime, len)`.
  ⚠ **`(Pfad, mtime, len)` ist nachweislich unsicher und darf nicht
  reimplementiert werden:** zwei committete Regressionstests
  (`thumbnail_source_replacement_invalidates_cached_and_pending_state`,
  `thumbnail_artifact_change_drops_ram_pixels_and_requeues_worker`) fallen unter
  diesem Schlüssel um, denn er kann eine Datei derselben Länge und mit
  wiederhergestelltem `mtime` nicht von der Vorversion unterscheiden. `ctime` ist
  kernelgepflegt, per `utimensat`/`touch -r` **nicht** zurücksetzbar und kostet
  keinen zusätzlichen Syscall. Messung: 12 MB CR3, ~115 ms pro Lookup vorher,
  ~0,004 ms warm nachher; ~0,79 s pro Frame bei 3 Zellen → vernachlässigbar.
  Normativ in `feature/platform/cli-gui-wasm.md` § *Quell-Identitäts-Cache im
  UI-Thread*.
- **(b) Fixture-seitig**: ein herunterskaliertes echtes RAW-Derivat als
  committete Fixture (z. B. ~1–2 MP statt 24 MP). Senkt Hash **und** Decode
  um ~12×. Erfordert eine neue Fixture-Formatentscheidung und ist deshalb
  **nicht** eigenmächtig umgesetzt.
- **(c) Rahmen unverändert lassen** (aktueller Stand): die R1-Stage ist
  funktional korrekt und aussagekräftig, kostet aber ~5 min statt ~2,5 s für
  dieses lokale, GPU-gegatete Gate.

## 7. Verweise

- `Agents.md` (Verifikationsregeln, Anti-Gaming), `DoD.md` §7
- [`fixtures-licensing.md`](fixtures-licensing.md) — Fixture-Inventar und Lizenzen
  (F-073/F-078)
- [`conflicts-and-acceptance.md`](conflicts-and-acceptance.md) — Abnahmeszenarien
- `sample-data/raw/README.md` §4/§8 — Provenienz und Lizenzgewährung der CR3s
- `crates/lumina-gui/tests/kittest_fixtures_support/` — technische Umsetzung
  (R1-Staging, Wächter)
- Schwesterdokument: `feature/quality/golden-references.md` (GOLDEN-REF-30,
  Referenzplattform und Fingerabdruck) — orthogonal, ergänzt die Referenz-
  *umgebung*, dieser Vertrag definiert die Fixture- und Beweis-*semantik*.
