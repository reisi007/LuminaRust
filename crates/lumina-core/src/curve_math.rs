//! Shared tone-curve evaluation for the global curve stage and the
//! mask-local tone stage.
//!
//! MASK-LOCAL-P1.2a deliberately reuses this exact function for a local
//! curve: a local curve is the same mathematical function as the global one,
//! evaluated at a different place in the chain. Keeping one implementation is
//! what makes "local curves use the global kernel" provable instead of
//! aspirational — a second copy could drift by one rounding step and no test
//! would notice until a golden moved.

/// Monotone cubic Hermite (PCHIP) evaluation of `curve` at `x`.
///
/// The control points must already satisfy the sidecar curve contract
/// (2..=32 finite points, `0..=1`, strictly ascending input, `(0,0)`/`(1,1)`
/// endpoints); both the global recipe validator and the typed mask-local
/// validator enforce it before a pixel is touched.
#[must_use]
pub(crate) fn monotone_curve(curve: &[lumina_sidecar::CurvePoint], x: f32) -> f32 {
    let p = curve;
    let x = x.clamp(0.0, 1.0);
    let i = p
        .windows(2)
        .position(|w| x <= w[1].input)
        .unwrap_or(p.len() - 2);
    let (a, b) = (&p[i], &p[i + 1]);
    let h = b.input - a.input;
    let t = ((x - a.input) / h).clamp(0.0, 1.0);
    let slope = |j: usize| {
        if j == 0 {
            (p[1].output - p[0].output) / (p[1].input - p[0].input)
        } else if j + 1 == p.len() {
            (p[j].output - p[j - 1].output) / (p[j].input - p[j - 1].input)
        } else {
            (p[j + 1].output - p[j - 1].output) / (p[j + 1].input - p[j - 1].input)
        }
    };
    let m0 = slope(i);
    let m1 = slope(i + 1);
    let d = (b.output - a.output) / h;
    let (m0, m1) = if d == 0.0 {
        (0.0, 0.0)
    } else {
        let lo = 0.0f32.min(3.0 * d);
        let hi = 0.0f32.max(3.0 * d);
        (m0.clamp(lo, hi), m1.clamp(lo, hi))
    };
    let t2 = t * t;
    let t3 = t2 * t;
    ((2.0 * t3 - 3.0 * t2 + 1.0) * a.output
        + (t3 - 2.0 * t2 + t) * h * m0
        + (-2.0 * t3 + 3.0 * t2) * b.output
        + (t3 - t2) * h * m1)
        .clamp(0.0, 1.0)
}
