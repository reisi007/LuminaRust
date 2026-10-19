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
//!   ( 0,0073 % ).
//! * Nearest -> Bilinear: der exakte Flaechenmittelwert-Referenzabstand ( L1 )
//!   des Thumbnail-Pfades 640x400 @ LongEdge(200) sinkt von 0.419052
//!   ( Nearest, replizierte `filmstrip::downscale_rgba` ) auf 0.259108
//!   ( bilinear, kanonischer Pfad ) — die Qualitaetsverbesserung ist hier
//!   dauerhaft pinnend geprueft.
//!
//! **Was ueber die Pixel-Bytes der alten f32-Fassung belegt ist** ( Stand
//! 2026-10-19, DOWSCALE-ANCHOR-PIXEL-DELTA-77 ): die alte f32-Fassung liegt
//! als testseitige Referenz vor ( `downscale_anchor_support::f32_long_edge_replication`,
//! woertlich transkribiert aus HEAD `cb2da59`, `crates/lumina-core/src/lib.rs:486-538` ).
//!
//! * **In-repo belegt (jeden Tag laufend):** die Kontrollgruppe in
//!   `downscale_anchor/pixel_delta.rs` faehrt **alle zehn** gepinnten
//!   LongEdge-Aufrufe des Byte-Ankers durch **beide** Fassungen. Fuer jeden
//!   Pin traegt die f32-Replikation **denselben** gepinnten SHA-256 wie die
//!   f64-Fassung und es gibt **null** abweichende Bytes. Die Aussage „die
//!   Bytes der alten f32-Fassung und der neuen f64-Fassung sind identisch"
//!   ist damit fuer die gepinnten Gradienten-Fixtures **gemessen, nicht
//!   behauptet** — in-repo, ohne Wegwerf-Programm.
//! * **Externer Prozessbeleg (2026-10-19, nicht im Repo reproduzierbar):**
//!   die **Transkription** selbst wurde gegen die alte Implementierung an
//!   HEAD `cb2da59` validiert — ein Wegwerf-Programm extrahierte den alten
//!   Methodenrumpf woertlich aus `git show cb2da59:…` und verglich ihn in
//!   **elf** Fixture/Target-Kombinationen (Gradient und nicht-glatt, inklusive
//!   der Early-Returns) Byte fuer Byte mit der Transkription: identisch in
//!   allen elf. Das Validierungsprogramm ist **nicht** committet; im Repo
//!   bleibt die Transkription mit ihrer Herkunftsangabe.
//! * **Nicht-glatte Bilder**: die Identitaet ist **nicht** allgemein wahr. Auf
//!   dem nicht-glatten Fixture ( harte Kanten plus Textur ) weicht die
//!   f64-Fassung von der f32-Fassung ab: **229 von 64.000 Bytes**
//!   ( 0,3578125 % ) bei 320x200 @ LongEdge(160) und **2543 von 100.000
//!   Bytes** ( 2,543 % ) bei 640x400 @ LongEdge(200) — jeweils **maximal
//!   1 LSB**. Die Zahlen sind in `downscale_anchor/pixel_delta.rs` gepinnt.
//!   Die **Geometrie** ist in beiden Fassungen identisch; weicht sie ab
//!   ( 82 der 1.120.249 Tripel ), ist ein Byte-Vergleich unmoeglich und wird
//!   getrennt behandelt und ebenfalls gepinnt.
//!
//! **Der Geltungsbereich des Resampler-Gates** ( Stand 2026-10-19,
//! RESAMPLER-GATE-SCOPE-78 ): das Gate `exactly_one_resampler_exists` war eine
//! **3-Datei-String-Blacklist** — es pruefte nur `lumina-core/src/lib.rs`,
//! `lumina-gui/src/filmstrip.rs` und `lumina-gui/src/preview_jobs.rs`. Damit
//! lag jeder Resampler **ausserhalb** dieser drei Dateien ausserhalb des
//! Gates. Das Gate liegt jetzt in `downscale_anchor/resampler_gate.rs`:
//! **workspaceweit** — alle Crates, je `src/`, `tests/`, `benches/`,
//! `examples/`, `bench/`, nur `.rs`, ohne `target/` und `.git/`; **nicht**
//! erfasst (jeweils namentlich begruendet in
//! `downscale_anchor/resampler_scan.rs`): `build.rs` auf der Crate-Wurzel und
//! `agent-harness/` (kein Workspace-Mitglied). Das Gate prueft zusaetzlich den
//! kanonischen Pfad als Gegenpol (kein vakuum-gruenes Gate) und benennt die
//! Frame-vs-Masken-Plane-Grenze **namentlich**.
//!
//! Kein `#[ignore]`, keine Kruecken: jeder Test laeuft headless mit.

