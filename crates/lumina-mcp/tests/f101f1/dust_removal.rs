//! Shared helpers are inherited from the F-101-F1 integration-test parent.
use super::*;

// ---------------------------------------------------------------------------
// lumina_dust_removal
// ---------------------------------------------------------------------------

#[test]
fn dust_removal_persists_action_and_verifies_via_render_out() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("photo.png");
    let replacement = dir.path().join("replacement.png");
    let region_path = dir.path().join("region.u16le");
    // LE bytes [0x00, 0x80] decode to 0x8000; the opposite endian is 0x0080.
    // Only the first pixel should therefore be above the replacement threshold.
    fs::write(&region_path, [0x00, 0x80, 0, 0, 0, 0, 0, 0]).unwrap();
    let render_out = dir.path().join("verified.png");
    // Source: mid-gray; replacement: SOLID red plate (built directly, since
    // make_png gradients wrap at 255) — the asymmetric LE plane selects only
    // the first pixel for replacement.
    make_png(&source, 2, 2, 128);
    let red = ImageFrame::new(
        2,
        2,
        vec![
            255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
        ],
    )
    .unwrap();
    fs::write(&replacement, red.encode(ImageFileFormat::Png).unwrap()).unwrap();
    let mut server = new_server(&dir.path().join("previews"));

    tool_ok(
        &mut server,
        "lumina_import",
        json!({ "path": source.to_string_lossy() }),
    );

    let result = tool_ok(
        &mut server,
        "lumina_dust_removal",
        json!({
            "input": source.to_string_lossy(),
            "repair_region": {
                "id": "spot-1",
                "kind": "dust-removal",
                "region_width": 2,
                "region_height": 2,
                "region_path": region_path.to_string_lossy(),
                "replacement_path": replacement.to_string_lossy(),
            },
            "render_out": render_out.to_string_lossy(),
        }),
    );
    assert_eq!(result["ok"], true);
    assert_eq!(result["artifact_id"], "spot-1");
    assert_eq!(result["virtual_copy"], "vc-original");
    let bundle = result["bundle"].as_str().unwrap().to_string();
    assert!(bundle.ends_with(".lumina.zdata"));
    assert!(dir.path().join(&bundle).is_file());

    // Recipe carries exactly one source action with the reported checksum.
    let sidecar = lumina_sidecar::load_sidecar(&sidecar_json(&source)).unwrap();
    let actions = &sidecar.virtual_copies[0].recipe.source_actions;
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].artifact.id, "spot-1");
    assert_eq!(actions[0].artifact.checksum, result["checksum"]);
    // Relative reference only (portability rule).
    assert!(!actions[0].artifact.relative_path.contains('/'));

    // Verification render replaced only the pixel selected by the LE plane.
    let verified = ImageFrame::decode(&fs::read(&render_out).unwrap()).unwrap();
    assert_eq!((verified.width(), verified.height()), (2, 2));
    let pixels = verified.pixels().as_chunks::<4>().0;
    assert_eq!(&pixels[0][..3], &[255, 0, 0], "LE 0x8000 must replace");
    for pixel in &pixels[1..] {
        assert_ne!(&pixel[..3], &[255, 0, 0], "other pixels stay untouched");
    }

    // Original untouched.
    let original = ImageFrame::decode(&fs::read(&source).unwrap()).unwrap();
    assert_eq!(
        &original.pixels().as_chunks::<4>().0[0][..3],
        &[128, 0, 128]
    );
}

