//! DOWSCALE-ANCHOR-PIXEL-DELTA-77 — der **Pixel-Byte-Delta** zwischen der
//! alten f32-Fassung des LongEdge-Downscales (HEAD `cb2da59`) und dem
//! kanonischen f64-Pfad.
//!
//! Der Anker `downscale_long_edge_byte_anchor_frozen` pinnt die Bytes des
//! f64-Pfades an **zehn LongEdge-Aufrufen** auf Gradienten-Fixtures (acht
//! distinkte Byte-Werte); der Geometrie-Anker pinnt den
//! Zaehler 82/1.120.249. Was damit **nicht** belegt war: die Behauptung, die
//! alten f32- und die neuen f64-Bytes seien (auch) fuer **nicht-glatte**
//! Bilder identisch. Dieser Test misst das Delta und pinnt die Zahlen.
//!
//! Die alte f32-Fassung liegt als testseitige Referenz im Support-Modul
//! (`downscale_anchor_support::f32_long_edge_replication`,
//! woertlich transkribiert aus `cb2da59` und gegen die alte Implementierung
//! byte-identisch validiert).

use super::gradient_frame;
use crate::downscale_anchor_support::{
    f32_long_edge_replication, non_smooth_pixels, sha256, LONG_EDGE_ANCHOR_PINS,
};
use lumina_core::{downscale, DownscaleTarget, ImageFrame};

/// Der gemessene Byte-Delta zwischen zwei gleich langen Byte-Puffern.
///
/// Rueckgabe: `(abweichende Bytes, Gesamtbytes, max |Delta| in LSB)`.
fn byte_delta(f32_bytes: &[u8], f64_bytes: &[u8]) -> (usize, usize, u8) {
    assert_eq!(
        f32_bytes.len(),
        f64_bytes.len(),
        "die Byte-Vergleiche setzen gleiche Laenge (gleiche Geometrie) voraus"
    );
    let mut differing = 0usize;
    let mut max_delta = 0u8;
    for (a, b) in f32_bytes.iter().zip(f64_bytes.iter()) {
        let delta = a.abs_diff(*b);
        if delta != 0 {
            differing += 1;
        }
        max_delta = max_delta.max(delta);
    }
    (differing, f32_bytes.len(), max_delta)
}

/// Anteil der abweichenden Bytes in Prozent (f64, aus der Byte-Zahl).
fn percent(differing: usize, total: usize) -> f64 {
    100.0 * differing as f64 / total as f64
}

