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

## Dual-Backend Native: resolved — `eframe` wgpu renderer + shared device
(GUI-WGPU-PRESENT-1)

> **Versionsstand (2026-08-26):** `lumina-gui` baut auf `eframe`/`egui` **0.36**
> mit dem **wgpu**-Renderer (Feature `wgpu`, `NativeOptions::renderer =
> Renderer::Wgpu`), `lumina-gpu` auf **dieselbe** `wgpu`-30-Version, die auch
> `egui-wgpu` 0.36 zieht. Der frühere Dual-Backend-Konflikt (glow-Present vs.
> Metal-Compute) ist damit **aufgelöst**.

The native GUI now runs `eframe` with the **wgpu** renderer. In the app builder,
`CreationContext::wgpu_render_state` hands the renderer's
`Instance`/`Adapter`/`Device`/`Queue` to [`lumina_gpu::GpuContext::from_parts`]
(`lumina_gui::attach_wgpu_render_state`). All VRAM textures therefore live on
the *same* device that presents to the swapchain.

| Plane | Owner | Backend | Shared with presenting device? |
|-------|-------|---------|-------------------------------|
| egui UI | eframe wgpu renderer | wgpu (Metal on Apple Silicon) | yes |
| tone `output` + brush/evaluated `mask` VRAM | `lumina_gpu::GpuContext::VramState` pool | same wgpu device | **yes** |
| present target (`lumina-gui-present-target`) | GUI-owned `Rgba8Unorm` texture | same wgpu device | **yes** |

### Readback-free present path

1. Drag hot path: `render_to_vram(frame, recipe)` renders tone (+ bound
   source-action artifacts) into the pooled VRAM output and marks the app's
   `vram_fresh` flag.
2. `update_texture` → `gpu_present_if_ready`: eligibility gate (fresh VRAM, not
   Before/After, no CPU ROI crop, recipe fully GPU-supported), then
   `copy_vram_to_texture(&present_target)` composites output+mask on the GPU.
3. The target is registered once per size via
   `egui_wgpu::Renderer::register_native_texture`; `draw_preview` paints it via
   `painter().image(id, …)`.

**No `map_async`, no `ColorImage` upload on this path.** The CPU
`ColorImage`/`load_texture` upload remains fully functional as fallback for:
no adapter, Before/After (original is never in VRAM), zoomed ROI previews,
recipes whose stages are not all GPU-supported (the documented GPU-STAGE-1
Restrisiko — CPU pixels are exact there), and after any edit invalidates
`vram_fresh` (`mark_dirty`).

### Overlay & double-tint guard

When a frame is GPU-presented, the overlay shader already composites the VRAM
mask plane; the CPU overlay painter skips content that lives in VRAM (live
brush strokes, evaluated planes pushed after each full render via
`combine_mask_planes` + `upload_mask_plane`). Gradient/radial prompts have no
VRAM representation and keep their CPU overlay.

### `VramState` LRU pool

`VramState` entries live in a dimension-keyed LRU pool ([`VramPool`]) bounded
by entry count (`LUMINA_GPU_VRAM_POOL_ENTRIES`, default 4) and total resident
bytes (`LUMINA_GPU_VRAM_BUDGET_MB`, default 1024 MiB; per entry
`w·h·(4+2)` for RGBA8 output + R16Uint mask). Evictions are logged loudly; a
single oversized entry is kept (a frame must always be renderable) but
prevents pooling a second source. A full 512² `TiledCache`/`DraftPyramid`
remains the M2 roadmap item for >45 MP interactive zoom.

### Init-failure warning (no silent fallback)

Adapter/device failures during context construction surface through
`log_gpu_init_failure` (`warn!`) — regression-tested in
`crates/lumina-gpu/tests/init_warning.rs`.

### WASM

`lumina-gpu` stays absent on `wasm32` (`--no-default-features`). eframe itself
compiles its wgpu backend (WebGL2/WebGPU) for the browser; the capability
matrix in `feature/platform/cli-gui-wasm.md` records `lumina-gpu` as
WASM-unavailable.

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
  Sharpening, Effects, Geometry, Lens Correction and Perspective.
  **GPU-STAGE-1:** non-empty SourceActions are flagged **unless** matching
  artifacts are bound via `GpuContext::set_source_action_artifacts` (use
  [`lumina_gpu::unsupported_gpu_stages_for`] with `source_actions_bound`);
  more actions than the stage's slot limit (`MAX_SOURCE_ACTIONS = 7`, bounded
  by wgpu's default per-stage sampled-texture limit of 16) keep the recipe on
  the CPU route.
- `GpuContext::render_with_gpu` validates before rendering: with an adapter
  bound and an unsupported recipe it **explicitly routes the render to the full
  CPU pipeline** and logs once per unique reason set (`log::info!`, never per
  frame). The GPU is an accelerator, never a semantic change (Agents.md: no
  silent fallbacks).
- The CLI routing layer (`render_best_effort` in `lumina-cli`) additionally
  checks context-level features that exist only on the CPU path — active-copy
  mask layers, source-action *context* artifacts and a non-identity Lensfun
  corrector — and CPU-routes with the same visible log when any are present.
- The VRAM interactive preview path (`render_to_vram`) cannot CPU-route without
  a readback; it warns once per reason set so a divergent interactive preview
  is never silent.

Not a divergence today: `RenderContext::camera_white_balance` is validated but
never re-applied to pixels in `lumina-core` (the decoder has already applied
the As-Shot gains), so it triggers no routing. If core semantics change, the
validator must grow a corresponding check.

### Dedicated GPU stages (GPU-STAGE-1)

Two stages beyond tone/WB now run as real WGSL passes:

- **Source-action stage** (`SOURCE_ACTION_STAGE_SRC`): composites up to
  `MAX_SOURCE_ACTIONS` bound artifacts exactly like the CPU oracle —
  `out = replacement` where the region coverage reaches `>= 32768`
  (exact integer compare on an `R16Uint` texture read via `textureLoad`),
  alpha included. All reads are pure texel copies, so with a neutral recipe the
  output is **byte-identical** to the CPU render; behind the tone stage it
  stays within the golden tolerances. Gated by
  `crates/lumina-gpu/tests/stages.rs`.
- **Mask plane data path**: evaluated layer planes are combined CPU-side with
  `combine_mask_planes` (F-041 intersection-product weights, unit-tested) and
  uploaded byte-exactly into the pooled VRAM mask texture
  (`upload_mask_plane`, roundtrip-tested via `readback_mask_plane`). The
  overlay pass composites them during present; mask *pixel modulation* does
  not exist in the CPU pipeline yet (planes feed measurement + overlay only),
  so when local-adjustment masking lands in core this plane is already the
  modulation input.

**Format decision:** mask/region textures use `R16Uint`, not `R16Unorm` — the
unorm-16 family requires the optional `TEXTURE_FORMAT_16BIT_NORM` feature that
neither our nor eframe's shared devices enable (found by the GPU-STAGE-1 test:
creating the VRAM mask texture failed validation). Integer formats need no
extra features and preserve the exact u16 domain; integer textures reject
filtering samplers, so the shaders read them via `textureLoad`.

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
