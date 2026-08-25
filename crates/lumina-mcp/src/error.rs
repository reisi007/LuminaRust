//! MCP error model and JSON-RPC response construction.
//!
//! Domain errors from the LuminaRust stack are mapped onto the JSON-RPC error
//! space. Each [`McpError`] carries a stable `name()` (e.g. `FileNotFound`,
//! `InvalidAdjustment`) that is echoed both in the human-readable `message` and
//! in `error.data.error`, so agents and tests can branch on the structured
//! `data` field rather than parsing free text.

use lumina_core::CoreError;
use serde_json::{json, Value};

/// All error conditions the MCP server can surface to a client.
///
/// The integer `code()` follows JSON-RPC conventions (method/parse/params are
/// the standard `-32xxx` codes) with Lumina-specific server errors in the
/// `-32000` reservation block.
#[derive(Debug)]
pub enum McpError {
    Parse(String),
    InvalidRequest(String),
    MethodNotFound(String),
    InvalidParams(String),
    Internal(String),
    FileNotFound(String),
    UnsupportedFormat(String),
    NoImageLoaded,
    UnknownImage(String),
    UnknownCopy(String),
    InvalidAdjustment {
        name: String,
        value: f64,
        minimum: f64,
        maximum: f64,
    },
    Decode(String),
    Render(String),
    Encode(String),
    Sidecar(String),
    /// The sidecar changed on disk between `lumina_load` and a write-back
    /// (compare-and-swap miss). The session never overwrites an externally
    /// modified sidecar; the caller must re-load to pick up external changes.
    SidecarConflict(String),
}

impl McpError {
    /// JSON-RPC integer code for this error.
    pub fn code(&self) -> i64 {
        match self {
            McpError::Parse(_) => -32700,
            McpError::InvalidRequest(_) => -32600,
            McpError::MethodNotFound(_) => -32601,
            McpError::InvalidParams(_) | McpError::InvalidAdjustment { .. } => -32602,
            McpError::Internal(_) => -32603,
            McpError::FileNotFound(_) => -32001,
            McpError::UnsupportedFormat(_) => -32002,
            McpError::NoImageLoaded => -32003,
            McpError::UnknownImage(_) => -32004,
            McpError::UnknownCopy(_) => -32005,
            McpError::Decode(_) => -32006,
            McpError::Render(_) => -32007,
            McpError::Encode(_) => -32008,
            McpError::Sidecar(_) => -32009,
            McpError::SidecarConflict(_) => -32010,
        }
    }

