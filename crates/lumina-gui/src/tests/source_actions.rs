//! GUI-SRCACC-1: strict persisted repair-region loading, active/export parity,
//! cache identity, and stand-in coverage.

use super::*;
use lumina_sidecar::{
    append_repair_region, load_sidecar, save_sidecar, save_zdata, sidecar_path_for,
    RepairRegionArtifact, SourceActionArtifactRef, SourceActionKind, SourceActionSpec,
    ZDataContainer, SOURCE_ACTION_VERSION,
};
use std::path::PathBuf;

#[cfg(test)]
#[path = "source_action_stale_state.rs"]
mod stale_state;
#[cfg(test)]
#[path = "thumbnail_disk_identity.rs"]
mod thumbnail_disk_identity;

#[derive(Debug, Clone, Copy)]
pub(super) enum ActionFixtureMode {
    Valid,
    MissingBundle,
    MissingArtifact,
    ChecksumMismatch,
}

pub(super) struct ActionFixture {
    pub(super) _dir: tempfile::TempDir,
    pub(super) source: PathBuf,
    pub(super) frame: ImageFrame,
    pub(super) artifact: RepairRegionArtifact,
}

fn reference(fixture: &ActionFixture, checksum: Option<&str>) -> SourceActionArtifactRef {
    SourceActionArtifactRef {
        id: "repair-1".into(),
        relative_path: lumina_sidecar::zdata_path_for(&fixture.source)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        checksum: checksum
            .map(str::to_owned)
            .unwrap_or_else(|| fixture.artifact.checksum()),
    }
}

/// Create a real source/sidecar/zdata trio. The mode changes only the persisted
/// recipe/bundle relationship, so callers exercise the production open/render
/// path instead of injecting runtime artifacts into the app.
pub(super) fn action_fixture(mode: ActionFixtureMode) -> ActionFixture {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source-action.png");
    let frame = ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 255]).unwrap();
    std::fs::write(&source, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();

    // Let the production app create a source-valid sidecar, then add the
    // binary artifact and recipe reference independently.
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    assert!(app.error().is_none(), "fixture source must decode");
    app.save_sidecar();
    assert!(app.error().is_none(), "fixture sidecar must save");

    let stored_id = if matches!(mode, ActionFixtureMode::MissingArtifact) {
        "other-repair"
    } else {
        "repair-1"
    };
    let artifact = RepairRegionArtifact {
        id: stored_id.into(),
        width: 2,
        height: 1,
        region: vec![u16::MAX, 0],
        replacement: vec![201, 0, 0, 255, 9, 9, 9, 9],
    };
    if !matches!(mode, ActionFixtureMode::MissingBundle) {
        append_repair_region(&lumina_sidecar::zdata_path_for(&source), artifact.clone()).unwrap();
    }

    let mut fixture = ActionFixture {
        _dir: dir,
        source,
        frame,
        artifact,
    };
    let sidecar_path = sidecar_path_for(&fixture.source);
    let mut document = load_sidecar(&sidecar_path).unwrap();
    let checksum = if matches!(mode, ActionFixtureMode::ChecksumMismatch) {
        Some("deadbeef")
    } else {
        None
    };
    document.virtual_copies[0].recipe.source_actions = vec![SourceActionSpec {
        version: SOURCE_ACTION_VERSION,
        kind: SourceActionKind::DustRemoval,
        artifact: reference(&fixture, checksum),
    }];
    save_sidecar(&sidecar_path, &document).unwrap();
    fixture
}

/// Replace the sole binary artifact with different pixels/checksum while
/// retaining a valid zdata container. Callers update the in-memory reference
/// checksum to model an intentional, valid artifact revision.
pub(super) fn replace_fixture_artifact(
    fixture: &ActionFixture,
    replacement: [u8; 8],
) -> RepairRegionArtifact {
    let changed = RepairRegionArtifact {
        id: "repair-1".into(),
        width: 2,
        height: 1,
        region: vec![u16::MAX, 0],
        replacement: replacement.to_vec(),
    };
    let container = ZDataContainer::new(Vec::new())
        .unwrap()
        .add_repair_region(changed.clone())
        .unwrap();
    save_zdata(&lumina_sidecar::zdata_path_for(&fixture.source), &container).unwrap();
    changed
}

fn first_pixel(frame: &ImageFrame) -> [u8; 4] {
    frame.pixels[..4].try_into().unwrap()
}

#[test]
fn strict_resolver_returns_runtime_artifact_and_stable_identity() {
    let fixture = action_fixture(ActionFixtureMode::Valid);
    let sidecar = load_sidecar(&sidecar_path_for(&fixture.source)).unwrap();
    let recipe = &sidecar.virtual_copies[0].recipe;
    let resolved = crate::source_actions::resolve_source_actions(
        recipe,
        &lumina_sidecar::zdata_path_for(&fixture.source),
        &fixture.frame,
    )
    .unwrap();

    assert_eq!(resolved.artifacts().len(), 1);
    assert_eq!(
        resolved.artifact_checksums(),
        vec![fixture.artifact.checksum()]
    );
    assert_eq!(resolved.identities()[0].id, "repair-1");
    assert_eq!(
        resolved.identities()[0].checksum,
        fixture.artifact.checksum()
    );
}

