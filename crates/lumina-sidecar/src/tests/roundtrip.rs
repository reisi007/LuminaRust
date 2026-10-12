use super::*;

#[test]
fn complete_roundtrip() {
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.analysis_fingerprint = Some(AnalysisFingerprint {
        algorithm: "scene-analysis".into(),
        version: "2.1".into(),
        input_fingerprint: "sha256:analysis-input".into(),
        extras: Extras::from([("analysis_extra".into(), Value::from(true))]),
    });
    d.extras
        .insert("future_root".into(), Value::from("preserved"));

    let mut source_mask = mask("a");
    source_mask
        .source_fingerprint
        .extras
        .insert("future_source_fingerprint".into(), Value::from(7));
    source_mask
        .decode_context
        .parameters
        .insert("quality".into(), "high".into());
    source_mask.geometry_context.pixel_aspect_ratio = 1.25;
    source_mask
        .model
        .extras
        .insert("future_model".into(), Value::from("kept"));
    source_mask.inference_resolution.width = 512;
    source_mask
        .preprocessing
        .parameters
        .insert("mean".into(), "0.5".into());
    source_mask.rescaling_method = "lanczos".into();
    source_mask
        .rescaling_parameters
        .insert("radius".into(), "3".into());
    source_mask.coordinate_system = CoordinateSystem::Normalized;
    source_mask.status = MaskStatus::Corrupt;
    source_mask.created_at = "2026-02-03T04:05:06Z".into();
    source_mask.generator_version = "segmenter-2.4".into();
    source_mask.error_text = Some("model output checksum mismatch".into());
    source_mask.artifact = Some(ArtifactReference {
        relative_path: "masks/a.zdata".into(),
        format: "zdata-mask".into(),
        checksum: "sha256:artifact".into(),
        width: 512,
        height: 256,
        channels: "f32".into(),
        data_version: "1".into(),
        extras: Extras::new(),
    });
    source_mask
        .extras
        .insert("future_mask".into(), Value::from("kept"));
    d.virtual_copies[0].mask_library.push(source_mask);
    d.virtual_copies.push(VirtualCopy {
        id: "vc-bw".into(),
        name: "B&W".into(),
        is_default: false,
        rating: 0,
        flag: Flag::Unflagged,
        recipe: EditRecipe {
            recipe_version: "1".into(),
            adjustments: BTreeMap::from([("exposure".into(), 1.25)]),
            curves: None,
            hsl: None,
            point_color: None,
            color_grading: None,
            presence: None,
            noise_reduction: None,
            denoise_ai: None,
            sharpening: None,
            red_eye: None,
            geometry: None,
            lens_correction: None,
            perspective: None,
            upright: None,
            effects: None,
            lens_blur: None,
            generative_edit: None,
            source_actions: Vec::new(),
            spot_removals: Vec::new(),
            options: BTreeMap::from([("profile".into(), "neutral".into())]),
            auto_features: AutoFeatures::default(),
            extras: Extras::from([("future_recipe".into(), Value::from(42))]),
        },
        mask_library: vec![],
        mask_layers: vec![MaskLayer {
            id: "layer".into(),
            mask: MaskReference {
                copy_id: "vc-original".into(),
                mask_id: "a".into(),
                extras: Extras::new(),
            },
            inverted: false,
            feather: 0.0,
            blur: 0.0,
            density: 1.0,
            extras: Extras::new(),
            visible: true,
            local_adjustments: None,
        }],
        history: vec![HistoryEntry {
            id: "h".into(),
            recipe: EditRecipe {
                recipe_version: "1".into(),
                adjustments: BTreeMap::from([("contrast".into(), -0.4)]),
                curves: None,
                hsl: None,
                point_color: None,
                color_grading: None,
                presence: None,
                noise_reduction: None,
                denoise_ai: None,
                sharpening: None,
                red_eye: None,
                geometry: None,
                lens_correction: None,
                perspective: None,
                upright: None,
                effects: None,
                lens_blur: None,
                generative_edit: None,
                source_actions: Vec::new(),
                spot_removals: Vec::new(),
                options: BTreeMap::from([("source".into(), "preset".into())]),
                auto_features: AutoFeatures::default(),
                extras: Extras::new(),
            },
            recorded_at: Some("2026-02-03T04:05:06Z".into()),
            extras: Extras::from([("future_history".into(), Value::from(true))]),
        }],
        export_records: vec![ExportRecord {
            id: "e".into(),
            relative_path: "exports/out.jpg".into(),
            format: "jpeg".into(),
            exported_at: Some("2026-02-03T04:06:06Z".into()),
            extras: Extras::from([("future_export".into(), Value::from("kept"))]),
        }],
        extras: Extras::from([("future_copy".into(), Value::from(true))]),
    });
    d.presets.push(Preset {
        id: "preset-1".into(),
        name: "Monochrome Contrast".into(),
        recipe: EditRecipe {
            recipe_version: "1".into(),
            adjustments: BTreeMap::from([("highlights".into(), -0.75)]),
            curves: None,
            hsl: None,
            point_color: None,
            color_grading: None,
            presence: None,
            noise_reduction: None,
            denoise_ai: None,
            sharpening: None,
            red_eye: None,
            geometry: None,
            lens_correction: None,
            perspective: None,
            upright: None,
            effects: None,
            lens_blur: None,
            generative_edit: None,
            source_actions: Vec::new(),
            spot_removals: Vec::new(),
            options: BTreeMap::from([("curve".into(), "film".into())]),
            auto_features: AutoFeatures::default(),
            extras: Extras::new(),
        },
        extras: Extras::new(),
    });
    let json = d.to_json().unwrap();
    assert_eq!(d, SidecarDocument::from_json(&json).unwrap());
}

