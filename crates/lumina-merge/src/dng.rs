//! LRPAR-G13-MERGE-15 / MERGE-DNG-1: linear 16-bit DNG writer + re-import gate.
//!
//! SOLL: `feature/decisions/LRPAR-G13-MERGE-15.md` (§DNG-Schreibung,
//! MERGE-DNG-1-Konkretisierung 2026-09-05).
//!
//! Writer backend: `tiff` 0.11.3 (pure Rust, MIT, exact pin; no native code,
//! no capability gate, already inventoried in `THIRD-PARTY-NOTICES.md`).
//! DNG is TIFF/EP-based, so the writer emits a TIFF IFD0 with the DNG tags
//! LibRaw requires plus an unchained EXIF sub-IFD carrying the reference
//! source's exposure identity. Proven by re-import: LibRaw 0.22.2 decodes
//! the output (`packed_dng_load_raw`); a baseline TIFF *without* DNG tags is
//! rejected, so the DNG tags are mandatory, not decoration.
//!
//! LibRaw limits enforced loudly (verified against LibRaw 0.22.2
//! `src/metadata/identify.cpp`): width and height must each be within
//! 22..=64000 px, otherwise [`DngError::Unsupported`]. There is no silent
//! TIFF/PNG substitute and no silent rescale.
//!
//! Value mapping: `u16 = round(clamp(radiance / white_scale, 0, 1) * 65535)`
//! with `white_scale = max(1.0, peak radiance)`, deterministic from the
//! pixels. Highlights are never silently clipped and no `BaselineExposure`
//! tag is written, keeping the LibRaw decode predictable. Quantisation
//! error is at most 0.5/65535.
//!
//! This module performs file IO (atomic temp-file + rename) but contains no
//! RAW *decoding*: re-import goes through `lumina-raw`/LibRaw and is pinned
//! by integration tests (`tests/dng.rs`), not by a production dependency —
//! `lumina-merge` stays pure Rust with no native dependency.
//!
//! EXIF strings are written verbatim (no trimming, no transliteration); empty
//! or NUL-containing values are rejected loudly instead of being repaired.

use std::io::{Cursor, Seek, Write};
use std::path::{Path, PathBuf};

use lumina_sidecar::{MergeMode, MAX_MERGE_EXPOSURE_TIME_S, MAX_MERGE_F_NUMBER};
use thiserror::Error;
use tiff::encoder::{Ifd, Rational, SRational, TiffEncoder};
use tiff::tags::Tag;

use crate::LinearImage;

/// Minimum/maximum frame geometry accepted by the writer, mirroring LibRaw
/// 0.22.2 (`identify.cpp`: `height < 22 || width < 22 → is_raw = 0`,
/// `raw_width/raw_height > 64000 → is_raw = 0`).
pub const DNG_MIN_DIMENSION_PX: u32 = 22;
pub const DNG_MAX_DIMENSION_PX: u32 = 64_000;
/// Written DNG flavour: linear 16-bit non-mosaic (mirrors the merge-recipe
/// output contract `bits = 16, mosaic = false`).
pub const DNG_BITS_PER_SAMPLE: u16 = 16;
/// DNG version stamped into the file (also the backward version).
pub const DNG_VERSION: [u16; 4] = [1, 4, 0, 0];
/// PhotometricInterpretation for linear (demosaiced) DNG.
pub const DNG_PHOTOMETRIC_LINEAR_RAW: u16 = 34892;
/// Upper bound for the EXIF ISO tag (SHORT): larger values are rejected
/// loudly, never clamped.
pub const DNG_MAX_ISO_SHORT: u32 = 65_535;
/// Software tag stamped into every written DNG (deterministic per build).
pub const DNG_SOFTWARE: &str = concat!("LuminaRust lumina-merge/", env!("CARGO_PKG_VERSION"));

/// Linear-DNG file suffix per merge mode (see [`merge_dng_filename`]).
pub const HDR_DNG_SUFFIX: &str = "-HDR.dng";
pub const PANO_DNG_SUFFIX: &str = "-Pano.dng";

