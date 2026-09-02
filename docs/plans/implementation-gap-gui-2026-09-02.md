# Implementation-Gap GUI — LuminaRust

**Datum:** 2026-09-02  
**Status:** Analyse (Doku-first, kein Code)  
**Vorgänger:** `docs/plans/gap-lightroom-parity-2026-09-02.md` (153d4ba), `docs/plans/gap-generative-fill-transparent-2026-09-02.md` (28b7782), `docs/plans/gui-tests-2026-09-02.md`  
**Bezug:** `Agents.md` (Muss: headless GUI-Test je sichtbarem Feature), `feature/README.md`, `feature/product/generative-expand.md` (GEN-EXPAND-1), `feature/product/spot-removal.md` (SPOT-REMOVE-1), `feature/architecture/pipeline.md` (F-089..099), `feature/architecture/sidecar.md` (F-001), `feature/platform/capability-matrix.md` (F-006), `feature/platform/cli-gui-wasm.md` (F-100), `crates/lumina-gui/src/lib.rs` (Shortcuts, Module, Sections)

> **Pflicht neu in `Agents.md`:** Jede sichtbare GUI-Funktion (Regler, Modul, Shortcut, Preview, Sidecar-Persistenz) braucht automatischen headless GUI-Test (`egui::Context` + `LuminaApp`, `tempdir`, ohne GPU/WASM) + bei visueller Änderung zusätzlich kittest Golden/PSNR/Histogram-Digest (byte-identisch/PSNR, kein stiller Fallback). `cargo test -p lumina-gui` muss ohne `--ignored` grün bleiben. Kein manueller Screenshot als einziges Gate.

---

## 0 Teststände verifiziert (2026-09-02)

| Suite | Kommando | Ist | Ziel |
|---|---|---|---|
| `lumina-gui` | `cargo test -p lumina-gui` | **147 passed, 8 ignored** (5 Goldens + 3 Interaktion `#[ignore]`) | **≥160 passed**, 8 ignored |
| `lumina-core` | `cargo test -p lumina-core` | 277+7 passed | ≥290 nach F-089..096 |
| `lumina-sidecar` | `cargo test -p lumina-sidecar` | 86 passed | ≥95 nach Schema-Bump |
| `lumina-onnx --features onnx-rt` | `cargo test -p lumina-onnx --features onnx-rt` | 107 passed | unverändert bis Inpaint |
| `cargo check --workspace` | `cargo check --workspace` | **grün (wasm32 `cargo check` grün)** | grün halten |
| `preview-cache` | `cargo bench` + `compare.mjs` | 6/6 buds OK `gate:false` | `warn` grün |

Lücke Headless: `147 → ≥160` = **+13 Tests** (Kap. 6). 8 `#[ignore]` kittest bleiben `#[ignore]` (CI gold-frei, lokal `UPDATE_SNAPSHOTS=true`).

---

## 1 Library L-01..13 — SOLL vs Ist

