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
//!
//! # MCP-PARITY-B: the path-based and artefact commands
//!
//! The same one-implementation rule extends to the five remaining counted gaps
//! `collections`, `smart-collections`, `relocate`, `generative` and
//! `regenerate`. They are **path-based** (one call = one path, no `image_id`,
//! the existing bulk-tool pattern) or artefact-based, and they live here too:
//!
//! | command | module | CLI adapter | MCP tool |
//! | --- | --- | --- | --- |
//! | `collections` | [`collections`] | `crates/lumina-cli/src/library.rs` | `lumina_collections` |
//! | `smart-collections` | [`smart_collections`] | `crates/lumina-cli/src/library.rs` | `lumina_smart_collections` |
//! | `relocate` | [`relocate`] | `crates/lumina-cli/src/library.rs` | `lumina_relocate` |
//! | `generative` | [`generative`] | `crates/lumina-cli/src/library.rs` | `lumina_generative` |
//! | `regenerate` | [`regenerate`] | `crates/lumina-cli/src/library.rs` | `lumina_regenerate` |
//!
//! [`generative`] and [`regenerate`] stay behind their artefact gates: when the
//! model/artefact a module needs cannot be produced or resolved, both
//! transports abort with the **same** error and write no sidecar bytes — never a
//! faked result. [`auto_tone`] is here for the same reason as in the CLI: the
//! `regenerate --module auto-tone` freshness predicate and the single write path
//! are one function, so a second copy in the MCP layer could not stay
//! consistent with the writer.

pub mod auto_tone;
pub mod collections;
pub mod copy;
pub mod decode;
pub mod error;
pub mod generative;
pub mod generative_artifact;
pub mod generative_status;
pub mod geometry;
pub mod geometry_fields;
pub mod lens_blur;
pub mod paths;
pub mod pipeline;
pub mod regenerate;
pub mod relocate;
pub mod report;
pub mod smart_collections;
pub mod spot;
pub mod spot_ops;
#[cfg(test)]
mod tests;
pub mod upright;

pub use error::StageError;
pub use generative::GenerativeRequest;
pub use geometry::GeometryRequest;
pub use lens_blur::LensBlurRequest;
pub use regenerate::{RegenerateModule, RegenerateRequest, RegenerateRun};
pub use report::{BulkReport, BulkRun, Persist, StageReport, StageRun};
pub use spot::SpotRequest;
pub use upright::UprightRequest;
