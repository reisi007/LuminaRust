//! CPU-side precomputed Lensfun warp/gain map (GPU-LENSFUN-PARITY-1).
//!
//! A strict Lensfun profile correction is an arbitrary per-pixel
//! destination→source coordinate function (distortion, plus per-channel TCA
//! coordinates) together with a per-pixel, per-channel vignetting gain.
//! Reimplementing Lensfun's distortion/vignetting/TCA models in WGSL is
//! explicitly out of scope; instead the CPU evaluates the corrector's
//! row-batch wrappers once per source/dimensions and stores the result in a
//! [`LensfunMap`]. The GPU (`lumina-gpu::lensfun`) then runs a plain resample
//! pass over the source with exactly the oracle's inverse-bilinear `sample`
//! semantics — no per-pixel FFI and no model re-implementation.
//!
//! The map is platform-neutral data (no Lensfun/native dependency); only
//! [`LensfunMap::from_corrector`] needs the `lensfun` feature. The GPU pass is
//! meaningless without a bound map, and the CPU reference keeps using the
//! corrector directly, so the map is never a second source of truth.

use crate::CoreError;

/// Precomputed destination→source coordinate + vignetting gain map for one
/// source at one set of dimensions (a Lensfun correction is
/// dimension-preserving, so `width`/`height` are both the source and the
/// output dimensions).
#[derive(Debug, Clone, PartialEq)]
pub struct LensfunMap {
    pub width: u32,
    pub height: u32,
    /// Green (reference) destination→source coordinates, row-major.
    pub green: Vec<[f32; 2]>,
    /// Red coordinates when the corrector carries TCA calibration, else `None`.
    pub red: Option<Vec<[f32; 2]>>,
    /// Blue coordinates when the corrector carries TCA calibration, else `None`.
    pub blue: Option<Vec<[f32; 2]>>,
    /// Per-pixel per-channel vignetting gains `(r, g, b)`, row-major.
    pub gain: Vec<[f32; 3]>,
    /// `true` when the corrector applies geometric distortion. The CPU oracle
    /// then activates its content-based default crop when no explicit crop is
    /// set (the GPU must route such recipes to the caller's CPU reference).
    pub has_distortion: bool,
}

