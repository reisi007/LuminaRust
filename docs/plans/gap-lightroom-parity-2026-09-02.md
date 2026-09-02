# Gap-Analyse Lightroom CC Parität — LuminaRust

**Datum:** 2026-09-02  
**Status:** Analyse (kein Code, Doku-first)  
**Auftrag:** Lightroom-Aufzählung Bibliothek/Entwickeln vs LuminaRust Ist — SOLL/IST/Gap, Shortcuts, priorisierte Lücken, auto-Verifikation  
**Bezug:** `Agents.md`, `Agents.todo.md` (Block A/C, F-103-INTEGRATION-PREVIEW-SIDECAR 43b1b73, CI-ONNX-RT 953987e), `feature/README.md`, `feature/platform/cli-gui-wasm.md` (F-100, F-103, F-010), `feature/architecture/pipeline.md` (F-089..099, F-036, F-042), `feature/architecture/sidecar.md` (F-001), `feature/product/virtual-copies.md` (F-002/F-014/F-009), `feature/product/ai-masks.md` (F-004/F-079..F-083), `feature/product/spot-removal.md` (SPOT-REMOVE-1), `feature/product/generative-expand.md` (GEN-EXPAND-1), `feature/product/export.md` (F-037), `feature/platform/capability-matrix.md` (F-006/F-069..F-071), `feature/platform/wasm-limits.md`, `docs/plans/gap-generative-fill-transparent-2026-09-02.md` (28b7782), `docs/plans/gui-tests-2026-09-02.md` (T01-T10), `crates/lumina-gui/src/lib.rs` (Module, Shortcuts, Library/Develop Slices), `crates/lumina-core` (`pipeline.rs`, `render.rs`), `crates/lumina-cli`, `crates/lumina-mcp`

---

## 0 Lesehinweis — Teststände (2026-09-02 verifiziert)

| Suite | Kommando | Ist | Hinweis |
|---|---|---|---|
| `lumina-gui` | `cargo test -p lumina-gui` | **147 passed, 8 ignored** (43b1b73; vorher 133 in `gui-tests` Plan) | Unit 95+7+5+8+7+14, kittest 5 Goldens + 3 Interaktion `#[ignore]` |
| `lumina-core` | `cargo test -p lumina-core` | **277 + 7** passed | `pipeline.rs`, SourceActions, Masks, MaskLoader, tone/histogram, RenderKey |
| `lumina-sidecar` | `cargo test -p lumina-sidecar` | **86 passed** | JSON-Roundtrip v1→v2, atomic write, `.zdata` container, `artifact_status` eager BLAKE3 |
| `lumina-onnx --features onnx-rt` | `cargo test -p lumina-onnx --features onnx-rt` | **107 passed** (49f4f76) | BiRefNet + SAM2 4 Varianten, hash-gepinnte Fixture `2a2ede66…` |
| kittest/Preview-Cache | `cargo test -- --ignored` / `cargo bench` | 5 Goldens, 6 Benches `preview_cache/*` | `gui-tests-2026-09-02.md` T01-T10 Doku-first offen |

Legende Gap: **vollständig** = Code + Test + verifiziert; **teilweise** = Code vorhanden, aber z.B. GUI-Bindung/Golden fehlt; **fehlt** = Doku-first, kein Code.

---

## 1 Bibliothek (Library) — SOLL vs Ist

SOLL laut Auftrag: Raster/Lupe/Vergleich/Übersicht; Sterne 1-5, Farben 6-9, Flaggen P/X/U, Kataloge/Ordner/Smart-Sammlungen, Stapel Cmd+G, Metadaten, QuickDevelop, Filterleiste `\\`, virtuelle Kopien Cmd+', Shortcuts G/E/C/N/1-5/6-9/P/X/U/Cmd+G/Cmd+Shift+I/E

