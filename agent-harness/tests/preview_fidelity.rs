//! AGENT-HARNESS-4 Bildkorrektheit (F-100 Preview): Pixel-Asserts auf dem
//! In-App-Frame (`LuminaApp::preview()`), kein reiner Layout-Nachweis.
//!
//! Abgrenzung zu HARNESS-2/3 (Verweise, keine Duplikate):
//! - `opaque` / `center-content` / `deterministisch` wurden in
//!   `preview_noise.rs` (Painter-home) bereits als PASS belegt; hier kommen
//!   Schwellen mit Begründung, Fit-Rahmen, sRGB-Toleranz, Thumbnail-PSNR
//!   gegen committete Fixtures und der Stale-Generation-Guard dazu.
//! - Was in HARNESS-2 OPEN blieb (Composit-Content: die GPU-Textur malt
//!   headless schwarz), bleibt hier OPEN — gemessen und benannt, nicht
//!   stillschweigend als PASS verbucht.

use agent_harness::{
    artifact_dir,
    painter::{
        app_frame_center_stats, app_frame_stats, corner_max_abs_diff, frame_hash_fnv1a, frame_psnr,
        CORNER_TOL, PANEL_BG_RGB, WORKING_BG_RGB,
    },
    save_report, GuiProbe, Verdict,
};

// ---------------------------------------------------------------------------
// Schwellen mit Begründung (s. README § HARNESS-4)
// ---------------------------------------------------------------------------

/// Mindest-Luma-Delta (sRGB) zwischen geladenem Center und leerem Zustand.
/// 1 LSB = 1/255 ≈ 0.0039; Hintergrund-Textur (AA/Text) streut < 0.02 (vgl.
/// `empty_baseline` in `preview_noise.rs`: std < 0.02); 0.15 ≈ 38 LSB liegt
/// ~8× über jeder Hintergrundstreuung und weit unter der beobachteten
/// Content-Lücke (~0.35).
const CENTER_DELTA_MIN: f64 = 0.15;

/// Mindest-PSNR (dB) Preview gegen unabhängige sRGB-Referenz (direkter
/// `image`-Decode der committeten Sample-Bytes). Eine identische Pipeline
/// liegt bei ≥ 40 dB (nur PNG-/Float-Rundung); 30 dB ≙ RMSE ≈ 8 LSB fängt
/// Farbraum-Fehler (Doppel-Gamma, falsches Profil: kostet ≫ 10 dB), ohne bei
/// legitimer Rundung zu klemmen. Vergleich in sRGB, NICHT linear — s.
/// Gamma-Falle unten.
const SRGB_PSNR_MIN_DB: f64 = 30.0;

/// Relative sRGB-Luminanz von WORKING (`0x14`): 20/255 ≈ 0.078 — dunkel per
/// Theme-Vertrag (`BG_MAX_LUMINANCE = 0.12` in `theme.rs`).
fn working_luma() -> f64 {
    20.0 / 255.0
}

