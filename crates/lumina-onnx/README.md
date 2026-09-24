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
  (GEN-EXPAND-1) flags, the face `face_detect`/`face_embed` flags
  (LRPAR-G12-FACE-20) and the KI-denoise `denoise` flag
  (LRPAR-G14-DENOISE-IMPL-20)). At least one capability must be set; unknown
  fields are rejected.
- `inpaint.rs` — deterministic `StubInpaintBackend` for spot-heal inpaint
  (`inpaint_heal_manifest`, 512×512, `pending-integration`).
- `outpaint.rs` — deterministic `StubOutpaintBackend` for generative canvas
  expansion (`outpaint_expand_manifest`, 1024×1024, `pending-integration`):
  source block copied at `source_offset`, border filled from source mean plus
  a prompt/seed/canvas hash offset. `available == false` reports
  `ModelUnavailable`, a manifest without `outpaint` is rejected with
  `UnsupportedModel` — never a silent fallback.
- `generative.rs` — GEN-ONNX-1 producer: `produce_canvas` (role from the
  persisted `GenerativeEdit` flags, capability + hash gated), the real
  hash-pinned `fixture_manifest`/`verify_fixture_manifest`, and the documented
  real-weight attachment surface `GenerativeModelSource::artifact`.
- `face.rs` + `face/` — LRPAR-G12-FACE-20 S2/S3/S6: face detection/embedding
  manifests (`face_detect`/`face_embed` capabilities, verified `sha256:` pins),
  the shared inference identity + digest onto the S1 sidecar schema, the
  deterministic, tests-only stub backends, the model-free DBSCAN-over-cosine
  clustering with confirm/split/merge, and the real ORT face backends behind
  `onnx-rt` (`face/ort.rs`).
- `denoise.rs` + `denoise/` — LRPAR-G14-DENOISE-IMPL-20: KI-denoise manifest
  (`denoise` capability, `pending-integration`), `DenoiseModelSuite`/
  `DenoiseTileSpec` with the deterministic versioned
  `lumina-denoise-input-spec-v2` `input_spec_digest` (resolution +
  tile/overlap + identity preprocessing + core blend-v1 and
  distance-to-edge-assembly-v1 contracts), the GEN-ONNX-1-style fixture pin,
  the tiled producer (`produce_denoise_artifact`, seam-free via the core
  `assemble_denoise_tiles` contract, provenance persisted through
  `set_denoise_producer_provenance`), the deterministic, tests-only stub and
  the real ORT backend behind `onnx-rt` (`denoise/ort.rs`). A
  `pending-integration` manifest is refused loudly as `ModelUnavailable`.
  The default v2 input-spec pin is
  `sha256:e02314484356c026f5f4f64d4823450a450a833945a163f9d9abe07e024cda07`;
  the deterministic fixture-spec pin is
  `sha256:0a5917d19b0e786042e493eb967bb02ca024c51a5711703d6357815c067feca0`
  (fixture identity only, never a weight-file pin).
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

- **GEN-ONNX-1 Welle 1 (`generative.rs`):** `produce_canvas(frame, edit, source)`
  emits the full composited `generative_canvas` (RGBA8) plus its complete
  identity (the core `GenerativeCacheKey::digest`), gated on the manifest
  capability and — for a real artifact — the pinned SHA-256. `fixture_manifest`
  pins the deterministic fixture model with a **real** `sha256:<64 hex>` over the
  versioned fixture specification (`verify_fixture_manifest` recomputes it);
  `GenerativeModelSource::artifact(manifest, path)` is the documented real-weight
  attachment surface (`resolve_manifest` refuses a stale/missing artifact
  loudly). `pending-integration` descriptors stay loud — the fixture is never a
  silent substitute for real weights. See `feature/product/generative-expand.md`
  § Modell-Entscheid Welle 1.
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

## Face detection (LRPAR-G12-FACE-20, S2 + S3)

The decision `feature/decisions/LRPAR-G12-FACE-20.md` splits face recognition
into three independently versioned stages; `crates/lumina-onnx` implements the
native detection/embedding stages (S2) and the model-free clustering stage (S3).

- **Models + licences (verified, FACE-20-S6):** `face_detect_manifest()` =
  **YuNet** (OpenCV Zoo `face_detection_yunet`, **MIT** © 2020 Shiqi Yu);
  `face_embed_manifest()` = **SFace/MobileFaceNet** (OpenCV Zoo
  `face_recognition_sface`, **Apache-2.0** © 2021 Shenzhen Institute of AI and
  Robotics for Society). The model-directory `LICENSE` plus its README clause
  "all files in this directory" is the **weight grant** (verified 2026-09-20 at
  `opencv/opencv_zoo` `main` @ `47534e27…`). Both descriptors carry verified
  `sha256:<hex>` pins of the exact artifacts
  (`FACE_DETECT_MODEL_HASH` / `FACE_EMBED_MODEL_HASH`); the InsightFace/ArcFace
  non-commercial weight licence and the AGPL `ultralytics` tooling are
  explicitly avoided. **No weights are committed and nothing is downloaded**;
  license texts live in `licenses/models/`.
- **Real I/O adapter (LRPAR-G12-FACE-ADAPTER-25):** the shipped graphs are
  consumed through their real contract — YuNet `input` (raw `0..=255`, BGR) →
  twelve per-stride outputs decoded by `face::yunet` (decode + NMS) and SFace
  `data` (raw `0..=255`, RGB) → 128-d `fc1` (the graph bakes
  `(x−127.5)·1/128`). A graph that does not match its declared tensors is
  refused loudly at load (`InferenceFailed` listing the available tensors) — no
  silent re-shaping and no stub substitution.
- **Capabilities:** `ModelCapabilities.face_detect` / `face_embed`
  (additive, `#[serde(default)]`; manifests written before them keep parsing).
- **Identity (`face_identity`, `face_identity_digest`):** source + decode +
  geometry + both model identities (each carrying its input-spec digest) +
  inference resolution + shared preprocessing (alignment, normalization, score
  threshold, NMS threshold, top-k, embedding dimension) + clustering
  method/version/thresholds. A
  model, preprocessing or clustering change makes a persisted analysis `stale`;
  a missing/corrupt artifact is `missing`/`corrupt`
  (`face_artifact_status`). Clustering is image-local — no cross-catalogue
  person identity, no cloud, no telemetry.
- **Backends:** deterministic, tests-only stubs (`StubFaceDetector`,
  `StubFaceEmbedder`) are the default, network-free surface; the real ORT path
  (`face/ort.rs`) verifies the artifact SHA-256 and the declared tensor names
  and fails loudly (`MissingModel` / `ModelArtifactStale` / `InferenceFailed`) —
  never a silent stub substitution. `try_load_face_engine` is the consumer
  surface (`OnnxRuntime` / `RuntimeDisabled`).
- **Clustering:** deterministic DBSCAN over cosine distance
  (`FACE_CLUSTERING_METHOD`/`_VERSION`, versioned `eps`/`min_samples`);
  permutation-invariant via canonical ordering, image-local, plus pure
  confirm/split/merge operations on sidecar clusters/persons.

## Error handling

`OnnxError` (`thiserror`) distinguishes `UnsupportedModel` (manifest/license/
capability mismatch), `InferenceFailed`, `InvalidDimensions`, `MissingModel`,
`ModelArtifactStale` and `InvalidFaceData` (face clustering/embedding/box
contract violations).
There are **no silent fallbacks**: a missing or mismatched artifact is reported,
never guessed.
