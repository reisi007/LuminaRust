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
- `painter` additions (AGENT-HARNESS-4) — `PANEL_BG_RGB` / `WORKING_BG_RGB`
  (theme mirrors), `CORNER_INSET` / `CORNER_TOL`, `corner_max_abs_diff`
  (Fit-Rahmen/Chrom-Vertrag), `frame_hash_fnv1a` (stale-guard identity).

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
| `library_greenpath.rs` | AGENT-HARNESS-3 Library (Open/Select/Toggle/Range) |
| `develop_sliders.rs` | AGENT-HARNESS-3 Develop Slider-Klassen |
| `develop_actions.rs` | AGENT-HARNESS-3 Develop Aktionen (Auto-Tone/Match/WB/Rotate/Reset/Render) |
| `sync_match.rs`      | AGENT-HARNESS-3 Sync/Match-Selection |
| `zoom_pan.rs`        | AGENT-HARNESS-3 Navigator/Zoom/Pan |
| `export_greenpath.rs` | AGENT-HARNESS-3 Export |
| `error_paths.rs`     | AGENT-HARNESS-3 Fehlerpfade |
| `preview_fidelity.rs` | AGENT-HARNESS-4 Bildkorrektheit (F-100 Preview) |

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

## AGENT-HARNESS-3 Green-Path-Matrix (F-100, alle Module)

Pro Matrix-Zeile ein headless Test + DoD-§7-Mapping. GPU-Szenarien sind
`#[ignore]` (Upstream-Konvention); lokal verifiziert auf dieser Maschine
(headless wgpu), Stand der Läufe s. Tabelle. `crates/` wurde nur LESEND
genutzt; fehlende Prod-Getter sind Folgeaufgaben (s. unten), keine
Workarounds.

| Zeile (Task-Vorgabe) | Szenario/Test | Status (lokal `-- --ignored`) |
| --- | --- | --- |
| Library Open | `library_greenpath.rs`: `set_directory`, 3 PNGs gelistet | **PASS** |
| Library Select (Erstbild auto) | dto.: Auswahl = erstes Grid-Bild, Preview geladen, kein Fehler | **PASS** |
| Library Toggle/Range | dto. auf echten RAW-Kopien (`sample-data/raw/*.cr3` → tempdir, Originale nur gelesen): Toggle-Add {A,B}, Range A..C, Toggle-Remove {A,C} | **PASS** |
| Library Click-Matrix (ohne GPU) | `filmstrip_click_matrix_no_gpu` über `apply_filmstrip_click` (plain/toggle/range/unknown) | **PASS** (`cargo test`) |
| Library RAW Full-Res-Load | dto. | **OPEN**: 6032x4024-Preview > headless-Texture-Cap 2048 (echte GPUs: 8192+) — Decode+Auswahl belegt, Present braucht GPU-Maschine/kleinere Fixture |
| Develop Tonwerte (alle 6) | `develop_sliders.rs`: exposure/contrast/highlights/shadows/whites/blacks je Edit→Commit→Sidecar→Reload | **PASS** |
| Develop Präsenz (alle 3) | dto.: texture/clarity/dehaze | **PASS** |
| Develop Vibrance/Saturation | dto. | **PASS** |
| Develop WB flach + Geometrie (Rotation/Spiegel) + Einzel-Reset | dto. | **PASS** |
| Develop Kurve/HSL/Grading/Effekte/Schärfen/NR/Lens/Perspektive/Crop | dto. | **OPEN** (9 Klassen): kein öffentlicher Setter — Folgeaufgaben F-HARNESS3-1…9 |
| Develop Auto-Tone | `develop_actions.rs`: 6 Regler + Spiegel + Sidecar im Aufruf, Reload | **PASS** |
| Develop Match (Ziel) | dto.: `match_total_exposure(0.5)`, Delta persistiert | **PASS** |
| Develop WB-Pick | dto.: Punkt→Temperatur (Domäne), persistiert; Fehlerpfad s. Error-Zeile | **PASS** |
| Develop Rotate/Reset/Render | dto.: ±90° + Normalisierung (-70°), Reset→nächster Commit, `render()` Generation-Bump | **PASS** |
| Sync N Sidecars | `sync_match.rs`: {A,B}-Auswahl, Rezept auf 2 Sidecars, Status | **PASS** |
| Match N Sidecars | dto.: Median-Angleich, `match_total_exposure=true` je Sidecar | **PASS** |
| Sync/Match Fehler isoliert | dto. auf frischen Zielen {C,D} (D gelöscht): 1 applied + 1 laut, Rest läuft | **PASS** |
| Sync-Wiederholung (Befund) | dto.: 2. Sync identischer Ziele → laut `duplicate history entry id` (nicht idempotent) | **PASS (Befund)**: laut, aber legitime Wiederholung scheitert — Folgeaufgabe F-HARNESS3-10 |
| Zoom alle Stufen | `zoom_pan.rs`: Fit/25/50/75/100/200/Fit-Breite je Mapping-Test Eingabe→Tree | **PASS** (7/7) |
| Custom-Pin | dto.: `zoom_step` pinnt Custom; Navigator offen + Rect-Stroke (Painter, 1195px) | **PASS** |
| Pan pinnt Custom | dto. | **OPEN**: kein öffentlicher Pan-Setter — Folgeaufgabe F-HARNESS3-11 |
| Export byte-valide | `export_greenpath.rs`: lesbar (4x3), Quelle byte-identisch, PNG-Determinismus | **PASS** |
| Fehler: Bild fehlt | `error_paths.rs`: laut, kein Preview, kein Phantom-Sidecar | **PASS** |
| Fehler: Sidecar fehlt | dto.: Defaults per Design, kein Fehler, Status benennt Stand | **PASS** |
| Fehler: Export ohne Bild | dto.: harter Fehler, keine Datei | **PASS** |
| Fehler: Maske/Modell | dto.: Recalc ohne Auswahl → Err; frische Maske → Pending + Recalc-Angebot (nie still valide) | **PASS** |
| Fehler: WB-Punkt schwarz | dto.: Err, kein Fallback-Wert | **PASS** |
| Fehler: Auto-Tone ohne Bild | dto. | **OPEN**: `Ok(())` mit generischem Status — Folgeaufgabe F-HARNESS3-12 |

