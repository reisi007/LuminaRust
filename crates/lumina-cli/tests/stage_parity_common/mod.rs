//! Shared helpers for the MCP-PARITY-A stage-editor parity tests
//! (`stage_parity.rs` = read/write byte identity, `stage_parity_errors.rs` = the
//! loud rejections). Both test files drive the real `lumina-cli` process and the
//! in-process MCP server over two byte-identical fixtures.

use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_mcp::tools::{geometry, lens_blur, load, spot, upright};
use lumina_mcp::{McpError, Server};
pub use lumina_sidecar::sidecar_path_for;
pub use serde_json::json;
pub use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The four stage editors under test: (tool module name, MCP tool name, CLI
/// subcommand, MCP stage-report `command`).
#[allow(dead_code)] // only stage_parity_errors.rs iterates the stages
pub const STAGES: &[&str] = &["spot", "lens-blur", "geometry", "upright"];

/// A `size`x`size` bright-bar grid rotated by `angle_deg`: a real line signal for
/// the `upright-lines-v1` detector and a non-flat image for the spot heuristic.
#[allow(dead_code)] // not every parity test file uses every helper
pub fn tilted_png(size: u32, angle_deg: f32) -> Vec<u8> {
    let (sin, cos) = angle_deg.to_radians().sin_cos();
    let period = 0.4f32;
    let mut pixels = vec![0u8; (size as usize) * (size as usize) * 4];
    for y in 0..size {
        for x in 0..size {
            let nx = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let ny = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let rx = cos * nx + sin * ny;
            let dy = (rx / period - (rx / period).round()).abs() * period;
            let value = if dy < 0.05 { 235u8 } else { 20u8 };
            let index = ((y * size + x) as usize) * 4;
            pixels[index..index + 4].copy_from_slice(&[value, value, value, 255]);
        }
    }
    ImageFrame::new(size, size, pixels)
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap()
}

#[allow(dead_code)] // not every parity test file uses every helper
pub fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

/// Writes `<dir>/tilted.png` and materialises its sidecar via `lumina import`,
/// exactly as a user would. Returns the source path.
pub fn fixture(dir: &Path) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let input = dir.join("tilted.png");
    fs::write(&input, tilted_png(64, 12.0)).unwrap();
    let output = cli()
        .args(["import", "--input", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "import failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(sidecar_path_for(&input).is_file());
    input
}

/// Runs a CLI command and parses its `--json` document.
#[allow(dead_code)] // only the byte-identity file reads a `--json` report
pub fn cli_json(args: &[&str]) -> Value {
    let output = cli().args(args).output().unwrap();
    assert!(
        output.status.success(),
        "`lumina {}` failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// Runs a CLI command that must abort, returning its stderr and exit code.
#[allow(dead_code)] // only stage_parity_errors.rs needs the failing-CLI path
pub fn cli_fails(args: &[&str]) -> (String, i32) {
    let output = cli().args(args).output().unwrap();
    (
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

pub struct Session {
    server: Server,
    image_id: String,
    input: PathBuf,
}

impl Session {
    pub fn new(dir: &Path) -> Self {
        let mut server = Server::with_preview_dir(dir.join("previews"));
        let input = fixture(dir);
        let loaded = load::run(&mut server, &json!({ "path": input.to_str().unwrap() })).unwrap();
        let image_id = loaded["image_id"].as_str().unwrap().to_string();
        Self {
            server,
            image_id,
            input,
        }
    }

    pub fn call(&mut self, stage: &str, arguments: Value) -> Result<Value, McpError> {
        let mut payload = json!({ "image_id": self.image_id, "op": arguments["op"].clone() });
        for (key, value) in arguments.as_object().unwrap() {
            if key == "op" {
                continue;
            }
            payload[key.clone()] = value.clone();
        }
        match stage {
            "spot" => spot::run(&mut self.server, &payload),
            "lens-blur" => lens_blur::run(&mut self.server, &payload),
            "geometry" => geometry::run(&mut self.server, &payload),
            "upright" => upright::run(&mut self.server, &payload),
            other => panic!("no such stage tool: {other}"),
        }
    }

    #[allow(dead_code)] // only the byte-identity file asserts on a success payload
    pub fn ok(&mut self, stage: &str, arguments: Value) -> Value {
        let label = arguments.to_string();
        self.call(stage, arguments)
            .unwrap_or_else(|error| panic!("lumina_{stage} {label} failed: {}", error.message()))
    }

    pub fn sidecar(&self) -> PathBuf {
        sidecar_path_for(&self.input)
    }

    pub fn sidecar_bytes(&self) -> Vec<u8> {
        fs::read(self.sidecar()).unwrap()
    }
}

#[allow(dead_code)] // not every parity test file uses every helper
pub fn bytes(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap()
}

/// Masks the two non-reproducible history fields so that two *processes* can be
/// compared: the `geometry-<ms>` / `upright-<ms>` history-entry ids and the
/// matching `recorded_at` millisecond value. Every occurrence is replaced, not
/// just the first.
#[allow(dead_code)] // not every parity test file uses every helper
pub fn mask_history(text: &str) -> String {
    fn mask_all(text: &mut String, marker: &str) {
        let mut cursor = 0usize;
        while cursor < text.len() {
            let Some(offset) = text[cursor..].find(marker) else {
                return;
            };
            let from = cursor + offset + marker.len();
            let length = text[from..]
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(text.len() - from);
            if length == 0 {
                // Already masked (or the marker is at the very end): skip past it
                // so the scan terminates.
                cursor = from;
                continue;
            }
            text.replace_range(from..from + length, "<TS>");
            cursor = from + "<TS>".len();
        }
    }
    let mut out = text.to_string();
    for marker in ["\"geometry-", "\"upright-"] {
        mask_all(&mut out, marker);
    }
    mask_all(&mut out, "\"recorded_at\": \"");
    out
}

/// Asserts that two sidecar files are byte-identical, with no mask at all.
///
/// This is the literal claim `spot` and `lens-blur` (read and write) make:
/// neither editor appends a history entry, so nothing in the file is
/// process-dependent and every byte must match. A single differing byte fails
/// the test.
#[allow(dead_code)] // not every parity test file uses every helper
pub fn assert_identical_sidecar(a: &Path, b: &Path, label: &str) {
    assert_eq!(
        bytes(a),
        bytes(b),
        "{label}: the two sidecars must be byte-identical, with no mask"
    );
}

/// Asserts that two sidecar files are byte-identical *after masking ONLY* the
/// millisecond history stamp — the `geometry-<ms>` / `upright-<ms>` history-entry
/// ids and the matching `recorded_at` value, which two processes cannot share.
/// Used only for the two editors that append such an entry.
///
/// The mask is applied to both sides, so any other difference still fails the
/// test, and the second assertion refuses a comparison where masking changed
/// nothing: such a pair is not a stamped comparison at all and belongs in
/// [`assert_identical_sidecar`].
#[allow(dead_code)] // not every parity test file uses every helper
pub fn assert_same_sidecar_ignoring_history_stamp(a: &Path, b: &Path, label: &str) {
    let left = bytes(a);
    let right = bytes(b);
    if left == right {
        return;
    }
    let left_text = String::from_utf8(left).expect("sidecar is utf-8");
    let right_text = String::from_utf8(right).expect("sidecar is utf-8");
    let masked_left = mask_history(&left_text);
    let masked_right = mask_history(&right_text);
    assert_eq!(
        masked_left, masked_right,
        "{label}: the two sidecars differ by more than the millisecond history stamp"
    );
    assert!(
        masked_left != left_text || masked_right != right_text,
        "{label}: masking changed nothing, so the difference is not a history stamp"
    );
}

/// The stage section of a stage payload, with the echoed `input` path and the
/// MCP envelope fields removed: those are transport facts, not stage state, and
/// the two fixtures live in different directories.
#[allow(dead_code)] // not every parity test file uses every helper
pub fn stage_section(payload: &Value, tool: &str) -> Value {
    let mut object = payload
        .as_object()
        .unwrap_or_else(|| panic!("{tool} payload is not an object: {payload}"))
        .clone();
    // `input` (absolute path), `revision`/`saved` (MCP envelope) and `actions`
    // (the actions of *this* call) are per-transport or per-call facts. The CLI
    // applies the same net state in one call where the MCP tool applies it in
    // several, so only the resulting state is compared.
    for key in ["input", "revision", "saved", "action", "actions"] {
        object.remove(key);
    }
    assert_eq!(
        object["command"],
        Value::String(tool.to_string()),
        "{tool} payload carries the wrong `command`"
    );
    Value::Object(object)
}

/// One rejected call, expressed on both transports.
///
/// `mcp_fragment` and `cli_fragment` are separate because the MCP layer can
/// catch a missing value one step earlier than the CLI (its schema is a JSON
/// object, the CLI's is a text field). What must be identical is the *verdict*:
/// both abort loudly and both leave the sidecar byte-identical.
#[allow(dead_code)] // only the two `*_errors*` files build a Case
pub struct Case {
    pub label: &'static str,
    pub mcp: Value,
    pub flags: &'static [&'static str],
    pub mcp_fragment: &'static str,
    pub cli_fragment: &'static str,
}
