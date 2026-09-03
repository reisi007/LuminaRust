//! GUI-PREVIEW-NOISE-1 (critical): the Fit main preview must show image
//! content, not gray noise, while the navigator thumbnail is correct.
//!
//! Headless check: rendered-frame center stats must look like content
//! (variance + non-gray), plus the in-app `preview()` frame stats. The PNG is
//! the vision-review input (landscape sample).

use agent_harness::{
    artifact_dir, center_stats, frame_stats,
    painter::{app_frame_center_stats, app_frame_stats, frame_psnr},
    save_report, GuiProbe, Verdict,
};

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
    // Painter-home (HARNESS-2): the composited photo is a GPU texture (black
    // headless), so content proofs live on the in-app RGBA frame instead.
    if let Some(frame) = probe.app().preview() {
        let (fw, fh) = (frame.width, frame.height);
        match app_frame_stats(fw, fh, &frame.pixels) {
            Some(s) if s.opaque_fraction >= 1.0 => verdicts.push(Verdict::pass(
                "app preview frame is opaque",
                format!("{fw}x{fh} opaque_fraction=1.0"),
            )),
            Some(s) => verdicts.push(Verdict::fail(
                "app preview frame is opaque",
                format!(
                    "{fw}x{fh} opaque_fraction={:.4} — transparent pixels reach composition",
                    s.opaque_fraction
                ),
            )),
            None => verdicts.push(Verdict::fail(
                "app preview frame is opaque",
                format!("{fw}x{fh} len={} mismatches RGBA8", frame.pixels.len()),
            )),
        }
        match app_frame_center_stats(fw, fh, &frame.pixels, 0.5) {
            Some(c) if c.std_luma > 0.05 && c.mean_luma > 0.05 && c.mean_luma < 0.95 => {
                verdicts.push(Verdict::pass(
                    "app preview center holds image content",
                    format!("center mean={:.3} std={:.3}", c.mean_luma, c.std_luma),
                ));
            }
            Some(c) => verdicts.push(Verdict::fail(
                "app preview center holds image content",
                format!(
                    "center mean={:.3} std={:.3} looks empty",
                    c.mean_luma, c.std_luma
                ),
            )),
            None => verdicts.push(Verdict::fail(
                "app preview center holds image content",
                "center crop failed on a present frame",
            )),
        }
        // Determinism: consecutive frames without edits must be byte-identical.
        let before = frame.pixels.clone();
        let gen_before = probe.app().preview_generation();
        probe.run_steps(5);
        let after = probe.app().preview().map(|f| f.pixels.clone());
        match after {
            Some(after) => match frame_psnr(&before, &after) {
                Some(psnr) if psnr.is_infinite() => verdicts.push(Verdict::pass(
                    "app preview frame is deterministic",
                    format!("gen={gen_before} identical bytes across 5 idle frames"),
                )),
                Some(psnr) => verdicts.push(Verdict::fail(
                    "app preview frame is deterministic",
                    format!("gen={gen_before} idle frames differ (PSNR={psnr:.1}dB)"),
                )),
                None => verdicts.push(Verdict::fail(
                    "app preview frame is deterministic",
                    "frame length changed across idle frames",
                )),
            },
            None => verdicts.push(Verdict::fail(
                "app preview frame is deterministic",
                "preview frame vanished across idle frames",
            )),
        }
        // Edit response: an exposure lift must change frame bytes (content
        // follows the recipe — the gray-noise fault would not).
        probe.app_mut().set_adjustment("exposure", 2.0);
        probe.run_steps(150);
        probe.wait_quiescent(150, 8);
        let edited = probe.app().preview().map(|f| f.pixels.clone());
        match edited {
            Some(edited) => match frame_psnr(&before, &edited) {
                Some(psnr) if psnr.is_finite() => verdicts.push(Verdict::pass(
                    "edit changes preview pixels",
                    format!("exposure 0→2.0 re-rendered content (PSNR={psnr:.1}dB vs pre-edit)"),
                )),
                Some(_) => verdicts.push(Verdict::fail(
                    "edit changes preview pixels",
                    "exposure 0→2.0 left frame bytes identical — edit did not reach the pipeline",
                )),
                None => verdicts.push(Verdict::fail(
                    "edit changes preview pixels",
                    "frame length changed after edit",
                )),
            },
            None => verdicts.push(Verdict::fail(
                "edit changes preview pixels",
                "no preview frame after edit",
            )),
        }
    } else {
        for check in [
            "app preview frame is opaque",
            "app preview center holds image content",
            "app preview frame is deterministic",
            "edit changes preview pixels",
        ] {
            verdicts.push(Verdict::fail(check, "no preview frame present"));
        }
    }
    save_report(&dir, &verdicts, &probe.ui_tree_json());
    assert!(shot.is_file());
    assert!(gen >= 1);
}
