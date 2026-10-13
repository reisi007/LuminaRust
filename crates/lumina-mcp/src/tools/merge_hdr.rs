//! `lumina_merge_hdr` — exposure-bracketed frames → one linear DNG, as an MCP
//! tool (MCP-PARITY-C).
//!
//! Mirrors `lumina merge-hdr`: the call goes through the shared core
//! [`merge_common::run_mode`] onto the same
//! [`lumina_merge::bundle::run_merge`] orchestration and the same
//! `lumina_stages::merge` decode + exposure adapter the CLI uses, so the merge
//! digest, the DNG checksum and the error classes are transport-independent.
//! Path-based: one call is one merge job over `inputs`, no `image_id`, no
//! session state.
//!
//! Exposure is never guessed from pixels: explicit `exposure_times`/`isos`/
//! `f_numbers` win, RAW EXIF is the fallback, and a source without either is a
//! loud `merge unsupported:` (`MergeUnsupported`). A stale bundle is a loud
//! `merge stale:` (`MergeStale`) unless `force: true` re-merges.

use crate::error::McpError;
use crate::tools::merge_common;
use crate::Server;
use lumina_sidecar::MergeMode;
use serde_json::Value;

pub const NAME: &str = "lumina_merge_hdr";
pub const DESCRIPTION: &str = "Merge an exposure-bracketed frame set into one linear DNG \
(`lumina merge-hdr` twin). Path-based: `inputs` are the source paths in merge order (first = \
reference, 2..=256 sources), one call is one merge job. Writes the merge DNG plus its \
`.lumina.json` merge sidecar next to it and returns the CLI's own JSON fields (`command`, \
`output`, `sidecar`, `digest`, `checksum`, `cached`, `status`, plus `warning` when the alignment \
residual exceeds the documented threshold). Exposure comes from the explicit \
`exposure_times`/`isos`/`f_numbers` lists (one value per input) or RAW EXIF — never guessed from \
pixels; a source without either fails loudly (`merge unsupported:`). An existing bundle that no \
longer matches the sources fails loudly (`merge stale:`) unless `force: true`. Argument shape \
errors are `InvalidParams`; every merge-domain rejection keeps the CLI's own error class and \
prefix text.";

pub fn schema() -> Value {
    merge_common::schema(MergeMode::Hdr)
}

pub fn run(_server: &mut Server, args: &Value) -> Result<Value, McpError> {
    merge_common::run_mode(MergeMode::Hdr, args)
}
