use super::*;
use crate::auto_tone_cli::auto_tone_input_fingerprint;
use lumina_sidecar::AnalysisFingerprint;

/// Explicit `--module auto-tone` writes the full six-slider contract and
/// leaves the mask artifacts and the matching value untouched.
#[test]
fn regenerate_auto_tone_module_is_independent_and_persisted() {
    let directory = tempfile::tempdir().unwrap();
    let (input, frame) = regenerate_input(directory.path(), "regen-auto-tone.png");
    let bytes = fs::read(&input).unwrap();
    write_sidecar_with_valid_layer(&input, &bytes, &frame);
    let sidecar_path = sidecar_path_for(&input);
    let before = load_sidecar(&sidecar_path).unwrap();

    regenerate(regenerate_args(&input, vec![RegenerateModule::AutoTone])).unwrap();

    let after = load_sidecar(&sidecar_path).unwrap();
    let auto = &after.virtual_copies[0].recipe.auto_features;
    assert!(auto.enable_auto_tone);
    assert!(auto.auto_exposure.is_some());
    assert!(auto.auto_whites.is_some());
    assert!(auto.analysis_fingerprint.is_some());
    for key in [
        "exposure",
        "contrast",
        "whites",
        "blacks",
        "highlights",
        "shadows",
    ] {
        assert!(
            after.virtual_copies[0].recipe.adjustments.contains_key(key),
            "Auto-Tone must persist the `{key}` slider"
        );
    }
    // Only the auto-tone module ran: masks keep their exact definition and
    // no matching value appears.
    assert_eq!(
        after.virtual_copies[0].mask_library,
        before.virtual_copies[0].mask_library
    );
    assert!(!auto.match_total_exposure);
}

/// Explicit `--module matching` persists `matched_exposure` and never runs
/// Auto-Tone or touches the masks.
#[test]
fn regenerate_matching_module_persists_matched_exposure_only() {
    let directory = tempfile::tempdir().unwrap();
    let (input, frame) = regenerate_input(directory.path(), "regen-matching.png");
    let bytes = fs::read(&input).unwrap();
    write_sidecar_with_valid_layer(&input, &bytes, &frame);
    let before = load_sidecar(&sidecar_path_for(&input)).unwrap();

    regenerate(regenerate_args(&input, vec![RegenerateModule::Matching])).unwrap();

    let after = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let auto = &after.virtual_copies[0].recipe.auto_features;
    assert!(auto.match_total_exposure);
    assert!(auto.matched_exposure.is_some());
    assert!(!auto.enable_auto_tone, "matching must not run Auto-Tone");
    assert_eq!(
        after.virtual_copies[0].mask_library,
        before.virtual_copies[0].mask_library
    );
}

/// Explicit `--module masks` marks the stale mask `Pending` **and** arms
/// the one-shot `update_masks` refresh so the next render recognizes the
/// work as deliberately requested (M2). Everything else stays unchanged.
#[test]
fn regenerate_masks_module_arms_the_explicit_refresh() {
    let directory = tempfile::tempdir().unwrap();
    let (input, frame) = regenerate_input(directory.path(), "regen-masks.png");
    let bytes = fs::read(&input).unwrap();
    let before = write_sidecar_with_valid_layer(&input, &bytes, &frame);
    // No `.lumina.zdata` artifact exists → the persisted mask is missing.
    assert_eq!(
        before.virtual_copies[0].mask_library[0].status,
        MaskStatus::Valid
    );

    regenerate(regenerate_args(&input, vec![RegenerateModule::Masks])).unwrap();

    let after = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(
        after.virtual_copies[0].mask_library[0].status,
        MaskStatus::Pending
    );
    // The recipe differs from `before` *only* by the armed one-shot flag.
    let mut expected = before.virtual_copies[0].recipe.clone();
    expected
        .options
        .insert("update_masks".into(), "true".into());
    assert_eq!(after.virtual_copies[0].recipe, expected);
}

/// M2: an explicit `regenerate --module masks` refresh is not reported as
/// an implicit re-inference by the following render.
#[test]
fn explicit_mask_refresh_render_has_no_implicit_warning() {
    let directory = tempfile::tempdir().unwrap();
    let (input, frame) = regenerate_input(directory.path(), "regen-explicit.png");
    let bytes = fs::read(&input).unwrap();
    write_sidecar_with_valid_layer(&input, &bytes, &frame);
    // No zdata → the persisted mask is missing until a refresh runs.
    regenerate(regenerate_args(&input, vec![RegenerateModule::Masks])).unwrap();

    let output = directory.path().join("out.png");
    let mut warnings = Vec::new();
    process_selected(
        ProcessArgs {
            input: input.clone(),
            output,
            preset: None,
            exposure: None,
            contrast: None,
            whites: None,
            blacks: None,
            highlights: None,
            shadows: None,
            auto_tone: false,
            match_total_exposure: false,
            target_luminance: 0.5,
            write_metadata: false,
        },
        90,
        None,
        MaskPolicy::Warn,
        &mut warnings,
    )
    .unwrap();
    assert!(
        warnings.is_empty(),
        "an explicit refresh must not warn as implicit: {warnings:?}"
    );
}

