# agent-harness

Standalone instrumenting GUI harness for the open LuminaRust GUI tasks
(`Agents.todo.md`: TOAST-OVERLAP, ZOOM-CUSTOM, RIGHT-THUMB, FILMSTRIP-DUP,
NAV-RECT, PREVIEW-NOISE, SIDECAR-RESTORE, OPTICS, SUBFOLDERS).

## Scope rule

This directory is the **only** place this agent writes. `crates/`,
`feature/`, `Agents.todo.md`, `DoD.md` and the workspace `Cargo.toml` are
untouched (another agent owns `crates/lumina-gui`). The crate has its own
`[workspace]` so the main workspace is unaffected, and depends on
`lumina-gui` + `egui_kittest` via path/version dependencies only.
No tokio, no networking — in-process harness.

## Harness API (`src/lib.rs`)

- `GuiProbe::new()` — headless wgpu harness, 1280x800, `pixels_per_point=1`
  (builder pattern from `crates/lumina-gui/tests/kittest_snapshots.rs`).
- `GuiProbe::develop_with_sample()` — sample image loaded in Develop.
- `wait_quiescent(max_steps, stable)` — run frames until
  `preview_generation` is stable.
- `render_png(path)` — on-demand frame readback (`Harness::render`) as PNG.
- `ui_tree_json()` / `tree_nodes()` — AccessKit tree via the kittest
  query API as JSON / owned records.
- `click_label_contains()` — scripted clicks.
- `intersection_area`, `frame_stats`, `center_stats`, `Verdict`,
  `artifact_dir`, `save_report` — geometric/pixel/verdict helpers.

## Scenarios (`tests/`, one per open GUI task)

| Test file            | Task                  |
| -------------------- | --------------------- |
| `toast_overlap.rs`   | GUI-TOAST-OVERLAP-1   |
| `zoom_custom.rs`     | GUI-ZOOM-CUSTOM-1     |
| `right_thumb.rs`     | GUI-RIGHT-THUMB-1     |
| `filmstrip_dup.rs`   | GUI-FILMSTRIP-DUP-1   |
| `nav_rect.rs`        | GUI-NAV-RECT-1        |
| `preview_noise.rs`   | GUI-PREVIEW-NOISE-1   |
| `sidecar_restore.rs` | GUI-SIDECAR-RESTORE-1 |
| `optics.rs`          | GUI-OPTICS-1          |
| `subfolders.rs`      | GUI-LIBRARY-SUBFOLDERS-1 |

GPU scenarios are `#[ignore]`d (upstream convention — CI without GPU stays
green); run locally with e.g.:

```bash
cargo test --test preview_noise -- --ignored --nocapture
```

Each scenario writes `artifacts/<scenario>/{shot.png,tree.json,verdict.json}`.
Checks that encode currently-open SHOULD state record `FAIL`/`OPEN`
**verdicts** instead of failing the test, so the crate gates stay green
while the verdicts document what needs fixing.

## Verification results (2026-09-03, headless wgpu, this machine)

Re-run 2026-09-03 (evening) against main `777f3e6` (8er-Fix-Batch +
HISTOGRAM-FULL): TOAST-OVERLAP, OPTICS and RIGHT-THUMB flip to PASS as
expected; ZOOM-CUSTOM, FILMSTRIP-DUP, SIDECAR-RESTORE, SUBFOLDERS stay PASS;
NAV-RECT and PREVIEW-NOISE stay OPEN+PASS with the same honest justification
(Painter-composited content is invisible to AccessKit — HARNESS-2 gap, see
below — and live GPU textures paint black in kittest readback).

| Scenario (`tests/`)      | Task | Machine verdict |
| ------------------------ | ---- | --------------- |
| `toast_overlap.rs`       | GUI-TOAST-OVERLAP-1 | **PASS**: no per-cell `"Vorschau bereit"` badge; overlay state machine (show → visible at deadline → hidden past 4s → dismiss); 3 toast nodes (message + Dismiss) exposed, all clear of the left rail |
| `zoom_custom.rs`         | GUI-ZOOM-CUSTOM-1 | **PASS**: readout `"Zoom: Fit"` after load (navigator open + History expanded — Custom-gate holds) |
| `right_thumb.rs`         | GUI-RIGHT-THUMB-1 | **PASS**: right band holds no Image node (Presets expanded) — `draw_crop_thumb` removed, no 3x duplication |
| `filmstrip_dup.rs`       | GUI-FILMSTRIP-DUP-1 | **PASS**: 3 files → 3 cells, each exactly once (RAW-only filmstrip needs `.ARW` fixtures) |
| `nav_rect.rs`            | GUI-NAV-RECT-1 | **OPEN** (rect-vs-ROI needs painted pixels) + **PASS**: Custom pins via `zoom_step` (`"Zoom: Custom"`) |
| `preview_noise.rs`       | GUI-PREVIEW-NOISE-1 | **OPEN** (composited frame paints background only headless = golden baseline) + **PASS**: app frame has content (4x3, std=0.315) — pipeline OK, fault (if any) is in display composition |
| `sidecar_restore.rs`     | GUI-SIDECAR-RESTORE-1 | **PASS** full DoD-§1 chain: edit → sidecar (`exposure=-0.62`) → fresh-probe reopen restores value; status `"Sidecar saved"` |
| `optics.rs`              | GUI-OPTICS-1 | **PASS**: `"No lens profile — automatic correction inactive …"` status row + grouped manual sliders (`Distortion (radial)`, `Vignette (light falloff)`, `CA … (lateral)`), zero `(unset)` rows |
| `subfolders.rs`          | GUI-LIBRARY-SUBFOLDERS-1 | **PASS**: flat=1, recursive=3, distinct thumb keys, symlink loop terminates, `sub` badge in tree |

All 9 `shot.png` are pairwise byte-distinct (Befund 4): every scenario
performs a genuine interaction before the readback (toast raised / Optics
expanded / Presets vs History expanded / navigator opened / directory set /
`open_file`+`set_adjustment` / `zoom_step`), so no two shots coincide without
reason. (`zoom_custom` expands History and `right_thumb` Presets deliberately,
so their shots cannot coincide.)

Cross-cutting findings: rail/filmstrip/Library-grid are RAW-only by design
(`is_raw_name` filter) — raster fixtures render no cells there; Painter-drawn
content (navigator rect, section rows partially) is invisible or
text-only in AccessKit; live GPU textures paint black in kittest readback, so
pixel-identity checks need the app-frame API (`preview()`) instead.

### Scope decision (Befund 1 — no code action)

The old scope hunk (`adopt_neighbor_preview_frame` in `crates/lumina-gui`,
PREVIEW-CACHE draft-placement) went up inside the main `777f3e6` batch and
was co-verified there (259 passed unit/integration tests incl. the draft
HUD/status instrumentation). It is therefore **not** a harness task: the
harness covers the GUI surface only, and the batch's own tests own that
logic. No file in `agent-harness/` needed a change for it.

### HARNESS-2 gap (honest OPEN, unchanged)

Painter-composited pixels stay invisible to AccessKit: the navigator
viewport rect (`nav_rect`) and the composited preview content
(`preview_noise`) keep their **OPEN** sub-verdicts with PNG-vision-review
notes. Partially closed for the toast: the overlay `egui::Area` exposes its
message + Dismiss button as real tree nodes (verified: 3 nodes, all clear
of the left rail), so `toast_overlap` needs no OPEN sub-verdict. The gap
itself (no painted-pixel queries via `ui_tree_json`) remains and is the
documented reason for the two remaining OPENs.

## Gates

```bash
cargo test                # unit helpers (GPU scenarios ignored)
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```
