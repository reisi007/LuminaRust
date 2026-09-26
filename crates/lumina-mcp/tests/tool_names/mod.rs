//! The documented MCP tool set, by name, plus the drift guard that pins it.
//!
//! MCP-PARITY-B: this list and its guard used to live inline in `mcp.rs`. They
//! moved here for two reasons:
//!
//! * **the ratchet.** `mcp.rs` is baseline-listed, so it may only shrink. Adding
//!   five tool names inline would have grown it; extracting the list is the
//!   documented alternative.
//! * **the guard got stronger, not just longer.** Slice A's review found the
//!   count guard stale-able: a rename that keeps the total passes a bare
//!   `assert_eq!(len)`. The guard here pins the *membership* in both directions
//!   and rejects duplicates, and every claim is a membership assertion, so a
//!   deletion in `list_tool_definitions` that also shrinks this constant cannot
//!   hide. The same list is pinned a second time, over the real protocol, by
//!   `crates/lumina-cli/src/tests/mcp_onnx.rs`.

use lumina_mcp::tools::{is_known_tool, list_tool_definitions};
use lumina_mcp::Server;
use serde_json::json;

/// Every tool the server registers, in registry order.
pub const TOOL_NAMES: &[&str] = &[
    // Editing session tools (original F-101 scope).
    "lumina_load",
    "lumina_edit",
    "lumina_get_recipe",
    "lumina_save",
    "lumina_preview",
    "lumina_list_virtual_copies",
    "lumina_inspect",
    "lumina_analyze",
    // F-101-F1: full CLI coverage (path-based bulk tools).
    "lumina_import",
    "lumina_batch",
    "lumina_reindex",
    "lumina_dust_removal",
    // LRPAR-G15-IPTC-S7: path-based metadata tools.
    "lumina_get_metadata_draft",
    "lumina_update_metadata_draft",
    "lumina_apply_meta_preset",
    "lumina_batch_sync_metadata",
    "lumina_trigger_export",
    // MCP-PARITY-A: the four session-based recipe stage editors.
    "lumina_spot",
    "lumina_lens_blur",
    "lumina_geometry",
    "lumina_upright",
    // MCP-PARITY-B: the five path-based / artefact commands.
    "lumina_collections",
    "lumina_smart_collections",
    "lumina_relocate",
    "lumina_generative",
    "lumina_regenerate",
];

/// `tools/list` reports exactly [`TOOL_NAMES`] — no more, no less, no twice.
///
/// Driven through the real JSON-RPC path, not through the helper functions, so a
/// tool that is only in the helper cannot pass here.
#[test]
fn tools_list_reports_exactly_the_documented_tool_names() {
    let mut server = Server::with_preview_dir(std::env::temp_dir());
    let response = server
        .handle_message(json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
        .expect("tools/list expects a response");
    let tools = response["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();

    // Membership, both directions, plus the count. The count alone would let a
    // rename slip through.
    assert_eq!(names.len(), TOOL_NAMES.len(), "tool set drifted: {names:?}");
    for name in &names {
        assert!(
            TOOL_NAMES.contains(name),
            "`{name}` is registered but not in TOOL_NAMES"
        );
    }
    for name in TOOL_NAMES {
        assert!(
            names.contains(name),
            "`{name}` is in TOOL_NAMES but not registered: {names:?}"
        );
    }
    // No duplicates: a repeated name would satisfy both directions above while
    // `tools/call` would only ever reach one of the two entries.
    let mut unique = names.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        names.len(),
        "tools/list reports a duplicate name: {names:?}"
    );
    // Every registered name is also dispatchable and accepted by
    // `is_known_tool`, which is what the protocol layer gates `tools/call` on.
    for name in &names {
        assert!(
            is_known_tool(name),
            "`{name}` is listed but is_known_tool refuses it"
        );
    }
    assert_eq!(
        list_tool_definitions().len(),
        TOOL_NAMES.len(),
        "list_tool_definitions and tools/list must agree"
    );
}
