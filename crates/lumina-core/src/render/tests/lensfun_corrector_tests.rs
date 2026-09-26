//! The F-098-N1 Lensfun `Corrector` render tests of `render_frame`.
//!
//! Pure move out of `render.rs` (file-size ratchet), the same
//! Gegenextraktion the sibling `render/tests/*.rs` blocks describe: the two
//! tests below are character-for-character the ones that were in `render.rs`,
//! only in their own file now. Nothing is merged, weakened or dropped — the
//! LENSFUN-DB-33 "loud resolve" expectations and both doc comments move
//! verbatim. The shared `lensfun_gradient_frame` fixture stays in `render.rs`
//! and is reached through `super::*`, exactly like `tca_isolation_tests.rs`.
//! The block is only dedented by the module level; the single further textual
//! change is rustfmt joining the `let d = (a - b).unsigned_abs();` subtraction
//! onto one line, which it now has room for — the character stream without
//! whitespace is unchanged.

// The whole block below is `lensfun`-gated, so its scope import is too.
#[cfg(feature = "lensfun")]
use super::*;

/// A real Lensfun profile must deviate from the manual (identity) model at
/// the image corners / edges where distortion + vignetting are strongest
/// (F-098-N1). Uses the same real camera present in `lumina-lensfun`'s
/// database tests.
#[cfg(feature = "lensfun")]
#[test]
fn lensfun_corrector_changes_corner_pixels_vs_manual() {
    use lumina_lensfun::{Corrector, LensfunDb};
    // LENSFUN-DB-33: the loud path, so a machine without a resolvable
    // Lensfun profile database fails with the full named diagnostic
    // (every probed location + remediation) instead of a silent skip.
    let db = LensfunDb::resolve_system().expect("system lensfun db must resolve");
    let corrector = Corrector::for_camera(
        &db,
        "Nikon Corporation",
        "Nikon D40",
        Some("Nikon AF-S DX Zoom-Nikkor 18-55mm f/3.5-5.6G VR"),
        300,
        200,
        18.0,
        5.6,
        10.0,
    )
    .expect("real lensfun profile found for test camera");

    let frame = lensfun_gradient_frame(300, 200);
    let recipe = EditRecipe::default();

    let with_lensfun = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: Some(LensfunCorrectorRef(&corrector)),
            depth: None,
        },
    )
    .unwrap();
    let manual = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        },
    )
    .unwrap();

    // Sample near the four corners and the two top/bottom edge midpoints:
    // distortion + vignetting are strongest there.
    let samples = [
        (3u32, 3u32),
        (296, 3),
        (3, 196),
        (296, 196),
        (150, 3),
        (150, 196),
    ];
    let mut max_diff: u32 = 0;
    for &(cx, cy) in &samples {
        let i = (cy * 300 + cx) as usize * 4;
        for ch in 0..3 {
            let d = (with_lensfun.frame.pixels[i + ch] as i32 - manual.frame.pixels[i + ch] as i32)
                .unsigned_abs();
            max_diff = max_diff.max(d);
        }
    }
    assert!(
        max_diff > 1,
        "lensfun render must differ from manual at corners/edges, got max_diff={max_diff}"
    );
}

/// An unknown camera yields `None` from `for_camera`, so rendering with
/// that (absent) corrector must be byte-identical to the manual pipeline
/// (graceful fallback, F-098-N1).
#[cfg(feature = "lensfun")]
#[test]
fn unknown_camera_yields_identity_fallback_render() {
    use lumina_lensfun::{Corrector, LensfunDb};
    // LENSFUN-DB-33: the loud path, so a machine without a resolvable
    // Lensfun profile database fails with the full named diagnostic
    // (every probed location + remediation) instead of a silent skip.
    let db = LensfunDb::resolve_system().expect("system lensfun db must resolve");
    let corrector = Corrector::for_camera(
        &db,
        "NoSuchMake__XYZ",
        "NoSuchModel__XYZ",
        None,
        300,
        200,
        18.0,
        5.6,
        10.0,
    );
    assert!(
        corrector.is_none(),
        "unknown camera must yield no corrector"
    );

    let frame = lensfun_gradient_frame(120, 80);
    let recipe = EditRecipe::default();
    let render = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: corrector.as_ref().map(LensfunCorrectorRef),
            depth: None,
        },
    )
    .unwrap();
    let mut manual = frame.clone();
    manual
        .apply_recipe_with_white_balance(&recipe, None)
        .unwrap();
    assert_eq!(
        render.frame, manual,
        "unknown camera (None corrector) must equal the manual render"
    );
}
