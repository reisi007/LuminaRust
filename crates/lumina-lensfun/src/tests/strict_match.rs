//! GUI-ROUTING-N6: strict Lensfun profile matching.
//!
//! These tests pin the fix for the F-103-N6-Runde-1 routing badge: the former
//! `LF_SEARCH_LOOSE` search fabricated a camera (`EOS R1` → `EOS R`) and a lens
//! (`RF200-800mm …` → `RF 24-240mm …`) that are not in the database, silently
//! applying a **wrong** correction and forcing the
//! `lens_correction (Lensfun corrector)` CPU route. Matching is strict now: a
//! named camera/lens must exist in the database; otherwise the manual model
//! applies. Hermetic fixture databases, no system DB, no network.
//!
//! Extracted from `lib.rs` (file-size ratchet, User-Vorgabe 2026-09-17).

use super::*;

/// A Lensfun database with a camera and TWO similar zoom lenses. Used to pin the
/// strict (non-loose) matching contract: a named lens that is not in the
/// database must never be replaced by a similar-looking one.
fn write_strict_match_fixture(tag: &str) -> std::path::PathBuf {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<lensdatabase>
    <camera>
        <maker>Lumina Test Corp</maker>
        <model>Lumina Test Body</model>
        <mount>LuminaTestMount</mount>
        <cropfactor>1.5</cropfactor>
    </camera>
    <lens>
        <maker>Lumina Test Corp</maker>
        <model>Lumina Zoom 24-240mm f/4-6.3</model>
        <mount>LuminaTestMount</mount>
        <cropfactor>1.5</cropfactor>
        <calibration>
            <distortion model="ptlens" focal="24" a="0.02" b="-0.05" c="0.01"/>
        </calibration>
    </lens>
    <lens>
        <maker>Lumina Test Corp</maker>
        <model>Lumina Zoom 200-800mm f/6.3-9</model>
        <mount>LuminaTestMount</mount>
        <cropfactor>1.5</cropfactor>
        <calibration>
            <distortion model="ptlens" focal="200" a="0.08" b="-0.10" c="0.02"/>
            <tca model="poly3" focal="200" vr="1.005" vb="0.995"/>
        </calibration>
    </lens>
</lensdatabase>
"#;
    write_fixture_xml(tag, xml)
}

/// GUI-ROUTING-N6: a **named** lens must match a real database entry; the exact
/// 200-800mm profile is found and reports distortion + TCA.
#[test]
fn named_lens_matches_exactly_and_reports_flags() {
    let path = write_strict_match_fixture("strict-exact");
    let db = LensfunDb::load_file(&path).expect("fixture database must load");
    let _ = std::fs::remove_file(&path);

    let exact = Corrector::for_camera(
        &db,
        FIXTURE_CAM_MAKE,
        FIXTURE_CAM_MODEL,
        Some("Lumina Zoom 200-800mm f/6.3-9"),
        400,
        300,
        400.0,
        6.3,
        10.0,
    )
    .expect("the exact named lens must resolve to its profile");
    assert!(exact.has_distortion());
    assert!(exact.has_tca());
}

/// GUI-ROUTING-N6: the F-103-N6 root cause. A named lens that is **not** in the
/// database (here a 70-200mm against a 24-240mm / 200-800mm database) must
/// yield `None` — never a similar-looking substitute. Before the fix loose
/// matching silently returned the 24-240mm profile for the 200-800mm name,
/// applying a wrong correction and forcing the GPU→CPU routing badge.
#[test]
fn absent_named_lens_is_never_substituted() {
    let path = write_strict_match_fixture("strict-absent-lens");
    let db = LensfunDb::load_file(&path).expect("fixture database must load");
    let _ = std::fs::remove_file(&path);

    for name in [
        "Lumina Zoom 70-200mm f/2.8",
        "Lumina Zoom 200-800mm f/6.3-9 IS USM",
        "Totally Nonsense Lens XYZ",
    ] {
        assert!(
            Corrector::for_camera(
                &db,
                FIXTURE_CAM_MAKE,
                FIXTURE_CAM_MODEL,
                Some(name),
                400,
                300,
                240.0,
                6.3,
                10.0,
            )
            .is_none(),
            "named lens `{name}` has no database entry and must not be guessed"
        );
    }
}

/// GUI-ROUTING-N6: an unknown camera model must yield `None` instead of a
/// fabricated body match (loose matching used to map `EOS R1` → `EOS R`).
#[test]
fn absent_camera_model_is_never_fabricated() {
    let path = write_strict_match_fixture("strict-absent-camera");
    let db = LensfunDb::load_file(&path).expect("fixture database must load");
    let _ = std::fs::remove_file(&path);

    assert!(Corrector::for_camera(
        &db,
        FIXTURE_CAM_MAKE,
        "Lumina Test Body Mk II",
        Some("Lumina Zoom 200-800mm f/6.3-9"),
        400,
        300,
        400.0,
        6.3,
        10.0,
    )
    .is_none());
}

/// GUI-ROUTING-N6: the documented body/mount fallback for an absent lens name
/// stays intact (the SOLL allows it), so strict named-lens matching does not
/// over-reach.
#[test]
fn absent_lens_name_keeps_body_mount_fallback() {
    let path = write_vignetting_only_fixture("strict-body-fallback");
    let db = LensfunDb::load_file(&path).expect("fixture database must load");
    let _ = std::fs::remove_file(&path);

    assert!(Corrector::for_camera(
        &db,
        FIXTURE_CAM_MAKE,
        FIXTURE_CAM_MODEL,
        None,
        400,
        300,
        50.0,
        2.8,
        10.0,
    )
    .is_some());
}