/// M2: the same holds for the `mask --update-masks` path (it must arm the
/// one-shot flag alongside the `Pending` statuses).
#[test]
fn mask_update_masks_render_has_no_implicit_warning() {
    let directory = tempfile::tempdir().unwrap();
    let (input, frame) = regenerate_input(directory.path(), "mask-explicit.png");
    let bytes = fs::read(&input).unwrap();
    write_sidecar_with_valid_layer(&input, &bytes, &frame);
    let mut args = mask_args(input.clone());
    args.update_masks = true;
    mask(args).unwrap();

    let output = directory.path().join("out.png");
    let mut warnings = Vec::new();
    process_selected(
        ProcessArgs {
            input: input.clone(),
            output,
            preset: None,
            exposure: None,
            contrast: None,
            whites: None,
            blacks: None,
            highlights: None,
            shadows: None,
            auto_tone: false,
            match_total_exposure: false,
            target_luminance: 0.5,
            write_metadata: false,
        },
        90,
        None,
        MaskPolicy::Warn,
        &mut warnings,
    )
    .unwrap();
    assert!(
        warnings.is_empty(),
        "an explicit `mask --update-masks` must not warn as implicit: {warnings:?}"
    );
}

/// The collective default (no `--module`) regenerates an enabled
/// stale Auto-Tone value.
#[test]
fn regenerate_collective_generates_enabled_stale_auto_tone() {
    let directory = tempfile::tempdir().unwrap();
    let (input, frame) = regenerate_input(directory.path(), "regen-collective.png");
    let bytes = fs::read(&input).unwrap();
    write_sidecar_with_valid_layer(&input, &bytes, &frame);
    let sidecar_path = sidecar_path_for(&input);
    let mut document = load_sidecar(&sidecar_path).unwrap();
    // Enabled but without values/fingerprint → stale/missing.
    document.virtual_copies[0]
        .recipe
        .auto_features
        .enable_auto_tone = true;
    save_sidecar(&sidecar_path, &document).unwrap();

    regenerate(regenerate_args(&input, Vec::new())).unwrap();

    let after = load_sidecar(&sidecar_path).unwrap();
    let auto = &after.virtual_copies[0].recipe.auto_features;
    assert!(auto.enable_auto_tone);
    assert!(auto.auto_exposure.is_some());
    assert!(auto.analysis_fingerprint.is_some());
}

/// N2: even the forced `--module masks` excludes deterministic range masks
/// (they carry no artifact and are always reproducible from pixels).
#[test]
fn regenerate_masks_module_excludes_range_masks_when_forced() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _frame) = regenerate_input(directory.path(), "regen-range.png");
    import_file(ImportArgs {
        input: input.clone(),
        json: true,
        migrate: false,
    })
    .unwrap();
    let mut add = mask_args(input.clone());
    add.add_luminance_range = true;
    add.name = Some("Bright".into());
    add.range_min = Some(0.0);
    add.range_max = Some(1.0);
    mask(add).unwrap();

    regenerate(regenerate_args(&input, vec![RegenerateModule::Masks])).unwrap();

    let after = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(
        after.virtual_copies[0].mask_library[0].status,
        MaskStatus::Valid,
        "a range mask must never be marked pending"
    );
    assert!(
        !after.virtual_copies[0]
            .recipe
            .options
            .contains_key("update_masks"),
        "no refresh may be armed when only range masks exist"
    );
}

/// M1: a two-slider Auto-Tone artifact (the historic
/// `process --auto-tone` subset with a matching fingerprint) is treated as
/// stale by the collective default and regenerated to the full contract.
#[test]
fn regenerate_collective_completes_two_slider_auto_tone_artifact() {
    let directory = tempfile::tempdir().unwrap();
    let (input, frame) = regenerate_input(directory.path(), "regen-two-slider.png");
    let bytes = fs::read(&input).unwrap();
    write_sidecar_with_valid_layer(&input, &bytes, &frame);
    let sidecar_path = sidecar_path_for(&input);
    let mut document = load_sidecar(&sidecar_path).unwrap();
    let input_fingerprint = auto_tone_input_fingerprint(&frame, 0.5);
    {
        let auto = &mut document.virtual_copies[0].recipe.auto_features;
        auto.enable_auto_tone = true;
        auto.auto_exposure = Some(0.1);
        auto.auto_contrast = Some(0.1);
        auto.analysis_fingerprint = Some(AnalysisFingerprint {
            algorithm: "tone-rgba8-rec709".into(),
            version: "1".into(),
            input_fingerprint,
            extras: BTreeMap::new(),
        });
    }
    document.virtual_copies[0]
        .recipe
        .adjustments
        .insert("exposure".into(), 0.1);
    document.virtual_copies[0]
        .recipe
        .adjustments
        .insert("contrast".into(), 0.1);
    save_sidecar(&sidecar_path, &document).unwrap();

    regenerate(regenerate_args(&input, Vec::new())).unwrap();

    let after = load_sidecar(&sidecar_path).unwrap();
    let auto = &after.virtual_copies[0].recipe.auto_features;
    assert!(
        auto.auto_whites.is_some() && auto.auto_blacks.is_some(),
        "the two-slider artifact must be completed to the full contract"
    );
    for key in ["whites", "blacks", "highlights", "shadows"] {
        assert!(
            after.virtual_copies[0].recipe.adjustments.contains_key(key),
            "missing regenerated slider `{key}`"
        );
    }
}

