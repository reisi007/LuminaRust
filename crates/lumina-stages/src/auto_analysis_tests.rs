//! AUTO-TONE-ANALYSIS-INPUT-8: the analysis **domain** of Auto-Tone, measured.
//!
//! The owner decision `AUTO-DOMAIN` (2026-10-02) fixes the frame Auto-Tone
//! evaluates: **after SourceActions and after Crop, without Adjustments**. The
//! tests here prove that with numbers on the production seam
//! [`auto_analysis_frame`], and they pin the two halves separately so a partial
//! implementation cannot pass:
//!
//! * the SourceActions stage is inside the domain (artifact + spot heals),
//! * the Crop stage is inside the domain, and it runs **last**,
//! * Adjustments are **outside** — no feedback from the previous Auto run,
//! * mask layers cannot reach the domain.
//!
//! GUI-level proofs of the same contract through the real `auto_tone` button live
//! in `crates/lumina-gui/src/tests/auto_tone_analysis_domain.rs`.

use crate::auto_tone::auto_analysis_frame;
use lumina_core::{ImageFrame, MaskPlane, SourceActionArtifact};
use lumina_sidecar::{Crop, EditRecipe, Geometry};

/// A 16x4 fixture of four vertical bands, levels `20 | 60 | 140 | 235`
/// (4 px each). Documented pixel function, no external asset.
fn banded_frame() -> ImageFrame {
    let (width, height) = (16u32, 4u32);
    let bands = [20u8, 60, 140, 235];
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..height {
        for x in 0..width {
            let level = bands[(x / (width / bands.len() as u32)) as usize];
            pixels.extend_from_slice(&[level, level, level, 255]);
        }
    }
    ImageFrame::new(width, height, pixels).unwrap()
}

/// A free crop keeping the left two bands (`20|60`).
fn left_half_crop() -> Geometry {
    Geometry {
        version: 1,
        crop: Some(Crop::Free {
            x: 0.0,
            y: 0.0,
            width: 0.5,
            height: 1.0,
        }),
        rotation_degrees: 0.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    }
}

/// A source action that replaces the **bright** right half with a bright gray.
///
/// It must change the analysis domain, which is the claim under test: a repair
/// region Auto does not see would be a silent wrong measurement.
fn repair_bright_half() -> SourceActionArtifact {
    let (width, height) = (16u32, 4u32);
    let mut region = Vec::with_capacity((width * height) as usize);
    // The replacement must cover the WHOLE frame; only the region mask selects
    // which pixels are actually taken from it.
    let mut replacement = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..height {
        for x in 0..width {
            region.push(if x < width / 2 { 0 } else { u16::MAX });
            let level = if x < width / 2 { 20 } else { 90 };
            replacement.extend_from_slice(&[level, level, level, 255]);
        }
    }
    SourceActionArtifact {
        region: MaskPlane::new(width, height, region).unwrap(),
        replacement: ImageFrame::new(width, height, replacement).unwrap(),
    }
}

/// **SourceActions are inside the domain.** The same source frame measured with
/// and without a repair action yields different analysis input bytes — and a
/// different tone analysis, not just a different buffer.
#[test]
fn a_source_action_changes_the_analysis_frame_and_the_tone_analysis() {
    let source = banded_frame();
    let recipe = EditRecipe::default();

    let without = auto_analysis_frame(&source, &recipe, &[]).unwrap();
    let with = auto_analysis_frame(&source, &recipe, &[repair_bright_half()]).unwrap();

    assert_ne!(
        without.pixels, with.pixels,
        "AUTO-TONE-ANALYSIS-INPUT-8: the SourceActions stage must be inside the analysis domain"
    );
    // The repair replaces level 235 with level 90, so p99 must fall measurably.
    let analysis_without = lumina_core::analyze_tone(&without);
    let analysis_with = lumina_core::analyze_tone(&with);
    assert!(
        analysis_with.p99 < analysis_without.p99 - 0.1,
        "p99 must fall once the bright half is repaired: {} vs {}",
        analysis_without.p99,
        analysis_with.p99
    );
    // The median moves too, and measurably so: the repair darkens half of the
    // bright half (235 -> 90), so the 50th percentile drops. Pinned so a change
    // in the repair geometry cannot pass unnoticed.
    assert_eq!(
        analysis_without.median, 0.392578125,
        "the un-repaired median is pinned"
    );
    assert_eq!(
        analysis_with.median, 0.294921875,
        "the repaired median is pinned"
    );
    assert!(
        analysis_with.median < analysis_without.median,
        "darkening the right half must lower the median: {} vs {}",
        analysis_without.median,
        analysis_with.median
    );
}

