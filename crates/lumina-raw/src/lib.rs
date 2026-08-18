//! LibRaw-backed RAW decoding. The native backend is deliberately absent from WASM.

use lumina_core::ImageFrame;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DemosaicMethod {
    #[default]
    LibRawDefault,
    Linear,
    Vng,
    Ppg,
    Ahd,
    Dcb,
    Dht,
    Aahd,
}

impl DemosaicMethod {
    #[cfg(not(target_arch = "wasm32"))]
    fn libraw_value(self) -> Option<i32> {
        match self {
            Self::LibRawDefault => None,
            Self::Linear => Some(1),
            Self::Vng => Some(2),
            Self::Ppg => Some(3),
            Self::Ahd => Some(4),
            Self::Dcb => Some(11),
            Self::Dht => Some(12),
            Self::Aahd => Some(13),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawDecodeOptions {
    pub demosaicing: DemosaicMethod,
    pub output_bits: u8,
}

impl Default for RawDecodeOptions {
    fn default() -> Self {
        Self {
            demosaicing: DemosaicMethod::LibRawDefault,
            output_bits: 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawMetadata {
    /// Visible output geometry; this is not necessarily the pre-orientation buffer geometry.
    pub width: u32,
    /// Visible output geometry; this is not necessarily the pre-orientation buffer geometry.
    pub height: u32,
    pub orientation: u8,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub iso: Option<f32>,
    pub shutter: Option<f32>,
    pub aperture: Option<f32>,
    pub lens: Option<String>,
    pub focal_length: Option<f32>,
    pub timestamp: Option<i64>,
    pub artist: Option<String>,
    pub description: Option<String>,
    pub camera_matrix: [[f32; 4]; 3],
    pub camera_white_balance: [f32; 4],
    pub pre_multipliers: [f32; 4],
    pub icc_profile: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawImage {
    pub frame: ImageFrame,
    pub metadata: RawMetadata,
}

#[derive(Debug, Error)]
pub enum RawError {
    #[error("RAW decoding is not available on this platform (WASM/browser)")]
    UnsupportedPlatform,
    #[error("could not read RAW file `{path}`: {message}")]
    Io { path: String, message: String },
    #[error("LibRaw {operation} failed ({code}): {message}")]
    LibRaw {
        operation: &'static str,
        code: i32,
        message: String,
    },
    #[error("LibRaw returned an invalid {0}")]
    InvalidData(&'static str),
    #[error("RAW input name is required for byte decoding")]
    MissingName,
}

pub fn decode_file(path: impl AsRef<std::path::Path>) -> Result<RawImage, RawError> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        return Err(RawError::UnsupportedPlatform);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|error| RawError::Io {
            path: path.display().to_string(),
            message: error.to_string(),
        })?;
        let name = path
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or(RawError::MissingName)?;
        decode_bytes(&bytes, name)
    }
}

pub fn decode_bytes(bytes: &[u8], name: impl AsRef<str>) -> Result<RawImage, RawError> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (bytes, name);
        return Err(RawError::UnsupportedPlatform);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        native::decode_bytes_with_options(bytes, name.as_ref(), &RawDecodeOptions::default())
    }
}

pub fn decode_bytes_with_options(
    bytes: &[u8],
    name: impl AsRef<str>,
    options: &RawDecodeOptions,
) -> Result<RawImage, RawError> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (bytes, name, options);
        return Err(RawError::UnsupportedPlatform);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        native::decode_bytes_with_options(bytes, name.as_ref(), options)
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use libraw_sys as raw;
    use std::ffi::CStr;
    use std::os::raw::c_void;

    struct Handle(*mut raw::libraw_data_t);

    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { raw::libraw_close(self.0) };
            }
        }
    }

    struct Processed(*mut raw::libraw_processed_image_t);

    impl Drop for Processed {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { raw::libraw_dcraw_clear_mem(self.0) };
            }
        }
    }

    fn error(operation: &'static str, code: i32) -> RawError {
        let message = unsafe {
            CStr::from_ptr(raw::libraw_strerror(code))
                .to_string_lossy()
                .into_owned()
        };
        RawError::LibRaw {
            operation,
            code,
            message,
        }
    }

    fn text(value: &[std::os::raw::c_char]) -> Option<String> {
        let bytes: Vec<u8> = value.iter().map(|byte| *byte as u8).collect();
        let end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(bytes.len());
        let value = String::from_utf8_lossy(&bytes[..end]).trim().to_owned();
        (!value.is_empty()).then_some(value)
    }

    fn positive(value: f32) -> Option<f32> {
        value
            .is_finite()
            .then_some(value)
            .filter(|value| *value > 0.0)
    }

    pub fn decode_bytes_with_options(
        bytes: &[u8],
        name: &str,
        options: &RawDecodeOptions,
    ) -> Result<RawImage, RawError> {
        if bytes.is_empty() {
            return Err(RawError::InvalidData("empty input"));
        }
        if !matches!(options.output_bits, 8 | 16) {
            return Err(RawError::InvalidData("output bit depth"));
        }
        let handle = Handle(unsafe { raw::libraw_init(raw::LIBRAW_OPTIONS_NONE) });
        if handle.0.is_null() {
            return Err(RawError::InvalidData("LibRaw handle"));
        }
        let mut input = bytes.to_vec();
        let code = unsafe {
            raw::libraw_open_buffer(handle.0, input.as_mut_ptr().cast::<c_void>(), input.len())
        };
        if code != raw::LIBRAW_SUCCESS {
            return Err(error("opening input", code));
        }
        let data = unsafe { &*handle.0 };
        let orientation = match data.sizes.flip {
            1..=8 => data.sizes.flip as u8,
            _ => 1,
        };
        let camera_matrix = data.color.rgb_cam;
        let camera_white_balance = data.color.cam_mul;
        let pre_multipliers = data.color.pre_mul;
        let icc_profile = if data.color.profile.is_null() || data.color.profile_length == 0 {
            None
        } else {
            Some(unsafe {
                std::slice::from_raw_parts(
                    data.color.profile.cast::<u8>(),
                    data.color.profile_length as usize,
                )
                .to_vec()
            })
        };
        let metadata = RawMetadata {
            width: 0,
            height: 0,
            orientation,
            camera_make: text(&data.idata.make),
            camera_model: text(&data.idata.model),
            iso: positive(data.other.iso_speed),
            shutter: positive(data.other.shutter),
            aperture: positive(data.other.aperture),
            lens: None,
            focal_length: positive(data.other.focal_len),
            timestamp: (data.other.timestamp != 0).then_some(data.other.timestamp as i64),
            artist: text(&data.other.artist),
            description: text(&data.other.desc),
            camera_matrix,
            camera_white_balance,
            pre_multipliers,
            icc_profile,
        };
        unsafe {
            (*handle.0).params.user_flip = 0;
            if let Some(value) = options.demosaicing.libraw_value() {
                raw::libraw_set_demosaic(handle.0, value);
            }
            (*handle.0).params.use_camera_wb = 1;
            (*handle.0).params.use_camera_matrix = 1;
            raw::libraw_set_output_bps(handle.0, options.output_bits as i32);
            raw::libraw_set_output_color(handle.0, 1);
            raw::libraw_set_no_auto_bright(handle.0, 0);
        }
        let code = unsafe { raw::libraw_unpack(handle.0) };
        if code != raw::LIBRAW_SUCCESS {
            return Err(error("unpacking input", code));
        }
        let code = unsafe { raw::libraw_dcraw_process(handle.0) };
        if code != raw::LIBRAW_SUCCESS {
            return Err(error("processing input", code));
        }
        let mut image_error = raw::LIBRAW_SUCCESS;
        let processed =
            Processed(unsafe { raw::libraw_dcraw_make_mem_image(handle.0, &mut image_error) });
        if processed.0.is_null() {
            return Err(error("creating processed image", image_error));
        }
        let image = unsafe { &*processed.0 };
        if !(image.bits == 8 || image.bits == 16) || !(image.colors == 3 || image.colors == 4) {
            return Err(RawError::InvalidData("RGB processed image"));
        }
        if image.width == 0 || image.height == 0 {
            return Err(RawError::InvalidData("image dimensions"));
        }
        let frame = if image.bits == 8 {
            let length = (image.width as usize)
                .checked_mul(image.height as usize)
                .and_then(|value| value.checked_mul(image.colors as usize))
                .ok_or(RawError::InvalidData("image data length"))?;
            if (image.data_size as usize) < length {
                return Err(RawError::InvalidData("image data size"));
            }
            let source = unsafe { std::slice::from_raw_parts(image.data.as_ptr(), length) };
            orient(
                source,
                image.width as u32,
                image.height as u32,
                image.colors as usize,
                1,
            )?
        } else {
            let length = (image.width as usize)
                .checked_mul(image.height as usize)
                .and_then(|value| value.checked_mul(image.colors as usize))
                .ok_or(RawError::InvalidData("image data length"))?;
            if (image.data_size as usize) < length.saturating_mul(2) {
                return Err(RawError::InvalidData("image data size"));
            }
            let source =
                unsafe { std::slice::from_raw_parts(image.data.as_ptr().cast::<u16>(), length) };
            orient_16(
                source,
                image.width as u32,
                image.height as u32,
                image.colors as usize,
                1,
            )?
        };
        let mut metadata = metadata;
        metadata.width = frame.width;
        metadata.height = frame.height;
        let _ = name;
        Ok(RawImage { frame, metadata })
    }

    fn orient_16(
        source: &[u16],
        width: u32,
        height: u32,
        channels: usize,
        orientation: u8,
    ) -> Result<ImageFrame, RawError> {
        let source_8: Vec<u8> = source.iter().map(|value| (value >> 8) as u8).collect();
        orient(&source_8, width, height, channels, orientation)
    }

    fn orient(
        source: &[u8],
        width: u32,
        height: u32,
        channels: usize,
        orientation: u8,
    ) -> Result<ImageFrame, RawError> {
        let (out_width, out_height) = if (5..=8).contains(&orientation) {
            (height, width)
        } else {
            (width, height)
        };
        let mut pixels = vec![0; out_width as usize * out_height as usize * 4];
        for y in 0..out_height {
            for x in 0..out_width {
                let (sx, sy) = match orientation {
                    2 => (width - 1 - x, y),
                    3 => (width - 1 - x, height - 1 - y),
                    4 => (x, height - 1 - y),
                    5 => (y, x),
                    6 => (y, height - 1 - x),
                    7 => (width - 1 - y, height - 1 - x),
                    8 => (width - 1 - y, x),
                    _ => (x, y),
                };
                let source_offset = (sy * width + sx) as usize * channels;
                let target_offset = (y * out_width + x) as usize * 4;
                pixels[target_offset..target_offset + 3]
                    .copy_from_slice(&source[source_offset..source_offset + 3]);
                pixels[target_offset + 3] = if channels == 4 {
                    source[source_offset + 3]
                } else {
                    255
                };
            }
        }
        ImageFrame::new(out_width, out_height, pixels)
            .map_err(|_| RawError::InvalidData("RGBA frame"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_raw_bytes_are_reported_as_decode_errors() {
        let error = decode_bytes(b"not a raw", "bad.cr2").unwrap_err();
        assert!(!matches!(error, RawError::UnsupportedPlatform));
    }

    #[test]
    fn empty_bytes_are_rejected() {
        assert!(decode_bytes(&[], "empty.nef").is_err());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "set LUMINA_RAW_FIXTURE to a licensed fixture"]
    fn optional_real_fixture_checks_decode_orientation_and_dimensions() {
        let path = std::env::var_os("LUMINA_RAW_FIXTURE")
            .expect("LUMINA_RAW_FIXTURE must point to a licensed RAW fixture");
        let image = decode_file(std::path::PathBuf::from(path)).unwrap();
        assert!(image.metadata.width > 0 && image.metadata.height > 0);
        assert!((1..=8).contains(&image.metadata.orientation));
        assert_eq!(
            (image.frame.width, image.frame.height),
            (image.metadata.width, image.metadata.height)
        );
        assert_eq!(
            image.frame.pixels.len(),
            image.frame.width as usize * image.frame.height as usize * 4
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn aircraft_landscape_fixture_has_expected_geometry_and_metadata() {
        let bytes = include_bytes!("../../../sample-data/raw/aircraft-landscape.cr3");
        let image = decode_bytes(bytes, "aircraft-landscape.cr3").unwrap();
        assert_eq!(image.metadata.orientation, 1);
        assert_eq!((image.frame.width, image.frame.height), (6032, 4024));
        assert_eq!((image.metadata.width, image.metadata.height), (6032, 4024));
        assert_eq!(image.frame.pixels.len(), 6032 * 4024 * 4);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn aircraft_portrait_fixture_applies_exif_orientation() {
        let bytes = include_bytes!("../../../sample-data/raw/aircraft-portrait.cr3");
        let image = decode_bytes(bytes, "aircraft-portrait.cr3").unwrap();
        assert_eq!(image.metadata.orientation, 5);
        assert_eq!((image.frame.width, image.frame.height), (4024, 6032));
        assert_eq!((image.metadata.width, image.metadata.height), (4024, 6032));
        assert_eq!(image.frame.pixels.len(), 4024 * 6032 * 4);
    }

    #[cfg(target_arch = "wasm32")]
    #[test]
    fn wasm_is_explicitly_unsupported() {
        assert!(matches!(
            decode_bytes(b"raw", "x.cr2"),
            Err(RawError::UnsupportedPlatform)
        ));
    }
}