### DoD-§7-Mapping pro Zeile

1. **End-to-End-Kette (Edit→Commit→Datei→Reload):** `develop_sliders`
   (15 persistente + 6 Reload-Verdicts), `develop_actions`
   (Auto-Tone/Match/WB-Pick/Rotate/Reset je mit Sidecar- + Reload-Glied),
   `sidecar_restore` (Bestand). Klassen ohne Setter haben kein
   Edit-Glied → OPEN (s. Tabelle).
2. **Zeitbasierte Pfade:** 150-ms-Debounce (`pending_full_render`,
   `commit_pending_slider_save`, `crates/lumina-gui/src/lib.rs:6312`)
   wird via `run_steps`/`wait_quiescent` getrieben — jeder persistente
   Verdict oben hängt an einem Sidecar-Read NACH den Frames; ohne
   Commit kein PASS. Async-Decode (`begin_load_path`) ebenso
   (Library-Select, Fehler-Bild).
3. **Klassen-Vollständigkeit:** Tonwerte 6/6, Präsenz 3/3,
   Vibrance/Saturation 2/2, Zoom 7/7 + Custom, Filmstrip-Click
   plain/toggle/range/unknown; Kurve/HSL/Grading/Effekte/Schärfen/NR/
   Lens/Perspektive/Crop je als eigene OPEN-Zeile (keine Stichprobe).
4. **Log-Level:** keine neue User-Aktion harness-seitig (nur lesende/
   bestehende Aufrufe); geprüfte Aktionen loggen prod-seitig `info!`
   (Sync/Match je Bild) bzw. `trace!` (Slider-Commits) — kein neues
   `trace!`-only sichtbares Verhalten eingeführt.
5. **Spez→Test:** F-100-Sätze → Tests: „Slider speichern Sidecar"
   → `develop_sliders`; „Auto-Tone/Match/WB/Rotate/Reset/Render" →
   `develop_actions`; „Sync/Match je eigenes Sidecar, Fehler laut" →
   `sync_match`; „Zoomstufen/Custom/Navigator" → `zoom_pan`;
   „Export byte-identisch, nicht-destruktiv" → `export_greenpath`;
   „Fehler laut, nie still" → `error_paths`; „Open/Select/Toggle/Range"
   → `library_greenpath`.
