use super::*;

/// Build a [`ModelIdentity`] whose `extras` optionally carries the
/// `INPUT_SPEC_DIGEST_KEY` digest — mirroring what the ONNX adapter's
/// `to_model_identity` writes. `None` produces a legacy identity without
/// the key (an old sidecar before R2-ONNX-01).
fn identity_with_digest(digest: Option<&str>) -> ModelIdentity {
    let mut extras = Extras::new();
    if let Some(digest) = digest {
        extras.insert(
            INPUT_SPEC_DIGEST_KEY.to_owned(),
            serde_json::Value::String(digest.to_owned()),
        );
    }
    ModelIdentity {
        name: "BiRefNet".into(),
        version: "1.0.0".into(),
        hash: "h".into(),
        extras,
    }
}

// R2-ONNX-01 — the decision layer [`model_identity_matches`] must honor the
// persisted input-spec digest in `extras` exactly as the producer writes
// it, closing the stale-detection hole where a normalization/resolution
// change kept a cached mask `valid` despite changed inference semantics.

#[test]
fn same_input_spec_digest_matches() {
    // Identical digests → the persisted mask stays confirmably valid.
    assert!(model_identity_matches(
        &identity_with_digest(Some("sha256:aaaa")),
        Some(&identity_with_digest(Some("sha256:aaaa"))),
    ));
}

#[test]
fn changed_input_spec_digest_does_not_match() {
    // Only the inference contract (digest) differs; name/version/hash are
    // identical — this is exactly the stale hole R2-ONNX-01 closes.
    assert!(!model_identity_matches(
        &identity_with_digest(Some("sha256:aaaa")),
        Some(&identity_with_digest(Some("sha256:bbbb"))),
    ));
}

#[test]
fn digest_present_on_one_side_only() {
    // Persisted mask carries a digest the configured context lacks → the
    // context is older/foreign (incomparable) → treat as changed.
    assert!(!model_identity_matches(
        &identity_with_digest(Some("sha256:aaaa")),
        Some(&identity_with_digest(None)),
    ));
    // The persisted mask predates the digest feature (or is synthetic) but
    // the configured context carries one → fall back to the legacy
    // name/version/hash comparison (already checked above) → still matches.
    assert!(model_identity_matches(
        &identity_with_digest(None),
        Some(&identity_with_digest(Some("sha256:aaaa"))),
    ));
}

#[test]
fn legacy_identity_without_digest_still_matches() {
    // Old sidecars (no digest key) fall back to the legacy comparison.
    assert!(model_identity_matches(
        &identity_with_digest(None),
        Some(&identity_with_digest(None)),
    ));
    // ...and a legacy identity still fails when name/version/hash differ.
    assert!(!model_identity_matches(
        &ModelIdentity {
            name: "Other".into(),
            version: "1.0.0".into(),
            hash: "h".into(),
            extras: Extras::new(),
        },
        Some(&identity_with_digest(None)),
    ));
}

#[test]
fn changed_input_spec_digest_triggers_reinference() {
    // End-to-end: a persisted mask whose digest differs from the currently
    // configured model must be re-inferred, never served as
    // `LoadedPersisted`.
    let definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Valid,
        "src",
        identity_with_digest(Some("sha256:OLD")),
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
        Some(identity_with_digest(Some("sha256:NEW"))),
        false,
        "src",
    )
    .unwrap();
    assert_eq!(result.outcomes[0].from, MaskResolvedFrom::ReInferred);
}

#[test]
fn same_input_spec_digest_prefers_persisted() {
    // End-to-end mirror of the above: identical digest keeps the persisted
    // plane (no re-inference).
    let definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Valid,
        "src",
        identity_with_digest(Some("sha256:SAME")),
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
        Some(identity_with_digest(Some("sha256:SAME"))),
        false,
        "src",
    )
    .unwrap();
    assert_eq!(result.outcomes[0].from, MaskResolvedFrom::LoadedPersisted);
}