/// **Crop is inside the domain, and it runs AFTER the SourceActions.** The crop
/// is evaluated on the *repaired* frame, so a repair in the region the crop keeps
/// still changes the result — if the order were reversed (crop, then repair) the
/// repair would be applied to a differently sized frame and the combination would
/// differ.
#[test]
fn the_crop_is_applied_after_the_source_actions() {
    let source = banded_frame();
    let recipe = EditRecipe {
        geometry: Some(left_half_crop()),
        ..Default::default()
    };

    let cropped = auto_analysis_frame(&source, &recipe, &[]).unwrap();
    let cropped_repaired = auto_analysis_frame(&source, &recipe, &[repair_bright_half()]).unwrap();

    assert_eq!(
        (cropped.width, cropped.height),
        (8, 4),
        "the crop must be applied to the analysis frame"
    );
    // The repair only paints the right half, which the crop removes — so after
    // cropping, the repair is invisible. That is the proof that the crop runs
    // last: if the repair ran after the crop it would paint the kept half.
    assert_eq!(
        cropped.pixels, cropped_repaired.pixels,
        "with the crop applied last, a right-half repair cannot reach the kept half"
    );
}

/// **Adjustments are outside the domain — no feedback loop.** Two Auto runs where
/// the second sees the first run's written adjustments must still measure the
/// same frame, because the domain deliberately excludes Adjustments.
#[test]
fn adjustments_from_a_previous_auto_run_do_not_feed_back_into_the_domain() {
    let source = banded_frame();

    let mut recipe = EditRecipe::default();
    let first = auto_analysis_frame(&source, &recipe, &[]).unwrap();

    // Simulate the state a previous Auto-Tone run leaves behind.
    recipe.adjustments.insert("exposure".into(), 2.0);
    recipe.adjustments.insert("contrast".into(), -0.9);
    recipe.adjustments.insert("whites".into(), 0.7);
    let second = auto_analysis_frame(&source, &recipe, &[]).unwrap();

    assert_eq!(
        first.pixels, second.pixels,
        "Adjustments are outside the Auto-Tone domain, so a previous run cannot feed back into it"
    );
}

/// **The crop stage runs with `use_content_default = false`.** The
/// maximum-content default rect is defined on the lens/perspective-corrected
/// canvas; on this un-corrected analysis frame it must not silently shrink the
/// measurement. A recipe with lens correction but **no** explicit crop therefore
/// keeps the full frame here, while the render would apply the content default.
#[test]
fn an_implicit_content_default_crop_does_not_shrink_the_analysis_frame() {
    let source = banded_frame();
    let recipe = EditRecipe {
        lens_correction: Some(lumina_sidecar::LensCorrection {
            version: 1,
            profile: None,
            distortion_k1: Some(0.05),
            distortion_k2: None,
            distortion_k3: None,
            vignette_c0: None,
            vignette_c1: None,
            vignette_c2: None,
            ca_red: None,
            ca_blue: None,
        }),
        ..Default::default()
    };
    let frame = auto_analysis_frame(&source, &recipe, &[]).unwrap();
    assert_eq!(
        (frame.width, frame.height),
        (source.width, source.height),
        "the documented boundary: the implicit content-default crop stays out of the Auto domain"
    );
}
