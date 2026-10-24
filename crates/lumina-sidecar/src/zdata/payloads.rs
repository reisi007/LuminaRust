use super::{
    invalid, RecordKind, ZDataError, FACE_EMBEDDING_ENCODING_VERSION, MAX_DIMENSION,
    MAX_FACE_EMBEDDING_DIMENSION, MAX_ID_LEN, MAX_UNCOMPRESSED, REPAIR_ENCODING_VERSION,
    RGBA_ENCODING_VERSION, RGB_ENCODING_VERSION,
};

/// An F-042-N1 repair-region artifact stored in the `.lumina.zdata` bundle.
///
/// `region` is a `MaskPlane`-compatible `uint16` plane (`0..=u16::MAX`) and
/// `replacement` is an RGBA8 image.  Both MUST have identical dimensions; the
/// relation is enforced on write and read.  The artifact is applied by the core
/// render path as `out = replacement` for pixels with `region >= 32768`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairRegionArtifact {
    pub id: String,
    pub width: u32,
    pub height: u32,
    /// Row-major little-endian `u16` region values, `width * height` long.
    pub region: Vec<u16>,
    /// Row-major RGBA8 replacement pixels, `width * height * 4` long.
    pub replacement: Vec<u8>,
}

impl RepairRegionArtifact {
    /// Validates id, dimensions and value/byte counts (region and replacement
    /// must have identical dimensions).
    pub fn validate(&self) -> Result<(), ZDataError> {
        if self.id.is_empty()
            || self.id.len() > MAX_ID_LEN
            || !self.id.is_char_boundary(self.id.len())
        {
            return Err(invalid("invalid repair region id"));
        }
        if self.width == 0
            || self.height == 0
            || self.width > MAX_DIMENSION
            || self.height > MAX_DIMENSION
        {
            return Err(invalid("invalid repair region dimensions"));
        }
        let count = u64::from(self.width)
            .checked_mul(u64::from(self.height))
            .ok_or_else(|| invalid("repair region dimensions overflow"))?;
        if count > MAX_UNCOMPRESSED / 6 {
            return Err(invalid("repair region exceeds payload size limit"));
        }
        if self.region.len() != count as usize {
            return Err(invalid("repair region plane does not match dimensions"));
        }
        if self.replacement.len() != count as usize * 4 {
            return Err(invalid(
                "repair region replacement does not match dimensions",
            ));
        }
        Ok(())
    }

    /// Canonical pre-compression encoding (encoding version + dimensions +
    /// region + replacement).  This is exactly what the container checksum and
    /// [`RepairRegionArtifact::checksum`] cover.
    pub(super) fn encode_raw(&self) -> Vec<u8> {
        let mut raw = Vec::with_capacity(12 + self.region.len() * 2 + self.replacement.len());
        raw.extend_from_slice(&REPAIR_ENCODING_VERSION.to_le_bytes());
        raw.extend_from_slice(&self.width.to_le_bytes());
        raw.extend_from_slice(&self.height.to_le_bytes());
        for value in &self.region {
            raw.extend_from_slice(&value.to_le_bytes());
        }
        raw.extend_from_slice(&self.replacement);
        raw
    }

    /// Reconstructs an artifact from its canonical raw payload.  Verifies the
    /// encoding version and that the payload splits exactly into `width*height`
    /// u16 region values and `width*height*4` RGBA8 replacement bytes.
    pub(super) fn decode_raw(id: String, raw: &[u8]) -> Result<Self, ZDataError> {
        if raw.len() < 12 {
            return Err(invalid("repair region payload is truncated"));
        }
        let version = u32::from_le_bytes(raw[0..4].try_into().unwrap());
        if version != REPAIR_ENCODING_VERSION {
            return Err(invalid("unsupported repair region encoding version"));
        }
        let width = u32::from_le_bytes(raw[4..8].try_into().unwrap());
        let height = u32::from_le_bytes(raw[8..12].try_into().unwrap());
        let count = width as usize * height as usize;
        let region_end = 12 + count * 2;
        if raw.len() != region_end + count * 4 {
            return Err(invalid(
                "repair region payload length does not match dimensions",
            ));
        }
        let region = raw[12..region_end]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|bytes| u16::from_le_bytes(*bytes))
            .collect();
        let replacement = raw[region_end..].to_vec();
        let artifact = RepairRegionArtifact {
            id,
            width,
            height,
            region,
            replacement,
        };
        artifact.validate()?;
        Ok(artifact)
    }

    /// BLAKE3 hex digest of the canonical raw payload.  This is the checksum
    /// stored in the recipe's `SourceActionArtifactRef.checksum` and must match
    /// the container record checksum, so recipe and bundle stay aligned.
    pub fn checksum(&self) -> String {
        blake3::hash(&self.encode_raw()).to_hex().to_string()
    }

    /// Stable kind string used by diagnostics (`repair_region`).
    pub fn kind_str(&self) -> &'static str {
        RecordKind::RepairRegion.as_str()
    }
}