#[test]
fn empty_sidecar_roundtrip() {
    let d = SidecarDocument::new(source(), "pipeline-1");
    let json = d.to_json().unwrap();
    assert_eq!(d, SidecarDocument::from_json(&json).unwrap());
}

#[test]
fn auto_features_roundtrip_with_result_and_fingerprint() {
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    let features = &mut d.virtual_copies[0].recipe.auto_features;
    features.enable_auto_tone = true;
    features.match_total_exposure = true;
    features.target_luminance = 0.42;
    features.auto_exposure = Some(1.25);
    features.auto_contrast = Some(-0.2);
    features.auto_whites = Some(0.35);
    features.auto_blacks = Some(-0.45);
    features.auto_highlights = Some(0.15);
    features.auto_shadows = Some(-0.25);
    features.matched_exposure = Some(0.5);
    features.analysis_fingerprint = Some(AnalysisFingerprint {
        algorithm: "tone-rgba8-rec709".into(),
        version: "1".into(),
        input_fingerprint: "tone-rgba8-rec709:abc".into(),
        extras: Extras::new(),
    });
    let json = d.to_json().unwrap();
    assert!(json.contains("auto_exposure"));
    assert!(json.contains("auto_whites"));
    assert!(json.contains("auto_blacks"));
    assert!(json.contains("auto_highlights"));
    assert!(json.contains("auto_shadows"));
    assert!(json.contains("tone-rgba8-rec709:abc"));
    assert_eq!(d, SidecarDocument::from_json(&json).unwrap());
    assert!(d.validate().is_ok());
}

