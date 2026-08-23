# Lumina GPU-First Path — Bootstrap & DAG Plan

Status: **bootstrap**. `lumina-gpu` exists as a crate with a working wgpu
context init (Metal backend) and a stable public API. The actual compute/render
stages are **not implemented yet** — `GpuContext::render_with_gpu` is a stub that
currently falls back to the CPU pipeline in `lumina-core`. This document is the
plan that parallel subagents implement against; it is the source of truth for the
GPU DAG until a dedicated feature doc is split out.

## Architecture boundary (per `Agents.md`)

- `lumina-core` stays **platform-neutral** and CPU-only (`Vec<u8>` frames). It is
  never allowed to depend on `wgpu`, `lumina-raw`, or any native/GPU crate.
- `lumina-gpu` is a **separate crate** that owns the GPU stack. It *may* depend on
  `lumina-core`/`lumina-sidecar` for types and the CPU fallback.
- GPU is the **primary** path for native GUI/CLI/MCP; CPU is the **fallback**
  when no adapter is available. The fallback must always produce a correct frame.
- The `gpu` feature gates all wgpu/bytemuck/pollster usage so the crate still
  builds on `wasm32` (where the dependency is omitted entirely) and in
  `--no-default-features` CI.

## Public API (stable, implemented in bootstrap)

```rust
pub struct Frame { pub width: u32, pub height: u32, pub pixels: Vec<u8> } // RGBA8
pub enum GpuError { AdapterUnavailable(String), DeviceUnavailable(String), Core(...) }

pub struct GpuContext { /* Option<GpuResources> under `gpu` */ }
impl GpuContext {
    pub fn new() -> Result<Self, GpuError>;          // graceful: Ok even w/o adapter
    pub fn is_available(&self) -> bool;              // true iff adapter+device bound
    pub fn render_with_gpu(&self, frame: &ImageFrame, recipe: &EditRecipe)
        -> Result<Frame, GpuError>;                  // stub → CPU fallback for now
}
```

The GPU init sequence (`init_gpu_resources`) is: `Instance` (Metal backend) →
`request_adapter` (HighPerformance, no fallback adapter) → `request_device` →
`(Device, Queue)`. Any adapter/device failure returns `Ok(context)` with
`resources = None`, so callers degrade to CPU instead of erroring.

## Planned render DAG (stages to be implemented by parallel subagents)

```
                 ┌──────────────────────────────────────────────────────┐
   RAW bytes ──► │ 1. DECODE / DEMOSAIC                                │
                 │    • decoded once per source, cached as a **base    │
                 │      texture in VRAM** (keyed by content hash)       │
                 │    • cache owner: lumina-gpu tiling/LRU module       │
                 └───────────────────────┬──────────────────────────────┘
                                         │ base texture (sampled)
                 ┌───────────────────────▼──────────────────────────────┐
                 │ 2. COLOR / TONE  (fragment shader stage)             │
                 │    • white balance, exposure, contrast, shadows,     │
                 │      highlights, whites, blacks, HSL, curves,        │
                 │      vibrance/saturation, color grading              │
                 │    • **driven by uniform buffers** (recipe → UBO)    │
                 │    • output to an **FP16 framebuffer**               │
                 └───────────────────────┬──────────────────────────────┘
                                         │ linear FP16
                 ┌───────────────────────▼──────────────────────────────┐
                 │ 3. 3D LUT  (32³)                                     │
                 │    • **baked once** from the tone/color-grade portion │
                 │      of the recipe into a 32×32×32 RGBA16F texture    │
                 │    • sampled trilinearly in the tone/look stage      │
                 └───────────────────────┬──────────────────────────────┘
                                         │ graded FP16
                 ┌───────────────────────▼──────────────────────────────┐
                 │ 4. EFFECTS + COMPOSITE                               │
                 │    • vignette, grain, noise reduction, sharpening,   │
                 │      masks/blend, geometry (crop/rotate/lens)        │
                 │    • masked layers modulate via the mask texture     │
                 └───────────────────────┬──────────────────────────────┘
                                         │
                 ┌───────────────────────▼──────────────────────────────┐
                 │ 5. TILING + PRESENT                                  │
                 │    • render in **512² tiles**, VRAM **LRU** keeps    │
                 │      hot base/effect textures resident               │
                 │    • read back to RGBA8 `Frame` for export/preview   │
                 └──────────────────────────────────────────────────────┘
```

