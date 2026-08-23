# Lumina GPU Shader Stage — Design & DAG

Owner: `lumina-gpu` (native GPU path). Companion to `docs/gpu-bootstrap.md`
(bootstrap + workspace wiring). This document covers the **shader/tiling stage**
implemented in `crates/lumina-gpu/src/{shaders,tiling}.rs` and wired through
`GpuContext` in `lib.rs`.

All GPU code is gated behind `#[cfg(feature = "gpu")]`; `--no-default-features`
builds a pure-CPU crate (the same CPU fallback `lumina-core` provides).

## Data contract (the "shader stage" inputs)

| Source | Type | Producer | Consumer |
|--------|------|----------|----------|
| Slider parameters | `shaders::Uniforms` (`#[repr(C)]`, `bytemuck::Pod`, 64 B) | `Uniforms::from_recipe(&EditRecipe)` | uniform buffer → `@group(0) @binding(0)` |
| Tone/look grade | `shaders::Lut32x32x32` (32³ RGBA f32) | `bake_3d_lut(&EditRecipe)` | 3D texture, trilinear sample |
| Working buffers | `Rgba16Float` (`shaders::FP16_FORMAT`) | `create_fp16_framebuffer(...)` | color/tone + effect passes |
| Source tiles | `tiling::TiledCache` (512² + LRU) | `TiledCache::insert` | draft/ROI draw |

## Render DAG

```
  RAW / decoded source
          │
          ▼
  ┌───────────────────────────────────────────────────────────────────┐
  │ 0. DECODE / DEMOSAIC  (lumina-raw → base texture in VRAM)        │
  │    • cached once per source (keyed by content hash)               │
  │    • split into 512² tiles → TiledCache (LRU atlas)             │
  └───────────────────────────────┬─────────────────────────────────┘
                                  │ base tiles (sampled)
                                  ▼
  ┌───────────────────────────────────────────────────────────────────┐
  │ 1. COLOR / TONE  (fragment shader)                               │
  │    • driven by `shaders::Uniforms` via the uniform buffer        │
  │      (exposure, contrast, highlights, shadows, whites, blacks,   │
  │       wb_temperature, wb_tint, vibrance, saturation)             │
  │    • output written to an FP16 framebuffer (`Rgba16Float`)       │
  └───────────────────────────────┬─────────────────────────────────┘
                                  │ linear FP16
                                  ▼
  ┌───────────────────────────────────────────────────────────────────┐
  │ 2. 3D LUT  (32³)                                                 │
  │    • `bake_3d_lut(recipe)` → Lut32x32x32, baked once per grade   │
  │    • sampled trilinearly in the look/tone stage                  │
  └───────────────────────────────┬─────────────────────────────────┘
                                  │ graded FP16
                                  ▼
  ┌───────────────────────────────────────────────────────────────────┐
  │ 3. EFFECTS + COMPOSITE (vignette, grain, NR, sharpen, masks,     │
  │    geometry) — FP16 throughout                                   │
  └───────────────────────────────┬─────────────────────────────────┘
                                  │
                                  ▼
  ┌───────────────────────────────────────────────────────────────────┐
  │ 4. TILING + PRESENT                                              │
  │    • 512² tiles, VRAM LRU (TiledCache) keeps hot tiles resident  │
  │    • draft pyramid (mip chain) for ROI preview                   │
  │    • read back to RGBA8 `Frame` for export/preview               │
  └───────────────────────────────────────────────────────────────────┘
```

### Draft pyramid + ROI (`render_draft`)

```
   full-res base (level 0)
        ├── level 1  (½ res)      ─┐
        ├── level 2  (¼ res)       │ DraftPyramid::level_for_zoom(zoom)
        └── ...                    ┘  picks coarsest level covering viewport
                                        │
   Viewport (ROI) ──► TiledCache::keys_for_viewport(viewport, zoom)
                        → Vec<TileKey{level,tx,ty}>
                        → drawn / evicted via LRU (capacity-bounded)
```

While a slider is dragged, `render_draft(frame, viewport)` selects the coarsest
pyramid level that still spans the viewport (so each 512² tile covers a useful
on-screen area) and expands the ROI into the tile set. On commit the caller
re-renders at level 0 (full res). The tile set is currently `log::debug!`-ed; the
parallel tiling subagent plugs the actual GPU upload/dispatch into this call site.

## Module responsibilities

- **`shaders.rs`** — `Uniforms` (uniform-buffer layout), `Lut32x32x32` +
  `bake_3d_lut` (32³ LUT; currently an identity-LUT stub + log), `FP16_FORMAT`
  and `create_fp16_framebuffer`, plus `create_uniform_buffer` / `write_uniforms`
  helpers. Also holds the placeholder WGSL in `lib.rs` (`SHADER_SRC`).
- **`tiling.rs`** — `TILE_SIZE = 512`, `Viewport`, `TileKey`, `TiledCache`
  (HashMap tile key → `wgpu::Texture` with an LRU eviction ring), and
  `DraftPyramid` (mip descriptors + `level_for_zoom` / `level_dimensions`).
- **`lib.rs`** — `GpuContext` owns `GpuResources` (device/queue) + `PipelineState`
  (compiled pipeline + uniform buffer + bind group). Public surface:
  `new`, `is_available`, `device`, `queue`, `create_pipeline`,
  `update_uniforms(recipe)`, `render_with_gpu(frame, recipe)`,
  `render_draft(frame, viewport)`. When no adapter is bound, every render routes
  through the `lumina-core` CPU pipeline (`render_cpu` / `render_draft_cpu`).

## Current bootstrap status

- The color/tone **fragment shader is implemented** (`SHADER_SRC` in `lib.rs`):
  it samples the decoded source frame (uploaded as an `Rgba8Unorm` texture,
  nearest sampler) and applies the same integer-rounded per-channel math as
  `lumina-core::apply_channel_lut_adjustments` — white balance → exposure →
  contrast → shadows → highlights → whites → blacks — writing the graded result
  into an `Rgba8Unorm` target. `create_pipeline` builds this real pipeline
  (3-entry bind group: uniform + input texture + sampler) and `render_with_gpu`
  uploads the frame, draws a fullscreen triangle and reads the result back into a
  `Frame` via `map_async`. The golden-image harness (`tests/golden.rs`) gates the
  GPU output against the CPU oracle at maxAbsDiff ≤ 1 / PSNR ≥ 45 dB; the
  readback path is a PERF TODO (a later stage should present to a swapchain or
  keep a persistent VRAM `Frame`).
- White-balance absence is handled by defaulting `wb_temperature` to **6500 K**
  (neutral) in `Uniforms::from_recipe`, which is byte-equivalent to the CPU's
  `temperature.unwrap_or(6500.0)` / `tint.unwrap_or(0.0)` and avoids a separate
  presence flag in the 64-byte uniform block.
- `bake_3d_lut` still returns the **identity LUT** and logs; the graded bake
  reuses the `lumina-core` CPU reference kernel once wired. The current shader
  does not yet sample a 3D LUT (it implements the channel adjustments directly),
  which is sufficient for the channel-LUT adjustment family the gold harness
  covers.
- `render_draft` exercises the full scaffold (pipeline → uniforms → ROI tile set)
  but still produces pixels via the CPU fallback, keeping the CPU path the test
  oracle until the GPU draw is implemented.

## Verification

```
cargo build -p lumina-gpu                 # with gpu feature (default) — green
cargo build -p lumina-gpu --no-default-features   # pure CPU — green
```

Never touch `lumina-gui/src/lib.rs` from this stage (owned by a separate job);
the GUI `gpu` feature simply enables the `lumina-gpu` dependency.