impl LensfunMap {
    /// Build a map from explicitly supplied planes. Lengths are validated
    /// against `width * height`; a violation is a loud
    /// [`CoreError::InvalidAdjustment`] (never a silent clamp).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        width: u32,
        height: u32,
        green: Vec<[f32; 2]>,
        red: Option<Vec<[f32; 2]>>,
        blue: Option<Vec<[f32; 2]>>,
        gain: Vec<[f32; 3]>,
        has_distortion: bool,
    ) -> Result<Self, CoreError> {
        let map = Self {
            width,
            height,
            green,
            red,
            blue,
            gain,
            has_distortion,
        };
        map.validate()?;
        Ok(map)
    }

    /// Whether the map carries per-channel (TCA) coordinates.
    pub fn has_tca(&self) -> bool {
        self.red.is_some()
    }

    fn len_error(width: u32, height: u32, length: usize) -> CoreError {
        CoreError::InvalidMaskPlane {
            width,
            height,
            length,
        }
    }

    /// Validate dimensions, plane lengths, the red/blue TCA pairing and that
    /// every coordinate/gain is finite. Mirrors the loud-validation contract of
    /// the other render-context inputs (depth plane, As-Shot gains): an invalid
    /// map is rejected before any GPU state changes, never silently clamped.
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.width == 0 || self.height == 0 {
            return Err(CoreError::InvalidAdjustment {
                name: "lensfun_map.dimensions".into(),
                value: 0.0,
                minimum: 1.0,
                maximum: f64::from(u32::MAX),
            });
        }
        let expected = self.width as usize * self.height as usize;
        if self.green.len() != expected {
            return Err(Self::len_error(self.width, self.height, self.green.len()));
        }
        if self.gain.len() != expected {
            return Err(Self::len_error(self.width, self.height, self.gain.len()));
        }
        match (&self.red, &self.blue) {
            (Some(red), Some(blue)) => {
                if red.len() != expected {
                    return Err(Self::len_error(self.width, self.height, red.len()));
                }
                if blue.len() != expected {
                    return Err(Self::len_error(self.width, self.height, blue.len()));
                }
            }
            (None, None) => {}
            _ => {
                return Err(CoreError::InvalidAdjustment {
                    name: "lensfun_map.tca_coordinates".into(),
                    value: -1.0,
                    minimum: 0.0,
                    maximum: 0.0,
                });
            }
        }
        for (name, values) in [("green", Some(&self.green)), ("red", self.red.as_ref())] {
            if let Some(values) = values {
                if values
                    .iter()
                    .any(|pair| !pair[0].is_finite() || !pair[1].is_finite())
                {
                    return Err(CoreError::InvalidAdjustment {
                        name: format!("lensfun_map.{name}"),
                        value: f64::NAN,
                        minimum: -1.0e9,
                        maximum: 1.0e9,
                    });
                }
            }
        }
        if let Some(blue) = self.blue.as_ref() {
            if blue
                .iter()
                .any(|pair| !pair[0].is_finite() || !pair[1].is_finite())
            {
                return Err(CoreError::InvalidAdjustment {
                    name: "lensfun_map.blue".into(),
                    value: f64::NAN,
                    minimum: -1.0e9,
                    maximum: 1.0e9,
                });
            }
        }
        if self
            .gain
            .iter()
            .any(|rgb| rgb.iter().any(|value| !value.is_finite()))
        {
            return Err(CoreError::InvalidAdjustment {
                name: "lensfun_map.gain".into(),
                value: f64::NAN,
                minimum: 0.0,
                maximum: f64::MAX,
            });
        }
        Ok(())
    }

    /// Precompute the map from a strict Lensfun corrector at `width × height`.
    ///
    /// Uses exactly the row-batch wrappers `lumina_core::apply_lens` uses
    /// (`geometry_row` / `subpixel_row` + `apply_vignetting_row`), so the
    /// stored coordinates/gains reproduce the CPU oracle bit-for-bit. The
    /// vignetting gains come from running the batch colour pass over a row of
    /// ones: Lensfun's vignetting modification is a pure per-channel multiply,
    /// so `ones * factor == factor` exactly and the shader's
    /// `sample * gain` matches the oracle's in-place batch call.
    #[cfg(feature = "lensfun")]
    pub fn from_corrector(
        corrector: &lumina_lensfun::Corrector,
        width: u32,
        height: u32,
    ) -> Result<Self, CoreError> {
        if width == 0 || height == 0 {
            return Err(CoreError::InvalidAdjustment {
                name: "lensfun_map.dimensions".into(),
                value: 0.0,
                minimum: 1.0,
                maximum: f64::from(u32::MAX),
            });
        }
        let row = width as usize;
        let total = row * height as usize;
        let has_tca = corrector.has_tca();
        let mut green = vec![[0.0f32; 2]; total];
        let mut red = has_tca.then(|| vec![[0.0f32; 2]; total]);
        let mut blue = has_tca.then(|| vec![[0.0f32; 2]; total]);
        let mut gain = vec![[0.0f32; 3]; total];
        let mut coords = vec![(0.0f64, 0.0f64); row];
        let mut triples = vec![((0.0, 0.0), (0.0, 0.0), (0.0, 0.0)); row];
        let mut ones = vec![1.0f32; row * 3];
        for y in 0..height {
            let base = y as usize * row;
            if has_tca {
                corrector.subpixel_row(0.0, y as f64, &mut triples);
                for (i, (r, g, b)) in triples.iter().enumerate() {
                    red.as_mut().expect("tca red plane")[base + i] = [r.0 as f32, r.1 as f32];
                    green[base + i] = [g.0 as f32, g.1 as f32];
                    blue.as_mut().expect("tca blue plane")[base + i] = [b.0 as f32, b.1 as f32];
                }
            } else {
                corrector.geometry_row(0.0, y as f64, &mut coords);
                for (i, (x, y_src)) in coords.iter().enumerate() {
                    green[base + i] = [*x as f32, *y_src as f32];
                }
            }
            ones.fill(1.0);
            corrector.apply_vignetting_row(&mut ones, 0.0, y as f64);
            for i in 0..row {
                gain[base + i] = [ones[i * 3], ones[i * 3 + 1], ones[i * 3 + 2]];
            }
        }
        Self::new(
            width,
            height,
            green,
            red,
            blue,
            gain,
            corrector.has_distortion(),
        )
    }
}

#[cfg(all(test, feature = "lensfun"))]
mod tests {
    use super::*;
    use crate::{apply_lens, sample, ImageFrame, EMPTY_LENS};
    use lumina_lensfun::{Corrector, LensfunDb};

