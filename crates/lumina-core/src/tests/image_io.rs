use super::*;

// ---- R2-MCP-06: downscale_bilinear moved into core ----

/// Pins the invariants the MCP preview relied on: never upscales, keeps the
/// aspect ratio (width-driven), clamps to at least 1px, is deterministic,
/// and preserves pixels unchanged when no downscale is needed.
#[test]
fn downscale_bilinear_never_upsizes_and_is_deterministic() {
    let frame = ImageFrame::new(64, 48, vec![10u8; 64 * 48 * 4]).unwrap();
    let small = downscale_bilinear(&frame, 1024).unwrap();
    assert_eq!((small.width, small.height), (64, 48), "must never upscale");
    assert_eq!(small.pixels, frame.pixels, "no-op downscale keeps pixels");

    let scaled = downscale_bilinear(&frame, 32).unwrap();
    assert_eq!(scaled.width, 32, "output width must honour max_width");
    assert_eq!(scaled.height, 24, "aspect ratio (64:48) preserved");

    let again = downscale_bilinear(&frame, 32).unwrap();
    assert_eq!(scaled.pixels, again.pixels, "deterministic bytes");

    // Max-width smaller than one pixel still yields at least 1x1.
    let tiny = downscale_bilinear(&frame, 0).unwrap();
    assert_eq!(tiny.width, 1);
    assert_eq!(tiny.height, 1);
}

/// Moved from the mcp helper's guarding semantics: the flat-colour mid-pixel
/// average stays inside the valid RGBA byte range even for maximal
/// downscales (no overflow/underflow in the f64 interpolation path).
#[test]
fn downscale_bilinear_averages_flat_colour_identically() {
    let mut flat = vec![0u8; 128 * 64 * 4];
    for px in flat.as_chunks_mut::<4>().0 {
        px.copy_from_slice(&[50, 100, 150, 200]);
    }
    let frame = ImageFrame::new(128, 64, flat).unwrap();
    let scaled = downscale_bilinear(&frame, 7).unwrap();
    assert_eq!(scaled.width, 7);
    assert!(
        scaled
            .pixels
            .iter()
            .all(|&b| b == 50 || b == 100 || b == 150 || b == 200),
        "flat colour must stay flat: {:?}",
        scaled.pixels
    );
}

#[test]
fn all_supported_formats_roundtrip() {
    let frame = ImageFrame::new(2, 1, vec![0, 10, 20, 255, 250, 240, 230, 128]).unwrap();
    for format in [
        ImageFileFormat::Png,
        ImageFileFormat::Jpeg,
        ImageFileFormat::WebP,
    ] {
        let decoded = ImageFrame::decode(&frame.encode(format).unwrap()).unwrap();
        assert_eq!((decoded.width, decoded.height), (2, 1));
        assert_eq!(decoded.pixels.len(), 8);
    }
}

#[test]
fn decode_rejects_oversized_headers_before_allocating() {
    // 20000x20000 = 400 MP exceeds the default 200 MP raw-pixel budget.
    // The header check must reject the image BEFORE the decoder allocates
    // any pixel buffer (the file carries no pixel data at all).
    let bytes = png_header(20_000, 20_000);
    match ImageFrame::decode(&bytes) {
        Err(CoreError::Decode(message)) => {
            assert!(
                message.contains("memory budget"),
                "unexpected message: {message}"
            );
            assert!(message.contains("20000x20000"));
        }
        other => panic!("expected CoreError::Decode with budget message, got {other:?}"),
    }
}

#[test]
fn decode_accepts_normal_sized_images_after_budget_check() {
    let frame = ImageFrame::new(3, 2, vec![7; 24]).unwrap();
    let encoded = frame.encode(ImageFileFormat::Png).unwrap();
    assert_eq!(ImageFrame::decode(&encoded).unwrap(), frame);
}

#[test]
fn png_options_are_lossless_and_dither_is_deterministic() {
    let frame = ImageFrame::new(2, 1, vec![1, 20, 240, 255, 100, 101, 102, 17]).unwrap();
    let options = ExportOptions {
        format: ImageFileFormat::Png,
        dither: false,
        ..ExportOptions::default()
    };
    let bytes = frame.encode_with_options(options).unwrap();
    assert_eq!(ImageFrame::decode(&bytes).unwrap(), frame);
    let dithered = ExportOptions {
        dither: true,
        seed: 42,
        ..options
    };
    assert_eq!(
        frame.encode_with_options(dithered).unwrap(),
        frame.encode_with_options(dithered).unwrap()
    );
}

#[test]
fn png_export_without_dither_is_deterministic_and_lossless() {
    // Exercises the no-mutation (no-clone) encode path: dither=false must
    // pass the original buffer by reference and still produce a valid,
    // byte-deterministic, losslessly roundtripping PNG.
    let frame = ImageFrame::new(4, 3, (0..48).map(|v| v as u8).collect()).unwrap();
    let options = ExportOptions {
        format: ImageFileFormat::Png,
        dither: false,
        ..ExportOptions::default()
    };
    let first = frame.encode_with_options(options).unwrap();
    let second = frame.encode_with_options(options).unwrap();
    assert_eq!(
        first, second,
        "PNG encode without dither must be deterministic"
    );
    assert_eq!(ImageFrame::decode(&first).unwrap(), frame);
}
