//! MCP tool definitions and dispatch.
//!
//! Each tool lives in its own submodule and exposes three items: a `NAME`
//! constant, a `schema()` returning its JSON-Schema input description, and a
//! `run(server, args)` handler. [`list_tool_definitions`] and
//! [`dispatch_tool`] aggregate them.

pub mod analyze;
pub mod copies;
pub mod edit;
pub mod inspect;
pub mod load;
pub mod preview;
pub mod recipe;
pub mod save;

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
        other => Err(McpError::MethodNotFound(format!("unknown tool: {other}"))),
    }
}
