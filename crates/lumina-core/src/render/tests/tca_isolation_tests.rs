//! MASK-LOCAL-P1.2d counter-extraction: the G-06 Lensfun TCA isolation tests.
//!
//! Pure move out of `render.rs` (file-size ratchet): the block is byte-for-byte
//! the same code, it only lives in its own file now. The tests are feature-gated
//! on `lensfun` and hermetic — they build their own fixture database, so they
//! need no system Lensfun installation.

// The whole block below is `lensfun`-gated, so its scope import is too.
#[cfg(feature = "lensfun")]
use super::*;

// ---- G-06 Lensfun-Vollausbau: TCA (feature-gated; fixture DB, hermetic) ----

/// Minimal fixture database XML with ONE lens carrying distortion
/// (PTLens) calibration; `with_tca` adds a poly3 TCA calibration line.
/// Same distortion in both variants isolates the TCA render effect.
#[cfg(feature = "lensfun")]
fn write_tca_isolation_fixture(tag: &str, with_tca: bool) -> std::path::PathBuf {
    let tca = if with_tca {
        r#"<tca model="poly3" focal="50" vr="1.005" vb="0.995"/>"#
    } else {
        "<!-- no TCA calibration -->"
    };
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<lensdatabase>
    <camera>
        <maker>Lumina TCA Corp</maker>
        <model>Lumina TCA Body</model>
        <mount>LuminaTcaMount</mount>
        <cropfactor>1.5</cropfactor>
    </camera>
    <lens>
        <maker>Lumina TCA Corp</maker>
        <model>Lumina TCA 50mm f/2.8</model>
        <mount>LuminaTcaMount</mount>
        <cropfactor>1.5</cropfactor>
        <calibration>
            <distortion model="ptlens" focal="50" a="0.08" b="-0.10" c="0.02"/>
            {tca}
        </calibration>
    </lens>
</lensdatabase>
"#
    );
    let path =
        std::env::temp_dir().join(format!("lumina-core-tca-{tag}-{}.xml", std::process::id()));
    std::fs::write(&path, xml).expect("write tca fixture database");
    path
}

#[cfg(feature = "lensfun")]
fn tca_fixture_corrector(
    tag: &str,
    with_tca: bool,
) -> (lumina_lensfun::LensfunDb, lumina_lensfun::Corrector) {
    use lumina_lensfun::{Corrector, LensfunDb};
    let path = write_tca_isolation_fixture(tag, with_tca);
    let db = LensfunDb::load_file(&path).expect("tca fixture database must load");
    let _ = std::fs::remove_file(&path);
    let corrector = Corrector::for_camera(
        &db,
        "Lumina TCA Corp",
        "Lumina TCA Body",
        None,
        120,
        80,
        50.0,
        2.8,
        10.0,
    )
    .expect("tca fixture corrector must be built");
    (db, corrector)
}

/// A TCA-capable corrector must shift R/B relative to G in the render:
/// with the same distortion, the TCA render differs from the non-TCA
/// render at the corners (G-06 Lensfun-Vollausbau, TCA path active).
#[cfg(feature = "lensfun")]
#[test]
fn tca_corrector_render_differs_from_non_tca_render() {
    let (_tca_db, tca) = tca_fixture_corrector("diff-tca", true);
    let (_plain_db, plain) = tca_fixture_corrector("diff-plain", false);
    assert!(tca.has_tca());
    assert!(!plain.has_tca());
    let frame = lensfun_gradient_frame(120, 80);
    let recipe = EditRecipe::default();
    let render_with = |corrector: &lumina_lensfun::Corrector| {
        render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
                lensfun: Some(LensfunCorrectorRef(corrector)),
                depth: None,
            },
        )
        .unwrap()
        .frame
    };
    let tca_frame = render_with(&tca);
    let plain_frame = render_with(&plain);
    assert_ne!(
        tca_frame.pixels, plain_frame.pixels,
        "TCA render must differ from the same-distortion non-TCA render"
    );
    // Deterministic: the same TCA render repeats byte-identically.
    assert_eq!(tca_frame.pixels, render_with(&tca).pixels);
}

/// No double correction: with a TCA-capable corrector the manual
/// `ca_red`/`ca_blue` model is skipped, so setting manual CA changes
/// nothing (byte-identical renders); without a corrector the same manual
/// CA visibly applies (existing behaviour preserved).
#[cfg(feature = "lensfun")]
#[test]
fn manual_ca_skipped_under_tca_corrector_and_applied_without() {
    use lumina_sidecar::LensCorrection;
    let (_tca_db, tca) = tca_fixture_corrector("skip-tca", true);
    let frame = lensfun_gradient_frame(120, 80);
    let mut recipe = EditRecipe::default();
    recipe.lens_correction = Some(LensCorrection {
        version: 1,
        profile: None,
        distortion_k1: None,
        distortion_k2: None,
        distortion_k3: None,
        vignette_c0: None,
        vignette_c1: None,
        vignette_c2: None,
        ca_red: Some(0.02),
        ca_blue: Some(-0.02),
    });
    let plain_recipe = EditRecipe::default();
    let render_with = |recipe: &EditRecipe, corrector: Option<&lumina_lensfun::Corrector>| {
        render_frame(
            &frame,
            &RenderContext {
                recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
                lensfun: corrector.map(LensfunCorrectorRef),
                depth: None,
            },
        )
        .unwrap()
        .frame
    };
    // Under TCA: manual CA is skipped → identical to no-manual-CA.
    assert_eq!(
        render_with(&recipe, Some(&tca)).pixels,
        render_with(&plain_recipe, Some(&tca)).pixels,
        "manual CA must be skipped when Lensfun TCA is active"
    );
    // Without a corrector: manual CA applies → differs from identity.
    assert_ne!(
        render_with(&recipe, None).pixels,
        render_with(&plain_recipe, None).pixels,
        "manual CA must still apply without a Lensfun corrector"
    );
}
