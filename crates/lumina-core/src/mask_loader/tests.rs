use super::*;
use crate::masks::MaskPlane;
use lumina_sidecar::{
    AiSelect, AiSelectKind, ArtifactReference, CoordinateSystem, EditRecipe, Extras,
    GeometryFingerprint, MaskPrompt, Preprocessing, PromptTransform, Resolution, SourceFingerprint,
};
use std::collections::BTreeMap;

/// Deterministic test backend. `infer` always returns a uniform plane of
/// `value`, which lets a test distinguish a re-inferred plane from a
/// persisted one (a different `value`).
struct FakeInference {
    available: bool,
    value: u16,
}

impl MaskInference for FakeInference {
    fn is_available(&self) -> bool {
        self.available
    }
    fn infer(&self, _frame: &ImageFrame) -> Result<MaskPlane, CoreError> {
        MaskPlane::new(4, 4, vec![self.value; 16]).map_err(|error| CoreError::MaskInference {
            reason: error.to_string(),
        })
    }
}

/// Backend whose `infer` always fails, to assert that a failed re-inference
/// is surfaced as a hard error and never silently falls back to a cached
/// (stale) plane (F-051 / F-050: "keine stillen Fallbacks").
struct FailingInference {
    available: bool,
}

impl MaskInference for FailingInference {
    fn is_available(&self) -> bool {
        self.available
    }
    fn infer(&self, _frame: &ImageFrame) -> Result<MaskPlane, CoreError> {
        Err(CoreError::MaskInference {
            reason: "simulated re-inference failure".into(),
        })
    }
}

/// Like [`resolve_one`] but lets the caller specify the *context's* decode
/// fingerprint independently of the definition's, so decode-context-change
/// scenarios can be exercised.
#[allow(clippy::too_many_arguments)]
fn resolve_single(
    definition: MaskDefinition,
    loaded_plane: Option<u16>,
    inference: Option<&dyn MaskInference>,
    model_identity: Option<ModelIdentity>,
    refresh: bool,
    source_hash: &str,
    ctx_decode: DecodeFingerprint,
) -> Result<MaskLoadResult, CoreError> {
    let mut copy = copy_with("vc", vec![definition]);
    copy.mask_layers = vec![layer_for("vc", "subject")];
    let loaded_planes = loaded_plane
        .map(|value| {
            BTreeMap::from([(
                ("vc".into(), "subject".into()),
                MaskPlane::new(4, 4, vec![value; 16]).unwrap(),
            )])
        })
        .unwrap_or_default();
    let model_ref = model_identity.as_ref();
    resolve_mask_planes(
        MaskLoadContext {
            copies: &[copy],
            active_copy_id: "vc",
            source_hash,
            decode_context: &ctx_decode,
            loaded_planes,
            inference,
            model_identity: model_ref,
            refresh,
            policy: MaskPolicy::Warn,
        },
        &frame(),
    )
}

/// REVIEW-SIDECAR-LOADER-RES: like [`resolve_one`] but takes an explicit
/// [`MaskPlane`], so a test controls the *decoded* plane dimensions
/// independently of the artifact record (always declared 4x4).
fn resolve_with_plane(
    definition: MaskDefinition,
    plane: Option<MaskPlane>,
    inference: Option<&dyn MaskInference>,
    model_identity: Option<ModelIdentity>,
) -> Result<MaskLoadResult, CoreError> {
    let mut copy = copy_with("vc", vec![definition]);
    copy.mask_layers = vec![layer_for("vc", "subject")];
    let loaded_planes = plane
        .map(|value| BTreeMap::from([(("vc".into(), "subject".into()), value)]))
        .unwrap_or_default();
    let model_ref = model_identity.as_ref();
    resolve_mask_planes(
        MaskLoadContext {
            copies: &[copy],
            active_copy_id: "vc",
            source_hash: "src",
            decode_context: &decode_context(),
            loaded_planes,
            inference,
            model_identity: model_ref,
            refresh: false,
            policy: MaskPolicy::Warn,
        },
        &frame(),
    )
}

/// Status of the single `vc`/`subject` mask in the result copies.
fn persisted_status(result: &MaskLoadResult) -> MaskStatus {
    result
        .copies
        .iter()
        .find(|copy| copy.id == "vc")
        .and_then(|copy| copy.mask_library.iter().find(|mask| mask.id == "subject"))
        .map(|mask| mask.status.clone())
        .unwrap()
}

