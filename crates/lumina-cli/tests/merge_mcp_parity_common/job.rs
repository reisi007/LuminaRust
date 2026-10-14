//! MCP-PARITY-C: one merge job, expressed once and rendered onto both
//! transports.
//!
//! Every byte-parity claim in this slice rests on "the same input set on both
//! transports", so a test writes the values down once and [`MergeCall`] renders
//! them into the CLI spelling (`--exposure-times 0.004,0.016`) and into the MCP
//! JSON (`"exposure_times": [0.004, 0.016]`). A test that spelled the two
//! transports separately would be free to compare two *different* input sets,
//! which is exactly the defect a parity test must be unable to have.
//!
//! Both spellings come from the same field set, so the mapping itself is
//! visible: the explicit lists are the CLI's comma-separated `value_delimiter`
//! form (`merge.rs:55-62`) on one side and real JSON arrays on the other. Every
//! other argument is one CLI flag ↔ one JSON key.
//!
//! `MergeCall` never runs anything on its own that a test could get wrong: the
//! CLI side runs the real binary through `CARGO_BIN_EXE_lumina-cli`, the MCP
//! side runs the tool handler in-process (so a test can branch on the mapped
//! `McpError`) *or* over the real `tools/call` JSON-RPC seam (so a test can see
//! what an agent sees).

#![allow(dead_code)]

use lumina_mcp::error::McpError;
use lumina_mcp::tools::{merge_hdr, merge_pano};
use lumina_mcp::Server;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Output;

/// The MCP twin of `lumina merge-hdr`.
pub const HDR_TOOL: &str = "lumina_merge_hdr";
/// The MCP twin of `lumina merge-pano`.
pub const PANO_TOOL: &str = "lumina_merge_pano";

