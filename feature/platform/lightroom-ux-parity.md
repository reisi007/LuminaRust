# Lightroom-UX-Parität (UI/UX; Zielbild 2026-09-17 präzisiert)

**Status (2026-09-17):** Bestandsaufnahme + Zielbild. User-Entscheide: 2026-09-12
1:1-Klon von Adobe Lightroom Classic (UI/UX); **Präzisierung 2026-09-17: kein
reines 1:1, sondern leicht modernerer Feinschliff — jeder Lightroom-Nutzer soll
sich sofort zu Hause fühlen** (gleiche Orte, Begriffe, Abläufe; Modernisierung
nur im Feinschliff, kein Umlernen). Engine-Parität läuft weiter über die
LRPAR-Tasks. Dieses Dokument ersetzt
nicht die F-100-Konventionen
(`platform/cli-gui-wasm.md`), sondern inventarisiert die Lücken dagegen und
definiert die Slices zum Schließen. Jede Slice-Implementierung beginnt mit
der hier vermerkten SOLL-Entscheidung (Doku-first-Regel); ohne sie kein Code.

**Quellen:** UX-Gap-Analyse „Lightroom-Classic-Klon" (15 Gaps, general-Agent,
read-only, 2026-09-12), UX-Gap-Analyse „Dust Removal" (8 Gaps, general-Agent,
read-only, 2026-09-12), `.goal/Goal.md` G-01…G-16,
`.goal/lighroom screenshots batch 1/` + `Content.md`.

## Gap-Inventory Klon (UXG)

