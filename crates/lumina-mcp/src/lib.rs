//! `lumina-mcp` — MCP (Model Context Protocol) server exposing LuminaRust
//! editing as agent tools over stdio (JSON-RPC).
//!
//! The server is a thin orchestration layer over `lumina-core` (rendering),
//! `lumina-sidecar` (atomic sidecar IO) and `lumina-raw` (decoding). It keeps
//! exactly one image in memory at a time ([`session::McpSession`]) and never
//! reimplements image processing.

pub mod error;
pub mod session;
pub mod tools;
pub mod util;

use error::{error_response, ok_response, tool_result_response};
use serde::Deserialize;
use serde_json::{json, Value};
use std::env;
use std::path::PathBuf;

pub use error::McpError;
pub use session::{ImageState, McpSession};

/// Incoming JSON-RPC request (only the fields the server reads are declared).
#[derive(Deserialize)]
struct Request {
    #[serde(default)]
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

const PROTOCOL_VERSION: &str = "2024-11-05";

/// The MCP server: owns the single-image session and the preview directory.
pub struct Server {
    pub session: McpSession,
    pub preview_dir: PathBuf,
    counter: u64,
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

impl Server {
    /// Creates a server, resolving the preview directory from
    /// `LUMINA_MCP_PREVIEW_DIR` (falling back to `$TMPDIR/lumina-previews/`).
    pub fn new() -> Self {
        let preview_dir = env::var("LUMINA_MCP_PREVIEW_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let tmpdir = env::var("TMPDIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| std::env::temp_dir());
                tmpdir.join("lumina-previews")
            });
        let _ = std::fs::create_dir_all(&preview_dir);
        Self {
            session: McpSession::default(),
            preview_dir,
            counter: 0,
        }
    }

    /// Generates a process-local, stable 8-hex-char image id.
    pub fn generate_image_id(&mut self, path: &std::path::Path) -> String {
        self.counter += 1;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let mut buffer: Vec<u8> = Vec::new();
        buffer.extend_from_slice(path.to_string_lossy().as_bytes());
        buffer.extend_from_slice(&self.counter.to_le_bytes());
        buffer.extend_from_slice(&nanos.to_le_bytes());
        let digest = blake3::hash(&buffer);
        let bytes = digest.as_bytes();
        format!(
            "{:02x}{:02x}{:02x}{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3]
        )
    }

    /// Parses a single stdio line and dispatches it. Returns `None` for
    /// notifications (no id) so the loop writes nothing.
    pub fn handle_line(&mut self, line: &str) -> Option<Value> {
        match serde_json::from_str::<Value>(line) {
            Ok(value) => self.handle_message(value),
            Err(error) => Some(error_response(
                None,
                -32700,
                format!("parse error: {error}"),
                None,
            )),
        }
    }

    /// Dispatches a parsed JSON-RPC message.
    pub fn handle_message(&mut self, value: Value) -> Option<Value> {
        let request: Request = match serde_json::from_value(value) {
            Ok(request) => request,
            Err(error) => {
                return Some(error_response(
                    None,
                    -32700,
                    format!("parse error: {error}"),
                    None,
                ));
            }
        };

        // Notifications carry no id and expect no response.
        let id = request.id?;

        match request.method.as_str() {
            "initialize" => Some(ok_response(id, initialize_result())),
            "ping" => Some(ok_response(id, json!({}))),
            "tools/list" => Some(ok_response(id, tools_list_result())),
            "tools/call" => {
                let params = request.params.clone().unwrap_or(Value::Null);
                let name = params
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .to_string();
                let arguments = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                match tools::dispatch_tool(self, &name, &arguments) {
                    Ok(payload) => Some(tool_result_response(id, payload)),
                    Err(error) => Some(error_response(
                        Some(id),
                        error.code(),
                        error.message(),
                        Some(error.data()),
                    )),
                }
            }
            other => Some(error_response(
                Some(id),
                -32601,
                format!("method not found: {other}"),
                Some(json!({ "method": other })),
            )),
        }
    }
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "lumina-mcp",
            "version": env!("CARGO_PKG_VERSION"),
        },
    })
}

fn tools_list_result() -> Value {
    json!({ "tools": tools::list_tool_definitions() })
}
