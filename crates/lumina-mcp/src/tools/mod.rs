//! MCP tool definitions and dispatch.
//!
//! Each tool lives in its own submodule and exposes three items: a `NAME`
//! constant, a `schema()` returning its JSON-Schema input description, and a
//! `run(server, args)` handler. [`list_tool_definitions`] and
//! [`dispatch_tool`] aggregate them.

pub mod analyze;
pub mod batch;
pub mod collections;
pub mod copies;
pub mod dust_removal;
pub mod edit;
pub mod generative;
pub mod geometry;
pub mod import;
pub mod inspect;
pub mod lens_blur;
pub mod library_common;
pub mod load;
pub mod meta_common;
pub mod meta_export;
pub mod meta_get;
pub mod meta_preset;
pub mod meta_sync;
pub mod meta_update;
pub mod preview;
pub mod recipe;
pub mod regenerate;
pub mod reindex;
pub mod relocate;
pub mod save;
pub mod smart_collections;
pub mod spot;
pub mod stage_common;
pub mod upright;

use crate::error::McpError;
use crate::Server;
use serde_json::{json, Value};

/// Returns every tool definition for `tools/list`.
pub fn list_tool_definitions() -> Vec<Value> {
    vec![
        tool_def(load::NAME, load::DESCRIPTION, load::schema()),
        tool_def(edit::NAME, edit::DESCRIPTION, edit::schema()),
        tool_def(recipe::NAME, recipe::DESCRIPTION, recipe::schema()),
        tool_def(save::NAME, save::DESCRIPTION, save::schema()),
        tool_def(preview::NAME, preview::DESCRIPTION, preview::schema()),
        tool_def(copies::NAME, copies::DESCRIPTION, copies::schema()),
        tool_def(inspect::NAME, inspect::DESCRIPTION, inspect::schema()),
        tool_def(analyze::NAME, analyze::DESCRIPTION, analyze::schema()),
        // F-101-F1: full CLI coverage (path-based bulk tools).
        tool_def(import::NAME, import::DESCRIPTION, import::schema()),
        tool_def(batch::NAME, batch::DESCRIPTION, batch::schema()),
        tool_def(reindex::NAME, reindex::DESCRIPTION, reindex::schema()),
        tool_def(
            dust_removal::NAME,
            dust_removal::DESCRIPTION,
            dust_removal::schema(),
        ),
        // MCP-PARITY-A: the four session-based recipe stage editors. Each one has
        // a read op (`list`, plus `detect` for spot) and explicit per-field write
        // ops; none of them can set a whole stage implicitly.
        tool_def(spot::NAME, spot::DESCRIPTION, spot::schema()),
        tool_def(lens_blur::NAME, lens_blur::DESCRIPTION, lens_blur::schema()),
        tool_def(geometry::NAME, geometry::DESCRIPTION, geometry::schema()),
        tool_def(upright::NAME, upright::DESCRIPTION, upright::schema()),
        // LRPAR-G15-IPTC-S7: path-based metadata tools (beside the session).
        tool_def(meta_get::NAME, meta_get::DESCRIPTION, meta_get::schema()),
        tool_def(
            meta_update::NAME,
            meta_update::DESCRIPTION,
            meta_update::schema(),
        ),
        tool_def(
            meta_preset::NAME,
            meta_preset::DESCRIPTION,
            meta_preset::schema(),
        ),
        tool_def(meta_sync::NAME, meta_sync::DESCRIPTION, meta_sync::schema()),
        tool_def(
            meta_export::NAME,
            meta_export::DESCRIPTION,
            meta_export::schema(),
        ),
        // MCP-PARITY-B: the five path-based / artefact commands. Each one is
        // path-based (one call = one path, no `image_id`), like the F-101-F1
        // bulk tools, and each has an explicit op vocabulary with a read path
        // that never writes.
        tool_def(
            collections::NAME,
            collections::DESCRIPTION,
            collections::schema(),
        ),
        tool_def(
            smart_collections::NAME,
            smart_collections::DESCRIPTION,
            smart_collections::schema(),
        ),
        tool_def(relocate::NAME, relocate::DESCRIPTION, relocate::schema()),
        tool_def(
            generative::NAME,
            generative::DESCRIPTION,
            generative::schema(),
        ),
        tool_def(
            regenerate::NAME,
            regenerate::DESCRIPTION,
            regenerate::schema(),
        ),
    ]
}

fn tool_def(name: &str, description: &str, schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": schema,
    })
}

/// Dispatches a `tools/call` to the matching handler.
pub fn dispatch_tool(server: &mut Server, name: &str, args: &Value) -> Result<Value, McpError> {
    match name {
        load::NAME => load::run(server, args),
        edit::NAME => edit::run(server, args),
        recipe::NAME => recipe::run(server, args),
        save::NAME => save::run(server, args),
        preview::NAME => preview::run(server, args),
        copies::NAME => copies::run(server, args),
        inspect::NAME => inspect::run(server, args),
        analyze::NAME => analyze::run(server, args),
        import::NAME => import::run(server, args),
        batch::NAME => batch::run(server, args),
        reindex::NAME => reindex::run(server, args),
        dust_removal::NAME => dust_removal::run(server, args),
        spot::NAME => spot::run(server, args),
        lens_blur::NAME => lens_blur::run(server, args),
        geometry::NAME => geometry::run(server, args),
        upright::NAME => upright::run(server, args),
        meta_get::NAME => meta_get::run(server, args),
        meta_update::NAME => meta_update::run(server, args),
        meta_preset::NAME => meta_preset::run(server, args),
        meta_sync::NAME => meta_sync::run(server, args),
        meta_export::NAME => meta_export::run(server, args),
        collections::NAME => collections::run(server, args),
        smart_collections::NAME => smart_collections::run(server, args),
        relocate::NAME => relocate::run(server, args),
        generative::NAME => generative::run(server, args),
        regenerate::NAME => regenerate::run(server, args),
        other => Err(McpError::MethodNotFound(format!("unknown tool: {other}"))),
    }
}

/// Returns `true` if `name` refers to a registered tool. The protocol layer
/// uses this to answer an unknown tool with the MCP-spec protocol error
/// (`-32602`, "Unknown tool") instead of treating it as a tool execution
/// failure.
pub fn is_known_tool(name: &str) -> bool {
    list_tool_definitions()
        .iter()
        .any(|tool| tool["name"].as_str() == Some(name))
}
