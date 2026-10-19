//! DOWNSCALER-UNIFY-71 — permanenter Byte-Anker und Vertragstest des einen
//! Downscale-Pfades.
//!
//! Die eingefrorenen SHA-256-Werte wurden am 2026-10-18 gegen HEAD `cb2da59`
//! ( vor der Konsolidierung ) gemessen:
//!
//! * `DownscaleTarget::MaxWidth` — gegen die alte `downscale_bilinear`
//!   ( `lib.rs:194-230` zu damals ). Byte-Gleichheit ist der Anker des Tasks:
//!   gleiche Geometrie inkl. Early-Returns, gleiche f64-Kernel-Arithmetik.
//! * `DownscaleTarget::LongEdge` — gegen die neue f64-Fassung. Messwerte der
//!   Konsolidierung: 82 von 1.120.249 reprasentativen `(w, h, d)`-Tripeln
//!   rundeten in der alten f32-Geometrie anders als in der neuen f64-Geometrie
//!   ( 0,0073 % ); an allen acht angekernten LongEdge-Fixtures sind die Bytes
//!   der alten f32-Fassung und der neuen f64-Fassung identisch.
//! * Nearest -> Bilinear: der exakte Flaechenmittelwert-Referenzabstand ( L1 )
//!   des Thumbnail-Pfades 640x400 @ LongEdge(200) sinkt von 0.419052
//!   ( Nearest, replizierte `filmstrip::downscale_rgba` ) auf 0.259108
//!   ( bilinear, kanonischer Pfad ) — die Qualitaetsverbesserung ist hier
//!   dauerhaft pinnend geprueft.
//!
//! Kein `#[ignore]`, keine Kruecken: jeder Test laeuft headless mit.

mod downscale_anchor_support;

use downscale_anchor_support::{area_reference, mean_l1, nearest_replication, sha256};
use lumina_core::{downscale, DownscaleTarget, ImageFrame};

// ---------------------------------------------------------------------------
// Determistische Fixtures
// ---------------------------------------------------------------------------

fn gradient_frame(w: u32, h: u32) -> ImageFrame {
    let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
    let wx = (w - 1).max(1);
    let hy = (h - 1).max(1);
    for y in 0..h {
        for x in 0..w {
            pixels.extend_from_slice(&[
                (x * 255 / wx) as u8,
                (y * 255 / hy) as u8,
                ((x + y) * 255 / (wx + hy)) as u8,
                255,
            ]);
        }
    }
    ImageFrame::new(w, h, pixels).expect("gradient fixture")
}

fn assert_bytes(tag: &str, frame: &ImageFrame, dims: (u32, u32), expected: &str) {
    assert_eq!((frame.width(), frame.height()), dims, "{tag}: dims");
    assert_eq!(sha256(frame.pixels()), expected, "{tag}: bytes");
}

// ---------------------------------------------------------------------------
// T1a — Byte-Anker `MaxWidth` ( Bytes der alten `downscale_bilinear`,
// HEAD cb2da59, vor der Konsolidierung gemessen ).
// ---------------------------------------------------------------------------