// =====================================================================
// JSON-FLOAT-ROUNDTRIP (owner decision `JSON-ROUNDTRIP`, 2026-10-02):
// every persisted `f64` survives a `save` -> `load` -> `save` cycle
// bit-identically, and no digest over the serialized bytes moves.
//
// The values are pinned as **literals** on purpose, at the measured boundary
// and no further: the 1-ULP loss happens once the `u64` significand exceeds
// 2^53 (a significand `f64` cannot hold exactly) or once `10^|exp|` stops
// being exact (`|exp| > 22`) — **not** once a literal needs 17 significant
// digits. Measured over 2,000,000 samples of this same splitmix64 draw
// against a real feature-off build of `serde_json` 1.0.151: at ≤ 15
// significant digits **nothing** lost (0 of 180,915); 16-digit significands
// above 2^53 lost as well (63,278 of 1,302,251 = 4.86 %, e.g.
// `0.9126441839803001` → `0.9126441839803`); 17-digit ones lost most (93,658
// of 516,833 = 18.12 %). 17 significant digits therefore **implies** the loss
// class (≥ 10^16 > 2^53), it does not define it. These six mirrors and the
// corpus further down need exactly 17 significant digits, so each of them
// sits inside the class by the sufficient witness; the guards in
// `auto_tone_float.rs` (`significant_digits(..) >= 17`) read a **narrower**
// set than the true class and can only undercount it — fail-safe in the
// conservative direction, which is why no assertion here stands on the digit
// count being the boundary. The two marked `measured lossy 2026-09-26` are
// the very values the feature-off build dropped 1 ULP on, carried over from
// `crates/lumina-cli/src/tests/auto_tone_float.rs`.
// =====================================================================

/// The auto-tone mirror values, in `AutoFeatures` field order.
const MIRROR_LOSS_VALUES: [(&str, f64); 6] = [
    ("auto_exposure", -0.20253906249999998), // measured lossy 2026-09-26
    ("auto_contrast", 0.013476562499999997), // measured lossy 2026-09-26
    ("auto_whites", 0.16653084324880307),
    ("auto_blacks", 0.13614329540558123),
    ("auto_highlights", -0.29034012055496694),
    ("auto_shadows", -0.14210602365280334),
];

/// The mask-local scalars, in `LOCAL_ADJUSTMENT_RANGES` order. The two marked
/// literals are the ones the feature-off build of 2026-09-26 dropped 1 ULP on
/// (carried over verbatim from the CLI side so the corpus provenance stays
/// auditable); the unmarked ones are members of the splitmix64 stream named
/// below.
///
/// **Provenance of the whole 20-literal corpus, corrected in the verification
/// round of 2026-10-12 (DoD §9).** The previous note claimed every remaining
/// literal is a member of the same splitmix64 draw as the 200,000-sample
/// domain measurement and that none was invented by hand. Recounted against
/// the stream itself (seed `0x243f6a8885a308d3`, `unit = (z >> 11) / 2^53`):
/// that holds for **19 of the 20** entries — 4 entries are the two
/// `measured lossy 2026-09-26` values (each used twice), and 15 are stream
/// members: 13 of them as `unit * 2 - 1`, `exposure` (`1.5695229505050552`) as
/// `unit * 20 - 10` and `temperature_delta_k` (`-2328.6878550005117`) as
/// `unit * 10000 - 5000` — the same stream, a different linear map per field.
/// It does **not** hold for `wb_temperature = 10415.777975832698`, which is a
/// member under no linear map: that one literal was placed by hand. What this
/// corpus actually rests on, and what is verified, is the class property:
/// every one of the 20 literals is the text `serde_json`'s own serializer
/// writes (the shortest round-tripping form) and needs exactly 17 significant
/// digits — which **implies** the loss condition (`u64` significand > 2^53, or
/// `10^|exp|` inexact for `|exp| > 22`) without being that condition: the
/// true loss class is wider, 16-digit significands above 2^53 lost a bit too
/// (measured 63,278 of 1,302,251 = 4.86 %, against 93,658 of 516,833 =
/// 18.12 % at 17 digits, over 2,000,000 samples of the same splitmix64 draw).
/// The `>= 17` guards are therefore a **narrower** set than the class they
/// witness: they can only undercount it, i.e. they fail safe in the
/// conservative direction, and nothing here may lean on them as the boundary.
const LOCAL_LOSS_VALUES: [(&str, f64); 8] = [
    ("exposure", -0.20253906249999998), // measured lossy 2026-09-26
    ("contrast", 0.013476562499999997), // measured lossy 2026-09-26
    ("highlights", -0.40265701370987705),
    ("shadows", 0.30660334957630253),
    ("temperature_delta_k", -2328.6878550005117),
    ("tint_delta", -0.18431407904044672),
    ("vibrance", 0.46295146649924623),
    ("saturation", -0.12930242027940353),
];

