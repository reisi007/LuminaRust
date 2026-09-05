//! Deterministic digest over merge inputs (stale detection).
//!
//! The digest covers the merge identity: schema version, mode, per-frame
//! dimensions, pixel bytes (`f32::to_le_bytes`, fixed order), relative
//! exposures and alignment parameters. Artefact metadata (output file,
//! timestamp, status text) is excluded, mirroring
//! `lumina_sidecar::merge_recipe::merge_digest`.
//!
//! Rendered as `blake3:<64 lowercase hex>`. Identical inputs give
//! identical digests (bitwise on the same build); cross-toolchain float
//! formatting never enters the digest because raw bytes are hashed.

use crate::{LinearImage, MERGE_VERSION};
use lumina_sidecar::{MergeExposure, MERGE_HASH_HEX_LEN, MERGE_HASH_PREFIX};

/// Digest over HDR merge inputs.
pub fn hdr_inputs_digest(
    frames: &[LinearImage],
    exposures: &[MergeExposure],
    shifts_px: &[(f64, f64)],
) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&MERGE_VERSION.to_le_bytes());
    hasher.update(b"hdr");
    digest_frames(&mut hasher, frames);
    for exposure in exposures {
        hasher.update(&exposure.exposure_time_s.to_le_bytes());
        hasher.update(&exposure.iso.to_le_bytes());
        hasher.update(&exposure.f_number.to_le_bytes());
    }
    for (dx, dy) in shifts_px {
        hasher.update(&dx.to_le_bytes());
        hasher.update(&dy.to_le_bytes());
    }
    render(&hasher)
}

/// Digest over panorama merge inputs.
pub fn pano_inputs_digest(
    frames: &[LinearImage],
    offsets_px: &[(i32, i32)],
    blend_width_px: u32,
    matrices: &[[f64; 9]],
) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&MERGE_VERSION.to_le_bytes());
    hasher.update(b"panorama");
    digest_frames(&mut hasher, frames);
    for (x, y) in offsets_px {
        hasher.update(&x.to_le_bytes());
        hasher.update(&y.to_le_bytes());
    }
    hasher.update(&blend_width_px.to_le_bytes());
    for matrix in matrices {
        for v in matrix.iter() {
            hasher.update(&v.to_le_bytes());
        }
    }
    render(&hasher)
}

/// Re-export name used by the crate root: digest over generic merge inputs.
///
/// Kept for API symmetry with the schema digest; HDR callers use
/// [`hdr_inputs_digest`], panorama callers [`pano_inputs_digest`].
pub fn merge_inputs_digest(frames: &[LinearImage], tag: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&MERGE_VERSION.to_le_bytes());
    hasher.update(tag.as_bytes());
    digest_frames(&mut hasher, frames);
    render(&hasher)
}

fn digest_frames(hasher: &mut blake3::Hasher, frames: &[LinearImage]) -> usize {
    hasher.update(&(frames.len() as u64).to_le_bytes());
    for frame in frames {
        hasher.update(&frame.width().to_le_bytes());
        hasher.update(&frame.height().to_le_bytes());
        for v in frame.pixels() {
            hasher.update(&v.to_le_bytes());
        }
    }
    frames.len()
}

fn render(hasher: &blake3::Hasher) -> String {
    let hex = hasher.finalize().to_hex();
    debug_assert_eq!(hex.len(), MERGE_HASH_HEX_LEN);
    format!("{MERGE_HASH_PREFIX}{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merge::relative_exposure;

    fn exposure(time: f64) -> MergeExposure {
        MergeExposure {
            exposure_time_s: time,
            iso: 100,
            f_number: 8.0,
        }
    }

    #[test]
    fn digest_deterministic_and_prefixed() {
        let frames = vec![
            LinearImage::solid(4, 4, [0.2, 0.3, 0.4]),
            LinearImage::solid(4, 4, [0.5, 0.5, 0.5]),
        ];
        let ex = vec![exposure(0.01), exposure(0.02)];
        let shifts = vec![(0.0, 0.0), (-1.0, 0.5)];
        let a = hdr_inputs_digest(&frames, &ex, &shifts);
        let b = hdr_inputs_digest(&frames, &ex, &shifts);
        assert_eq!(a, b, "same inputs -> same digest");
        assert!(a.starts_with(MERGE_HASH_PREFIX));
        assert_eq!(a.len(), MERGE_HASH_PREFIX.len() + MERGE_HASH_HEX_LEN);
        let _ = relative_exposure(&ex[0]).unwrap();
    }

    #[test]
    fn digest_changes_on_any_identity_change() {
        let frames = vec![
            LinearImage::solid(4, 4, [0.2, 0.3, 0.4]),
            LinearImage::solid(4, 4, [0.5, 0.5, 0.5]),
        ];
        let ex = vec![exposure(0.01), exposure(0.02)];
        let shifts = vec![(0.0, 0.0), (0.0, 0.0)];
        let base = hdr_inputs_digest(&frames, &ex, &shifts);
        // Pixel change.
        let mut changed_frames = frames.clone();
        changed_frames[0] = LinearImage::solid(4, 4, [0.21, 0.3, 0.4]);
        assert_ne!(hdr_inputs_digest(&changed_frames, &ex, &shifts), base);
        // Exposure change.
        let changed_ex = vec![exposure(0.01), exposure(0.04)];
        assert_ne!(hdr_inputs_digest(&frames, &changed_ex, &shifts), base);
        // Shift change.
        let changed_shifts = vec![(0.0, 0.0), (1.0, 0.0)];
        assert_ne!(hdr_inputs_digest(&frames, &ex, &changed_shifts), base);
        // Mode tag change.
        assert_ne!(
            merge_inputs_digest(&frames, "panorama"),
            merge_inputs_digest(&frames, "hdr")
        );
    }

    #[test]
    fn pano_digest_covers_offsets_and_matrices() {
        let frames = vec![LinearImage::solid(4, 4, [0.2, 0.2, 0.2])];
        let ident = [[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]];
        let base = pano_inputs_digest(&frames, &[(0, 0)], 64, &ident);
        assert_ne!(pano_inputs_digest(&frames, &[(1, 0)], 64, &ident), base);
        assert_ne!(pano_inputs_digest(&frames, &[(0, 0)], 32, &ident), base);
        let shifted_m = [[1.0, 0.0, 2.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]];
        assert_ne!(pano_inputs_digest(&frames, &[(0, 0)], 64, &shifted_m), base);
        assert_eq!(
            pano_inputs_digest(&frames, &[(0, 0)], 64, &ident),
            base,
            "deterministic"
        );
    }
}