#[test]
fn dust_removal_error_paths_are_loud_and_leave_no_partial_state() {
    let dir = tempfile::tempdir().unwrap();
    let orphan = dir.path().join("orphan.png");
    make_png(&orphan, 2, 2, 0);
    let source = dir.path().join("photo.png");
    let replacement = dir.path().join("replacement.png");
    let region_path = dir.path().join("region.u16le");
    fs::write(&region_path, [255, 255, 255, 255, 255, 255, 255, 255]).unwrap();
    let region_3x3 = dir.path().join("region-3x3.u16le");
    fs::write(&region_3x3, [255; 18]).unwrap();
    make_png(&source, 2, 2, 0);
    make_png(&replacement, 2, 2, 255);
    let mut server = new_server(&dir.path().join("previews"));

    tool_ok(
        &mut server,
        "lumina_import",
        json!({ "path": source.to_string_lossy() }),
    );

    // Exact byte count is mandatory: reject both truncation and trailing data.
    for bytes in [&[255u8; 7][..], &[255u8; 9][..]] {
        fs::write(&region_path, bytes).unwrap();
        let (name, _) = tool_error(
            &mut server,
            "lumina_dust_removal",
            json!({
                "input": source.to_string_lossy(),
                "repair_region": {
                    "id": "bad-length",
                    "region_width": 2,
                    "region_height": 2,
                    "region_path": region_path.to_string_lossy(),
                    "replacement_path": replacement.to_string_lossy(),
                },
            }),
        );
        assert_eq!(name, "InvalidParams");
    }
    fs::write(&region_path, [255; 8]).unwrap();
    let (name, _) = tool_error(
        &mut server,
        "lumina_dust_removal",
        json!({
            "input": source.to_string_lossy(),
            "repair_region": {
                "id": "oversized",
                "region_width": u32::MAX,
                "region_height": u32::MAX,
                "region_path": region_path.to_string_lossy(),
                "replacement_path": replacement.to_string_lossy(),
            },
        }),
    );
    assert_eq!(name, "InvalidParams");

    // Missing sidecar points at lumina_import instead of failing cryptically.
    let (name, message) = tool_error(
        &mut server,
        "lumina_dust_removal",
        json!({
            "input": orphan.to_string_lossy(),
            "repair_region": {
                "id": "s",
                "region_width": 2,
                "region_height": 2,
                "region_path": region_path.to_string_lossy(),
                "replacement_path": replacement.to_string_lossy(),
            },
        }),
    );
    assert_eq!(name, "SidecarError");
    assert!(message.contains("run lumina_import first"));

    // Region/source dimension mismatch fails BEFORE the bundle is touched:
    // no zdata file may appear (REVIEW-CLI-N2 ordering parity).
    let zdata = lumina_sidecar::zdata_path_for(&source);
    let (dims, _) = tool_error(
        &mut server,
        "lumina_dust_removal",
        json!({
            "input": source.to_string_lossy(),
            "repair_region": {
                "id": "s",
                "region_width": 3,
                "region_height": 3,
                "region_path": region_3x3.to_string_lossy(),
                "replacement_path": replacement.to_string_lossy(),
            },
        }),
    );
    assert_eq!(dims, "InvalidParams");
    assert!(!zdata.exists(), "rejected call must leave no bundle bytes");

    // Unknown virtual copy.
    let (unknown_copy, _) = tool_error(
        &mut server,
        "lumina_dust_removal",
        json!({
            "input": source.to_string_lossy(),
            "virtual_copy": "does-not-exist",
            "repair_region": {
                "id": "s",
                "region_width": 2,
                "region_height": 2,
                "region_path": region_path.to_string_lossy(),
                "replacement_path": replacement.to_string_lossy(),
            },
        }),
    );
    assert_eq!(unknown_copy, "UnknownCopy");

    // render_out refusing to overwrite the source (non-destructive guard).
    let (protected, message) = tool_error(
        &mut server,
        "lumina_dust_removal",
        json!({
            "input": source.to_string_lossy(),
            "repair_region": {
                "id": "s",
                "region_width": 2,
                "region_height": 2,
                "region_path": region_path.to_string_lossy(),
                "replacement_path": replacement.to_string_lossy(),
            },
            "render_out": source.to_string_lossy(),
        }),
    );
    assert_eq!(protected, "EncodeError");
    assert!(message.contains("refusing"), "{message}");
    assert!(!zdata.exists(), "guard fires before any mutation");
}
