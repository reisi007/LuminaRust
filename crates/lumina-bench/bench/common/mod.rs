//! Deterministic, synthetic fixtures for the Lumina benchmark harness.
//!
//! Every fixture here is generated locally and reproducibly from a single
//! fixed seed. **Never change the seed** (`FIXTURE_SEED`) without re-recording
//! `perf/baseline.json`: the recorded medians/p95s are only comparable against
//! the exact bytes produced by this generator (see
//! `feature/quality/performance-benchmarks.md`, "Fixtures und Daten").
//!
//! No benchmark loads data from the network; the only external inputs are the
//! committeten RAW fixtures read by `decode.rs` (behind `LUMINA_RAW_FIXTURE`).

// Shared fixtures module: not every helper is used by every bench binary, but
// the module is compiled into each. Allow dead code so the crate-level
// `-D warnings` gate (F-072 / ADR 0003) passes for each consumer.
#![allow(dead_code)]

use lumina_core::{
    DenoiseIdentity, DenoiseRgbArtifact, FolderCache, ImageFileFormat, ImageFrame, MaskPlane,
    DENOISE_RGB_ENCODING_VERSION,
};
use lumina_cull::{analyze_heuristic, record_culling, CullConfig, CullSourceInput};
use lumina_merge::LinearImage;
use lumina_sidecar::{
    CoordinateSystem, CullingIdentity, DecodeFingerprint, DenoiseAi, DenoiseArtifactKind,
    DenoiseArtifactRef, DenoiseModelIdentity, EditRecipe, Extras, GeometryFingerprint,
    MaskDefinition, MaskLayer, MaskOperation, MaskReference, MaskStatus, ModelIdentity,
    Preprocessing, Resolution, SidecarDocument, SourceFingerprint, SourceIdentity, VirtualCopy,
    DENOISE_AI_VERSION,
};
use std::collections::BTreeMap;

/// Fixed RNG seed for ALL synthetic fixtures (F-074-N3). Documented and frozen;
/// changing it invalidates the recorded baseline.
pub const FIXTURE_SEED: u64 = 0x5EED;

/// Fixed resolution steps used across the Core/Pipeline and Batch classes
/// (512 / 1024 / 2048), matching the normative fixture rules.
pub const SIZES: &[u32] = &[512, 1024, 2048];

/// SplitMix64 — a tiny, dependency-free, fully deterministic PRNG. We use it
/// instead of `rand` to keep the harness free of extra runtime deps and to make
/// the determinism obvious and auditable.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A per-size deterministic seed derived from the frozen [`FIXTURE_SEED`].
fn seeded_rng(size: u32) -> impl FnMut() -> u8 {
    let mut state = FIXTURE_SEED ^ (u64::from(size).wrapping_mul(0x2545F4914F6CDD1D));
    move || {
        // Take one low byte from the 64-bit mixer per call.
        (splitmix64(&mut state) & 0xFF) as u8
    }
}

/// Builds a deterministic `size × size` RGBA8 frame. Alpha is always 255 so the
/// luminance/measurement helpers ignore it consistently.
pub fn make_frame(size: u32) -> ImageFrame {
    let pixels = make_pixels(size);
    ImageFrame::new(size, size, pixels).expect("deterministic pixel count matches dimensions")
}

fn make_pixels(size: u32) -> Vec<u8> {
    let len = (size as usize) * (size as usize) * 4;
    let mut rng = seeded_rng(size);
    let mut pixels = Vec::with_capacity(len);
    for _ in 0..(size as usize * size as usize) {
        let r = rng();
        let g = rng();
        let b = rng();
        pixels.extend_from_slice(&[r, g, b, 255]);
    }
    pixels
}

fn default_extras() -> Extras {
    Extras::new()
}