mod downscale_anchor_support;
// Das Nested-Testmodul-Muster des Repos (vgl. `lumina-gui/src/tests/histogram.rs`
// + `tests/histogram/…`): der Pixel-Delta-Anker liegt in einem eigenen
// Unterverzeichnis, damit `downscale_anchor.rs` unter der 500-Zeilen-Schanke
// bleibt. `#[path]` ist noetig, weil der Testkroot direkt in `tests/` liegt —
// ein `mod` ohne `#[path]` wuerde `tests/pixel_delta.rs` suchen (ein eigenes
// Testtarget) statt `tests/downscale_anchor/pixel_delta.rs`.
#[path = "downscale_anchor/pixel_delta.rs"]
mod pixel_delta;
// Das Resampler-Gate (RESAMPLER-GATE-SCOPE-78) liegt in zwei eigenen
// Unterverzeichnis-Dateien: es ist ein **workspaceweiter Quelltext-Scan** und
// laesst sich nicht in die 500-Zeilen-Schranke dieses Testkroots legen. Der
// `#[path]`-Zusatz ist noetig, weil der Testkroot direkt in `tests/` liegt.
#[path = "downscale_anchor/resampler_gate.rs"]
mod resampler_gate;
#[path = "downscale_anchor/resampler_inventory.rs"]
mod resampler_inventory;
#[path = "downscale_anchor/resampler_lex.rs"]
mod resampler_lex;
#[path = "downscale_anchor/resampler_scan.rs"]
mod resampler_scan;

use downscale_anchor_support::{
    area_reference, gradient_pixels, mean_l1, nearest_replication, sha256, LONG_EDGE_ANCHOR_PINS,
};
use lumina_core::{downscale, DownscaleTarget, ImageFrame};

// ---------------------------------------------------------------------------
// Determistische Fixtures
// ---------------------------------------------------------------------------

fn gradient_frame(w: u32, h: u32) -> ImageFrame {
    // Die Formel steht im Support-Modul (`fixtures::gradient_pixels`), damit
    // der Wegwerf-Validierungslauf gegen `cb2da59` und der Pixel-Delta-Anker
    // exakt dieselben Fixture-Bytes benutzen.
    let (pixels, width, height) = gradient_pixels(w, h);
    ImageFrame::new(width, height, pixels).expect("gradient fixture")
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
// der alten f32-Fassung an den gepinnten Gradienten-Fixtures — siehe
// Modul-Doku ).
// ---------------------------------------------------------------------------

#[test]
fn downscale_long_edge_byte_anchor_frozen() {
    // Die Pin-Werte stehen in `LONG_EDGE_ANCHOR_PINS` (Support-Modul) — eine
    // Quelle fuer den Byte-Anker und die Kontrollgruppe des Pixel-Delta-
    // Ankers.
    for pin in LONG_EDGE_ANCHOR_PINS {
        let source = gradient_frame(pin.width, pin.height);
        let out = downscale(&source, DownscaleTarget::LongEdge(pin.max_dim)).unwrap();
        assert_bytes(pin.tag, &out, pin.dims, pin.sha256);
    }
}

// ---------------------------------------------------------------------------
// T2 — Genau ein Resampler ( Quelltext-Gate ): die drei Altnamen existieren
// nicht mehr; `ImageFrame::downscale` ist nur noch die dokumentierte
// Delegation.
//
// Das Gate selbst liegt in `downscale_anchor/resampler_gate.rs`
// (RESAMPLER-GATE-SCOPE-78): es ist **workspaceweit** (alle Crates, `src/`,
// `tests/`, `benches/`, `examples/`, ohne `target/` und `.git/`) und benennt
// die Frame-vs-Masken-Plane-Grenze ausdruecklich. Hier steht nur noch der
// Verweis, damit die Dateigroessen-Schranke des Testkroots gehalten wird.
// ---------------------------------------------------------------------------

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