### Draft mode (interactive preview)

- A **draft pyramid** (mip chain) of the base texture is maintained so slider
  drags render against a downsampled source at viewport resolution, then swap to
  full-res on commit. Mirrors the existing `ImageFrame::downscale` CPU quick-win
  but kept entirely in VRAM.

### Key design decisions (locked for this bootstrap)

- **Base texture cached in VRAM**, keyed by content hash, owned by the tiling/LRU
  module — never re-decoded per frame. Submitters must reuse this cache; do not
  re-create textures per `render_with_gpu` call.
- **Color/Tone as a shader stage fed by uniform buffers** — the recipe's scalar
  adjustments map to a UBO; the CPU LUT-fusion approach in `lumina-core` is the
  reference for byte-equivalence, not the GPU implementation.
- **3D LUT 32³ baked once** per recipe/grade change (not per pixel, not per
  frame). Trilinear sampling.
- **FP16 framebuffers** throughout the linear pipeline to avoid 8-bit banding
  before final encode.
- **Tiling 512² with VRAM LRU** for large (45 MP+) sources and masked layers;
  read-back only at present time.

## Ownership for parallel subagents

| Module                  | Owns                                                        |
|-------------------------|-------------------------------------------------------------|
| `lumina-gpu/src/lib.rs` | `GpuContext`, `Frame`, `GpuError`, init. **Do not break API** |
| `lumina-gpu/src/shaders.rs` | decode-base texture, color/tone UBO stage, 3D LUT bake, FP16 FBO |
| `lumina-gpu/src/tiling.rs`  | 512² tiling scheduler, VRAM LRU, draft pyramid, read-back   |

When the stages exist, `GpuContext::render_with_gpu` switches from the CPU
fallback to the GPU DAG; the CPU path remains the fallback and the test oracle.

## Feature wiring

- `lumina-gpu` `gpu` feature (default on) pulls `wgpu`/`bytemuck`/`pollster`.
- `lumina-gui` `gpu = ["lumina-gpu", "dep:bytemuck"]` (in default for native desktop; wasm32 uses
  `--no-default-features` so `lumina-gpu` is never compiled).
- CLI/MCP can opt in with `--features lumina-gpu,gpu` once they consume it.
- `lumina-gpu` is **optional and WASM-clean**: `--no-default-features` builds a pure-CPU crate,
  and `wasm32` consumers omit `lumina-gpu` entirely (no `wgpu` dependency graph).

## Dual-Backend Native: `eframe` glow (present) vs `wgpu` (offscreen compute)

> **Versionsstand (DEP-EGUI-WGPU-1, 2026-08-23):** `lumina-gui` baut auf
> `eframe`/`egui` **0.36** (weiterhin **glow**-Renderer), `lumina-gpu` auf
> `wgpu` **30**. Alle Aussagen in diesem Abschnitt (getrennte GPU-Kontexte,
> kein Surface-Sharing, `egui_wgpu` als offener Follow-up
> `GUI-WGPU-PRESENT-1`) sind durch das Upgrade **unverändert** gültig.

The native GUI (`lumina-gui`) today renders its `egui` UI via `eframe` with the
**glow** (GL) renderer (`eframe = { features = ["glow"] }`). `lumina-gpu` owns a
**separate** `wgpu::Instance` (Metal on Apple Silicon, `Backends::METAL`) and its
`Device`/`Queue` pair. They are **not the same GPU context and do not share a
surface** — a `wgpu::Texture` created from `lumina-gpu`'s Metal instance cannot be
handed to the glow swapchain or to an egui texture that will be sampled by the GL
renderer.