/// ColorMatrix1: sRGB (D65) to XYZ (D50), Bradford-adapted (Bruce Lindbloom),
/// scaled by 10^7 into exact integers. Maps the merge frame's linear RGB
/// (treated as sRGB primaries) into the XYZ reference LibRaw's colour
/// pipeline expects. With the neutral [`AsShotNeutral`](DNG_AS_SHOT_NEUTRAL)
/// below, grey input decodes to grey output.
const COLOR_MATRIX1_NUMERATORS: [i32; 9] = [
    4_360_747, 3_850_649, 1_430_804, //
    2_225_045, 7_168_786, 606_169, //
    139_322, 971_045, 7_141_733,
];
const COLOR_MATRIX1_DENOMINATOR: i32 = 10_000_000;
/// AsShotNeutral: identity white balance (the merge output is already
/// white-balanced scene data; no per-channel scaling on re-import).
const AS_SHOT_NEUTRAL: [(u32, u32); 3] = [(1, 1), (1, 1), (1, 1)];

/// EXIF identity carried over from the reference (first) source. Every field
/// is optional: a missing reference field omits the tag (never guessed, never
/// defaulted). Present fields are validated loudly on write.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DngExif {
    /// Top-level TIFF `Make` (ASCII).
    pub make: Option<String>,
    /// Top-level TIFF `Model` (ASCII).
    pub model: Option<String>,
    /// EXIF `LensModel` (0xA434, ASCII).
    pub lens: Option<String>,
    /// EXIF `ExposureTime` (seconds, RATIONAL). Contract mirrors
    /// `MergeExposure`: finite, within (0, 86400].
    pub exposure_time_s: Option<f64>,
    /// EXIF `FNumber` (RATIONAL). Contract mirrors `MergeExposure`: finite,
    /// within (0, 256].
    pub f_number: Option<f64>,
    /// EXIF `ISOSpeedRatings` (SHORT): 1..=65535.
    pub iso_speed: Option<u32>,
    /// EXIF `DateTimeOriginal` source: Unix timestamp (UTC). Must denote a
    /// year within 0..=9999 (EXIF `YYYY:MM:DD HH:MM:SS` cannot express more).
    pub timestamp: Option<i64>,
}

/// Loud DNG failure: `Unsupported` is a LibRaw/scope limit (never a silent
/// fallback), `Invalid` is a caller contract violation, `Io`/`Tiff` are
/// transport failures.
#[derive(Debug, Error)]
pub enum DngError {
    #[error("merge DNG unsupported: {0}")]
    Unsupported(String),
    #[error("invalid merge DNG input: {0}")]
    Invalid(String),
    #[error("could not write merge DNG `{path}`: {message}")]
    Io { path: String, message: String },
    #[error("TIFF encoding failed: {0}")]
    Tiff(String),
}

/// Deterministic merge-DNG file name from the reference source's file name:
/// `<stem>-HDR.dng` / `<stem>-Pano.dng`, where the stem is the reference
/// name without its last extension (`IMG_0001.ARW` → `IMG_0001-HDR.dng`).
///
/// The reference name must be a bare file name (no directory separators,
/// no drive colon, no `..` segments); paths are rejected loudly so the
/// caller resolves the bundle-relative location explicitly.
pub fn merge_dng_filename(reference_file_name: &str, mode: MergeMode) -> Result<String, DngError> {
    if reference_file_name.is_empty()
        || reference_file_name.contains('/')
        || reference_file_name.contains('\\')
        || reference_file_name.contains(':')
    {
        return Err(DngError::Invalid(format!(
            "merge reference name must be a bare file name, got `{reference_file_name}`"
        )));
    }
    if reference_file_name.split('/').any(|p| p == "..") || reference_file_name == ".." {
        return Err(DngError::Invalid(format!(
            "merge reference name must not contain `..`, got `{reference_file_name}`"
        )));
    }
    let stem = match reference_file_name.rsplit_once('.') {
        Some((stem, _)) => stem,
        None => reference_file_name,
    };
    if stem.is_empty() {
        return Err(DngError::Invalid(format!(
            "merge reference name has no usable stem, got `{reference_file_name}`"
        )));
    }
    let suffix = match mode {
        MergeMode::Hdr => HDR_DNG_SUFFIX,
        MergeMode::Panorama => PANO_DNG_SUFFIX,
    };
    Ok(format!("{stem}{suffix}"))
}

