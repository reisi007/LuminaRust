//! Die **alte f32-Fassung** des LongEdge-Downscales als testseitige
//! Referenz — woertlich transkribiert aus HEAD `cb2da59`
//! (`crates/lumina-core/src/lib.rs:486-538`, dort die Methode
//! `ImageFrame::downscale`, die die Konsolidierung `c2c2ce1` ersetzte).
//!
//! **Herkunft und Validierung.** Die Transkription ist byte-identisch gegen
//! die alte Implementierung an HEAD `cb2da59` validiert: der Wegwerf-Lauf
//! `git show cb2da59:crates/lumina-core/src/lib.rs` extrahiert die Zeilen
//! 486-538 **woertlich** (unveraendert, bis auf die Benennung der
//! Quell-Dimensionen) und vergleicht sie Byte fuer Byte mit dieser
//! Funktion — in **elf** Fixture/Target-Kombinationen (Gradient und
//! nicht-glatt, inklusive der Early-Returns): identisch in allen elf. Der
//! Wegwerf-Code wird nicht committet; die **in-repo** Belege stehen in
//! `downscale_anchor/pixel_delta.rs` (Kontrollgruppe ueber alle gepinnten
//! LongEdge-Fixtures).
//!
//! **Was die Transkription aendert und was nicht.** Der alte Code war eine
//! Methode auf `ImageFrame` und baute das Ergebnis ueber ein Struct-Literal
//! (`ImageFrame { width, height, pixels }`). Seit FRAME-INVARIANT-SEAL-77
//! sind die Felder privat, deshalb liefert diese Funktion **Bytes** und die
//! Aufrufstelle in `mod.rs` baut den Frame ueber `ImageFrame::new`. Die
//! **Rechenarithmetik ist unveraendert**: `scale`, `width`/`height` **und**
//! die Abtastpositionen `sx`/`sy` sowie die Gewichte `fx`/`fy` rechnet die
//! alte Fassung durchgaengig in **f32**; genau daraus entsteht das
//! Byte-Delta zum kanonischen f64-Pfad.
//!
//! Die Reihenfolge der Gleitkommaoperationen ist die des Originals
//! (Zwischenwert `a * (1 - f) + b * f`, Rundung erst am Ende) — eine
//! Umstellung auf `a + (b - a) * f` waere zwar mathematisch gleichwertig,
//! aber **nicht** byte-gleich, weil sie andere Rundungsfehler erzeugt.

/// Die alte f32-Fassung des LongEdge-Downscales, byte-treu zu HEAD
/// `cb2da59`.
///
/// Rueckgabe: `(bytes, ziel-breite, ziel-hoehe)`. Der Early-Return
/// (`max_dim == 0` oder `long <= max_dim`) liefert die Eingangsbytes
/// unveraendert zurueck — genau wie die alte Fassung, die dann den Frame
/// klonte.
#[must_use]
pub fn f32_long_edge_bytes(
    pixels: &[u8],
    src_w: u32,
    src_h: u32,
    max_dim: u32,
) -> (Vec<u8>, u32, u32) {
    let long = src_w.max(src_h);
    if max_dim == 0 || long <= max_dim {
        return (pixels.to_vec(), src_w, src_h);
    }
    let scale = max_dim as f32 / long as f32;
    let new_w = (src_w as f32 * scale).round().max(1.0) as u32;
    let new_h = (src_h as f32 * scale).round().max(1.0) as u32;
    let mut out = vec![0u8; new_w as usize * new_h as usize * 4];
    for y in 0..new_h {
        let sy =
            ((y as f32 + 0.5) / new_h as f32 * src_h as f32 - 0.5).clamp(0.0, src_h as f32 - 1.0);
        let y0 = sy.floor() as u32;
        let y1 = (y0 + 1).min(src_h - 1);
        let fy = sy - y0 as f32;
        for x in 0..new_w {
            let sx = ((x as f32 + 0.5) / new_w as f32 * src_w as f32 - 0.5)
                .clamp(0.0, src_w as f32 - 1.0);
            let x0 = sx.floor() as u32;
            let x1 = (x0 + 1).min(src_w - 1);
            let fx = sx - x0 as f32;
            let at = |xx: u32, yy: u32| -> [u8; 4] {
                let base = (yy * src_w + xx) as usize * 4;
                [
                    pixels[base],
                    pixels[base + 1],
                    pixels[base + 2],
                    pixels[base + 3],
                ]
            };
            let top = at(x0, y0);
            let top_r = top[0] as f32 * (1.0 - fx) + at(x1, y0)[0] as f32 * fx;
            let top_g = top[1] as f32 * (1.0 - fx) + at(x1, y0)[1] as f32 * fx;
            let top_b = top[2] as f32 * (1.0 - fx) + at(x1, y0)[2] as f32 * fx;
            let top_a = top[3] as f32 * (1.0 - fx) + at(x1, y0)[3] as f32 * fx;
            let bot = at(x0, y1);
            let bot_r = bot[0] as f32 * (1.0 - fx) + at(x1, y1)[0] as f32 * fx;
            let bot_g = bot[1] as f32 * (1.0 - fx) + at(x1, y1)[1] as f32 * fx;
            let bot_b = bot[2] as f32 * (1.0 - fx) + at(x1, y1)[2] as f32 * fx;
            let bot_a = bot[3] as f32 * (1.0 - fx) + at(x1, y1)[3] as f32 * fx;
            let dst = (y * new_w + x) as usize * 4;
            out[dst] = (top_r * (1.0 - fy) + bot_r * fy).round() as u8;
            out[dst + 1] = (top_g * (1.0 - fy) + bot_g * fy).round() as u8;
            out[dst + 2] = (top_b * (1.0 - fy) + bot_b * fy).round() as u8;
            out[dst + 3] = (top_a * (1.0 - fy) + bot_a * fy).round() as u8;
        }
    }
    (out, new_w, new_h)
}