    fn write_fixture(tag: &str, with_tca: bool) -> std::path::PathBuf {
        let tca = if with_tca {
            r#"<tca model="poly3" focal="50" vr="1.005" vb="0.995"/>"#
        } else {
            "<!-- no TCA calibration -->"
        };
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<lensdatabase>
    <camera>
        <maker>Lumina Map Corp</maker>
        <model>Lumina Map Body</model>
        <mount>LuminaMapMount</mount>
        <cropfactor>1.5</cropfactor>
    </camera>
    <lens>
        <maker>Lumina Map Corp</maker>
        <model>Lumina Map 50mm f/2.8</model>
        <mount>LuminaMapMount</mount>
        <cropfactor>1.5</cropfactor>
        <calibration>
            <distortion model="ptlens" focal="50" a="0.08" b="-0.10" c="0.02"/>
            <vignetting model="pa" focal="50" aperture="2.8" distance="10" k1="-0.08" k2="-0.03" k3="-0.01"/>
            {tca}
        </calibration>
    </lens>
</lensdatabase>
"#
        );
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("lumina-map-{tag}-{}-{seq}.xml", std::process::id()));
        std::fs::write(&path, xml).expect("write fixture database");
        path
    }

    fn corrector(tag: &str, with_tca: bool, width: u32, height: u32) -> Corrector {
        let path = write_fixture(tag, with_tca);
        let db = LensfunDb::load_file(&path).expect("fixture database must load");
        let _ = std::fs::remove_file(&path);
        Corrector::for_camera(
            &db,
            "Lumina Map Corp",
            "Lumina Map Body",
            None,
            width,
            height,
            50.0,
            2.8,
            10.0,
        )
        .expect("combined-profile corrector must be built")
    }

    fn gradient_frame(w: u32, h: u32) -> ImageFrame {
        let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
        for y in 0..h {
            for x in 0..w {
                let r = (x * 255 / w.max(1)) as u8;
                let g = (y * 255 / h.max(1)) as u8;
                let b = ((x ^ y) & 0xff) as u8;
                pixels.extend_from_slice(&[r, g, b, 255]);
            }
        }
        ImageFrame::new(w, h, pixels).unwrap()
    }

    /// Applying the map with the oracle's exact inverse-bilinear `sample` (the
    /// semantics the GPU pass replicates) must reproduce the corrector's row
    /// path byte-for-byte — for the non-TCA and the TCA profile alike.
    fn render_from_map(src: &ImageFrame, map: &LensfunMap) -> ImageFrame {
        let mut out = src.clone();
        for y in 0..map.height {
            for x in 0..map.width {
                let i = (y * map.width + x) as usize;
                let [rx, ry] = map.red.as_ref().map_or(map.green[i], |r| r[i]);
                let [gx, gy] = map.green[i];
                let [bx, by] = map.blue.as_ref().map_or(map.green[i], |b| b[i]);
                let dst = i * 4;
                out.pixels[dst] = (sample(src, rx, ry, 0) * map.gain[i][0])
                    .round()
                    .clamp(0.0, 255.0) as u8;
                out.pixels[dst + 1] = (sample(src, gx, gy, 1) * map.gain[i][1])
                    .round()
                    .clamp(0.0, 255.0) as u8;
                out.pixels[dst + 2] = (sample(src, bx, by, 2) * map.gain[i][2])
                    .round()
                    .clamp(0.0, 255.0) as u8;
                out.pixels[dst + 3] = sample(src, gx, gy, 3).round().clamp(0.0, 255.0) as u8;
            }
        }
        out
    }

    #[test]
    fn map_matches_corrector_row_render_bit_identically() {
        for with_tca in [false, true] {
            let (w, h) = (120u32, 80u32);
            let corrector = corrector("bit", with_tca, w, h);
            let src = gradient_frame(w, h);
            let map = LensfunMap::from_corrector(&corrector, w, h).unwrap();
            assert_eq!(map.has_tca(), with_tca);
            assert!(map.has_distortion);
            let mut oracle = src.clone();
            apply_lens(&mut oracle, &EMPTY_LENS, Some(&corrector));
            let mapped = render_from_map(&src, &map);
            assert_eq!(
                oracle.pixels, mapped.pixels,
                "map resample must be bit-identical to the corrector row path (tca={with_tca})"
            );
        }
    }

    #[test]
    fn invalid_map_is_rejected_loudly() {
        let err = LensfunMap::new(
            4,
            4,
            vec![[0.0, 0.0]; 3],
            None,
            None,
            vec![[1.0; 3]; 16],
            false,
        )
        .unwrap_err();
        assert!(matches!(err, CoreError::InvalidMaskPlane { .. }));
        // Half TCA pairing is rejected.
        let err = LensfunMap::new(
            2,
            2,
            vec![[0.0, 0.0]; 4],
            Some(vec![[0.0, 0.0]; 4]),
            None,
            vec![[1.0; 3]; 4],
            false,
        )
        .unwrap_err();
        assert!(matches!(err, CoreError::InvalidAdjustment { .. }));
        // Non-finite coordinate is rejected.
        let mut green = vec![[0.0f32, 0.0]; 4];
        green[0][0] = f32::NAN;
        let err = LensfunMap::new(2, 2, green, None, None, vec![[1.0; 3]; 4], false).unwrap_err();
        assert!(matches!(err, CoreError::InvalidAdjustment { .. }));
    }
}