#[test]
fn changed_model_is_reinferred_when_available() {
    let definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Valid,
        "src",
        // Persisted for a different model identity.
        ModelIdentity {
            name: "OldModel".into(),
            version: "0.9".into(),
            hash: "old".into(),
            extras: Extras::new(),
        },
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
fn missing_persisted_plane_is_reinferred_when_model_available() {
    let definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Valid,
        "src",
        model_identity(),
        decode_context(),
        true,
    );
    // No loaded plane (artifact file missing) → re-infer.
    let result = resolve_one(
        definition,
        None,
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
fn model_unavailable_uses_cached_stale_mask_with_warning() {
    // No model wired at all; a persisted (stale) plane exists.
    let definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Stale,
        "src",
        model_identity(),
        decode_context(),
        true,
    );
    let result = resolve_one(definition, Some(PERSISTED_VALUE), None, None, false, "src").unwrap();
    assert_eq!(result.outcomes[0].from, MaskResolvedFrom::CachedUnavailable);
    assert!(result.model_unavailable);
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("model is unavailable"));
    assert!(result.warnings[0].contains("stale"));
    assert_eq!(
        result
            .planes
            .get(&("vc".into(), "subject".into()))
            .unwrap()
            .values,
        vec![PERSISTED_VALUE; 16]
    );
}

// REVIEW-MASK-N3: without a configured expected model identity the
// persisted artifact's model context cannot be confirmed — it must be
// treated as stale (cache-with-warning / re-inference / hard error),
// never as confirmably valid.
#[test]
fn unconfirmable_model_identity_is_never_loaded_persisted() {
    let definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Valid,
        "src",
        model_identity(),
        decode_context(),
        true,
    );
    // No model wired at all: the otherwise perfectly matching artifact
    // may not silently pass as `LoadedPersisted`. The F-051 path serves
    // it from cache WITH an explicit staleness warning instead.
    let result = resolve_one(
        definition.clone(),
        Some(PERSISTED_VALUE),
        None,
        None,
        false,
        "src",
    )
    .unwrap();
    assert_eq!(result.outcomes[0].from, MaskResolvedFrom::CachedUnavailable);
    assert!(result.model_unavailable);
    assert_eq!(result.warnings.len(), 1);
    assert!(!result.warnings.is_empty());

    // With an available model but no configured identity to compare
    // against, validity is likewise unconfirmable → re-inference.
    let result = resolve_one(
        definition,
        Some(PERSISTED_VALUE),
        Some(&FakeInference {
            available: true,
            value: INFERRED_VALUE,
        }),
        None,
        false,
        "src",
    )
    .unwrap();
    assert_eq!(result.outcomes[0].from, MaskResolvedFrom::ReInferred);
    assert!(!result.model_unavailable);
    // F-100/GUI-GEN-GRANULAR-10: the re-inference is surfaced loudly.
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("re-inferred"));
}

#[test]
fn model_unavailable_without_cache_is_a_hard_error() {
    let definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Missing,
        "src",
        model_identity(),
        decode_context(),
        false,
    );
    // No model and no persisted plane → hard error, never a silent fallback.
    let error = resolve_one(definition, None, None, None, false, "src").unwrap_err();
    assert!(matches!(
        error,
        CoreError::MaskUnavailable {
            ref copy_id,
            ref mask_id,
            ref status,
        } if copy_id == "vc" && mask_id == "subject" && status == "model-unavailable"
    ));
}

#[test]
fn model_unavailable_with_missing_plane_is_a_hard_error_even_if_status_valid() {
    let definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Valid,
        "src",
        model_identity(),
        decode_context(),
        true,
    );
    // Status says Valid, but the artifact cannot be loaded and no model is
    // available. Validity cannot be confirmed → treated as missing → error.
    let error = resolve_one(definition, None, None, None, false, "src").unwrap_err();
    assert!(matches!(
        error,
        CoreError::MaskUnavailable {
            ref status,
            ..
        } if status == "model-unavailable"
    ));
}

