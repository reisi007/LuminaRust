//! MCP-MASKPOLICY: `lumina_preview` renders WITHOUT masks, even when the
//! sidecar carries a confirmably valid mask — driven over the REAL MCP stdio
//! server (the `lumina-mcp` binary, spawned as a subprocess, spoken to with
//! newline-delimited JSON-RPC, exactly like an MCP client).
//!
//! # Why this test exists
//!
//! Every MCP render path constructs `RenderContext` with `masks: None`
//! (`crates/lumina-mcp/src/util.rs:308,382,404,627`,
//! `tools/dust_removal.rs:232`); the F-048/F-051 mask decision layer is
//! CLI-side. The CLI applies the same mask with policy `warn`. That
//! divergence is documented in `feature/platform/mcp-server.md` § „Masken im
//! MCP-Renderpfad" — this file pins it so it cannot drift silently.
//!
//! # What is compared
//!
//! The preview bytes are decoded and compared BYTE-FOR-BYTE against a
//! `masks: None` oracle computed in-process through the same public
//! `render_frame` entry point. Two further assertions keep the test from
//! being vacuous: the no-mask oracle MUST differ from a `warn` masked oracle
//! (proving the fixture's mask is actually effective), and the preview MUST
//! differ from the masked oracle (the divergence itself).
//!
//! The CLI half of the divergence (the CLI DOES apply the same mask) lives in
//! `crates/lumina-cli/tests/mask_policy_divergence.rs`.

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
use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};

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
    // only ever composited when a render resolves masks. The MCP render path
    // never does — that is the divergence under test.
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
/// builds (`masks: None`).
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

/// One JSON-RPC roundtrip over the real stdio transport: write one
/// newline-delimited request, read one line back.
fn roundtrip(
    stdin: &mut impl Write,
    stdout: &mut BufReader<ChildStdout>,
    id: u64,
    request: Value,
) -> Value {
    writeln!(stdin, "{}", serde_json::to_string(&request).unwrap()).unwrap();
    stdin.flush().unwrap();
    let mut line = String::new();
    stdout.read_line(&mut line).expect("server answered");
    let response: Value = serde_json::from_str(line.trim()).expect("response is JSON");
    assert_eq!(response["id"], json!(id), "response id matches the request");
    assert!(
        response.get("error").is_none(),
        "no protocol error for `{}`: {response}",
        request["method"]
    );
    response
}

fn spawn_server(preview_dir: &Path) -> (Child, BufReader<ChildStdout>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_lumina-mcp"))
        .env("LUMINA_MCP_PREVIEW_DIR", preview_dir)
        .env("LUMINA_MCP_KEEP_PREVIEWS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the real lumina-mcp stdio server");
    let stdout = child.stdout.take().expect("server stdout");
    (child, BufReader::with_capacity(4096, stdout))
}

#[test]
fn preview_renders_without_masks_even_when_the_sidecar_carries_a_valid_one() {
    let root = tempfile::tempdir().unwrap();
    let (frame, document, zdata_path) = fixture(root.path());
    let no_mask = no_mask_oracle(&frame, &document);
    let masked = masked_oracle(&frame, &document, &zdata_path);
    // The fixture's mask must actually do something, otherwise a
    // "preview == no-mask" comparison would pass vacuously.
    assert_ne!(
        no_mask.pixels, masked.pixels,
        "the half-covered mask must change the masked half of the render"
    );

    let preview_dir = root.path().join("previews");
    let (mut child, mut stdout) = spawn_server(&preview_dir);
    let mut stdin = child.stdin.take().expect("server stdin");

    roundtrip(
        &mut stdin,
        &mut stdout,
        1,
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
    );
    let loaded = roundtrip(
        &mut stdin,
        &mut stdout,
        2,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "lumina_load",
                "arguments": { "path": root.path().join("input.png").to_str().unwrap() }
            }
        }),
    );
    let image_id = loaded["result"]["structuredContent"]["image_id"]
        .as_str()
        .expect("lumina_load returns an image_id")
        .to_string();

    // max_width == frame width: downscale_bilinear returns the render
    // unchanged, so the preview pixels ARE the render pixels.
    let preview = roundtrip(
        &mut stdin,
        &mut stdout,
        3,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "lumina_preview",
                "arguments": { "image_id": image_id, "max_width": SIZE }
            }
        }),
    );
    let preview_path = PathBuf::from(
        preview["result"]["structuredContent"]["preview_path"]
            .as_str()
            .expect("lumina_preview returns a preview_path"),
    );
    assert_eq!(preview["result"]["structuredContent"]["width"], json!(SIZE));
    assert_eq!(
        preview["result"]["structuredContent"]["height"],
        json!(SIZE)
    );
    drop(stdin);
    child.wait().expect("server exits cleanly on EOF");

    let preview_frame = ImageFrame::decode(&fs::read(&preview_path).unwrap()).unwrap();
    assert_eq!(
        preview_frame.pixels, no_mask.pixels,
        "MCP-MASKPOLICY: lumina_preview must render WITHOUT masks — the \
         documented divergence from the CLI is observed over the real stdio \
         server (masks: None), not just asserted"
    );
    assert_ne!(
        preview_frame.pixels, masked.pixels,
        "the preview must not equal the masked CLI render of the same \
         fixture — that equality would mean the mask was applied on the MCP \
         path (the MCP-MASK-APPLY feature) and this pinning test is stale"
    );
}