    /// Stable, machine-readable error name (used in `data.error`).
    pub fn name(&self) -> &'static str {
        match self {
            McpError::Parse(_) => "ParseError",
            McpError::InvalidRequest(_) => "InvalidRequest",
            McpError::MethodNotFound(_) => "MethodNotFound",
            McpError::InvalidParams(_) => "InvalidParams",
            McpError::Internal(_) => "InternalError",
            McpError::FileNotFound(_) => "FileNotFound",
            McpError::UnsupportedFormat(_) => "UnsupportedFormat",
            McpError::NoImageLoaded => "NoImageLoaded",
            McpError::UnknownImage(_) => "UnknownImage",
            McpError::UnknownCopy(_) => "UnknownCopy",
            McpError::InvalidAdjustment { .. } => "InvalidAdjustment",
            McpError::Decode(_) => "DecodeError",
            McpError::Render(_) => "RenderError",
            McpError::Encode(_) => "EncodeError",
            McpError::Sidecar(_) => "SidecarError",
            McpError::SidecarConflict(_) => "SidecarConflict",
        }
    }

    /// Human-readable message that embeds the stable name.
    pub fn message(&self) -> String {
        match self {
            McpError::InvalidAdjustment {
                name,
                value,
                minimum,
                maximum,
            } => format!(
                "{}: invalid adjustment `{}` = {} (must be finite and in {}..={})",
                self.name(),
                name,
                value,
                minimum,
                maximum
            ),
            McpError::FileNotFound(m) => format!("{}: file not found: {}", self.name(), m),
            McpError::UnsupportedFormat(m) => {
                format!("{}: unsupported format `{}`", self.name(), m)
            }
            McpError::NoImageLoaded => format!("{}: no image is loaded", self.name()),
            McpError::UnknownImage(m) => format!("{}: unknown image_id `{}`", self.name(), m),
            McpError::UnknownCopy(m) => format!("{}: unknown virtual copy `{}`", self.name(), m),
            McpError::InvalidParams(m) => format!("{}: {}", self.name(), m),
            McpError::Sidecar(m) => format!("{}: {}", self.name(), m),
            McpError::SidecarConflict(m) => format!(
                "{}: sidecar changed concurrently (`{}`); re-run lumina_load and retry",
                self.name(),
                m
            ),
            McpError::Decode(m) => format!("{}: {}", self.name(), m),
            McpError::Render(m) => format!("{}: {}", self.name(), m),
            McpError::Encode(m) => format!("{}: {}", self.name(), m),
            McpError::Internal(m) => format!("{}: {}", self.name(), m),
            McpError::InvalidRequest(m) => format!("{}: {}", self.name(), m),
            McpError::MethodNotFound(m) => format!("{}: {}", self.name(), m),
            McpError::Parse(m) => format!("{}: {}", self.name(), m),
        }
    }

    /// Structured `data` payload for the JSON-RPC error object.
    pub fn data(&self) -> Value {
        match self {
            McpError::InvalidAdjustment {
                name,
                value,
                minimum,
                maximum,
            } => json!({
                "error": self.name(),
                "name": name,
                "value": value,
                "minimum": minimum,
                "maximum": maximum,
            }),
            McpError::FileNotFound(m) => json!({ "error": self.name(), "path": m }),
            McpError::UnsupportedFormat(m) => json!({ "error": self.name(), "format": m }),
            McpError::UnknownImage(m) => json!({ "error": self.name(), "image_id": m }),
            McpError::UnknownCopy(m) => json!({ "error": self.name(), "copy": m }),
            McpError::InvalidParams(m) => json!({ "error": self.name(), "message": m }),
            McpError::Sidecar(m) => json!({ "error": self.name(), "message": m }),
            McpError::SidecarConflict(m) => json!({ "error": self.name(), "path": m }),
            McpError::Decode(m) => json!({ "error": self.name(), "message": m }),
            McpError::Render(m) => json!({ "error": self.name(), "message": m }),
            McpError::Encode(m) => json!({ "error": self.name(), "message": m }),
            _ => json!({ "error": self.name() }),
        }
    }
}

/// Maps a `lumina-core` error onto the MCP error model.
pub fn map_core_error(error: CoreError) -> McpError {
    match error {
        CoreError::InvalidAdjustment {
            name,
            value,
            minimum,
            maximum,
        } => McpError::InvalidAdjustment {
            name,
            value,
            minimum,
            maximum,
        },
        CoreError::Decode(message) => McpError::Decode(message),
        CoreError::Encode(message) => McpError::Encode(message),
        CoreError::UnsupportedAdjustment { key } => McpError::InvalidAdjustment {
            name: key,
            value: 0.0,
            minimum: f64::MIN,
            maximum: f64::MAX,
        },
        other => McpError::Render(other.to_string()),
    }
}

/// Builds a JSON-RPC error object.
pub fn error_response(id: Option<Value>, code: i64, message: String, data: Option<Value>) -> Value {
    let mut error = json!({ "code": code, "message": message });
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": error })
}

/// Builds a successful JSON-RPC response with the given `result`.
pub fn ok_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// Wraps a tool payload into the MCP `tools/call` result shape.
///
/// The payload is exposed twice: as a `text` content block (so any client can
/// read it) and as `structuredContent` (so structured clients need not parse
/// the text).
pub fn tool_result_response(id: Value, payload: Value) -> Value {
    let text = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    let result = json!({
        "content": [{ "type": "text", "text": text }],
        "isError": false,
        "structuredContent": payload,
    });
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// Wraps a tool *execution* error into the MCP `tools/call` result shape with
/// `isError: true`. Per the MCP specification, execution failures belong
/// inside the result object — where the calling model can read and react to
/// them — while protocol-level errors (unknown tool, malformed request,
/// unknown method) remain JSON-RPC error responses on the transport layer.
pub fn tool_error_result(id: Value, error: &McpError) -> Value {
    let result = json!({
        "content": [{ "type": "text", "text": error.message() }],
        "isError": true,
        "structuredContent": error.data(),
    });
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// Convenience for line-level JSON parse failures in the stdio loop.
pub fn parse_error_response(message: String) -> Value {
    error_response(None, -32700, message, None)
}
