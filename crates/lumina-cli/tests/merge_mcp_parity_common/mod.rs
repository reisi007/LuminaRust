//! MCP-PARITY-C: the shared harness of the CLI↔MCP byte-parity tests
//! (`merge_mcp_parity.rs` = digest/checksum/DNG/sidecar/pano/cache/force,
//! `merge_mcp_parity_errors.rs` = the loud rejections).
//!
//! # What is actually compared
//!
//! Not "structurally" and not "semantically": the real `lumina-cli` process
//! (`env!("CARGO_BIN_EXE_lumina-cli")`) runs `merge-hdr` / `merge-pano` on one
//! fixture tree, the MCP tool runs in-process on a *second, byte-identical*
//! tree, and then the results are compared — the `--json` /
//! `structuredContent` document field for field, the DNG byte for byte, the
//! merge sidecar byte for byte, the refusal text clause for clause. That is the
//! strongest statement available that two transports produce one artifact and
//! not two that merely resemble each other.
//!
//! # Where the compare lives, and why it is NOT in `lumina-mcp/tests/`
//!
//! SOLL §10.1 lists `crates/lumina-mcp/tests/merge_parity.rs`. A byte-parity
//! claim needs the CLI *result*, and `CARGO_BIN_EXE_lumina-cli` is injected by
//! Cargo only for the package that declares that binary — the variable does not
//! exist in a `lumina-mcp` integration test. Both ways around it were rejected:
//!
//! * building a path to `target/{debug,release}/lumina-cli` out of
//!   `CARGO_MANIFEST_DIR` — it depends on the build profile and on nobody ever
//!   setting `CARGO_TARGET_DIR`; a harness that has to *guess* where the
//!   reference lives is not a reference;
//! * a dev-dependency `lumina-mcp -> lumina-cli` — a package cycle Cargo
//!   rejects, and it would not supply the variable anyway.
//!
//! This repo already solved the problem for the two preceding parity slices:
//! `library_parity_common/mod.rs` (MCP-PARITY-B) and `stage_parity_common/mod.rs`
//! (MCP-PARITY-A) hold the CLI side **inside** `crates/lumina-cli/tests/`, where
//! `CARGO_BIN_EXE_lumina-cli` resolves and `lumina-mcp` is reachable as an
//! ordinary default-feature dependency of `lumina-cli`
//! (`lumina-cli/Cargo.toml:41,54-56`). This harness mirrors that pattern instead
//! of inventing a second one. The baseline-locked `tests/merge_e2e.rs` and
//! `tests/merge_parity.rs` stay untouched; what lives in the MCP crate instead
//! is the MCP-surface half of the matrix (`crates/lumina-mcp/tests/
//! merge_tools.rs`, `merge_tools_validation.rs`), which is where a
//! `lumina-mcp`-only claim belongs.
//!
//! # Why two trees
//!
//! Both documents echo absolute paths (`output`, `sidecar`), so a single tree
//! would compare addresses instead of state. Two trees plus
//! [`graders::normalize_paths`] leave every other byte under comparison (the
//! same argument as `library_parity_common/mod.rs:22-24`).

#![allow(dead_code)]
// Each test binary uses a part of this harness; an unused helper here is a
// helper another file in this slice uses, not dead code.

/// The PNG fixtures and the byte helpers around them.
pub mod fixtures;
/// The payload/artifact/refusal graders.
pub mod graders;
/// One merge job, rendered onto both transports.
pub mod job;
