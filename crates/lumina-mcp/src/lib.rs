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

use error::{error_response, ok_response, tool_error_result, tool_result_response};
use serde::Deserialize;
use serde_json::{json, Value};
use std::env;
use std::path::PathBuf;

pub use error::McpError;
pub use session::{ImageState, McpSession};

/// Minimal stderr logger installed once (no-op if another logger is already
/// registered). All MCP logging goes to stderr so it never corrupts the
/// JSON-RPC stream on stdout. The level is driven by `LUMINA_MCP_LOG`
/// (default `warn`; see `feature/platform/mcp-server.md`).
struct StderrLogger {
    level: log::LevelFilter,
}

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[lumina-mcp][{}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

/// Initialises the stderr logger from `LUMINA_MCP_LOG`. Only installs when no
/// logger has been set yet in this process.
fn init_logger() {
    use log::LevelFilter;
    let level = match std::env::var("LUMINA_MCP_LOG").ok().as_deref() {
        Some("error") => LevelFilter::Error,
        Some("info") => LevelFilter::Info,
        Some("debug") => LevelFilter::Debug,
        _ => LevelFilter::Warn,
    };
    if log::set_boxed_logger(Box::new(StderrLogger { level })).is_ok() {
        log::set_max_level(level);
    }
}

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
        init_logger();
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
        let request: Request = match Request::deserialize(&value) {
            Ok(request) => request,
            Err(error) => {
                // The line parsed as JSON but does not satisfy the JSON-RPC
                // request shape: this is Invalid Request (-32600), not a
                // Parse error (-32700). Echo the id when one is detectable.
                let id = value.get("id").filter(|id| !id.is_null()).cloned();
                return Some(error_response(
                    id,
                    -32600,
                    format!("invalid request: {error}"),
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

                // An unknown tool is a protocol-level error (MCP spec:
                // -32602 with "Unknown tool"), distinct from a registered
                // tool failing during execution.
                if !tools::is_known_tool(&name) {
                    return Some(error_response(
                        Some(id),
                        -32602,
                        format!("Unknown tool: {name}"),
                        Some(json!({ "tool": name })),
                    ));
                }

                match tools::dispatch_tool(self, &name, &arguments) {
                    Ok(payload) => Some(tool_result_response(id, payload)),
                    // Tool execution errors stay inside the result with
                    // `isError: true` so the calling model can read and react
                    // to them; they are not transport-layer failures.
                    Err(error) => Some(tool_error_result(id, &error)),
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
