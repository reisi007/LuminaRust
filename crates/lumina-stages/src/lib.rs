//! `lumina-stages` — the ONE implementation of the four session-based recipe
//! stage editors `spot`, `lens-blur`, `geometry` and `upright`.
//!
//! # Why this crate exists (MCP-PARITY-A)
//!
//! The CLI already implements these four stage editors. The MCP server must
//! expose the *same* editors, and "the same" has to be structural, not a
//! promise: a second copy of the mutation, validation and listing logic would
//! drift from the CLI and the byte-identity requirement (an MCP call and the
//! equivalent CLI call leave byte-identical sidecars) could not be met.
//!
//! `lumina-cli` depends on `lumina-mcp` (the `mcp` feature, on by default), so
//! `lumina-mcp` cannot depend on `lumina-cli` — Cargo rejects the package
//! cycle. The shared logic therefore lives here, in a third crate that both
//! sides depend on:
//!
//! | caller | entry point |
//! | --- | --- |
//! | `lumina-cli` | `spot()` / `lens_blur()` / `geometry()` / `upright()` in `crates/lumina-cli/src/stages.rs` |
//! | `lumina-mcp` | `lumina_spot` / `lumina_lens_blur` / `lumina_geometry` / `lumina_upright` in `crates/lumina-mcp/src/tools/` |
//!
//! Both callers build a `*Request` from their own transport (`clap` flags vs.
//! a JSON-Schema-validated tool call), call [`spot::run`] / [`lens_blur::run`] /
//! [`geometry::run`] / [`upright::run`], and then render the returned
//! [`report::StageReport`]. The report carries the *exact* `--json` document the
//! CLI prints plus the *exact* human-readable lines, so the two transports
//! cannot format the same state differently either.
//!
//! # Contracts preserved verbatim
//!
//! Everything that used to live in `lumina-cli/src/main.rs` and
//! `lumina-cli/src/spot_ops.rs` is here unchanged: the same conflict matrices,
//! the same error texts, the same `document.validate()` gate before any write,
//! the same atomic `save_sidecar`, the same history entries, the same
//! `lumina_core` detectors/analyzers, and the same `info!` log lines. A caller
//! that used an out-of-range value, an unknown copy, an unknown field, an
//! inverted focal range or a non-portable artifact path still aborts loudly and
//! still writes zero bytes.

pub mod copy;
pub mod decode;
pub mod error;
pub mod geometry;
pub mod geometry_fields;
pub mod lens_blur;
pub mod report;
pub mod spot;
pub mod spot_ops;
#[cfg(test)]
mod tests;
pub mod upright;

pub use error::StageError;
pub use geometry::GeometryRequest;
pub use lens_blur::LensBlurRequest;
pub use report::{Persist, StageReport, StageRun};
pub use spot::SpotRequest;
pub use upright::UprightRequest;