| # | Lightroom SOLL | LuminaRust Ist (Code?) | Tests? | Gap | Pri | Crate | Shortcut |
|---|---|---|---|---|---|---|---|
| L-01 | **Ansichten Grid/Loupe** (`G`/`E`) | `LuminaApp::active_module` Library vs Develop; `module_for_key` mappt `G→Library`, `E→Library` (Loupe-Alias, kein separates Modul) — Filmstrip + Navigator + Preview vorhanden | kittest `library_empty/with_image` | **teilweise** — Loupe ist Alias, kein Vollbild-Loupe-Switch | MVP | `lumina-gui` | `G` gebunden, `E` alias gebunden |
| L-02 | **Compare/Survey** (`C`/`N`) | Kein Compare/Survey Modul, kein Side-by-Side, kein Survey-Raster | — | **fehlt** | post-MVP | `lumina-gui` | `C`/`N` ungebunden |
| L-03 | **Sterne 1-5** | Kein Sterne-Feld in `EditRecipe`/`VirtualCopy`/`SidecarDocument`; kein Sidecar-Persistenz | — | **fehlt** | MVP | `lumina-sidecar` + `lumina-gui` | `1-5` ungebunden |
| L-04 | **Farbmarkierungen 6-9** | Kein Farb-Label Feld | — | **fehlt** | post-MVP | `lumina-sidecar` | `6-9` ungebunden |
| L-05 | **Flaggen P/X/U** (Pick/Reject/Unflag) | Kein Flag-Feld im Sidecar; nur `source_status`/`conflict` für Sidecar-Konflikt, nicht Pick/Reject Workflow | — | **fehlt** | MVP | `lumina-sidecar` | `P/X/U` ungebunden |
| L-06 | **Kataloge/Ordner** (Folder Tree) | `directory`/`entries`/`list_directory` + `open_folders/folder_children/folder_raw_counts` vorhanden; `library_thumb_size` slider; Auto-Load erster RAW | Unit `filmstrip` 5 Tests | **teilweise** — Folder Tree ja, Katalog/Collection-Semantik nein | MVP | `lumina-gui` | — |
| L-07 | **Smart-Sammlungen** | Keine Query/Filter-Collection, kein `lumina-index` im Workspace (Post-MVP, `feature/architecture/index.md` normativ) | — | **fehlt** (post-MVP per Agents.todo Phase 9) | post-MVP | `lumina-sidecar` | — |
| L-08 | **Stapel Cmd+G** (Stack) | Kein Stack-Feld/Bündelung im Sidecar | — | **fehlt** | post-MVP | `lumina-sidecar` | `Cmd+G` ungebunden |
| L-09 | **Metadaten & Verschlagwortung** EXIF/IPTC, Stichwörter, Standort | `lumina-raw` decodiert EXIF via LibRaw + `RawMetadata` (Kamera/Lens/Orientierung); IPCT/Keywords/Location nicht im Sidecar persistiert; `inspect --json` zeigt RAW-Metadaten (R2-CLI) | `lumina-raw` via Fixture | **teilweise** — EXIF vorhanden, IPTC/Keywords fehlen | post-MVP | `lumina-raw`/`lumina-sidecar` | — |
| L-10 | **Quick Develop** (Vorgaben+Tonwert auf mehrere Bilder) | `apply_adjustment_to_selection` für `exposure/contrast/highlights/shadows` via `save_sidecar` je Pfad vorhanden; kein Quick-Develop Panel in GUI | kein GUI-Test | **teilweise** — CLI/MCP Pfad vorhanden, Library-Panel fehlt | post-MVP | `lumina-gui`/`lumina-sidecar` | — |
| L-11 | **Filterleiste `\\` nach Brennweite/Kamera/ISO/Metadaten** | Kein Filterbar-Widget, keine EXIF-Filterlogik in GUI | — | **fehlt** | post-MVP | `lumina-gui` | `\\` ungebunden |
| L-12 | **Virtuelle Kopien Strg/Cmd+'** | `virtual_copies` stabile ID, Standardkopie, `duplicate_virtual_copy`/`select_virtual_copy`/`history`, Preset `.lumina-preset.json` dateibasiert (F-009/F-014 verifiziert) | Sidecar 86p | **teilweise** — Core/Sidecar vollständig, GUI Shortcut fehlt | MVP | `lumina-sidecar`/`lumina-gui` | `Cmd+'` ungebunden, nur API |
| L-13 | **Import/Export Batch Shortcuts Cmd+Shift+I/E** | CLI `import`/`batch`/`export` vorhanden (R2-CLI), GUI Import via `open_file`/`load_bytes`/`begin_load_path` + Drag&drop; Export Modul via `export_image` byte-identisch CLI | CLI E2E `mask_warnings` | **teilweise** — CLI ja, GUI Shortcut fehlt, `batch` Shortcut ungebunden | MVP | `lumina-cli`/`lumina-gui` | ungebunden |

**Bibliothek-Fazit:** Folder-Navigation + Sidecar-Scan + Preset-Liste vorhanden; Bewertung/Flag/Label/Filter/Stack/Smart-Sammlungen = größter Library-Gap (Metadaten-Only, keine Persistenz).

---

## 2 Entwickeln (Develop) — SOLL vs Ist

SOLL laut Auftrag: Belichtung/Kontrast/Lichter/Tiefen/WB/Klarheit/Dynamik/Gradationskurve; Maskierung & KI Pinsel/Verlauf/Radial + KI Personen/Motiv/Himmel (BiRefNet/SAM); HSL, Color Grading; Reparieren KI Bereichsreparatur/Objektentfernung/Rote-Augen; Details/Geometrie Schärfen/Rausch inkl. KI/Objektiv/Upright; HDR/Panorama DNG Cmd+H/M; KI-Unschärfe/Entrauschen, Schnappschüsse `Cmd+Alt+S`, Shortcuts D/R/Q/K/M/Y/V/J/L/Cmd+Shift+C/V, Alt+Regler, Tab, F, etc.

| # | Lightroom SOLL | LuminaRust Ist | Pipeline/Sidecar Feld | Tests | Gap | Pri | Crate |
|---|---|---|---|---|---|---|---|
| D-01 | **Grundeinstellungen** Exposure/Contrast/Highlights/Shadows/Whites/Blacks | F-036 `EditRecipe.adjustments` Keys vorhanden, `apply_recipe_with_white_balance` + WB `temperature 1500..12000`/`tint -1..=1` deterministisch sRGB; Pipeline `exposure→contrast→shadows/highlights→whites/blacks` normativ | `adjustments` `exposure -10..=10` etc. | core 277p `tone.rs` | **vollständig** (sRGB MVP) | MVP | `lumina-core` |
| D-02 | **WB Klarheit/Dynamik (Vibrance/Saturation)** | Presence `texture/clarity/dehaze` F-094 + Vibrance/Saturation F-092 als `recipe.presence`/`vibrance/saturation` vorhanden; UI `set_presence` | `presence`, `adjustments.vibrance/saturation` | — | **teilweise** — Core ja, GUI Slider vorhanden aber kein WB-Preset-Picker | MVP | `lumina-core`/`lumina-gui` |
| D-03 | **Gradationskurve** F-089 | `recipe.adjustments.curves {version, master, channels red/green/blue, points 2..32 monoton Hermite}` normativ, in `adjustments` | `curves` | — | **fehlt** (normativ dokumentiert, kein Code nachgewiesen) | MVP | `lumina-core`/`lumina-gui` |
| D-04 | **Maskierung Pinsel/Linear/Radial** K/M/Shift+M | `MaskTool::Brush/LinearGradient/Radial` + `commit_brush_stroke`/`commit_gradient`/`commit_radial` + `MaskPrompt::Brush/Gradient/Ellipse` + `rasterize_prompt` F-079 vorhanden; Overlay persistiert | `MaskPrompt`/`MaskDefinition.prompt` | — | **teilweise** — Core/GUI-Arming vorhanden, aber Geometrie blockiert (`geometry_blocks_source_mapping`), kein `K/M` Shortcut | MVP | `lumina-gui`/`lumina-core` |
| D-05 | **KI Personen/Motiv/Himmel (BiRefNet/SAM)** | BiRefNet Subject + SAM2 4 Varianten `sam2.1_hiera_*` via `lumina-onnx` `MaskInference`/`PromptMaskInference`; `resolve_mask_planes` + F-051 Cache-Fallback; GUI `create_mask` Pending→Valid via geometrischen Fallback | `ModelManifest`/`ModelIdentity` + `.lumina.zdata` | onnx-rt 107p | **teilweise** — Inferenz vorhanden (Stub + echter ORT via `LUMINA_MODEL_PATH`), GUI Kategorie-Buttons (Person/Himmel) fehlen | MVP | `lumina-onnx`/`lumina-core` |
| D-06 | **HSL** F-090 8 Kanäle | Normativ `recipe.adjustments.hsl {version, 8× hue/sat/lum}` in `pipeline.md` §F-090 | `hsl` nested | — | **fehlt** | MVP | `lumina-core` |
| D-07 | **Color Grading** F-091 Schatten/Mitten/Lichter | Normativ `recipe.adjustments.color_grading {shadows/midtones/highlights hue 0..360 sat 0..=1 balance -1..=1}` §F-091 | `color_grading` | — | **fehlt** | MVP | `lumina-core` |
| D-08 | **Reparieren: KI Bereichsreparatur / Objektentfernung** | Spot-Heal **fehlt** — nur generische `SourceActionArtifact {region: MaskPlane u16, replacement: RGBA8}` full-frame + `dust-removal` CLI; SPOT-REMOVE-1 + GEN-EXPAND-1 Doku-first, kein `SpotRemoval`/`GenerativeEdit` | `source_actions` (F-042) | `source_action_*` 3 Tests | **fehlt** — Doku-first G5/G6 | high | `lumina-sidecar`/`lumina-core`/`lumina-onnx` |
| D-09 | **Rote-Augen** | Kein Red-Eye Feld/Algorithmus | — | — | **fehlt** | post-MVP | `lumina-core` |
| D-10 | **Details Schärfen** F-095 | Normativ `recipe.adjustments.sharpening {amount 0..=3 radius 0.1..=10 detail/masking 0..=1}` Unsharp-Mask | `sharpening` | — | **fehlt** (Spec vorhanden, Code nicht nachgewiesen) | MVP | `lumina-core` |
| D-11 | **Rauschreduzierung inkl. KI** F-096 | Deterministischer 5×5 kantenbewusst vorhanden als Spec; KI-Denoise Post-MVP (F-096) | `noise_reduction {luminance/color 0..=1}` | — | **teilweise** — manuell Spec, KI fehlt (F-070 off) | post-MVP | `lumina-core`/`lumina-onnx` |
| D-12 | **Objektivkorrektur** F-098 + **Lensfun** | `LensCorrection {distortion_k1..k3, vignette_c0..c2, ca_red/blue}` + Lensfun dynamisch `lumina-lensfun` Feature `lensfun` (Mutex wegen 0.3.4 Regex-Race) | `lens_correction` | 6 native + feature-gated | **vollständig** (mvp) | MVP | `lumina-sidecar`/`lumina-core`/`lumina-lensfun` |
| D-13 | **Upright/Perspektive** F-099 | `Perspective {vertical/horizontal/rotation/scale/aspect/shift_x/y}` Homographie | `perspective` | — | **teilweise** — Rezept vorhanden, GUI-Regler ja, Auto-Upright Analyse post-MVP | MVP | `lumina-core` |
| D-14 | **Zuschneiden/Rotation** F-093 | `Geometry {crop Aspect/Free, rotation -180..=180, mirror_h/v}` + `apply_geometry` 5-in-1 (enthält Lens+Perspective) | `geometry` | — | **teilweise** — Core ja, Pipeline 5-in-1 statt entkoppelter Stufen (G2) | MVP | `lumina-core`/`lumina-gui` |
| D-15 | **Vignette/Grain** F-097 | `Effects {vignette amount -1..=1, grain seed u64}` als letztes Adjustment vor Masks | `effects` | 13 Tests (11 core +2 sidecar) | **vollständig** | MVP | `lumina-core` |
| D-16 | **HDR/Panorama Merge Cmd+H/M → DNG** | Kein Merge, kein DNG-Writer; `export_image` nur sRGB PNG/JPEG/WebP, keine 16-bit DNG | — | — | **fehlt** | post-MVP | `lumina-core` |
| D-17 | **KI Entrauschen / KI-Unschärfe** | Kein KI-Denoise/`inpaint` Modell beyond BiRefNet/SAM; `capability-matrix.md` listet `onnx-wasm` off | — | — | **fehlt** | post-MVP | `lumina-onnx` |
| D-18 | **Schnappschüsse & Protokoll `Cmd+Alt+S` / History** | `VirtualCopy.history: Vec<HistoryEntry>` + `restore_history` + `history_selected` vorhanden; Snapshot ≠ History (Lightroom Snapshots sind benannte Rezept-Freeze-Punkte) | `history` | — | **teilweise** — History ja, Snapshots fehlen | MVP | `lumina-sidecar`/`lumina-gui` |
| D-19 | **Vorher/Nachher Y / Shift+Y, V, J, L** | `before_after: bool` + `update_texture` pfad vorhanden; `Y` Handling existiert (Before/After Toggle) | — | kittest 5 | **teilweise** — Y vorhanden, V/J/L fehlen | MVP | `lumina-gui` |

**Develop-Fazit:** Exposure/WB/Lens/Vignette/Grain/Mask-Geometrie + ONNX Stub/ORT vorhanden; Kurve/HSL/ColorGrading/Sharpen/NoiseReduction/Spot-Heal/RedEye/HDR/Dehaze-GUI/KI-Denoise = größte Develop-Gaps (F-089/090/091/094/095/096 teilweise unimplementiert).

---

## 3 Shortcut-Abgleich (verbindlich)

| Gruppe | Shortcut | Lightroom Bedeutung | LuminaRust Bindung | Datei | Status |
|---|---|---|---|---|---|
| Module | `G` | Library Grid | `module_for_key(G)→Library` | `lib.rs:121` | **gebunden** |
| | `E` | Loupe | Alias `E→Library` (kein separates Loupe-Modul) | `lib.rs:123` | **gebunden (alias)** |
| | `D` | Develop | `module_for_key(D)→Develop` | `lib.rs:122` | **gebunden** |
| Bibliothek | `C` | Compare | nicht gemappt (`_→None`) | `lib.rs:125` | **fehlt** |
| | `N` | Survey | nicht gemappt | — | **fehlt** |
| Bewertung | `1-5` | Sterne | ungebunden (kein Sterne-Feld) | — | **fehlt** |
| | `6-9` | Farb-Label | ungebunden | — | **fehlt** |
| | `P` / `X` / `U` | Pick/Reject/Unflag | ungebunden | — | **fehlt** |
| Organisation | `Cmd/Ctrl+G` | Stack | ungebunden | — | **fehlt** |
| | `Cmd/Ctrl+'` | Virtuelle Kopie | nur `duplicate_virtual_copy` API, kein Key | `lib.rs:1345` | **fehlt (API vorhanden)** |
| | `Cmd/Ctrl+Shift+I` | Import | CLI `import`, GUI kein Shortcut | — | **fehlt** |
| | `Cmd/Ctrl+Shift+E` / `E` | Export | GUI `export_path/format/quality` Modul, kein Shortcut | — | **fehlt** |
| Develop | `R` | Crop | ungebunden (Crop via Panel) | — | **fehlt** |
| | `Q` | Spot Removal | ungebunden (SPOT-REMOVE-1 Doku-first) | — | **fehlt** |
| | `K` / `M` / `Shift+M` | Pinsel/Verlauf/Radial | `MaskTool` existiert, aber per Panel-Toggle, kein Key `K/M` | `lib.rs:135` | **fehlt (Tool vorhanden)** |
| | `Y` / `Shift+Y` | Before/After | `before_after` Toggle vorhanden (`Y` in `update`) | `lib.rs:443,917` | **teilweise** — `Y` gebunden, `Shift+Y` fehlt |
| | `V` | SW | ungebunden | — | **fehlt** |
| | `J` | Clipping warn | ungebunden | — | **fehlt** |
| | `L` | Lights Out | ungebunden | — | **fehlt** |
| | `Cmd/Ctrl+Shift+C` / `V` | Copy/Paste Settings | ungebunden (nur `apply_preset`, `apply_adjustment_to_selection`) | `lib.rs:2017,1272` | **fehlt** |
| | `Alt+Regler` | Feinjustierung | `slider.rs` `Alt`-Scroll dokumentiert (F-100) | `slider.rs` | **teilweise** — Alt-Scroll ja, Alt+Drag fehlt |
| | `Tab` / `Shift+Tab` | Panels ein/aus | ungebunden | — | **fehlt** |
| | `F` | Vollbild | ungebunden (eframe Fenster) | — | **fehlt** |
| | `Alt+Reglerklick` | Reset | Doppelklick Reset vorhanden (F-100), Alt-Klick Reset ungebunden | `slider.rs` | **teilweise** |
| | `Shift+Doppelklick` | Auto Weiß/Schwarz | Auto-Tone `auto_tone()` Button vorhanden, kein Shift+Doppelklick | `lib.rs:2231` | **fehlt** |
| Filter | `\\` | Filterleiste | ungebunden | — | **fehlt** |
| History | `Cmd/Ctrl+Alt+S` | Schnappschuss | nur `HistoryEntry`, kein Snapshot-Key | — | **fehlt** |
| HDR/Pano | `Cmd/Ctrl+H` / `M` | HDR/Pano Merge | ungebunden | — | **fehlt** |

