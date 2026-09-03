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
- `painter` module (AGENT-HARNESS-2) — Painter-home evidence for content
  AccessKit cannot see: `badge_chip_pixels` / `nav_accent_pixels`
  (composited-shot presence proofs), `app_frame_stats` /
  `app_frame_center_stats` / `frame_psnr` (in-app `preview()` RGBA proofs:
  opaque, content, deterministic, edit-delta), `texts_containing`.

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
below — and live GPU textures paint black in kittest readback). AGENT-HARNESS-2
adds the painter-home pixel evidence (`src/painter.rs`) on top: badge chips
and the navigator rect stroke are proven present in composited pixels, and the
preview pipeline is proven opaque/deterministic/edit-responsive on the in-app
frame; the remaining OPENs are the exact-geometry / composited-content statements
that pixels-alone cannot carry (see HARNESS-2 table).

| Scenario (`tests/`)      | Task | Machine verdict |
| ------------------------ | ---- | --------------- |
| `toast_overlap.rs`       | GUI-TOAST-OVERLAP-1 | **PASS**: no per-cell `"Vorschau bereit"` badge; overlay state machine (show → visible at deadline → hidden past 4s → dismiss); 3 toast nodes (message + Dismiss) exposed, all clear of the left rail |
| `zoom_custom.rs`         | GUI-ZOOM-CUSTOM-1 | **PASS**: readout `"Zoom: Fit"` after load (navigator open + History expanded — Custom-gate holds) |
| `right_thumb.rs`         | GUI-RIGHT-THUMB-1 | **PASS**: right band holds no Image node (Presets expanded) — `draw_crop_thumb` removed, no 3x duplication |
| `filmstrip_dup.rs`       | GUI-FILMSTRIP-DUP-1 | **PASS**: 3 files → 3 cells, each exactly once (RAW-only filmstrip needs `.ARW` fixtures) |
| `nav_rect.rs`            | GUI-NAV-RECT-1 | **OPEN** (rect-vs-ROI geometry needs painted pixels / a prod getter) + **PASS**: Custom pins via `zoom_step` (`"Zoom: Custom"`), rect stroke painted (`nav_accent_pixels`) |
| `preview_noise.rs`       | GUI-PREVIEW-NOISE-1 | **OPEN** (composited frame paints background only headless = golden baseline) + **PASS**: app frame has content (4x3, std=0.315) — pipeline OK, fault (if any) is in display composition; app frame opaque, deterministic (PSNR=inf across idle frames), edit-delta (exposure 0→2.0 changes bytes) |
| `sidecar_restore.rs`     | GUI-SIDECAR-RESTORE-1 | **PASS** full DoD-§1 chain: edit → sidecar (`exposure=-0.62`) → fresh-probe reopen restores value; status `"Sidecar saved"` |
| `optics.rs`              | GUI-OPTICS-1 | **PASS**: `"No lens profile — automatic correction inactive …"` status row + grouped manual sliders (`Distortion (radial)`, `Vignette (light falloff)`, `CA … (lateral)`), zero `(unset)` rows |
| `subfolders.rs`          | GUI-LIBRARY-SUBFOLDERS-1 | **PASS**: flat=1, recursive=3, distinct thumb keys, symlink loop terminates, `sub` badge in tree + badge-chip pixels (painter-home) |

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

### HARNESS-2 gap (AGENT-HARNESS-2 analysis, 2026-09-03)

Painter-composited pixels stay invisible to AccessKit — immediate-mode
`painter().text()` / `rect_stroke()` create no AccessKit nodes (only widgets
do), and the preview photo is a GPU texture that paints black in kittest
readback. No semantic AccessKit exposition is possible harness-side (that
would need production widgets); the verdict home for these contents is
therefore pixels, not the tree:

