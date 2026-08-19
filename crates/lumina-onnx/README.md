# lumina-onnx

Native ONNX inference adapter for Lumina (F-047), implementing the exchangeable
backend for `BiRefNet` as the first automatic subject model, with the model
capability surface from F-080.

This crate is **native-only** (mirrors `lumina-raw`): it is gated out on
`wasm32` and is never built for the browser. It intentionally does **not**
depend on `lumina-sidecar` in this iteration — the mapping of
`ModelManifest`/`ModelCapabilities` to the sidecar `ModelIdentity` is deferred
to F-048.

## Layout

- `manifest.rs` — `ModelManifest` (serde identity + I/O contract) and
  `ModelCapabilities` (F-080: `subject_segmentation`, `box_prompt`,
  `point_prompt`, `mask_prompt`, `class_detection`, `instance_segmentation`).
  At least one capability must be set; unknown fields are rejected.
- `preprocess.rs` — pure, deterministic, dependency-free resize /
  rescale helpers (nearest-neighbor, documented integer mapping).
- `backend.rs` — the `SubjectInference` trait and the deterministic
  `StubBackend` (centered radial matte, no weights/network). This is the
  complete, tested default surface.
- `ort_backend.rs` — real ONNX Runtime backend, **gated behind the `onnx-rt`
  feature** (see below).

## BiRefNet

`birefnet_manifest()` describes BiRefNet: automatic subject segmentation from a
single RGB input to an alpha matte, no prompts (`subject_segmentation` only),
documented inference resolution 1024×1024, license `Apache-2.0`. The model hash
is a placeholder (`pending-integration`) until real weights are provided in
F-048.

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

## Error handling

`OnnxError` (`thiserror`) distinguishes `UnsupportedModel` (manifest/license/
capability mismatch), `InferenceFailed`, `InvalidDimensions`, and `MissingModel`.
There are **no silent fallbacks**: a missing or mismatched artifact is reported,
never guessed.