#[test]
fn long_edge_f32_to_f64_pixel_delta_is_the_measured_one() {
    // ------------------------------------------------------------------
    // 1) Das nicht-glatte Fixture: harte Kanten plus Textur.
    //    320x200 @ LongEdge(160) -> 160x100, 64.000 Bytes.
    // ------------------------------------------------------------------
    let (pixels, width, height) = non_smooth_pixels(320, 200);
    let textured = ImageFrame::new(width, height, pixels).expect("non-smooth fixture");
    let old_f32 = f32_long_edge_replication(&textured, 160);
    let new_f64 = downscale(&textured, DownscaleTarget::LongEdge(160)).unwrap();

    // Geometrie-Identitaet ist die Voraussetzung des Byte-Vergleichs: nur bei
    // gleicher Zielgeometrie sind die Puffer vergleichbar.
    assert_eq!(
        (old_f32.width(), old_f32.height()),
        (new_f64.width(), new_f64.height()),
        "f32- und f64-Fassung muessen dieselbe Zielgeometrie liefern"
    );
    assert_eq!((new_f64.width(), new_f64.height()), (160, 100));

    let (differing, total, max_delta) = byte_delta(old_f32.pixels(), new_f64.pixels());
    assert_eq!(total, 64_000, "Puffer-Groesse des nicht-glatten Fixtures");
    // Die gemessenen Zahlen (Messung vom 2026-10-19, Wegwerf-Lauf gegen
    // `cb2da59` und gegen den kanonischen f64-Pfad):
    assert!(
        differing > 0,
        "das nicht-glatte Fixture muss sensitiv sein — sonst waere der Pin vakuum"
    );
    assert_eq!(
        differing, 229,
        "abweichende Bytes f32 vs f64 beim nicht-glatten Fixture"
    );
    assert!(
        (percent(differing, total) - 0.3578125).abs() < 1e-9,
        "Byte-Anteil: {}",
        percent(differing, total)
    );
    assert_eq!(
        max_delta, 1,
        "max |Delta| in LSB beim nicht-glatten Fixture"
    );

    // ------------------------------------------------------------------
    // 2) Dasselbe Fixture auf dem Thumbnail-Pfad (640x400 @ LongEdge(200)
    //    -> 200x125, 100.000 Bytes) — die groesste gemessene Abweichung.
    // ------------------------------------------------------------------
    let (pixels, width, height) = non_smooth_pixels(640, 400);
    let textured = ImageFrame::new(width, height, pixels).expect("non-smooth fixture");
    let old_f32 = f32_long_edge_replication(&textured, 200);
    let new_f64 = downscale(&textured, DownscaleTarget::LongEdge(200)).unwrap();
    assert_eq!(
        (old_f32.width(), old_f32.height()),
        (new_f64.width(), new_f64.height()),
        "f32- und f64-Fassung muessen dieselbe Zielgeometrie liefern"
    );
    assert_eq!((new_f64.width(), new_f64.height()), (200, 125));

    let (differing, total, max_delta) = byte_delta(old_f32.pixels(), new_f64.pixels());
    assert_eq!(total, 100_000, "Puffer-Groesse des nicht-glatten Fixtures");
    assert_eq!(
        differing, 2543,
        "abweichende Bytes f32 vs f64 beim nicht-glatten Thumbnail-Pfad"
    );
    assert!(
        (percent(differing, total) - 2.543).abs() < 1e-9,
        "Byte-Anteil: {}",
        percent(differing, total)
    );
    assert_eq!(
        max_delta, 1,
        "max |Delta| in LSB beim nicht-glatten Fixture"
    );

    // ------------------------------------------------------------------
    // 3) Kontrollgruppe: der **glatte** Gradient — **alle** gepinnten
    //    LongEdge-Fixtures des Byte-Ankers. Dort sind die Bytes der alten
    //    f32- und der neuen f64-Fassung identisch; die Behauptung der
    //    Modul-Doku ist damit fuer jedes dieser Fixtures **gemessen, nicht
    //    behauptet**: die f32-Replikation traegt denselben gepinnten
    //    SHA-256 wie die f64-Fassung.
    //
    //    Die Pin-Liste ist dieselbe wie im Byte-Anker
    //    (`LONG_EDGE_ANCHOR_PINS`) — es kann kein Pin fehlen und keiner
    //    doppelt gepflegt werden. Die vier Clone-Faelle
    //    (`48x64 @ 0` / `@ 64`, `320x200 @ 320` / `@ 321`) sind
    //    Early-Returns: beide Fassungen klonen den Frame unveraendert,
    //    die Bytes sind identisch. **Kein** Fixture wird uebersprungen:
    //    die Geometrie-Identitaet wird fuer jeden Pin geprueft (sie ist
    //    die Voraussetzung des Byte-Vergleichs) — divergierte sie, wuerde
    //    der Test an dieser Stelle rot, statt still zu ueberspringen.
    // ------------------------------------------------------------------
    for pin in LONG_EDGE_ANCHOR_PINS {
        let source = gradient_frame(pin.width, pin.height);
        let old_f32 = f32_long_edge_replication(&source, pin.max_dim);
        let new_f64 = downscale(&source, DownscaleTarget::LongEdge(pin.max_dim)).unwrap();
        assert_eq!(
            (old_f32.width(), old_f32.height()),
            (new_f64.width(), new_f64.height()),
            "{}: f32- und f64-Fassung muessen dieselbe Zielgeometrie liefern",
            pin.tag
        );
        assert_eq!(
            (new_f64.width(), new_f64.height()),
            pin.dims,
            "{}: Zielgeometrie",
            pin.tag
        );
        let (differing, total, max_delta) = byte_delta(old_f32.pixels(), new_f64.pixels());
        assert_eq!(total, (new_f64.width() * new_f64.height() * 4) as usize);
        assert_eq!(
            differing, 0,
            "{}: im glatten Gradienten sind die Bytes der alten f32- und der \
             neuen f64-Fassung identisch",
            pin.tag
        );
        assert_eq!(
            max_delta, 0,
            "{}: im glatten Gradienten gibt es kein Delta",
            pin.tag
        );
        // Die alten f32-Bytes sind zugleich die gepinnten Bytes des
        // kanonischen f64-Pfades (identische SHA-256) — die Kontrollgruppe
        // belegt die Modul-Doku-Aussage fuer die gepinnten Gradienten.
        assert_eq!(
            sha256(old_f32.pixels()),
            pin.sha256,
            "{}: f32-Bytes (Kontrolle)",
            pin.tag
        );
        assert_eq!(
            sha256(new_f64.pixels()),
            pin.sha256,
            "{}: f64-Bytes (Kontrolle)",
            pin.tag
        );
    }

    // ------------------------------------------------------------------
    // 4) Abgrenzung: weicht die **Geometrie** ab, ist der Byte-Vergleich
    //    unmoeglich (unterschiedliche Puffer-Groessen). Das wird getrennt
    //    behandelt und gepinnt: 147x196 @ 2 rundet in f32 auf 2x2, in f64
    //    auf 1x2.
    // ------------------------------------------------------------------
    let (pixels, width, height) = non_smooth_pixels(147, 196);
    let divergent = ImageFrame::new(width, height, pixels).expect("non-smooth fixture");
    let old_f32 = f32_long_edge_replication(&divergent, 2);
    let new_f64 = downscale(&divergent, DownscaleTarget::LongEdge(2)).unwrap();
    assert_eq!(
        (old_f32.width(), old_f32.height()),
        (2, 2),
        "f32-Geometrie (2x2)"
    );
    assert_eq!(
        (new_f64.width(), new_f64.height()),
        (1, 2),
        "f64-Geometrie (1x2)"
    );
    assert_ne!(
        old_f32.pixels().len(),
        new_f64.pixels().len(),
        "bei abweichender Geometrie ist der Byte-Vergleich unmoeglich"
    );

    // ------------------------------------------------------------------
    // 5) Der Vertrag: der kanonische f64-Pfad weicht von der alten f32-
    //    Fassung **nie** um mehr als 1 LSB ab — die gemessene Schranke.
    // ------------------------------------------------------------------
    for (source_width, source_height, max_dim) in [(320u32, 200u32, 160u32), (640, 400, 200)] {
        let (pixels, width, height) = non_smooth_pixels(source_width, source_height);
        let source = ImageFrame::new(width, height, pixels).expect("non-smooth fixture");
        let old_f32 = f32_long_edge_replication(&source, max_dim);
        let new_f64 = downscale(&source, DownscaleTarget::LongEdge(max_dim)).unwrap();
        let (_, _, max_delta) = byte_delta(old_f32.pixels(), new_f64.pixels());
        assert!(
            max_delta <= 1,
            "f32 -> f64 darf um nicht mehr als 1 LSB abweichen, gemessen {max_delta}"
        );
    }
}
