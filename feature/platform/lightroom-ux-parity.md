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

**Methodenvermerk (Stand 2026-10-21):** die Statusangaben — „geschlossen",
„teilweise geschlossen" und die in der Spalte „Fundstelle" genannten
Restbefunde („Restbefund:", „behoben:"/"offen:") — sind **per
Code-Lesung** belegt — Quelldatei + Symbolname, ggf. der schließende Commit
im Doc-Kommentar des Moduls — und **zusätzlich per Testlauf** bestätigt: je
Zeile der eine entscheidende Test, der die Aussage der Zeile trägt, im
Abschnitt „Testbeleg je Zeile (Lauf 2026-10-21)". Die Restbefunde der
„teilweise geschlossen"-Zeilen sind **offene SOLL-Fragen** (Abschnitt
„Offene SOLL-Entscheide"); die Produktentscheidung dazu liegt beim Owner.

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
| UXG-01 | **Geschlossen (UX-LOOK-CROP-18):** Crop nicht interaktiv (nur Badge + x/y/w/h-Felder statt Rahmen/Handles/Drag/Enter) | `crates/lumina-gui/src/develop_geometry/crop_overlay.rs` → `LuminaApp::draw_crop_overlay` (vier Eck-Handles, Drag, Drittel-Gitter, `Enter`/`Esc`); `crates/lumina-gui/src/develop_geometry.rs` → `LuminaApp::draw_geometry` | nein (erledigt) — UX-LOOK-CROP-18 | UX-LOOK-CROP-18 · Commit `eb13ba6` |
| UXG-02 | **Teilweise geschlossen (UX-LOOK-LAYOUT-18):** Rating-Zeile rechts statt links | `crates/lumina-gui/src/develop_left_rail.rs` → `LuminaApp::draw_left_rail_panel`, `LuminaApp::draw_develop_left_rail`, `LuminaApp::draw_snapshots_section`; `crates/lumina-gui/src/develop_ops.rs` → `LuminaApp::draw_presets_section` — Presets/History/Snapshots liegen heute links; **Restbefund: die Rating-Zeile liegt weiter rechts** (`crates/lumina-gui/src/develop_ops.rs` → `LuminaApp::draw_rating_section`) | ja (Layout) — siehe „Offene SOLL-Entscheide" | UX-LOOK-LAYOUT-18 · Restbefund: neu |
| UXG-03 | **Teilweise geschlossen:** Kein Import-Dialog | `crates/lumina-gui/src/library_grid.rs` → `LuminaApp::draw_library_grid` (View-Toolbar, Filterleiste, Quick Develop sind vorhanden); `crates/lumina-gui/src/library_tree.rs` → `LuminaApp::draw_folder_tree`; **Restbefund: es gibt keinen Import-Dialog** — `grep -rni "import_dialog\|import_window" crates/lumina-gui/src/` liefert 0 Treffer — `crates/lumina-gui/src/app_frame.rs` → `LuminaApp::apply_import_export_action` schaltet beim Import nur das Modul | ja (G-09-Scope) — siehe „Offene SOLL-Entscheide" | neu |
| UXG-04 | **Geschlossen (UX-LOOK-TOOLBAR-18):** Keine Werkzeugleiste über dem Bild (Q/K/M/R nur via Panel) | `crates/lumina-gui/src/app_frame.rs` → `LuminaApp::draw_preview_area`; Werkzeugleiste: `LuminaApp::draw_view_toolbar` (Crop/Heal/Red-Eye/Masken + View-Toggles); Icons: `crates/lumina-gui/src/icon_toolbar.rs` → `ToolbarIcon`, `icon_button` | nein (erledigt) — UX-LOOK-TOOLBAR-18 | UX-LOOK-TOOLBAR-18 / LRPAR-G11-OVERLAYS · Commit `ca2488d` |
| UXG-05 | Panels ohne Kontext-Dynamik (Sections sind kollabierbar, aber nichts blendet je nach Tool/Selektion ein/aus) | `crates/lumina-gui/src/develop_frame.rs` → `LuminaApp::develop_scroll_content` (Section-Aufbau über `LuminaApp::DEVELOP_SECTIONS`); Offen-/Solo-State: `crates/lumina-gui/src/develop_sections.rs` → `LuminaApp::is_section_open`, `LuminaApp::set_section_open`, `LuminaApp::set_solo_mode` | ja (Panel-Dynamik) | neu |
| UXG-06 | **Teilweise geschlossen:** Develop-Reihenfolge ohne Calibration-Panel | `crates/lumina-gui/src/develop_frame.rs` → `LuminaApp::DEVELOP_SECTIONS` (einzige Reihenfolge-Quelle); Reihenfolge-Beleg: `crates/lumina-gui/src/tests/startup.rs` → `develop_section_order_is_lightroom_conform`; **Restbefund: ein Calibration-Panel fehlt weiterhin** (nicht in `LuminaApp::DEVELOP_SECTIONS`) | ja (F-100-Reihenfolge) — siehe „Offene SOLL-Entscheide" | neu |
| UXG-07 | **Geschlossen (UX-LOOK-HISTORY-18):** History zeigt Maschinen-IDs statt Namen/Werte/Zeit | `crates/lumina-gui/src/develop_ops.rs` → `LuminaApp::draw_history_section` (Label `Name from → to — Zeit`); strukturiertes Schema: `crates/lumina-sidecar/src/history.rs` → `HistoryChange` | nein (erledigt) — UX-LOOK-HISTORY-18 | UX-LOOK-HISTORY-18 · Commit `f5e3ef7` |
| UXG-08 | **Teilweise geschlossen:** Compare/Survey ohne Bild-Render (nur Thumbnails) | `crates/lumina-gui/src/library_views.rs` → `LuminaApp::draw_library_loupe` (rendert heute das entwickelte Bild), `LuminaApp::draw_library_compare`, `LuminaApp::draw_library_survey`; **Restbefund: Compare/Survey zeigen weiterhin nur Thumbnails** (`crates/lumina-gui/src/filmstrip_frame.rs` → `thumbnail_for_entry`), keinen Bild-Render | ja (G-09-Scope) — siehe „Offene SOLL-Entscheide" | neu |
| UXG-09 | **Teilweise geschlossen:** Filmstreifen ohne Filter-/Sort-Leiste | `crates/lumina-gui/src/filmstrip_frame.rs` → `LuminaApp::draw_filmstrip`; Badges und „n von N“-Zähler sind vorhanden (`crates/lumina-gui/src/library_paths.rs` → `paint_entry_badge`, `crates/lumina-gui/src/i18n.rs` → `Str::FilmstripCounter`); **Restbefund: Filter-/Sort-Leiste im Filmstreifen fehlt** | ja (Filter-/Sort-Leiste im Filmstreifen) — siehe „Offene SOLL-Entscheide" | neu |
| UXG-10 | Before/After unvollständig (kein echtes Split); Softproof nur Badge | `crates/lumina-gui/src/lib.rs` → `LuminaApp::toggle_before_after`, `LuminaApp::toggle_split_view`; **Restbefund: kein echtes Split-Render** — der Code-Kommentar zu `LuminaApp::toggle_split_view` führt das Side-by-Side-Split als dokumentierten Follow-up | teils | LRPAR-G10-VIEWER |
| UXG-11 | Keine Menüleiste; Modul-Leiste nicht Classic-artig | `crates/lumina-gui/src/lib.rs` → `LuminaApp::ui` (Top: Brand + Status, **keine** Menüleiste — `grep -rn -e menu_bar -e MenuBar -e draw_menu crates/lumina-gui/src/` liefert 0 Treffer); Modul-Leiste: `crates/lumina-gui/src/app_frame.rs` → `LuminaApp::draw_module_bar` | ja (Menüumfang) | neu |
| UXG-12 | **Teilweise geschlossen:** Presets ohne Amount-Konzept | `crates/lumina-gui/src/develop_ops.rs` → `LuminaApp::draw_presets_section` (Datei-Liste); Baum mit Gruppen: `crates/lumina-gui/src/preset_tree.rs` → `draw_preset_tree`; **Restbefund: kein Amount-Konzept** (`grep -rni amount crates/lumina-gui/src/preset_tree.rs crates/lumina-gui/src/develop_ops.rs` = 0 Treffer) | ja (Amount-Semantik) — siehe „Offene SOLL-Entscheide" | neu |
| UXG-13 | Kürzel `O`/`Z`/`Shift+L`/`I` fehlen; `E`-Alias unsauber | `crates/lumina-gui/src/key_bindings.rs` → `module_for_key`, `view_toggle_for_key`, `import_export_for_key` | nein (F-100 erweitern) | LRPAR-G16-POWER |
| UXG-14 | **Teilweise geschlossen:** Keine Canvas-Stepper | `crates/lumina-gui/src/brush_tool.rs` → `LuminaApp::brush_cursor_allowed`; `crates/lumina-gui/src/preview_masks.rs` → `LuminaApp::brush_cursor_radius`; `crates/lumina-gui/src/spot_tool.rs` → `LuminaApp::handle_spot_tool_interaction`; der alte Beleg „Grep `CursorIcon` leer“ ist **überholt** — den Canvas-Cursor setzt heute `crates/lumina-gui/src/develop_geometry/crop_overlay.rs` → `paint_crop_mode_chrome`; die **Reset-Affordanz** gibt es: `crates/lumina-gui/src/develop_frame.rs` → `ui.button(Str::Reset.t())`; **Restbefund: keine Canvas-Stepper** — die `egui::DragValue`-Stepper sitzen in den Panels, nicht auf dem Canvas | ja (Canvas-Stepper) — siehe „Offene SOLL-Entscheide" | neu |
| UXG-15 | Navigator-Rail: **R4-UX-1 (2026-09-20, User)** entfernt die Duplikat-Thumbnail-Rail unter dem Navigator — Navigator-Viewport bleibt, der untere Filmstrip ist die einzige Selektionsfläche; R4-NAV-1 gattert den Viewport-Drag auf vergrößerte Ansichten | `crates/lumina-gui/src/navigator.rs` → `LuminaApp::draw_navigator_viewport` (R4-NAV-1-Drag-Gate), `LuminaApp::draw_navigator`; Filmstrip: `crates/lumina-gui/src/filmstrip_frame.rs` → `LuminaApp::draw_filmstrip` | entschieden | R4-UX-1 |
| UXG-16 | **Geschlossen (UX-LOOK-TONECURVE-18):** Tone Curve ohne Kurvengrafik (P0/P1-Slider + Add-Point statt Punkte setzen/ziehen; Vision 2026-09-17: „bitterste Umlernstelle") | `crates/lumina-gui/src/develop_tone.rs` → `LuminaApp::draw_tone_curve`; Kurvengrafik: `crates/lumina-gui/src/develop_tone/tone_curve_graph.rs` (ersetzt die P0/P1-Sliderreihen); Golden `crates/lumina-gui/tests/snapshots/develop_section_tone_curve.png` | nein (erledigt) — UX-LOOK-TONECURVE-18 | UX-LOOK-TONECURVE-18 · Commit `6a409f5` |
| UXG-17 | Hardcodierte UI-Literale statt `Str::` (F-100: englisch, 0 deutsche UI-Literale); Generative-Panels 2026-09-17 behoben (`ExpandDragFrameHint`/`ExpandCropToImage`), Spot-Panel (`draw_spot_heal`) offen | behoben: `crates/lumina-gui/src/develop_generative.rs` → `LuminaApp::draw_generative_expand` (heute `Str::ExpandDragFrameHint`/`Str::ExpandCropToImage`); offen: `crates/lumina-gui/src/develop_heal.rs` → `LuminaApp::draw_spot_tool_options` — das alte Sidebar-Panel `draw_spot_heal` **existiert nicht mehr** (R5-DUST-23: Werkzeugleiste über dem Bild); Literal-Tabelle: `crates/lumina-gui/src/i18n.rs` → `Str` | nein | neu |
| UXG-18 | Vision-Feinschliff 2026-09-17 (je einzeln entscheidbar): HSL ohne 8 Color-Mixer-Farbfelder; Compare/Survey-Tiles unausgerichtet; Toast überdeckt Toolbar-Kante; Preset-Dialog unzentriert/beschnitten; Meta-History als Rohdaten-Dump (ISO/Pipe); Histogramm ohne RGB-Kanäle/Clipping-Dreiecke/EXIF-Zeile; kein EXIF-Overlay über dem Bild; Slider-Blau ohne Akzent-Disziplin | Goldens: `crates/lumina-gui/tests/snapshots/` (95 Dateien); Erzeugung/Vergleich: `crates/lumina-gui/tests/kittest_snapshots.rs` → `develop_section_tone_curve` (u. a.) | teils | neu |

## Gap-Inventory Dust Removal (UXD, G-04)

| ID | Lücke | Fundstelle (Pfad + Symbol) | SOLL-Entscheid nötig | Task-Anker |
| --- | --- | --- | --- | --- |
| UXD-01 | **Geschlossen (R5-DUST-23):** Spot-Tool armiert Vorschau nicht (Klick/Zug wirkungslos — „kein brush") | `crates/lumina-gui/src/preview_draws.rs` → `LuminaApp::draw_preview`; `crates/lumina-gui/src/preview_masks.rs` → `LuminaApp::handle_mask_tool_drag`; Spot-Pfad: `crates/lumina-gui/src/spot_tool.rs` → `LuminaApp::handle_spot_tool_interaction` (R5-DUST-23) | nein (erledigt) — R5-DUST-23 | R5-DUST-23 · Commit `5ba13da` |
| UXD-02 | **Geschlossen (R5-DUST-23):** Kein Brush-Cursor (Kreis = Radius, folgt Maus) | `crates/lumina-gui/src/spot_tool.rs` → `LuminaApp::handle_spot_tool_interaction` (Kreis-Cursor, R5-DUST-23); `crates/lumina-gui/src/brush_tool.rs` → `LuminaApp::brush_cursor_allowed`; `crates/lumina-gui/src/preview_masks.rs` → `LuminaApp::brush_cursor_radius` — der alte Beleg „kein `CursorIcon`“ ist **überholt** | nein (erledigt) — R5-DUST-23 | R5-DUST-23 · Commit `5ba13da` |
| UXD-03 | Size/Feather nur Panel-Slider (Rad = Zoom statt Size) | `crates/lumina-gui/src/preview_draws.rs` → `LuminaApp::draw_preview` — das Rad zoomt **nur bei gehaltener Ctrl/Cmd-Taste** (`crates/lumina-gui/src/app_frame.rs` → `LuminaApp::wants_wheel_zoom`), sonst schiebt es das Bild; Size/Feather: `crates/lumina-gui/src/develop_heal.rs` → `LuminaApp::draw_spot_tool_options`; Tastatur: `crates/lumina-gui/src/spot_tool.rs` → `LuminaApp::handle_spot_size_shortcuts` | klein (Shift-Feather präzisieren) | — |
| UXD-04 | Pins: einzelner Fixpunkt, kein Target/Source-Paar, kein Drag/Delete/`/` | `crates/lumina-gui/src/overlay_visibility.rs` → `LuminaApp::visible_edit_pins`; Maler: `crates/lumina-gui/src/preview_masks.rs` → `LuminaApp::draw_edit_pins`; Pin-Typen: `crates/lumina-gui/src/edit_pins.rs` → `EditPinKind` (nur `Mask`/`Spot`, **kein** Target/Source-Paar) | **ja** (Pin-Semantik) | — |
| UXD-05 | **Geschlossen (R5-DUST-23):** Panel statisch/überladen statt kontextuell | das Sidebar-Panel `draw_spot_heal` **existiert nicht mehr** (R5-DUST-23); Nachfolger: `crates/lumina-gui/src/develop_heal.rs` → `LuminaApp::draw_spot_tool_options` — er wird nur gezeichnet, wenn das Heal-Werkzeug aktiv ist (`crates/lumina-gui/src/app_frame.rs` → `LuminaApp::draw_preview_area`) | nein (erledigt) — R5-DUST-23 | R5-DUST-23 · Commit `5ba13da` |
| UXD-06 | Visualize = Dauer-Tint statt werkzeuggekoppelter High-Contrast-Ansicht | `crates/lumina-gui/src/render_pipeline.rs` → `LuminaApp::render_from`; Schwelle: `crates/lumina-gui/src/lib.rs` → `LuminaApp::spot_visualize_overlay_threshold` | teils | — |
| UXD-07 | Kein Quell-Pin/Alt-Ziehen (Offset unsichtbar) | `crates/lumina-gui/src/spot_select.rs` → `LuminaApp::commit_spot_heal`; Offset: `crates/lumina-gui/src/spot_tool.rs` → `LuminaApp::spot_auto_offset` | nein (SOLL deckt ab) | — |
| UXD-08 | **Geschlossen (R5-DUST-23):** Kein Einzel-Klick-Anlegen (nur Detect→Apply/CLI) | das alte Panel `draw_spot_heal` existiert nicht mehr; das Anlegen läuft heute über `crates/lumina-gui/src/spot_tool.rs` → `LuminaApp::spot_dab` / `LuminaApp::handle_spot_tool_interaction` (je Klick ein Dab, R5-DUST-23) | nein (erledigt) — R5-DUST-23 | R5-DUST-23 · Commit `5ba13da` |

Direkt umsetzbar ohne Doc-Entscheid: **UXD-07**. UXD-01, -02, -05 und -08
sind geschlossen (R5-DUST-23; Beleg in der Spalte „Fundstelle"). UXD-03 braucht
noch eine (kleine) Doc-Entscheidung — siehe seine SOLL-Spalte. Der Lauf-Beleg
je Zeile steht im Abschnitt „Testbeleg je Zeile (Lauf 2026-10-21)".

## Testbeleg je Zeile (Lauf 2026-10-21)

Der Einstufungs-Lauf für die 15 Zeilen mit Statusangabe (8 „geschlossen",
7 „teilweise geschlossen") ist erfolgt: `cargo test -p lumina-gui --lib`
(Default-Feature-Set = CI-Feature-Set) →
`test result: ok. 950 passed; 0 failed; 5 ignored; 0 measured; 0 filtered out; finished in 59.87s`.
Die 5 ignorierten Tests sind
`dropped_files::tests::dropped_raw_path_preserves_orientation_metadata_and_identity`,
`matrix::tests::real_matrix_headless`,
`tests::brush_management::mask_management_controls_have_a_representative_kittest_golden`,
`tests::gpu_audit::gpu_action_routing_audit_metal` und
`tests::gpu_audit::gpu_audit_lensfun_corrector_presents_gpu_without_badge` —
keiner gehört zu einer der Zeilen dieser Tabelle; die Einstufung jeder Zeile
ist damit durch einen gelaufenen, grünen Test gedeckt. Die acht Wächter für die
**Restbefunde** und UXD-05 stehen in der Tabelle unten; sie pinnen den jeweils
beobachteten Zustand und ersetzen keine offene Produktentscheidung.

Je Zeile trägt **ein** Test die Aussage (der Test, der bei Rückkehr des
Befunds rot wird). „Kommando" ist das Nachprüf-Kommando (ein Modul-Filter auf
denselben Gesamtlauf), „Ergebnis" die Zahl der gelaufenen Tests dieses
Filters.

| ID | entscheidender Test | Kommando | Ergebnis |
| --- | --- | --- | --- |
| UXG-01 | `tests::geometry::crop_overlay::crop_enter_commits_and_persists` | `cargo test -p lumina-gui --lib tests::geometry::crop_overlay` | ok — 10/10 in `tests::geometry::crop_overlay` |
| UXG-02 | `tests::layout::develop_left_rail_paints_navigator_presets_snapshots_history` | `cargo test -p lumina-gui --lib tests::layout` | ok — 5/5 in `tests::layout` |
| UXG-03 | `tests::g16_shortcuts::w3_shortcut_mappings_are_exact` | `cargo test -p lumina-gui --lib tests::g16_shortcuts` | ok — 7/7 in `tests::g16_shortcuts` |
| UXG-04 | `tests::toolbar_icons::toolbar_paints_all_interactive_tools` | `cargo test -p lumina-gui --lib tests::toolbar_icons` | ok — 7/7 in `tests::toolbar_icons` |
| UXG-06 | `tests::startup::develop_section_order_is_lightroom_conform` | `cargo test -p lumina-gui --lib tests::startup::` | ok — 14/14 in `tests::startup` |
| UXG-07 | `tests::history_presets_look::history_entry_is_readable_and_restores_on_click` | `cargo test -p lumina-gui --lib tests::history_presets_look` | ok — 5/5 in `tests::history_presets_look` |
| UXG-08 | `tests::library_views::r5_loupe::loupe_paints_the_developed_render_not_the_thumbnail` | `cargo test -p lumina-gui --lib tests::library_views` | ok — 20/20 in `tests::library_views` |
| UXG-09 | `tests::badges::entry_badge_text_covers_rating_flag_label` | `cargo test -p lumina-gui --lib tests::badges` | ok — 6/6 in `tests::badges` |
| UXG-12 | `tests::history_presets_look::preset_tree_groups_and_applies` | `cargo test -p lumina-gui --lib tests::history_presets_look` | ok — 5/5 in `tests::history_presets_look` |
| UXG-14 | `tests::layout::develop_section_previous_and_reset_buttons_change_recipe` | `cargo test -p lumina-gui --lib tests::layout` | ok — 5/5 in `tests::layout` |
| UXG-16 | `tests::tone_curve_graph::graph_set_drag_delete_persists_and_reloads` | `cargo test -p lumina-gui --lib tests::tone_curve_graph` | ok — 10/10 in `tests::tone_curve_graph` |
| UXD-01 | `tests::spot_tool::spot_dab_persists_heals_and_reloads` | `cargo test -p lumina-gui --lib tests::spot_tool` | ok — 7/7 in `tests::spot_tool` |
| UXD-02 | `tests::brush_interaction::brush_cursor_paints_at_live_radius_and_is_gated_by_a_competing_picker` | `cargo test -p lumina-gui --lib tests::brush_interaction` | ok — 7/7 in `tests::brush_interaction` |
| UXD-05 | `tests::spot_followup_ui::followup_panel_shows_detail_only_when_selected` | `cargo test -p lumina-gui --lib tests::spot_followup_ui` | ok — 4/4 in `tests::spot_followup_ui` |
| UXD-08 | `tests::spot_followup_ui::followup_pin_click_selects_instead_of_dabbing` | `cargo test -p lumina-gui --lib tests::spot_followup_ui` | ok — 4/4 in `tests::spot_followup_ui` |
| UXG-02 Restbefund | `tests::support::ux_gaps::uxg_develop_rating_row_paints_in_the_right_panel` | `cargo test -p lumina-gui --lib -- --test-threads=1` | ok — 962 passed, 5 ignored; 8 UX-Gap-Wächter enthalten |
| UXG-03 Restbefund | `tests::support::ux_gaps::uxg_import_opens_a_dialog_instead_of_only_switching_the_module` | `cargo test -p lumina-gui --lib -- --test-threads=1` | ok — 962 passed, 5 ignored; 8 UX-Gap-Wächter enthalten |
| UXG-08 Restbefund | `tests::support::ux_gaps::uxg_compare_and_survey_paint_the_full_render` | `cargo test -p lumina-gui --lib -- --test-threads=1` | ok — 962 passed, 5 ignored; 8 UX-Gap-Wächter enthalten |
| UXG-09 Restbefund | `tests::support::ux_gaps::uxg_filmstrip_has_a_filter_and_sort_bar` | `cargo test -p lumina-gui --lib -- --test-threads=1` | ok — 962 passed, 5 ignored; 8 UX-Gap-Wächter enthalten |
| UXG-09 Counter | `tests::support::ux_gaps::uxg_filmstrip_counter_paints_n_of_n` | `cargo test -p lumina-gui --lib -- --test-threads=1` | ok — 962 passed, 5 ignored; 8 UX-Gap-Wächter enthalten |
| UXG-12 Restbefund | `tests::support::ux_gaps::uxg_presets_expose_an_amount_control` | `cargo test -p lumina-gui --lib -- --test-threads=1` | ok — 962 passed, 5 ignored; 8 UX-Gap-Wächter enthalten |
| UXG-14 Restbefund | `tests::support::ux_gaps::uxg_canvas_paints_size_and_feather_steppers` | `cargo test -p lumina-gui --lib -- --test-threads=1` | ok — 962 passed, 5 ignored; 8 UX-Gap-Wächter enthalten |
| UXD-05 | `tests::support::ux_gaps::uxd_spot_options_panel_paints_only_when_the_heal_tool_is_active` | `cargo test -p lumina-gui --lib -- --test-threads=1` | ok — 962 passed, 5 ignored; 8 UX-Gap-Wächter enthalten |

Weitere Module derselben Zeilen sind im selben Lauf ebenfalls grün:
UXG-03 `tests::timing_instrumentation` 13/13 · UXG-04
`tests::button_audit_click_toggles` 15/15 · UXG-06 `tests::startup_warmup` 5/5 ·
UXG-09 `tests::library_sort` 23/23 und `tests::selection` 12/12 · UXD-01
`tests::spot_followup_ui` 4/4 · UXD-05 und -08 `tests::spot_tool` 7/7.

**Golden-Ebene — derzeit nirgends ausführbar:** die adapter-pflichtigen
Golden-Tests (`crates/lumina-gui/tests/kittest_snapshots.rs`,
`crates/lumina-gui/tests/kittest_crop_overlay.rs`,
`crates/lumina-gui/tests/kittest_spot_tool.rs`) sind mit `#[ignore = "headless
GPU required; …"]` markiert (Begründung im Code:
`crates/lumina-gui/tests/kittest_snapshots.rs` → „CI without a GPU therefore
stays green"), und die CI führt `cargo test -p lumina-gui --all-targets` **ohne**
`--ignored` aus (`.github/workflows/ci.yml`, Job „Rust GUI tests") — sie laufen
also **weder hier noch in der CI**. Die Golden-Ebene ist damit eine
**zusätzliche**, derzeit **nicht ausführbare** Ebene; sie gehört zu
`GOLDEN-REPIN-R-85` (Referenzmaschine). Der Lauf-Beleg der Zeilen oben ist
der `--lib`-Lauf.

### Restbefund-Wächter (UX-GAP-RESTBEFUNDE-TESTS-101)

Die acht Restbefunde der „teilweise geschlossen"-Zeilen werden durch
headless-GUI-Wächter in `crates/lumina-gui/src/tests/ux_gaps.rs` beobachtet.
Sie pinnen den heutigen Zustand; sie treffen keine Produktentscheidung. Die
UXD-05-Prüfung sichert ausdrücklich beide Seiten des geschlossenen Verhaltens
(Options-Panel aus bei inaktivem Heal-Tool, an bei aktivem Tool).

| Zeile | gepinnter aktueller Zustand | Testname |
| --- | --- | --- |
| UXG-02 | die Rating-Zeile liegt weiter rechts (`crates/lumina-gui/src/develop_ops.rs` → `LuminaApp::draw_rating_section`) | `uxg_develop_rating_row_paints_in_the_right_panel` |
| UXG-03 | es gibt keinen Import-Dialog (nur Modulwechsel) | `uxg_import_opens_a_dialog_instead_of_only_switching_the_module` |
| UXG-08 | Compare/Survey zeigen weiterhin nur Thumbnails | `uxg_compare_and_survey_paint_the_full_render` |
| UXG-09 | Filter-/Sort-Leiste im Filmstreifen fehlt | `uxg_filmstrip_has_a_filter_and_sort_bar` |
| UXG-09 | der „n von N"-Zähler (`crates/lumina-gui/src/i18n.rs` → `Str::FilmstripCounter`) ist nicht per Test gepinnt | `uxg_filmstrip_counter_paints_n_of_n` |
| UXG-12 | kein Amount-Konzept bei den Presets | `uxg_presets_expose_an_amount_control` |
| UXG-14 | keine Canvas-Stepper (die `egui::DragValue`-Stepper sitzen in den Panels) | `uxg_canvas_paints_size_and_feather_steppers` |
| UXD-05 | das Options-Panel wird nur bei aktivem Heal-Werkzeug gezeichnet | `uxd_spot_options_panel_paints_only_when_the_heal_tool_is_active` |

Der Restbefund von **UXG-06** (fehlendes Calibration-Panel) braucht keinen
neuen Test: die Assertion von
`tests::startup::develop_section_order_is_lightroom_conform` listet die
Develop-Sektionen **vollständig** auf — ein Calibration-Panel ist damit bereits
negativ gepinnt.

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

### Offene Fragen aus den „teilweise geschlossen"-Zeilen (Stand 2026-10-23)

Je Zeile: der benannte Restbefund und der belegte Test (gelaufen, grün;
Abschnitt „Testbeleg je Zeile (Lauf 2026-10-21)"). Die Produktentscheidung
liegt beim Owner; die SOLL-Entscheide UX-LOOK-18 (2026-09-19) bleiben
verbindlich und werden hier nicht geändert.

- **UXG-02 — Rating-Zeile links wie in LR-Classic?** Die linke Rail trägt
  Navigator/Presets/Snapshots/History, die Rating-Zeile liegt weiterhin im
  rechten Panel (`crates/lumina-gui/src/develop_ops.rs` →
  `LuminaApp::draw_rating_section`); die SOLL-Entscheidung UX-LOOK-LAYOUT-18
  ordnet die linke Rail, nennt die Rating-Zeile aber nicht →
  **Entscheidung offen.**
  **Beleg:** `tests::layout::develop_left_rail_paints_navigator_presets_snapshots_history`
  (gelaufen, grün). **Restbefund-Wächter:**
  `uxg_develop_rating_row_paints_in_the_right_panel`.

- **UXG-03 — Import-Dialog nachziehen?** Der Import schaltet heute nur das
  Modul (`crates/lumina-gui/src/app_frame.rs` →
  `LuminaApp::apply_import_export_action`); einen Import-Dialog gibt es nicht
  (vgl. Fundstelle der Zeile UXG-03). Das Zielbild (Slice C) nennt einen
  Import-Dialog → **Entscheidung offen (G-09-Scope).**
  **Beleg:** `tests::g16_shortcuts::w3_shortcut_mappings_are_exact` (gelaufen,
  grün) — der Import-Pfad ist bis zur Aktion belegt (`I` →
  `ImportExportAction::Import`), der Dialog nicht.
  **Restbefund-Wächter:**
  `uxg_import_opens_a_dialog_instead_of_only_switching_the_module`.

- **UXG-06 — Calibration-Panel nachziehen?** Die rechte Rail folgt der
  F-100-Reihenfolge; ein Calibration-Panel fehlt vollständig (nicht in
  `LuminaApp::DEVELOP_SECTIONS`, vgl. Fundstelle der Zeile UXG-06). Das
  Zielbild (Slice A) nennt „Basic … Calibration in Classic-Reihenfolge" →
  **Entscheidung offen (F-100-Reihenfolge).**
  **Beleg:** `tests::startup::develop_section_order_is_lightroom_conform`
  (gelaufen, grün) — die Assertion listet die Develop-Sektionen vollständig
  auf; das fehlende Calibration-Panel ist damit bereits negativ gepinnt (kein
  neuer Test nötig).

- **UXG-08 — Compare/Survey als echte Renderansichten?** Die Loupe rendert
  heute das entwickelte Bild, Compare und Survey zeigen weiterhin nur
  Thumbnails (`crates/lumina-gui/src/filmstrip_frame.rs` →
  `thumbnail_for_entry`; vgl. Fundstelle der Zeile UXG-08). Das Zielbild
  (Slice B) nennt „echte … Loupe-/Compare-/Survey-Renderansichten" →
  **Entscheidung offen (G-09-Scope).**
  **Beleg:** `tests::library_views::r5_loupe::loupe_paints_the_developed_render_not_the_thumbnail`
  (gelaufen, grün) — die Loupe ja, Compare/Survey nein.
  **Restbefund-Wächter:**
  `uxg_compare_and_survey_paint_the_full_render`.

- **UXG-09 — Filter-/Sort-Leiste in den Filmstreifen?** Badges sind vorhanden
  (`crates/lumina-gui/src/library_paths.rs` → `paint_entry_badge`), der „n von
  N"-Zähler existiert im Code (`crates/lumina-gui/src/i18n.rs` →
  `Str::FilmstripCounter`), eine Filter-/Sort-Leiste **im Filmstreifen** fehlt
  (vgl. Fundstelle der Zeile UXG-09). Das Zielbild (Slice A) nennt „darunter
  Filmstreifen mit Badges + Filter-/Zähl-Leiste" → **Entscheidung offen**
  (zwei Teilfragen: Filter-/Sort-Leiste im Filmstreifen, Testpinnung des
  Zählers).
  **Beleg:** `tests::badges::entry_badge_text_covers_rating_flag_label`
  (gelaufen, grün) — der Badge-Datenpfad ist gepinnt, die Leiste nicht.
  **Restbefund-Wächter:**
  `uxg_filmstrip_has_a_filter_and_sort_bar` und
  `uxg_filmstrip_counter_paints_n_of_n`.

- **UXG-12 — Amount-Konzept bei den Presets?** Presets liegen als Datei-Liste
  bzw. Gruppen-Baum vor (`crates/lumina-gui/src/develop_ops.rs` →
  `LuminaApp::draw_presets_section`, `crates/lumina-gui/src/preset_tree.rs` →
  `draw_preset_tree`); ein Amount-Konzept fehlt (vgl. Fundstelle der Zeile
  UXG-12). Die SOLL-Entscheidung UX-LOOK-HISTORY-18 verlangt „Presets als
  Gruppen-Baum ohne absolute Pfade", sagt zum Amount nichts →
  **Entscheidung offen (Amount-Semantik).**
  **Beleg:** `tests::history_presets_look::preset_tree_groups_and_applies`
  (gelaufen, grün). **Restbefund-Wächter:**
  `uxg_presets_expose_an_amount_control`.

- **UXG-14 — Stepper auf dem Canvas?** Size/Feather lassen sich heute nur im
  Panel per `egui::DragValue` regeln, auf dem Canvas gibt es keine Stepper
  (vgl. Fundstelle der Zeile UXG-14) → **Entscheidung offen.**
  **Beleg:** `tests::layout::develop_section_previous_and_reset_buttons_change_recipe`
  (gelaufen, grün) — die Panel-Affordanz (Reset) ist ein echter Button; den
  Canvas-Cursor setzt heute `crates/lumina-gui/src/develop_geometry/crop_overlay.rs`
  → `paint_crop_mode_chrome`. **Restbefund-Wächter:**
  `uxg_canvas_paints_size_and_feather_steppers`.

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
