//! MCP-PARITY-C: the shared harness of the CLI↔MCP matrix byte-parity tests
//! (`matrix_mcp_parity.rs` = verify/expected-route/baseline/export/filter/
//! defaults/sidecar, `matrix_mcp_parity_errors.rs` = the loud rejections plus
//! the red run, `matrix_mcp_parity_gate.rs` = the Real-RAW gate T26).
//!
//! # What is actually compared
//!
//! Not "structurally" and not "semantically": the real `lumina-cli` process
//! (`env!("CARGO_BIN_EXE_lumina-cli")`) runs `matrix` on one fixture tree,
//! the MCP tool runs in-process on a *second, byte-identical* tree, and then
//! the results are compared — the `--json` / `structuredContent` document
//! field for field, the golden PNGs byte for byte, the full-resolution
//! exports byte for byte, the diff images byte for byte, the refusal text
//! clause for clause. That is the strongest statement available that two
//! transports produce one artifact and not two that merely resemble each
//! other.
//!
//! # Where the compare lives, and why it is NOT in `lumina-mcp/tests/`
//!
//! SOLL §10.1 lists the parity tests without pinning the crate. A byte-parity
//! claim needs the CLI *result*, and `CARGO_BIN_EXE_lumina-cli` is injected by
//! Cargo only for the package that declares that binary — the variable does
//! not exist in a `lumina-mcp` integration test. Both ways around it were
//! rejected:
//!
//! * building a path to `target/{debug,release}/lumina-cli` out of
//!   `CARGO_MANIFEST_DIR` — it depends on the build profile and on nobody ever
//!   setting `CARGO_TARGET_DIR`; a harness that has to *guess* where the
//!   reference lives is not a reference;
//! * a dev-dependency `lumina-mcp -> lumina-cli` — a package cycle Cargo
//!   rejects, and it would not supply the variable anyway.
//!
//! This repo already solved the problem for the three preceding parity slices:
//! `merge_mcp_parity_common/mod.rs` (MCP-PARITY-C merge) holds the CLI side
//! **inside** `crates/lumina-cli/tests/`, where `CARGO_BIN_EXE_lumina-cli`
//! resolves and `lumina-mcp` is reachable as an ordinary default-feature
//! dependency of `lumina-cli` (`lumina-cli/Cargo.toml:41,54-56`). This harness
//! mirrors that pattern instead of inventing a second one. The baseline-locked
//! `tests/matrix_e2e.rs` and `tests/matrix_paths.rs` stay untouched; what
//! lives in the MCP crate instead is the MCP-surface half
//! (`crates/lumina-mcp/tests/matrix_tools.rs`), which is where a
//! `lumina-mcp`-only claim belongs.
//!
//! # Why two trees
//!
//! Both documents echo absolute paths (`recipe_set`, `golden_dir`, `golden`,
//! `output`), so a single tree would compare addresses instead of state. Two
//! trees plus [`graders::normalize_roots`] leave every other byte under
//! comparison (the same argument as `merge_mcp_parity_common/mod.rs:42-47`).
//!
//! # The two masks, and only these (SOLL §3.1)
//!
//! 1. **`duration_ms`** (top level and per pair): wall-clock timings, dropped
//!    on both sides before the compare.
//! 2. **absolute paths** (`recipe_set`, `golden_dir`, `golden`, `output`, and
//!    any path echoed inside `message`): both roots rewritten to `<TREE>`.
//!
//! Everything else — `psnr_db` **strings**, `max_abs_diff`, `status`,
//! `message`, `route`, `route_reasons`, `expected_route` — is compared raw.
//! That is the place where a stub turns red (SOLL §10).

#![allow(dead_code)]
// Each test binary uses a part of this harness; an unused helper here is a
// helper another file in this slice uses, not dead code.

/// The synthetic anchor fixtures (64×48 gradient + minimal recipe set).
pub mod fixtures;
/// The report/refusal/byte graders.
pub mod graders;
/// The green premise every parity test starts from.
pub mod green;
/// One matrix run, rendered onto both transports.
pub mod job;
