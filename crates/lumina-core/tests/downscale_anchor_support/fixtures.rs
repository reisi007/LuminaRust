//! Deterministische Fixtures des Downscale-Ankers: die **glatte**
//! Kontrollgruppe (Gradient) und das **nicht-glatte** Fixture (harte Kanten
//! plus LCG-Textur).
//!
//! Beide Generatoren sind bewusst **rein** (sie liefern Bytes, kein
//! `ImageFrame`): der Wegwerf-Validierungslauf gegen HEAD `cb2da59` benutzt
//! damit exakt dieselben Fixture-Bytes wie der Anker-Test — eine zweite
//! Kopie der Formel waere eine stille Driftquelle.
//!
//! Beide Fixtures sind **deterministisch**: es gibt keinen Zufall, keinen
//! Seed-Draw und keine Zeitabhaengigkeit. Der Gradient entsteht aus
//! Ganzzahl-Divisionen, das Nicht-Glatt-Fixture aus einem festen
//! Linear-Congruential-Generator (Numerik-Recipes-Konstanten).

/// Der glatte Gradient — die Kontrollgruppe des Pixel-Delta-Ankers.
///
/// Identisch zum Gradienten, den die zehn angekernten LongEdge-Aufrufe
/// (acht distinkte Byte-Werte) seit DOWNSCALER-UNIFY-71 benutzen: die
/// Kanalverlaeufe sind **lineare
/// Treppen** (Ganzzahl-Division), nicht stetig. Das ist der Grund, warum
/// dieser Pfad byte-identisch bleibt: innerhalb einer Stufe haben zwei
/// Nachbarpixel denselben Wert, das Interpolationsgewicht faellt also nicht
/// auf das Ergebnis — nur an den Stufenkanten (alle ~0,4 LSB hoch) kann ein
/// Gewichtsunterschied ueberhaupt ein Byte verschieben.
///
/// Rueckgabe: `(bytes, breite, hoehe)`.
#[must_use]
pub fn gradient_pixels(w: u32, h: u32) -> (Vec<u8>, u32, u32) {
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
    (pixels, w, h)
}

/// Ein gepinnter LongEdge-Aufruf des Byte-Ankers (DOWNSCALER-UNIFY-71).
///
/// `tag` ist die Fehlermeldung, `sha256` der gepinnte Byte-Wert. Die Liste
/// ist die **eine Quelle** der Pin-Werte: der Byte-Anker
/// (`downscale_long_edge_byte_anchor_frozen`) und die Kontrollgruppe des
/// Pixel-Delta-Ankers lesen dieselbe Liste — so kann kein Pin fehlen und
/// keiner doppelt gepflegt werden.
#[derive(Debug, Clone, Copy)]
pub struct LongEdgeAnchorPin {
    /// Herkunft des Fixtures (Gradient `w x h`).
    pub width: u32,
    /// Herkunft des Fixtures (Gradient `w x h`).
    pub height: u32,
    /// Zielwert `LongEdge(max_dim)`.
    pub max_dim: u32,
    /// Erwartete Zielgeometrie.
    pub dims: (u32, u32),
    /// Gepinnter SHA-256 der Bytes.
    pub sha256: &'static str,
    /// Kurzes Label fuer die Fehlermeldung.
    pub tag: &'static str,
}

/// Die zehn gepinnten LongEdge-Aufrufe des Byte-Ankers.
///
/// Acht **distinkte** Byte-Werte: `48x64 @ 0` und `48x64 @ 64` sind derselbe
/// Clone, ebenso `320x200 @ 320` und `320x200 @ 321`.
pub const LONG_EDGE_ANCHOR_PINS: [LongEdgeAnchorPin; 10] = [
    LongEdgeAnchorPin {
        width: 48,
        height: 64,
        max_dim: 32,
        dims: (24, 32),
        sha256: "3120f9754600195d2d8e5723b8f5157b04123b1c39b308e330abeddfcf7aebd4",
        tag: "48x64 @32",
    },
    LongEdgeAnchorPin {
        width: 48,
        height: 64,
        max_dim: 0,
        dims: (48, 64),
        sha256: "5ff86acb8a435984fd3b8c8c8054eb3aab07889c69a6f055ee1d8516d4566d62",
        tag: "48x64 @0 (guard -> clone)",
    },
    LongEdgeAnchorPin {
        width: 48,
        height: 64,
        max_dim: 64,
        dims: (48, 64),
        sha256: "5ff86acb8a435984fd3b8c8c8054eb3aab07889c69a6f055ee1d8516d4566d62",
        tag: "48x64 @64 (exact fit -> clone)",
    },
    LongEdgeAnchorPin {
        width: 320,
        height: 200,
        max_dim: 160,
        dims: (160, 100),
        sha256: "829cbadcf1dffc231bf95ff379bcd4786ce972f534ee50d2b8cec157bd7ac8a6",
        tag: "320x200 @160",
    },
    LongEdgeAnchorPin {
        width: 320,
        height: 200,
        max_dim: 320,
        dims: (320, 200),
        sha256: "5e92b39da49ff1ff2889c53d25daea6cae689ed98cc138ae7ef0f49c53b13f37",
        tag: "320x200 @320 (exact fit -> clone)",
    },
    LongEdgeAnchorPin {
        width: 320,
        height: 200,
        max_dim: 321,
        dims: (320, 200),
        sha256: "5e92b39da49ff1ff2889c53d25daea6cae689ed98cc138ae7ef0f49c53b13f37",
        tag: "320x200 @321 (target > source -> clone)",
    },
    LongEdgeAnchorPin {
        width: 640,
        height: 400,
        max_dim: 200,
        dims: (200, 125),
        sha256: "d69d5e0a150755ea306b796c56a8a378c52d48a9923085a1e20ece12e2dbdd8b",
        tag: "640x400 @200 (thumbnail path)",
    },
    LongEdgeAnchorPin {
        width: 1,
        height: 100,
        max_dim: 1,
        dims: (1, 1),
        sha256: "a502826c9e4bb400b3479350945acf0a78ce737e1bf164f957ffab28aad9f6c1",
        tag: "1x100 @1 (1px edge)",
    },
    LongEdgeAnchorPin {
        width: 333,
        height: 221,
        max_dim: 100,
        dims: (100, 66),
        sha256: "72c9f88804e817a894ed53b16d1bcb3e6f9519ea51000194510a0212a25b69bf",
        tag: "333x221 @100 (knapp)",
    },
    LongEdgeAnchorPin {
        width: 1000,
        height: 2000,
        max_dim: 1999,
        dims: (1000, 1999),
        sha256: "f488a95252a19bc750270417eea99445e935f92c14a7663a6f1ff98a0b29171c",
        tag: "1000x2000 @1999 (rounds to width, shrinks height)",
    },
];