| Content | Tree-exposable? | Painter-home evidence (PASS-capable) | Still OPEN + why |
| ------- | --------------- | ------------------------------------ | ---------------- |
| Library path badge | No — `painter().text()` over a chip (`LIBRARY_BADGE_BG`, `crates/lumina-gui/src/lib.rs`), tooltip only on hover | `badge_chip_pixels` in `subfolders.rs` (`path badge pixels (painter-home)`): fixtures carry no ratings/flags/labels, so `0x42`-chip pixels are path badges | OPEN fallback only when chips are unscrolled/absent headless (vision review) |
| Navigator viewport rect | No — 2px ACCENT `rect_stroke` (`draw_navigator_viewport`), no widget | `nav_accent_pixels` in `nav_rect.rs` (`navigator rect stroke painted (painter-home)`): presence proof + tree preconditions (Custom pinned, Navigator open) | `navigator rect vs ROI` stays OPEN: exact geometry (smaller than overview, tracks pan) needs vision review or the prod getters below |
| Preview photo content | No — native texture via `painter().image`, black headless | `preview_noise.rs` on the in-app `preview()` RGBA frame: `app preview frame is opaque`, `app preview center holds image content`, `app preview frame is deterministic` (PSNR=inf across idle frames), `edit changes preview pixels` (exposure 0→2.0) | `preview shows content` (composited) stays OPEN when the readback paints background-only headless — pipeline vs composition fault split is the honest statement |

Helpers live in `src/painter.rs` (unit-tested, `cargo test`): `count_near_color`,
`badge_chip_pixels` / `nav_accent_pixels` (+ `MIN_*` thresholds with
rationale), `app_frame_stats` / `app_frame_center_stats` (luma, gray fraction,
opaque fraction), `frame_psnr` (inf = byte-identical), `combined_text` /
`texts_containing` (label+value tree search).

Partially closed for the toast (unchanged): the overlay `egui::Area` exposes
its message + Dismiss button as real tree nodes (verified: 3 nodes, all clear
of the left rail), so `toast_overlap` needs no OPEN sub-verdict.

### HARNESS-2 Folgeaufgaben (prod hooks, NOT written by this harness)

Exact rect-vs-ROI geometry and the badge data model need small read-only
production getters (`crates/` is owned by another agent — proposals only):

- `crates/lumina-gui/src/lib.rs:4815` (`roi_from_zoom`, privat) → Vorschlag:
  `pub fn preview_roi(&self) -> Option<[u32; 4]>` (aktuelle ROI-Kopie).
- `crates/lumina-gui/src/lib.rs:4999` (`navigator_viewport_rect`, privat,
  braucht nav-`Rect`+Skala+Pan) → Vorschlag:
  `pub fn navigator_rect_debug(&self) -> Option<[f32; 4]>` (letzter
  `view`-Rect in Navigator-Punkten, `None` ohne Bild/bei Fit-Vollbild).
- View-State dazu (`navigator_open`, `preview_pan`, `preview_effective_scale`,
  `preview_pane_w/h` — Felder privat ohne Getter) → Vorschlag: dieselben
  Werte im obigen Getter bündeln oder einzeln exponieren.
- `crates/lumina-gui/src/lib.rs:1083` (`FileBrowserEntry`, Feld `folder`
  privat; `pub thumb_key()` `:1235`; freie `folder_label`/`folder_badge`
  `:10188`/`:10202`) → Vorschlag: `pub fn folder(&self) -> &str`,
  damit der Badge-Text modellseitig (statt nur per Pixel/Substring) prüfbar wird.
- `crates/lumina-gui/src/lib.rs:9773` (`zoom_label`, privat) → optional;
  der Readout steht bereits als Tree-Text (`"Zoom: …"`), kein Blocker.

Local verification of the new painter-home verdicts (AGENT-HARNESS-2, this
machine, headless wgpu, `cargo test --test <scenario> -- --ignored`):
`subfolders` → 3915 chip pixels (PASS); `nav_rect` → 1223 accent pixels,
`nav_open=true`, Custom pinned (presence PASS, geometry OPEN);
`preview_noise` → opaque 1.0, center mean=0.858/std=0.142, determinism
PSNR=inf, edit-delta PSNR=9.2dB (all PASS, composited content OPEN).

## Gates

```bash
cargo test                # unit helpers (GPU scenarios ignored)
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```