/// The real `lumina-cli` binary. Cargo injects this variable for the package
/// that declares the binary — which is why the CLI side of every comparison in
/// this slice lives in `crates/lumina-cli/tests/`.
pub fn cli() -> std::process::Command {
    std::process::Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

pub fn stderr_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

pub fn stdout_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[derive(Debug, Clone)]
pub struct MergeCall {
    inputs: Vec<PathBuf>,
    output: Option<PathBuf>,
    exposure_times: Vec<f64>,
    isos: Vec<u32>,
    f_numbers: Vec<f64>,
    max_shift_px: Option<i32>,
    blend_width_px: Option<u32>,
    force: bool,
}

impl MergeCall {
    /// A job over `inputs` — source paths in merge order, `inputs[0]` the
    /// reference (`merge.rs:45-50`).
    pub fn new(inputs: Vec<PathBuf>) -> Self {
        Self {
            inputs,
            output: None,
            exposure_times: Vec::new(),
            isos: Vec::new(),
            f_numbers: Vec::new(),
            max_shift_px: None,
            blend_width_px: None,
            force: false,
        }
    }

    /// The explicit exposure lists: empty = EXIF-or-fallback per source
    /// (`merge.rs:58-65`).
    pub fn exposure(mut self, exposure_times: &[f64], isos: &[u32], f_numbers: &[f64]) -> Self {
        self.exposure_times = exposure_times.to_vec();
        self.isos = isos.to_vec();
        self.f_numbers = f_numbers.to_vec();
        self
    }

    pub fn output(mut self, output: PathBuf) -> Self {
        self.output = Some(output);
        self
    }

    pub fn max_shift_px(mut self, px: i32) -> Self {
        self.max_shift_px = Some(px);
        self
    }

    pub fn blend_width_px(mut self, px: u32) -> Self {
        self.blend_width_px = Some(px);
        self
    }

    /// Re-merge a stale or dangling bundle (`merge.rs:70-73`).
    pub fn force(mut self) -> Self {
        self.force = true;
        self
    }

    pub fn inputs(&self) -> &[PathBuf] {
        &self.inputs
    }

    /// The CLI spelling (`merge.rs:45-80`).
    pub fn to_cli_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(output) = &self.output {
            args.push("--output".to_string());
            args.push(output.display().to_string());
        }
        if !self.exposure_times.is_empty() {
            args.push("--exposure-times".to_string());
            args.push(csv(&self.exposure_times));
        }
        if !self.isos.is_empty() {
            args.push("--isos".to_string());
            args.push(csv(&self.isos));
        }
        if !self.f_numbers.is_empty() {
            args.push("--f-numbers".to_string());
            args.push(csv(&self.f_numbers));
        }
        if let Some(px) = self.max_shift_px {
            args.push("--max-shift-px".to_string());
            args.push(px.to_string());
        }
        if let Some(px) = self.blend_width_px {
            args.push("--blend-width-px".to_string());
            args.push(px.to_string());
        }
        if self.force {
            args.push("--force".to_string());
        }
        args
    }

    /// The MCP spelling (§2 of the SOLL contract: snake_case keys, real JSON
    /// arrays for the explicit lists).
    pub fn to_mcp_args(&self) -> Value {
        let mut args = json!({ "inputs": self.inputs });
        if let Some(output) = &self.output {
            args["output"] = json!(output);
        }
        if !self.exposure_times.is_empty() {
            args["exposure_times"] = json!(self.exposure_times);
        }
        if !self.isos.is_empty() {
            args["isos"] = json!(self.isos);
        }
        if !self.f_numbers.is_empty() {
            args["f_numbers"] = json!(self.f_numbers);
        }
        if let Some(px) = self.max_shift_px {
            args["max_shift_px"] = json!(px);
        }
        if let Some(px) = self.blend_width_px {
            args["blend_width_px"] = json!(px);
        }
        if self.force {
            args["force"] = json!(true);
        }
        args
    }

    /// Runs the real CLI binary. `json` adds `--json`, the pure transport flag
    /// that makes a report comparable at all (`merge.rs:74-76`).
    pub fn cli(&self, command: &str, json: bool) -> Output {
        let mut cmd = cli();
        cmd.arg(command);
        for input in &self.inputs {
            cmd.arg("--input").arg(input);
        }
        for arg in self.to_cli_args() {
            cmd.arg(arg);
        }
        if json {
            cmd.arg("--json");
        }
        cmd.output().expect("the CLI binary runs")
    }

    /// Runs the CLI binary and parses its `--json` report.
    pub fn cli_json(&self, command: &str) -> Value {
        let output = self.cli(command, true);
        assert!(
            output.status.success(),
            "`lumina {command}` must succeed for {}\nstderr: {}",
            self.inputs
                .iter()
                .map(|input| input.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
            stderr_of(&output)
        );
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "`lumina {command} --json` did not print one JSON document ({error}): {}",
                stdout_of(&output)
            )
        })
    }

    /// Runs the CLI binary and asserts it refuses loudly (non-zero exit).
    pub fn cli_failure(&self, command: &str) -> Output {
        let output = self.cli(command, false);
        assert_ne!(
            output.status.code(),
            Some(0),
            "`lumina {command}` must refuse this input set (stderr: {})",
            stderr_of(&output)
        );
        output
    }

    /// Runs the tool handler in-process, so the caller sees the mapped
    /// [`McpError`] with its stable `name()`, `code()` and `data()`.
    pub fn mcp(&self, tool: &str) -> Result<Value, McpError> {
        let mut server = Server::with_preview_dir(std::env::temp_dir());
        match tool {
            HDR_TOOL => merge_hdr::run(&mut server, &self.to_mcp_args()),
            PANO_TOOL => merge_pano::run(&mut server, &self.to_mcp_args()),
            other => panic!("not a merge tool: {other}"),
        }
    }

    /// [`MergeCall::mcp`] for a run that must succeed.
    pub fn mcp_ok(&self, tool: &str) -> Value {
        let label = self.to_mcp_args().to_string();
        self.mcp(tool)
            .unwrap_or_else(|error| panic!("{tool} {label} failed: {}", error.message()))
    }

    /// Runs the tool over the real `tools/call` JSON-RPC seam and returns
    /// `result.structuredContent`, asserting `isError: false`.
    pub fn mcp_seam(&self, tool: &str) -> Value {
        let response = self.seam_call(tool);
        assert_eq!(
            response["result"]["isError"],
            json!(false),
            "{tool} must succeed over the seam: {}",
            response["result"]["structuredContent"]
        );
        response["result"]["structuredContent"].clone()
    }

    /// Runs the tool over the seam and returns `result.structuredContent` for a
    /// refusal (`isError: true`).
    pub fn mcp_seam_failure(&self, tool: &str) -> Value {
        let response = self.seam_call(tool);
        assert_eq!(
            response["result"]["isError"],
            json!(true),
            "{tool} must refuse over the seam: {}",
            response["result"]["structuredContent"]
        );
        response["result"]["structuredContent"].clone()
    }

    fn seam_call(&self, tool: &str) -> Value {
        let response = Server::with_preview_dir(std::env::temp_dir())
            .handle_message(json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": tool, "arguments": self.to_mcp_args() }
            }))
            .unwrap_or_else(|| panic!("{tool} must answer a well-formed tools/call"));
        assert!(
            response.get("error").is_none(),
            "{tool} must answer on the protocol layer, not with a JSON-RPC error: {response}"
        );
        response
    }
}

/// The comma-separated spelling clap's `value_delimiter = ','` parses back into
/// one value per source (`merge.rs:55-62`).
fn csv<T: std::fmt::Display>(values: &[T]) -> String {
    values
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(",")
}