**Shortcut-Fazit:** Nur `G/D/E` + `Y` (+ Alt-Scroll/Doubleklick) gebunden; ~25 Lightroom-Shortcuts fehlen oder nur API-seitig vorhanden. MVP-Pflicht: `1-5/P/X/U` + `K/M/Q/R` + Copy/Paste + `Cmd+'` sollten vor Post-MVP Shortcuts priorisiert werden.

---

## 4 Priorisierte Lückenliste (PRIO hoch/mittel/niedrig)

Legende: Aufwand `S<1d, M1-3d, L>1w`; Risiko `Lizenz/Modell/Performance/UX-Break`; Abhängigkeit = seriell vor paralleler Welle; Crate exklusiv pro Agent.

| # | PRIO | Lücke | SOLL-Anker | Crate | Aufwand | Risiko | Abhängigkeit | Ist-Gap |
|---|---|---|---|---|---|---|---|---|
| **LR-01** | **hoch** | **Bewertung/Flag** (`1-5` Sterne + `P/X/U` Pick/Reject) Sidecar-Felder + GUI + Shortcuts | Library SOLL; `sidecar.md` Schema v2 | `lumina-sidecar` (Schema) + `lumina-gui` (Panel) | M | UX-Break gering (Pre-MVP Schema-Break erlaubt, `schema_version` bleibt 1) | G16 Schema-Bump seriell vor GUI | **fehlt** |
| **LR-02** | **hoch** | **Gradationskurve F-089** Core-Stufe + GUI Kurven-Editor | `pipeline.md` F-089 | `lumina-core` + `lumina-gui` | M | Performance (Kurve vor HSL) | LR-07 Render-Reihenfolge | **fehlt** |
| **LR-03** | **hoch** | **HSL F-090 (8 Kanäle)**  + **ColorGrading F-091** | `pipeline.md` F-090/091, F-100 Section-Reihenfolge | `lumina-core` | M | — | LR-02 | **fehlt** |
| **LR-04** | **hoch** | **Schärfen F-095 + Rauschreduzierung F-096 manuell** (Unsharp-Mask, 5×5 Luminanz/Chroma) | `pipeline.md` F-095/096 | `lumina-core` | M | Performance (Draft muss ohne Sharpen) | — | **fehlt** |
| **LR-05** | **hoch** | **Spot-Heal schnell (heuristisch Clone, Q)** + Shortcut `Q` | `spot-removal.md` Heuristic; `pipeline.md` F-042; `gap-generative-fill` G5 | `lumina-core` (`HealingPass`) + `lumina-gui` | M | Performance <200 ms 24 MP sonst Preview-Cache Budget | LR-11 | **fehlt** (G5) |
| **LR-06** | **hoch** | **GenerativeEdit Canvas (Auto-Fill transparent + Expand >100%, keep_generative)** — `canvas` + `auto_fill_transparent`/`expand_beyond_image`/`keep_generative` + Pipeline `Lens→GenerativeEdit→Perspective→Crop` | `generative-expand.md` GEN-EXPAND-1; `pipeline.md` 5-in-1 Entkopplung | `lumina-sidecar` (G1) + `lumina-core` (G2) | L | Lizenz F-078, RAM 45 MP Canvas 180 MiB, WASM `not available` | **Welle1** Sidecar seriell → Core → GUI | **fehlt** (G1/G2/G4) |
| **LR-07** | **hoch** | **Pipeline Entkopplung** `apply_geometry` 5-in-1 → eigene Stages Lens/Perspective/Crop + `GenerativeEdit` Stufe | `pipeline.md` Pipeline-Reihenfolge; `gap-generative-fill` G2 | `lumina-core` | M | `stage_digest`/`RenderKey` Break | LR-06 | **teilweise** |
| **LR-08** | **hoch** | **Presence/Dynamik GUI Finalisierung** F-094 Texture/Clarity/Dehaze + F-092 Vibrance/Saturation in F-100 Reihenfolge ColorGrading→Presence→Vibrance | `pipeline.md` F-094/092, `cli-gui-wasm.md` F-100 | `lumina-gui` | S | — | — | **teilweise** |
| **LR-09** | **hoch** | **Core Shortcuts Prio** `Cmd+'` (VC duplizieren) + `C/V` (Copy/Paste Settings) + `Y` Shift+Y + `V/J/L` | Develop Shortcuts, `virtual-copies.md`, `cli-gui-wasm.md` F-100 `Y` | `lumina-gui` | S | — | — | **fehlt** |
| **LR-10** | **mittel** | **Masken-Shortcuts `K/M/Shift+M/R/Q` + `Alt+Regler` + `Tab/F`** binden | `lib.rs` `MaskTool`, `slider.rs`, F-100 | `lumina-gui` | S | `geometry_blocks_source_mapping` muss Key-Gate | LR-05 | **fehlt (Tool vorhanden)** |
| **LR-11** | **mittel** | **Staub generativ lokal (ONNX Inpaint) `inpaint_heal`** + `lumina-onnx` Capability + GUI Maske malen + `kind=spot_heal_generative` | `spot-removal.md` generativ; `gap-generative-fill` G6 | `lumina-onnx` + `lumina-core` + `lumina-gui` | L | **Lizenz** (Inpaint oft CC BY-NC, SD-Inpaint Apache-2.0 Prüfpflicht, `pending-integration` bis LIZ) | LR-05/06, G13/G14 | **fehlt** |
| **LR-12** | **mittel** | **Snapshot `Cmd+Alt+S` vs History** — benannte `Snapshot` Rezept-Freeze-Punkte (≠ History) | Develop SOLL, `virtual-copies.md` History | `lumina-sidecar` + `lumina-gui` | S | — | — | **teilweise** |
| **LR-13** | **mittel** | **Quick Develop + Filterleiste `\\` + Import/Export Shortcuts** (Mehrbild `Cmd+Shift+I/E`, Filter nach Brennweite/Kamera/ISO) | Library SOLL, `cli-gui-wasm.md` CLI | `lumina-gui` + `lumina-cli` | M | CLI `reindex` nur Sidecar-Scan | — | **fehlt** |
| **LR-14** | **mittel** | **GUI-Tests Outcome ausbauen (T01-T10) + visuelle Auto-Gates** | `docs/plans/gui-tests-2026-09-02.md`, `gap-generative-fill` G10 | `lumina-gui` (`preview_ctrl`/`filmstrip`) + `lumina-core` (histogram) | M | `file_stamp` Mtime-Granularität, GPU CI-Nein (nur `cargo check`) | — | **teilweise** |
| **LR-15** | **mittel** | **RenderKey/Cache Invalidierung** für generative/Spot-Artefakte + Canvas in `stage_digest` | `pipeline.md` Reproduzierbarkeit, `generative-expand.md` Identität | `lumina-core` | S | — | LR-06 | **fehlt** (G11) |
| **LR-16** | **mittel** | **Sidecar Schema Versionierung** additiv `curves/hsl/color_grading/presence/sharpening/effects` etc., Migration v1→v2 mit Backup | `sidecar.md` Migration, `feature/README.md` Pre-MVP Break-Erlaubnis | `lumina-sidecar` | S | Pre-MVP `schema_version` bleibt 1, Break ohne Migration erlaubt | vor LR-02/03 | **teilweise** |
| **LR-17** | **niedrig** | **Farb-Label `6-9`, Stack `Cmd+G`, Kataloge/Smart-Sammlungen/Index** | Library SOLL, Phase 9 Index Post-MVP | `lumina-sidecar` + `lumina-gui` | M | — | — | **fehlt** (post-MVP) |
| **LR-18** | **niedrig** | **HDR/Panorama DNG Merge `Cmd+H/M` + Rote-Augen** | Develop SOLL, `export.md` nur sRGB 8-bit | `lumina-core` | L | DNG-Writer, 16-bit Pipeline (Post-MVP) | — | **fehlt** |
| **LR-19** | **niedrig** | **KI Entrauschen/Unschärfe + Capability-Matrix `inpaint/outpaint` (lokal vs Cloud getrennt) + WASM `onnx-wasm` off** | `capability-matrix.md`, `wasm-limits.md`, SPOT-REMOVE-1 | `lumina-onnx` + `capability-matrix.md` | S | Cloud-Falle (kein stiller Fallback) | LR-11 | **fehlt** (G13) |
| **LR-20** | **niedrig** | **Library Compare/Survey `C/N` + Lights Out `L` + Clipping `J`** | Library/Develop SOLL | `lumina-gui` | M | — | — | **fehlt** |