const PERSISTED_VALUE: u16 = 32768;
const INFERRED_VALUE: u16 = 12345;

fn decode_context() -> DecodeFingerprint {
    DecodeFingerprint {
        decoder: "image".into(),
        version: "1".into(),
        parameters: BTreeMap::new(),
        extras: Extras::new(),
    }
}

fn model_identity() -> ModelIdentity {
    ModelIdentity {
        name: "BiRefNet".into(),
        version: "1.0.0".into(),
        hash: "h".into(),
        extras: Extras::new(),
    }
}

fn source_definition(
    _copy_id: &str,
    id: &str,
    status: MaskStatus,
    source_hash: &str,
    model: ModelIdentity,
    decode: DecodeFingerprint,
    with_artifact: bool,
) -> MaskDefinition {
    MaskDefinition {
        id: id.into(),
        name: id.into(),
        source_fingerprint: SourceFingerprint {
            content_hash: source_hash.into(),
            byte_length: 1,
            extras: Extras::new(),
        },
        decode_context: decode,
        geometry_context: GeometryFingerprint {
            width: 4,
            height: 4,
            orientation: 1,
            pixel_aspect_ratio: 1.0,
            extras: Extras::new(),
        },
        model,
        inference_resolution: Resolution {
            width: 4,
            height: 4,
            extras: Extras::new(),
        },
        preprocessing: Preprocessing {
            name: "p".into(),
            version: "1".into(),
            parameters: BTreeMap::new(),
            extras: Extras::new(),
        },
        rescaling_method: "none".into(),
        rescaling_parameters: BTreeMap::new(),
        coordinate_system: CoordinateSystem::SourceOriented,
        status,
        created_at: "now".into(),
        generator_version: "g".into(),
        error_text: None,
        artifact: with_artifact.then(|| ArtifactReference {
            relative_path: "x.zdata".into(),
            format: "lumina-zdata".into(),
            checksum: "c".into(),
            width: 4,
            height: 4,
            channels: "u16".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        }),
        operation: MaskOperation::Source,
        references: vec![],
        prompt: None,
        extras: Extras::new(),
        ai_select: None,
    }
}

fn copy_with(copy_id: &str, definitions: Vec<MaskDefinition>) -> VirtualCopy {
    VirtualCopy {
        id: copy_id.into(),
        name: copy_id.into(),
        is_default: copy_id == "vc",
        rating: 0,
        flag: lumina_sidecar::Flag::Unflagged,
        recipe: EditRecipe::default(),
        mask_library: definitions,
        mask_layers: vec![],
        history: vec![],
        export_records: vec![],
        extras: Extras::new(),
    }
}

fn layer_for(copy_id: &str, mask_id: &str) -> lumina_sidecar::MaskLayer {
    use lumina_sidecar::MaskReference;
    lumina_sidecar::MaskLayer {
        id: format!("layer-{mask_id}"),
        mask: MaskReference {
            copy_id: copy_id.into(),
            mask_id: mask_id.into(),
            extras: Extras::new(),
        },
        inverted: false,
        feather: 0.0,
        blur: 0.0,
        density: 1.0,
        extras: Extras::new(),
        visible: true,
        local_adjustments: None,
    }
}
fn frame() -> ImageFrame {
    ImageFrame::new(4, 4, vec![100; 64]).unwrap()
}

/// Build a single active copy `vc` whose only layer references `subject`.
fn resolve_one(
    definition: MaskDefinition,
    loaded_plane: Option<u16>,
    inference: Option<&dyn MaskInference>,
    model_identity: Option<ModelIdentity>,
    refresh: bool,
    source_hash: &str,
) -> Result<MaskLoadResult, CoreError> {
    let mut copy = copy_with("vc", vec![definition]);
    copy.mask_layers = vec![layer_for("vc", "subject")];
    let loaded_planes = loaded_plane
        .map(|value| {
            BTreeMap::from([(
                ("vc".into(), "subject".into()),
                MaskPlane::new(4, 4, vec![value; 16]).unwrap(),
            )])
        })
        .unwrap_or_default();
    let model_ref = model_identity.as_ref();
    resolve_mask_planes(
        MaskLoadContext {
            copies: &[copy],
            active_copy_id: "vc",
            source_hash,
            decode_context: &decode_context(),
            loaded_planes,
            inference,
            model_identity: model_ref,
            refresh,
            policy: MaskPolicy::Warn,
        },
        &frame(),
    )
}

