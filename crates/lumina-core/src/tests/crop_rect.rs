use super::*;

// ---- REVIEW-CORE-CROP-1: crop rect hardening ----

#[test]
fn crop_rect_identity_full_frame_and_half_crop_are_stable() {
    assert_eq!(crop_rect(64, 48, None).unwrap(), (0, 0, 64, 48));
    let full = lumina_sidecar::Crop::Free {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };
    assert_eq!(crop_rect(64, 48, Some(&full)).unwrap(), (0, 0, 64, 48));
    let half = lumina_sidecar::Crop::Free {
        x: 0.25,
        y: 0.5,
        width: 0.5,
        height: 0.5,
    };
    assert_eq!(crop_rect(64, 48, Some(&half)).unwrap(), (16, 24, 32, 24));
}

#[test]
fn crop_rect_edge_origins_clamp_into_the_frame_without_underflow() {
    // x/y == 1.0 previously rounded px/py onto (or past) the frame edge:
    // `width - px` underflowed or produced a zero-size crop that flowed
    // through the pipeline as an empty frame. The hardened arithmetic
    // clamps the origin to the last pixel and keeps a non-empty extent.
    for (x, y) in [(1.0f32, 0.5f32), (0.5f32, 1.0f32), (1.0f32, 1.0f32)] {
        let crop = lumina_sidecar::Crop::Free {
            x,
            y,
            // Tiny extents so the rectangle sums stay inside the
            // documented 1e-6 tolerance — this is exactly the input
            // class that previously underflowed the extent arithmetic.
            width: 1e-7,
            height: 1e-7,
        };
        let (px, py, pw, ph) = crop_rect(64, 48, Some(&crop))
            .unwrap_or_else(|error| panic!("crop with origin ({x},{y}) failed: {error:?}"));
        assert!(pw >= 1 && ph >= 1, "empty rect for origin ({x},{y})");
        assert!(px + pw <= 64 && py + ph <= 48);
        if x == 1.0 {
            assert_eq!(px + pw, 64, "x-edge crop must end at the last column");
        }
        if y == 1.0 {
            assert_eq!(py + ph, 48, "y-edge crop must end at the last row");
        }
    }
}

#[test]
fn crop_rect_rejects_origins_above_one_inside_tolerance_window() {
    // The rectangle sum stays inside the old 1e-6 tolerance, but the
    // origin above 1 would push the rounded pixel origin past a large
    // frame edge and underflow the extent arithmetic.
    let crop = lumina_sidecar::Crop::Free {
        x: 1.000_000_5,
        y: 0.0,
        width: 1e-7,
        height: 0.5,
    };
    assert!(crop_rect(10_000_000, 10, Some(&crop)).is_err());
}

#[test]
fn crop_rect_valid_sweep_always_yields_in_bounds_non_empty_rects() {
    // Deterministic xorshift sweep over accepted rectangles (including
    // exact-fit and within-tolerance edges): every resulting pixel rect
    // must be non-empty and inside the frame.
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..2000 {
        let x = (next() % 1001) as f32 / 1000.0;
        let y = (next() % 1001) as f32 / 1000.0;
        let room_x = 1000 - (x * 1000.0) as u64;
        let room_y = 1000 - (y * 1000.0) as u64;
        let w = (next() % (room_x + 2)) as f32 / 1000.0; // may exceed the sum by ≤ 1e-6-ish
        let h = (next() % (room_y + 2)) as f32 / 1000.0;
        if !(w > 0.0 && h > 0.0 && x + w <= 1.0 + 1e-6 && y + h <= 1.0 + 1e-6) {
            continue;
        }
        let crop = lumina_sidecar::Crop::Free {
            x,
            y,
            width: w,
            height: h,
        };
        let (px, py, pw, ph) = crop_rect(97, 61, Some(&crop)).unwrap();
        assert!(pw >= 1 && ph >= 1, "empty rect for {x},{y},{w},{h}");
        assert!(px + pw <= 97 && py + ph <= 61);
    }
}