Wellen-Vorschlag (Ein-Crate-Regel):
- **Welle 1 seriell:** LR-16 Sidecar Schema (`lumina-sidecar`) + LR-06 zdata Canvas + LR-15 RenderKey
- **Welle 2:** LR-07 Pipeline Entkopplung + LR-02/03/04 Core Adjustments (`lumina-core`)
- **Welle 3:** LR-01 / LR-08 / LR-09 / LR-10 / LR-14 GUI (`lumina-gui`) — ein schreibender Agent
- **Nach Lizenz:** LR-11 Inpaint (blockiert auf LR-19/G14 Lizenz-Pin)

---

## 5 Visuelle Auto-Verifikation je Gap (keine manuelle Prüfung)

Grundsatz: Deterministische Fixture + Seed + Golden/PSNR-Gate + Histogram-Digest; `UPDATE_SNAPSHOTS=true` nur lokal (GPU), CI nur `cargo test` (ohne `--ignored`), `wasm32` `cargo check` grün halten (Agents.md Verifizierung).

| Gap | Ansatz | Crate | Toleranz/Gate |
|---|---|---|---|
| LR-01 Sterne/Flagge | Unit Sidecar Roundtrip (5/Flagge P/X/U), kittest Library Grid mit Badge (Sternzahl/Farbpunkt) | `lumina-sidecar` + `lumina-gui` kittest | byte-identisch Sidecar, Snapshot diff `tests/snapshots/library_*` |
| LR-02 Kurve | Unit monotone Hermite kein Overshoot, Golden sRGB Ramp vor/nach Kurve | `lumina-core` `curves` | 1/255 pro Kanal |
| LR-03 HSL/ColorGrading | Unit 8 Zentren Nachbarübergang zyklisch, Golden ColorChecker (Hue-Dreh 30°) | `lumina-core` | PSNR > 40 dB vs CPU-Referenz, `stage_digest` Miss bei Kanal-Change |
| LR-04 Sharpen/Noise | Unit Unsharp `masking` Flächen vs Kanten, Golden Checker 24 MP Draft vs Full | `lumina-core` | PSNR Gate, `sharpening_render_scale_changes_render_only` analog |
| LR-05 Heal schnell | Golden 8×8 Spot auf Checker, Heal deterministisch; kittest Dust Panel (Q Toggle) | `lumina-core` + `lumina-gui` | **byte-identisch** (kein Modell, deterministisch) |
| LR-06 Generative Canvas | Golden synthetische Lens-Keile (15% Distortion auf Checker) → Auto-Fill; Expand Canvas >100% mit Seed-Pin; kittest Expand-Rahmen vor/nach Bestätigen | `lumina-core` + `lumina-gui` | PSNR > 35 dB (Seed pin **Pflicht**, sonst flakey), BLAKE3 Artefakt |
| LR-07 Pipeline | `Pipeline::validate` + `render_frame_from_base` byte-identisch vor/nach Entkopplung, `stage_digest` Trennung | `lumina-core` | unit `stages()` exakt |
| LR-08 Presence/Vibrance | Unit `presence` -1..=1, Golden Hautschutz (Hue 15..55°) | `lumina-core` | Delta < 1/255 außerhalb Haut |
| LR-09/10 Shortcuts | Headless `module_for_key` + `set_mask_tool` + `commit_brush_stroke` Unit; kittest Before/After `Y` hält Rezept unverändert (generational bump) | `lumina-gui` | `preview_generation` bump, Shortcut-Keycode exakt |
| LR-11 Inpaint generativ | StubInpaint Backend deterministisch (Crafted ONNX ReduceMax 1024²), hash-gepinnte Fixture `pending-integration` bis Modell committet | `lumina-onnx` (`onnx-rt`) | PSNR-Gate, `artifact_status` Corrupt eager BLAKE3 |
| LR-12 Snapshot | Roundtrip `snapshot: {id,name,recipe}`; kittest History Panel | `lumina-sidecar` | JSON roundtrip |
| LR-13 Filter/QuickDev | Unit `apply_adjustment_to_selection` auf 3 Sidecars batch, Filter `focal==50mm` synthetische EXIF | `lumina-sidecar` | `-batch --jobs` deterministisch |
| LR-14 GUI Outcome | T01-T10 `preview_ctrl` (LRU vor Disk, Stale-Badge, korrupt `.tmp` nie Hit, OneToOne Invalidierung, Prio +1>+2>-1>+3>-2>+4, WB-Gate) | `lumina-gui`/`lumina-core` | `preview-cache-baseline/budgets.json` `compare.mjs --mode warn` 6/6 OK |
| LR-15 RenderKey | Unit `source_action_artifact_hashes` + `canvas` in `stage_digest` (non-decode), `export_options` analog | `lumina-core` `pipeline.rs` | `digest !=` bei Hash-Wechsel, `decode` digest unverändert |
| LR-19 Capability | `cargo check --target wasm32-unknown-unknown -p lumina-onnx --features onnx-rt` grün (`RuntimeDisabled`), Matrix Zeile `inpaint` | `capability-matrix.md` | wasm Gate grün |
| Allgemein | Golden-Image Toleranz: `LuminanceHistogram::digest` + `analyze_tone` Median/p01/p99 Delta `≤1/256` (R2-PERF-01); Dithering/Seed deterministisch (`seed` + `image` Encoder) | `lumina-core` | 1/256 Schranke |

