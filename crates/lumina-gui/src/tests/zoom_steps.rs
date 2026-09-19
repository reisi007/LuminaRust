//! zoom steps, nominal stages and neighbour badges tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

/// B4: the toolbar readout names the nominal zoom step (F-100), never the
/// effective on-screen scale; `Custom` names itself.
#[test]
fn zoom_label_names_nominal_step() {
    let mut app = new_app();
    for (mode, expected) in [
        (ZoomMode::Fit, "Fit"),
        (ZoomMode::Quarter, "25%"),
        (ZoomMode::Half, "50%"),
        (ZoomMode::ThreeQuarter, "75%"),
        (ZoomMode::OneToOne, "100%"),
        (ZoomMode::TwoHundred, "200%"),
        (ZoomMode::FitWidth, "Fit Width"),
        (ZoomMode::Custom, "Custom"),
    ] {
        app.zoom_mode = mode;
        assert_eq!(app.zoom_label(), expected, "{mode:?}");
    }
}

/// PREVIEW-CACHE-FEATURE (A2) + GUI-TOAST-OVERLAP-1: Loading/Stale/Failed
/// probes raise small corner badges (label + color); the active image and
/// Miss/Ready probes raise none (Ready owns the overlay toast instead).
#[test]
fn neighbor_preview_badges_for_loading_stale_failed() {
    use lumina_core::preview_cache::PreviewKind;
    use std::time::{Duration, Instant};

    fn seed_png(dir: &std::path::Path, name: &str, seed: u8) -> std::path::PathBuf {
        let (w, h) = (32u32, 20u32);
        let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
        for y in 0..h {
            for x in 0..w {
                let r = ((x * 255 / (w - 1)) as u8).wrapping_add(seed);
                let g = ((y * 255 / (h - 1)) as u8).wrapping_add(seed);
                pixels.extend_from_slice(&[r, g, 128, 255]);
            }
        }
        let png = ImageFrame::new(w, h, pixels)
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap();
        let path = dir.join(name);
        std::fs::write(&path, png).unwrap();
        path
    }

    fn neighbor_job(source: std::path::PathBuf, probe: &str) -> preview_ctrl::PreviewJob {
        let name = source.file_name().unwrap().to_string_lossy().into_owned();
        preview_ctrl::PreviewJob {
            probe_id: probe.to_string(),
            source,
            name,
            virtual_copy: "vc-original".into(),
            target: (64, 64),
            kind: PreviewKind::Screen,
            priority: 0,
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app();
    let (mut ctrl, _queue) = preview_ctrl::PreviewController::spawn(1);
    // Loading: enqueued but never polled — the worker result is still queued.
    let loading_src = seed_png(dir.path(), "loading.png", 1);
    assert!(ctrl.enqueue(neighbor_job(loading_src, "loading-probe")));
    assert_eq!(
        ctrl.probe_state("loading-probe"),
        preview_ctrl::PreviewProbeState::Loading
    );
    app.preview_ctrl = Some(ctrl);
    app.preview_ctrl.as_mut().unwrap().set_active("other-probe");
    let (label, color) = app
        .neighbor_preview_badge("loading-probe")
        .expect("a Loading probe must raise a badge");
    assert_eq!(label, Str::NeighborLoading.t());
    assert_eq!(color, egui::Color32::from_rgb(0x44, 0x66, 0x88));
    // The active image never shows a badge, whatever its probe state.
    app.preview_ctrl
        .as_mut()
        .unwrap()
        .set_active("loading-probe");
    assert!(
        app.neighbor_preview_badge("loading-probe").is_none(),
        "the active image must not carry a neighbor badge"
    );
    app.preview_ctrl.as_mut().unwrap().set_active("other-probe");
    // Stale: 8 distinct previews into the 7-slot RAM LRU evict exactly one.
    let mut ctrl = app.preview_ctrl.take().unwrap();
    let mut probes = vec!["loading-probe".to_string()];
    for i in 0..7u8 {
        let src = seed_png(dir.path(), &format!("stale-{i}.png"), 10 + i);
        let probe = format!("stale-probe-{i}");
        assert!(ctrl.enqueue(neighbor_job(src, &probe)), "enqueue {probe}");
        probes.push(probe);
    }
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        ctrl.poll();
        let pending = probes.iter().any(|probe| {
            matches!(
                ctrl.probe_state(probe),
                preview_ctrl::PreviewProbeState::Loading | preview_ctrl::PreviewProbeState::Miss
            )
        });
        if !pending {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "neighbor previews must settle, states: {:?}",
            probes
                .iter()
                .map(|probe| (probe, ctrl.probe_state(probe)))
                .collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let stale: Vec<String> = probes
        .iter()
        .filter(|probe| ctrl.probe_state(probe) == preview_ctrl::PreviewProbeState::Stale)
        .cloned()
        .collect();
    assert_eq!(
        stale.len(),
        1,
        "exactly one preview must be evicted to Stale, got {stale:?}"
    );
    app.preview_ctrl = Some(ctrl);
    app.preview_ctrl.as_mut().unwrap().set_active("other-probe");
    let (label, color) = app
        .neighbor_preview_badge(&stale[0])
        .expect("a Stale probe must raise a badge");
    assert_eq!(label, Str::NeighborStale.t());
    assert_eq!(color, egui::Color32::from_rgb(0xb0, 0x8a, 0x00));
    // Failed: a missing source exhausts the worker visibly, never silently.
    let mut ctrl = app.preview_ctrl.take().unwrap();
    assert!(
        ctrl.enqueue(neighbor_job(dir.path().join("gone.png"), "failed-probe")),
        "a missing source must still enqueue visibly"
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while ctrl.probe_state("failed-probe") == preview_ctrl::PreviewProbeState::Loading
        && Instant::now() < deadline
    {
        ctrl.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    ctrl.poll();
    assert_eq!(
        ctrl.probe_state("failed-probe"),
        preview_ctrl::PreviewProbeState::Failed,
        "a missing source must end Failed, never stuck Loading"
    );
    let message = ctrl
        .failure("failed-probe")
        .unwrap_or("unbekannt")
        .to_string();
    app.preview_ctrl = Some(ctrl);
    app.preview_ctrl.as_mut().unwrap().set_active("other-probe");
    let (label, color) = app
        .neighbor_preview_badge("failed-probe")
        .expect("a Failed probe must raise a badge");
    assert_eq!(label, Str::NeighborFailedPattern.format_arg(&message));
    assert_eq!(color, egui::Color32::from_rgb(0xb0, 0x2a, 0x2a));
}

/// GUI-PREVIEW-NAV-1 (F-100): every nominal zoom stage derives its
/// relative-to-fit multiplier and names itself in the toolbar readout;
/// continuous zoom pins `Custom` instead.
#[test]
fn zoom_step_cycles_all_nominal_stages() {
    let mut app = new_app();
    // Pane 800x600 over a 600x400 source: fit = 4/3.
    app.preview_base_fit_scale = (800.0f32 / 600.0).min(600.0 / 400.0);
    app.preview_pane_w = 800.0;
    app.preview_pane_h = 600.0;
    app.preview_src_w = 600.0;
    app.preview_src_h = 400.0;
    let fit = app.preview_base_fit_scale;
    for (mode, expected_zoom, expected_label) in [
        (ZoomMode::Fit, 1.0, "Fit"),
        (ZoomMode::Quarter, 0.25 / fit, "25%"),
        (ZoomMode::Half, 0.5 / fit, "50%"),
        (ZoomMode::ThreeQuarter, 0.75 / fit, "75%"),
        (ZoomMode::OneToOne, 1.0 / fit, "100%"),
        (ZoomMode::TwoHundred, 2.0 / fit, "200%"),
        (ZoomMode::FitWidth, (800.0 / 600.0) / fit, "Fit Width"),
    ] {
        app.preview_pan = egui::vec2(24.0, -12.0);
        app.set_zoom_mode(mode);
        app.sync_zoom();
        assert!(
            (app.preview_zoom - expected_zoom).abs() < 1e-4,
            "{mode:?} must derive zoom {expected_zoom}, got {}",
            app.preview_zoom
        );
        assert_eq!(app.zoom_label(), expected_label, "{mode:?} label");
        assert_eq!(
            app.preview_pan,
            egui::Vec2::ZERO,
            "{mode:?} must re-centre the pan"
        );
    }
    // Continuous zoom pins Custom with its own readout.
    app.set_zoom_mode(ZoomMode::Fit);
    app.sync_zoom();
    app.zoom_step(1.5);
    assert_eq!(app.zoom_mode, ZoomMode::Custom);
    assert!((app.preview_zoom - 1.5).abs() < 1e-6);
    assert_eq!(app.zoom_label(), "Custom");
}