/// Shared validation for RGBA8 generative payloads (canvas + spot heal):
/// non-empty portable id, non-zero dimensions within [`MAX_DIMENSION`], and
/// exactly `width*height*4` pixel bytes inside the container size budget.
fn validate_rgba_payload(
    id: &str,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<(), ZDataError> {
    if id.is_empty() || id.len() > MAX_ID_LEN || !id.is_char_boundary(id.len()) {
        return Err(invalid("invalid generative artifact id"));
    }
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(invalid("invalid generative artifact dimensions"));
    }
    let count = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| invalid("generative artifact dimensions overflow"))?;
    if count > MAX_UNCOMPRESSED / 4 {
        return Err(invalid("generative artifact exceeds payload size limit"));
    }
    if pixels.len() != count as usize * 4 {
        return Err(invalid(
            "generative artifact pixels do not match dimensions",
        ));
    }
    Ok(())
}

/// Shared canonical RGBA8 encoding (`encoding_version + width + height +
/// pixels`).  Exactly what the container checksum covers.
fn encode_rgba_raw(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(12 + pixels.len());
    raw.extend_from_slice(&RGBA_ENCODING_VERSION.to_le_bytes());
    raw.extend_from_slice(&width.to_le_bytes());
    raw.extend_from_slice(&height.to_le_bytes());
    raw.extend_from_slice(pixels);
    raw
}

/// Shared RGBA8 decoding: verifies the encoding version and that the payload
/// splits exactly into `width*height*4` bytes.
fn decode_rgba_raw(raw: &[u8]) -> Result<(u32, u32, Vec<u8>), ZDataError> {
    if raw.len() < 12 {
        return Err(invalid("generative artifact payload is truncated"));
    }
    let version = u32::from_le_bytes(raw[0..4].try_into().unwrap());
    if version != RGBA_ENCODING_VERSION {
        return Err(invalid("unsupported generative artifact encoding version"));
    }
    let width = u32::from_le_bytes(raw[4..8].try_into().unwrap());
    let height = u32::from_le_bytes(raw[8..12].try_into().unwrap());
    let count = width as usize * height as usize;
    if raw.len() != 12 + count * 4 {
        return Err(invalid(
            "generative artifact payload length does not match dimensions",
        ));
    }
    Ok((width, height, raw[12..].to_vec()))
}

/// A GEN-EXPAND-1 generative canvas stored in the `.lumina.zdata` bundle.
///
/// `pixels` is the **full composited canvas** (including unchanged source
/// pixels) as row-major RGBA8, `width*height*4` bytes.  The JSON recipe keeps
/// only a portable `ArtifactReference` (relative path, format, BLAKE3
/// checksum, resolution, channel type, data version).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerativeCanvasArtifact {
    pub id: String,
    pub width: u32,
    pub height: u32,
    /// Row-major RGBA8 canvas pixels, `width * height * 4` long.
    pub pixels: Vec<u8>,
}

impl GenerativeCanvasArtifact {
    /// Validates id, dimensions and pixel-byte count.
    pub fn validate(&self) -> Result<(), ZDataError> {
        validate_rgba_payload(&self.id, self.width, self.height, &self.pixels)
    }

    pub(super) fn encode_raw(&self) -> Vec<u8> {
        encode_rgba_raw(self.width, self.height, &self.pixels)
    }

    pub(super) fn decode_raw(id: String, raw: &[u8]) -> Result<Self, ZDataError> {
        let (width, height, pixels) = decode_rgba_raw(raw)?;
        let artifact = GenerativeCanvasArtifact {
            id,
            width,
            height,
            pixels,
        };
        artifact.validate()?;
        Ok(artifact)
    }

    /// BLAKE3 hex digest of the canonical raw payload (uncompressed RGBA8
    /// stream).  Stored in the recipe's `ArtifactReference.checksum`.
    pub fn checksum(&self) -> String {
        blake3::hash(&self.encode_raw()).to_hex().to_string()
    }

    /// Stable kind string used by diagnostics (`generative_canvas`).
    pub fn kind_str(&self) -> &'static str {
        RecordKind::GenerativeCanvas.as_str()
    }
}