#[test]
fn cross_copy_source_is_resolved() {
    // Active copy `vc` references a source mask in `other`.
    let source = source_definition(
        "other",
        "subject",
        MaskStatus::Valid,
        "src",
        model_identity(),
        decode_context(),
        true,
    );
    let mut copy = copy_with("vc", vec![]);
    copy.mask_layers = vec![layer_for("other", "subject")];
    let other = copy_with("other", vec![source]);
    let loaded_planes = BTreeMap::from([(
        ("other".into(), "subject".into()),
        MaskPlane::new(4, 4, vec![PERSISTED_VALUE; 16]).unwrap(),
    )]);
    let result = resolve_mask_planes(
        MaskLoadContext {
            copies: &[copy, other],
            active_copy_id: "vc",
            source_hash: "src",
            decode_context: &decode_context(),
            loaded_planes,
            inference: Some(&FakeInference {
                available: true,
                value: INFERRED_VALUE,
            }),
            model_identity: Some(model_identity()).as_ref(),
            refresh: false,
            policy: MaskPolicy::Warn,
        },
        &frame(),
    )
    .unwrap();
    assert_eq!(result.outcomes[0].from, MaskResolvedFrom::LoadedPersisted);
    assert!(result
        .planes
        .contains_key(&("other".into(), "subject".into())));
}

#[test]
fn blessing_pass_resolves_source_and_derived_masks() {
    // A source mask plus a derived (invert) mask referencing it. Both must
    // be resolved by the decision layer when the source is reachable and
    // valid: the source is loaded, and the derived mask is "blessed"
    // (marked Valid) because its only dependency was resolved.
    let source = source_definition(
        "vc",
        "subject",
        MaskStatus::Valid,
        "src",
        model_identity(),
        decode_context(),
        true,
    );
    let derived = MaskDefinition {
        id: "inverted".into(),
        name: "inverted".into(),
        source_fingerprint: SourceFingerprint {
            content_hash: "src".into(),
            byte_length: 1,
            extras: Extras::new(),
        },
        decode_context: decode_context(),
        geometry_context: GeometryFingerprint {
            width: 4,
            height: 4,
            orientation: 1,
            pixel_aspect_ratio: 1.0,
            extras: Extras::new(),
        },
        model: model_identity(),
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
        status: MaskStatus::Valid,
        created_at: "now".into(),
        generator_version: "g".into(),
        error_text: None,
        artifact: None,
        operation: MaskOperation::Invert,
        references: vec![MaskReference {
            copy_id: "vc".into(),
            mask_id: "subject".into(),
            extras: Extras::new(),
        }],
        prompt: None,
        extras: Extras::new(),
        ai_select: None,
    };
    let mut copy = copy_with("vc", vec![source, derived]);
    copy.mask_layers = vec![layer_for("vc", "subject"), layer_for("vc", "inverted")];
    let loaded_planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(4, 4, vec![PERSISTED_VALUE; 16]).unwrap(),
    )]);
    let result = resolve_mask_planes(
        MaskLoadContext {
            copies: &[copy],
            active_copy_id: "vc",
            source_hash: "src",
            decode_context: &decode_context(),
            loaded_planes,
            inference: Some(&FakeInference {
                available: true,
                value: INFERRED_VALUE,
            }),
            model_identity: Some(model_identity()).as_ref(),
            refresh: false,
            policy: MaskPolicy::Warn,
        },
        &frame(),
    )
    .unwrap();

    // The source plane is loaded and resolved.
    assert!(result.planes.contains_key(&("vc".into(), "subject".into())));
    assert_eq!(
        result
            .planes
            .get(&("vc".into(), "subject".into()))
            .unwrap()
            .values,
        vec![PERSISTED_VALUE; 16]
    );
    // The source resolution is reported as loaded-from-persisted.
    assert!(result
        .outcomes
        .iter()
        .any(|o| o.mask_id == "subject" && o.from == MaskResolvedFrom::LoadedPersisted));
    // The derived mask is blessed (no own plane, but marked Valid) so the
    // downstream MaskGraph evaluation can resolve it.
    let blessed = result
        .copies
        .iter()
        .flat_map(|c| c.mask_library.iter())
        .find(|m| m.id == "inverted")
        .expect("derived mask present in result copies");
    assert_eq!(blessed.status, MaskStatus::Valid);
}