SOLL: Raster/Lupe/Vergleich/Übersicht; Sterne 1-5, Farben 6-9, Flaggen P/X/U, Kataloge/Ordner/Smart, Stapel Cmd+G, Metadaten, QuickDevelop, Filter `\`, VC Cmd+', Import/Export Cmd+Shift+I/E. Vgl. `feature/platform/cli-gui-wasm.md` F-100.

| # | Lightroom SOLL | Ist (`lumina-gui`/`lumina-sidecar`) | Tests | Gap |
|---|---|---|---|---|
| L-01 | **Grid/Loupe G/E** | `active_module` Library vs Develop; `module_for_key(G→Library, E→Library Loupe-Alias)`; Filmstrip+Navigator+Preview ja | kittest `library_empty/with_image` | **teilweise** (Loupe kein Vollbild-Switch) |
| L-02 | **Compare/Survey C/N** | nicht vorhanden | — | **fehlt** |
| L-03 | **Sterne 1-5** | kein Feld in `EditRecipe`/`VirtualCopy`/`SidecarDocument` | — | **fehlt** |
| L-04 | **Farb-Label 6-9** | kein Feld | — | **fehlt** |
| L-05 | **Flag P/X/U** | kein Flag-Feld (nur `source_status` Sidecar-Konflikt) | — | **fehlt** |
| L-06 | **Kataloge/Ordner** | `directory/entries/list_directory + open_folders/folder_children/folder_raw_counts` + `library_thumb_size` vorhanden | filmstrip 5 | **teilweise** (Folder-Tree ja, Collection nein) |
| L-07 | **Smart-Sammlungen** | keine Query/Filter-Collection, `lumina-index` Post-MVP | — | **fehlt** Post-MVP |
| L-08 | **Stapel Cmd+G** | kein `stack_id`/Bündelung | — | **fehlt** |
| L-09 | **Metadaten & Verschlagwortung** | `lumina-raw` EXIF `RawMetadata` vorhanden; IPTC/Keywords/Location nicht persistiert; `inspect --json` zeigt RAW-Meta | raw Fixture | **teilweise** (EXIF ja, IPTC nein) |
| L-10 | **Quick Develop** | `apply_adjustment_to_selection` (exposure/contrast/highlights/shadows) je Pfad via `save_sidecar`; kein GUI-Panel | kein GUI-Test | **teilweise** (CLI/MCP ja, Panel fehlt) |
| L-11 | **Filterleiste `\`** | kein Widget, keine EXIF-Filter | — | **fehlt** |
| L-12 | **Virtuelle Kopien Cmd+'** | stabile ID, Standardkopie, `duplicate/select/history`, Preset `.lumina-preset.json` | sidecar 86p | **teilweise** (Core vollständig, Shortcut fehlt) |
| L-13 | **Import/Export Cmd+Shift+I/E** | CLI `import/batch/export`; GUI `open_file/load_bytes/begin_load_path` + Drag&Drop + Export-Modul via `export_image` | CLI E2E | **teilweise** (GUI-Shortcut fehlt) |

**Library-Fazit:** Folder-Nav + Sidecar-Scan vorhanden; Bewertung/Flag/Label/Filter/Stack/Smart = größter Gap.

---

## 2 Develop D-01..19 — SOLL vs Ist

SOLL: Grundeinstellungen, WB/Klarheit/Dynamik, Kurve, Maske Pinsel/Verlauf/Radial + KI Personen/Motiv/Himmel, HSL, ColorGrading, Reparatur, Details/Geometrie, HDR/Pano, KI-Denoise, Snapshots, Y/V/J/L. Vgl. `feature/architecture/pipeline.md`.

| # | SOLL | Ist | Feld | Gap |
|---|---|---|---|---|
| D-01 | **Grundeinst.** Exposure/Contrast/Highlights/Shadows/Whites/Blacks + WB | `EditRecipe.adjustments` Keys + `apply_recipe_with_white_balance` (WB 1500..12000 / -1..=1) deterministisch sRGB | `adjustments` | **vollständig** |
| D-02 | **WB Klarheit/Dynamik** Vibrance/Saturation | `presence texture/clarity/dehaze` + `vibrance/saturation` in `recipe` vorhanden; GUI `set_presence` | `presence` | **teilweise** (Core ja, WB-Preset-Picker fehlt) |
| D-03 | **Kurve F-089** | normativ `curves {master, red/green/blue 2..32 monoton Hermite}` | `curves` | **fehlt** (Spec, kein Code) |
| D-04 | **Maske Pinsel/Linear/Radial K/M/Shift+M** | `MaskTool::Brush/LinearGradient/Radial` + `commit_brush_stroke/gradient/radial` + `MaskPrompt` vorhanden | `MaskPrompt` | **teilweise** (Tool vorhanden, K/M-Shortcut fehlt, Geometrie blockiert) |
| D-05 | **KI Personen/Motiv/Himmel** | BiRefNet + SAM2 4 Varianten via `lumina-onnx` `MaskInference`; `resolve_mask_planes` + Fallback | `ModelManifest` | **teilweise** (Inferenz vorhanden, Kategorie-Buttons fehlen) |
| D-06 | **HSL F-090** 8 Kanäle | normativ `hsl {8× hue/sat/lum}` | `hsl` | **fehlt** |
| D-07 | **ColorGrading F-091** | `color_grading {shadows/mids/highlights hue/sat + balance}` | `color_grading` | **fehlt** |
| D-08 | **Reparatur KI Heal/Entfernen** | nur `SourceActionArtifact {region u16, replacement RGBA8}` full-frame + CLI dust-removal; SPOT-REMOVE-1/GEN-EXPAND-1 Doku-first | `source_actions` | **fehlt** |
| D-09 | **Rote-Augen** | kein Feld/Algo | — | **fehlt** Post-MVP |
| D-10 | **Details Schärfen F-095** | `sharpening {amount 0..3 radius 0.1..10 detail/masking 0..1}` Unsharp-Mask normativ | `sharpening` | **fehlt** |
| D-11 | **Rauschreduzierung F-096 (+KI)** | 5×5 kantenbewusst Spec; KI post-MVP | `noise_reduction` | **teilweise** (manuell Spec, KI fehlt) |
| D-12 | **Objektivkorrektur F-098 + Lensfun** | `LensCorrection {distortion k1..k3, vignette c0..c2, ca}` + `lumina-lensfun` dynamisch | `lens_correction` | **vollständig** |
| D-13 | **Upright/Perspektive F-099** | `Perspective {vertical/horizontal/rotation/scale/aspect/shift}` | `perspective` | **teilweise** (Rezept+Regler ja, Auto-Upright nein) |
| D-14 | **Zuschneiden/Rotation F-093** | `Geometry {crop, rotation -180..180, mirror}` + `apply_geometry` 5-in-1 | `geometry` | **teilweise** (Core ja, Pipeline entkoppelt fehlt) |
| D-15 | **Vignette/Grain F-097** | `Effects {vignette amount -1..1, grain seed}` | `effects` | **vollständig** (13 Tests) |
| D-16 | **HDR/Panorama → DNG Cmd+H/M** | kein Merge/DNG-Writer; Export nur 8-bit PNG/JPEG/WebP | — | **fehlt** Post-MVP |
| D-17 | **KI Entrauschen/Unschärfe** | nur BiRefNet/SAM, `onnx-wasm` off | — | **fehlt** Post-MVP |
| D-18 | **Snapshots/Protokoll Cmd+Alt+S** | `history: Vec<HistoryEntry>` + `restore_history` vorhanden; Snapshots ≠ History | `history` | **teilweise** (History ja, Snapshots fehlen) |
| D-19 | **Vorher/Nachher Y / Shift+Y, V/J/L** | `before_after: bool` + `Y` Toggle vorhanden | — | **teilweise** (Y ja, Shift+Y/V/J/L fehlen) |

**Develop-Fazit:** Exposure/WB/Lens/Vignette/Grain/Mask-Geometrie vorhanden; Kurve/HSL/ColorGrading/Presence-GUI/Shapen/Noise/Spot-Heal/HDR = Haupt-Gap.

---

## 3 Sidecar-Felder — SOLL vs Ist

**SOLL** `feature/architecture/sidecar.md` + `feature/product/generative-expand.md` + `spot-removal.md`: Sidecar `.lumina.json` + `.lumina.zdata` (BLAKE3/zstd, relativ, atomar, `schema_version` bump additiv). Vgl. `gap-lightroom` LR-16.

| Feld | SOLL | Ist | Gap |
|---|---|---|---|
| `rating` (0..5, 0=unrated) | Sterne 1-5 + 0, persistiert je VC | **fehlt** | LR-01 |
| `color_label` (1..4 aus 6-9) | Farbmarkierung je VC | **fehlt** | LR-17 |
| `flag` (Pick/Reject/Unflag) | P/X/U je VC | **fehlt** | LR-01 |
| `stack_id` (String \| null) | Stapel-Bündelung `Cmd+G`, post-MVP | **fehlt** | LR-17 |
| `generative_edit` (`GenerativeEdit` v1) | `canvas {output_width/height, source_offset_x/y}` + `auto_fill_transparent/expand_beyond_image/keep_generative_content` + `model/prompt/seed/inference_resolution/region/mask_reference/artifact/status` | **fehlt** (Doku GEN-EXPAND-1, kein Code) | G1/G4 |
| `spot_removals` (heuristisch vs `spot_heal_generative`) | SPOT-REMOVE-1 `center/radius/feather/source_offset/opacity` vs `mask_reference/model/prompt/seed/artifact` | **fehlt** | G5/G6 |
| `curves/hsl/color_grading/presence/sharpening/noise_reduction` | additive v2 Felder in `recipe.adjustments` | **fehlen** | LR-02..04 |
| `source_actions` | F-042 `SourceActionSpec` vorhanden | **vorhanden** | — |

Schema-Entscheidung: Pre-MVP `schema_version` bleibt 1, inkompatibel darf ohne Migration brechen (`sidecar.md`); ab MVP Bump additiv mit `migrate` + Backup. `serde(flatten) extras` hält unbekannte Felder roundtrip.

---

## 4 Pipeline — SOLL vs Ist

**SOLL** `feature/architecture/pipeline.md` MVP `Decode→SourceActions→AutoAnalysis→Adjustments→Masks→Crop→Output` (sRGB RGBA8). Ziel nach GEN-EXPAND-1/SPOT-REMOVE-1 (Doku-first): `Decode→SourceActions→SpotHeal(quick/generative)→LensCorrection(F-098)→GenerativeEdit(auto-fill)→Perspective(F-099)→GenerativeEdit(expand)→Crop(F-093)→Output`; `keep_generative_content` steuert Materialisierung (vgl. `generative-expand.md` §Koordinatenreferenz: post-GenerativeEdit-Canvas ist Referenz für Crop/Masken/Vignette/F-041 Messbereich).

**Ist:** `Pipeline::default()` unverändert MVP; `apply_geometry` 5-in-1 (Lens+Perspective+Crop+Rotation+Mirror); kein `GenerativeEdit`/`SpotHeal` Stage; `RenderKey`/`stage_digest` kennt `source_actions` checksums + `geometry`, aber kein `canvas`/`spot_heal`/`generative_canvas`. Lensfun per-feature `lumina-lensfun` byte-identisch Fallback verifiziert.

**Gap:** Entkopplung 5-in-1 → eigene Stages + `GenerativeEdit`/`SpotHeal` Stufe + `RenderKey`/`stage_digest` um `canvas/prompt/seed/model`/`spot_*`.

---

## 5 Shortcuts — gebunden vs fehlend

Quelle `crates/lumina-gui/src/lib.rs` `module_for_key` + Verifiz. 2026-09-02.

| Gebunden | Taste | Mod |
|---|---|---|
| `G` → Library Grid | `module_for_key` | `lib.rs:121` |
| `E` → Library (Loupe-Alias) | alias, kein separates Loupe-Modul | `lib.rs:123` |
| `D` → Develop | — | `lib.rs:122` |
| `Y` → Before/After | `before_after` Toggle | `lib.rs:443,917` |
| Zoom | `Fit/1:1/2:1/FitWidth/Custom` + Mausrad `+/-` + `preview_pan` | — |
| Alt-Scroll / Doppelklick | Feinjustierung + Reset (slider) | `slider.rs` |

| Fehlend / nur API | Lightroom | Status |
|---|---|---|
| `C/N` | Compare/Survey | ungebunden (Modul `Compare`/`Survey` nur Typ, kein Widget) |
| `1-5` | Sterne | ungebunden (kein Sidecar-Feld) |
| `6-9` | Farb-Label | ungebunden |
| `P/X/U` | Pick/Reject/Unflag | ungebunden |
| `Cmd/Ctrl+G` | Stack | ungebunden |
| `Cmd/Ctrl+'` (Apostroph) | VC duplizieren | API `duplicate_virtual_copy` vorhanden, Key fehlt |
| `Cmd/Ctrl+Shift+I/E` | Import/Export | CLI ja, GUI-Key fehlt |
| `R` | Crop | Panel vorhanden, Key fehlt |
| `Q` | SpotHeal | SPOT-REMOVE-1 Doku, Key fehlt |
| `K/M/Shift+M` | Pinsel/Verlauf/Radial | `MaskTool` vorhanden, Key fehlt |
| `Shift+Y` | Split Before/After | `Y` ja, `Shift+Y` fehlt |
| `V/J/L` | SW / Clipping / Lights Out | ungebunden |
| `Cmd/Ctrl+Shift+C/V` | Copy/Paste Settings | `apply_preset`/`apply_adjustment_to_selection` vorhanden, Key fehlt |
| `Tab/Shift+Tab` | Panels ein/aus | ungebunden |
| `F` | Vollbild | ungebunden (eframe) |
| `\` | Filterleiste | ungebunden |
| `Alt+Regler-Drag` | Feinjustierung | Alt-Scroll ja, Alt+Drag fehlt |
| `Alt+Regler-Klick` / `Shift+Doppelklick` | Reset / Auto Schwarz/Weiß | Doppelklick ja, Alt-Klick/Shift+Doppelklick fehlt |
| `Cmd/Ctrl+Alt+S` | Schnappschuss | nur `HistoryEntry`, Key fehlt |
| `Cmd/Ctrl+H/M` | HDR/Pano Merge | ungebunden (kollidiert mit macOS Hide/Minimize) |

Fazit MVP-Pflicht: `1-5/P/X/U` + `K/M/Q/R` + Copy/Paste + `Cmd+'` vor Post-MVP Shortcuts.