/// Das **nicht-glatte** Fixture: harte Kanten plus deterministische Textur.
///
/// Aufbau (pro Pixel `(x, y)`; zwei LCG-Woerter pro Pixel):
///
/// * **Rot** — **binaeres** LCG-Rauschen (`0` oder `255`): an **jeder**
///   Pixelgrenze eine harte Kante mit maximalem Kontrast (255 LSB).
/// * **Gruen** — graues LCG-Rauschen (`0..=255`): Textur mit
///   gleichverteilten Bruchanteilen.
/// * **Blau** — **Quadranten-Wechsel** `255`/`0` mit den Kanten bei
///   `x = w / 3` und `y = h / 2`: zwei grosse geometrische Kanten (eine
///   vertikale, eine horizontale) ueber die volle Bildhoehe/-breite.
/// * **Alpha** — konstant `255` (deckend).
///
/// **Warum das die schlimmste Klasse fuer Rundungsdifferenzen ist:**
///
/// 1. **Maximaler Nachbarkontrast.** Der Byte-Unterschied zwischen einer
///    f32- und einer f64-Rechnung entsteht aus einem **Gewichtsunterschied**
///    der bilinearen Interpolation — dieser wird mit dem Kontrast der
///    beiden Nachbarpixel **multipliziert**. Hier betraegt der Kontrast
///    255 LSB (binaeres Rauschen) bzw. ~85 LSB (graues Rauschen); der
///    Gradient hat dagegen ~0,4 LSB. Gleicher relativer Rechenfehler,
///    ~200x groessere absolute Byte-Abweichung als im Gradienten.
/// 2. **Gleichverteilte Bruchanteile.** Die Textur macht den Nachkommateil
///    des interpolierten Wertes (annaehernd) uniform auf `[0, 1)`. Damit ist
///    die Dichte der Werte in der Naehe einer `.5`-Rundungsgrenze maximal —
///    genau dort schlaegt eine Gewichtsstoerung auf das Byte durch.
/// 3. **Messung statt Behauptung.** Der gemessene Delta gegen die alte
///    f32-Fassung liegt bei diesem Fixture um Groessenordnungen ueber dem
///    des Gradienten (siehe `pixel_delta.rs`); beim Gradienten ist er
///    exakt null.
///
/// **Was die Messung zeigt und was nicht:** Die beiden **grossen**
/// geometrischen Kanten (Blau) haben in der Messung **null** abweichende
/// Bytes geliefert — der Blend ueber eine reine Stufe kreuzt die
/// `.5`-Rundungsgrenze an genau einem Punkt, und kein Abtastpunkt faellt in
/// das Stoerfenster. Der Delta stammt aus den beiden Rauschkanaeln. Beides
/// ist gemessen und wird gepinnt, nichts wird behauptet.
///
/// Rueckgabe: `(bytes, breite, hoehe)`.
#[must_use]
pub fn non_smooth_pixels(w: u32, h: u32) -> (Vec<u8>, u32, u32) {
    // Numerik-Recipes-LCG, fester Startwert -> deterministisch ohne Seed-Draw.
    let mut state: u32 = 0x2545_f491;
    let mut next = move || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        state
    };
    let x_edge = w / 3;
    let y_edge = h / 2;
    let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h {
        for x in 0..w {
            let edge_noise = next();
            let texture_noise = next();
            let red = if edge_noise >> 31 == 0 { 0 } else { 255 };
            let green = (texture_noise >> 16) as u8;
            let blue = if (x < x_edge) == (y < y_edge) { 255 } else { 0 };
            pixels.extend_from_slice(&[red, green, blue, 255]);
        }
    }
    (pixels, w, h)
}