/// Linear `f32` frame to interleaved `u16` samples plus the applied
/// `white_scale` (`max(1.0, peak)`). Quantisation error is at most
/// 0.5/65535 per sample; values above the scale cannot occur because the
/// scale covers the peak, and `f32` division rounding past 1.0 is clamped.
pub fn linear_to_u16(image: &LinearImage) -> (Vec<u16>, f32) {
    let peak = image
        .pixels()
        .iter()
        .fold(0.0f32, |a, &v| a.max(v))
        .max(1.0);
    let samples = image
        .pixels()
        .iter()
        .map(|&v| ((v / peak).clamp(0.0, 1.0) * 65535.0).round() as u16)
        .collect();
    (samples, peak)
}

/// Formats a Unix timestamp (UTC) as EXIF `YYYY:MM:DD HH:MM:SS`
/// (Howard Hinnant's `civil_from_days`, no calendar dependency).
/// Years outside 0..=9999 are rejected loudly (not representable).
pub fn exif_timestamp_utc(timestamp: i64) -> Result<String, DngError> {
    let days = timestamp.div_euclid(86_400);
    let secs = timestamp.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days)?;
    if !(0..=9999).contains(&year) {
        return Err(DngError::Invalid(format!(
            "merge timestamp {timestamp} is outside EXIF-representable years 0..=9999"
        )));
    }
    Ok(format!(
        "{year:04}:{month:02}:{day:02} {:02}:{:02}:{:02}",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    ))
}

fn civil_from_days(days: i64) -> Result<(i64, u32, u32), DngError> {
    // Hinnant's algorithm operates on days since 0000-03-01 in a proleptic
    // Gregorian calendar; i64 arithmetic cannot overflow for i64 input days.
    let z = days
        .checked_add(719_468)
        .ok_or_else(|| DngError::Invalid("merge timestamp out of range".into()))?;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    Ok((if m <= 2 { y + 1 } else { y }, m, d))
}

/// Lossless-in-spirit `f64` to EXIF RATIONAL with an adaptive denominator
/// (10^6 down to 1): keeps up to microsecond precision while guaranteeing a
/// `u32` numerator for every value within the merge exposure contract.
/// Non-finite or non-positive values are rejected loudly.
fn f64_to_rational(value: f64, field: &'static str) -> Result<Rational, DngError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(DngError::Invalid(format!(
            "merge DNG {field} must be finite and positive, got {value}"
        )));
    }
    let mut denom = 1_000_000u64;
    loop {
        let num = (value * denom as f64).round();
        if num >= 1.0 && num <= u32::MAX as f64 {
            return Ok(Rational {
                n: num as u32,
                d: denom as u32,
            });
        }
        if denom == 1 {
            return Err(DngError::Invalid(format!(
                "merge DNG {field} {value} is not RATIONAL-representable"
            )));
        }
        denom /= 10;
    }
}

fn validate_ascii(field: &'static str, value: &str) -> Result<(), DngError> {
    if value.trim().is_empty() {
        return Err(DngError::Invalid(format!(
            "merge DNG {field} must not be empty or whitespace-only"
        )));
    }
    if value.contains('\0') {
        return Err(DngError::Invalid(format!(
            "merge DNG {field} must not contain NUL bytes"
        )));
    }
    Ok(())
}

fn validate_exif(exif: &DngExif) -> Result<(), DngError> {
    if let Some(make) = &exif.make {
        validate_ascii("make", make)?;
    }
    if let Some(model) = &exif.model {
        validate_ascii("model", model)?;
    }
    if let Some(lens) = &exif.lens {
        validate_ascii("lens", lens)?;
    }
    if let Some(t) = exif.exposure_time_s {
        if !t.is_finite() || t <= 0.0 || t > MAX_MERGE_EXPOSURE_TIME_S {
            return Err(DngError::Invalid(format!(
                "merge DNG exposure_time_s must be finite within (0, {MAX_MERGE_EXPOSURE_TIME_S}], got {t}"
            )));
        }
    }
    if let Some(f) = exif.f_number {
        if !f.is_finite() || f <= 0.0 || f > MAX_MERGE_F_NUMBER {
            return Err(DngError::Invalid(format!(
                "merge DNG f_number must be finite within (0, {MAX_MERGE_F_NUMBER}], got {f}"
            )));
        }
    }
    if let Some(iso) = exif.iso_speed {
        if iso == 0 || iso > DNG_MAX_ISO_SHORT {
            return Err(DngError::Invalid(format!(
                "merge DNG iso_speed must be within 1..={DNG_MAX_ISO_SHORT} (EXIF SHORT), got {iso}"
            )));
        }
    }
    if let Some(ts) = exif.timestamp {
        // Fail loudly now (not mid-encode): year representability.
        exif_timestamp_utc(ts)?;
    }
    Ok(())
}