6. **Gates:** `cargo test`, `cargo clippy --all-targets -- -D warnings`,
   `cargo fmt --check` (in `agent-harness/`), alle grün; GPU-Szenarien
   zusätzlich lokal `-- --ignored` grün (s. Tabelle; OPENs sind
   Verdicts, keine Gate-Failures).

### Folgeaufgaben (Prod-Getter/Setter, NICHT vom Harness geschrieben)

- **F-HARNESS3-1…9** (Slider-Klassen öffentlich machen, je `pub fn`
  statt privat `fn` in `crates/lumina-gui/src/lib.rs`): `set_tone_curve_region`
  (:4195), `set_hsl_value` (:4220), `set_color_grading_value` (:4250) +
  `set_color_grading_balance` (:4290), `set_effects_value` (:4315),
  `set_sharpening_value` (:4383), `set_noise_reduction_value` (:4407),
  `set_lens_correction_value` (:4427), `set_perspective_value` (:4524),
  Crop-Edit-Pfad (derzeit nur `toggle_crop_mode` öffentlich).
- **F-HARNESS3-10** (Sync-Idempotenz): Zweit-Sync identischer Ziele
  scheitert an `duplicate history entry id` (`apply_recipe_to_path`,
  :3875) — History-IDs pro Lauf eindeutig machen (z. B. Zähler/UUID
  statt `sync-{index}`).
- **F-HARNESS3-11** (Pan/Navigator-Geometrie): `pub fn pan_by(&mut self,
  dx: f32, dy: f32)` + lesende Getter für `preview_pan` (:835) und
  `preview_roi` (:842); ergänzt HARNESS-2-`preview_roi`/`navigator_rect_debug`.
- **F-HARNESS3-12** (Auto-Tone ohne Bild): `pub fn auto_tone` (:4552)
  gibt `Ok(())` ohne Signal zurück — `Err` oder Status statt
  generischem Default.

## AGENT-HARNESS-4 Bildkorrektheit (F-100 Preview, G-10)

Pixel-Asserts auf dem In-App-Frame (`LuminaApp::preview()`), kein reiner
Layout-Nachweis. Szenario: `tests/preview_fidelity.rs` (`preview_fidelity`,
`#[ignore]` — headless wgpu nötig); reine Schwellen-Mathematik (Gamma-Falle,
Toleranz-Konsistenz) läuft ohne GPU in `cargo test`. Neue Helfer in
`src/painter.rs`: `PANEL_BG_RGB`/`WORKING_BG_RGB` (Mirrors der privaten
`crates/lumina-gui/src/theme.rs`-Konstanten — `mod theme` ist privat, daher
Mirror-Pattern wie `BADGE_CHIP_RGB`), `CORNER_INSET`/`CORNER_TOL`,
`corner_max_abs_diff`, `frame_hash_fnv1a` (alle unit-getestet).

### Asserts + Schwellen (alle im Code begründet)

| Assert (Verdict-Name) | Schwelle | Begründung (kurz) |
| --- | --- | --- |
| `app preview frame is opaque` | alpha == 255 für 100 % der Pixel | HARNESS-2-Vertrag, auf `preview()`-Frame (nicht Composit-Readback — Texturen malen headless schwarz) |
| `center pixel delta loaded vs empty` | sRGB-Luma-Delta ≥ 0.15 | 1 LSB ≈ 0.0039; Hintergrund streut < 0.02; 0.15 ≈ 38 LSB ≈ 8× über Hintergrund, weit unter Content-Lücke |
| `composited frame corners are theme background` | max. Kanal-Abw. ≤ 6 LSB vs PANEL | Flat-Fills exakt; 6 LSB Headroom für Swapchain-Rundung/AA (≈ `spread <= 6`-Bound); Content (≥ 38 LSB weg) fällt weiter laut durch |
| `preview matches sRGB source (PSNR)` | PSNR ≥ 30 dB vs. unabhängiger `image`-Decode | Ident-Pipeline ≥ 40 dB; 30 dB ≙ RMSE ≈ 8 LSB fängt Doppel-Gamma/Profilfehler (≫ 10 dB), klemmt nicht bei Rundung |
| `thumbnail matches committed fixture (PSNR)` | dto. auf 32×24-Thumbnail + FNV-1a-Hash | Determinismus-Anker ohne Binär-Commit |
| `generation switches on image change` / `stale frame is not valued as new` | gen strikt monoton, Hash-Ungleichheit | Async-`open_file`-Pfad (A→B, versch. Geometrie/Farben): alter Frame darf nie als neuer gelten |

