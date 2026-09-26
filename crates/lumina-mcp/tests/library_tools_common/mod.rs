//! Shared helpers for the MCP-PARITY-B MCP-surface tests
//! (`library_tools.rs` = registry + schema,
//! `library_tools_gate.rs` = the model/artefact gate and the read/write split).
//!
//! Each test binary uses a part of this module, so the unused half is expected.
#![allow(dead_code)]

use lumina_mcp::Server;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The five tools, in registry order.
pub const LIBRARY_TOOLS: &[&str] = &[
    "lumina_collections",
    "lumina_smart_collections",
    "lumina_relocate",
    "lumina_generative",
    "lumina_regenerate",
];

pub fn server() -> Server {
    Server::with_preview_dir(std::env::temp_dir())
}

pub fn call_tool(name: &str, args: Value) -> Value {
    server()
        .handle_message(json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": args }
        }))
        .expect("a response")
}

/// A successful tool result: `isError: false` and the structured payload.
pub fn ok(name: &str, args: Value) -> Value {
    let response = call_tool(name, args);
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

/// A refused tool result: `isError: true` plus the stable error name.
pub fn err(name: &str, args: Value) -> Value {
    let response = call_tool(name, args);
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

pub fn error_name(payload: &Value) -> String {
    payload["error"].as_str().unwrap_or_default().to_string()
}

// -------------------------------------------------------------------- helpers

/// Writes a fixture image and materialises its sidecar through the shared
/// `lumina_sidecar` API (no CLI process needed here).
pub fn write_fixture(dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let input = dir.join("input.png");
    std::fs::write(&input, fixture_png()).unwrap();
    let bytes = std::fs::read(&input).unwrap();
    let frame = lumina_core::ImageFrame::decode(&bytes).unwrap();
    let identity = lumina_sidecar::SourceIdentity {
        relative_name: "input.png".into(),
        content_hash: format!("blake3:{}", blake3::hash(&bytes).to_hex()),
        byte_length: bytes.len() as u64,
        modified_at: None,
        raw_format: "PNG".into(),
        orientation: 1,
        decode_fingerprint: lumina_sidecar::DecodeFingerprint {
            decoder: "image".into(),
            version: "test".into(),
            parameters: Default::default(),
            extras: Default::default(),
        },
        geometry_fingerprint: lumina_sidecar::GeometryFingerprint {
            width: frame.width,
            height: frame.height,
            orientation: 1,
            pixel_aspect_ratio: 1.0,
            extras: Default::default(),
        },
        extras: Default::default(),
    };
    let document = lumina_sidecar::SidecarDocument::new(identity, "raster-mvp-1");
    lumina_sidecar::save_sidecar(&lumina_sidecar::sidecar_path_for(&input), &document).unwrap();
    input
}

pub fn fixture_png() -> Vec<u8> {
    let size = 32u32;
    let mut pixels = vec![0u8; (size as usize) * (size as usize) * 4];
    for y in 0..size {
        for x in 0..size {
            let index = ((y * size + x) as usize) * 4;
            let transparent = x < 6 && y < 6;
            pixels[index..index + 4].copy_from_slice(&[
                (x * 8) as u8,
                (y * 8) as u8,
                160,
                if transparent { 0 } else { 255 },
            ]);
        }
    }
    lumina_core::ImageFrame::new(size, size, pixels)
        .unwrap()
        .encode(lumina_core::ImageFileFormat::Png)
        .unwrap()
}

/// `name -> (length, content fingerprint)` for every file under `dir`. A content
/// fingerprint, not a cryptographic hash: its only job is to notice a changed
/// byte.
pub fn snapshot(dir: &Path) -> Vec<(String, usize, u64)> {
    let mut out = Vec::new();
    collect(dir, dir, &mut out);
    out.sort();
    out
}

pub fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, usize, u64)>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect(root, &path, out);
        } else {
            let bytes = std::fs::read(&path).unwrap();
            let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
            for byte in &bytes {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x1000_0000_01b3);
            }
            out.push((
                path.strip_prefix(root).unwrap().display().to_string(),
                bytes.len(),
                hash,
            ));
        }
    }
}
