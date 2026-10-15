//! MCP-PARITY-C: one matrix run, expressed once and rendered onto both
//! transports.
//!
//! Every byte-parity claim in this slice rests on "the same input set on both
//! transports", so a test writes the values down once and [`MatrixCall`]
//! renders them into the CLI spelling (`--recipe-set … --recipe tone`) and
//! into the MCP JSON (`"recipe_set": …, "recipes": ["tone"]`). A test that
//! spelled the two transports separately would be free to compare two
//! *different* input sets, which is exactly the defect a parity test must be
//! unable to have.
//!
//! Both spellings come from the same field set, so the mapping itself is
//! visible: the repeatable `--recipe` flag on one side and a real JSON array
//! on the other, every other argument one CLI flag ↔ one JSON key. `--json`
//! has no MCP twin — it is a pure CLI transport flag, the MCP tool always
//! answers `structuredContent` (SOLL §2).
//!
//! `MatrixCall` never runs anything on its own that a test could get wrong:
//! the CLI side runs the real binary through `CARGO_BIN_EXE_lumina-cli`, the
//! MCP side runs the tool handler in-process (so a test can branch on the
//! mapped `McpError`) *or* over the real `tools/call` JSON-RPC seam (so a
//! test can see what an agent sees).

#![allow(dead_code)]

use lumina_mcp::error::McpError;
use lumina_mcp::tools::{dispatch_tool, matrix};
use lumina_mcp::Server;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Output;

/// The MCP twin of `lumina matrix`.
pub const TOOL: &str = matrix::NAME;

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
pub struct MatrixCall {
    recipe_set: Option<PathBuf>,
    golden_dir: Option<PathBuf>,
    work_dir: Option<PathBuf>,
    recipes: Vec<String>,
    comparison_width: Option<u32>,
    update_goldens: bool,
    require_gpu: bool,
}

impl MatrixCall {
    /// A run with every argument defaulted (env chain for the set, `<set>`
    /// `/golden` for the goldens, a fresh tempdir for the work dir).
    pub fn new() -> Self {
        Self {
            recipe_set: None,
            golden_dir: None,
            work_dir: None,
            recipes: Vec::new(),
            comparison_width: None,
            update_goldens: false,
            require_gpu: false,
        }
    }

    /// A run over `recipe_set` with an explicit work dir — the standard
    /// parity shape, so report paths stay inside the two compared trees.
    pub fn verify(recipe_set: PathBuf, work_dir: PathBuf) -> Self {
        Self::new().recipe_set(recipe_set).work_dir(work_dir)
    }

    pub fn recipe_set(mut self, path: PathBuf) -> Self {
        self.recipe_set = Some(path);
        self
    }

    pub fn golden_dir(mut self, path: PathBuf) -> Self {
        self.golden_dir = Some(path);
        self
    }

    pub fn work_dir(mut self, path: PathBuf) -> Self {
        self.work_dir = Some(path);
        self
    }

    /// Only run these recipe ids, in the given order on the CLI spelling and
    /// as one array on the MCP spelling. Empty = all recipes of the set.
    pub fn recipes(mut self, ids: &[&str]) -> Self {
        self.recipes = ids.iter().map(|id| (*id).to_string()).collect();
        self
    }

    pub fn comparison_width(mut self, width: u32) -> Self {
        self.comparison_width = Some(width);
        self
    }

    /// Baseline mode: write/refresh the goldens instead of verifying.
    pub fn baseline(mut self) -> Self {
        self.update_goldens = true;
        self
    }

    pub fn require_gpu(mut self) -> Self {
        self.require_gpu = true;
        self
    }