// --- F-050: comprehensive invalidation / re-inference coverage ----------

#[test]
fn decode_context_change_is_reinferred_when_model_available() {
    // The persisted mask was decoded with a different LibRaw/decode
    // fingerprint than the current source; it is therefore not confirmable
    // and must be re-inferred rather than loaded.
    let definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Valid,
        "src",
        model_identity(),
        decode_context(),
        true,
    );
    let mut ctx_decode = decode_context();
    ctx_decode.version = "2".into();
    let result = resolve_single(
        definition,
        Some(PERSISTED_VALUE),
        Some(&FakeInference {
            available: true,
            value: INFERRED_VALUE,
        }),
        Some(model_identity()),
        false,
        "src",
        ctx_decode,
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
fn decode_context_change_falls_back_to_cache_when_model_unavailable() {
    // Decode context changed but no model is wired: the (possibly stale)
    // cached plane is used with a warning (F-051), never a silent drop.
    let definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Valid,
        "src",
        model_identity(),
        decode_context(),
        true,
    );
    let mut ctx_decode = decode_context();
    ctx_decode.version = "2".into();
    let result = resolve_single(
        definition,
        Some(PERSISTED_VALUE),
        None,
        None,
        false,
        "src",
        ctx_decode,
    )
    .unwrap();
    assert_eq!(result.outcomes[0].from, MaskResolvedFrom::CachedUnavailable);
    assert!(result.model_unavailable);
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("model is unavailable"));
}

#[test]
fn corrupt_status_is_reinferred_when_model_available() {
    // A mask marked `Corrupt` is not confirmable; with a model it is
    // re-inferred (no reuse of the corrupt artifact).
    let definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Corrupt,
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
fn corrupt_status_falls_back_to_cache_when_model_unavailable() {
    let definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Corrupt,
        "src",
        model_identity(),
        decode_context(),
        true,
    );
    let result = resolve_one(definition, Some(PERSISTED_VALUE), None, None, false, "src").unwrap();
    assert_eq!(result.outcomes[0].from, MaskResolvedFrom::CachedUnavailable);
    assert!(result.model_unavailable);
    assert!(result.warnings[0].contains("Corrupt"));
}

#[test]
fn inference_failure_is_hard_error_not_silent_fallback() {
    // The mask is `Stale` (re-inference required) and a cached plane exists,
    // but re-inference fails. F-051 forbids silently falling back to the
    // stale cache — the failure is surfaced as a hard error.
    let definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Stale,
        "src",
        model_identity(),
        decode_context(),
        true,
    );
    let error = resolve_one(
        definition,
        Some(PERSISTED_VALUE),
        Some(&FailingInference { available: true }),
        Some(model_identity()),
        false,
        "src",
    )
    .unwrap_err();
    assert!(matches!(error, CoreError::MaskInference { .. }));
}

#[test]
fn inference_failure_without_cache_is_hard_error() {
    let definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Valid,
        "src",
        model_identity(),
        decode_context(),
        true,
    );
    let error = resolve_one(
        definition,
        None,
        Some(&FailingInference { available: true }),
        Some(model_identity()),
        false,
        "src",
    )
    .unwrap_err();
    assert!(matches!(error, CoreError::MaskInference { .. }));
}

// --- REVIEW-SIDECAR-LOADER-RES: loading-path resolution validation ------

/// A decoded plane whose dimensions match the declared artifact record
/// loads normally: `LoadedPersisted`, no warning, confirmably valid.
#[test]
fn matching_dimensions_load_persisted_normally() {
    let definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Valid,
        "src",
        model_identity(),
        decode_context(),
        true,
    );
    let result = resolve_with_plane(
        definition,
        Some(MaskPlane::new(4, 4, vec![PERSISTED_VALUE; 16]).unwrap()),
        None,
        Some(model_identity()),
    )
    .unwrap();
    assert_eq!(result.outcomes[0].from, MaskResolvedFrom::LoadedPersisted);
    assert_eq!(
        result
            .planes
            .get(&("vc".into(), "subject".into()))
            .unwrap()
            .values,
        vec![PERSISTED_VALUE; 16]
    );
    assert!(result.warnings.is_empty());
    assert_eq!(persisted_status(&result), MaskStatus::Valid);
}

