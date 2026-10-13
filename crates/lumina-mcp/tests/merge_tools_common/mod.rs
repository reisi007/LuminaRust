//! Shared helpers for the MCP-PARITY-C merge-tool tests
//! (`merge_tools.rs` = registry + schema, `merge_tools_validation.rs` =
//! argument validation, error classes and the payload contract).
//!
//! The calling convention mirrors the MCP-PARITY-B surface tests
//! (`library_tools.rs` + `library_tools_common/mod.rs`): a fresh `Server` per
//! call, the real `tools/call` / `tools/list` JSON-RPC seam for the registry
//! and envelope claims, and `dispatch_tool` directly for the error-class
//! claims (an `McpError` exposes its stable `name()`, `code()` and `data()`).
//!
//! Fixtures are generated PNGs, no user photos: the 64×48 gradient anchor of
//! `crates/lumina-cli/tests/merge_parity.rs` (§9.1 acceptance anchor) and the
//! 32×24 gradient of `crates/lumina-cli/tests/merge_e2e.rs` (the smallest
//! HDR-viable pair).
//!
//! Each test file uses a part of this module; the unused half is expected.
#![allow(dead_code)]

use lumina_mcp::error::McpError;
use lumina_mcp::tools::dispatch_tool;
use lumina_mcp::Server;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The two merge tools, in registry order.
pub const MERGE_TOOLS: &[&str] = &["lumina_merge_hdr", "lumina_merge_pano"];

pub fn server() -> Server {
    Server::with_preview_dir(std::env::temp_dir())
}

/// Drives a `tools/call` through the real JSON-RPC seam.
pub fn call(name: &str, args: Value) -> Value {
    server()
        .handle_message(json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": args }
        }))
        .expect("a response")
}

/// Calls the handler through `dispatch_tool` and returns the mapped
/// [`McpError`] — the assertions branch on its stable name/code/data.
pub fn run(name: &str, args: Value) -> Result<Value, McpError> {
    dispatch_tool(&mut server(), name, &args)
}

/// A successful tool result over the seam (`isError: false`).
pub fn ok(name: &str, args: Value) -> Value {
    let response = call(name, args);
    assert!(
        response.get("error").is_none(),
        "unexpected transport error for `{name}`: {response}"
    );
    assert_eq!(
        response["result"]["isError"], false,
        "`{name}` must succeed: {}",
        response["result"]["structuredContent"]
    );
    response["result"]["structuredContent"].clone()
}

/// A refused tool result over the seam (`isError: true`).
pub fn err(name: &str, args: Value) -> Value {
    let response = call(name, args);
    assert!(
        response.get("error").is_none(),
        "`{name}` must fail as a tool error, not a protocol error: {response}"
    );
    assert_eq!(
        response["result"]["isError"], true,
        "`{name}` must be refused: {}",
        response["result"]["structuredContent"]
    );
    response["result"]["structuredContent"].clone()
}

/// Removes every `description` key recursively — the ONE intended difference
/// between the two schemas of a shared core (T4).
pub fn strip_descriptions(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .filter(|(key, _)| key != "description")
                .map(|(key, value)| (key, strip_descriptions(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(strip_descriptions).collect()),
        other => other,
    }
}

/// `true` when `key` appears anywhere in the JSON tree.
pub fn has_key(value: &Value, key: &str) -> bool {
    match value {
        Value::Object(map) => {
            map.contains_key(key) || map.values().any(|value| has_key(value, key))
        }
        Value::Array(items) => items.iter().any(|value| has_key(value, key)),
        _ => false,
    }
}

// ------------------------------------------------------------------ fixtures

/// The §9.1 acceptance anchor: 64×48 grayscale gradient `level + x % 16`
/// (identical bytes to `crates/lumina-cli/tests/merge_parity.rs:34-49`).
pub fn anchor_png(dir: &Path, name: &str, level: u8) -> PathBuf {
    let pixels: Vec<u8> = (0..64 * 48)
        .flat_map(|i| {
            let x = (i % 64) as u8;
            let value = level.saturating_add(x % 16);
            [value, value, value, 255]
        })
        .collect();
    write_png(dir, name, 64, 48, pixels)
}

/// The smallest HDR-viable fixture: 32×24 non-periodic gradient, optionally
/// scaled and shifted right (mirrors `merge_e2e.rs:26-57`).
pub fn gradient_png(dir: &Path, name: &str, scale: f32, shift_x: u32) -> PathBuf {
    let (w, h) = (32u32, 24u32);
    let mut pixels = Vec::with_capacity((w * h) as usize * 4);
    for y in 0..h {
        for x in 0..w {
            let sx = x.saturating_sub(shift_x);
            let v = if x < shift_x && shift_x > 0 {
                0.0
            } else {
                (sx as f32 * 0.7 + y as f32 * 3.1) / 128.0 * scale
            };
            let b = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            pixels.extend_from_slice(&[b, b, b, 255]);
        }
    }
    write_png(dir, name, w, h, pixels)
}

fn write_png(dir: &Path, name: &str, w: u32, h: u32, pixels: Vec<u8>) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    let png = lumina_core::ImageFrame::new(w, h, pixels)
        .unwrap()
        .encode(lumina_core::ImageFileFormat::Png)
        .unwrap();
    std::fs::write(&path, png).unwrap();
    path
}

/// `blake3:<64 hex>` of the file bytes (the checksum contract of §5).
pub fn blake3_file(path: &Path) -> String {
    format!(
        "blake3:{}",
        blake3::hash(&std::fs::read(path).unwrap()).to_hex()
    )
}