/// A document carrying a 17-significant-digit `f64` in **every** persisted
/// `f64` field the task names: the six `auto_features` mirrors,
/// `matched_exposure`, `target_luminance`, `EditRecipe::adjustments`, and the
/// eight mask-local scalars of `LocalAdjustments`.
fn lossy_value_document() -> SidecarDocument {
    let mut document = SidecarDocument::new(source(), "pipeline-1");
    let recipe = &mut document.virtual_copies[0].recipe;

    let features = &mut recipe.auto_features;
    features.enable_auto_tone = true;
    features.match_total_exposure = true;
    features.target_luminance = 0.050431953687194664;
    features.auto_exposure = Some(MIRROR_LOSS_VALUES[0].1);
    features.auto_contrast = Some(MIRROR_LOSS_VALUES[1].1);
    features.auto_whites = Some(MIRROR_LOSS_VALUES[2].1);
    features.auto_blacks = Some(MIRROR_LOSS_VALUES[3].1);
    features.auto_highlights = Some(MIRROR_LOSS_VALUES[4].1);
    features.auto_shadows = Some(MIRROR_LOSS_VALUES[5].1);
    features.matched_exposure = Some(0.10705782901764338);
    features.analysis_fingerprint = Some(AnalysisFingerprint {
        algorithm: "tone-rgba8-rec709".into(),
        version: "1".into(),
        input_fingerprint: "tone-rgba8-rec709:scheme".into(),
        extras: Extras::new(),
    });

    recipe
        .adjustments
        .insert("exposure".into(), 1.5695229505050552);
    recipe
        .adjustments
        .insert("contrast".into(), -0.15769134665636964);
    recipe
        .adjustments
        .insert("wb_temperature".into(), 10415.777975832698);
    recipe
        .adjustments
        .insert("vibrance".into(), 0.45078177482447446);

    let mut local = LocalAdjustments::default();
    for (key, value) in LOCAL_LOSS_VALUES {
        local.set_value(key, value).expect("in-range loss value");
    }
    document.virtual_copies[0].mask_library.push(mask("a"));
    document.virtual_copies[0].mask_layers.push(MaskLayer {
        id: "layer-loss".into(),
        mask: MaskReference {
            copy_id: "vc-original".into(),
            mask_id: "a".into(),
            extras: Extras::new(),
        },
        inverted: false,
        feather: 0.0,
        blur: 0.0,
        density: 1.0,
        extras: Extras::new(),
        visible: true,
        local_adjustments: Some(local),
    });

    document
        .validate()
        .expect("the lossy-value document must be a valid sidecar");
    document
}

/// Every persisted `f64` survives a `save` -> `load` -> `save` cycle
/// bit-identically. This is the class statement, not a single example: the
/// document carries a 17-significant-digit value in each of the six
/// `auto_features` mirrors, in `matched_exposure`, `target_luminance`, in
/// `EditRecipe::adjustments` and in all eight mask-local scalars.
///
/// **Not vacuous** (DoD §10): with `float_roundtrip` removed from the
/// manifests the emitted text of at least one of these values denotes a
/// different `f64`, and this test goes **red**. Measured in the mutation run.
#[test]
fn every_persisted_f64_survives_a_save_load_save_cycle_bit_identically() {
    let document = lossy_value_document();
    let first = document.to_json().unwrap();

    let loaded = SidecarDocument::from_json(&first).unwrap();
    assert_eq!(
        document, loaded,
        "a loaded document must equal the written one bit-for-bit (f64 Eq is bit-exact for \
         non-NaN)"
    );

    let second = loaded.to_json().unwrap();
    assert_eq!(
        first, second,
        "re-serializing a loaded document must rewrite the identical bytes; a difference here \
         means the parser lost a bit and the serializer reshaped it"
    );

    // The mask-local scalars must survive the typed layer path too, not only
    // the free-form `adjustments` map.
    let layer = &loaded.virtual_copies[0].mask_layers[0];
    let local = layer
        .effective_local_adjustments()
        .unwrap()
        .expect("typed local recipe");
    for (key, _) in LOCAL_LOSS_VALUES {
        assert_eq!(
            local.value(key),
            Some(lossy_value(key)),
            "local scalar `{key}` must come back bit-identically"
        );
    }
}