fn validate_dimensions(image: &LinearImage) -> Result<(), DngError> {
    let (w, h) = (image.width(), image.height());
    if w < DNG_MIN_DIMENSION_PX || h < DNG_MIN_DIMENSION_PX {
        return Err(DngError::Unsupported(format!(
            "merge DNG needs at least {DNG_MIN_DIMENSION_PX}x{DNG_MIN_DIMENSION_PX}px for LibRaw re-import, got {w}x{h}"
        )));
    }
    if w > DNG_MAX_DIMENSION_PX || h > DNG_MAX_DIMENSION_PX {
        return Err(DngError::Unsupported(format!(
            "merge DNG exceeds the LibRaw {DNG_MAX_DIMENSION_PX}px limit, got {w}x{h}"
        )));
    }
    Ok(())
}

fn dng_tag(code: u16) -> Tag {
    Tag::from_u16_exhaustive(code)
}

fn write_exif_sub_ifd<W: Write + Seek>(
    enc: &mut TiffEncoder<W>,
    exif: &DngExif,
) -> Result<tiff::encoder::DirectoryOffset<tiff::encoder::TiffKindStandard>, DngError> {
    let mut x = enc
        .extra_directory()
        .map_err(|e| DngError::Tiff(e.to_string()))?;
    if let Some(t) = exif.exposure_time_s {
        let r = f64_to_rational(t, "exposure_time_s")?;
        x.write_tag(dng_tag(33434), r)
            .map_err(|e| DngError::Tiff(e.to_string()))?;
    }
    if let Some(f) = exif.f_number {
        let r = f64_to_rational(f, "f_number")?;
        x.write_tag(dng_tag(33437), r)
            .map_err(|e| DngError::Tiff(e.to_string()))?;
    }
    if let Some(iso) = exif.iso_speed {
        x.write_tag(dng_tag(34855), iso as u16)
            .map_err(|e| DngError::Tiff(e.to_string()))?;
    }
    if let Some(ts) = exif.timestamp {
        let text = exif_timestamp_utc(ts)?;
        x.write_tag(dng_tag(36867), text.as_str())
            .map_err(|e| DngError::Tiff(e.to_string()))?;
    }
    if let Some(lens) = &exif.lens {
        x.write_tag(dng_tag(42036), lens.as_str())
            .map_err(|e| DngError::Tiff(e.to_string()))?;
    }
    x.finish_with_offsets()
        .map_err(|e| DngError::Tiff(e.to_string()))
}