/// A representative, non-trivial edit recipe exercising the adjustment stages
/// (including white balance). Geometry stays `None` so the output keeps the
/// source dimensions and the benchmark stays a pure per-pixel adjustment run.
pub fn make_recipe() -> EditRecipe {
    let mut recipe = EditRecipe::default();
    recipe.adjustments.insert("exposure".into(), 0.3);
    recipe.adjustments.insert("contrast".into(), -0.2);
    recipe.adjustments.insert("highlights".into(), 0.1);
    recipe.adjustments.insert("shadows".into(), 0.1);
    recipe.adjustments.insert("wb_temperature".into(), 5200.0);
    recipe.adjustments.insert("wb_tint".into(), -0.1);
    recipe.adjustments.insert("vibrance".into(), 0.2);
    recipe.adjustments.insert("saturation".into(), 0.1);
    recipe
}

fn mask_definition(
    id: &str,
    status: MaskStatus,
    operation: MaskOperation,
    references: Vec<MaskReference>,
) -> MaskDefinition {
    MaskDefinition {
        id: id.into(),
        name: id.into(),
        source_fingerprint: SourceFingerprint {
            content_hash: "bench".into(),
            byte_length: 1,
            extras: default_extras(),
        },
        decode_context: DecodeFingerprint {
            decoder: "bench".into(),
            version: "1".into(),
            parameters: BTreeMap::new(),
            extras: default_extras(),
        },
        geometry_context: GeometryFingerprint {
            width: 2,
            height: 1,
            orientation: 1,
            pixel_aspect_ratio: 1.0,
            extras: default_extras(),
        },
        model: ModelIdentity {
            name: "bench".into(),
            version: "1".into(),
            hash: "bench".into(),
            extras: default_extras(),
        },
        inference_resolution: Resolution {
            width: 2,
            height: 1,
            extras: default_extras(),
        },
        preprocessing: Preprocessing {
            name: "bench".into(),
            version: "1".into(),
            parameters: BTreeMap::new(),
            extras: default_extras(),
        },
        rescaling_method: "none".into(),
        rescaling_parameters: BTreeMap::new(),
        coordinate_system: CoordinateSystem::SourceOriented,
        status,
        created_at: "bench".into(),
        generator_version: "bench".into(),
        error_text: None,
        artifact: None,
        operation,
        references,
        prompt: None,
        extras: default_extras(),
        ai_select: None,
    }
}

fn mask_reference(copy_id: &str, mask_id: &str) -> MaskReference {
    MaskReference {
        copy_id: copy_id.into(),
        mask_id: mask_id.into(),
        extras: default_extras(),
    }
}

/// Deterministic mask fixture: a single valid source mask sized to the frame
/// with a smooth gradient pattern (0..=u16::MAX around the ring). Returned as
/// owned copies + planes so callers can either build a [`lumina_core::MaskContext`]
/// (for `render_frame`) or a [`lumina_core::MaskGraph`] (for `mask_graph_eval`).
pub struct MaskFixture {
    pub copies: Vec<VirtualCopy>,
    pub planes: BTreeMap<(String, String), MaskPlane>,
}

pub fn make_mask_fixture(size: u32) -> MaskFixture {
    let count = (size as usize) * (size as usize);
    let values: Vec<u16> = (0..count)
        .map(|i| (((i as u64).wrapping_mul(7919)) % (u16::MAX as u64 + 1)) as u16)
        .collect();
    let plane = MaskPlane::new(size, size, values).expect("plane dimensions match count");

    let definitions = vec![mask_definition(
        "subject",
        MaskStatus::Valid,
        MaskOperation::Source,
        vec![],
    )];
    let copies = vec![VirtualCopy {
        id: "vc-original".into(),
        name: "vc-original".into(),
        is_default: true,
        rating: 0,
        flag: lumina_sidecar::Flag::Unflagged,
        recipe: EditRecipe::default(),
        mask_library: definitions,
        mask_layers: vec![MaskLayer {
            id: "subject-layer".into(),
            mask: mask_reference("vc-original", "subject"),
            inverted: false,
            feather: 0.0,
            blur: 0.0,
            density: 1.0,
            extras: default_extras(),
            visible: true,
        }],
        history: vec![],
        export_records: vec![],
        extras: default_extras(),
    }];

    let mut planes = BTreeMap::new();
    planes.insert(("vc-original".into(), "subject".into()), plane);
    MaskFixture { copies, planes }
}

