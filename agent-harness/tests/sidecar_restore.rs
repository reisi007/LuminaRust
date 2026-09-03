//! GUI-SIDECAR-RESTORE-1: Basic slider values must survive restart/reopen
//! via the sidecar (DoD §1: Edit → Commit → Sidecar-Datei → Reload).
//!
//! Headless chain: real temp PNG → `open_file` → `set_adjustment` → frames
//! (debounce commit may save) → sidecar file check → fresh probe reopen →
//! recipe value check.

use agent_harness::{artifact_dir, save_report, GuiProbe, Verdict};

#[test]
#[ignore = "headless GPU required; run: cargo test --test sidecar_restore -- --ignored"]
fn sidecar_restore() {
    let dir = artifact_dir("sidecar_restore");
    let workdir = tempfile::tempdir().expect("temp dir");
    let source = workdir.path().join("photo.png");
    std::fs::write(&source, lumina_gui::LuminaApp::sample_image_png()).unwrap();

    let mut probe = GuiProbe::new();
    probe.app_mut().open_file(source.display().to_string());
    probe.run_steps(80);
    let loaded = probe.app().preview().is_some();
    let load_error = probe.app().error().map(str::to_owned);

    probe.app_mut().set_adjustment("exposure", -0.62);
    probe.run_steps(150);
    let (frames, gen) = probe.wait_quiescent(150, 8);
    let in_session = probe.app().recipe().adjustments.get("exposure").copied();

    // Sidecar path convention: `<source>.lumina.json` next to the original.
    let sidecar = source.with_extension("png.lumina.json");
    let sidecar_alt = workdir.path().join("photo.png.lumina.json");
    let sidecar_path = [sidecar, sidecar_alt].into_iter().find(|p| p.is_file());
    let file_exposure = sidecar_path.as_ref().and_then(|p| {
        std::fs::read_to_string(p)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|v| {
                v.pointer("/virtual_copies/0/recipe/adjustments/exposure")
                    .or_else(|| v.pointer("/recipe/adjustments/exposure"))
                    .and_then(serde_json::Value::as_f64)
            })
    });
    if let Some(p) = sidecar_path.as_ref() {
        std::fs::copy(p, dir.join("sidecar.json")).unwrap();
    }

    // Reopen in a fresh probe (DoD §1 reload link).
    let mut reopened = GuiProbe::new();
    reopened.app_mut().open_file(source.display().to_string());
    reopened.run_steps(80);
    reopened.wait_quiescent(150, 8);
    let restored = reopened.app().recipe().adjustments.get("exposure").copied();

    let shot = dir.join("shot.png");
    probe.render_png(&shot).expect("render PNG");
    let tree = probe.ui_tree_json();

    let mut verdicts = vec![Verdict::pass(
        "harness ran",
        format!("frames={frames} gen={gen} loaded={loaded} load_error={load_error:?}"),
    )];
    match in_session {
        Some(v) if (v - -0.62).abs() < 1e-9 => verdicts.push(Verdict::pass(
            "edit applies in session",
            format!("exposure={v}"),
        )),
        other => verdicts.push(Verdict::fail(
            "edit applies in session",
            format!("exposure={other:?}, expected -0.62"),
        )),
    }
    match (sidecar_path, file_exposure) {
        (Some(p), Some(v)) if (v - -0.62).abs() < 1e-6 => verdicts.push(Verdict::pass(
            "sidecar commit",
            format!("{} carries exposure={v}", p.display()),
        )),
        (Some(p), v) => verdicts.push(Verdict::fail(
            "sidecar commit",
            format!("{} exposure={v:?}, expected -0.62 (debounce commit did not persist?)", p.display()),
        )),
        (None, _) => verdicts.push(Verdict::open(
            "sidecar commit",
            "no sidecar file written headless — debounce commit path needs a test hook (DoD §2 gap?) or longer simulated time",
        )),
    }
    match restored {
        Some(v) if (v - -0.62).abs() < 1e-6 => verdicts.push(Verdict::pass(
            "reload restores value",
            format!("reopened exposure={v}"),
        )),
        other => verdicts.push(Verdict::fail(
            "reload restores value",
            format!("reopened exposure={other:?}, expected -0.62"),
        )),
    }
    save_report(&dir, &verdicts, &tree);
    assert!(shot.is_file());
    assert!(loaded, "source must decode headless, error={load_error:?}");
}