/// Decoded 2x8 against a declared 4x4 record: the artifact cannot be
/// confirmed → with a model available it is re-inferred (never served as
/// confirmably valid and never silently resampled to the declared size).
#[test]
fn dimension_mismatch_is_reinferred_when_model_available() {
    let definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Valid,
        "src",
        model_identity(),
        decode_context(),
        true,
    );
    let result = resolve_with_plane(
        definition,
        Some(MaskPlane::new(2, 8, vec![PERSISTED_VALUE; 16]).unwrap()),
        Some(&FakeInference {
            available: true,
            value: INFERRED_VALUE,
        }),
        Some(model_identity()),
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
    // The fresh matte resolved the layer again — it is not left marked
    // `Corrupt`, because the corrupt persisted artifact was replaced.
    assert_eq!(persisted_status(&result), MaskStatus::Valid);
}

/// Same mismatch without a model (F-051): the cached plane is used with an
/// explicit dimension-mismatch warning and reported as `Corrupt` in the
/// result copies — visible, not a silent fallback or resample.
#[test]
fn dimension_mismatch_marks_corrupt_when_cached_without_model() {
    let definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Valid,
        "src",
        model_identity(),
        decode_context(),
        true,
    );
    let result = resolve_with_plane(
        definition,
        Some(MaskPlane::new(8, 2, vec![PERSISTED_VALUE; 16]).unwrap()),
        None,
        Some(model_identity()),
    )
    .unwrap();
    assert_eq!(result.outcomes[0].from, MaskResolvedFrom::CachedUnavailable);
    assert!(result.model_unavailable);
    // The plane is passed through byte-identical — no resampling.
    assert_eq!(
        result
            .planes
            .get(&("vc".into(), "subject".into()))
            .unwrap()
            .values,
        vec![PERSISTED_VALUE; 16]
    );
    assert!(result.warnings.iter().any(|warning| {
        warning.contains("8x2")
            && warning.contains("4x4")
            && warning.contains("Corrupt")
            && warning.contains("no silent resample")
    }));
    assert_eq!(persisted_status(&result), MaskStatus::Corrupt);
}

/// Without an artifact record there is no declared reference resolution to
/// compare against: the pre-existing behavior applies unchanged (artifact
/// absent ⇒ validity unconfirmable ⇒ F-051 paths). No dimension check and
/// no `Corrupt` override is invented from thin air.
#[test]
fn missing_artifact_reference_keeps_existing_behavior() {
    // Plane present but `artifact: None`.
    let definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Valid,
        "src",
        model_identity(),
        decode_context(),
        false,
    );
    let result = resolve_with_plane(
        definition,
        Some(MaskPlane::new(4, 4, vec![PERSISTED_VALUE; 16]).unwrap()),
        None,
        Some(model_identity()),
    )
    .unwrap();
    assert_eq!(result.outcomes[0].from, MaskResolvedFrom::CachedUnavailable);
    assert!(result.model_unavailable);
    // Only the F-051 cache warning — no dimension-mismatch warning.
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("model is unavailable"));
    // Existing blessing semantics apply untouched.
    assert_eq!(persisted_status(&result), MaskStatus::Valid);
}

// ----- G-03: deterministic range stages + loud AI-select -----

fn range_definition(id: &str, prompt: MaskPrompt) -> MaskDefinition {
    let mut definition = source_definition(
        "vc",
        id,
        MaskStatus::Pending,
        "src",
        model_identity(),
        decode_context(),
        false,
    );
    definition.prompt = Some(prompt);
    definition
}

fn luminance_prompt(min: f32, max: f32) -> MaskPrompt {
    MaskPrompt::LuminanceRange {
        min,
        max,
        feather: 0.0,
        transformation: PromptTransform::default(),
    }
}

