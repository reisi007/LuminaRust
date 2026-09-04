use crate::{manifest::inpaint_heal_manifest, OnnxError};
/// Deterministic generative variant seed (G-04, LRPAR-G04-REMOVE).
/// Twin of `lumina-core::generative_variant_seed` — keep the algorithms in
/// sync: `variant == 0` keeps `base` (pre-variant back-compat), otherwise a
/// SplitMix64 hash over base + variant.
pub fn variant_seed(base_seed: u64, variant: u64) -> u64 {
    if variant == 0 {
        return base_seed;
    }
    let mut z = base_seed
        .wrapping_add(0x9E3779B97F4A7C15)
        .wrapping_add(variant.wrapping_mul(0xBF58476D1CE4E5B9));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}
#[derive(Debug, Clone)]
pub struct InpaintRequest {
    pub prompt: String,
    pub negative_prompt: Option<String>,
    pub seed: u64,
    pub region: Option<[f32; 4]>,
}
pub struct StubInpaintBackend {
    pub available: bool,
}
impl Default for StubInpaintBackend {
    fn default() -> Self {
        Self { available: true }
    }
}
impl StubInpaintBackend {
    pub fn manifest() -> crate::ModelManifest {
        inpaint_heal_manifest()
    }
    pub fn heal(
        &self,
        image: &[u8],
        width: u32,
        height: u32,
        mask: &[u8],
        request: &InpaintRequest,
    ) -> Result<Vec<u8>, OnnxError> {
        if !self.available {
            return Err(OnnxError::ModelUnavailable {
                name: "inpaint-heal-xl".into(),
            });
        }
        if image.len() != width as usize * height as usize * 4 {
            return Err(OnnxError::InvalidDimensions {
                expected_width: width,
                expected_height: height,
                actual_width: width,
                actual_height: height,
            });
        }
        if mask.len() != width as usize * height as usize {
            return Err(OnnxError::InvalidDimensions {
                expected_width: width,
                expected_height: height,
                actual_width: width,
                actual_height: height,
            });
        }
        let mut out = image.to_vec();
        let mut sum = [0u64; 3];
        let mut cnt = 0u64;
        for (i, &m) in mask.iter().enumerate() {
            if m < 128 {
                let base = i * 4;
                sum[0] += image[base] as u64;
                sum[1] += image[base + 1] as u64;
                sum[2] += image[base + 2] as u64;
                cnt += 1;
            }
        }
        let mean = [
            sum[0].checked_div(cnt).map(|v| v as u8).unwrap_or(128),
            sum[1].checked_div(cnt).map(|v| v as u8).unwrap_or(128),
            sum[2].checked_div(cnt).map(|v| v as u8).unwrap_or(128),
        ];
        let mut seed_hash = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut h = DefaultHasher::new();
            request.prompt.hash(&mut h);
            request.negative_prompt.hash(&mut h);
            request.seed.hash(&mut h);
            if let Some(r) = request.region {
                for v in r {
                    v.to_bits().hash(&mut h);
                }
            }
            h.finish()
        };
        let r_off = (seed_hash & 0xFF) as i16 - 128;
        seed_hash >>= 8;
        let g_off = (seed_hash & 0xFF) as i16 - 128;
        seed_hash >>= 8;
        let b_off = (seed_hash & 0xFF) as i16 - 128;
        for (i, &m) in mask.iter().enumerate() {
            if m >= 128 {
                let base = i * 4;
                out[base] = (mean[0] as i16 + r_off / 32).clamp(0, 255) as u8;
                out[base + 1] = (mean[1] as i16 + g_off / 32).clamp(0, 255) as u8;
                out[base + 2] = (mean[2] as i16 + b_off / 32).clamp(0, 255) as u8;
            }
        }
        let manifest = Self::manifest();
        if !manifest.capabilities.inpaint_heal {
            return Err(OnnxError::UnsupportedModel {
                name: manifest.model_name,
                reason: "inpaint_heal not declared".into(),
            });
        }
        Ok(out)
    }
    /// Deterministic variant heal (G-04): re-runs [`Self::heal`] with
    /// `seed = variant_seed(request.seed, variant)`. Same variant is
    /// byte-identical, a new variant is explicitly requested — never silent.
    pub fn heal_variant(
        &self,
        image: &[u8],
        width: u32,
        height: u32,
        mask: &[u8],
        request: &InpaintRequest,
        variant: u64,
    ) -> Result<Vec<u8>, OnnxError> {
        let derived = InpaintRequest {
            prompt: request.prompt.clone(),
            negative_prompt: request.negative_prompt.clone(),
            seed: variant_seed(request.seed, variant),
            region: request.region,
        };
        self.heal(image, width, height, mask, &derived)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deterministic_heal_same_inputs_byte_identical() {
        let backend = StubInpaintBackend::default();
        let w = 4;
        let h = 4;
        let image = vec![100u8; 64];
        let mut mask2 = vec![0u8; 16];
        mask2[5] = 255;
        mask2[6] = 255;
        let req = InpaintRequest {
            prompt: "remove dust".into(),
            negative_prompt: None,
            seed: 7,
            region: Some([0.25, 0.25, 0.5, 0.5]),
        };
        let a = backend.heal(&image, w, h, &mask2, &req).unwrap();
        let b = backend.heal(&image, w, h, &mask2, &req).unwrap();
        assert_eq!(a, b);
        let req2 = InpaintRequest {
            seed: 8,
            ..req.clone()
        };
        let c = backend.heal(&image, w, h, &mask2, &req2).unwrap();
        assert_ne!(a, c);
    }
    #[test]
    fn unavailable_model_reported_visible_not_silent() {
        let backend = StubInpaintBackend { available: false };
        let image = vec![100u8; 64];
        let mask = vec![0u8; 16];
        let req = InpaintRequest {
            prompt: "".into(),
            negative_prompt: None,
            seed: 7,
            region: None,
        };
        assert!(matches!(
            backend.heal(&image, 4, 4, &mask, &req),
            Err(OnnxError::ModelUnavailable { .. })
        ));
    }
    #[test]
    fn manifest_carries_inpaint_heal_capability() {
        let m = StubInpaintBackend::manifest();
        assert!(m.capabilities.inpaint_heal);
        assert_eq!(m.input.resolution.width, 512);
    }
    #[test]
    fn variant_seed_zero_stable_and_matches_core_twin() {
        // G-04: the onnx twin must agree with the core twin on the pinned
        // vectors (kept in sync by hand; a drift fails here, loudly).
        assert_eq!(variant_seed(7, 0), 7);
        assert_eq!(
            variant_seed(7, 1),
            lumina_core::generative_variant_seed(7, 1)
        );
        assert_eq!(
            variant_seed(7, 2),
            lumina_core::generative_variant_seed(7, 2)
        );
        assert_eq!(
            variant_seed(8, 1),
            lumina_core::generative_variant_seed(8, 1)
        );
        assert_ne!(variant_seed(7, 1), 7);
    }
    #[test]
    fn heal_variant_same_variant_byte_identical_other_variant_differs() {
        let backend = StubInpaintBackend::default();
        let image = vec![100u8; 64];
        let mut mask = vec![0u8; 16];
        mask[5] = 255;
        mask[6] = 255;
        let req = InpaintRequest {
            prompt: "remove dust".into(),
            negative_prompt: None,
            seed: 7,
            region: Some([0.25, 0.25, 0.5, 0.5]),
        };
        let a = backend.heal_variant(&image, 4, 4, &mask, &req, 1).unwrap();
        let b = backend.heal_variant(&image, 4, 4, &mask, &req, 1).unwrap();
        assert_eq!(a, b);
        let c = backend.heal_variant(&image, 4, 4, &mask, &req, 2).unwrap();
        assert_ne!(a, c);
        // Variant 0 reproduces the base request exactly.
        let base = backend.heal(&image, 4, 4, &mask, &req).unwrap();
        let v0 = backend.heal_variant(&image, 4, 4, &mask, &req, 0).unwrap();
        assert_eq!(base, v0);
    }
}
