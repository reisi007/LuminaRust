# lumina-onnx

Native ONNX inference adapter for Lumina (F-047), implementing the exchangeable
backend for `BiRefNet` as the first automatic subject model, with the model
capability surface from F-080.

This crate is **native-only** (mirrors `lumina-raw`): it is never built
for the browser. It depends on `lumina-sidecar`
solely for the identity mapping `ModelManifest → ModelIdentity`
(F-048/`to_model_identity`); all native/ONNX concerns stay inside this crate.

## Capability matrix (native-only)

| Target | `onnx-rt` default | `onnx-rt` enabled |
| --- | --- | --- |
| native (macOS/Linux) | StubBackend, `resolve` reports `RuntimeDisabled` | `resolve` loads/verifies the real ORT engine or fails visibly |

ONNX inference is a native capability, the browser remains explicitly "offen"
(see `feature/platform/capability-matrix.md`).

## Layout

- `manifest.rs` — `ModelManifest` (serde identity + I/O contract) and
  `ModelCapabilities` (F-080: `subject_segmentation`, `box_prompt`,
  `point_prompt`, `mask_prompt`, `class_detection`, `instance_segmentation`,
  plus the generative `inpaint_heal` (SPOT-REMOVE-1) and `outpaint`
  (GEN-EXPAND-1) flags). At least one capability must be set; unknown fields
  are rejected.
- `inpaint.rs` — deterministic `StubInpaintBackend` for spot-heal inpaint
  (`inpaint_heal_manifest`, 512×512, `pending-integration`).
- `outpaint.rs` — deterministic `StubOutpaintBackend` for generative canvas
  expansion (`outpaint_expand_manifest`, 1024×1024, `pending-integration`):
  source block copied at `source_offset`, border filled from source mean plus
  a prompt/seed/canvas hash offset. `available == false` reports
  `ModelUnavailable`, a manifest without `outpaint` is rejected with
  `UnsupportedModel` — never a silent fallback.
- `preprocess.rs` — pure, deterministic, dependency-free resize /
  rescale helpers (nearest-neighbor, documented integer mapping).
- `backend.rs` — the `SubjectInference` trait and the deterministic
  `StubBackend` (centered radial matte, no weights/network). This is the
  complete, tested default surface.
- `resolve.rs` — backend resolution for consumers (CLI/core):
  [`try_load_onnx_engine`] loads the real ORT engine when `onnx-rt` is
  compiled in and the artifact verifies; otherwise it reports the explicit
  states `RuntimeDisabled` / `MissingModel` / `ModelArtifactStale` /
  `InferenceFailed` — **never a silent fallback to the stub**.
- `ort_backend.rs` — real ONNX Runtime backend, **gated behind the `onnx-rt`
  feature** (see below).

## BiRefNet

`birefnet_manifest()` describes BiRefNet: automatic subject segmentation from a
single RGB input to an alpha matte, no prompts (`subject_segmentation` only),
documented inference resolution 1024×1024, license `MIT` (verified 2026-08-20
against GitHub `LICENSE` and the HF model card `ZhengPeng7/BiRefNet`). The
model hash is a placeholder (`pending-integration`) until real weights are
provided in F-048.

## Real ONNX Runtime backend (`onnx-rt`)

The `ort` crate (v2.0.0-rc.13) **is fetchable and builds** in this environment,
including its prebuilt ONNX Runtime binary download. It is therefore wired in
behind the non-default `onnx-rt` feature:

```toml
lumina-onnx = { features = ["onnx-rt"] }
```

The default build/test of `lumina-onnx` does **not** enable `onnx-rt` and needs
**no network access** — it relies solely on `StubBackend`. Enabling `onnx-rt`
requires network at build time (prebuilt binaries) and a real `.onnx` artifact
at runtime. Numeric correctness against an actual BiRefNet model (input/output
tensor names, value ranges) is validated later in F-048/F-082 once model
weights are available; until then the `OrtBackend` is compile-verified and
handles the `MissingModel` case without weights.

### Backend resolution (no silent fallback)

Consumers (CLI/core) obtain the real engine via
[`try_load_onnx_engine`](`crate::resolve::try_load_onnx_engine`):

- `onnx-rt` **disabled** → `Ok(OnnxEngine::RuntimeDisabled)`: the caller
  decides explicitly between `StubBackend` and no engine — the capability
  statement, never a silent fallback.
- `onnx-rt` **enabled** + present, identity-verified artifact → `Ok(OnnxEngine::OnnxRuntime(Box<dyn MaskInference>))`,
  the exact contract `lumina-core`'s mask-loading decision layer consumes.
- `onnx-rt` **enabled** + missing / hash-mismatched / wrong-tensor-name
  artifact → `Err(MissingModel)` / `Err(ModelArtifactStale)` /
  `Err(InferenceFailed)` — visible, hard errors; no fallback to the stub.

### Hash-pinned ONNX behavior fixture
`tests/fixtures/lumina-crafted-reducemax.onnx` is a committed, hash-pinned
behavior fixture (SHA-256 pin in `tests/fixtures/README.md`): a minimal,
deterministically generated ReduceMax graph (no downloads, no model weights)
that exercises the real ORT load/verify/infer code paths under a pinned
identity. Real BiRefNet/SAM-2 model weights remain `pending-integration`
(Agents.md: no spontaneous downloads; hash-pinned fixtures required).

## Generative outpaint (GEN-EXPAND-1, local vs. Cloud)

`outpaint_expand_manifest()` declares the planned local ONNX outpaint model
(`inpaint-outpaint-xl` 1.0.0, 1024×1024, capability `outpaint`):

- **Local vs. Cloud are separate capabilities** (no silent fallback):
  local ONNX inference lives in this crate (native); a Cloud-API path is
  **not planned** and needs an explicit capability decision first
  (`feature/product/generative-expand.md`, capability matrix). The stub never
  calls a network.
- **License / hash pin (F-078, pre-integration):** no weights are committed;
  `model_hash` is `pending-integration` and the `Apache-2.0` license entry is
  a placeholder declaration only. Many state-of-the-art inpaint/outpaint
  weights are non-commercial — the license MUST be verified against the actual
  weight source before any hash pin lands (same caution as the `ultralytics`
  AGPL note in `feature/quality/fixtures-licensing.md` §5). Tests run against
  the deterministic stub only and require no network access.
- **Browser:** outpaint is unavailable (native `StubOutpaintBackend`
  reports `ModelUnavailable`, engine resolves to `RuntimeDisabled`).

## Error handling

`OnnxError` (`thiserror`) distinguishes `UnsupportedModel` (manifest/license/
capability mismatch), `InferenceFailed`, `InvalidDimensions`, and `MissingModel`.
There are **no silent fallbacks**: a missing or mismatched artifact is reported,
never guessed.