fn resolve_with_frame(
    definition: MaskDefinition,
    frame: &ImageFrame,
    refresh: bool,
) -> Result<MaskLoadResult, CoreError> {
    let mut copy = copy_with("vc", vec![definition]);
    copy.mask_layers = vec![layer_for("vc", "subject")];
    resolve_mask_planes(
        MaskLoadContext {
            copies: &[copy],
            active_copy_id: "vc",
            source_hash: "src",
            decode_context: &decode_context(),
            loaded_planes: BTreeMap::new(),
            inference: None,
            model_identity: None,
            refresh,
            policy: MaskPolicy::Warn,
        },
        frame,
    )
}

/// G-03 cache-hit: a range stage resolves without any model, artifact or
/// cache — deterministically computed from the frame — and is `Valid`.
#[test]
fn range_mask_computes_deterministically_without_model() {
    let prompt = luminance_prompt(0.0, 1.0);
    let frame = ImageFrame::new(2, 1, vec![0, 0, 0, 255, 255, 255, 255, 255]).unwrap();
    let expected = crate::range_masks::evaluate_range_prompt(&frame, &prompt).unwrap();
    let result = resolve_with_frame(range_definition("subject", prompt), &frame, false).unwrap();
    assert_eq!(result.outcomes.len(), 1);
    assert_eq!(
        result.outcomes[0].from,
        MaskResolvedFrom::ComputedDeterministic
    );
    assert!(result.warnings.is_empty());
    assert!(!result.model_unavailable);
    assert_eq!(
        result.planes.get(&("vc".into(), "subject".into())).unwrap(),
        &expected
    );
    assert_eq!(persisted_status(&result), MaskStatus::Valid);
}

/// G-03 cache-hit is byte-identical across refreshes (recomputation is
/// the hit); a parameter change is the invalidation (different plane).
#[test]
fn range_mask_hit_identical_and_param_change_invalidates() {
    let frame = ImageFrame::new(2, 1, vec![0, 0, 0, 255, 255, 255, 255, 255]).unwrap();
    let first = resolve_with_frame(
        range_definition("subject", luminance_prompt(0.0, 1.0)),
        &frame,
        false,
    )
    .unwrap();
    let second = resolve_with_frame(
        range_definition("subject", luminance_prompt(0.0, 1.0)),
        &frame,
        true,
    )
    .unwrap();
    assert_eq!(
        first.planes.get(&("vc".into(), "subject".into())).unwrap(),
        second.planes.get(&("vc".into(), "subject".into())).unwrap()
    );
    let changed = resolve_with_frame(
        range_definition("subject", luminance_prompt(0.5, 1.0)),
        &frame,
        false,
    )
    .unwrap();
    assert_ne!(
        first.planes.get(&("vc".into(), "subject".into())).unwrap(),
        changed
            .planes
            .get(&("vc".into(), "subject".into()))
            .unwrap()
    );
}

/// G-03: an AI selection without a model and without a cached plane is a
/// loud hard error — never a silent empty mask.
#[test]
fn ai_select_without_model_and_cache_fails_loudly() {
    let mut definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Pending,
        "src",
        model_identity(),
        decode_context(),
        false,
    );
    definition.ai_select = Some(AiSelect {
        kind: AiSelectKind::People,
        detail: Some("face".into()),
        extras: Extras::new(),
    });
    let error = resolve_one(definition, None, None, None, false, "src").unwrap_err();
    assert!(
        error.to_string().contains("vc/subject"),
        "unexpected error: {error}"
    );
}

/// G-03: an AI selection re-infers through the model path when available.
#[test]
fn ai_select_reinfers_with_available_model() {
    let mut definition = source_definition(
        "vc",
        "subject",
        MaskStatus::Pending,
        "src",
        model_identity(),
        decode_context(),
        false,
    );
    definition.ai_select = Some(AiSelect {
        kind: AiSelectKind::Sky,
        detail: None,
        extras: Extras::new(),
    });
    let result = resolve_one(
        definition,
        None,
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
    assert_eq!(
        result
            .planes
            .get(&("vc".into(), "subject".into()))
            .unwrap()
            .values,
        vec![INFERRED_VALUE; 16]
    );
}