#[test]
fn missing_bundle_artifact_and_checksum_mismatch_fail_loudly() {
    for mode in [
        ActionFixtureMode::MissingBundle,
        ActionFixtureMode::MissingArtifact,
        ActionFixtureMode::ChecksumMismatch,
    ] {
        let fixture = action_fixture(mode);
        let sidecar = load_sidecar(&sidecar_path_for(&fixture.source)).unwrap();
        let error = crate::source_actions::resolve_source_actions(
            &sidecar.virtual_copies[0].recipe,
            &lumina_sidecar::zdata_path_for(&fixture.source),
            &fixture.frame,
        )
        .unwrap_err()
        .to_string();
        let expected = match mode {
            ActionFixtureMode::MissingBundle => "could not read source-action bundle",
            ActionFixtureMode::MissingArtifact => "artifact missing or corrupt",
            ActionFixtureMode::ChecksumMismatch => "checksum mismatch",
            ActionFixtureMode::Valid => panic!("valid mode is not a failure case"),
        };
        assert!(error.contains(expected), "{mode:?}: {error}");
    }
}

#[test]
fn corrupt_bundle_is_rejected_before_runtime_artifact_use() {
    let fixture = action_fixture(ActionFixtureMode::Valid);
    let zdata = lumina_sidecar::zdata_path_for(&fixture.source);
    let mut bytes = std::fs::read(&zdata).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    std::fs::write(&zdata, bytes).unwrap();
    let sidecar = load_sidecar(&sidecar_path_for(&fixture.source)).unwrap();
    let error = crate::source_actions::resolve_source_actions(
        &sidecar.virtual_copies[0].recipe,
        &zdata,
        &fixture.frame,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("bundle") || error.contains("checksum"),
        "{error}"
    );
}

#[test]
fn gui_open_failure_clears_preview_instead_of_showing_recipe_only_pixels() {
    for mode in [
        ActionFixtureMode::MissingBundle,
        ActionFixtureMode::MissingArtifact,
        ActionFixtureMode::ChecksumMismatch,
    ] {
        let fixture = action_fixture(mode);
        let mut app = new_app();
        open_and_decode(&mut app, fixture.source.display().to_string());
        let error = app.error().expect("source action failure must be visible");
        assert!(
            error.contains("source-action") || error.contains("source action"),
            "{mode:?}: {error}"
        );
        assert!(app.preview().is_none(), "{mode:?}: stale/default preview");
        assert!(app.render_key().is_none(), "{mode:?}: no valid render key");
    }
}

#[test]
fn wrong_bundle_path_is_rejected_even_when_id_and_checksum_exist() {
    let fixture = action_fixture(ActionFixtureMode::Valid);
    let mut sidecar = load_sidecar(&sidecar_path_for(&fixture.source)).unwrap();
    sidecar.virtual_copies[0].recipe.source_actions[0]
        .artifact
        .relative_path = "different.lumina.zdata".into();
    save_sidecar(&sidecar_path_for(&fixture.source), &sidecar).unwrap();

    let error = crate::source_actions::resolve_source_actions(
        &sidecar.virtual_copies[0].recipe,
        &lumina_sidecar::zdata_path_for(&fixture.source),
        &fixture.frame,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("bundle path mismatch"), "{error}");
}

#[test]
fn invalid_plane_replacement_and_source_dimensions_are_rejected() {
    let fixture = action_fixture(ActionFixtureMode::Valid);
    let link = reference(&fixture, None);

    let bad_plane = RepairRegionArtifact {
        region: vec![u16::MAX],
        ..fixture.artifact.clone()
    };
    let error = crate::source_actions::resolve_repair_region(&link, bad_plane, &fixture.frame)
        .unwrap_err()
        .to_string();
    assert!(error.contains("invalid repair-region data"), "{error}");

    let bad_replacement = RepairRegionArtifact {
        replacement: vec![1, 2, 3],
        ..fixture.artifact.clone()
    };
    let error =
        crate::source_actions::resolve_repair_region(&link, bad_replacement, &fixture.frame)
            .unwrap_err()
            .to_string();
    assert!(error.contains("invalid repair-region data"), "{error}");

    let wrong_source = ImageFrame::new(1, 1, vec![0, 0, 0, 255]).unwrap();
    let error = crate::source_actions::resolve_repair_region(
        &link,
        fixture.artifact.clone(),
        &wrong_source,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("do not match source frame"), "{error}");
}

#[test]
fn corrupt_sidecar_never_falls_back_to_the_reset_default_recipe() {
    let fixture = action_fixture(ActionFixtureMode::Valid);
    std::fs::write(
        lumina_sidecar::sidecar_path_for(&fixture.source),
        b"{broken",
    )
    .unwrap();
    let mut app = new_app();
    open_and_decode(&mut app, fixture.source.display().to_string());

    assert!(app.error().is_some(), "corrupt sidecar must be visible");
    assert!(app.preview().is_none(), "no original/recipe-only preview");
    assert!(
        app.render().is_err(),
        "a later render must preserve the strict sidecar failure"
    );
    assert!(
        app.render_draft([800, 600], None).is_err(),
        "the draft fast path must not bypass strict sidecar resolution"
    );
}