#[test]
fn downscale_max_width_byte_anchor_matches_pre_consolidation() {
    let f64x48 = gradient_frame(64, 48);
    assert_bytes(
        "64x48 @32",
        &downscale(&f64x48, DownscaleTarget::MaxWidth(32)).unwrap(),
        (32, 24),
        "3379a6e103ffc6948efb55165fefa45401c040609549f109ea8daceba8df12ea",
    );
    assert_bytes(
        "64x48 @64 (exact fit -> clone)",
        &downscale(&f64x48, DownscaleTarget::MaxWidth(64)).unwrap(),
        (64, 48),
        "90d27f3c511228275ec1e6e0f28da7c35250c1e0f76b69e1faf5febdcac95639",
    );
    assert_bytes(
        "64x48 @1024 (target > source -> clone)",
        &downscale(&f64x48, DownscaleTarget::MaxWidth(1024)).unwrap(),
        (64, 48),
        "90d27f3c511228275ec1e6e0f28da7c35250c1e0f76b69e1faf5febdcac95639",
    );
    assert_bytes(
        "64x48 @0 (clamps to 1x1)",
        &downscale(&f64x48, DownscaleTarget::MaxWidth(0)).unwrap(),
        (1, 1),
        "35c55b39e12bb820cea37dc9184b88488974b5e08c4f1c8a3817025c8cc05d33",
    );

    let f100x1 = gradient_frame(100, 1);
    assert_bytes(
        "100x1 @1",
        &downscale(&f100x1, DownscaleTarget::MaxWidth(1)).unwrap(),
        (1, 1),
        "9cc133040915a49019d1fe30ba53ef93060dbaee07b36ae2ffc03c219b5ac566",
    );

    let f1x1 = gradient_frame(1, 1);
    assert_bytes(
        "1x1 @0",
        &downscale(&f1x1, DownscaleTarget::MaxWidth(0)).unwrap(),
        (1, 1),
        "e3820096cb82366b860b8a4e668453a7aaaf423af03bdf289fa308ea03a79332",
    );
}

// ---------------------------------------------------------------------------
// T1b — Byte-Anker `LongEdge` ( Bytes der neuen f64-Fassung; identisch mit
// der alten f32-Fassung an allen acht Fixtures — siehe Modul-Doku ).
// ---------------------------------------------------------------------------

#[test]
fn downscale_long_edge_byte_anchor_frozen() {
    let p48x64 = gradient_frame(48, 64);
    assert_bytes(
        "48x64 @32",
        &downscale(&p48x64, DownscaleTarget::LongEdge(32)).unwrap(),
        (24, 32),
        "3120f9754600195d2d8e5723b8f5157b04123b1c39b308e330abeddfcf7aebd4",
    );
    assert_bytes(
        "48x64 @0 (guard -> clone)",
        &downscale(&p48x64, DownscaleTarget::LongEdge(0)).unwrap(),
        (48, 64),
        "5ff86acb8a435984fd3b8c8c8054eb3aab07889c69a6f055ee1d8516d4566d62",
    );
    assert_bytes(
        "48x64 @64 (exact fit -> clone)",
        &downscale(&p48x64, DownscaleTarget::LongEdge(64)).unwrap(),
        (48, 64),
        "5ff86acb8a435984fd3b8c8c8054eb3aab07889c69a6f055ee1d8516d4566d62",
    );

    let f320x200 = gradient_frame(320, 200);
    assert_bytes(
        "320x200 @160",
        &downscale(&f320x200, DownscaleTarget::LongEdge(160)).unwrap(),
        (160, 100),
        "829cbadcf1dffc231bf95ff379bcd4786ce972f534ee50d2b8cec157bd7ac8a6",
    );
    assert_bytes(
        "320x200 @320 (exact fit -> clone)",
        &downscale(&f320x200, DownscaleTarget::LongEdge(320)).unwrap(),
        (320, 200),
        "5e92b39da49ff1ff2889c53d25daea6cae689ed98cc138ae7ef0f49c53b13f37",
    );
    assert_bytes(
        "320x200 @321 (target > source -> clone)",
        &downscale(&f320x200, DownscaleTarget::LongEdge(321)).unwrap(),
        (320, 200),
        "5e92b39da49ff1ff2889c53d25daea6cae689ed98cc138ae7ef0f49c53b13f37",
    );

    let f640x400 = gradient_frame(640, 400);
    assert_bytes(
        "640x400 @200 (thumbnail path)",
        &downscale(&f640x400, DownscaleTarget::LongEdge(200)).unwrap(),
        (200, 125),
        "d69d5e0a150755ea306b796c56a8a378c52d48a9923085a1e20ece12e2dbdd8b",
    );

    let s1x100 = gradient_frame(1, 100);
    assert_bytes(
        "1x100 @1 (1px edge)",
        &downscale(&s1x100, DownscaleTarget::LongEdge(1)).unwrap(),
        (1, 1),
        "a502826c9e4bb400b3479350945acf0a78ce737e1bf164f957ffab28aad9f6c1",
    );

    let f333x221 = gradient_frame(333, 221);
    assert_bytes(
        "333x221 @100 (knapp)",
        &downscale(&f333x221, DownscaleTarget::LongEdge(100)).unwrap(),
        (100, 66),
        "72c9f88804e817a894ed53b16d1bcb3e6f9519ea51000194510a0212a25b69bf",
    );

    let f1000x2000 = gradient_frame(1000, 2000);
    assert_bytes(
        "1000x2000 @1999 (rounds to width, shrinks height)",
        &downscale(&f1000x2000, DownscaleTarget::LongEdge(1999)).unwrap(),
        (1000, 1999),
        "f488a95252a19bc750270417eea99445e935f92c14a7663a6f1ff98a0b29171c",
    );
}

