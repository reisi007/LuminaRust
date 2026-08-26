//! Stdio transport entry point for the LuminaRust MCP server.
//!
//! The loop itself lives in the library ([`lumina_mcp::run_stdio`]) so this
//! binary and the `lumina mcp` CLI subcommand (F-101-F1) run byte-identical
//! transport code. All logging goes to stderr so it never corrupts the
//! JSON-RPC stream.

fn main() {
    lumina_mcp::run_stdio();
}
