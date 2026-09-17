//! GUI-GEN-GRANULAR-10 (F-100): end-to-end tests for the explicit per-module
//! regeneration command.
//!
//! These drive the **real binary** (clap parsing included) and verify the
//! F-100 contract:
//!
//! * `lumina regenerate --module <m>` regenerates exactly that value,
//! * the collective default (no `--module`) is the no-op on a fresh document,
//! * no module is recomputed implicitly.

use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_sidecar::{load_sidecar, sidecar_path_for, MaskStatus};
use std::fs;
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

/// 4x4 gray-gradient PNG so Auto-Tone and Exposure Matching produce real,
/// non-identity values.
fn write_gradient_png(directory: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
    let path = directory.path().join(name);
    let mut pixels = Vec::with_capacity(4 * 4 * 4);
    for y in 0..4u32 {
        for x in 0..4u32 {
            let value = ((x * 40 + y * 20) % 256) as u8;
            pixels.extend_from_slice(&[value, value, value, 255]);
        }
    }
    let frame = ImageFrame::new(4, 4, pixels).unwrap();
    fs::write(&path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    path
}

fn import(input: &std::path::Path) {
    let result = cli()
        .args(["import", "--input", input.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "import failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn run_regenerate(input: &std::path::Path, extra: &[&str]) -> (bool, serde_json::Value, String) {
    let mut args = vec!["regenerate", "--input", input.to_str().unwrap(), "--json"];
    args.extend_from_slice(extra);
    let result = cli().args(&args).output().unwrap();
    let stderr = String::from_utf8_lossy(&result.stderr).to_string();
    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or(serde_json::Value::Null);
    (result.status.success(), payload, stderr)
}

fn module_action<'a>(payload: &'a serde_json::Value, module: &str) -> Option<&'a str> {
    payload["modules"]
        .as_array()?
        .iter()
        .find(|entry| entry["module"].as_str() == Some(module))
        .and_then(|entry| entry["action"].as_str())
}

/// `regenerate --module auto-tone` regenerates exactly the auto features and
/// leaves every other persisted artifact untouched.
#[test]
fn regenerate_auto_tone_module_is_isolated() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_gradient_png(&directory, "regen-e2e.png");
    import(&input);

    let before = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let (ok, payload, stderr) = run_regenerate(&input, &["--module", "auto-tone"]);
    assert!(ok, "stderr: {stderr}");
    assert_eq!(payload["status"], "updated");
    assert_eq!(module_action(&payload, "auto-tone"), Some("generated"));

    let after = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let auto = &after.virtual_copies[0].recipe.auto_features;
    assert!(auto.enable_auto_tone);
    assert!(auto.auto_exposure.is_some());
    assert!(auto.analysis_fingerprint.is_some());
    for key in [
        "exposure",
        "contrast",
        "whites",
        "blacks",
        "highlights",
        "shadows",
    ] {
        assert!(
            after.virtual_copies[0].recipe.adjustments.contains_key(key),
            "missing auto slider `{key}`"
        );
    }
    // Isolated: masks and the (empty) mask layers are byte-identical.
    assert_eq!(
        after.virtual_copies[0].mask_library,
        before.virtual_copies[0].mask_library
    );
    assert_eq!(
        after.virtual_copies[0].mask_layers,
        before.virtual_copies[0].mask_layers
    );
}

/// `regenerate --module matching` only persists the matched exposure.
#[test]
fn regenerate_matching_module_is_isolated() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_gradient_png(&directory, "regen-match-e2e.png");
    import(&input);

    let before = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let (ok, payload, stderr) = run_regenerate(&input, &["--module", "matching"]);
    assert!(ok, "stderr: {stderr}");
    assert_eq!(module_action(&payload, "matching"), Some("generated"));

    let after = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let auto = &after.virtual_copies[0].recipe.auto_features;
    assert!(auto.match_total_exposure);
    assert!(auto.matched_exposure.is_some());
    assert!(!auto.enable_auto_tone, "matching must not run Auto-Tone");
    assert_eq!(
        after.virtual_copies[0].mask_library,
        before.virtual_copies[0].mask_library
    );
}