/// Builds a `FolderCache` with one stored entry and returns `(cache, hit_key,
/// miss_key)` so the hit path (`get(present)`) and miss path (`get(absent)`)
/// can be benchmarked separately.
pub fn make_cache_fixture(size: u32) -> (FolderCache, String, String) {
    let frame = make_frame(size);
    let bytes = frame
        .encode(ImageFileFormat::Png)
        .expect("deterministic PNG encode");
    let mut cache = FolderCache::default();
    let hit_key = "present".to_string();
    cache.store(hit_key.clone(), bytes);
    let miss_key = "absent".to_string();
    (cache, hit_key, miss_key)
}

/// Deterministic `size × size` linear-RGB gradient for the merge benchmark
/// class (F-074 / MERGE-IMPL-15): `v = (x*0.7 + y*3.1) / (2*size)`, clamped
/// to 1. Non-periodic, so the SAD alignment has a unique minimum. Derived
/// from the frozen [`FIXTURE_SEED`] regime (no randomness; documented sizes
/// 512/1024/2048).
pub fn make_linear_gradient(size: u32) -> LinearImage {
    linear_gradient(size, 0.0)
}

/// [`make_linear_gradient`] resampled with a horizontal subpixel shift (zero
/// fill on the vacated edge) — the HDR/pano alignment stand-in.
pub fn make_linear_gradient_shifted(size: u32, shift_x: f32) -> LinearImage {
    linear_gradient(size, shift_x)
}

fn linear_gradient(size: u32, shift_x: f32) -> LinearImage {
    let denom = (size as f32 * 2.0).max(1.0);
    let mut pixels = Vec::with_capacity(size as usize * size as usize * 3);
    for y in 0..size {
        for x in 0..size {
            let sample_x = x as f32 - shift_x;
            let value = if sample_x < 0.0 {
                0.0
            } else {
                ((sample_x * 0.7 + y as f32 * 3.1) / denom).min(1.0)
            };
            pixels.extend_from_slice(&[value, value, value]);
        }
    }
    LinearImage::new(size, size, pixels).expect("deterministic linear frame")
}

/// Scales every linear sample by `factor` (clamped to 1) — the HDR bracket
/// under-exposure stand-in; geometry is preserved.
pub fn scale_linear(image: &LinearImage, factor: f32) -> LinearImage {
    let pixels: Vec<f32> = image
        .pixels()
        .iter()
        .map(|value| (value * factor).min(1.0))
        .collect();
    LinearImage::new(image.width(), image.height(), pixels).expect("scaled frame keeps geometry")
}

// ---------------------------------------------------------------------------
// KI-Denoise fixtures (F-074 / LRPAR-G14-DENOISE-IMPL-20)
// ---------------------------------------------------------------------------

/// Fixed `sha256:<64 hex>` model pin used by the denoise fixtures. It is
/// deliberately **not** `pending-integration`: the deterministic, weight-free
/// fixture path must resolve as an active/`ready` stage, while real ONNX
/// weights stay absent by design (decision §3.1). The value is a documented
/// filler, not a real model hash.
pub fn bench_model_hash() -> String {
    format!("sha256:{}", "11".repeat(32))
}

/// Fixed input-spec digest matching [`bench_model_hash`] (same documented-filler
/// contract).
fn bench_input_spec_digest() -> String {
    format!("sha256:{}", "22".repeat(32))
}

/// Deterministic `size × size` denoised RGB8 artifact derived from the same
/// frozen seed regime as [`make_frame`] (every source byte halved). Geometry
/// matches the frame exactly, so the blend/render benchmarks accept it without
/// a loud `corrupt` mismatch.
pub fn make_denoise_artifact(size: u32) -> DenoiseRgbArtifact {
    let frame = make_frame(size);
    let pixels = frame
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|pixel| [pixel[0] / 2, pixel[1] / 2, pixel[2] / 2])
        .collect();
    DenoiseRgbArtifact::new(size, size, pixels).expect("deterministic artifact geometry")
}