### Gamma-Falle (dokumentiert, gemessen)

Der Vergleich läuft in **sRGB (Gamma-Domäne), nicht linear**: Auf der
Referenz klaffen sRGB-Mittel (0.428) und Linear-Licht-Mittel (0.277) um
0.151 — ein Linear-Vergleich würde einen byte-identischen Pipeline-Output
als „falsch" verwerfen. Unit-Test `gamma_trap_…` belegt die Domänen-Differenz
(0.502 vs. 0.216 auf Mid-Gray 128); Szenario-Verdict `gamma domain gap
documented` misst sie auf der echten Referenz.

### Fixtures (committet, via Pfad — kein neues Binär)

- `LuminaApp::sample_image_png()`-Bytes (committeter Code) als unabhängige
  sRGB-Referenz: Default-Rezept rendert **byte-identisch** (PSNR=inf).
- `../crates/lumina-gui/tests/snapshots/develop_basic.png` +
  `histogram_graphic.png`: Ecken-Anker (alle vier Ecken exakt PANEL,
  Hash verankert). Byte-PSNR Shot-vs-Golden wird bewusst NICHT assertiert
  (1280×800 vs. 1024×720 — Junk-Vergleich) → OPEN-Verdict.
- Beobachtung (kein Scope): `library_with_image.png`-Ecke (1021,717) ist
  `[0,0,0]` statt PANEL — vorbestehendes Golden-Artefakt, daher als Anker
  verworfen (kein Rebaseline in diesem Task).

### Ergebnisse (lokal headless wgpu, diese Maschine)

14× PASS (u. a. opaque 1.0, Delta 0.693 ≥ 0.15, PSNR=inf, Thumb-Hash
`0x74937359ff531256` idle-stabil, gen 1→3 mit Hash-Wechsel 4×3→8×6),
3× OPEN (s. unten). `cargo test --test preview_fidelity -- --ignored` grün.

### Ehrliche OPENs (bleiben OPEN, mit Grund)

- `preview frame corners carry letterbox background`: **Design-Widerlegung,
  kein Bug** — der Pipeline-Output hat per Design kein Letterbox
  (Quellgeometrie, Zuweisung `crates/lumina-gui/src/lib.rs:5382`; Fit passiert
  draw-seitig per `preview_draw_dims` `:6717`). Ecken = Content
  (`[20,30,40]…[240,240,240]`). Der Background-Vertrag lebt in der
  Composit-Ebene und ist dort PASS-belegt.
- `composited shot PSNR vs golden`: Geometrie-Mismatch s. oben.
- `composited center shows content`: HARNESS-2-OPEN bleibt OPEN (loaded
  mean=0.165 vs. empty 0.166 — Textur schwarz headless); In-App-Beleg s.
  Center-Delta (PASS).

### Folgeaufgaben (prod Getter, NICHT vom Harness geschrieben)

- **F-HARNESS4-NR-1** (Letterbox-Präzision): `crates/lumina-gui/src/lib.rs`
  (Zeichnung um `:6716`, Felder `preview_pane_w/h` `:6705`, privat ohne
  Getter) → Vorschlag: `pub fn preview_pane_rect(&self) -> Option<[f32; 4]>`
  (Pane in Punkten) — damit der Fit-Rahmen (WORKING-Bars) koordinatenscharf
  statt nur per Fenster-Ecken prüfbar wird.
- **F-HARNESS4-NR-2** (Theme-Vertrag modellseitig): `crates/lumina-gui/src/theme.rs`
  (`mod theme`, `:23` in `lib.rs`, privat) → Vorschlag: `pub mod theme`
  re-exportieren oder `pub fn working_bg(&self) -> [u8; 3]`, damit der
  Background-Vergleich gegen das echte Prod-Symbol statt Mirror läuft.

## Gates

```bash
cargo test                # unit helpers (GPU scenarios ignored)
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```
