//! Shared helpers for the MCP-PARITY-B parity tests
//! (`library_parity.rs` = byte identity in both directions,
//! `library_parity_errors.rs` = the loud rejections). Both test files drive the
//! real `lumina-cli` process and the in-process MCP server over two
//! byte-identical fixture trees.
//!
//! # What is actually compared
//!
//! Not a "structural" or "semantic" comparison: the real
//! `lumina-cli` process (`env!("CARGO_BIN_EXE_lumina-cli")`) runs against one
//! fixture tree and the MCP tool runs in-process against a *second,
//! byte-identical* tree. Then
//!
//! * **read direction** — the sidecar bytes are compared with a literal byte
//!   compare (a read must change nothing) **and** the MCP tool payload is
//!   compared against the CLI's `--json` document;
//! * **write direction** — the produced bytes are compared after the same
//!   operation on both sides: the sidecar for `collections`, `generative` and
//!   `regenerate`, the *moved* sidecar for `relocate`, and the whole produced
//!   tree for `smart-collections` (which is read-only).
//!
//! Two fixture trees because the CLI's `--json` document echoes the absolute
//! input path; the payload comparison normalises exactly that path and nothing
//! else.

use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_mcp::tools::{collections, generative, regenerate, relocate, smart_collections};
use lumina_mcp::{McpError, Server};
pub use lumina_sidecar::sidecar_path_for;
pub use serde_json::json;
pub use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The five path-based commands under test, as the CLI spells them.
#[allow(dead_code)] // not every parity test file uses every helper
pub const COMMANDS: &[&str] = &[
    "collections",
    "smart-collections",
    "relocate",
    "generative",
    "regenerate",
];

/// A 32x32 image with a real gradient plus a transparent-pixel corner.
///
/// The opaque gradient makes the Auto-Tone analysis and the exposure matching
/// derive non-trivial values; the transparent corner makes the generative
/// `auto_fill` role *required* after the lens stage, so the double-role
/// producer path is exercised instead of the "nothing to do" shortcut.
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
    ImageFrame::new(size, size, pixels)
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap()
}

#[allow(dead_code)] // not every parity test file uses every helper
pub fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

/// A smart-collection catalogue with two rules: one that matches every copy and
/// one that needs a keyword, so the report carries both a hit and a miss.
pub fn smart_catalog() -> String {
    serde_json::json!({
        "format": "lumina-smart-catalog",
        "version": 1,
        "collections": [
            {
                "id": "everything",
                "name": "Everything",
                "version": 1,
                "rule": { "op": "all" },
            },
            {
                "id": "tagged",
                "name": "Tagged",
                "version": 1,
                "rule": { "op": "keyword", "keyword": "parity" },
            },
        ],
    })
    .to_string()
}

/// The fixture PNG with exactly one pixel changed, so a caller can produce the
/// "source changed since the sidecar was written" state with a still-decodable
/// file (flipping a byte in the encoded stream would only corrupt the CRC).
pub fn mutated_png() -> Vec<u8> {
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
    // One pixel of the opaque body.
    let index = ((20 * size + 20) as usize) * 4;
    pixels[index] = pixels[index].wrapping_add(1);
    ImageFrame::new(size, size, pixels)
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap()
}

/// A `--repair-region` definition file (F-042-N1) whose region matches the
/// 32x32 fixture frame, so `dust-removal` can persist a `.lumina.zdata` bundle.
pub fn repair_region_file(input: &Path, dir: &Path) -> PathBuf {
    let definition = serde_json::json!({
        "id": "region-1",
        "kind": "dustremoval",
        "region_width": 32,
        "region_height": 32,
        "region_values": vec![0u16; 32 * 32],
        "replacement_path": input.to_str().unwrap(),
    });
    let path = dir.join("region.json");
    fs::write(&path, serde_json::to_string(&definition).unwrap()).unwrap();
    path
}

/// Writes `<dir>/input.png` plus its sidecar (via the real `lumina import`) and
/// the catalogue file next to it. Returns `(input, catalog)`.
pub fn fixture(dir: &Path) -> (PathBuf, PathBuf) {
    fs::create_dir_all(dir).unwrap();
    let input = dir.join("input.png");
    fs::write(&input, fixture_png()).unwrap();
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
    let catalog = dir.join("catalog.json");
    fs::write(&catalog, smart_catalog()).unwrap();
    (input, catalog)
}

/// Runs a CLI command and parses its `--json` document.
#[allow(dead_code)] // only the byte-identity file reads a `--json` report
pub fn cli_json(args: &[&str]) -> Value {
    let (value, _) = cli_json_with_code(args);
    value
}

/// Runs a CLI command, returning its parsed `--json` document **and** its exit
/// code. A command that prints nothing on `--json` yields `Value::Null`.
pub fn cli_json_with_code(args: &[&str]) -> (Value, i32) {
    let output = cli().args(args).output().unwrap();
    let code = output.status.code().unwrap_or(-1);
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    let value = if text.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or_else(|error| {
            panic!(
                "`lumina {}` did not print one JSON document ({error}): {text}",
                args.join(" ")
            )
        })
    };
    (value, code)
}