/// sRGB-Kanal (0..=255) → lineares Licht (WCAG-Formel, wie `theme.rs`).
fn linearize_channel(c: u8) -> f64 {
    let s = f64::from(c) / 255.0;
    if s <= 0.03928 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

// ---------------------------------------------------------------------------
// Unit-Tests ohne GPU (laufen in `cargo test`)
// ---------------------------------------------------------------------------

#[test]
fn gamma_trap_linear_vs_srgb_compare_false_fails() {
    // Die Gamma-Falle: Wer identische sRGB-Bytes im linearen Licht
    // vergleicht, misst auf Mid-Gray (128) eine Luma-Differenz von ~0.29 —
    // ein exakter Pipeline-Output würde als „falsch" gelten. Deshalb
    // vergleicht dieser Harness in sRGB (Gamma-Domäne).
    let srgb: f64 = 128.0 / 255.0;
    let linear = linearize_channel(128);
    assert!((srgb - 0.502).abs() < 1e-3, "srgb={srgb}");
    assert!((linear - 0.216).abs() < 1e-3, "linear={linear}");
    assert!(
        (srgb - linear) > 0.25,
        "Domänen müssen sich unterscheiden, sonst wäre die Falle gegenstandslos"
    );
}

#[test]
fn threshold_math_is_self_consistent() {
    // CENTER_DELTA_MIN ≈ 38 LSB — Größenordnungen über Quantisierung.
    let lsb = 1.0 / 255.0;
    assert!(CENTER_DELTA_MIN / lsb > 30.0);
    // 30 dB ≙ MSE = 255²/10³ = 65 ≙ RMSE ≈ 8 LSB.
    let mse = 255.0 * 255.0 / 10.0_f64.powf(SRGB_PSNR_MIN_DB / 10.0);
    assert!((mse.sqrt() - 8.06).abs() < 0.1, "rmse={}", mse.sqrt());
    // WORKING ist dunkel per Theme-Vertrag.
    assert!(working_luma() < 0.12);
    // CORNER_TOL (6 LSB) << CENTER_DELTA_MIN (38 LSB): Hintergrund-Toleranz
    // kann Content nie als Hintergrund durchgehen lassen.
    assert!(f64::from(CORNER_TOL) / 255.0 < CENTER_DELTA_MIN / 4.0);
}

// ---------------------------------------------------------------------------
// GPU-Szenario (headless wgpu, `#[ignore]` per Upstream-Konvention)
// ---------------------------------------------------------------------------

/// Committete Golden-Fixtures des GUI-Crates (via Pfad, kein neues Binär).
/// `library_with_image` ist bewusst KEIN Anker: seine Ecke (1021,717) ist
/// `[0,0,0]` (vorbestehendes Golden-Artefakt, s. README) — die beiden Anker
/// unten haben alle vier Ecken exakt auf PANEL.
const GOLDEN_DEVELOP_BASIC: &str = "../crates/lumina-gui/tests/snapshots/develop_basic.png";
const GOLDEN_HISTOGRAM_GRAPHIC: &str = "../crates/lumina-gui/tests/snapshots/histogram_graphic.png";

fn load_rgba(path: &str) -> Option<image::RgbaImage> {
    image::open(path).ok().map(|im| im.to_rgba8())
}

fn srgb_luma_mean(rgba: &image::RgbaImage) -> (f64, f64) {
    let n = (rgba.width() * rgba.height()).max(1) as f64;
    let mut sum = 0.0;
    let mut sum2 = 0.0;
    for px in rgba.pixels() {
        let l = (0.2126 * f64::from(px[0]) + 0.7152 * f64::from(px[1]) + 0.0722 * f64::from(px[2]))
            / 255.0;
        sum += l;
        sum2 += l * l;
    }
    let mean = sum / n;
    (mean, (sum2 / n - mean * mean).max(0.0).sqrt())
}

fn frame_to_rgba(width: u32, height: u32, pixels: &[u8]) -> Option<image::RgbaImage> {
    image::RgbaImage::from_raw(width, height, pixels.to_vec())
}

fn thumbnail_32(rgba: &image::RgbaImage) -> image::RgbaImage {
    image::imageops::resize(rgba, 32, 24, image::imageops::FilterType::Triangle)
}

#[test]
#[ignore = "headless GPU required; run: cargo test --test preview_fidelity -- --ignored"]
fn preview_fidelity() {
    let dir = artifact_dir("preview_fidelity");
    let mut verdicts: Vec<Verdict> = Vec::new();

    // ---- Phase 0: leerer Zustand (Baseline) -------------------------------
    let mut empty_probe = GuiProbe::new();
    empty_probe.run_steps(3);
    let empty_has_preview = empty_probe.app().preview().is_some();
    let empty_gen = empty_probe.app().preview_generation();
    let empty_shot = dir.join("shot_empty.png");
    empty_probe
        .render_png(&empty_shot)
        .expect("render empty PNG");
    let empty_img = load_rgba(empty_shot.to_str().unwrap()).expect("empty shot readable");
    let (empty_center_mean, empty_center_std) = {
        let c = agent_harness::center_stats(&empty_img, 0.5);
        (c.mean_luma, c.std_luma)
    };
    let empty_corner_diff = corner_max_abs_diff(&empty_img, PANEL_BG_RGB);
    if !empty_has_preview {
        verdicts.push(Verdict::pass(
            "empty state has no preview frame",
            format!("preview=None gen={empty_gen}"),
        ));
    } else {
        verdicts.push(Verdict::fail(
            "empty state has no preview frame",
            "preview present without image".to_string(),
        ));
    }
    if empty_corner_diff <= CORNER_TOL {
        verdicts.push(Verdict::pass(
            "empty window corners are theme background",
            format!("corner diff vs PANEL={empty_corner_diff} <= {CORNER_TOL}"),
        ));
    } else {
        verdicts.push(Verdict::fail(
            "empty window corners are theme background",
            format!("corner diff vs PANEL={empty_corner_diff} > {CORNER_TOL}"),
        ));
    }
    verdicts.push(Verdict::pass(
        "empty baseline recorded",
        format!("center mean={empty_center_mean:.3} std={empty_center_std:.3}"),
    ));

    // ---- Phase 1: Bild laden (In-App-Frame) --------------------------------
    let mut probe = GuiProbe::new();
    probe
        .app_mut()
        .load_bytes(lumina_gui::LuminaApp::sample_image_png(), "sample.png")
        .expect("sample image loads");
    probe.run_steps(2);
    let (_frames, gen_loaded) = probe.wait_quiescent(150, 8);
    let frame = probe
        .app()
        .preview()
        .expect("preview frame present after load")
        .clone();
    let (fw, fh) = (frame.width, frame.height);

    // 1a. Opaque-Alpha (Preview-Frame, NICHT Composit-Readback — Texturen
    // malen headless schwarz, s. HARNESS-2-Entscheid).
    match app_frame_stats(fw, fh, &frame.pixels) {
        Some(s) if s.opaque_fraction >= 1.0 => verdicts.push(Verdict::pass(
            "app preview frame is opaque",
            format!("{fw}x{fh} opaque_fraction=1.0"),
        )),
        Some(s) => verdicts.push(Verdict::fail(
            "app preview frame is opaque",
            format!("opaque_fraction={:.4}", s.opaque_fraction),
        )),
        None => verdicts.push(Verdict::fail(
            "app preview frame is opaque",
            "shape mismatch".to_string(),
        )),
    }

    // 1b. Center-Pixel-Delta geladen vs. leer (Schwelle s. oben).
    let center = app_frame_center_stats(fw, fh, &frame.pixels, 0.5).expect("center stats");
    let delta = (center.mean_luma - empty_center_mean).abs();
    if delta >= CENTER_DELTA_MIN {
        verdicts.push(Verdict::pass(
            "center pixel delta loaded vs empty",
            format!(
                "loaded={:.3} empty={:.3} delta={:.3} >= {CENTER_DELTA_MIN}",
                center.mean_luma, empty_center_mean, delta
            ),
        ));
    } else {
        verdicts.push(Verdict::fail(
            "center pixel delta loaded vs empty",
            format!(
                "loaded={:.3} empty={:.3} delta={:.3} < {CENTER_DELTA_MIN}",
                center.mean_luma, empty_center_mean, delta
            ),
        ));
    }

    // 1c. Composit-Ecken = Theme-Background (Fit-Rahmen/Chrom-Vertrag).
    // Der PREVIEW-Frame selbst trägt KEIN Letterbox (Quellgeometrie,
    // `render_from` → Zuweisung `:5382`; Fit passiert draw-seitig per
    // `preview_draw_dims` `:6717`) — seine Ecken sind Content, s. 1d.
    let shot = dir.join("shot_loaded.png");
    probe.render_png(&shot).expect("render loaded PNG");
    let shot_img = load_rgba(shot.to_str().unwrap()).expect("shot readable");
    let corner_diff = corner_max_abs_diff(&shot_img, PANEL_BG_RGB);
    if corner_diff <= CORNER_TOL {
        verdicts.push(Verdict::pass(
            "composited frame corners are theme background",
            format!("corner diff vs PANEL={corner_diff} <= {CORNER_TOL}"),
        ));
    } else {
        verdicts.push(Verdict::fail(
            "composited frame corners are theme background",
            format!("corner diff vs PANEL={corner_diff} > {CORNER_TOL}"),
        ));
    }

    // 1d. Preview-Frame-Ecken: per Design Content (kein Letterbox im
    // Pipeline-Output) — dokumentiert statt als Background-Fehlschlag.
    let corner_px: Vec<[u8; 3]> = {
        let rgba = frame_to_rgba(fw, fh, &frame.pixels).expect("frame to rgba");
        let (w, h) = (rgba.width(), rgba.height());
        [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)]
            .iter()
            .map(|(x, y)| {
                let p = rgba.get_pixel(*x, *y).0;
                [p[0], p[1], p[2]]
            })
            .collect()
    };
    let corners_are_working = corner_px.iter().all(|p| {
        p[0].abs_diff(WORKING_BG_RGB[0]) <= CORNER_TOL
            && p[1].abs_diff(WORKING_BG_RGB[1]) <= CORNER_TOL
            && p[2].abs_diff(WORKING_BG_RGB[2]) <= CORNER_TOL
    });
    verdicts.push(Verdict::open(
        "preview frame corners carry letterbox background",
        format!(
            "corners={corner_px:?} vs WORKING — {} (Pipeline-Output hat per Design kein Letterbox, s. README)",
            if corners_are_working {
                "unerwartet Background"
            } else {
                "Content wie erwartet"
            }
        ),
    ));

    // ---- Phase 2: Luminanz-Toleranz gegen sRGB-Referenz (Gamma-Falle) ------
    let sample_bytes = lumina_gui::LuminaApp::sample_image_png();
    let src_rgba = image::load_from_memory(&sample_bytes)
        .expect("sample decodes")
        .to_rgba8();
    let (sw, sh) = (src_rgba.width(), src_rgba.height());
    let src_resized =
        image::imageops::resize(&src_rgba, fw, fh, image::imageops::FilterType::Triangle);
    let psnr_full = frame_psnr(&src_resized.as_raw()[..], &frame.pixels);
    match psnr_full {
        Some(psnr) if psnr.is_infinite() => verdicts.push(Verdict::pass(
            "preview matches sRGB source (PSNR)",
            format!("{sw}x{sh}->{fw}x{fh} byte-identical (PSNR=inf, sRGB-Domäne)"),
        )),
        Some(psnr) if psnr >= SRGB_PSNR_MIN_DB => verdicts.push(Verdict::pass(
            "preview matches sRGB source (PSNR)",
            format!("{sw}x{sh}->{fw}x{fh} PSNR={psnr:.1}dB >= {SRGB_PSNR_MIN_DB} (sRGB-Domäne)"),
        )),
        Some(psnr) => verdicts.push(Verdict::fail(
            "preview matches sRGB source (PSNR)",
            format!("{sw}x{sh}->{fw}x{fh} PSNR={psnr:.1}dB < {SRGB_PSNR_MIN_DB}"),
        )),
        None => verdicts.push(Verdict::fail(
            "preview matches sRGB source (PSNR)",
            "length mismatch".to_string(),
        )),
    }
    // Gamma-Kontrollmessung: linear vs. sRGB auf derselben Referenz.
    let (srgb_mean, _) = srgb_luma_mean(&src_resized);
    let mut lin_sum = 0.0;
    let n = (src_resized.width() * src_resized.height()).max(1) as f64;
    for px in src_resized.pixels() {
        lin_sum += 0.2126 * linearize_channel(px[0])
            + 0.7152 * linearize_channel(px[1])
            + 0.0722 * linearize_channel(px[2]);
    }
    let domain_gap = (srgb_mean - lin_sum / n).abs();
    verdicts.push(Verdict::pass(
        "gamma domain gap documented",
        format!("sRGB-mean={srgb_mean:.3} linear-mean={:.3} gap={domain_gap:.3} — Vergleich läuft in sRGB", lin_sum / n),
    ));

    // ---- Phase 3: Thumbnail-Hash/PSNR --------------------------------------
    let frame_rgba = frame_to_rgba(fw, fh, &frame.pixels).expect("frame to rgba");
    let thumb = thumbnail_32(&frame_rgba);
    let thumb_hash = frame_hash_fnv1a(thumb.as_raw());
    // Determinismus auf Thumbnail-Ebene (HARNESS-2-Determinismus reused).
    probe.run_steps(5);
    let frame2 = probe.app().preview().expect("preview stable").clone();
    let thumb2 =
        thumbnail_32(&frame_to_rgba(frame2.width, frame2.height, &frame2.pixels).expect("rgba"));
    let thumb_psnr = frame_psnr(thumb.as_raw(), thumb2.as_raw());
    match thumb_psnr {
        Some(psnr) if psnr.is_infinite() => verdicts.push(Verdict::pass(
            "thumbnail is deterministic",
            format!("32x24 hash={thumb_hash:#x} identical across idle frames"),
        )),
        Some(psnr) => verdicts.push(Verdict::fail(
            "thumbnail is deterministic",
            format!("idle thumbnails differ (PSNR={psnr:.1}dB)"),
        )),
        None => verdicts.push(Verdict::fail(
            "thumbnail is deterministic",
            "shape mismatch".to_string(),
        )),
    }
    // Thumbnail gegen unabhängige Referenz (committete Sample-Bytes).
    let src_thumb =
        image::imageops::resize(&src_rgba, 32, 24, image::imageops::FilterType::Triangle);
    match frame_psnr(src_thumb.as_raw(), thumb.as_raw()) {
        Some(psnr) if psnr.is_infinite() || psnr >= SRGB_PSNR_MIN_DB => {
            verdicts.push(Verdict::pass(
                "thumbnail matches committed fixture (PSNR)",
                format!(
                    "hash={thumb_hash:#x} PSNR={} >= {SRGB_PSNR_MIN_DB}",
                    if psnr.is_infinite() {
                        "inf".to_string()
                    } else {
                        format!("{psnr:.1}dB")
                    }
                ),
            ));
        }
        Some(psnr) => verdicts.push(Verdict::fail(
            "thumbnail matches committed fixture (PSNR)",
            format!("hash={thumb_hash:#x} PSNR={psnr:.1}dB < {SRGB_PSNR_MIN_DB}"),
        )),
        None => verdicts.push(Verdict::fail(
            "thumbnail matches committed fixture (PSNR)",
            "shape mismatch".to_string(),
        )),
    }

    // ---- Phase 4: Golden-Anker (committete PNGs via Pfad) -------------------
    for (name, path) in [
        ("develop_basic", GOLDEN_DEVELOP_BASIC),
        ("histogram_graphic", GOLDEN_HISTOGRAM_GRAPHIC),
    ] {
        match load_rgba(path) {
            Some(golden) => {
                let (gw, gh) = (golden.width(), golden.height());
                let gcorner = corner_max_abs_diff(&golden, PANEL_BG_RGB);
                let (gmean, gstd) = srgb_luma_mean(&golden);
                let ghash = frame_hash_fnv1a(golden.as_raw());
                if gcorner <= CORNER_TOL {
                    verdicts.push(Verdict::pass(
                        format!("golden anchor {name}: corners are theme background"),
                        format!("{gw}x{gh} corner diff={gcorner} hash={ghash:#x} center-ish mean={gmean:.3} std={gstd:.3}"),
                    ));
                } else {
                    verdicts.push(Verdict::fail(
                        format!("golden anchor {name}: corners are theme background"),
                        format!("{gw}x{gh} corner diff={gcorner} > {CORNER_TOL}"),
                    ));
                }
            }
            None => verdicts.push(Verdict::fail(
                format!("golden anchor {name}: fixture readable"),
                format!("cannot open {path} (CWD-abhängig — von agent-harness/ aus laufen)"),
            )),
        }
    }
    verdicts.push(Verdict::open(
        "composited shot PSNR vs golden",
        "nicht assertiert: Harness-Fenster 1280x800 vs. Golden 1024x720 — Byte-PSNR über verschiedene Geometrien wäre Junk; Ecken/Historie s. Anker oben".to_string(),
    ));

    // ---- Phase 5: Composit-Center geladen vs. leer (HARNESS-2-OPEN reused) --
    let (loaded_center_mean, loaded_center_std) = {
        let c = agent_harness::center_stats(&shot_img, 0.5);
        (c.mean_luma, c.std_luma)
    };
    verdicts.push(Verdict::open(
        "composited center shows content",
        format!(
            "loaded mean={loaded_center_mean:.3} std={loaded_center_std:.3} vs empty mean={empty_center_mean:.3} — Textur malt headless schwarz (HARNESS-2-OPEN bleibt OPEN); In-App-Frame s. Center-Delta oben"
        ),
    ));

    // ---- Phase 6: Stale-Generation-Guard nach Bildwechsel -------------------
    let workdir = tempfile::tempdir().expect("temp dir");
    let file_a = workdir.path().join("a.png");
    let file_b = workdir.path().join("b.png");
    std::fs::write(&file_a, lumina_gui::LuminaApp::sample_image_png()).unwrap();
    // B: deterministisch generiert (kein Binär-Commit), andere Geometrie +
    // andere Farben als A, damit ein staler Frame auffliegt.
    let mut b_img = image::RgbaImage::new(8, 6);
    for (x, y, px) in b_img.enumerate_pixels_mut() {
        *px = if (x + y) % 2 == 0 {
            image::Rgba([30, 200, 80, 255])
        } else {
            image::Rgba([180, 40, 60, 255])
        };
    }
    {
        let mut f = std::fs::File::create(&file_b).unwrap();
        b_img
            .write_to(&mut f, image::ImageFormat::Png)
            .expect("write b.png");
    }
    let mut swap = GuiProbe::new();
    swap.app_mut().open_file(file_a.display().to_string());
    swap.run_steps(80);
    swap.wait_quiescent(120, 8);
    assert!(
        swap.app().preview().is_some(),
        "A must decode, error={:?}",
        swap.app().error()
    );
    let gen_a = swap.app().preview_generation();
    let frame_a = swap.app().preview().expect("frame A").clone();
    let hash_a = frame_hash_fnv1a(&frame_a.pixels);
    swap.app_mut().open_file(file_b.display().to_string());
    swap.run_steps(80);
    swap.wait_quiescent(120, 8);
    assert!(
        swap.app().preview().is_some(),
        "B must decode, error={:?}",
        swap.app().error()
    );
    let gen_b = swap.app().preview_generation();
    let frame_b = swap.app().preview().expect("frame B").clone();
    let hash_b = frame_hash_fnv1a(&frame_b.pixels);
    if gen_b > gen_a {
        verdicts.push(Verdict::pass(
            "generation switches on image change",
            format!("gen {gen_a} -> {gen_b}"),
        ));
    } else {
        verdicts.push(Verdict::fail(
            "generation switches on image change",
            format!("gen {gen_a} -> {gen_b} (stale)"),
        ));
    }
    if hash_b != hash_a {
        verdicts.push(Verdict::pass(
            "stale frame is not valued as new",
            format!(
                "hash {hash_a:#x} -> {hash_b:#x} ({}x{} -> {}x{})",
                frame_a.width, frame_a.height, frame_b.width, frame_b.height
            ),
        ));
    } else {
        verdicts.push(Verdict::fail(
            "stale frame is not valued as new",
            format!("hash unchanged {hash_a:#x} across image switch"),
        ));
    }

    save_report(&dir, &verdicts, &probe.ui_tree_json());
    assert!(shot.is_file());
    assert!(gen_loaded >= 1);
    assert!(
        !verdicts.iter().any(|v| v.result == "FAIL"),
        "verdicts: {verdicts:?}"
    );
}
