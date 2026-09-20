#![cfg(feature = "onnx-rt")]

//! ONNX Runtime session plumbing shared by the face backends
//! (LRPAR-G12-FACE-ADAPTER-25 extraction; `onnx-rt` only).
//!
//! These helpers are deliberately free of backend state: they verify the
//! manifest-declared tensor names against a loaded graph, run one session with
//! the manifest's declared resolution/channel-layout/normalization and return
//! the requested outputs. Both the detector (canonical single output or the
//! twelve YuNet per-stride tensors) and the embedder (single output) use them,
//! so there is exactly one preprocessing/run/extract path.

use std::path::Path;

use lumina_core::ImageFrame;

use crate::face::yunet::{detection_head_for, FaceDetectHead, YUNET_OUTPUT_NAMES};
use crate::manifest::ModelManifest;
use crate::preprocess::{normalize_rgb_to_nchw, preprocess_image_to_model};
use crate::OnnxError;

/// One named model output: `(name, shape, data)`.
pub(super) type NamedTensor = (String, Vec<usize>, Vec<f32>);

/// Descriptive [`OnnxError::InferenceFailed`] for a manifest-declared tensor
/// name missing from the loaded graph (mirrors `ort_backend.rs`).
pub(super) fn tensor_name_error<T: AsRef<str>>(
    kind: &str,
    requested: &str,
    available: &[T],
    model_name: &str,
) -> OnnxError {
    let listed = if available.is_empty() {
        "<none>".to_owned()
    } else {
        available
            .iter()
            .map(|name| format!("`{}`", name.as_ref()))
            .collect::<Vec<_>>()
            .join(", ")
    };
    OnnxError::InferenceFailed {
        name: model_name.to_owned(),
        reason: format!(
            "the loaded ONNX graph has no {kind} tensor `{requested}` \
             (available {kind}s: {listed})"
        ),
    }
}

/// Expected detection output names for `manifest`: YuNet's twelve per-stride
/// tensors or the canonical single fused tensor.
pub(super) fn detection_output_names(manifest: &ModelManifest) -> Vec<String> {
    match detection_head_for(manifest) {
        Some(FaceDetectHead::YuNetPerStride) => YUNET_OUTPUT_NAMES
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
        None => vec![manifest.output_tensor_name.clone()],
    }
}

/// Load a session and validate its declared I/O tensor names against
/// `manifest`. Every name in `expected_outputs` must exist in the graph.
pub(super) fn load_session(
    model_path: &Path,
    manifest: &ModelManifest,
    expected_outputs: &[String],
) -> Result<ort::session::Session, OnnxError> {
    if !model_path.exists() {
        return Err(OnnxError::MissingModel {
            path: model_path.display().to_string(),
        });
    }
    let session = ort::session::Session::builder()
        .and_then(|mut builder| builder.commit_from_file(model_path))
        .map_err(|error| OnnxError::InferenceFailed {
            name: manifest.model_name.clone(),
            reason: format!("failed to load ONNX session: {error}"),
        })?;
    let input_names: Vec<&str> = session.inputs().iter().map(|io| io.name()).collect();
    if !input_names
        .iter()
        .any(|name| *name == manifest.input.tensor_name)
    {
        return Err(tensor_name_error(
            "input",
            &manifest.input.tensor_name,
            &input_names,
            &manifest.model_name,
        ));
    }
    let output_names: Vec<&str> = session.outputs().iter().map(|io| io.name()).collect();
    for expected in expected_outputs {
        if !output_names.contains(&expected.as_str()) {
            return Err(tensor_name_error(
                "output",
                expected,
                &output_names,
                &manifest.model_name,
            ));
        }
    }
    Ok(session)
}

/// Run the session for `image` and return the requested outputs as
/// `(name, shape, data)` in the requested order.
///
/// Preprocessing uses the manifest's declared resolution, channel layout,
/// tensor name and normalization; the input channels are emitted in the
/// declared layout (RGB or BGR) — never silently swapped.
pub(super) fn run_model_named(
    manifest: &ModelManifest,
    session: &mut ort::session::Session,
    image: &ImageFrame,
    output_names: &[String],
) -> Result<Vec<NamedTensor>, OnnxError> {
    let res = manifest.input.resolution;
    let interleaved = preprocess_image_to_model(image, res, manifest.input.channel_layout);
    let data = normalize_rgb_to_nchw(
        &interleaved,
        &manifest.model_name,
        &manifest.input.normalization,
    )?;
    let tensor =
        ort::value::Tensor::from_array((vec![1i64, 3, res.height as i64, res.width as i64], data))
            .map_err(|error| OnnxError::InferenceFailed {
                name: manifest.model_name.clone(),
                reason: format!("failed to build input tensor: {error}"),
            })?;
    let input_name = manifest.input.tensor_name.clone();
    let outputs = session
        .run(ort::inputs![input_name => tensor])
        .map_err(|error| OnnxError::InferenceFailed {
            name: manifest.model_name.clone(),
            reason: format!("inference failed: {error}"),
        })?;
    let available: Vec<&str> = outputs.keys().collect();
    let mut result = Vec::with_capacity(output_names.len());
    for output_name in output_names {
        let output = outputs.get(output_name.as_str()).ok_or_else(|| {
            tensor_name_error("output", output_name, &available, &manifest.model_name)
        })?;
        let (shape, raw) =
            output
                .try_extract_tensor::<f32>()
                .map_err(|error| OnnxError::InferenceFailed {
                    name: manifest.model_name.clone(),
                    reason: format!("failed to read output tensor `{output_name}`: {error}"),
                })?;
        let shape: Vec<usize> = shape.iter().map(|axis| *axis as usize).collect();
        result.push((output_name.clone(), shape, raw.to_vec()));
    }
    Ok(result)
}

/// Run one single-output session for `image`, returning the raw `f32` output
/// and its shape (as `usize` axes).
pub(super) fn run_model(
    manifest: &ModelManifest,
    session: &mut ort::session::Session,
    image: &ImageFrame,
) -> Result<(Vec<usize>, Vec<f32>), OnnxError> {
    let names = vec![manifest.output_tensor_name.clone()];
    let mut outputs = run_model_named(manifest, session, image, &names)?;
    let (_, shape, data) = outputs
        .pop()
        .expect("exactly one output name was requested");
    Ok((shape, data))
}

/// Look up one named output in the owned result of [`run_model_named`].
pub(super) fn find_output<'a>(
    outputs: &'a [NamedTensor],
    name: &str,
    model_name: &str,
) -> Result<(&'a [usize], &'a [f32]), OnnxError> {
    outputs
        .iter()
        .find(|(output, _, _)| output == name)
        .map(|(_, shape, data)| (shape.as_slice(), data.as_slice()))
        .ok_or_else(|| OnnxError::InferenceFailed {
            name: model_name.to_owned(),
            reason: format!("requested output tensor `{name}` is missing from the model result"),
        })
}