/// Runs a CLI command that must abort, returning its stderr and exit code.
pub fn cli_fails(args: &[&str]) -> (String, i32) {
    let output = cli().args(args).output().unwrap();
    (
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

#[allow(dead_code)] // not every parity test file uses every helper
pub fn bytes(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap()
}

/// An in-process MCP server plus the fixture tree it addresses.
pub struct Session {
    pub server: Server,
    #[allow(dead_code)] // the payload normaliser takes the tree root explicitly
    pub dir: PathBuf,
    pub input: PathBuf,
    pub catalog: PathBuf,
}

impl Session {
    /// Builds the fixture tree and a server. The server is *not* given a loaded
    /// image on purpose: these five tools are path-based and must work without
    /// one, exactly like the other bulk tools.
    pub fn new(dir: &Path) -> Self {
        let (input, catalog) = fixture(dir);
        Self {
            server: Server::with_preview_dir(dir.join("previews")),
            dir: dir.to_path_buf(),
            input,
            catalog,
        }
    }

    pub fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, McpError> {
        match tool {
            "collections" => collections::run(&mut self.server, &arguments),
            "smart_collections" => smart_collections::run(&mut self.server, &arguments),
            "relocate" => relocate::run(&mut self.server, &arguments),
            "generative" => generative::run(&mut self.server, &arguments),
            "regenerate" => regenerate::run(&mut self.server, &arguments),
            other => panic!("no such library tool: {other}"),
        }
    }

    #[allow(dead_code)] // only the byte-identity file asserts on a success payload
    pub fn ok(&mut self, tool: &str, arguments: Value) -> Value {
        let label = arguments.to_string();
        self.call(tool, arguments)
            .unwrap_or_else(|error| panic!("{tool} {label} failed: {}", error.message()))
    }

    /// The tool's `path` argument pointing at this fixture's image.
    pub fn path(&self) -> String {
        self.input.display().to_string()
    }

    pub fn sidecar(&self) -> PathBuf {
        sidecar_path_for(&self.input)
    }

    pub fn sidecar_bytes(&self) -> Vec<u8> {
        bytes(&self.sidecar())
    }

    pub fn zdata(&self) -> PathBuf {
        lumina_sidecar::zdata_path_for(&self.input)
    }
}

/// Asserts that two produced files are byte-identical, **with no mask at all**.
///
/// This is the literal claim these five commands make: none of them appends a
/// `history` entry, so nothing in the file is process-dependent and every byte
/// must match. A single differing byte fails the test.
pub fn assert_identical(a: &Path, b: &Path, label: &str) {
    assert_eq!(
        bytes(a),
        bytes(b),
        "{label}: the two files must be byte-identical, with no mask"
    );
}

/// Asserts that a read changed no byte on either side.
pub fn assert_unchanged(before: &[u8], after: &[u8], label: &str) {
    assert_eq!(
        before, after,
        "{label}: a read must not change a single byte"
    );
}

/// A stable `name -> (length, content fingerprint)` snapshot of every file under
/// `dir`, so a refused call can be proven to have changed nothing. A content
/// fingerprint, not a cryptographic hash: its only job is to notice a changed
/// byte.
pub fn snapshot(dir: &Path) -> Vec<(String, usize, u64)> {
    let mut out = Vec::new();
    collect(dir, dir, &mut out);
    out.sort();
    out
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, usize, u64)>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect(root, &path, out);
        } else {
            let bytes = fs::read(&path).unwrap();
            out.push((
                path.strip_prefix(root).unwrap().display().to_string(),
                bytes.len(),
                content_fingerprint(&bytes),
            ));
        }
    }
}

/// FNV-1a 64: a content fingerprint, not a cryptographic digest.
pub fn content_fingerprint(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

/// Both transports abort, and the fixture tree is byte-identical afterwards.
///
/// `before` is a snapshot taken before the two calls.
pub fn assert_both_refuse(
    label: &str,
    dir: &Path,
    before: &[(String, usize, u64)],
    cli: (String, i32),
    mcp: Result<Value, McpError>,
) {
    assert_ne!(cli.1, 0, "{label}: the CLI must exit non-zero");
    assert!(
        cli.0.contains("error:"),
        "{label}: the CLI must say why on stderr: {}",
        cli.0
    );
    let error = match mcp {
        Ok(value) => panic!("{label}: the MCP tool must refuse, got {value}"),
        Err(error) => error,
    };
    assert!(
        !error.message().is_empty(),
        "{label}: a refusal must carry a reason"
    );
    assert_eq!(
        *before,
        snapshot(dir),
        "{label}: a refused call must not change a single file"
    );
}

/// Rewrites every occurrence of the two fixture roots with the same placeholder,
/// so the compared payloads differ only in state, not in the address of the tree
/// they were produced in. **Only the roots are rewritten** — every other byte of
/// the payload is compared literally.
pub fn normalize_paths(payload: &Value, from: &Path, to: &Path) -> Value {
    let text = serde_json::to_string(payload).unwrap();
    let replaced = text
        .replace(&from.display().to_string(), "<TREE>")
        .replace(&to.display().to_string(), "<TREE>");
    serde_json::from_str(&replaced).unwrap_or_else(|error| {
        panic!("payload is not valid JSON after path normalisation ({error}): {replaced}")
    })
}

/// Compares an MCP payload against a CLI `--json` document, normalising only the
/// fixture roots and the two MCP envelope fields (`saved`, `action`) that the
/// CLI document has no counterpart for.
pub fn assert_same_state(mcp: &Value, cli: &Value, from: &Path, to: &Path, label: &str) {
    let mut mcp_value = normalize_paths(mcp, from, to);
    let cli_value = normalize_paths(cli, from, to);
    if let Some(object) = mcp_value.as_object_mut() {
        // The MCP envelope has no CLI counterpart: `saved` (was this a write),
        // `action` (the actions of *this* call) and `text` (the human line the
        // CLI prints without `--json`). `status` is present in both documents
        // with the same meaning and is therefore compared.
        for key in ["saved", "action", "text"] {
            object.remove(key);
        }
    }
    assert_eq!(
        mcp_value, cli_value,
        "{label}: the MCP payload must render the same state as the CLI document"
    );
}