Kein manueller Screenshot als Gate: Jeder visuelle Test braucht committeten Golden + `UPDATE_SNAPSHOTS` Doku, sonst `#[ignore]`.

---

## 6 Offene Risiken — vor Umsetzung zu entscheiden

1. **Pipeline-Platzierung „nach Lens" (Wunsch) vs GEN-EXPAND-1 „vor Lens" (Canvas vor Geometrie):** Auto-Fill als post-Geometry Inpaint (im gleichen Canvas) von Outpaint (>100% Canvas-Expand) trennen — ersteres Wunsch 1, letzteres Wunsch 2 (siehe `gap-generative-fill` §6-1).
2. **Canvas-Koordinaten Break:** `crop_rect` rechnet heute auf Quell-`width/height`; bei Canvas>Quelle muss auf Canvas rechnen — Geometry-Hash ändert sich. Pre-MVP Break ohne Migration erlaubt (`Agents.todo.md` Gepinnte), aber `schema_version` Bump-Entscheidung dokumentieren.
3. **Modell-Lizenz Inpaint/Outpaint:** Viele SOTA Inpaint-Modelle CC BY-NC/AGPL (Analogon `ultralytics` in `fixtures-licensing.md` §5) — kein Code vor Hash/Lizenz-Pin (F-078), `pending-integration` bis LIZ.
4. **Performance:** 24 MP Canvas Expand 96 MiB + 45 MP Desktop Voll-Canvas 180 MiB + Preview-Cache 7 Slots 1.5 GiB + VRAM 1024/4 → 8 GB GUI Budget nur knapp nicht gerissen — F-074 `compare.mjs gate:true` erst nach Kalibrierung.
5. **Seed-Pflicht:** Inpaint ohne `seed: u64` Pflicht nicht deterministisch — Golden flakey (LR-11 Gate braucht Seed-Pin).
6. **WASM Parity:** `zdata`/`zstd` native-only, generatives Canvas auf WASM `not available` — Capability-Anzeige muss `inpaint` als nicht verfügbar ausweisen, sonst stiller Fallback (Agents.md).
7. **Shortcut-Kollision:** `Cmd+H/M` (HDR/Pano) kollidiert mit macOS Hide/Minimize — alternative Belegung oder Menü-only prüfen.