/// A SPOT-REMOVE-1 generative spot-heal tile stored in the `.lumina.zdata`
/// bundle.
///
/// Unlike [`GenerativeCanvasArtifact`] this carries only the replaced spot
/// tile (no canvas expansion) as row-major RGBA8, `width*height*4` bytes.
/// Separate `kind` discriminator so canvas and spot records never mix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpotHealGenerativeArtifact {
    pub id: String,
    pub width: u32,
    pub height: u32,
    /// Row-major RGBA8 replacement pixels, `width * height * 4` long.
    pub pixels: Vec<u8>,
}

impl SpotHealGenerativeArtifact {
    /// Validates id, dimensions and pixel-byte count.
    pub fn validate(&self) -> Result<(), ZDataError> {
        validate_rgba_payload(&self.id, self.width, self.height, &self.pixels)
    }

    pub(super) fn encode_raw(&self) -> Vec<u8> {
        encode_rgba_raw(self.width, self.height, &self.pixels)
    }

    pub(super) fn decode_raw(id: String, raw: &[u8]) -> Result<Self, ZDataError> {
        let (width, height, pixels) = decode_rgba_raw(raw)?;
        let artifact = SpotHealGenerativeArtifact {
            id,
            width,
            height,
            pixels,
        };
        artifact.validate()?;
        Ok(artifact)
    }

    /// BLAKE3 hex digest of the canonical raw payload (uncompressed RGBA8
    /// stream).  Stored in the recipe's `ArtifactReference.checksum`.
    pub fn checksum(&self) -> String {
        blake3::hash(&self.encode_raw()).to_hex().to_string()
    }

    /// Stable kind string used by diagnostics (`spot_heal_generative`).
    pub fn kind_str(&self) -> &'static str {
        RecordKind::SpotHealGenerative.as_str()
    }
}

/// Shared validation for RGB8 denoise payloads: non-empty portable id,
/// non-zero dimensions within [`MAX_DIMENSION`], and exactly
/// `width*height*3` pixel bytes inside the container size budget.
fn validate_rgb_payload(
    id: &str,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<(), ZDataError> {
    if id.is_empty() || id.len() > MAX_ID_LEN || !id.is_char_boundary(id.len()) {
        return Err(invalid("invalid denoise artifact id"));
    }
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(invalid("invalid denoise artifact dimensions"));
    }
    let count = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| invalid("denoise artifact dimensions overflow"))?;
    if count > MAX_UNCOMPRESSED / 3 {
        return Err(invalid("denoise artifact exceeds payload size limit"));
    }
    if pixels.len() != count as usize * 3 {
        return Err(invalid("denoise artifact pixels do not match dimensions"));
    }
    Ok(())
}

/// Shared canonical RGB8 encoding (`encoding_version + width + height +
/// pixels`).  Exactly what the container checksum and
/// [`DenoiseRgbArtifact::checksum`] cover, and byte-identical to
/// `lumina_core::DenoiseRgbArtifact::encode_raw`.
fn encode_rgb_raw(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(12 + pixels.len());
    raw.extend_from_slice(&RGB_ENCODING_VERSION.to_le_bytes());
    raw.extend_from_slice(&width.to_le_bytes());
    raw.extend_from_slice(&height.to_le_bytes());
    raw.extend_from_slice(pixels);
    raw
}

/// Shared RGB8 decoding: verifies the encoding version and that the payload
/// splits exactly into `width*height*3` bytes.
fn decode_rgb_raw(raw: &[u8]) -> Result<(u32, u32, Vec<u8>), ZDataError> {
    if raw.len() < 12 {
        return Err(invalid("denoise artifact payload is truncated"));
    }
    let version = u32::from_le_bytes(raw[0..4].try_into().unwrap());
    if version != RGB_ENCODING_VERSION {
        return Err(invalid("unsupported denoise artifact encoding version"));
    }
    let width = u32::from_le_bytes(raw[4..8].try_into().unwrap());
    let height = u32::from_le_bytes(raw[8..12].try_into().unwrap());
    let count = width as usize * height as usize;
    if raw.len() != 12 + count * 3 {
        return Err(invalid(
            "denoise artifact payload length does not match dimensions",
        ));
    }
    Ok((width, height, raw[12..].to_vec()))
}

/// A LRPAR-G14-DENOISE-IMPL-20 KI-denoise result stored in the
/// `.lumina.zdata` bundle.
///
/// `pixels` is the full-frame denoised result as row-major **RGB8**
/// (`width*height*3` bytes, no alpha — the source alpha is preserved by the
/// render pipeline).  The JSON recipe keeps only a portable
/// `DenoiseArtifactRef` (relative path, format, BLAKE3 checksum over the
/// canonical uncompressed stream, resolution, channel type, data version).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DenoiseRgbArtifact {
    pub id: String,
    pub width: u32,
    pub height: u32,
    /// Row-major RGB8 pixels, `width * height * 3` long.
    pub pixels: Vec<u8>,
}