| Plane | Owner | Backend | Surface / Context | Shareable with `eframe` glow? |
|-------|-------|---------|-------------------|-------------------------------|
| egui UI, preview `ColorImage` | `eframe` / `LuminaApp::update_texture` | **glow** (GL) | GL swapchain | — |
| tone `output` + brush `mask` VRAM | `lumina-gpu::GpuContext::VramState` | **wgpu/Metal** | `wgpu::Instance::new(METAL)` | **No** |

### Consequences (GUI-60FPS-1)

- **Mask tiles** are correctly GPU-accelerated: the GUI keeps a persistent
  `Vec<u16>` R16 mask plane CPU-side (`LuminaApp::brush_mask_plane`), stamps each
  dirty 512² tile via `lumina_core::mask_tiles::stamp_brush_mark` and uploads only
  that tile with `queue.write_texture` → `bytemuck::cast_slice(&Vec<u16>)` → `&[u8]`
  (H1 fix, no `vec![0u8; tw*th*2]` dummy). Errors are `warn!`-logged, not `let _ =`d.
- **`copy_vram_to_texture(dest: wgpu::Texture)` is offscreen only today.** It
  composites `output` + `mask` via its overlay shader into a `wgpu::Texture`
  destination, but that destination must come from the *same* Metal instance. It
  **cannot present into the glow-backed egui/swapchain texture** and therefore
  cannot be called from `draw_preview` under the current renderer. It remains
  useful for offscreen/CLI renders and for its test (whether the overlay pipeline
  builds).
- **On-screen present stays `ColorImage`/`load_texture`.** The tone render path
  (`render_to_vram` for interactive drafts, `render_with_gpu` for full frames) can
  stay VRAM-resident for compute, but presenting still copies via the CPU staging
  (`egui::ColorImage::from_rgba_unmultiplied` + `ctx.load_texture` in
  `LuminaApp::update_texture`). This keeps `lumina-core` wgpu-free per `Agents.md`
  while providing the interactive < 16 ms mask-tile hot path (no `map_async`
  readback per brush stamp).
- **Future option: `egui_wgpu`.** Switching `eframe` from `glow` to `wgpu`
  (`eframe = { features = ["wgpu"] }`) would give `egui` a shared `wgpu::Device`/
  surface and allow a single-pass present: `render_to_vram` → `copy_vram_to_texture`
  → swapchain, eliminating the CPU staging. That switch is a documented follow-up
  (`GUI-60FPS-1` roadmap) and is **not** done in this fix to avoid a renderer
  migration in the same commit. Until it lands, `copy_vram_to_texture` stays
  documented as offscreen-only and the GUI comments point to the upgrade path.

### Roadmap: `VramState` → LRU tile pool

`VramState` today holds one full-resolution `output` + `mask` texture (acceptable
for interactive preview at viewport resolution; creation is `MemoryBudget`-gated).
For 45 MP sources and multi-layer masks it will become a `TiledCache`/`DraftPyramid`
LRU pool (512² tiles, generation-counted eviction, hot-set resident in VRAM) as
planned in `tiling.rs`. This is the `M2` roadmap item and remains a non-blocking
enhancement.

### WASM

`lumina-gpu` is absent on `wasm32` (`--no-default-features` or omitted dependency).
No `wgpu` code is compiled for the browser; the capability matrix in
`feature/platform/cli-gui-wasm.md` records `lumina-gpu` as WASM-unavailable. See
that document for the full matrix.

## Equivalence verification (PERF-GUI-8)

The GPU render path must be **numerically equivalent** to the CPU oracle
(`lumina_core::render_frame`). The regression net for this is the integration
test `crates/lumina-gpu/tests/golden.rs`, which renders a set of small synthetic
frames (gradients + noise) with several recipes through both paths and gates the
two RGBA8 outputs on the policy below.

### Tolerance policy

| Metric | Threshold | Meaning |
|--------|-----------|---------|
| `maxAbsDiff` per channel (R,G,B,A) | **≤ 1** (8-bit) | No channel may differ by more than one code value at any pixel. |
| `PSNR` (global, MAX=255) | **≥ 45 dB** | RMS error ≤ ~1.4/255; the two renders are visually indistinguishable. |
| `ΔE` / blake3 hash | **reported only** | A blake3 content hash of each output is logged; equality is informational and **never asserted** (a future FP16 GPU path may legitimately differ by sub-LSB rounding). |