---

## 7 Referenzen (SOLL-Docs)

- `feature/README.md` — Index, Invarianten, Feature-Matrix (F-089..099, F-100)
- `feature/platform/cli-gui-wasm.md` (F-100) — UI-Konventionen, Develop-Sektionen Reihenfolge, `Y`/`Y`-Toggle, Module Library/Develop/Export
- `feature/architecture/pipeline.md` (F-089..099, F-036, F-042, F-041) — Pipeline `Decode→SourceActions→…→Crop→Output`, Arbeitsfarbraum sRGB, Kurve/HSL/ColorGrading/Presence/Sharpen/Noise/Vignette/Lens/Perspective, `stage_digest`
- `feature/architecture/sidecar.md` (F-001) — `.lumina.json` + `.lumina.zdata` (BLAKE3/zstd, relativ, atomar), `artifact_status`
- `feature/product/virtual-copies.md` (F-002/F-014/F-009) — stabile ID, Standardkopie, Presets `<name>.lumina-preset.json`
- `feature/product/ai-masks.md` (F-004, F-079..F-083) — Maskenidentität, BiRefNet/SAM2, `lumina-onnx` Capabilities
- `feature/product/spot-removal.md` (SPOT-REMOVE-1) — heuristisch vs generativ `spot_heal_generative`
- `feature/product/generative-expand.md` (GEN-EXPAND-1) — `GenerativeEdit` Canvas >100%, `auto_fill_transparent`/`expand_beyond_image`/`keep_generative`
- `feature/product/export.md` (F-037) — sRGB 8-bit, `export_image` byte-identisch GUI/CLI
- `feature/platform/capability-matrix.md` + `feature/platform/wasm-limits.md` (F-069..F-071) — `zdata`/`onnx` native-only, `onnx-wasm` off, Limits 45/24 MP, 8 GB/512 MiB/48 MiB, VRAM 1024/4, LibRaw 0.22.2
- `feature/quality/preview-cache.md` + `feature/quality/performance-benchmarks.md` (F-074) — Preview-Cache Budgets, `compare.mjs`
- `docs/plans/gap-generative-fill-transparent-2026-09-02.md` (28b7782) — G1-G15, Welle 1 Sidecar→Pipeline→GUI
- `docs/plans/gui-tests-2026-09-02.md` — T01-T10 Outcome-Tests
- `crates/lumina-gui/src/lib.rs` — `module_for_key`, `MaskTool`, `LuminaApp` Sections, `IdleQueue`, kittest

---

**Pfad:** `docs/plans/gap-lightroom-parity-2026-09-02.md`  
**Summary:** Bibliothek: Folder/Entries vorhanden; Bewertung/Flag/Label/Filter/Stack/Smart-Sammlungen fehlen (LR-01/LR-17). Entwickeln: Exposure/WB/Lens/Vignette/Grain/Mask-Brush vorhanden (teilweise); Kurve/HSL/ColorGrading/Presence/Shapen/Noise/Spot-Heal/HDR/RedEye/KI-Denoise fehlen (LR-02..06/LR-18). Shortcuts: Nur `G/D/E/Y` + Alt-Scroll/Doppelklick gebunden; ~25 Lr-Shortcuts (1-5/P/X/U/K/M/Q/R/Cmd+'/C/V/Tab/F/Alt) fehlen. Priorisiert 20 Lücken (hoch LR-01..09, mittel LR-10..16, niedrig LR-17..20) mit Crate, Aufwand S/M/L, Lizenz/Performance-Risiko; auto-Verifikation je Gap via kittest/Golden/Histogram/PSNR/`stage_digest`/`compare.mjs` ohne manuelle Prüfung.
