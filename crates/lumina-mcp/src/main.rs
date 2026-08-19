//! Stdio transport for the LuminaRust MCP server.
//!
//! Reads newline-delimited JSON-RPC messages from stdin, dispatches them via
//! [`lumina_mcp::Server`], and writes the response (if any) as one JSON object
//! per line on stdout. Notifications receive no response. All logging goes to
//! stderr so it never corrupts the JSON-RPC stream.

use lumina_mcp::Server;
use std::io::{BufRead, Write};

fn main() {
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
}