/// The active (non-identity) `denoise_ai` recipe stage used by the benchmark
/// fixtures: pinned model hash, fixed input-spec digest, external artifact ref.
pub fn make_denoise_ai(
    size: u32,
    strength: f32,
    preserve_detail: f32,
    artifact_checksum: &str,
) -> DenoiseAi {
    DenoiseAi {
        version: DENOISE_AI_VERSION,
        enabled: true,
        model: DenoiseModelIdentity {
            name: "bench-fixture-srgb".into(),
            version: "1".into(),
            model_hash: bench_model_hash(),
            extras: default_extras(),
        },
        input_spec_digest: bench_input_spec_digest(),
        strength,
        preserve_detail,
        artifact: Some(DenoiseArtifactRef {
            kind: DenoiseArtifactKind::DenoiseRgb,
            relative_path: "bench.lumina.zdata".into(),
            format: "lumina-zdata".into(),
            checksum: artifact_checksum.into(),
            width: size,
            height: size,
            channels: "rgb8".into(),
            data_version: DENOISE_RGB_ENCODING_VERSION.to_string(),
            extras: default_extras(),
        }),
        extras: default_extras(),
    }
}

/// Recorded producer identity matching [`make_denoise_ai`], for the `ready`
/// §6 status-classification benchmark.
pub fn make_denoise_identity(artifact_checksum: &str) -> DenoiseIdentity {
    DenoiseIdentity {
        source_content_hash: "blake3:bench-source".into(),
        decode_fingerprint: "libraw:1:bench".into(),
        model_name: "bench-fixture-srgb".into(),
        model_version: "1".into(),
        model_hash: bench_model_hash(),
        input_spec_digest: bench_input_spec_digest(),
        artifact_checksum: artifact_checksum.into(),
    }
}

// ---------------------------------------------------------------------------
// Assisted-culling fixtures (F-074 / LRPAR-G09-CULL-25)
// ---------------------------------------------------------------------------

/// Sidecar document + live identity for the culling status-classification
/// benchmark (`evaluate_culling`, the CLI `--status` / GUI badge read path).
/// Built from a deterministic analysis; no file I/O, no model, no network.
pub fn make_culling_status_fixture(size: u32) -> (SidecarDocument, CullingIdentity) {
    let frame = make_frame(size);
    let decode = DecodeFingerprint {
        decoder: "libraw".into(),
        version: "1".into(),
        parameters: BTreeMap::new(),
        extras: default_extras(),
    };
    let geometry = GeometryFingerprint {
        width: size,
        height: size,
        orientation: 1,
        pixel_aspect_ratio: 1.0,
        extras: default_extras(),
    };
    let source = SourceFingerprint {
        content_hash: "blake3:bench-source".into(),
        byte_length: 1,
        extras: default_extras(),
    };
    let source_identity = SourceIdentity {
        relative_name: "BENCH.ARW".into(),
        content_hash: "blake3:bench-source".into(),
        byte_length: 1,
        modified_at: None,
        raw_format: "ARW".into(),
        orientation: 1,
        decode_fingerprint: decode.clone(),
        geometry_fingerprint: geometry.clone(),
        extras: default_extras(),
    };
    let analysis = analyze_heuristic(
        &CullSourceInput {
            frame: &frame,
            iso: None,
        },
        &CullConfig::default(),
    )
    .expect("deterministic culling analysis");
    let identity = analysis
        .core
        .identity(source.clone(), decode.clone(), geometry.clone());
    let section = analysis
        .core
        .to_section(source, decode, geometry, "2026-09-17T00:00:00Z")
        .expect("deterministic culling section");
    let mut document = SidecarDocument::new(source_identity, "bench-pipeline");
    record_culling(&mut document, section).expect("recording a valid proposal");
    (document, identity)
}
