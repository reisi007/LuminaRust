//! `lumina_merge_pano` — overlapping frames → one linear panorama DNG, as an
//! MCP tool (MCP-PARITY-C).
//!
//! Mirrors `lumina merge-pano`: the call goes through the shared core
//! [`merge_common::run_mode`] onto the same
//! [`lumina_merge::bundle::run_merge`] orchestration and the same
//! `lumina_stages::merge` decode + exposure adapter the CLI uses, so the merge
//! digest, the DNG checksum and the error classes are transport-independent.
//! Path-based: one call is one merge job over `inputs`, no `image_id`, no
//! session state.
//!
//! Panorama pixels never use exposure: `exposure_times`/`isos`/`f_numbers` are
//! stored as provenance only, and a source without EXIF exposure falls back to
//! the documented neutral defaults (0.01 s / ISO 100 / f/8) — a panorama merge
//! without any exposure list succeeds. A stale bundle is a loud `merge stale:`
//! (`MergeStale`) unless `force: true` re-merges.

use crate::error::McpError;
use crate::tools::merge_common;
use crate::Server;
use lumina_sidecar::MergeMode;
use serde_json::Value;

pub const NAME: &str = "lumina_merge_pano";
pub const DESCRIPTION: &str = "Merge overlapping frames into one linear panorama DNG \
(`lumina merge-pano` twin). Path-based: `inputs` are the source paths in merge order (first = \
reference, 2..=256 sources), one call is one merge job. Writes the merge DNG plus its \
`.lumina.json` merge sidecar next to it and returns the CLI's own JSON fields (`command`, \
`output`, `sidecar`, `digest`, `checksum`, `cached`, `status`, plus `warning` when the alignment \
residual exceeds the documented threshold). Exposure is provenance only: the explicit \
`exposure_times`/`isos`/`f_numbers` lists (one value per input) or RAW EXIF are recorded, and a \
source without either falls back to the neutral defaults — a panorama merge without any exposure \
list succeeds. An existing bundle that no longer matches the sources fails loudly \
(`merge stale:`) unless `force: true`. Argument shape errors are `InvalidParams`; every \
merge-domain rejection keeps the CLI's own error class and prefix text.";

pub fn schema() -> Value {
    merge_common::schema(MergeMode::Panorama)
}

pub fn run(_server: &mut Server, args: &Value) -> Result<Value, McpError> {
    merge_common::run_mode(MergeMode::Panorama, args)
}