/// Encodes a merged linear frame as linear 16-bit DNG bytes (deterministic:
/// fixed tag order, fixed rational encoding, no timestamps in the bytes —
/// `DngExif::timestamp` is *source* identity, not writer wall-clock).
pub fn encode_linear_dng(
    image: &LinearImage,
    mode: MergeMode,
    exif: &DngExif,
) -> Result<Vec<u8>, DngError> {
    validate_dimensions(image)?;
    validate_exif(exif)?;
    let (w, h) = (image.width(), image.height());
    let (samples, _) = linear_to_u16(image);

    let mut buf = Cursor::new(Vec::new());
    {
        let mut enc = TiffEncoder::new(&mut buf).map_err(|e| DngError::Tiff(e.to_string()))?;
        let exif_off = write_exif_sub_ifd(&mut enc, exif)?;
        let mut d = enc
            .image_directory()
            .map_err(|e| DngError::Tiff(e.to_string()))?;
        let tag = |d: &mut tiff::encoder::DirectoryEncoder<_, _>, t: Tag, v: u32| {
            d.write_tag(t, v).map_err(|e| DngError::Tiff(e.to_string()))
        };
        tag(&mut d, Tag::ImageWidth, w)?;
        tag(&mut d, Tag::ImageLength, h)?;
        d.write_tag(Tag::BitsPerSample, [DNG_BITS_PER_SAMPLE; 3].as_slice())
            .map_err(|e| DngError::Tiff(e.to_string()))?;
        d.write_tag(Tag::Compression, 1u16)
            .map_err(|e| DngError::Tiff(e.to_string()))?;
        d.write_tag(Tag::PhotometricInterpretation, DNG_PHOTOMETRIC_LINEAR_RAW)
            .map_err(|e| DngError::Tiff(e.to_string()))?;
        d.write_tag(Tag::SamplesPerPixel, 3u16)
            .map_err(|e| DngError::Tiff(e.to_string()))?;
        d.write_tag(Tag::RowsPerStrip, h)
            .map_err(|e| DngError::Tiff(e.to_string()))?;
        d.write_tag(Tag::PlanarConfiguration, 1u16)
            .map_err(|e| DngError::Tiff(e.to_string()))?;
        d.write_tag(Tag::SampleFormat, [1u16; 3].as_slice())
            .map_err(|e| DngError::Tiff(e.to_string()))?;
        d.write_tag(Tag::Orientation, 1u16)
            .map_err(|e| DngError::Tiff(e.to_string()))?;
        if let Some(make) = &exif.make {
            d.write_tag(Tag::Make, make.as_str())
                .map_err(|e| DngError::Tiff(e.to_string()))?;
        }
        if let Some(model) = &exif.model {
            d.write_tag(Tag::Model, model.as_str())
                .map_err(|e| DngError::Tiff(e.to_string()))?;
        }
        d.write_tag(Tag::Software, DNG_SOFTWARE)
            .map_err(|e| DngError::Tiff(e.to_string()))?;
        d.write_tag(Tag::ExifDirectory, Ifd(exif_off.pointer.0 as u32))
            .map_err(|e| DngError::Tiff(e.to_string()))?;
        d.write_tag(dng_tag(50706), DNG_VERSION.as_slice())
            .map_err(|e| DngError::Tiff(e.to_string()))?;
        d.write_tag(dng_tag(50707), DNG_VERSION.as_slice())
            .map_err(|e| DngError::Tiff(e.to_string()))?;
        let unique_model = match mode {
            MergeMode::Hdr => "LuminaMerge HDR",
            MergeMode::Panorama => "LuminaMerge Pano",
        };
        d.write_tag(dng_tag(50708), unique_model)
            .map_err(|e| DngError::Tiff(e.to_string()))?;
        d.write_tag(dng_tag(50717), u16::MAX)
            .map_err(|e| DngError::Tiff(e.to_string()))?;
        let matrix: Vec<SRational> = COLOR_MATRIX1_NUMERATORS
            .iter()
            .map(|&n| SRational {
                n,
                d: COLOR_MATRIX1_DENOMINATOR,
            })
            .collect();
        d.write_tag(dng_tag(50721), matrix.as_slice())
            .map_err(|e| DngError::Tiff(e.to_string()))?;
        let neutral: Vec<Rational> = AS_SHOT_NEUTRAL
            .iter()
            .map(|&(n, d)| Rational { n, d })
            .collect();
        d.write_tag(dng_tag(50728), neutral.as_slice())
            .map_err(|e| DngError::Tiff(e.to_string()))?;
        d.write_tag(dng_tag(50778), 21u16)
            .map_err(|e| DngError::Tiff(e.to_string()))?;
        let offset = d
            .write_data(samples.as_slice())
            .map_err(|e| DngError::Tiff(e.to_string()))?;
        let offset_u32 = u32::try_from(offset).map_err(|_| {
            DngError::Unsupported("merge DNG strip offset exceeds 32-bit TIFF range".into())
        })?;
        let byte_counts = u32::try_from(samples.len() * 2).map_err(|_| {
            DngError::Unsupported("merge DNG strip exceeds 32-bit TIFF range".into())
        })?;
        d.write_tag(Tag::StripOffsets, offset_u32)
            .map_err(|e| DngError::Tiff(e.to_string()))?;
        d.write_tag(Tag::StripByteCounts, byte_counts)
            .map_err(|e| DngError::Tiff(e.to_string()))?;
        d.finish().map_err(|e| DngError::Tiff(e.to_string()))?;
    }
    Ok(buf.into_inner())
}

