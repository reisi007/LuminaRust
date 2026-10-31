//! The one ONNX Runtime environment of the process.
//!
//! `ort` 2.0.0-rc.15 changed `Session::builder()` into `Session::builder(&env)`:
//! a session is created inside an [`Environment`], and ONNX Runtime allows
//! exactly one environment per process (`EnvironmentBuilder::build` rejects a
//! second one with "only one environment is allowed per process").
//!
//! Every loader in this crate therefore asks for the same value here instead of
//! building its own. The `OnceLock` holds the *result*, so a failure is reported
//! the same way on the second call as on the first — a failed initialisation
//! must not turn into a panic on the next model load.

use std::sync::OnceLock;

use crate::OnnxError;
use ort::environment::Environment;

/// The process-wide environment, built on first use.
static ENVIRONMENT: OnceLock<Result<Environment, String>> = OnceLock::new();

/// The ONNX Runtime environment, or the reason it could not be created.
pub(crate) fn environment() -> Result<&'static Environment, OnnxError> {
    match ENVIRONMENT.get_or_init(|| {
        ort::init()
            .with_name("lumina")
            .build()
            .map_err(|error| error.to_string())
    }) {
        Ok(environment) => Ok(environment),
        Err(reason) => Err(OnnxError::InferenceFailed {
            name: "onnxruntime".to_string(),
            reason: format!("ONNX Runtime environment could not be created: {reason}"),
        }),
    }
}