/// The collective default is a pure read (byte-identical sidecar) when no
/// module is stale or missing — no implicit recomputation.
#[test]
fn regenerate_collective_is_a_noop_when_nothing_is_stale() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _frame) = regenerate_input(directory.path(), "regen-fresh.png");
    import_file(ImportArgs {
        input: input.clone(),
        json: true,
        migrate: false,
    })
    .unwrap();
    // A deterministic range mask never needs an artifact and is never
    // stale; Auto-Tone/Matching stay disabled.
    let mut add = mask_args(input.clone());
    add.add_luminance_range = true;
    add.name = Some("Bright".into());
    add.range_min = Some(0.0);
    add.range_max = Some(1.0);
    mask(add).unwrap();
    let sidecar_path = sidecar_path_for(&input);
    let before = fs::read(&sidecar_path).unwrap();

    regenerate(regenerate_args(&input, Vec::new())).unwrap();

    assert_eq!(
        fs::read(&sidecar_path).unwrap(),
        before,
        "the collective default must not rewrite a fully fresh sidecar"
    );
}

/// M2b + N1: the collective default re-infers **only** the stale/missing
/// source masks. A fresh `Valid` mask (matching source hash, available
/// artifact) stays untouched on the persisted-valid fastpath, and the
/// copy-wide `update_masks` switch is never armed — otherwise the next
/// render would re-infer the fresh mask too. The second mask is made stale
/// purely by a changed source content hash (N1 branch), so the test pins
/// that branch as well.
#[test]
fn regenerate_collective_marks_only_stale_masks() {
    let directory = tempfile::tempdir().unwrap();
    let (input, frame) = regenerate_input(directory.path(), "regen-partial.png");
    let bytes = fs::read(&input).unwrap();
    let mut document = write_sidecar_with_valid_layer(&input, &bytes, &frame);
    // A second source mask, identical but with an outdated source hash →
    // stale only through the N1 hash branch.
    let mut stale = document.virtual_copies[0].mask_library[0].clone();
    stale.id = "stale-subject".into();
    stale.name = "stale-subject".into();
    stale.source_fingerprint.content_hash = "blake3:old-source".into();
    document.virtual_copies[0].mask_library.push(stale.clone());
    let copy_id = document.virtual_copies[0].id.clone();
    document.virtual_copies[0].mask_layers.push(MaskLayer {
        id: "layer-stale".into(),
        mask: MaskReference {
            copy_id,
            mask_id: stale.id.clone(),
            extras: BTreeMap::new(),
        },
        inverted: false,
        feather: 0.0,
        blur: 0.0,
        density: 1.0,
        extras: BTreeMap::new(),
        visible: true,
        local_adjustments: None,
    });
    let sidecar_path = sidecar_path_for(&input);
    save_sidecar(&sidecar_path, &document).unwrap();
    // Materialize the artifact both definitions reference so the fresh mask
    // counts as available (the stale one is stale regardless).
    let tile = lumina_sidecar::MaskTile {
        mask_id: zdata_mask_tile_id("vc-original", "subject"),
        tile_x: 0,
        tile_y: 0,
        width: frame.width,
        height: frame.height,
        values: vec![0; (frame.width * frame.height) as usize],
    };
    let container = lumina_sidecar::ZDataContainer::new(vec![tile]).unwrap();
    lumina_sidecar::save_zdata(&directory.path().join("x.zdata"), &container).unwrap();

    regenerate(regenerate_args(&input, Vec::new())).unwrap();

    let after = load_sidecar(&sidecar_path).unwrap();
    let status = |id: &str| {
        after.virtual_copies[0]
            .mask_library
            .iter()
            .find(|mask| mask.id == id)
            .unwrap()
            .status
            .clone()
    };
    assert_eq!(
        status("subject"),
        MaskStatus::Valid,
        "a fresh Valid mask must stay untouched by the collective default"
    );
    assert_eq!(
        status("stale-subject"),
        MaskStatus::Pending,
        "a mask with a changed source hash must be marked for refresh (N1)"
    );
    assert!(
        !after.virtual_copies[0]
            .recipe
            .options
            .contains_key("update_masks"),
        "the collective default must not arm the copy-wide refresh switch (M2b)"
    );
}
