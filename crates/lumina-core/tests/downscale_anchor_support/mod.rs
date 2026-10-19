//! Support-Helfer des DOWNSCALER-UNIFY-71-Anker-Tests: SHA-256 (FIPS 180-4),
//! die geloeschte Nearest-Nachbar-Replikation und der exakte
//! Flaechenmittelwert-Referenzabstand. Kein Resampler, keine Kern-Logik —
//! nur Mess- und Hash-Hilfsmittel, damit der Anker unter der
//! Dateigroessen-Schranke bleibt.

// ---------------------------------------------------------------------------
// Minimal SHA-256 ( FIPS 180-4 ) — test-only, keine neue Abhaengigkeit.
// ---------------------------------------------------------------------------

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn sha256_block(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for i in 0..16 {
        w[i] = u32::from_be_bytes([
            block[i * 4],
            block[i * 4 + 1],
            block[i * 4 + 2],
            block[i * 4 + 3],
        ]);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h) = (
        state[0], state[1], state[2], state[3], state[4], state[5], state[6], state[7],
    );
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let temp1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(SHA256_K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }
    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

/// SHA-256 als hex; zwei FIPS-Testvektoren belegen die Korrektheit.
#[must_use]
pub fn sha256(bytes: &[u8]) -> String {
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let total_len = bytes.len() as u64;
    let n_full = bytes.len() / 64;
    for i in 0..n_full {
        let mut block = [0u8; 64];
        block.copy_from_slice(&bytes[i * 64..(i + 1) * 64]);
        sha256_block(&mut state, &block);
    }
    // Final block: data remainder + 0x80 + zeros + 64-bit big-endian bit
    // length, spanning one or two blocks (length needs bytes 56..64).
    let rest = &bytes[n_full * 64..];
    let mut block = [0u8; 64];
    block[..rest.len()].copy_from_slice(rest);
    block[rest.len()] = 0x80;
    if rest.len() < 56 {
        block[56..64].copy_from_slice(&(total_len * 8).to_be_bytes());
        sha256_block(&mut state, &block);
    } else {
        sha256_block(&mut state, &block);
        let mut block2 = [0u8; 64];
        block2[56..64].copy_from_slice(&(total_len * 8).to_be_bytes());
        sha256_block(&mut state, &block2);
    }
    state
        .iter()
        .map(|word| format!("{word:08x}"))
        .collect::<Vec<_>>()
        .concat()
}

// ---------------------------------------------------------------------------
// Geloeschte Nearest-Nachbar-Replikation ( `filmstrip::downscale_rgba` ) und
// exakter Flaechenmittelwert-Referenzabstand.
// ---------------------------------------------------------------------------

/// Replikation der geloeschten `filmstrip::downscale_rgba` ( Nearest ), damit
/// der Nearest -> Bilinear-Qualitaetsbeweis dauerhaft reproduzierbar bleibt.
/// Byte-identisch gegen die reale Funktion an HEAD cb2da59 validiert.
#[must_use]
pub fn nearest_replication(
    pixels: &[u8],
    width: u32,
    height: u32,
    max_dim: u32,
) -> (Vec<u8>, u32, u32) {
    if width == 0 || height == 0 {
        return (pixels.to_vec(), width, height);
    }
    let longest = width.max(height);
    if longest <= max_dim {
        return (pixels.to_vec(), width, height);
    }
    let scale = max_dim as f64 / longest as f64;
    let new_width = (width as f64 * scale).max(1.0).round() as u32;
    let new_height = (height as f64 * scale).max(1.0).round() as u32;
    let mut out = vec![0u8; (new_width * new_height * 4) as usize];
    for y in 0..new_height {
        for x in 0..new_width {
            let sx = (x as f64 / new_width as f64 * width as f64) as u32;
            let sy = (y as f64 / new_height as f64 * height as f64) as u32;
            let src = ((sy * width + sx) * 4) as usize;
            let dst = ((y * new_width + x) * 4) as usize;
            out[dst..dst + 4].copy_from_slice(&pixels[src..src + 4]);
        }
    }
    (out, new_width, new_height)
}

/// Exakter Flaechenmittelwert-Referenz eines `w x h` -> `nw x nh`-Downscales, f64.
#[must_use]
pub fn area_reference(pixels: &[u8], w: u32, h: u32, nw: u32, nh: u32) -> Vec<f64> {
    let mut out = vec![0f64; nw as usize * nh as usize * 4];
    for y in 0..nh {
        let sy0 = (y as f64 * h as f64 / nh as f64).floor() as u32;
        let sy1 = ((y + 1) as f64 * h as f64 / nh as f64)
            .floor()
            .min(h as f64) as u32;
        for x in 0..nw {
            let sx0 = (x as f64 * w as f64 / nw as f64).floor() as u32;
            let sx1 = ((x + 1) as f64 * w as f64 / nw as f64)
                .floor()
                .min(w as f64) as u32;
            for c in 0..4 {
                let mut acc = 0f64;
                for sy in sy0..sy1 {
                    for sx in sx0..sx1 {
                        acc += pixels[((sy * w + sx) * 4 + c) as usize] as f64;
                    }
                }
                let area = ((sy1 - sy0) * (sx1 - sx0)) as f64;
                out[((y * nw + x) * 4 + c) as usize] = acc / area;
            }
        }
    }
    out
}

/// Mittlerer absoluter Abstand ( L1, normalisiert ) eines Ergebnisses zur Referenz.
#[must_use]
pub fn mean_l1(actual: &[u8], reference: &[f64]) -> f64 {
    actual
        .iter()
        .zip(reference.iter())
        .map(|(a, r)| (f64::from(*a) - r).abs())
        .sum::<f64>()
        / actual.len() as f64
}

#[cfg(test)]
mod tests {
    use super::sha256;

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(
            sha256(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