---

## 6 Headless Test-Gap — fehlende Tests (≥13 neu, T-LR-01..13)

Pflicht nach `Agents.md`: je sichtbarer GUI-Funktion ein `cargo test -p lumina-gui` grüner Headless-Test. Bestehend: 95 `lib.rs` + 7 `preview_ctrl` + 5 `filmstrip` + 8 `viewport` + 7 `theme` + 14 `presets`. Kittest Goldens decken nur 5 Module ab; **keine** Slider/Kurve/HSL/ColorGrading/Presence/Mask/Library-Headless-Outcome.

| ID | Feature | Fehlender Headless-Test (SOLL) | kittest/PSNR/Histogram Vorschlag | Crate | Aufw | Risiko |
|---|---|---|---|---|---|---|
| **T-LR-01** | Exposure/Contrast/Highlights/Shadows | `apply_recipe_with_white_balance` Headless via `LuminaApp` + `tempdir` Sidecar Roundtrip; Slider `exposure -10..10` bewegt `preview_generation` + `tone_analysis.median` Δ ≤1/256 | Histogram `LuminanceHistogram::digest` vor/nach | `lumina-gui` | S | — |
| **T-LR-02** | **Gradationskurve F-089** | Kurven-Editor monoton Hermite kein Overshoot; 2..32 Punkte Validierung + `recipe.curves` Roundtrip | Golden sRGB Ramp vor/nach Kurve 1/255 | `lumina-core` + `lumina-gui` | M | Performance |
| **T-LR-03** | **HSL F-090** (8 Kanäle) | 8 Zentren Nachbarübergang zyklisch, Hue-Dreh 30° | Golden ColorChecker PSNR >40 dB, `stage_digest` Miss | `lumina-core` | M | — |
| **T-LR-04** | **ColorGrading F-091** | Schatten/Mitten/Lichter `hue 0..360 sat 0..1 balance -1..1` weich + clip | Golden Haut, `balance` Verschiebung | `lumina-core` | M | — |
| **T-LR-05** | **Presence F-094** | `texture/clarity/dehaze` -1..1, DoG-Heuristik Radius 1..3/8..32, Headless Slider | Golden Hautschutz Hue 15..55 Δ außerhalb <1/255 | `lumina-core`/`lumina-gui` | S | — |
| **T-LR-06** | **Vibrance/Saturation F-092** | `vibrance` Hautschutz + `saturation` linear, Headless Slider | Golden ColorChecker | `lumina-core` | S | — |
| **T-LR-07** | **Schärfen F-095** | Unsharp-Mask `amount 0..3 radius 0.1..10` + `masking` Kanten | Golden Checker Draft vs Full PSNR-Gate | `lumina-core` | M | Draft-Scale |
| **T-LR-08** | **Rauschreduzierung F-096** | 5×5 kantenbewusst Luminance/Color `0..1` | PSNR-Gate, `sharpen_render_scale_changes_render_only` analog | `lumina-core` | S | — |
| **T-LR-09** | **Maskierung Pinsel/Linear/Radial** | `MaskTool::Brush/LinearGradient/Radial` + `commit_brush_stroke/gradient/radial` + `rasterize_prompt` Headless, `geometry_blocks_source_mapping` Gate | kittest Mask-Overlay Vor/Nach `Y` generational bump | `lumina-gui` | M | Geometrie-Lock |
| **T-LR-10** | **Library Grid / Folder / Flag-Rating** | `list_directory` + `open_folders/folder_children/folder_raw_counts` + Rating `1-5` + `P/X/U` Headless Sidecar Roundtrip 2 VC | kittest `library_empty/with_image` Badge Sterne/Farbpunkt/Stack + Snapshot | `lumina-gui`/`lumina-sidecar` | M | — |
| **T-LR-11** | **Crop/Geometry + Zoom/ROI + `Y` Before/After** | `Geometry.crop` + Rotation/Mirror + `ZoomMode` + `preview_roi` Headless Roundtrip; `before_after` hält Rezept unverändert | kittest Before/After Split + kittest Preview-Cache LRU/Stale | `lumina-gui` | S | Zoom-Loop |
| **T-LR-12** | **Quick Develop + Filter `\`** | `apply_adjustment_to_selection` auf 3 Sidecars batch Headless; Filter `\` nach Brennweite/Kamera/ISO (synthetische EXIF) | Unit Filter `focal==50mm`, `-batch --jobs` deterministisch | `lumina-gui`/`lumina-sidecar` | M | — |
| **T-LR-13** | **Spot-Heal schnell (heuristisch)** | Heal 3×3 Clone deterministisch, `source_action` Heal vs replacement, `Q` Toggle Headless | Golden 8×8 Spot auf Checker byte-identisch; Histogram-Delta 1/256 | `lumina-core`/`lumina-gui` | M | <200 ms 24 MP |

Zusätzlich **kittest/visuell** (ergänzend, nicht Ersatz für Headless): Expand-Rahmen (vor/nach Bestätigen), Auto-Fill Keile (mit/ohne `auto_fill_transparent`), Spot-Panel Schnell↔Generativ Badges, ColorGrading/HSL Preset-Wechsel; **Histogram-Delta** `analyze_tone median/p01/p99` + `LuminanceHistogram::digest` je Stufe mit Toleranz ≤1/256 (R2-PERF-01).

Alle 13 liefern `cargo test -p lumina-gui` ≥160p (147+13). Kein `#[ignore]` für Headless; kittest bleibt `#[ignore]` lokal mit `UPDATE_SNAPSHOTS=true`.

