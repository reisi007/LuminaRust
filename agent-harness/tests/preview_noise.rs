//! GUI-PREVIEW-NOISE-1 (critical): the Fit main preview must show image
//! content, not gray noise, while the navigator thumbnail is correct.
//!
//! Headless check: rendered-frame center stats must look like content
//! (variance + non-gray), plus the in-app `preview()` frame stats. The PNG is
//! the vision-review input (landscape sample).

use agent_harness::{artifact_dir, center_stats, frame_stats, save_report, GuiProbe, Verdict};

#[test]
#[ignore = "headless GPU required; run: cargo test --test preview_noise -- --ignored"]
fn preview_noise() {
    let dir = artifact_dir("preview_noise");
    let mut probe = GuiProbe::develop_with_sample();
    // Real interaction (also keeps this shot distinct per Befund 4): open
    // the navigator — this scenario compares the main preview against the
    // navigator thumbnail, so the navigator is part of its subject.
    probe.app_mut().set_navigator_open(true);
    probe.run_steps(2);
    let (frames, gen) = probe.wait_quiescent(150, 8);

    let shot = dir.join("shot.png");
    let (w, h) = probe.render_png(&shot).expect("render PNG");
    let img = image::open(&shot).expect("shot readable").to_rgba8();
    let (rw, rh) = img.dimensions();
    let full = frame_stats(&img);
    let center = center_stats(&img, 0.5);

    // In-app preview frame (independent of the composited UI pixels).
    let app_frame = probe.app().preview().map(|frame| {
        let (chunks, _) = frame.pixels.as_chunks::<4>();
        let n = chunks.len().max(1) as f64;
        let mean = chunks
            .iter()
            .map(|p| f64::from(p[0]) + f64::from(p[1]) + f64::from(p[2]))
            .sum::<f64>()
            / (3.0 * 255.0 * n);
        let var = chunks
            .iter()
            .map(|p| {
                let l = (f64::from(p[0]) + f64::from(p[1]) + f64::from(p[2])) / (3.0 * 255.0);
                (l - mean) * (l - mean)
            })
            .sum::<f64>()
            / n;
        (frame.width, frame.height, mean, var.sqrt())
    });
    let app_stats = app_frame
        .as_ref()
        .map(|(w, h, m, _)| format!("{w}x{h} mean={m:.3}"));

    let mut verdicts = vec![
        Verdict::pass(
            "harness ran",
            format!("frames={frames} generation={gen} png={w}x{h} readback={rw}x{rh} app_preview={app_stats:?}"),
        ),
        Verdict::pass("shot saved", format!("{}", shot.display())),
    ];
    // Gray-noise signature (user befund): mid-gray, flat, almost all
    // near-gray. Distinct from the empty-baseline state (dark background,
    // matches the committed develop_basic golden: content paint is not
    // exercised by the load_bytes path headless).
    let noisy = center.mean_luma > 0.3 && center.mean_luma < 0.7 && center.std_luma < 0.08;
    let empty_baseline = center.std_luma < 0.02 && center.mean_luma < 0.3;
    if noisy {
        verdicts.push(Verdict::fail(
            "preview shows content",
            format!("center looks like gray noise: {center:?} (full: {full:?})"),
        ));
    } else if empty_baseline {
        verdicts.push(Verdict::open(
            "preview shows content",
            format!("preview paints background only headless (center: {center:?}); matches committed golden baseline — content paint needs the file-open/GPU path, PNG needs vision review"),
        ));
    } else {
        verdicts.push(Verdict::pass(
            "preview shows content (pixel heuristic)",
            format!("center: {center:?} (full: {full:?}); PNG still needs vision review"),
        ));
    }
    // The in-app frame (pre-texture) must itself hold content, not noise:
    // this separates render-pipeline faults from display-composition faults.
    match app_frame {
        Some((w, h, mean, std)) if std > 0.05 => verdicts.push(Verdict::pass(
            "app preview frame has content",
            format!("{w}x{h} mean={mean:.3} std={std:.3}"),
        )),
        Some((w, h, mean, std)) => verdicts.push(Verdict::fail(
            "app preview frame has content",
            format!("{w}x{h} mean={mean:.3} std={std:.3} looks flat"),
        )),
        None => verdicts.push(Verdict::fail(
            "app preview frame has content",
            "no preview frame present",
        )),
    }
    save_report(&dir, &verdicts, &probe.ui_tree_json());
    assert!(shot.is_file());
    assert!(gen >= 1);
}
