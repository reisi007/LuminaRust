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
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::{BufRead, Write};
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

/// Parses the `LUMINA_MCP_KEEP_PREVIEWS` opt-out. Unset, empty or any other
/// value means "delete preview files on shutdown" — the documented default
/// (F-101 SOLL: Shutdown-Cleanup, Default: ja).
fn keep_previews_from_env(value: Option<&str>) -> bool {
    value.is_some_and(|raw| {
        matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes"
        )
    })
}

const PROTOCOL_VERSION: &str = "2024-11-05";

/// The MCP server: owns the single-image session and the preview directory.
pub struct Server {
    pub session: McpSession,
    pub preview_dir: PathBuf,
    /// Preview files written during this session. Removed at the end of the
    /// stdio loop ([`Server::cleanup_previews`]) unless `keep_previews` is set
    /// (REVIEW R2-MCP-05: previews no longer accumulate across sessions).
    pub(crate) preview_files: BTreeSet<PathBuf>,
    /// Opt-out switch for the shutdown cleanup (`LUMINA_MCP_KEEP_PREVIEWS`).
    pub(crate) keep_previews: bool,
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
        let keep_previews =
            keep_previews_from_env(env::var("LUMINA_MCP_KEEP_PREVIEWS").ok().as_deref());
        Self::with_preview_dir_and_policy(preview_dir, keep_previews)
    }

    /// Creates a server with an explicit preview directory (skips the
    /// `LUMINA_MCP_PREVIEW_DIR` lookup; used by tests and embedders).
    pub fn with_preview_dir(preview_dir: PathBuf) -> Self {
        let keep_previews =
            keep_previews_from_env(env::var("LUMINA_MCP_KEEP_PREVIEWS").ok().as_deref());
        Self::with_preview_dir_and_policy(preview_dir, keep_previews)
    }

    fn with_preview_dir_and_policy(preview_dir: PathBuf, keep_previews: bool) -> Self {
        // Initialize logging first so a failed directory creation warns
        // instead of failing silently (REVIEW R2-MCP-07).
        init_logger();
        if let Err(error) = fs::create_dir_all(&preview_dir) {
            log::warn!(
                "could not create preview directory `{}`: {error}; \
                 lumina_preview will fail until it exists",
                preview_dir.display()
            );
        }
        Self {
            session: McpSession::default(),
            preview_dir,
            preview_files: BTreeSet::new(),
            keep_previews,
            counter: 0,
        }
    }

    /// Deletes every preview file written during this session (F-101 SOLL:
    /// Shutdown-Cleanup, Default: ja). Files that no longer exist are skipped;
    /// removal failures are logged as warnings and never abort shutdown.
    pub fn cleanup_previews(&mut self) {
        let tracked = std::mem::take(&mut self.preview_files);
        if self.keep_previews {
            return;
        }
        for path in tracked {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    log::warn!("could not remove preview `{}`: {error}", path.display())
                }
            }
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

/// Runs the newline-delimited stdio loop: reads JSON-RPC lines from stdin,
/// dispatches them via [`Server::handle_line`], and writes each response as
/// one JSON object per line on stdout. Notifications receive no response.
/// When the loop ends (EOF, read error or closed stdout), all preview files
/// the session wrote are removed ([`Server::cleanup_previews`], F-101 SOLL:
/// Shutdown-Cleanup, Default: ja).
///
/// Shared by the `lumina-mcp` binary and the `lumina mcp` CLI subcommand
/// (F-101-F1) so both entry points run byte-identical transport code.
pub fn run_stdio() {
    let mut server = Server::new();
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(content) => content,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(response) = server.handle_line(trimmed) {
            let serialized = serde_json::to_string(&response).unwrap_or_else(|_| "{}".to_string());
            if out.write_all(serialized.as_bytes()).is_err() {
                break;
            }
            let _ = out.write_all(b"\n");
            let _ = out.flush();
        }
    }

    // Shutdown cleanup (REVIEW R2-MCP-05): never let preview files from this
    // session accumulate in $TMPDIR/lumina-previews/.
    server.cleanup_previews();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// F-101-F1 smoke: one initialize roundtrip through the shared stdio
    /// handler pipeline (parse → dispatch → serialize shape).
    #[test]
    fn handle_line_answers_initialize_handshake() {
        std::env::set_var("LUMINA_MCP_PREVIEW_DIR", std::env::temp_dir());
        let mut server = Server::new();
        let response = server
            .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#)
            .expect("initialize expects a response");
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], 1);
        assert_eq!(response["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(response["result"]["capabilities"]["tools"], json!({}));
    }

    /// R2-MCP-07: a preview directory that cannot be created (here: the path
    /// is an existing file) must not abort startup — the condition is logged
    /// as a warning instead of failing silently.
    #[test]
    fn failed_preview_dir_creation_does_not_abort_startup() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("not-a-dir");
        fs::write(&blocker, b"file").unwrap();
        let server = Server::with_preview_dir(blocker.clone());
        assert_eq!(server.preview_dir, blocker);
        assert!(!blocker.is_dir(), "the file must not be replaced");
    }

    /// R2-MCP-05: tracked previews are deleted by `cleanup_previews` under
    /// the default policy and kept when the opt-out is set.
    #[test]
    fn cleanup_previews_honors_the_keep_policy_and_removes_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let previews = dir.path().join("previews");
        fs::create_dir_all(&previews).unwrap();

        let mut server = Server::with_preview_dir(previews.clone());
        let marker = previews.join("session.png");
        fs::write(&marker, b"png-bytes").unwrap();
        server.preview_files.insert(marker.clone());

        // Opt-out: file survives, list is still reset.
        server.keep_previews = true;
        server.cleanup_previews();
        assert!(marker.exists(), "LUMINA_MCP_KEEP_PREVIEWS keeps the file");
        assert!(server.preview_files.is_empty());

        // Default policy: a tracked file is removed.
        server.preview_files.insert(marker.clone());
        server.keep_previews = false;
        server.cleanup_previews();
        assert!(!marker.exists(), "default cleanup removes tracked previews");

        // Idempotent: an empty list is a no-op (no spurious warnings).
        server.cleanup_previews();
    }

    #[test]
    fn keep_previews_env_values_are_parsed_strictly() {
        assert!(!keep_previews_from_env(None));
        assert!(!keep_previews_from_env(Some("")));
        assert!(!keep_previews_from_env(Some("0")));
        assert!(!keep_previews_from_env(Some("keep")));
        assert!(keep_previews_from_env(Some("1")));
        assert!(keep_previews_from_env(Some("true")));
        assert!(keep_previews_from_env(Some("YES")));
        assert!(keep_previews_from_env(Some(" yes ")));
    }
}
