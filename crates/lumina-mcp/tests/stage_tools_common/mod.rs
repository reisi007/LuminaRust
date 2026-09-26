//! Shared helper for the MCP-PARITY-A stage-tool surface tests
//! (`stage_tools.rs` = registry + schema, `stage_tools_session.rs` = the
//! read/write split and the compare-and-swap contract). Both drive the real
//! JSON-RPC `tools/call` / `tools/list` path of the in-process server.

pub use lumina_core::{ImageFileFormat, ImageFrame};
// `stage_tools.rs` names the four tool modules directly (name/description/
// schema); `stage_tools_session.rs` drives them over the protocol and never
// needs the module paths, so the re-export is unused in that binary.
#[allow(unused_imports)]
pub use lumina_mcp::tools::{geometry, lens_blur, spot, upright};
pub use lumina_mcp::Server;
pub use serde_json::json;
pub use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// The four stage tools of this slice.
pub const STAGE_TOOLS: &[&str] = &[
    "lumina_spot",
    "lumina_lens_blur",
    "lumina_geometry",
    "lumina_upright",
];

pub fn tilted_png(size: u32) -> Vec<u8> {
    let mut pixels = vec![0u8; (size as usize) * (size as usize) * 4];
    for y in 0..size {
        for x in 0..size {
            let value = if (x + y) % 8 < 4 { 230u8 } else { 25u8 };
            let index = ((y * size + x) as usize) * 4;
            pixels[index..index + 4].copy_from_slice(&[value, value, value, 255]);
        }
    }
    ImageFrame::new(size, size, pixels)
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap()
}

pub struct Session {
    pub server: Server,
    pub image_id: String,
    pub sidecar: PathBuf,
}

impl Session {
    pub fn new(dir: &Path) -> Self {
        fs::create_dir_all(dir).unwrap();
        let input = dir.join("tilted.png");
        fs::write(&input, tilted_png(64)).unwrap();
        let mut server = Server::with_preview_dir(dir.join("previews"));
        let loaded =
            lumina_mcp::tools::load::run(&mut server, &json!({ "path": input.to_str().unwrap() }))
                .unwrap();
        Self {
            image_id: loaded["image_id"].as_str().unwrap().to_string(),
            sidecar: lumina_sidecar::sidecar_path_for(&input),
            server,
        }
    }

    pub fn sidecar_bytes(&self) -> Vec<u8> {
        fs::read(&self.sidecar).unwrap()
    }

    /// A `tools/call` through the real JSON-RPC path, so the test covers
    /// `is_known_tool` + `dispatch_tool` + the result envelope.
    ///
    /// Per the MCP spec a *tool* failure lives inside the result with
    /// `isError: true`; only a protocol failure is a JSON-RPC `error`. Both are
    /// surfaced here so a test can assert on either.
    pub fn raw(&mut self, tool: &str, arguments: Value) -> Value {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": tool, "arguments": arguments }
        });
        self.server.handle_message(request).expect("a response")
    }

    /// The `structuredContent` of a successful call.
    #[allow(dead_code)] // only the read/write-split file asserts on a success payload
    pub fn call(&mut self, tool: &str, arguments: Value) -> Value {
        let response = self.raw(tool, arguments);
        assert!(
            response.get("error").is_none(),
            "{tool}: protocol error {response}"
        );
        let result = &response["result"];
        assert_eq!(result["isError"], json!(false), "{tool} failed: {result}");
        result["structuredContent"].clone()
    }

    /// The `structuredContent` of a failed call — the tool error's stable name
    /// plus its message, in the same shape the existing tools use.
    pub fn err(&mut self, tool: &str, arguments: Value) -> Value {
        let response = self.raw(tool, arguments);
        assert!(
            response.get("error").is_none(),
            "{tool}: expected a tool error, got a protocol error: {response}"
        );
        let result = &response["result"];
        assert_eq!(
            result["isError"],
            json!(true),
            "{tool} did not fail: {result}"
        );
        result["structuredContent"].clone()
    }
}
