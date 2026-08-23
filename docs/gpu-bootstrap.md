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
- `lumina-gui` `gpu = ["lumina-gpu"]` (in default for native desktop; wasm32 uses
  `--no-default-features` so `lumina-gpu` is never compiled).
- CLI/MCP can opt in with `--features lumina-gpu,gpu` once they consume it.

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
- `wb_temperature = 5800`, `wb_tint = 0.05`.

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

The bootstrap's `render_with_gpu` still routes through the CPU pipeline (the GPU
DAG is implemented by parallel subagents). That fallback must be **visible, not
silent**: the harness prints a `[WARN]` noting the comparison is currently
CPU-vs-CPU and that a genuine GPU/CPU divergence will be caught by the
`maxAbsDiff ≤ 1` / `PSNR ≥ 45 dB` gates once the shader/tiling stages land. The
production path must likewise surface any fallback loudly (log `warn!`, never
swallow it) so staleness is always observable rather than hidden behind a
transparent CPU render.

## Verification (bootstrap)

- `cargo build -p lumina-gpu` compiles (wgpu init, Metal backend, no adapter
  handled gracefully).
- `cargo build -p lumina-gpu --no-default-features` compiles as a pure-CPU crate.
- `cargo build -p lumina-gui` (native) compiles with the `gpu` feature active.
