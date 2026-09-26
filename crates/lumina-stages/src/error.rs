//! The single error type of the shared stage editors.
//!
//! Every variant renders the same text the CLI's `CliError` rendered before the
//! extraction, so `lumina-cli` only needs a transparent `From<StageError>` shim
//! and the `lumina-cli` stderr output is unchanged. `lumina-mcp` maps the same
//! variants onto its own error model (`InvalidParams` for a rejected request,
//! `Sidecar`/`Render`/`Decode` for an I/O or domain failure) — the *loudness* is
//! the shared contract, the transport-level naming is not.

use lumina_core::CoreError;
use lumina_raw::RawError;
use lumina_sidecar::SidecarError;

/// A stage-editor failure. Every variant aborts before or instead of a write;
/// no variant is ever downgraded to an empty result.
#[derive(Debug, thiserror::Error)]
pub enum StageError {
    /// A rejected request: unknown copy, unknown field, malformed value, a
    /// contradictory flag combination or a caller-supplied path problem.
    #[error("{0}")]
    Message(String),
    /// An I/O failure, reported with the same text as the CLI's
    /// `CliError::Io` (`I/O error for \`<path>\`: <message>`).
    #[error("I/O error for `{path}`: {message}")]
    Io { path: String, message: String },
    #[error(transparent)]
    Sidecar(#[from] SidecarError),
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error(transparent)]
    Raw(#[from] RawError),
}

impl StageError {
    /// Wraps a [`std::io::Error`] the way every stage editor reports it.
    pub fn io(path: &std::path::Path, error: std::io::Error) -> Self {
        StageError::Io {
            path: path.display().to_string(),
            message: error.to_string(),
        }
    }

    /// Convenience constructor for the dominant "loud rejection" shape.
    pub fn msg(message: impl Into<String>) -> Self {
        StageError::Message(message.into())
    }
}
