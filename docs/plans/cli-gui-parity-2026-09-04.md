# CLI↔GUI Feature-Parität (CLI-GUI-PARITY-1, 2026-09-04)

Quelle: Read-only-Analyse (CLI `crates/lumina-cli/src/main.rs:270-531`,
GUI-Sektionen `crates/lumina-gui/src/lib.rs`, SOLL `feature/README.md`,
`feature/architecture/pipeline.md`, `feature/platform/cli-gui-wasm.md`).
Kein Code in diesem Task; Lücken → Folge-Tasks unten.

## Matrix (Core-Feature × CLI × GUI × Rezept-Key)

| Feature | Rezept-Key | CLI | GUI | Status |
|---|---|---|---|---|
| F-036 WB+Tonwerte | `adjustments.{wb_temperature,wb_tint,exposure,…}` | `process` (4 Overrides) + `develop` (nur exposure/contrast) | `draw_basic` + Pipette + Auto-Tone | ok (partiell, Rest via Preset-JSON) |
| F-089 Kurve | `adjustments.curves` | nur Preset-JSON | `draw_tone_curve` | nur-GUI |
| F-090 HSL | `adjustments.hsl` | nur Preset-JSON | `draw_color` | nur-GUI |
| F-091 Grading | `adjustments.color_grading` | nur Preset-JSON | `draw_color` | nur-GUI |
| F-092 Vibrance/Sat | `adjustments.vibrance/saturation` | nur Preset-JSON | `draw_color` | nur-GUI |
| F-094 Presence | `adjustments.presence` | nur Preset-JSON | `draw_color` | nur-GUI |
| F-095/096 Schärfen/NR | `adjustments.sharpening/noise_reduction` | nur Preset-JSON | `draw_detail` | nur-GUI |
| F-097 Vignette/Grain | `recipe.effects` | nur Preset-JSON | `draw_effects` | nur-GUI |
| F-098 Lens | `recipe.lens_correction` | Render-Pfad, kein Set-Flag | `draw_optics` | ok (partiell) |
| F-093/099 Crop/Persp. | `recipe.geometry/perspective` | nur Preset-JSON | `draw_geometry` | nur-GUI |
| F-009 Presets | `<name>.lumina-preset.json` | `process --preset` | `draw_presets_section` | ok |
| F-002/F-014 VCs | `virtual_copies[].id` | `--virtual-copy` überall | Selektor + Duplikat + Copy/Paste + Snapshot | ok |
| F-004/F-012 Masken | `mask_layers` + zdata | `mask --update-masks`, `--mask-policy` | `draw_masking` + Overlay | ok (partiell) |
| SPOT-REMOVE-1 | CLI `source_actions`+zdata / GUI `extras["spot_removals"]` | `dust-removal` (persistiert) | `draw_spot_heal` Quick instant | Key-Split, Folge-Task |
| GEN-EXPAND-1 | `recipe.generative_edit` | kein Zugang | `draw_generative_expand` | nur-GUI (+ Doku-Widerspruch `pipeline.md:107-116`) |
| F-037 Export | RenderKey + `export_image` | `export/batch/render/process` | `draw_export_panel` | ok |
| F-006/F-064-67 Index | — (rebuildbar) | `reindex`/`validate` | nur Scan/Filter | nur-CLI (per Design ok) |
| F-101 MCP | Rezept via Tools | `mcp` (stdio) | — | nur-CLI (per Design ok) |
| History | `copy.history[]` | schreibt, kein Show/Clear | `draw_history_section` | nur-GUI-sichtbar |
| F-100 Shortcuts | `extras` | n/a | Module/Shortcuts/Navigator/Filmstrip | nur-GUI (per Design) |

Architekturgrenzen eingehalten (CLI keine Bildlogik, GUI keine Zweit-Pipeline).

## Folge-Tasks

1. `develop` auf die 4 `process`-Overrides angleichen (klein, Block A).
2. Spot-Removal Key-Vereinheitlichung + GUI-Auflösung (Block A mittel); generatives Rendering Post-MVP.
3. GEN-EXPAND-1 Doku-Widerspruch fixen (Block A klein); CLI-Anbindung Post-MVP.
4. F-089–F-099 volle CLI-Flag-Parität = Post-MVP (Preset-Roundtrip deckt Automation ab).
5. Masken-Capability-Anzeige + Mehrbild-Sync = Post-MVP (dokumentiert).
