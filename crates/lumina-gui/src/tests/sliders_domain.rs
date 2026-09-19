//! slider reset and display-domain mapping tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

// ---- F-103-N2: single-control reset semantics + display scaling ----

#[test]
fn slider_reset_only_this_control_keeps_other_adjustments() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    app.set_adjustment("exposure", 2.0);
    app.set_adjustment("contrast", 0.5);
    app.set_adjustment("wb_temperature", 7000.0);
    // Resetting one control must not touch the others or the whole recipe.
    app.reset_single_adjustment("exposure");
    assert_eq!(app.recipe().adjustments["exposure"], 0.0);
    assert_eq!(app.recipe().adjustments["contrast"], 0.5);
    assert_eq!(app.recipe().adjustments["wb_temperature"], 7000.0);
    assert!(!app.recipe().adjustments.is_empty());
}

#[test]
fn display_scale_percent_maps_internal_domain() {
    // `-1..=1` is shown as `-100..+100`; Exposure/Kelvin stay identity.
    let percent = crate::slider::percent_spec(-1.0..=1.0, 0.0);
    assert_eq!(percent.scale, crate::slider::DisplayScale::Percent);
    assert_eq!(
        crate::slider::to_display(-1.0, crate::slider::DisplayScale::Percent),
        -100.0
    );
    assert_eq!(
        crate::slider::to_display(0.5, crate::slider::DisplayScale::Percent),
        50.0
    );
    assert_eq!(
        crate::slider::from_display(-100.0, crate::slider::DisplayScale::Percent),
        -1.0
    );
    let identity = crate::slider::identity_spec(-10.0..=10.0, 0.0, 0.1);
    assert_eq!(identity.scale, crate::slider::DisplayScale::Identity);
    assert_eq!(
        crate::slider::to_display(2.5, crate::slider::DisplayScale::Identity),
        2.5
    );
}

// ---- F-103-N7: Presence + Vibrance/Saturation controls ----

#[test]
fn vibrance_and_saturation_write_correct_adjustment_keys_and_domain() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    // F-092 Dynamics/Saturation: flat adjustments on the `-1..=1` domain.
    app.set_adjustment("vibrance", 0.5);
    app.set_adjustment("saturation", -0.25);
    assert_eq!(app.recipe().adjustments["vibrance"], 0.5);
    assert_eq!(app.recipe().adjustments["saturation"], -0.25);
    // Stored in the normative domain (pipeline/sidecar validate `-1..=1`).
    assert!(((-1.0)..=1.0).contains(&app.recipe().adjustments["vibrance"]));
    assert!(((-1.0)..=1.0).contains(&app.recipe().adjustments["saturation"]));
    // The flat `saturation` adjustment is distinct from the HSL mixer's
    // per-channel saturation storage.
    assert!(app.recipe().hsl.is_none());
}

#[test]
fn presence_set_writes_recipe_fields_with_neutral_default_and_domain() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    // F-094 Presence: `texture` / `clarity` / `dehaze` on the `-1..=1`
    // domain; the GUI initializes to the neutral 0.0 each.
    app.set_presence("texture", 0.0);
    app.set_presence("clarity", 0.0);
    app.set_presence("dehaze", 0.0);
    let p = app.recipe().presence.as_ref().unwrap();
    assert_eq!((p.texture, p.clarity, p.dehaze), (0.0, 0.0, 0.0));

    // A non-zero setting lands in the correct recipe field, in-domain.
    app.set_presence("texture", 0.8);
    app.set_presence("clarity", -0.5);
    app.set_presence("dehaze", 0.3);
    let p = app.recipe().presence.as_ref().unwrap();
    assert_eq!(p.texture, 0.8);
    assert_eq!(p.clarity, -0.5);
    assert_eq!(p.dehaze, 0.3);
    for v in [p.texture, p.clarity, p.dehaze] {
        assert!((-1.0..=1.0).contains(&(v as f64)));
    }

    // Unknown field names are ignored, leaving the struct untouched.
    let before = *app.recipe().presence.as_ref().unwrap();
    app.set_presence("echo", 0.9);
    assert_eq!(*app.recipe().presence.as_ref().unwrap(), before);
}

#[test]
fn presence_display_scaling_is_percent_for_internal_domain() {
    // F-094 Presence shares the `-1..=1` -> `-100..+100` Lightroom scale.
    let spec = crate::slider::percent_spec(-1.0..=1.0, 0.0);
    assert_eq!(spec.scale, crate::slider::DisplayScale::Percent);
    assert_eq!(
        crate::slider::to_display(0.8, crate::slider::DisplayScale::Percent),
        80.0
    );
    assert_eq!(
        crate::slider::from_display(-50.0, crate::slider::DisplayScale::Percent),
        -0.5
    );
}

#[test]
fn single_control_reset_keeps_other_dynamics_and_presence() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    // Seed Dynamics and a Presence field, then reset only one of each.
    app.set_adjustment("vibrance", 0.7);
    app.set_adjustment("saturation", 0.4);
    app.set_presence("clarity", 0.6);
    // Resetting Vibrance must not touch Saturation or Presence.
    app.reset_single_adjustment("vibrance");
    assert_eq!(app.recipe().adjustments["vibrance"], 0.0);
    assert_eq!(app.recipe().adjustments["saturation"], 0.4);
    // Default for these flat keys is the documented neutral 0.0.
    assert_eq!(LuminaApp::default_for_adjustment("vibrance"), 0.0);
    assert_eq!(LuminaApp::default_for_adjustment("saturation"), 0.0);
    // Presence neutral default is 0.0 per the GUI initializer.
    assert_eq!(app.recipe().presence.as_ref().unwrap().clarity, 0.6);
}