// ---------------------------------------------------------------------------
// T2 — Genau ein Resampler ( Quelltext-Gate ): die drei Altnamen existieren
// nicht mehr; `ImageFrame::downscale` ist nur noch die dokumentierte
// Delegation.
// ---------------------------------------------------------------------------

#[test]
fn exactly_one_resampler_exists() {
    let core_src = include_str!("../src/lib.rs");
    assert!(
        core_src.contains("pub enum DownscaleTarget"),
        "the canonical target enum must exist"
    );
    assert!(
        core_src.contains("pub fn downscale(frame: &ImageFrame, target: DownscaleTarget)"),
        "the canonical entry point must exist"
    );
    assert!(
        core_src.contains("pub fn downscale(&self, max_dim: u32) -> ImageFrame"),
        "ImageFrame::downscale must remain as the documented delegation"
    );
    assert!(
        !core_src.contains("fn downscale_bilinear"),
        "the former MaxWidth resampler must be gone"
    );

    let filmstrip_src = include_str!("../../lumina-gui/src/filmstrip.rs");
    assert!(
        !filmstrip_src.contains("fn downscale_rgba"),
        "the former nearest-neighbour thumbnail resampler must be gone"
    );
    let preview_jobs_src = include_str!("../../lumina-gui/src/preview_jobs.rs");
    assert!(
        !preview_jobs_src.contains("fn downscale_to_target"),
        "the former tuple wrapper must be gone"
    );
}

// ---------------------------------------------------------------------------
// T3/T4 — Geometrie beider Zielformen inkl. Early-Returns, max=0,
// Ziel > Quelle, 1px-Kanten.
// ---------------------------------------------------------------------------

#[test]
fn long_edge_geometry_hits_the_cap_and_preserves_aspect() {
    for (w, h, d) in [
        (48u32, 64, 32),
        (320, 200, 160),
        (640, 400, 200),
        (333, 221, 100),
    ] {
        let out = downscale(&gradient_frame(w, h), DownscaleTarget::LongEdge(d)).unwrap();
        assert_eq!(
            out.width().max(out.height()),
            d,
            "{w}x{h} @{d}: long edge must hit the cap"
        );
        // Seitenverhaeltnis erhalten bis zum unabhaengigen Rundungsfehler
        // je Dimension ( |e| <= 0.5 ): |new_w*h - new_h*w| <= (w + h) / 2.
        let drift = out.width() as i64 * h as i64 - out.height() as i64 * w as i64;
        assert!(
            drift.unsigned_abs() * 2 <= w as u64 + h as u64,
            "{w}x{h} @{d}: aspect ratio drift {drift} exceeds rounding bound"
        );
    }

    // Guard: max_dim == 0 -> unveraenderter Klon.
    let f = gradient_frame(48, 64);
    let out = downscale(&f, DownscaleTarget::LongEdge(0)).unwrap();
    assert_eq!((out.width(), out.height()), (48, 64));
    assert_eq!(out.pixels(), f.pixels());

    // Guard: long <= max_dim -> unveraenderter Klon ( exakt und Ziel > Quelle ).
    for d in [64u32, 65, 1000] {
        let out = downscale(&f, DownscaleTarget::LongEdge(d)).unwrap();
        assert_eq!((out.width(), out.height()), (48, 64), "d={d}");
        assert_eq!(out.pixels(), f.pixels(), "d={d}");
    }

    // 1px-Kante.
    let out = downscale(&gradient_frame(1, 100), DownscaleTarget::LongEdge(1)).unwrap();
    assert_eq!((out.width(), out.height()), (1, 1));
}