---

## 7 Priorisierte Lücken — 20 (hoch/mittel/niedrig) + Wellen

Legende: Aufwand `S<1d, M1-3d, L>1w`; Risiko `Lizenz/Modell/Performance/UX-Break`; ein Crate = ein schreibender Agent.

| # | PRIO | Lücke | SOLL-Anker | Crate | Aufw | Risiko | Welle |
|---|---|---|---|---|---|---|---|
| **LR-01** | **hoch** | **Bewertung/Flag** (`rating`/`flag` + GUI + Shortcuts `1-5/P/X/U` + Headless T-LR-10) | Library SOLL, `sidecar.md` Schema | `lumina-sidecar` + `lumina-gui` | M | UX-Break gering (Pre-MVP Schema-Break erlaubt) | **W1** Sidecar |
| **LR-02** | **hoch** | **Gradationskurve F-089** Core + GUI Editor | `pipeline.md` F-089 | `lumina-core` + `lumina-gui` | M | Performance | **W2** Core |
| **LR-03** | **hoch** | **HSL F-090 + ColorGrading F-091** | `pipeline.md` F-090/091 | `lumina-core` | M | — | **W2** Core |
| **LR-04** | **hoch** | **Schärfen F-095 + Rausch F-096 manuell** | `pipeline.md` F-095/096 | `lumina-core` | M | Draft ohne Sharpen | **W2** Core |
| **LR-05** | **hoch** | **Spot-Heal schnell (heuristisch Clone, Q)** + `Q` + T-LR-13 | `spot-removal.md` heuristisch, `pipeline.md` F-042 | `lumina-core` + `lumina-gui` | M | <200 ms 24 MP | **W2** Core |
| **LR-06** | **hoch** | **GenerativeEdit Canvas (auto_fill_transparent + expand_beyond_image + keep_generative_content)** + Pipeline `Lens→GenerativeEdit→Perspective→Crop` | `generative-expand.md` GEN-EXPAND-1 | `lumina-sidecar` (G1) + `lumina-core` (G2) | L | Lizenz F-078, 45 MP 180 MiB, WASM not available | **W1→W2** Sidecar→Core |
| **LR-07** | **hoch** | **Pipeline Entkopplung** `apply_geometry` 5-in-1 → Stages Lens/Perspective/Crop + `GenerativeEdit` | `pipeline.md` | `lumina-core` | M | `stage_digest`/`RenderKey` Break | **W2** Core |
| **LR-08** | **hoch** | **Presence/Dynamik GUI** F-094 + F-092 Vibrance in `cli-gui-wasm.md` Reihenfolge | `pipeline.md` F-094/F-092 | `lumina-gui` | S | — | **W3** GUI |
| **LR-09** | **hoch** | **Core Shortcuts Prio** `Cmd+'` + `Copy/Paste C/V` + `Shift+Y` + `V/J/L` + Headless `develop_shortcut_for_key` | Develop/Library Shortcuts, `virtual-copies.md` | `lumina-gui` | S | — | **W3** GUI |
| **LR-10** | **mittel** | **Masken-Shortcuts `K/M/Shift+M/R/Q` + `Alt+Regler` + `Tab/F`** binden | `lib.rs` `MaskTool`, `slider.rs` | `lumina-gui` | S | `geometry_blocks_source_mapping` Gate | **W3** GUI |
| **LR-11** | **mittel** | **Staub generativ lokal `inpaint_heal`** (`lumina-onnx` `inpaint` Capability + GUI Maske malen) | `spot-removal.md` generativ | `lumina-onnx` + `lumina-core` + `lumina-gui` | L | **Lizenz** (Inpaint CC BY-NC/AGPL, `pending-integration` bis LIZ) | **Nach LIZ** |
| **LR-12** | **mittel** | **Snapshot `Cmd+Alt+S`** benannte Rezept-Freeze-Punkte vs History | `virtual-copies.md` History | `lumina-sidecar` + `lumina-gui` | S | — | **W1** Sidecar |
| **LR-13** | **mittel** | **Quick Develop + Filter `\` + Import/Export Shortcuts** | Library SOLL, `cli-gui-wasm.md` | `lumina-gui` + `lumina-cli` | M | — | **W3** GUI |
| **LR-14** | **mittel** | **GUI-Tests Outcome ausbauen T-LR-01..13 + T01-T10 `gui-tests` Plan** | `gui-tests-2026-09-02.md`, `preview-cache.md` | `lumina-gui` + `lumina-core` | M | `file_stamp` Mtime, GPU CI-Nein | **W3** GUI |
| **LR-15** | **mittel** | **RenderKey/Cache Invalidierung** für generative/Spot-Artefakte + Canvas in `stage_digest` | `pipeline.md` Reproduzierbarkeit, `generative-expand.md` Identität | `lumina-core` | S | — | **W2** Core |
| **LR-16** | **mittel** | **Sidecar Schema Versionierung** additiv `curves/hsl/.../effects` Migration v1→v2 + Backup | `sidecar.md`, `feature/README.md` Pre-MVP Break | `lumina-sidecar` | S | Pre-MVP Break ok | **W1** Sidecar |
| **LR-17** | **niedrig** | **Farb-Label `6-9` + Stack `Cmd+G` + Kataloge/Smart-Index** | Library SOLL, Phase 9 Index Post-MVP | `lumina-sidecar` + `lumina-gui` | M | — | Post-MVP |
| **LR-18** | **niedrig** | **HDR/Panorama DNG Merge `Cmd+H/M` + Rote-Augen** | Develop SOLL, `export.md` | `lumina-core` | L | DNG 16-bit Pipeline | Post-MVP |
| **LR-19** | **niedrig** | **KI Entrauschen/Unschärfe + Capability `inpaint/outpaint` lokal vs Cloud + `onnx-wasm` off** | `capability-matrix.md`, `wasm-limits.md` SPOT-REMOVE-1 | `lumina-onnx` + Doku | S | Cloud kein stiller Fallback | **W1** Doku |
| **LR-20** | **niedrig** | **Library Compare/Survey `C/N` + Lights Out `L` + Clipping `J`** | Library/Develop SOLL | `lumina-gui` | M | — | **W3** GUI |

### Wellen-Vorschlag (Ein-Crate-Regel, seriell wo Schema/Pipeline betroffen)

- **Welle 1 seriell Sidecar:** LR-16 Schema + LR-01/LR-12 + LR-06 `generative_edit` zdata `kind=generative_canvas` + LR-15/19 Capability (ein Agent `lumina-sidecar`)
- **Welle 2 Core:** LR-07 Entkopplung + LR-02/03/04 + LR-05 Heal + LR-06/11 Pipeline-Stufen + LR-15 digest (ein Agent `lumina-core`)
- **Welle 3 GUI exklusiv:** LR-08/09/10 + LR-13/14/20 + T-LR-01..13 Headless + kittest Refresh (ein Agent `lumina-gui`, `cargo test -p lumina-gui` 147→≥160)
- **Nach Lizenz:** LR-11 Inpaint `inpaint_heal`/`inpaint`/`outpaint` in `lumina-onnx` (blockiert auf LR-19/G14 Lizenz-Pin, `pending-integration` bis F-078)

---

## 8 Auto-Verifikation je Gap (kein manueller Test als Gate)

Grundsatz: deterministische Fixture + Seed + Golden/PSNR-Gate + Histogram-Digest; `UPDATE_SNAPSHOTS=true` nur lokal, CI nur `cargo test` ohne `--ignored`, `wasm32` `cargo check` grün.

| Gap | Ansatz | Gate |
|---|---|---|
| LR-01 Flag/Rating | Sidecar Roundtrip `rating/flag` + kittest Library Badge | byte-identisch Sidecar, Snapshot diff |
| LR-02 Kurve | Unit Hermite monoton kein Overshoot, Golden Ramp | 1/255 pro Kanal |
| LR-03/04 HSL/Grading/Sharpen | Unit Zentren/Gewichte, Golden ColorChecker | PSNR >40 dB, `stage_digest` Miss |
| LR-05/11 Heal | Golden Checker 8×8 Spot deterministisch; kittest Dust-Panel | byte-identisch (kein Modell) |
| LR-06 Generative Canvas | Golden synthetische Lens-Keile 15% → Auto-Fill; Expand >100% Seed-pin; kittest Rahmen | PSNR >35 dB (Seed Pflicht), BLAKE3 |
| LR-07 Pipeline | `Pipeline::validate` + `render_frame_from_base` byte-identisch vor/nach Entkopplung | `stages()` exakt |
| LR-08/09/10 Shortcuts/Presence | Headless `module_for_key/develop_shortcut_for_key/rating_for_key/flag_for_key` + `set_mask_tool/commit_brush_stroke` | `preview_generation` bump, Shortcut-Keycode exakt, Histogram 1/256 |
| LR-15 RenderKey | `source_action_artifact_hashes` + `canvas` in `stage_digest` (non-decode) | `digest !=` bei Hash-Wechsel, `decode` unverändert |
| LR-19 Capability | `cargo check --target wasm32-unknown-unknown -p lumina-onnx --features onnx-rt` grün (`RuntimeDisabled`), Matrix Zeile `inpaint` | wasm Gate grün |
| Allgemein | `LuminanceHistogram::digest` + `analyze_tone` Δ ≤1/256 (R2-PERF-01) | 1/256 Schranke |

---

## 9 Offene Risiken — vor Umsetzung entscheiden

1. **Pipeline-Platzierung nach Lens vs GEN-EXPAND-1 vor Lens:** Auto-Fill als post-Geometry Inpaint (gleicher Canvas) von Outpaint (Canvas >100%) trennen — ersteres Wunsch 1, letzteres Wunsch 2 (`gap-generative-fill` §6-1). Empfehlung: Lens→Auto-Fill→Perspective→Expand→Crop.
2. **Canvas-Koordinaten Break:** `crop_rect` rechnet heute auf Quell-`width/height`; bei Canvas>Quelle auf Canvas — Geometrie-Hash ändert sich. Pre-MVP Break erlaubt, aber `schema_version` Bump dokumentieren.
3. **Modell-Lizenz Inpaint/Outpaint:** viele SOTA CC BY-NC/AGPL (vgl. `fixtures-licensing.md` §5 `ultralytics`) — kein Code vor Hash/Lizenz-Pin (F-078), `pending-integration` bis LIZ.
4. **Performance:** 24 MP Canvas 96 MiB + 45 MP 180 MiB + Preview-Cache 7 Slots 1.5 GiB + VRAM 1024/4 → 8 GB Budget nur knapp nicht gerissen — `compare.mjs gate:true` erst nach Kalibrierung.
5. **Seed-Pflicht:** Inpaint ohne `seed: u64` nicht deterministisch — Golden flakey (LR-11 Gate braucht Seed-Pin).
6. **WASM Parity:** `zdata`/`zstd` native-only, generatives Canvas auf WASM `not available` — Capability muss `inpaint` als nicht verfügbar ausweisen, sonst stiller Fallback.
7. **Shortcut-Kollision:** `Cmd+H/M` (HDR/Pano) kollidiert mit macOS Hide/Minimize — alternative Belegung oder Menü-only prüfen.

---

## 10 Referenzen (SOLL-Docs)

- `feature/README.md` — Index, Invarianten, Feature-Matrix (F-089..099, F-100)
- `feature/platform/cli-gui-wasm.md` (F-100, F-103) — UI-Konventionen, Module Library/Develop/Export, `Y`-Toggle, Sections-Reihenfolge
- `feature/architecture/pipeline.md` (F-089..099, F-036/042/041) — Pipeline `Decode→SourceActions→…→Crop→Output`, Kurve/HSL/ColorGrading/Presence/Sharpen/Noise/Vignette/Lens/Perspective, `stage_digest`
- `feature/architecture/sidecar.md` (F-001) — `.lumina.json` + `.lumina.zdata` (BLAKE3/zstd, relativ, atomar), `artifact_status`
- `feature/product/virtual-copies.md` (F-002/F-014/F-009) — stabile ID, Standardkopie, Presets `<name>.lumina-preset.json`
- `feature/product/ai-masks.md` (F-004, F-079..F-083) — Maskenidentität, BiRefNet/SAM2
- `feature/product/spot-removal.md` (SPOT-REMOVE-1) — heuristisch vs generativ `spot_heal_generative`
- `feature/product/generative-expand.md` (GEN-EXPAND-1) — `GenerativeEdit` Canvas >100%, `auto_fill_transparent`/`expand_beyond_image`/`keep_generative_content`
- `feature/product/export.md` (F-037) — sRGB 8-bit, `export_image` byte-identisch GUI/CLI
- `feature/platform/capability-matrix.md` + `feature/platform/wasm-limits.md` (F-006/F-069..F-071) — `zdata`/`onnx` native-only, Limits 45/24 MP, 8 GB/512 MiB/48 MiB, Budgets
- `feature/quality/preview-cache.md` + `feature/quality/performance-benchmarks.md` (F-074) — Preview-Cache Budgets, `compare.mjs`
- `docs/plans/gap-generative-fill-transparent-2026-09-02.md` (28b7782) — G1..G15, Welle Sidecar→Pipeline→GUI
- `docs/plans/gap-lightroom-parity-2026-09-02.md` (153d4ba) — LR-01..20 SOLL vs Ist + Shortcuts
- `docs/plans/gui-tests-2026-09-02.md` — T01..T10 Outcome-Tests
- `crates/lumina-gui/src/lib.rs` — `module_for_key`, `develop_shortcut_for_key`, `MaskTool`, `LuminaApp` Sections, `IdleQueue`, kittest

---

**Pfad:** `docs/plans/implementation-gap-gui-2026-09-02.md`  
**Summary:** Bibliothek: Folder-Nav vorhanden; Bewertung/Flag/Label/Filter/Stack/Smart fehlen (LR-01/17). Entwickeln: Exposure/WB/Lens/Vignette/Grain/Mask-Brush vorhanden (teilweise); Kurve/HSL/ColorGrading/Presence/Shapen/Noise/Spot-Heal fehlen (LR-02..06). Sidecar: `rating/flag/stack_id/generative_edit` fehlen. Pipeline: 5-in-1 statt entkoppelt; `GenerativeEdit`/`SpotHeal` fehlt. Shortcuts: nur G/D/E/Y+Zoom gebunden; ~25 Lr-Shortcuts (C/N, 1-5/6-9/P/X/U, Cmd+G/', R/Q/K/M, Tab/F, \, Alt+Regler) fehlen. Tests: 147p+8 ignored → Ziel ≥160p via 13 Headless T-LR-01..13 (Slider/Kurve/HSL/Grading/Presence/Mask/Library, je mit kittest/PSNR/Histogram, Crate S/M/L). Priorisiert 20 Lücken mit Wellen Sidecar→Core→GUI exklusiv; auto-Verifikation je Gap ohne manuellen Test.