These constants live in `tests/golden.rs` (`MAX_ABS_DIFF_TOLERANCE = 1`,
`MIN_PSNR_DB = 45.0`) so the gate is reviewable in one place.

### Recipes exercised

- `default` (identity),
- `exposure = 0.5`,
- `contrast = -0.2`,
- `wb_temperature = 5800`, `wb_tint = 0.05`,
- **GPU-unsupported (REVIEW-GPU-DIVERGENCE-1):** non-zero `vibrance`/`saturation`,
  a curved tone master, an HSL channel shift, Presence clarity, a vignette
  effect, and a recipe with SourceActions. For every one of these the GPU path
  must CPU-route (see below) and produce byte-identical output to the CPU oracle.

### Recipe validation & explicit CPU routing (REVIEW-GPU-DIVERGENCE-1)

The GPU color/tone shader implements **white balance + the seven tone sliders
only**. A recipe that uses any other pipeline stage must never be rendered by
the shader — that would silently change pixels versus every CPU build.
Therefore:

- [`lumina_gpu::unsupported_gpu_stages`] lists the unsupported stages of a
  recipe: any adjustment key outside the implemented set (including
  vibrance/saturation, whose uniform fields exist but are not applied by the
  shader), non-neutral Curves/HSL/Presence, Color Grading, Noise Reduction,
  Sharpening, Effects, Geometry, Lens Correction, Perspective and non-empty
  SourceActions.
- `GpuContext::render_with_gpu` validates before rendering: with an adapter
  bound and an unsupported recipe it **explicitly routes the render to the full
  CPU pipeline** and logs once per unique reason set (`log::info!`, never per
  frame). The GPU is an accelerator, never a semantic change (Agents.md: no
  silent fallbacks).
- The CLI routing layer (`render_best_effort` in `lumina-cli`) additionally
  checks context-level features that exist only on the CPU path — source-action
  artifacts, active-copy mask layers and a non-identity Lensfun corrector — and
  CPU-routes with the same visible log when any are present.
- The VRAM interactive preview path (`render_to_vram`) cannot CPU-route without
  a readback; it warns once per reason set so a divergent interactive preview
  is never silent.

Not a divergence today: `RenderContext::camera_white_balance` is validated but
never re-applied to pixels in `lumina-core` (the decoder has already applied
the As-Shot gains), so it triggers no routing. If core semantics change, the
validator must grow a corresponding check.

### GPU availability & headless CI

The comparison is only run when a real adapter is bound
(`GpuContext::is_available()`). When no adapter is present — headless CI, or a
`--no-default-features` (pure-CPU) build — the equivalence check is **skipped,
not failed**: the test prints

```
GPU adapter unavailable - skipped equivalence check
```

and returns, so the suite stays green on GPU-less machines while still performing
the real comparison wherever a GPU exists.

### No silent fallback (per `Agents.md`)

Since REVIEW-GPU-DIVERGENCE-1, `render_with_gpu` validates the recipe and
explicitly CPU-routes anything its tone stage cannot render (see "Recipe
validation & explicit CPU routing" above); the harness prints an `[INFO]` noting
this policy. Tone/WB-only recipes exercise the real GPU stage and are gated on
the `maxAbsDiff ≤ 1` / `PSNR ≥ 45 dB` tolerances. Any future fallback or
unimplemented-stage path must likewise surface loudly (log `info!`/`warn!`,
never swallow it) so divergence is always observable rather than hidden behind a
transparent CPU render.

## Verification (bootstrap)

- `cargo build -p lumina-gpu` compiles (wgpu init, Metal backend, no adapter
  handled gracefully).
- `cargo build -p lumina-gpu --no-default-features` compiles as a pure-CPU crate.
- `cargo build -p lumina-gui` (native) compiles with the `gpu` feature active.