| ID | Lücke (Classic vs. Lumina) | Evidenz | SOLL-Entscheid nötig | Task-Anker |
| --- | --- | --- | --- | --- |
| UXG-01 | Crop nicht interaktiv (nur Badge + x/y/w/h-Felder statt Rahmen/Handles/Drag/Enter) | `lib.rs:12758`, `:14287` | ja (Commit-Semantik) | neu |
| UXG-02 | Presets/History/Rating rechts statt links; Snapshots-Panel fehlt | `lib.rs:18418`, `:16725` | ja (Layout) | neu |
| UXG-03 | Library-Chrome fehlt (Catalog-Panel, View-Toolbar, Filterleiste, Quick Develop, Import-Dialog) | `lib.rs:15924`, `:18208` | ja (G-09-Scope) | neu |
| UXG-04 | Keine Werkzeugleiste über dem Bild (Q/K/M/R nur via Panel) | `lib.rs:17059` | eher nein | LRPAR-G11-OVERLAYS |
| UXG-05 | Panels ohne Kontext-Dynamik (Sections sind kollabierbar, aber nichts blendet je nach Tool/Selektion ein/aus) | `lib.rs:16735` | ja (Panel-Dynamik) | neu |
| UXG-06 | Develop-Reihenfolge ≠ Classic; Calibration-Panel fehlt | `lib.rs:16651` | ja (F-100-Reihenfolge) | neu |
| UXG-07 | History zeigt Maschinen-IDs statt Namen/Werte/Zeit | `lib.rs:16526` | ja (Schema/Extras) | neu |
| UXG-08 | Loupe/Compare/Survey sind Thumbnail-Stubs (kein Zoom/Pan, kein Render) | `lib.rs:16203` | ja (G-09-Scope) | neu |
| UXG-09 | Filmstreifen ohne Badges/Filter/Sort/Zähler | `lib.rs:16768` | nein | neu |
| UXG-10 | Before/After unvollständig (kein echtes Split); Softproof nur Badge | `lib.rs:13034` | teils | LRPAR-G10-VIEWER |
| UXG-11 | Keine Menüleiste; Modul-Leiste nicht Classic-artig | `lib.rs:18364` | ja (Menüumfang) | neu |
| UXG-12 | Presets = Datei-Browser statt Baum (kein Amount/Gruppen) | `lib.rs:16418` | ja (Amount-Semantik) | neu |
| UXG-13 | Kürzel `O`/`Z`/`Shift+L`/`I` fehlen; `E`-Alias unsauber | `lib.rs:135` | nein (F-100 erweitern) | LRPAR-G16-POWER |
| UXG-14 | Keine Canvas-Cursor; keine Stepper/Reset-Affordanz | Grep `CursorIcon` leer | nein | neu |
| UXG-15 | Navigator-Rail: Größe/Drag-Affordanz schwach (Duplikat-These visuell nicht belegt, niedrig prior) | `lib.rs:17347` | ja (Layout) | neu |
| UXG-16 | Tone Curve ohne Kurvengrafik (P0/P1-Slider + Add-Point statt Punkte setzen/ziehen; Vision 2026-09-17: „bitterste Umlernstelle") | `lib.rs:15832`, `develop_section_tone_curve.png` | ja (Interaktion/Kanal) | neu |
| UXG-17 | Hardcodierte UI-Literale statt `Str::` (F-100: englisch, 0 deutsche UI-Literale); Generative-Panels 2026-09-17 behoben (`ExpandDragFrameHint`/`ExpandCropToImage`), Spot-Panel (`draw_spot_heal`) offen | `i18n.rs`, `lib.rs:17185/17188` (behoben), `lib.rs:17680–17719` (offen) | nein | neu |
| UXG-18 | Vision-Feinschliff 2026-09-17 (je einzeln entscheidbar): HSL ohne 8 Color-Mixer-Farbfelder; Compare/Survey-Tiles unausgerichtet; Toast überdeckt Toolbar-Kante; Preset-Dialog unzentriert/beschnitten; Meta-History als Rohdaten-Dump (ISO/Pipe); Histogramm ohne RGB-Kanäle/Clipping-Dreiecke/EXIF-Zeile; kein EXIF-Overlay über dem Bild; Slider-Blau ohne Akzent-Disziplin | Goldens + Vision-Urteil 2026-09-17 | teils | neu |

## Gap-Inventory Dust Removal (UXD, G-04)

| ID | Lücke | Evidenz | SOLL-Entscheid nötig |
| --- | --- | --- | --- |
| UXD-01 | Spot-Tool armiert Vorschau nicht (Klick/Zug wirkungslos — „kein brush") | `lib.rs:12223`, `:12432` | nein (SOLL deckt ab) |
| UXD-02 | Kein Brush-Cursor (Kreis = Radius, folgt Maus) | kein `CursorIcon` | klein (Cursor-SOLL präzisieren) |
| UXD-03 | Size/Feather nur Panel-Slider (Rad = Zoom statt Size) | `lib.rs:12158` | klein (Shift-Feather präzisieren) |
| UXD-04 | Pins: einzelner Fixpunkt, kein Target/Source-Paar, kein Drag/Delete/`/` | `lib.rs:4188` | **ja** (Pin-Semantik) |
| UXD-05 | Panel statisch/überladen statt kontextuell | `lib.rs:15123` | **ja** (Panel-Zielbild) |
| UXD-06 | Visualize = Dauer-Tint statt werkzeuggekoppelter High-Contrast-Ansicht | `lib.rs:10695` | teils |
| UXD-07 | Kein Quell-Pin/Alt-Ziehen (Offset unsichtbar) | `commit_spot_heal` | nein (SOLL deckt ab) |
| UXD-08 | Kein Einzel-Klick-Anlegen (nur Detect→Apply/CLI) | `lib.rs:15163` | nein (SOLL deckt ab) |

Direkt umsetzbar ohne Doc-Entscheid: UXD-01, -02, -03, -07, -08.

## Zielbild (3 Slices)

- **Slice A — Makro-Geometrie:** Menüleiste + zentrierte Modul-Leiste;
  Develop links Navigator + Presets + Snapshots + History; rechts Histogram +
  Basic … Calibration in Classic-Reihenfolge; darunter Filmstreifen mit
  Badges + Filter-/Zähl-Leiste. (UXG-02, -03 tlw., -06, -09, -11, -12, -15)
- **Slice B — Interaktives Werkzeug-Fundament:** Tool-Strip über dem Bild;
  On-Canvas-Crop mit Handles/Enter-Commit; werkzeugabhängige Cursor;
  Spot-Heal als echter Retusche-Modus (Brush-Cursor, Klick-Anlegen,
  Target/Source-Pins, Rad = Size); echte Split-Before/After- und
  Loupe-/Compare-/Survey-Renderansichten. (UXG-01, -04, -08, -10, -14; UXD)
- **Slice C — Library-Verwaltung:** Catalog-/Collections-Panel links;
  permanente Filter- und Quick-Develop-Leiste rechts; Import-Dialog; humane
  History-Labels; kontextuelle Panels (Tool-/Selektions-getrieben).
  (UXG-03 Rest, -05, -07, -13)

## Vision-Korrekturen (2026-09-12, 8 Goldens gesichtet)

UXG-05/UXG-15 wie oben präzisiert; UXG-09 verschärft (Filmstrip teils ganz
ohne Thumbs, nicht nur ohne Badges — Thumb-Infrastruktur aus Library
wiederverwenden). Zusätzlich gesichtet: Render-Hash als Canvas-Text
(Debug-Noise), doppelte Labels („Export/Export to"), weißer Randstreifen
neben Metadata-Panel (Layout-Bruch), kahler Library-Empty-State. Erster
Implementierungs-Slice: UX-SLICE-1 (Agents.todo.md).

## Offene SOLL-Entscheide (vor jeweiliger Slice-Implementierung)

Crop-Commit-Semantik, Seiten-Layout (Presets/History links, Menüumfang),
G-09-Scope-Anhebung (Catalog/Import/Loupe-Render), F-100-Reihenfolge
(Calibration, Color-Split, Optics vor Effects), History-Schema (Labels),
Pin-Semantik (Auswahl/Delete/`/`), Panel-Dynamik-Zielbild, Softproof-Anchor.

## SOLL-Entscheide UX-LOOK-18 (User-Entscheide 2026-09-19, verbindlich)

Alle fünf interaktiv abgefragt und mit der empfohlenen Option entschieden.
**Namen-Vorbehalt (User-Regel 2026-09-19):** Sämtliche sichtbaren Namen —
Panel-/Gruppen-/Preset-Baum-Labels, History-Eintragstexte, Toolbar-Tooltips —
werden noch NICHT final entschieden. Bis Pre-MVP gelten technische
Arbeitslabels (deutsch/englisch wie Bestand, `Str::`-Pflicht aus UXG-17
bleibt); die finale Benennung erfolgt in einem eigenen Naming-Durchgang
zusammen mit NAMING-F1. Implementierungen dürfen deshalb keine
Namen als „final" dokumentieren oder per Golden zementieren, was noch
offen ist — Goldens pinnen Layout/Geometrie, keine Wortlaute über das
Bestehende hinaus.

- **UX-LOOK-LAYOUT-18 (UXG-02): LR-Classic links.** Linke Rail: Navigator +
  Presets-Baum + Snapshots + History + Copy/Paste; Footer-Admin-Aktionen
  entzerrt, „Previous | Reset"-Äquivalent rechts verankert. Rezept-/Sidecar-
  Verhalten unverändert (reines Layout).
- **UX-LOOK-TOOLBAR-18 (UXG-04): Icons + Tooltips.** Crop/Heal/Red-Eye/Masken
  + View-Toggles als Icons am LR-Ort (unter Histogramm/über Bild), aktive
  Tools hervorgehoben, Tooltip mit Shortcut; Library-View-Tabs ikonisiert.
- **UX-LOOK-TONECURVE-18 (UXG-16): Spline pro Kanal.** Echte Kurvengrafik,
  Punkte setzen/ziehen/löschen pro Kanal (R/G/B/RGB-Master), weiche
  Spline-Interpolation durch die Punkte (LR-Verhalten); persistiert wie
  bisherige Kurvenparameter (kein Schema-Bruch, Migration wo nötig).
- **UX-LOOK-CROP-18 (UXG-01): Enter/Esc.** Eck-Handles + Drittel-Gitter +
  Abdunklung; Ziehen ändert live die Vorschau, Enter bestätigt (Rezept wird
  erst bei Commit geschrieben), Esc verwirft. Rezept-Semantik außer dem
  dokumentierten Commit unverändert.
- **UX-LOOK-HISTORY-18 (UXG-07/12): Strukturiert im Sidecar.** History-Einträge
  mit Reglername + alt→neu + Uhrzeit als strukturierte Felder im Sidecar
  (Migration nach Pre-MVP-Regel: Breaking bis MVP erlaubt, Loader lehnt
  Inkompatibles laut ab); Presets als Gruppen-Baum ohne absolute Pfade.
  Anzeigetexte sind Arbeitslabels (Namen-Vorbehalt oben).

## UX-SLICE-2 — Polish-Follow-ups (2026-09-13)

Fortsetzung von UX-SLICE-1; die Punkte F1–F6 stammen aus dessen
Verifizierung. SOLL-Entscheide, die vor der Implementierung festgehalten
wurden:

- **F1 — Render-Hash bei 0 Bildern (Gate, UX-SLICE-3 präzisiert):** Der Hash in der Statuszeile
  beschreibt den geladenen Render. Das Library-Raster ist RAW-only; eine
  geladene Nicht-RAW-Datei hat dort keine Repräsentation. Der Hash wird daher
  im Modul Library nicht gezeigt, solange das **gefilterte** RAW-Raster leer ist
  (`filtered_library_order()` — dasselbe Prädikat wie der Empty-State; ein
  Filter mit 0 Treffern zählt als leer, kein Widerspruch zwischen „No images“
  und einem Render-Hash); in Develop/Export und bei nicht-leerem Raster bleibt
  er sichtbar. `render_key` wird bewusst
  **nicht** gelöscht (der geladene Render bleibt gültig) — reines Anzeige-Gate.
- **F2 — „Open Folder“-CTA (Picker-Scope):** Der CTA öffnet den nativen
  Ordner-Picker (`rfd::FileDialog::pick_folder`) und setzt das gewählte
  Verzeichnis. `rfd` ist bereits GUI-Dependency und erfährt keine neue
  Capability; das Label „Open Folder“ ist damit ehrlich. Ein abgebrochener
  Dialog ist ein bewusster No-op (kein Status, kein Fehler). Für headless Tests
  ist der Picker injizierbar (Session-State, nie persistiert).
- **F3 — ein Empty-State für alle Library-Ansichten:** Grid, Loupe, Compare und
  Survey zeigen denselben zentrierten Empty-State (Icon + Titel + Body + CTA);
  der Navigator-Rail zeigt bei 0 Einträgen den ehrlichen Hinweis
  „No images in this folder“ statt „Click a thumbnail to open it“.
- **F4 — Badge-Painting pixel-belegt:** eigener Golden mit bewerteten,
  geflaggten und gelabelten Fixtures (Grid **und** Filmstreifen) plus
  Pixel-Assert auf `LIBRARY_BADGE_BG`; kein stiller Fallback.
- **F5 — schärfere Asserts:** Hash-Abwesenheit am Canvas (genau ein
  Hash-Knoten, im Header) und CTA-Verdrahtung über den injizierten Picker.
- **F6 — Traceability:** Mapping P1–P5 ↔ UXG ↔ Code-Anker (Tabelle unten);
  Code-Kommentare referenzieren die kanonischen IDs.

### UX-SLICE-1 Traceability (P1–P5 / UXG-Mapper)

| P | UX-SLICE-1-Punkt | UXG | Code-Anker |
| --- | --- | --- | --- |
| P1 | Filmstrip-Komponente vereinheitlicht (Thumbs, n-von-N, Badges, ehrlicher Empty-Text) | UXG-09 | `draw_filmstrip`, `paint_entry_badge` |
| P2 | Render-Hash von Canvas in Statuszeile; Draft/Stale als Rand-Badges | UXG-07 | `update`-Header, `draw_preview` |
| P3 | Export-Ziel-Label eindeutig (`Destination` statt Doppel-Label) | — (Vision) | Export-Panel (`ExportTarget`) |
| P4 | Weißer Randstreifen neben dem Metadata-Panel behoben (`auto_shrink`) | — (Vision) | rechtes Panel in `update` |
| P5 | Library-Empty-State mit deterministischem Icon + CTA | — (Vision) | `draw_library_empty_state` |

## Abnahme (je Slice)

DoD §6 Vision-Review der geänderten Layouts + kittest-Goldens (neu/geändert)
+ Headless-Asserts für Tool-Zustände; manueller Abgleich gegen die
`.goal`-Screenshots; unabhängige Verifizierung (BESTANDEN) vor Commit.

## Vision-Befunde UX-LOOK-LAYOUT-18 (Vision-Loop 2026-09-19, Build-Agent-sichtig)

Nachgelagerter DoD-§6.2-Loop über 5 Alt/Neu-Golden-Paare (Commit d34ba0a),
vom Build-Agenten am Bild gegengeprüft:

- **LAYOUT-V1 (mittel, Fix-Pflicht nach TOOLBAR-18):** Footer-Überlappung in
  schmalen Zuständen — „Regenerate Stale / Missing" und „Render / Apply"
  überlagern sich zu unlesbarem „…Missing der / Apply"
  (`develop_section_history`, `develop_section_presets`, `navigator_closed`;
  in `develop_basic`/`develop_overlay_crop` korrekt 3-zeilig). Die
  `horizontal_wrapped`-Footer-Zeilen wrappen nicht sauber. Fix als eigener
  Slice nach UX-LOOK-TOOLBAR-18 (kein neuer Todo-Task, Follow-up dieser
  Sektion).
- **LAYOUT-V2 (niedrig, dokumentiertes Verhalten, kein Fix):** Bei
  geschlossener linker Rail sind Presets/Snapshot/History nicht sichtbar.
  Das entspricht Lightroom (Panel-Toggle blendet aus, derselbe Toggle blendet
  ein — ein Klick auf „Navigator"/„Panels (Tab)"); kein Funktionsverlust.
  Kein Fallback im rechten Panel (bewusst per LAYOUT-SOLL entfernt).
- **LAYOUT-V3 (niedrig, Kosmetik, Folge-PR):** Toolbar-Ende („Panels (Tab)")
  wird bei schmaler Canvas-Fläche knapp gekappt.

## Umsetzungsstand UX-LOOK-TOOLBAR-18 (2026-09-19)

SOLL-Entscheid oben umgesetzt (Implementierung + headless/kittest-Goldens,
Commit ausstehend bis unabhängiger Verifizierung):

- **Preview-Werkzeugleiste ist ikonisch** (`src/icon_toolbar.rs`,
  `LuminaApp::draw_view_toolbar` in `src/app_frame.rs`): Crop / Heal / Red-Eye /
  Masken als Werkzeuge (LR-Reihenfolge, gleicher Pfad wie `R`/`Q`/Detail-Picker/
  `K`·`M`), danach Clipping / Split / Lights-Out / Panels / All-Panels /
  Fullscreen als View-Toggles (gleicher Pfad wie `J`/`Shift+Y`/`L`/`Tab`/
  `Shift+Tab`/`F`). Aktive Tools sind per Akzent-Highlight hervorgehoben,
  Tooltips tragen die bestehenden Arbeitslabels + Shortcuts
  (Namen-Vorbehalt: keine neuen finalen Wortlaute, `i18n.rs` unverändert).
- **Library-View-Tabs ikonisiert** (`src/library_grid.rs`): Grid / Loupe /
  Compare / Survey / People als Icons mit denselben `Str`-Tooltips; Klick
  routet unverändert über `set_library_view`.
- **Icons sind primitive-gezeichnet** (Linien/Kreise/Rects, keine Emoji-Glyphe):
  deterministisch in headless und kittest. Kein Rezept-, Sidecar- oder
  Persistenzverhalten geändert (reine Auslösung bestehender Aktionen).
- **Kein neuer Shortcut**; `Q` nutzt jetzt denselben `toggle_spot_heal_tool`
  wie der Toolbar-Button (Status „Spot heal armed/disarmed (Q)").
- **Risiko (bekannt, nicht Fix dieser Task):** die Tooltips liegen auf
  `Response::on_hover_text`; im headless/kittest-Pfad werden sie nicht
  gerendert, gepinnt wird über den Widget-Id-Treffer (`assert_icon_painted`).
- **Goldens:** `kittest_snapshots` mit `UPDATE_SNAPSHOTS=true` neu erzeugt
  (46/56 Views enthalten die Leiste), Diff-Sichtung bestätigt die erwarteten
  Deltas (Toolbar-Zeile unter dem Zoom-Toolbar, View-Tab-Zeile, dadurch leicht
  verschobener Canvas/Empty-State, identische Farb-/Zustands-Signale).
  `kittest_parity` (separates Target, GPU-abhängig) bewusst nicht verändert.