#[test]
fn valid_persisted_mask_is_preferred_without_reinference() {
    let definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Valid,
        "src",
        model_identity(),
        decode_context(),
        true,
    );
    let result = resolve_one(
        definition,
        Some(PERSISTED_VALUE),
        Some(&FakeInference {
            available: true,
            value: INFERRED_VALUE,
        }),
        Some(model_identity()),
        false,
        "src",
    )
    .unwrap();
    assert_eq!(result.outcomes.len(), 1);
    assert_eq!(result.outcomes[0].from, MaskResolvedFrom::LoadedPersisted);
    assert_eq!(
        result
            .planes
            .get(&("vc".into(), "subject".into()))
            .unwrap()
            .values,
        vec![PERSISTED_VALUE; 16]
    );
    assert!(!result.model_unavailable);
    assert!(result.warnings.is_empty());
}

#[test]
fn refresh_forces_reinference_even_when_valid() {
    let definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Valid,
        "src",
        model_identity(),
        decode_context(),
        true,
    );
    let result = resolve_one(
        definition,
        Some(PERSISTED_VALUE),
        Some(&FakeInference {
            available: true,
            value: INFERRED_VALUE,
        }),
        Some(model_identity()),
        true,
        "src",
    )
    .unwrap();
    assert_eq!(result.outcomes[0].from, MaskResolvedFrom::ReInferred);
    assert_eq!(
        result
            .planes
            .get(&("vc".into(), "subject".into()))
            .unwrap()
            .values,
        vec![INFERRED_VALUE; 16]
    );
}

#[test]
fn stale_mask_is_reinferred_when_model_available() {
    let definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Stale,
        "src",
        model_identity(),
        decode_context(),
        true,
    );
    let result = resolve_one(
        definition,
        Some(PERSISTED_VALUE),
        Some(&FakeInference {
            available: true,
            value: INFERRED_VALUE,
        }),
        Some(model_identity()),
        false,
        "src",
    )
    .unwrap();
    assert_eq!(result.outcomes[0].from, MaskResolvedFrom::ReInferred);
}

#[test]
fn changed_source_is_reinferred_when_model_available() {
    let definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Valid,
        "OLD-SRC",
        model_identity(),
        decode_context(),
        true,
    );
    // Current source hash differs → persisted mask is not confirmable.
    let result = resolve_one(
        definition,
        Some(PERSISTED_VALUE),
        Some(&FakeInference {
            available: true,
            value: INFERRED_VALUE,
        }),
        Some(model_identity()),
        false,
        "NEW-SRC",
    )
    .unwrap();
    assert_eq!(result.outcomes[0].from, MaskResolvedFrom::ReInferred);
    // No explicit request (no `refresh`, no `Pending` marker): the
    // re-inference stays loud.
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("re-inferred"));
}

/// F-100/GUI-GEN-GRANULAR-10 (M2b): a persisted per-mask `Pending` marker is
/// itself an explicit refresh request — the collective `regenerate` default
/// marks exactly the stale/missing masks this way (without the copy-wide
/// `update_masks` switch). Its re-inference must stay quiet, unlike the
/// implicit stale re-inference above.
#[test]
fn pending_mask_reinference_is_not_reported_as_implicit() {
    let definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Pending,
        "src",
        model_identity(),
        decode_context(),
        true,
    );
    let result = resolve_one(
        definition,
        Some(PERSISTED_VALUE),
        Some(&FakeInference {
            available: true,
            value: INFERRED_VALUE,
        }),
        Some(model_identity()),
        false,
        "src",
    )
    .unwrap();
    assert_eq!(result.outcomes[0].from, MaskResolvedFrom::ReInferred);
    assert!(
        result.warnings.is_empty(),
        "an explicitly requested (Pending) re-inference must be quiet: {:?}",
        result.warnings
    );
}

mod identity;