#[test]
fn max_width_geometry_honours_the_width_cap() {
    // Never upscales: new_width == min(width, max_width); new_height folgt der
    // vertraglichen Formel round(new_width / width * height).max(1.0).
    for (w, h, mw) in [(64u32, 48, 32), (320, 200, 160), (100, 1, 1), (48, 64, 47)] {
        let out = downscale(&gradient_frame(w, h), DownscaleTarget::MaxWidth(mw)).unwrap();
        assert_eq!(out.width(), w.min(mw), "{w}x{h} @{mw}");
        let expected_height = (((w.min(mw) as f64 / w as f64) * h as f64).round().max(1.0)) as u32;
        assert_eq!(out.height(), expected_height, "{w}x{h} @{mw}");
    }

    // max_width == width -> unveraenderter Klon.
    let f = gradient_frame(64, 48);
    let out = downscale(&f, DownscaleTarget::MaxWidth(64)).unwrap();
    assert_eq!(out.pixels(), f.pixels());

    // max_width == 0 -> 1x1 (aspektgetreu gemindert).
    let out = downscale(&gradient_frame(100, 50), DownscaleTarget::MaxWidth(0)).unwrap();
    assert_eq!((out.width(), out.height()), (1, 1));
}

#[test]
fn downscale_handles_degenerate_frames_without_panicking() {
    // 0x0-Frame: beide Zielformen geben den Klon zurueck ( width==0- Guard ).
    let empty = ImageFrame::new(0, 0, vec![]).unwrap();
    for target in [DownscaleTarget::MaxWidth(8), DownscaleTarget::LongEdge(8)] {
        let out = downscale(&empty, target).unwrap();
        assert_eq!((out.width(), out.height()), (0, 0));
        assert_eq!(out.pixels(), empty.pixels());
    }
}

// ---------------------------------------------------------------------------
// T5 — Messung f32 -> f64 ( SOLL Abschnitt 7 ): die alte f32-Geometrie und
// die neue f64-Geometrie sind ueber dieselbe reprasentative Matrix verglichen
// worden; die Zahlen sind eingefroren.
// ---------------------------------------------------------------------------

fn geometry_f32(width: u32, height: u32, max_dim: u32) -> (u32, u32) {
    let long = width.max(height);
    let scale = max_dim as f32 / long as f32;
    (
        (width as f32 * scale).round().max(1.0) as u32,
        (height as f32 * scale).round().max(1.0) as u32,
    )
}

fn geometry_f64(width: u32, height: u32, max_dim: u32) -> (u32, u32) {
    let long = width.max(height);
    let scale = max_dim as f64 / long as f64;
    (
        (width as f64 * scale).round().max(1.0) as u32,
        (height as f64 * scale).round().max(1.0) as u32,
    )
}