/// A recipe built **in memory** keeps its digest across the boundary. All the
/// workspace digests (`RenderKey::recipe_hash`, `mask_layers_digest`,
/// `LocalAdjustments::digest`, `MaskStateSnapshot::digest`) are blake3 over
/// serialized bytes, and the serializer is unaffected by the feature — so a
/// digest can only move through a value that was **parsed** first. This test
/// pins that half of the statement: the same values, serialized, hash the same
/// no matter how often they crossed the boundary.
///
/// **Not vacuous**: the expectation is a literal-independent comparison of two
/// hashes taken from the production serializer, not a value derived from the
/// function under test. Reverting the feature stops changing these hashes as
/// well, which is exactly why the load-side test above is the one that guards
/// the class.
#[test]
fn a_recipe_built_in_memory_keeps_its_digest_across_the_boundary() {
    let document = lossy_value_document();
    let layer = document.virtual_copies[0].mask_layers[0].clone();

    let digest_of_document = |doc: &SidecarDocument| -> String {
        let bytes = serde_json::to_vec(doc).expect("SidecarDocument is serializable");
        blake3::hash(&bytes).to_hex().to_string()
    };

    let before = digest_of_document(&document);
    let reloaded = SidecarDocument::from_json(&document.to_json().unwrap()).unwrap();
    assert_eq!(
        before,
        digest_of_document(&reloaded),
        "the document digest must be stable across a save/load cycle"
    );
    // ... and still stable after another full round trip, so a rewrite running
    // twice over the same file cannot move an identity.
    let again = SidecarDocument::from_json(&reloaded.to_json().unwrap()).unwrap();
    assert_eq!(before, digest_of_document(&again));

    // The typed local object and the layer/mask-state digests agree with the
    // free-form map: they are all blake3 over serialized bytes, and the local
    // scalars are the ones that used to drift.
    let local = layer
        .effective_local_adjustments()
        .unwrap()
        .expect("typed local recipe");
    assert_eq!(local, lossy_value_local());
    let layer_digest = layer
        .local_adjustments
        .as_ref()
        .expect("typed local recipe")
        .digest();
    assert_eq!(
        layer_digest,
        lossy_value_local().digest(),
        "the typed local digest must be a function of the value, not of the parse history"
    );
    assert_eq!(
        mask_layers_digest(std::slice::from_ref(&layer)),
        mask_layers_digest(&[lossy_value_layer()])
    );
    assert_eq!(
        MaskStateSnapshot::new(vec![layer]).digest(),
        MaskStateSnapshot::new(vec![lossy_value_layer()]).digest()
    );
}

fn lossy_value(key: &str) -> f64 {
    LOCAL_LOSS_VALUES
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| *value)
        .unwrap_or_else(|| panic!("unknown local adjustment `{key}`"))
}

fn lossy_value_local() -> LocalAdjustments {
    let mut local = LocalAdjustments::default();
    for (key, value) in LOCAL_LOSS_VALUES {
        local.set_value(key, value).expect("in-range loss value");
    }
    local
}

fn lossy_value_layer() -> MaskLayer {
    let mut document = lossy_value_document();
    document.virtual_copies[0].mask_layers.remove(0)
}