/// `regenerate --module masks` is an explicit refresh request (status
/// `Pending` + armed one-shot `update_masks`, M2) that changes nothing else.
#[test]
fn regenerate_masks_module_requests_refresh() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_gradient_png(&directory, "regen-masks-e2e.png");
    import(&input);
    let add = cli()
        .args([
            "mask",
            "--input",
            input.to_str().unwrap(),
            "--add-ai-select",
            "sky",
            "--name",
            "Sky",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        add.status.success(),
        "mask add failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );
    let before = load_sidecar(&sidecar_path_for(&input)).unwrap();

    let (ok, payload, stderr) = run_regenerate(&input, &["--module", "masks"]);
    assert!(ok, "stderr: {stderr}");
    assert_eq!(module_action(&payload, "masks"), Some("requested"));

    let after = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(
        after.virtual_copies[0]
            .mask_library
            .iter()
            .all(|mask| matches!(mask.status, MaskStatus::Pending)),
        "every requested mask must be Pending"
    );
    // The recipe differs from `before` only by the armed one-shot refresh.
    let mut expected = before.virtual_copies[0].recipe.clone();
    expected
        .options
        .insert("update_masks".into(), "true".into());
    assert_eq!(after.virtual_copies[0].recipe, expected);
}

/// M2: after an explicit `regenerate --module masks`, the following render
/// consumes the armed refresh and does **not** report the work as an implicit
/// re-inference. The mask is attached to a layer so the render decision layer
/// actually reaches it — without a layer the check would be vacuous (E1).
#[test]
fn explicit_refresh_render_does_not_warn_as_implicit() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_gradient_png(&directory, "regen-explicit-e2e.png");
    import(&input);
    let add = cli()
        .args([
            "mask",
            "--input",
            input.to_str().unwrap(),
            "--add-ai-select",
            "sky",
            "--name",
            "Sky",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        add.status.success(),
        "mask add failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );
    // Attach the freshly added mask so the render has a reachable layer;
    // otherwise `mask_layers` is empty and no re-inference (hence no warning)
    // could ever occur.
    let mask_id = load_sidecar(&sidecar_path_for(&input))
        .unwrap()
        .virtual_copies[0]
        .mask_library[0]
        .id
        .clone();
    let attach = cli()
        .args([
            "mask",
            "--input",
            input.to_str().unwrap(),
            "--attach-layer",
            &mask_id,
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        attach.status.success(),
        "mask attach failed: {}",
        String::from_utf8_lossy(&attach.stderr)
    );
    assert_eq!(
        load_sidecar(&sidecar_path_for(&input))
            .unwrap()
            .virtual_copies[0]
            .mask_layers
            .len(),
        1,
        "the render test needs a reachable mask layer to be meaningful"
    );

    let (ok, _, stderr) = run_regenerate(&input, &["--module", "masks"]);
    assert!(ok, "stderr: {stderr}");

    let output = directory.path().join("explicit-render.png");
    let result = cli()
        .args([
            "render",
            "--input",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "render failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        !stderr.contains("re-inferred"),
        "an explicit refresh must not warn as implicit: {stderr}"
    );
}

/// The collective default regenerates every enabled stale/missing module and
/// reports `updated`.
#[test]
fn regenerate_collective_generates_enabled_stale_modules() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_gradient_png(&directory, "regen-all-e2e.png");
    import(&input);
    // Enable both analysis modules without values → stale/missing.
    let sidecar = sidecar_path_for(&input);
    let mut document = load_sidecar(&sidecar).unwrap();
    document.virtual_copies[0]
        .recipe
        .auto_features
        .enable_auto_tone = true;
    document.virtual_copies[0]
        .recipe
        .auto_features
        .match_total_exposure = true;
    lumina_sidecar::save_sidecar(&sidecar, &document).unwrap();

    let (ok, payload, stderr) = run_regenerate(&input, &[]);
    assert!(ok, "stderr: {stderr}");
    assert_eq!(payload["status"], "updated");
    assert_eq!(module_action(&payload, "auto-tone"), Some("generated"));
    assert_eq!(module_action(&payload, "matching"), Some("generated"));

    let after = load_sidecar(&sidecar).unwrap();
    let auto = &after.virtual_copies[0].recipe.auto_features;
    assert!(auto.auto_exposure.is_some());
    assert!(auto.matched_exposure.is_some());
}

/// The collective default on a fully fresh document is a pure read: the
/// sidecar bytes are byte-identical (no implicit recomputation).
#[test]
fn regenerate_collective_is_a_noop_on_a_fresh_document() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_gradient_png(&directory, "regen-fresh-e2e.png");
    import(&input);
    let sidecar = sidecar_path_for(&input);
    let before = fs::read(&sidecar).unwrap();

    let (ok, payload, stderr) = run_regenerate(&input, &[]);
    assert!(ok, "stderr: {stderr}");
    assert_eq!(payload["status"], "unchanged");

    assert_eq!(
        fs::read(&sidecar).unwrap(),
        before,
        "a fully fresh document must not be rewritten"
    );
}

/// M2b: the collective default refreshes stale masks through their per-mask
/// `Pending` marker only — it never arms the copy-wide `update_masks` switch,
/// which would also re-infer fresh `Valid` masks at render time.
#[test]
fn collective_regenerate_does_not_arm_the_copy_wide_refresh() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_gradient_png(&directory, "regen-collective-masks-e2e.png");
    import(&input);
    // An AI mask starts `Pending` (no persisted artifact) → stale.
    let add = cli()
        .args([
            "mask",
            "--input",
            input.to_str().unwrap(),
            "--add-ai-select",
            "sky",
            "--name",
            "Sky",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        add.status.success(),
        "mask add failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );

    let (ok, payload, stderr) = run_regenerate(&input, &[]);
    assert!(ok, "stderr: {stderr}");
    assert_eq!(module_action(&payload, "masks"), Some("requested"));

    let after = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(after.virtual_copies[0]
        .mask_library
        .iter()
        .all(|mask| matches!(mask.status, MaskStatus::Pending)));
    assert!(
        !after.virtual_copies[0]
            .recipe
            .options
            .contains_key("update_masks"),
        "the collective default must not arm the copy-wide refresh switch"
    );
}

#[test]
fn regenerate_rejects_invalid_target_luminance() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_gradient_png(&directory, "regen-invalid.png");
    let result = cli()
        .args([
            "regenerate",
            "--input",
            input.to_str().unwrap(),
            "--target-luminance",
            "2.0",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("target-luminance"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

/// The command advertises its module surface (clap `--module` value set).
#[test]
fn regenerate_help_lists_the_1_0_modules() {
    let result = cli().args(["regenerate", "--help"]).output().unwrap();
    assert!(result.status.success());
    let help = String::from_utf8_lossy(&result.stdout);
    for module in ["masks", "auto-tone", "matching"] {
        assert!(help.contains(module), "help must list `{module}`: {help}");
    }
}