#[test]
fn long_edge_f32_to_f64_geometry_difference_is_the_measured_one() {
    // Zwei konkrete belegte Beispiele aus der Messmatrix.
    assert_eq!(geometry_f32(147, 196, 2), (2, 2));
    assert_eq!(geometry_f64(147, 196, 2), (1, 2));
    assert_eq!(geometry_f32(72, 45, 100), (100, 62));
    assert_eq!(geometry_f64(72, 45, 100), (100, 63));

    // Dieselbe Matrix wie die Messung: jedes (w, h) in 1..=256 je Zielwert,
    // plus das grosse Dimensionen-Kreuz inkl. beider CR3-Geometrien.
    let targets = [
        1u32, 2, 3, 7, 16, 100, 160, 200, 256, 384, 480, 800, 1024, 1280, 2000, 4096, 6032,
    ];
    let mut total = 0u64;
    let mut diff = 0u64;
    for &d in &targets {
        for w in 1..=256u32 {
            for h in 1..=256u32 {
                total += 1;
                if geometry_f32(w, h, d) != geometry_f64(w, h, d) {
                    diff += 1;
                }
            }
        }
    }
    let big = [
        257u32, 320, 333, 400, 480, 512, 513, 640, 800, 1000, 1024, 1280, 1600, 2000, 2048, 3000,
        4024, 4096, 6032,
    ];
    for &w in &big {
        for &h in &big {
            for &d in &targets {
                total += 1;
                if geometry_f32(w, h, d) != geometry_f64(w, h, d) {
                    diff += 1;
                }
            }
        }
    }
    assert_eq!(total, 1_120_249, "measurement matrix must not drift");
    assert_eq!(
        diff, 82,
        "f32/f64 geometry difference count is the named, measured change"
    );
}

// ---------------------------------------------------------------------------
// T8 — Nearest -> Bilinear: der kanonische Pfad ist dem exakten
// Flaechenmittelwert naeher als der alte Nearest-Nachbar.
// ---------------------------------------------------------------------------

#[test]
fn bilinear_thumbnail_path_is_closer_to_the_area_reference_than_nearest() {
    let source = gradient_frame(640, 400);
    let (nearest, nw, nh) = nearest_replication(source.pixels(), 640, 400, 200);
    let bilinear = downscale(&source, DownscaleTarget::LongEdge(200)).unwrap();

    assert_eq!((nw, nh), (200, 125), "nearest geometry");
    assert_eq!(
        (bilinear.width(), bilinear.height()),
        (200, 125),
        "bilinear geometry"
    );
    assert_ne!(
        bilinear.pixels(),
        nearest.as_slice(),
        "the resampler change must be a real byte change"
    );

    // Beide Pins sind Messwerte von HEAD cb2da59 ( 2026-10-18 ).
    assert_eq!(
        sha256(&nearest),
        "862a56abbafa987b8ddc55aa51a65938779e4d6019bf97cfe06444b92f486145"
    );
    assert_eq!(
        sha256(bilinear.pixels()),
        "d69d5e0a150755ea306b796c56a8a378c52d48a9923085a1e20ece12e2dbdd8b"
    );

    let reference = area_reference(source.pixels(), 640, 400, 200, 125);
    let nearest_l1 = mean_l1(&nearest, &reference);
    let bilinear_l1 = mean_l1(bilinear.pixels(), &reference);
    assert!(
        (nearest_l1 - 0.419052).abs() < 1e-6,
        "nearest L1 pin: {nearest_l1}"
    );
    assert!(
        (bilinear_l1 - 0.259108).abs() < 1e-6,
        "bilinear L1 pin: {bilinear_l1}"
    );
    assert!(
        bilinear_l1 < nearest_l1,
        "bilinear must sit closer to the exact area reference: \
         bilinear {bilinear_l1:.6} vs nearest {nearest_l1:.6}"
    );
}

// ---------------------------------------------------------------------------
// T9 — Determinismus des kanonischen Pfades.
// ---------------------------------------------------------------------------

#[test]
fn downscale_is_deterministic() {
    let frame = gradient_frame(96, 64);
    let a = downscale(&frame, DownscaleTarget::LongEdge(32)).unwrap();
    let b = downscale(&frame, DownscaleTarget::LongEdge(32)).unwrap();
    assert_eq!(sha256(a.pixels()), sha256(b.pixels()));
    let c = downscale(&frame, DownscaleTarget::MaxWidth(40)).unwrap();
    let d = downscale(&frame, DownscaleTarget::MaxWidth(40)).unwrap();
    assert_eq!(sha256(c.pixels()), sha256(d.pixels()));
}
