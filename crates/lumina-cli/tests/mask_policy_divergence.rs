//! MCP-MASKPOLICY (CLI half): the CLI applies the very mask that the MCP
//! render path skips. `lumina process` (the CLI's shared render command,
//! `process_selected` with the harmonized default policy `warn`) runs over the
//! same fixture that `crates/lumina-mcp/tests/mask_policy.rs` uses for the
//! MCP side, and the output is compared BYTE-FOR-BYTE against a `warn`
//! masked oracle built through the public `render_frame` entry point.
//!
//! # Why this test exists
//!
//! MCP-MASKPOLICY documents that every MCP `RenderContext` carries
//! `masks: None` while the CLI resolves persisted mask planes through the
//! F-048/F-051 decision layer (default `warn`). The MCP half of the
//! divergence is pinned in `crates/lumina-mcp/tests/mask_policy.rs`; this
//! file pins the CLI half so the divergence is demonstrated on BOTH sides —
//! a one-sided test could pass while the CLI silently stopped applying
//! masks (or started applying them on the MCP path) without anyone noticing.

use lumina_core::{
    render_frame, ImageFileFormat, ImageFrame, MaskContext, MaskPolicy, RenderContext,
};
use lumina_sidecar::{
    save_sidecar, save_zdata, sidecar_path_for, zdata_path_for, Extras, GeometryFingerprint,
    MaskDefinition, MaskLayer, MaskOperation, MaskReference, MaskStatus, MaskTile, ModelIdentity,
    Preprocessing, Resolution, SidecarDocument, ZDataContainer,
};
use lumina_stages::{
    decode::source_identity,
    pipeline::{load_persisted_mask_planes, zdata_mask_tile_id},
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Frame size of the synthetic fixture. Left half of the mask plane is
/// fully inside the mask (`u16::MAX`), right half fully outside (`0`), so a
/// masked render changes exactly the left half — the divergence is local and
/// measurable, not a global "something differs" assertion.
const SIZE: u32 = 8;

/// The fixture's mask plane, evaluated by the CLI's decision layer as
/// confirmably valid: source/decode/model identity match the running source
/// and the wired BiRefNet descriptor, status `Valid`, artifact dimensions
/// equal to the frame (the dimension check of the F-048 validity gate).
fn valid_mask_definition(id: &str, identity: &lumina_sidecar::SourceIdentity) -> MaskDefinition {
    MaskDefinition {
        id: id.into(),
        name: id.into(),
        source_fingerprint: lumina_sidecar::SourceFingerprint {
            content_hash: identity.content_hash.clone(),
            byte_length: identity.byte_length,
            extras: Extras::new(),
        },
        decode_context: identity.decode_fingerprint.clone(),
        geometry_context: GeometryFingerprint {
            width: SIZE,
            height: SIZE,
            orientation: 1,
            pixel_aspect_ratio: 1.0,
            extras: Extras::new(),
        },
        model: ModelIdentity {
            name: "BiRefNet".into(),
            version: "1.0.0".into(),
            hash: "pending-integration".into(),
            extras: Extras::new(),
        },
        inference_resolution: Resolution {
            width: SIZE,
            height: SIZE,
            extras: Extras::new(),
        },
        preprocessing: Preprocessing {
            name: "p".into(),
            version: "1".into(),
            parameters: Default::default(),
            extras: Extras::new(),
        },
        rescaling_method: "none".into(),
        rescaling_parameters: Default::default(),
        coordinate_system: lumina_sidecar::CoordinateSystem::SourceOriented,
        status: MaskStatus::Valid,
        created_at: "now".into(),
        generator_version: "g".into(),
        error_text: None,
        artifact: Some(lumina_sidecar::ArtifactReference {
            relative_path: "x.zdata".into(),
            format: "lumina-zdata".into(),
            checksum: "c".into(),
            width: SIZE,
            height: SIZE,
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

/// Writes `<dir>/input.png` plus a sidecar carrying one valid source mask
/// with a NON-IDENTITY local adjustment (local exposure `+1.0`), plus the
/// persisted zdata plane for that mask. Returns the decoded frame, the sidecar
/// document and the zdata path so the caller can build both oracles.
fn fixture(dir: &Path) -> (ImageFrame, SidecarDocument, PathBuf) {
    fs::create_dir_all(dir).unwrap();
    let input = dir.join("input.png");
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            // A gradient that stays clear of the 255 rail in every channel,
            // so a +1 EV local exposure visibly changes the masked pixels.
            pixels.extend_from_slice(&[(x * 32) as u8, (y * 32) as u8, 128, 255]);
        }
    }
    let frame = ImageFrame::new(SIZE, SIZE, pixels).unwrap();
    fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();

    let bytes = fs::read(&input).unwrap();
    let identity = source_identity(&input, &bytes, &frame, None).unwrap();
    let mut document = SidecarDocument::new(identity.clone(), "raster-mvp-1");
    let copy_id = document.virtual_copies[0].id.clone();
    let copy = &mut document.virtual_copies[0];
    copy.mask_library = vec![valid_mask_definition("subject", &identity)];
    // The local adjustment lives on the MASK LAYER, not in the recipe: it is
    // only ever composited when a render resolves masks — which the MCP path
    // never does and the CLI path always does.
    copy.mask_layers = vec![MaskLayer {
        id: "layer-1".into(),
        mask: MaskReference {
            copy_id: copy_id.clone(),
            mask_id: "subject".into(),
            extras: Default::default(),
        },
        inverted: false,
        feather: 0.0,
        blur: 0.0,
        density: 1.0,
        visible: true,
        local_adjustments: Some(lumina_sidecar::LocalAdjustments {
            exposure: 1.0,
            ..Default::default()
        }),
        extras: Default::default(),
    }];
    save_sidecar(&sidecar_path_for(&input), &document).unwrap();

    let mut values = Vec::with_capacity((SIZE * SIZE) as usize);
    for _y in 0..SIZE {
        for x in 0..SIZE {
            values.push(if x < SIZE / 2 { u16::MAX } else { 0 });
        }
    }
    let tile = MaskTile {
        mask_id: zdata_mask_tile_id(&copy_id, "subject"),
        tile_x: 0,
        tile_y: 0,
        width: SIZE,
        height: SIZE,
        values,
    };
    let container = ZDataContainer::new(vec![tile]).unwrap();
    let zdata_path = zdata_path_for(&input);
    save_zdata(&zdata_path, &container).unwrap();
    (frame, document, zdata_path)
}

/// The no-mask oracle: the exact `RenderContext` shape every MCP render path
/// builds (`masks: None`) — the render the CLI would produce if it ignored
/// masks.
fn no_mask_oracle(frame: &ImageFrame, document: &SidecarDocument) -> ImageFrame {
    render_frame(
        frame,
        &RenderContext {
            recipe: &document.virtual_copies[0].recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        },
    )
    .expect("no-mask oracle renders")
    .frame
}

/// The masked oracle: the exact `RenderContext` shape the CLI's
/// `process_selected` builds for this fixture (persisted planes loaded from
/// the bundle, `warn` policy, no source actions).
fn masked_oracle(frame: &ImageFrame, document: &SidecarDocument, zdata_path: &Path) -> ImageFrame {
    let mut warnings = Vec::new();
    let planes = load_persisted_mask_planes(document, zdata_path, &mut warnings);
    assert!(
        warnings.is_empty(),
        "the fixture plane must load warning-free: {warnings:?}"
    );
    assert_eq!(
        planes.len(),
        1,
        "the fixture must yield exactly one persisted plane"
    );
    assert!(
        planes.contains_key(&(document.virtual_copies[0].id.clone(), "subject".into())),
        "the persisted plane is keyed by the composite copy/mask id"
    );
    let copies = document.virtual_copies.clone();
    let active_copy_id = copies[0].id.clone();
    render_frame(
        frame,
        &RenderContext {
            recipe: &document.virtual_copies[0].recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: Some(MaskContext {
                copies: &copies,
                active_copy_id: &active_copy_id,
                planes,
                policy: MaskPolicy::Warn,
                source_roi: None,
            }),
            lensfun: None,
            depth: None,
        },
    )
    .expect("masked oracle renders")
    .frame
}

#[test]
fn process_applies_the_persisted_mask_that_the_mcp_preview_skips() {
    let root = tempfile::tempdir().unwrap();
    let (frame, document, zdata_path) = fixture(root.path());
    let no_mask = no_mask_oracle(&frame, &document);
    let masked = masked_oracle(&frame, &document, &zdata_path);
    // The fixture's mask must actually do something, otherwise a
    // "cli == masked oracle" comparison would pass vacuously.
    assert_ne!(
        no_mask.pixels, masked.pixels,
        "the half-covered mask must change the masked half of the render"
    );

    let input = root.path().join("input.png");
    let output = root.path().join("output.png");
    // `lumina process` renders through `process_selected` with the
    // harmonized CLI default policy `warn` HARDCODED (no `--mask-policy`
    // flag exists on this subcommand); the flag lives on render/export/
    // batch/develop. This test therefore pins the default by invocation,
    // not by flag.
    let status = Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
        .args([
            "process",
            "--input",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .status()
        .expect("run the real lumina-cli process");
    assert!(status.success(), "lumina process must succeed: {status}");

    let rendered = ImageFrame::decode(&fs::read(&output).unwrap()).unwrap();
    assert_eq!(
        rendered.pixels, masked.pixels,
        "MCP-MASKPOLICY (CLI half): the CLI must APPLY the persisted mask — \
         the output equals the warn-masked oracle built through the public \
         render_frame entry point"
    );
    assert_ne!(
        rendered.pixels, no_mask.pixels,
        "the CLI render must differ from the no-mask (MCP) render of the \
         same fixture — that difference is the documented divergence"
    );
}
