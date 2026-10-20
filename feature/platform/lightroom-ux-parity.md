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

**Anker-Form (Stand 2026-10-20):** Die Spalte „Fundstelle" nennt den
vollständigen repo-relativen **Pfad + Symbolnamen** — keine Zeilennummern.
Zeilennummern driften mit jedem Refactor-Slice; die Alt-Anker dieser Tabelle
sind deshalb gegen **zwei** Stände notiert:

- **UXG-01…UXG-15 und alle UXD-Zeilen** gegen `crates/lumina-gui/src/lib.rs`
  im Stand 2026-09-12 (Commit `b231920`;
  `git show b231920:crates/lumina-gui/src/lib.rs | wc -l` = 31 909 Zeilen),
- **UXG-16 und UXG-17** gegen denselben Pfad im Stand 2026-09-17 (Commit
  `cd440fb`) — diese beiden Zeilen hat Commit `8de5585`
  („Look-Analyse Top-Findings: Zielbild + UXG-16-18 + i18n-Fix") eingefügt.

**Methodenvermerk (Stand 2026-10-20):** die Statusangaben — „geschlossen",
„teilweise geschlossen" und die in der Spalte „Fundstelle" genannten
Restbefunde („Restbefund:", „behoben:"/"offen:") — sind **per
Code-Lesung** belegt — Quelldatei + Symbolname, ggf. der schließende Commit
im Doc-Kommentar des Moduls — und **nicht per Testlauf**. Ein Testlauf steht
aus; bis dahin sind die Einstufungen Quellcode-Stand, kein Lauftest-Ernis.

Die Alt-Tabelle (Zustand vor dieser Welle) trug 26 Zeilen-Anker: 22 in der
Form `datei.rs:NNNN` (commit-gepinnter Zählweg:
`git show 3bc67d8:feature/platform/lightroom-ux-parity.md | grep -oE
'[A-Za-z0-9_./-]+\.rs:[0-9]+' | wc -l`) und 4 Fortsetzungen der Form „`:Zahl`"
in derselben Tabellenzelle (`git show 3bc67d8:feature/platform/lightroom-ux-parity.md
| grep -oE '`:[0-9]+(-[0-9]+)?`' | wc -l`). Zum Zeitpunkt der Erhebung
(Commit `8de5585`) waren es 23 + 4 = 27 — die Differenz ist UXG-15, dessen
Anker später vom `lib.rs`-Zeilenanker auf den Dateinamen `navigator.rs`
umgestellt wurde.

Nach den 20 Refactor-Slices (GUI-LIB-SPLIT-65; Slice 20 = `ddbbb72`) ist
`crates/lumina-gui/src/lib.rs` stark geschrumpft (`wc -l
crates/lumina-gui/src/lib.rs`): 24 der 26 Alt-Anker lagen jenseits dieses EOF
(Zählweg: `git show 3bc67d8:feature/platform/lightroom-ux-parity.md | grep -oE
'[A-Za-z0-9_./-]+\.rs:[0-9]+|`:[0-9]+(-[0-9]+)?`' | sed 's/^[^0-9]*//' | awk
-v eof="$(wc -l < crates/lumina-gui/src/lib.rs)" '$1>eof' | wc -l`), die zwei
übrigen lagen innerhalb — der eine zeigte damals auf `module_for_key`, der
andere auf die `spot_removals`-Deserialisierung innerhalb von
`LuminaApp::visible_edit_pins`; heute zeigen die beiden Zeilen auf beliebigen
Code und damit auf keinen tragfähigen Anker.

Repo-Regel: `DoD.md` („Eine Zahl, die man nicht nachzählen kann, wird **nicht**
hingeschrieben") und `feature/quality/security-review-register.md` § 4
„Regeln für die Einträge", Punkt 7 „Stabile Bezüge" („Zeilen verweisen auf
Pfade und Testnamen, nicht auf Zeilennummern (die driften). Der Beleg nennt
das Kommando, das den Befund reproduzierbar zeigt."). Belegt wird jeder Anker
mit `grep -n <Symbol> <Pfad>`.

| ID | Lücke (Classic vs. Lumina) | Fundstelle (Pfad + Symbol) | SOLL-Entscheid nötig | Task-Anker |
| --- | --- | --- | --- | --- |
| UXG-01 | **Geschlossen (UX-LOOK-CROP-18):** Crop nicht interaktiv (nur Badge + x/y/w/h-Felder statt Rahmen/Handles/Drag/Enter) | `crates/lumina-gui/src/develop_geometry/crop_overlay.rs` → `LuminaApp::draw_crop_overlay` (vier Eck-Handles, Drag, Drittel-Gitter, `Enter`/`Esc`); `crates/lumina-gui/src/develop_geometry.rs` → `LuminaApp::draw_geometry` | nein (erledigt) | UX-LOOK-CROP-18 |
| UXG-02 | **Teilweise geschlossen (UX-LOOK-LAYOUT-18):** Rating-Zeile rechts statt links | `crates/lumina-gui/src/develop_left_rail.rs` → `LuminaApp::draw_left_rail_panel`, `LuminaApp::draw_develop_left_rail`, `LuminaApp::draw_snapshots_section`; `crates/lumina-gui/src/develop_ops.rs` → `LuminaApp::draw_presets_section` — Presets/History/Snapshots liegen heute links; **Restbefund: die Rating-Zeile liegt weiter rechts** (`crates/lumina-gui/src/develop_ops.rs` → `LuminaApp::draw_rating_section`) | ja (Layout) | UX-LOOK-LAYOUT-18 · Restbefund: neu |
| UXG-03 | **Teilweise geschlossen:** Kein Import-Dialog | `crates/lumina-gui/src/library_grid.rs` → `LuminaApp::draw_library_grid` (View-Toolbar, Filterleiste, Quick Develop sind vorhanden); `crates/lumina-gui/src/library_tree.rs` → `LuminaApp::draw_folder_tree`; **Restbefund: es gibt keinen Import-Dialog** — `grep -rni "import_dialog\|import_window" crates/lumina-gui/src/` liefert 0 Treffer — `crates/lumina-gui/src/app_frame.rs` → `LuminaApp::apply_import_export_action` schaltet beim Import nur das Modul | ja (G-09-Scope) | neu |
| UXG-04 | **Geschlossen (UX-LOOK-TOOLBAR-18):** Keine Werkzeugleiste über dem Bild (Q/K/M/R nur via Panel) | `crates/lumina-gui/src/app_frame.rs` → `LuminaApp::draw_preview_area`; Werkzeugleiste: `LuminaApp::draw_view_toolbar` (Crop/Heal/Red-Eye/Masken + View-Toggles); Icons: `crates/lumina-gui/src/icon_toolbar.rs` → `ToolbarIcon`, `icon_button` | nein (erledigt) | UX-LOOK-TOOLBAR-18 / LRPAR-G11-OVERLAYS |
| UXG-05 | Panels ohne Kontext-Dynamik (Sections sind kollabierbar, aber nichts blendet je nach Tool/Selektion ein/aus) | `crates/lumina-gui/src/develop_frame.rs` → `LuminaApp::develop_scroll_content` (Section-Aufbau über `LuminaApp::DEVELOP_SECTIONS`); Offen-/Solo-State: `crates/lumina-gui/src/develop_sections.rs` → `LuminaApp::is_section_open`, `LuminaApp::set_section_open`, `LuminaApp::set_solo_mode` | ja (Panel-Dynamik) | neu |
| UXG-06 | **Teilweise geschlossen:** Develop-Reihenfolge ohne Calibration-Panel | `crates/lumina-gui/src/develop_frame.rs` → `LuminaApp::DEVELOP_SECTIONS` (einzige Reihenfolge-Quelle); Reihenfolge-Beleg: `crates/lumina-gui/src/tests/startup.rs` → `develop_section_order_is_lightroom_conform`; **Restbefund: ein Calibration-Panel fehlt weiterhin** (nicht in `LuminaApp::DEVELOP_SECTIONS`) | ja (F-100-Reihenfolge) | neu |
| UXG-07 | **Geschlossen (UX-LOOK-HISTORY-18):** History zeigt Maschinen-IDs statt Namen/Werte/Zeit | `crates/lumina-gui/src/develop_ops.rs` → `LuminaApp::draw_history_section` (Label `Name from → to — Zeit`); strukturiertes Schema: `crates/lumina-sidecar/src/history.rs` → `HistoryChange` | nein (erledigt) | UX-LOOK-HISTORY-18 |
| UXG-08 | **Teilweise geschlossen:** Compare/Survey ohne Bild-Render (nur Thumbnails) | `crates/lumina-gui/src/library_views.rs` → `LuminaApp::draw_library_loupe` (rendert heute das entwickelte Bild), `LuminaApp::draw_library_compare`, `LuminaApp::draw_library_survey`; **Restbefund: Compare/Survey zeigen weiterhin nur Thumbnails** (`crates/lumina-gui/src/filmstrip_frame.rs` → `thumbnail_for_entry`), keinen Bild-Render | ja (G-09-Scope) | neu |
| UXG-09 | **Teilweise geschlossen:** Filmstreifen ohne Filter-/Sort-Leiste | `crates/lumina-gui/src/filmstrip_frame.rs` → `LuminaApp::draw_filmstrip`; Badges und „n von N“-Zähler sind vorhanden (`crates/lumina-gui/src/library_paths.rs` → `paint_entry_badge`, `crates/lumina-gui/src/i18n.rs` → `Str::FilmstripCounter`); **Restbefund: Filter-/Sort-Leiste im Filmstreifen fehlt** | nein | neu |
| UXG-10 | Before/After unvollständig (kein echtes Split); Softproof nur Badge | `crates/lumina-gui/src/lib.rs` → `LuminaApp::toggle_before_after`, `LuminaApp::toggle_split_view`; **Restbefund: kein echtes Split-Render** — der Code-Kommentar zu `LuminaApp::toggle_split_view` führt das Side-by-Side-Split als dokumentierten Follow-up | teils | LRPAR-G10-VIEWER |
| UXG-11 | Keine Menüleiste; Modul-Leiste nicht Classic-artig | `crates/lumina-gui/src/lib.rs` → `LuminaApp::ui` (Top: Brand + Status, **keine** Menüleiste — `grep -rn -e menu_bar -e MenuBar -e draw_menu crates/lumina-gui/src/` liefert 0 Treffer); Modul-Leiste: `crates/lumina-gui/src/app_frame.rs` → `LuminaApp::draw_module_bar` | ja (Menüumfang) | neu |
| UXG-12 | **Teilweise geschlossen:** Presets ohne Amount-Konzept | `crates/lumina-gui/src/develop_ops.rs` → `LuminaApp::draw_presets_section` (Datei-Liste); Baum mit Gruppen: `crates/lumina-gui/src/preset_tree.rs` → `draw_preset_tree`; **Restbefund: kein Amount-Konzept** (`grep -rni amount crates/lumina-gui/src/preset_tree.rs crates/lumina-gui/src/develop_ops.rs` = 0 Treffer) | ja (Amount-Semantik) | neu |
| UXG-13 | Kürzel `O`/`Z`/`Shift+L`/`I` fehlen; `E`-Alias unsauber | `crates/lumina-gui/src/key_bindings.rs` → `module_for_key`, `view_toggle_for_key`, `import_export_for_key` | nein (F-100 erweitern) | LRPAR-G16-POWER |
| UXG-14 | **Teilweise geschlossen:** Keine Canvas-Stepper | `crates/lumina-gui/src/brush_tool.rs` → `LuminaApp::brush_cursor_allowed`; `crates/lumina-gui/src/preview_masks.rs` → `LuminaApp::brush_cursor_radius`; `crates/lumina-gui/src/spot_tool.rs` → `LuminaApp::handle_spot_tool_interaction`; der alte Beleg „Grep `CursorIcon` leer“ ist **überholt** — den Canvas-Cursor setzt heute `crates/lumina-gui/src/develop_geometry/crop_overlay.rs` → `paint_crop_mode_chrome`; die **Reset-Affordanz** gibt es: `crates/lumina-gui/src/develop_frame.rs` → `ui.button(Str::Reset.t())`; **Restbefund: keine Canvas-Stepper** — die `egui::DragValue`-Stepper sitzen in den Panels, nicht auf dem Canvas | nein | neu |
| UXG-15 | Navigator-Rail: **R4-UX-1 (2026-09-20, User)** entfernt die Duplikat-Thumbnail-Rail unter dem Navigator — Navigator-Viewport bleibt, der untere Filmstrip ist die einzige Selektionsfläche; R4-NAV-1 gattert den Viewport-Drag auf vergrößerte Ansichten | `crates/lumina-gui/src/navigator.rs` → `LuminaApp::draw_navigator_viewport` (R4-NAV-1-Drag-Gate), `LuminaApp::draw_navigator`; Filmstrip: `crates/lumina-gui/src/filmstrip_frame.rs` → `LuminaApp::draw_filmstrip` | entschieden | R4-UX-1 |
| UXG-16 | **Geschlossen (UX-LOOK-TONECURVE-18):** Tone Curve ohne Kurvengrafik (P0/P1-Slider + Add-Point statt Punkte setzen/ziehen; Vision 2026-09-17: „bitterste Umlernstelle") | `crates/lumina-gui/src/develop_tone.rs` → `LuminaApp::draw_tone_curve`; Kurvengrafik: `crates/lumina-gui/src/develop_tone/tone_curve_graph.rs` (ersetzt die P0/P1-Sliderreihen); Golden `crates/lumina-gui/tests/snapshots/develop_section_tone_curve.png` | nein (erledigt) | UX-LOOK-TONECURVE-18 |
| UXG-17 | Hardcodierte UI-Literale statt `Str::` (F-100: englisch, 0 deutsche UI-Literale); Generative-Panels 2026-09-17 behoben (`ExpandDragFrameHint`/`ExpandCropToImage`), Spot-Panel (`draw_spot_heal`) offen | behoben: `crates/lumina-gui/src/develop_generative.rs` → `LuminaApp::draw_generative_expand` (heute `Str::ExpandDragFrameHint`/`Str::ExpandCropToImage`); offen: `crates/lumina-gui/src/develop_heal.rs` → `LuminaApp::draw_spot_tool_options` — das alte Sidebar-Panel `draw_spot_heal` **existiert nicht mehr** (R5-DUST-23: Werkzeugleiste über dem Bild); Literal-Tabelle: `crates/lumina-gui/src/i18n.rs` → `Str` | nein | neu |
| UXG-18 | Vision-Feinschliff 2026-09-17 (je einzeln entscheidbar): HSL ohne 8 Color-Mixer-Farbfelder; Compare/Survey-Tiles unausgerichtet; Toast überdeckt Toolbar-Kante; Preset-Dialog unzentriert/beschnitten; Meta-History als Rohdaten-Dump (ISO/Pipe); Histogramm ohne RGB-Kanäle/Clipping-Dreiecke/EXIF-Zeile; kein EXIF-Overlay über dem Bild; Slider-Blau ohne Akzent-Disziplin | Goldens: `crates/lumina-gui/tests/snapshots/` (95 Dateien); Erzeugung/Vergleich: `crates/lumina-gui/tests/kittest_snapshots.rs` → `develop_section_tone_curve` (u. a.) | teils | neu |

## Gap-Inventory Dust Removal (UXD, G-04)

| ID | Lücke | Fundstelle (Pfad + Symbol) | SOLL-Entscheid nötig |
| --- | --- | --- | --- |
| UXD-01 | **Geschlossen (R5-DUST-23):** Spot-Tool armiert Vorschau nicht (Klick/Zug wirkungslos — „kein brush") | `crates/lumina-gui/src/preview_draws.rs` → `LuminaApp::draw_preview`; `crates/lumina-gui/src/preview_masks.rs` → `LuminaApp::handle_mask_tool_drag`; Spot-Pfad: `crates/lumina-gui/src/spot_tool.rs` → `LuminaApp::handle_spot_tool_interaction` (R5-DUST-23) | nein (erledigt) |
| UXD-02 | **Geschlossen (R5-DUST-23):** Kein Brush-Cursor (Kreis = Radius, folgt Maus) | `crates/lumina-gui/src/spot_tool.rs` → `LuminaApp::handle_spot_tool_interaction` (Kreis-Cursor, R5-DUST-23); `crates/lumina-gui/src/brush_tool.rs` → `LuminaApp::brush_cursor_allowed`; `crates/lumina-gui/src/preview_masks.rs` → `LuminaApp::brush_cursor_radius` — der alte Beleg „kein `CursorIcon`“ ist **überholt** | nein (erledigt) |
| UXD-03 | Size/Feather nur Panel-Slider (Rad = Zoom statt Size) | `crates/lumina-gui/src/preview_draws.rs` → `LuminaApp::draw_preview` — das Rad zoomt **nur bei gehaltener Ctrl/Cmd-Taste** (`crates/lumina-gui/src/app_frame.rs` → `LuminaApp::wants_wheel_zoom`), sonst schiebt es das Bild; Size/Feather: `crates/lumina-gui/src/develop_heal.rs` → `LuminaApp::draw_spot_tool_options`; Tastatur: `crates/lumina-gui/src/spot_tool.rs` → `LuminaApp::handle_spot_size_shortcuts` | klein (Shift-Feather präzisieren) |
| UXD-04 | Pins: einzelner Fixpunkt, kein Target/Source-Paar, kein Drag/Delete/`/` | `crates/lumina-gui/src/overlay_visibility.rs` → `LuminaApp::visible_edit_pins`; Maler: `crates/lumina-gui/src/preview_masks.rs` → `LuminaApp::draw_edit_pins`; Pin-Typen: `crates/lumina-gui/src/edit_pins.rs` → `EditPinKind` (nur `Mask`/`Spot`, **kein** Target/Source-Paar) | **ja** (Pin-Semantik) |
| UXD-05 | **Geschlossen (R5-DUST-23):** Panel statisch/überladen statt kontextuell | das Sidebar-Panel `draw_spot_heal` **existiert nicht mehr** (R5-DUST-23); Nachfolger: `crates/lumina-gui/src/develop_heal.rs` → `LuminaApp::draw_spot_tool_options` — er wird nur gezeichnet, wenn das Heal-Werkzeug aktiv ist (`crates/lumina-gui/src/app_frame.rs` → `LuminaApp::draw_preview_area`) | nein (erledigt) |
| UXD-06 | Visualize = Dauer-Tint statt werkzeuggekoppelter High-Contrast-Ansicht | `crates/lumina-gui/src/render_pipeline.rs` → `LuminaApp::render_from`; Schwelle: `crates/lumina-gui/src/lib.rs` → `LuminaApp::spot_visualize_overlay_threshold` | teils |
| UXD-07 | Kein Quell-Pin/Alt-Ziehen (Offset unsichtbar) | `crates/lumina-gui/src/spot_select.rs` → `LuminaApp::commit_spot_heal`; Offset: `crates/lumina-gui/src/spot_tool.rs` → `LuminaApp::spot_auto_offset` | nein (SOLL deckt ab) |
| UXD-08 | **Geschlossen (R5-DUST-23):** Kein Einzel-Klick-Anlegen (nur Detect→Apply/CLI) | das alte Panel `draw_spot_heal` existiert nicht mehr; das Anlegen läuft heute über `crates/lumina-gui/src/spot_tool.rs` → `LuminaApp::spot_dab` / `LuminaApp::handle_spot_tool_interaction` (je Klick ein Dab, R5-DUST-23) | nein (erledigt) |

Direkt umsetzbar ohne Doc-Entscheid: **UXD-07**. UXD-01, -02, -05 und -08
sind geschlossen (R5-DUST-23; Beleg in der Spalte „Fundstelle"). UXD-03 braucht
noch eine (kleine) Doc-Entscheidung — siehe seine SOLL-Spalte.

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
  Survey zeigen denselben zentrierten Empty-State (Icon + Titel + Body + CTA).
  Der untere Filmstreifen zeigt bei 0 Einträgen den ehrlichen Hinweis
  „No images in this folder“ statt „Click a thumbnail to open it“. **R4-UX-1
  (2026-09-20, User):** Die frühere Navigator-Rail (zweite Bildleiste links)
  ist entfernt — der Filmstreifen ist die einzige Selektionsfläche.
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

DoD §6 Vision-Review der geänderten Layouts: alle neuen/geänderten Snapshots
legt der Build-Agent **dem konfigurierten Modell selbst** vor (max. 10 Bilder
je Durchgang) + kittest-Goldens (neu/geändert) + Headless-Asserts für
Tool-Zustände; manueller Abgleich gegen die `.goal`-Screenshots;
unabhängige Verifizierung (BESTANDEN) vor Commit.

## Vision-Befunde UX-LOOK-LAYOUT-18 (Vision-Loop 2026-09-19, Build-Agent-sichtig)

Nachgelagerter DoD-§6.2-Loop über 5 Alt/Neu-Golden-Paare (Commit d34ba0a),
vom Build-Agenten am Bild gegengeprüft:

- **LAYOUT-V1 (mittel, BEHOBEN 2026-09-19, verifiziert BESTANDEN):** Footer-Überlappung in
  schmalen Zuständen — „Regenerate Stale / Missing" und „Render / Apply"
  überlagerten sich zu unlesbarem „…Missing der / Apply".
  Ursache: `Layout::bottom_up` + `horizontal_wrapped` brach nach unten um.
  Fix: echter `egui::Panel::bottom`-Footer (top-down, gleiche Buttons/
  Reihenfolge/Handler), gepinnt per `develop_footer_buttons_never_overlap_in_narrow_panels`
  (320/220 px, Disjunktheit; Bug ohne Fix reproduziert). 28 Develop-Goldens
  rebaselined (Diffs nur im rechten Panel).
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

## Umsetzungsstand UX-LOOK-TONECURVE-18 (2026-09-19)

SOLL-Entscheid oben umgesetzt (Implementierung + headless/kittest-Goldens,
Commit nach unabhängiger Verifizierung BESTANDEN):

- **Echte Kurvengrafik pro Kanal** (`src/develop_tone/tone_curve_graph.rs`,
  P0/P1-Slider-Editor in `src/develop_tone.rs` ersetzt): Klick auf die Kurve
  setzt einen Punkt (Kurvennähe 8 pt, sonst No-op), Drag zieht (Input-Clamp
  zwischen Nachbarn, Output 0..=1), Doppelklick löscht Innenpunkte; Endpunkte
  `(0,0)`/`(1,1)` weder zieh- noch löschbar (laute Verweigerung, Status +
  `warn!`, kein Save). Kanalwahl Master/R/G/B unverändert.
- **Spline = PCHIP-Spiegel von `lumina_core::monotone_curve`**
  (Anzeige-Sampler nur für Grafik/Hit-Testing; Rendering bleibt Core bzw.
  GPU-`apply_curves_stage`, unberührt — keine neue CPU-Route).
- **Persistenz unverändert** über bestehenden `curves`-Rezeptblock (kein
  Schema-Bruch, keine Migration); Reload-Roundtrip getestet.
- **Tests:** 7 headless Tests (`src/tests/tone_curve_graph.rs`:
  Gesten-Instrumentierung, Zustand + Sidecar-Reload, pro-Kanal-Unabhängigkeit,
  Clamp/laute Verweigerung, Paint, 256-Rampen-Parität gegen `render_frame`).
- **Namen-Vorbehalt:** keine neuen sichtbaren Strings, `i18n.rs` unverändert
  (ungenutzte `Str::ToneCurveAddPoint/RemovePoint/PointInput/PointOutput`
  unter `allow(dead_code)`).
- **Golden:** `develop_section_tone_curve.png` rebaselined (Diff nur rechtes
  Panel, Canvas links byte-identisch).

## Umsetzungsstand UX-LOOK-CROP-18 (2026-09-19)

SOLL-Entscheid oben umgesetzt (Implementierung + headless/kittest-Goldens,
unabhängig verifiziert BESTANDEN):

- **Interaktives Crop-Overlay** (`src/develop_geometry/crop_overlay.rs`):
  4 Eck-Handles (Resize mit gegenüberliegendem Anker, Mindestgröße 0.02),
  Move innen, Bild-Clamp, Drittel-Gitter, Abdunklung außerhalb.
- **Draft-Trennung:** Gezogener Rahmen ist session-only (`egui`-Temp-Memory).
  „Live-Vorschau" (SOLL) = Overlay (Rahmen/Gitter/Abdunklung) aktualisiert
  live; die gerenderten Bildpixel werden erst bei Commit neu gerendert, da
  das Rezept erst bei Commit geschrieben wird. Kein Pixel-Live-Reframing
  (bewusst keine Architekturänderung).
- **Enter/Esc:** Enter committet über `set_crop_free` (genau ein
  History-Step + debounced Write); Esc verwirft (Rezept bitgleich, kein
  Sidecar); R-aus mit Draft verwirft laut (`warn!`).
- **Aspect→Free:** Ein interaktiver Commit schreibt dokumentiert ein
  `Crop::Free` (Schema kennt keine größen-erhaltenden Aspect-Commits) —
  gepinnt per Test, kein stiller Verlust. Keine neuen Rezeptfelder, kein
  Schema-Bruch.
- **Tests:** 10 neue headless Tests (Draft-Trennung, alle Ecken, Move/Clamp,
  Enter→Persistenz+Reload, Esc, Paint, Aspect→Free, R-off, Fall-through) +
  kittest-Golden `develop_overlay_crop_interactive.png` (Handles+Gitter+
  Abdunklung, Pixel-Guard).
- **Namen-Vorbehalt:** `i18n.rs` unverändert, keine neuen Wortlaute.

## SOLL-Entscheid + Umsetzungsstand UX-LOOK-CROP-18b (2026-09-19, Runde-2-Befund)

User-Befund: Nach Commit kein Vergrößern möglich (Referenz schrumpfte mit);
Rotation/Auto-Level fehlt im Tool. Entscheide (Build-Agent, Runde-2-Fix):

- **Vollbild-Referenz:** Crop-Session arbeitet immer auf Vollbild-Koordinaten
  (Display-Rezept ohne Geometry-Stage NUR im Crop-Modus, Normalpfad ohne
  Mehrkosten); Draft-Init aus Commit; Vergrößern bis Vollbild; Commit ersetzt.
- **Rotation im Tool per Regler** (kein On-Canvas-Drag): Straighten-Slider im
  Crop-Balken. **R5-FIX-WELLE-20 (2026-09-20):** Der Slider committet den
  Winkel **live** über `set_straighten` (`geometry.rotation_degrees`, `info!`,
  Debounce-Save) — der frühere reine Session-Draft war wirkungs- und loglos
  (R5-STRAIGHTEN-1). Rotation NICHT live im Modus (Core-Ordnung crop→rotate
  verbietet live-rotierte Fläche mit Quell-Rechteck) — dokumentierte Grenze,
  Wirkung nach Verlassen des Tools sichtbar.
- **Esc / Commit-Abgrenzung (R5-FIX-WELLE-20, normativ):** `Esc` (und
  Verlassen des Crop-Tools ohne aktiven Werkzeugwechsel) verwirft **nur den
  Rechteck-Draft**; ein bereits per Slider committeter Straighten-Winkel
  **bleibt** im Rezept (er ist ein echter Edit wie der Panel-Regler). `Enter`
  committet Rechteck + einen noch offenen Rotations-Draft (z. B. aus
  Auto-Level) als einen History-Step. Ein Werkzeugwechsel committet den
  Rechteck-Draft (R5-TOOLFLOW-1, siehe `cli-gui-wasm.md`).
- **Auto-Level-Button:** nutzt `upright`-Analyse unverändert, Konfidenz-Gate
  0.10; low → laut (`warn!` + Status), kein Save; high → Analyse mit
  Fingerprint als DEAKTIVIERTE Stage persistiert (Evidenz, kein Doppel-Apply,
  bestehende Stage nie überschrieben).
- Verlustfrei: nur Rezept; `Esc` verwirft den Rechteck-Draft bitgleich, ein
  slider-committeter Winkel bleibt. Namen-Vorbehalt gilt (Bestands-`Str`).

Umsetzung verifiziert BESTANDEN 2026-09-19 (13 Tests inkl. echter Balken-Bedienung,
Guard-Test, Goldens; Verifizierung + Re-Verifizierung).

## Umsetzungsstand UX-LOOK-HISTORY-18 (2026-09-19)

SOLL-Entscheid oben umgesetzt (Implementierung + Tests + Golden,
unabhängig verifiziert BESTANDEN):

- **Strukturierte History-Einträge (additiv, kein Schema-Bruch):** neuer
  typisierter `HistoryChange { parameter, from, to }` (`deny_unknown_fields`)
  pro Eintrag unter Extras-Schlüssel `"changes"` (`lumina-sidecar/src/history.rs`,
  `HistoryEntry` verlustfrei dorthin gezogen). Kein `schema_version`-Bump,
  keine Migration nötig.
- **Laute Ablehnung:** falscher Typ/fehlende/unbekannte Felder/leeres
  `parameter`/Steuerzeichen/Überlänge/Anzahl → `SidecarError::Invalid`;
  fehlendes `changes` = Legacy (leere Liste, nichts erfunden). Keine stille
  Normalisierung.
- **Lesbar + klickbar:** Label `Name from → to — Zeit` (Arbeitslabel,
  Namen-Vorbehalt), Klick-Restore mit Wirkung; Zeit via `recorded_at`
  (session-only Override für deterministische Goldens, nicht persistiert).
- **Presets-Baum:** Gruppierung nach relativem Unterordner (nie absolut),
  verschachtelte Header, kaputte Dateien als Fehlerknoten, Tiefenlimit 16;
  Klick = bestehender `apply_preset`-Pfad.
- **Tests:** 6 Sidecar (Roundtrip, Legacy, Ablehnung, `set_changes`,
  Revalidierung) + 4 GUI (Label+Restore, Legacy, Baum+Klick, Batch-Persistenz);
  Golden `develop_section_history.png` rebaselined.
- **Bekannte Grenzen (niedrig, Folgearbeit):** Überlängen-/Anzahl-Limits und
  Baum-Tiefenlimit ohne Negativtest; `deleted_virtual_copies` validieren
  `history` nicht (keine Wirkung, Doku vs. Verhalten); `recipe_changes`
  kappt theoretisch Unerreichbares still; Ablehnungs-Asserts teils nur
  `.is_err()`.
