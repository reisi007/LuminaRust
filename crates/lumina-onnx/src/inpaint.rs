#![allow(clippy::manual_checked_ops)]
use crate::{manifest::inpaint_heal_manifest, OnnxError};
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
        let mean = if cnt > 0 {
            [
                (sum[0] / cnt) as u8,
                (sum[1] / cnt) as u8,
                (sum[2] / cnt) as u8,
            ]
        } else {
            [128, 128, 128]
        };
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
}