#[test]
fn a_failed_source_action_render_clears_an_existing_texture() {
    let fixture = action_fixture(ActionFixtureMode::Valid);
    let mut app = new_app();
    open_and_decode(&mut app, fixture.source.display().to_string());
    assert!(app.error().is_none());
    app.update_texture(&egui::Context::default());
    assert!(app.texture.is_some(), "fixture must have a painted texture");

    std::fs::remove_file(lumina_sidecar::zdata_path_for(&fixture.source)).unwrap();
    assert!(app.render().is_err());
    assert!(app.preview().is_none());
    assert!(
        app.texture.is_none(),
        "stale pixels must not remain paintable"
    );
    assert!(app.navigator_texture.is_none());
}

#[test]
fn active_preview_applies_action_and_matches_core_cli_context() {
    let fixture = action_fixture(ActionFixtureMode::Valid);
    let mut app = new_app();
    open_and_decode(&mut app, fixture.source.display().to_string());
    assert!(app.error().is_none(), "valid action must not error");
    let preview = app.preview().expect("preview after open");
    assert_eq!(first_pixel(preview), [201, 0, 0, 255]);

    let resolved = app.resolve_current_source_actions(&fixture.frame).unwrap();
    let direct = render_frame(
        &fixture.frame,
        &RenderContext {
            recipe: app.recipe(),
            camera_white_balance: None,
            source_actions: resolved.artifacts(),
            masks: None,
            lensfun: None,
            depth: None,
        },
    )
    .unwrap()
    .frame;
    assert_eq!(preview.pixels, direct.pixels, "GUI preview vs core/CLI");
    assert_eq!(
        app.render_key().unwrap().source_action_artifact_hashes,
        vec![fixture.artifact.checksum()]
    );
    assert_eq!(
        app.last_stage_work()
            .unwrap()
            .source_action_artifacts_applied,
        1
    );
}

#[test]
fn navigator_neighbor_and_thumbnail_stand_ins_apply_action_before_downscale() {
    let fixture = action_fixture(ActionFixtureMode::Valid);
    let mut app = new_app();
    open_and_decode(&mut app, fixture.source.display().to_string());
    assert!(app.error().is_none());

    let navigator = app.navigator_zoomed_overview().expect("navigator overview");
    assert_eq!(first_pixel(&navigator), [201, 0, 0, 255]);

    let neighbor_job = crate::preview_ctrl::PreviewJob {
        probe_id: fixture.source.display().to_string(),
        source: fixture.source.clone(),
        name: "source-action.png".into(),
        virtual_copy: "vc-original".into(),
        target: (2, 1),
        kind: lumina_core::preview_cache::PreviewKind::Screen,
        priority: 0,
        denoise_policy: lumina_core::DenoisePolicy::Warn,
    };
    let neighbor = crate::preview_jobs::worker_preview(neighbor_job).unwrap();
    let crate::preview_ctrl::PreviewOutcome::Ready(neighbor_frame) = neighbor.outcome else {
        panic!("neighbor must render, not fail");
    };
    assert_eq!(first_pixel(&neighbor_frame), [201, 0, 0, 255]);

    let thumbnail_job = crate::thumb_worker::ThumbnailJob {
        source: fixture.source.clone(),
        name: "source-action.png".into(),
        key: fixture.source.display().to_string(),
        source_identity: crate::filmstrip::ThumbnailManager::source_identity(&fixture.source),
        cache: None,
        cached: false,
        enqueued_at: std::time::Instant::now(),
    };
    let thumbnail = crate::thumb_worker::decode_thumbnail_frame(&thumbnail_job).unwrap();
    assert_eq!(first_pixel(&thumbnail), [201, 0, 0, 255]);
}

#[test]
fn navigator_resolution_failure_clears_key_and_recovers_same_identity() {
    let fixture = action_fixture(ActionFixtureMode::Valid);
    let mut app = new_app();
    open_and_decode(&mut app, fixture.source.display().to_string());
    assert!(app.error().is_none());

    let first = app.navigator_zoomed_overview().expect("initial overview");
    assert_eq!(first_pixel(&first), [201, 0, 0, 255]);
    assert!(app.navigator_overview_key.is_some());

    let zdata = lumina_sidecar::zdata_path_for(&fixture.source);
    let original_bundle = std::fs::read(&zdata).unwrap();
    std::fs::remove_file(&zdata).unwrap();
    assert!(app.navigator_zoomed_overview().is_none());
    assert!(app.navigator_overview.is_none());
    assert!(
        app.navigator_overview_key.is_none(),
        "a failed resolution must not poison the same identity"
    );

    std::fs::write(&zdata, original_bundle).unwrap();
    let recovered = app
        .navigator_zoomed_overview()
        .expect("overview must rebuild after the bundle returns");
    assert_eq!(first_pixel(&recovered), [201, 0, 0, 255]);
}