    /// The CLI spelling (`matrix.rs:30-62`).
    pub fn to_cli_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(path) = &self.recipe_set {
            args.push("--recipe-set".to_string());
            args.push(path.display().to_string());
        }
        if let Some(path) = &self.golden_dir {
            args.push("--golden-dir".to_string());
            args.push(path.display().to_string());
        }
        if let Some(path) = &self.work_dir {
            args.push("--work-dir".to_string());
            args.push(path.display().to_string());
        }
        for id in &self.recipes {
            args.push("--recipe".to_string());
            args.push(id.clone());
        }
        if let Some(width) = self.comparison_width {
            args.push("--comparison-width".to_string());
            args.push(width.to_string());
        }
        if self.update_goldens {
            args.push("--update-goldens".to_string());
        }
        if self.require_gpu {
            args.push("--require-gpu".to_string());
        }
        args
    }

    /// The MCP spelling (§2: snake_case keys, a real JSON array for the
    /// recipe filter; absent = the runner default, exactly like the CLI).
    pub fn to_mcp_args(&self) -> Value {
        let mut args = json!({});
        if let Some(path) = &self.recipe_set {
            args["recipe_set"] = json!(path);
        }
        if let Some(path) = &self.golden_dir {
            args["golden_dir"] = json!(path);
        }
        if let Some(path) = &self.work_dir {
            args["work_dir"] = json!(path);
        }
        if !self.recipes.is_empty() {
            args["recipes"] = json!(self.recipes);
        }
        if let Some(width) = self.comparison_width {
            args["comparison_width"] = json!(width);
        }
        if self.update_goldens {
            args["update_goldens"] = json!(true);
        }
        if self.require_gpu {
            args["require_gpu"] = json!(true);
        }
        args
    }

    /// Runs the real CLI binary. `json` adds `--json`, the pure transport flag
    /// that makes a report comparable at all (`main.rs:5675-5682`).
    pub fn cli(&self, json: bool) -> Output {
        let mut cmd = cli();
        cmd.arg("matrix");
        for arg in self.to_cli_args() {
            cmd.arg(arg);
        }
        if json {
            cmd.arg("--json");
        }
        cmd.output().expect("the CLI binary runs")
    }

    /// Runs the CLI binary and parses its `--json` report.
    pub fn cli_json(&self) -> Value {
        let output = self.cli(true);
        assert!(
            output.status.success(),
            "`lumina matrix` must succeed for {}\nstderr: {}",
            self.to_mcp_args(),
            stderr_of(&output)
        );
        cli_stdout_json(&output)
    }

    /// Runs the CLI binary with `--json` and parses its report even though the
    /// run is red: the CLI prints the full report before failing loudly
    /// (`matrix.rs`, SOLL §7.1).
    pub fn cli_red_json(&self) -> (Value, Output) {
        let output = self.cli(true);
        assert_ne!(
            output.status.code(),
            Some(0),
            "`lumina matrix` must refuse this input set (stderr: {})",
            stderr_of(&output)
        );
        (cli_stdout_json(&output), output)
    }

    /// Runs the CLI binary and asserts it refuses loudly (non-zero exit).
    pub fn cli_failure(&self) -> Output {
        let output = self.cli(false);
        assert_ne!(
            output.status.code(),
            Some(0),
            "`lumina matrix` must refuse this input set (stderr: {})",
            stderr_of(&output)
        );
        output
    }

    /// Runs the tool handler in-process, so the caller sees the mapped
    /// [`McpError`] with its stable `name()`, `code()` and `data()`.
    pub fn mcp(&self) -> Result<Value, McpError> {
        dispatch_tool(
            &mut Server::with_preview_dir(std::env::temp_dir()),
            TOOL,
            &self.to_mcp_args(),
        )
    }

    /// [`MatrixCall::mcp`] for a run that must succeed.
    pub fn mcp_ok(&self) -> Value {
        let label = self.to_mcp_args().to_string();
        self.mcp()
            .unwrap_or_else(|error| panic!("{TOOL} {label} failed: {}", error.message()))
    }

    /// Runs the tool over the real `tools/call` JSON-RPC seam and returns
    /// `result.structuredContent`, asserting `isError: false`.
    pub fn mcp_seam(&self) -> Value {
        let response = self.seam_call();
        assert_eq!(
            response["result"]["isError"],
            json!(false),
            "{TOOL} must succeed over the seam: {}",
            response["result"]["structuredContent"]
        );
        response["result"]["structuredContent"].clone()
    }

    /// Runs the tool over the seam and returns `result.structuredContent` for a
    /// refusal (`isError: true`).
    pub fn mcp_seam_failure(&self) -> Value {
        let response = self.seam_call();
        assert_eq!(
            response["result"]["isError"],
            json!(true),
            "{TOOL} must refuse over the seam: {}",
            response["result"]["structuredContent"]
        );
        response["result"]["structuredContent"].clone()
    }

    fn seam_call(&self) -> Value {
        let response = Server::with_preview_dir(std::env::temp_dir())
            .handle_message(json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": TOOL, "arguments": self.to_mcp_args() }
            }))
            .unwrap_or_else(|| panic!("{TOOL} must answer a well-formed tools/call"));
        assert!(
            response.get("error").is_none(),
            "{TOOL} must answer on the protocol layer, not with a JSON-RPC error: {response}"
        );
        response
    }
}

impl Default for MatrixCall {
    fn default() -> Self {
        Self::new()
    }
}

/// Parses the CLI's `--json` stdout as exactly one JSON document.
fn cli_stdout_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "`lumina matrix --json` did not print one JSON document ({error}): {}",
            stdout_of(output)
        )
    })
}