impl DenoiseRgbArtifact {
    /// Validates id, dimensions and pixel-byte count.
    pub fn validate(&self) -> Result<(), ZDataError> {
        validate_rgb_payload(&self.id, self.width, self.height, &self.pixels)
    }

    pub(super) fn encode_raw(&self) -> Vec<u8> {
        encode_rgb_raw(self.width, self.height, &self.pixels)
    }

    pub(super) fn decode_raw(id: String, raw: &[u8]) -> Result<Self, ZDataError> {
        let (width, height, pixels) = decode_rgb_raw(raw)?;
        let artifact = DenoiseRgbArtifact {
            id,
            width,
            height,
            pixels,
        };
        artifact.validate()?;
        Ok(artifact)
    }

    /// BLAKE3 hex digest of the canonical raw payload (uncompressed RGB8
    /// stream).  Stored in the recipe's `DenoiseArtifactRef.checksum`; equal to
    /// the core artifact's `checksum()` for the same dimensions/pixels.
    pub fn checksum(&self) -> String {
        blake3::hash(&self.encode_raw()).to_hex().to_string()
    }

    /// Stable kind string used by diagnostics (`denoise_rgb`).
    pub fn kind_str(&self) -> &'static str {
        RecordKind::DenoiseRgb.as_str()
    }
}

/// A LRPAR-G12-FACE-20 face identity vector stored in the `.lumina.zdata`
/// bundle.
///
/// `values` is one L2-normalized descriptor (`dimension` long). The JSON
/// sidecar keeps only a portable `FaceVectorRef` (relative path, format,
/// BLAKE3 checksum over the canonical uncompressed stream, dimension, channel
/// type, data version); the numbers never appear in the JSON.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceEmbeddingArtifact {
    pub id: String,
    pub dimension: u32,
    /// Row-major little-endian `f32` values, `dimension` long.
    pub values: Vec<f32>,
}

impl FaceEmbeddingArtifact {
    /// Validates the id, the dimension bounds and the value count. Non-finite
    /// values are rejected loudly: a `NaN`/`Inf` descriptor would poison every
    /// downstream distance computation.
    pub fn validate(&self) -> Result<(), ZDataError> {
        if self.id.is_empty()
            || self.id.len() > MAX_ID_LEN
            || !self.id.is_char_boundary(self.id.len())
        {
            return Err(invalid("invalid face embedding id"));
        }
        if self.dimension == 0 || self.dimension > MAX_FACE_EMBEDDING_DIMENSION {
            return Err(invalid("invalid face embedding dimension"));
        }
        if self.values.len() != self.dimension as usize {
            return Err(invalid(
                "face embedding values do not match the declared dimension",
            ));
        }
        if self.values.iter().any(|value| !value.is_finite()) {
            return Err(invalid("face embedding values must be finite"));
        }
        Ok(())
    }

    pub(super) fn encode_raw(&self) -> Vec<u8> {
        let mut raw = Vec::with_capacity(8 + self.values.len() * 4);
        raw.extend_from_slice(&FACE_EMBEDDING_ENCODING_VERSION.to_le_bytes());
        raw.extend_from_slice(&self.dimension.to_le_bytes());
        for value in &self.values {
            raw.extend_from_slice(&value.to_le_bytes());
        }
        raw
    }

    pub(super) fn decode_raw(id: String, raw: &[u8]) -> Result<Self, ZDataError> {
        if raw.len() < 8 {
            return Err(invalid("face embedding payload is truncated"));
        }
        let version = u32::from_le_bytes(raw[0..4].try_into().unwrap());
        if version != FACE_EMBEDDING_ENCODING_VERSION {
            return Err(invalid("unsupported face embedding encoding version"));
        }
        let dimension = u32::from_le_bytes(raw[4..8].try_into().unwrap());
        if raw.len() != 8 + dimension as usize * 4 {
            return Err(invalid(
                "face embedding payload length does not match the dimension",
            ));
        }
        let values = raw[8..]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| f32::from_le_bytes(*bytes))
            .collect();
        let artifact = FaceEmbeddingArtifact {
            id,
            dimension,
            values,
        };
        artifact.validate()?;
        Ok(artifact)
    }

    /// BLAKE3 hex digest of the canonical raw payload (uncompressed
    /// `encoding_version || dimension || f32` stream). Stored in the analysis'
    /// `FaceVectorRef.checksum` — the exact value the evidence helper verifies.
    pub fn checksum(&self) -> String {
        blake3::hash(&self.encode_raw()).to_hex().to_string()
    }

    /// Stable kind string used by diagnostics (`face_embedding`).
    pub fn kind_str(&self) -> &'static str {
        RecordKind::FaceEmbedding.as_str()
    }
}