/// Validates a merge-DNG output file name against the portable-bundle
/// contract (relative, no separators, no drive colon, no `..`, non-empty):
/// the same rule the merge recipe enforces for `output.file`.
pub fn validate_dng_file_name(file_name: &str) -> Result<(), DngError> {
    if file_name.is_empty()
        || file_name.contains('/')
        || file_name.contains('\\')
        || file_name.contains(':')
        || file_name == "."
        || file_name == ".."
        || file_name.split('/').any(|p| p == "..")
        || file_name.split('/').any(str::is_empty)
    {
        return Err(DngError::Invalid(format!(
            "merge DNG file name must be a safe portable relative path, got `{file_name}`"
        )));
    }
    Ok(())
}

/// Atomically writes an encoded merge DNG into `dir` under `file_name`
/// (temp file in the same directory + rename; incomplete files are never
/// valid). Returns the final path. `file_name` must satisfy
/// [`validate_dng_file_name`]; absolute paths are rejected.
pub fn write_merge_dng(
    dir: &Path,
    file_name: &str,
    image: &LinearImage,
    mode: MergeMode,
    exif: &DngExif,
) -> Result<PathBuf, DngError> {
    validate_dng_file_name(file_name)?;
    let bytes = encode_linear_dng(image, mode, exif)?;
    let target = dir.join(file_name);
    let tmp_name = format!("{file_name}.tmp-{}", std::process::id());
    let tmp = dir.join(&tmp_name);
    std::fs::write(&tmp, &bytes).map_err(|e| DngError::Io {
        path: tmp.display().to_string(),
        message: e.to_string(),
    })?;
    if let Err(e) = std::fs::rename(&tmp, &target) {
        let _ = std::fs::remove_file(&tmp);
        return Err(DngError::Io {
            path: target.display().to_string(),
            message: e.to_string(),
        });
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exif_full() -> DngExif {
        DngExif {
            make: Some("LuminaProbe".into()),
            model: Some("MergeHDR-1".into()),
            lens: Some("ProbeLens 24-70".into()),
            exposure_time_s: Some(0.01),
            f_number: Some(8.0),
            iso_speed: Some(100),
            timestamp: Some(1_788_602_400),
        }
    }

    #[test]
    fn filename_rule_hdr_and_pano() {
        assert_eq!(
            merge_dng_filename("IMG_0001.ARW", MergeMode::Hdr).unwrap(),
            "IMG_0001-HDR.dng"
        );
        assert_eq!(
            merge_dng_filename("IMG_0001.ARW", MergeMode::Panorama).unwrap(),
            "IMG_0001-Pano.dng"
        );
        assert_eq!(
            merge_dng_filename("IMG_0001", MergeMode::Hdr).unwrap(),
            "IMG_0001-HDR.dng"
        );
        assert_eq!(
            merge_dng_filename("a.b.ARW", MergeMode::Hdr).unwrap(),
            "a.b-HDR.dng"
        );
    }

    #[test]
    fn filename_rule_rejects_paths_loudly() {
        for bad in [
            "",
            "/abs/IMG_0001.ARW",
            "dir/IMG_0001.ARW",
            "dir\\IMG_0001.ARW",
            "C:/IMG_0001.ARW",
            "..",
            ".ARW",
            ".",
        ] {
            assert!(
                merge_dng_filename(bad, MergeMode::Hdr).is_err(),
                "`{bad}` must be rejected"
            );
        }
        for bad in [
            "",
            "/abs/x.dng",
            "dir/x.dng",
            "C:/x.dng",
            "..",
            ".",
            "x//y.dng",
        ] {
            assert!(
                validate_dng_file_name(bad).is_err(),
                "`{bad}` must fail the file-name gate"
            );
        }
    }

    #[test]
    fn white_scale_covers_hdr_peak_without_clipping() {
        let img = LinearImage::new(22, 22, vec![2.5; 22 * 22 * 3]).unwrap();
        let (samples, scale) = linear_to_u16(&img);
        assert_eq!(scale, 2.5);
        assert!(samples.iter().all(|&s| s == 65535));
        // Unit peak maps exactly to white.
        let img = LinearImage::new(22, 22, vec![1.0; 22 * 22 * 3]).unwrap();
        let (samples, scale) = linear_to_u16(&img);
        assert_eq!(scale, 1.0);
        assert!(samples.iter().all(|&s| s == 65535));
    }

    #[test]
    fn quantisation_error_bounded() {
        // 0.25 maps to 16383.75 -> rounds to 16384: error 0.25/65535.
        let img = LinearImage::new(22, 22, vec![0.25; 22 * 22 * 3]).unwrap();
        let (samples, scale) = linear_to_u16(&img);
        assert_eq!(scale, 1.0);
        assert_eq!(samples[0], 16384);
        let back = samples[0] as f32 / 65535.0;
        assert!((back - 0.25).abs() <= 0.5 / 65535.0 + 1e-9);
    }

    #[test]
    fn timestamp_formatting_utc() {
        assert_eq!(exif_timestamp_utc(0).unwrap(), "1970:01:01 00:00:00");
        assert_eq!(exif_timestamp_utc(-1).unwrap(), "1969:12:31 23:59:59");
        assert_eq!(
            exif_timestamp_utc(1_788_602_400).unwrap(),
            "2026:09:05 10:00:00"
        );
        assert_eq!(
            exif_timestamp_utc(1_709_208_000).unwrap(),
            "2024:02:29 12:00:00"
        );
    }

    #[test]
    fn rational_adapts_denominator_for_large_values() {
        let r = f64_to_rational(8.0, "f_number").unwrap();
        assert!((r.n as f64 / r.d as f64 - 8.0).abs() < 1e-9);
        // 86400 s exceeds u32 at 1e6 denominator: must adapt, staying exact.
        let r = f64_to_rational(86_400.0, "exposure_time_s").unwrap();
        assert!((r.n as f64 / r.d as f64 - 86_400.0).abs() < 1e-6);
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(f64_to_rational(bad, "exposure_time_s").is_err());
        }
    }

    #[test]
    fn exif_validation_rejects_loudly() {
        let mut e = exif_full();
        e.iso_speed = Some(0);
        assert!(validate_exif(&e).is_err());
        let mut e = exif_full();
        e.iso_speed = Some(65_536);
        assert!(validate_exif(&e).is_err());
        let mut e = exif_full();
        e.exposure_time_s = Some(0.0);
        assert!(validate_exif(&e).is_err());
        let mut e = exif_full();
        e.f_number = Some(f64::NAN);
        assert!(validate_exif(&e).is_err());
        let mut e = exif_full();
        e.make = Some("   ".into());
        assert!(validate_exif(&e).is_err());
        let mut e = exif_full();
        e.lens = Some("a\0b".into());
        assert!(validate_exif(&e).is_err());
        assert!(validate_exif(&exif_full()).is_ok());
        assert!(validate_exif(&DngExif::default()).is_ok());
    }

    #[test]
    fn undersized_frames_are_unsupported() {
        let img = LinearImage::solid(8, 8, [0.3, 0.3, 0.3]);
        let err = encode_linear_dng(&img, MergeMode::Hdr, &DngExif::default()).unwrap_err();
        assert!(matches!(err, DngError::Unsupported(_)), "got: {err}");
        let img = LinearImage::solid(21, 64, [0.3, 0.3, 0.3]);
        assert!(matches!(
            encode_linear_dng(&img, MergeMode::Hdr, &DngExif::default()).unwrap_err(),
            DngError::Unsupported(_)
        ));
    }

    #[test]
    fn encode_is_byte_deterministic() {
        let mut px = Vec::new();
        for y in 0..24u32 {
            for x in 0..32u32 {
                let v = ((x + y * 32) % 251) as f32 / 255.0;
                px.extend_from_slice(&[v, v / 2.0, 1.0 - v]);
            }
        }
        let img = LinearImage::new(32, 24, px).unwrap();
        let a = encode_linear_dng(&img, MergeMode::Hdr, &exif_full()).unwrap();
        let b = encode_linear_dng(&img, MergeMode::Hdr, &exif_full()).unwrap();
        assert_eq!(a, b, "same inputs must encode byte-identically");
        assert!(a.len() > 32 * 24 * 3 * 2);
    }
}
