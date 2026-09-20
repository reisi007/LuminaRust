//! Scalar manifest types shared by [`super`] (F-080).
//!
//! Extracted verbatim from `manifest.rs` (LRPAR-G12-FACE-ADAPTER-25) so the
//! manifest module stays focused on the [`super::ModelManifest`] identity and
//! I/O contract. The public paths are unchanged through the re-exports in
//! [`super`] (`crate::manifest::ChannelLayout`, …).

use serde::{Deserialize, Serialize};

/// Channel layout of the model input tensor.
///
/// `Rgb` is the canonical order of a `lumina_core::ImageFrame` (R, G, B).
/// `Bgr` declares a graph that expects the OpenCV-style BGR order — the
/// shipped YuNet detector is trained/used that way (OpenCV Zoo
/// `FaceDetectorYN` feeds `blobFromImage` without `swapRB`). The adapter
/// therefore swaps the channels before inference; a machine with plain RGB
/// input must never silently feed the wrong order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelLayout {
    /// 3-channel RGB (no alpha) — the canonical Lumina frame order.
    Rgb,
    /// 3-channel BGR (no alpha) — OpenCV-native order, declared by YuNet.
    Bgr,
}

/// Memory layout of the model input tensor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TensorFormat {
    /// Batch, channels, height, width.
    Nchw,
    /// Batch, height, width, channels.
    Nhwc,
}

/// Model input resolution in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

/// Per-channel input normalization for the ONNX graph (REVIEW-ONNX-PREPROC-1).
///
/// Pixel value `v` (u8) is mapped to `(v / 255 - mean[c]) / std[c]` before it
/// is written into the input tensor. The normalization is part of the model
/// I/O contract and therefore lives in the manifest — backends must read it
/// from there instead of hardcoding a scheme.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputNormalization {
    /// Per-channel mean in RGB order, on the `[0, 1]` scale.
    pub mean: [f32; 3],
    /// Per-channel standard deviation in RGB order, on the `[0, 1]` scale.
    pub std: [f32; 3],
}

impl InputNormalization {
    /// ImageNet mean/std — the documented preprocessing of BiRefNet and the
    /// SAM 2.1 image encoder. This is also the serde default so manifests
    /// written before the field existed keep parsing with the correct target
    /// semantics (the previous `[0, 1]`-only behavior was reviewed as wrong,
    /// not as a compatibility requirement).
    pub const IMAGENET: InputNormalization = InputNormalization {
        mean: [0.485, 0.456, 0.406],
        std: [0.229, 0.224, 0.225],
    };

    /// Identity preprocessing: no mean shift and unit std, i.e. the model sees
    /// plain `[0, 1]`-scaled RGB. This is the documented KI-Denoise contract
    /// (`feature/decisions/LRPAR-G14-DENOISE-20.md` §4: denoisers are not
    /// ImageNet-normalized), and part of the denoise input-spec digest.
    pub const IDENTITY: InputNormalization = InputNormalization {
        mean: [0.0, 0.0, 0.0],
        std: [1.0, 1.0, 1.0],
    };

    /// Raw byte range: `(v / 255 - 0) / (1 / 255) == v`, i.e. the model sees
    /// the unnormalized `0..=255` values. This is the documented input
    /// contract of the shipped face graphs (LRPAR-G12-FACE-ADAPTER-25):
    /// SFace bakes `(x - 127.5) / 128` **inside** the graph and YuNet folds
    /// its training mean into the first convolution, so the adapter must feed
    /// raw bytes and perform **no** ImageNet preprocessing in front of them.
    /// Part of the face input-spec digest.
    pub const BYTE_RANGE: InputNormalization = InputNormalization {
        mean: [0.0, 0.0, 0.0],
        std: [1.0 / 255.0, 1.0 / 255.0, 1.0 / 255.0],
    };

    /// Default used by `#[serde(default)]`: ImageNet normalization.
    pub fn imagenet() -> Self {
        Self::IMAGENET
    }
}

impl Default for InputNormalization {
    fn default() -> Self {
        Self::IMAGENET
    }
}
