//! `clap` argument surface of the `lumina` CLI commands whose handlers live
//! in `main.rs`-sibling slices.
//!
//! Extracted verbatim from `main.rs` for the file-size ratchet
//! (FILE-SIZE-1000-ALL-EXCEPTIONS-100, argument slice). Pure move: no type was
//! renamed and no `#[arg]`/`#[command]` attribute, value name or help text was
//! changed. Only the visibility tokens (`pub(crate)` on the types, their fields
//! and `CliMaskPolicy::to_policy`), the `use` line and this header are new.
//!
//! Every handler module and the in-crate tests reach these types through
//! `use super::*` in the crate root, so the former flat `crate::…`/`super::…`
//! paths stay valid.

use super::*;
#[derive(Debug, Clone, Args)]
pub(crate) struct FileArgs {
    #[arg(long)]
    pub(crate) input: PathBuf,
    #[arg(long)]
    pub(crate) output: Option<PathBuf>,
    #[arg(long, default_value = "png")]
    pub(crate) format: String,
    #[arg(long, default_value_t = 90)]
    pub(crate) quality: u8,
    #[arg(long)]
    pub(crate) json: bool,
    #[arg(long)]
    pub(crate) migrate: bool,
    #[arg(long)]
    pub(crate) force_render: bool,
    #[arg(long)]
    pub(crate) virtual_copy: Option<String>,
    /// How missing or invalid mask artifacts are handled when rendering
    /// (REVIEW-CLI-EXPORTMASK-1): `warn` warns and continues (the harmonized
    /// default for every render-capable subcommand), `strict` aborts.
    #[arg(long, value_enum, default_value = "warn")]
    pub(crate) mask_policy: CliMaskPolicy,
}

#[derive(Debug, Args)]
pub(crate) struct DevelopArgs {
    #[arg(long)]
    pub(crate) input: PathBuf,
    #[arg(long)]
    pub(crate) virtual_copy: Option<String>,
    #[arg(long)]
    pub(crate) exposure: Option<f64>,
    #[arg(long)]
    pub(crate) contrast: Option<f64>,
    /// LRPAR-G01-BASIC: Develop treatment (`color` | `bw`). `bw` stashes the
    /// current saturation/vibrance and desaturates via the shared
    /// `apply_treatment` path; `color` restores the stash exactly.
    #[arg(long, value_name = "color|bw")]
    pub(crate) treatment: Option<String>,
    /// LRPAR-G01-BASIC: Develop profile (one of the normative
    /// `DEVELOP_PROFILES`; absent = `default`).
    #[arg(long, value_name = "PROFILE")]
    pub(crate) profile: Option<String>,
    #[arg(long)]
    pub(crate) update_masks: bool,
    #[arg(long)]
    pub(crate) migrate: bool,
    #[arg(long)]
    pub(crate) json: bool,
}

/// R2-CLI-10: slim argument set for `import` — only the flags the command
/// actually consumes. Import writes/validates a sidecar; it never renders, so
/// render-only flags would be silently ignored (see [`Command::Import`]).
#[derive(Debug, Clone, Args)]
pub(crate) struct ImportArgs {
    #[arg(long)]
    pub(crate) input: PathBuf,
    #[arg(long)]
    pub(crate) json: bool,
    #[arg(long)]
    pub(crate) migrate: bool,
}

#[derive(Debug, Args)]
pub(crate) struct ExportArgs {
    #[arg(long)]
    pub(crate) input: PathBuf,
    #[arg(long)]
    pub(crate) output: PathBuf,
    #[arg(long, default_value = "png")]
    pub(crate) format: String,
    #[arg(long, default_value_t = 90)]
    pub(crate) quality: u8,
    #[arg(long)]
    pub(crate) virtual_copy: Option<String>,
    #[arg(long)]
    pub(crate) update_masks: bool,
    #[arg(long)]
    pub(crate) force_render: bool,
    #[arg(long)]
    pub(crate) migrate: bool,
    #[arg(long)]
    pub(crate) json: bool,
    /// REVIEW-CLI-EXPORTMASK-1: harmonized stale-mask behaviour. Default
    /// `warn` continues with a warning (like render/batch/process); `strict`
    /// aborts before anything is decoded or written.
    #[arg(long, value_enum, default_value = "warn")]
    pub(crate) mask_policy: CliMaskPolicy,
    /// LRPAR-G15-IPTC-S6: opt-in IPTC bake-in (JPEG only, IIM+XMP from the
    /// sidecar draft + keywords, SOLL §7). Without the flag the export stays
    /// exactly as today (no metadata, no silent assumptions).
    #[arg(long)]
    pub(crate) write_metadata: bool,
}

#[derive(Debug, Args)]
pub(crate) struct BatchArgs {
    #[arg(long)]
    pub(crate) input: PathBuf,
    #[arg(long)]
    pub(crate) output: PathBuf,
    #[arg(long, default_value_t = 1)]
    pub(crate) jobs: usize,
    #[arg(long, default_value_t = 1)]
    pub(crate) retry: u32,
    #[arg(long)]
    pub(crate) resume: bool,
    #[arg(long)]
    pub(crate) dry_run: bool,
    #[arg(long)]
    pub(crate) update_masks: bool,
    #[arg(long)]
    pub(crate) force_render: bool,
    #[arg(long)]
    pub(crate) json: bool,
    #[arg(long, default_value = "png")]
    pub(crate) format: String,
    #[arg(long, default_value_t = 90)]
    pub(crate) quality: u8,
    #[arg(long)]
    pub(crate) virtual_copy: Option<String>,
    /// Same harmonized stale-mask behaviour as export/render (default warn).
    #[arg(long, value_enum, default_value = "warn")]
    pub(crate) mask_policy: CliMaskPolicy,
    /// LRPAR-G15-IPTC-S6: opt-in IPTC bake-in (JPEG only, IIM+XMP from the
    /// sidecar draft + keywords, SOLL §7). PNG/WebP items with the flag fail
    /// loudly per file (item `failed` with reason). Without the flag the batch
    /// stays exactly as today.
    #[arg(long)]
    pub(crate) write_metadata: bool,
}

/// G-03 Masking parity: inspect and edit the mask DAG of one image sidecar.
/// Reads and writes are loud (unknown copies/masks, bad ranges, arity/cycle
/// violations abort with exit 1); nothing is inferred silently. New mask ids
/// are stable (`mask-<blake3>` over kind + name), never positional.
#[derive(Debug, Args)]
pub(crate) struct MaskArgs {
    #[arg(long)]
    pub(crate) input: PathBuf,
    #[arg(long)]
    pub(crate) update_masks: bool,
    #[arg(long)]
    pub(crate) virtual_copy: Option<String>,
    #[arg(long)]
    pub(crate) json: bool,
    /// List every mask (library + layers) with its status.
    #[arg(long)]
    pub(crate) list: bool,
    /// Add an AI-select source mask (`subject|sky|background|objects|people`).
    #[arg(long, value_name = "KIND")]
    pub(crate) add_ai_select: Option<String>,
    /// Name for `--add-*`, `--combine` and `--duplicate` targets.
    #[arg(long, value_name = "NAME")]
    pub(crate) name: Option<String>,
    /// Optional person/object part for `--add-ai-select`
    /// (`face|hair|eyes|pupil|sclera|lips|teeth|skin|body`, …).
    #[arg(long, value_name = "PART")]
    pub(crate) detail: Option<String>,
    /// Add a deterministic luminance-range source mask.
    #[arg(long)]
    pub(crate) add_luminance_range: bool,
    #[arg(long, value_name = "0..=1")]
    pub(crate) range_min: Option<f32>,
    #[arg(long, value_name = "0..=1")]
    pub(crate) range_max: Option<f32>,
    /// Add a deterministic color-range source mask.
    #[arg(long)]
    pub(crate) add_color_range: bool,
    #[arg(long, value_name = "0..=360")]
    pub(crate) hue_center: Option<f32>,
    #[arg(long, value_name = "0..=360")]
    pub(crate) hue_width: Option<f32>,
    #[arg(long, value_name = "0..=1")]
    pub(crate) sat_min: Option<f32>,
    #[arg(long, value_name = "0..=1")]
    pub(crate) sat_max: Option<f32>,
    #[arg(long, value_name = "0..=1")]
    pub(crate) lum_min: Option<f32>,
    #[arg(long, value_name = "0..=1")]
    pub(crate) lum_max: Option<f32>,
    /// Feather for `--add-*-range` (`0..=1`, default 0).
    #[arg(long, value_name = "0..=1")]
    pub(crate) feather: Option<f32>,
    /// Combine existing masks into a derived node
    /// (`union|intersect|subtract|invert`).
    #[arg(long, value_name = "OP")]
    pub(crate) combine: Option<String>,
    /// Combine/duplicate inputs as `mask-id` (same copy) or
    /// `copy-id/mask-id`, comma-separated for `--combine`.
    #[arg(long, value_name = "REF,...")]
    pub(crate) inputs: Option<String>,
    /// Duplicate an existing mask under a new name (`--name` required).
    #[arg(long, value_name = "REF")]
    pub(crate) duplicate: Option<String>,
    /// Attach an existing library mask to the target copy's layers.
    #[arg(long, value_name = "REF")]
    pub(crate) attach_layer: Option<String>,
    /// Set a layer visible (eye open) by layer id.
    #[arg(long, value_name = "LAYER")]
    pub(crate) show_layer: Option<String>,
    /// Set a layer invisible (eye closed) by layer id.
    #[arg(long, value_name = "LAYER")]
    pub(crate) hide_layer: Option<String>,
    /// Target layer for the P0 local-adjustment flags. If omitted, a copy with
    /// exactly one layer is accepted; ambiguous copies fail loudly.
    #[arg(long, value_name = "LAYER")]
    pub(crate) local_layer: Option<String>,
    /// Set one local control (`exposure|contrast|highlights|shadows|
    /// temperature_delta_k|tint_delta=value`; local tone curves use the
    /// `curves.<master|red|green|blue>=I,O;I,O;...` namespace). Repeatable.
    #[arg(long = "set-local-adjustment", value_name = "KEY=VALUE")]
    pub(crate) set_local_adjustments: Vec<String>,
    /// Reset one local control (`curves` resets all local curves). Repeatable.
    #[arg(long = "reset-local-adjustment", value_name = "KEY")]
    pub(crate) reset_local_adjustments: Vec<String>,
}

#[derive(Debug, Args)]
pub(crate) struct IndexArgs {
    #[arg(long)]
    pub(crate) input: PathBuf,
    #[arg(long)]
    pub(crate) json: bool,
    #[arg(long)]
    pub(crate) migrate: bool,
}

/// G-15 META-MVP (Slice 2): list (`--add`/`--remove` absent) or mutate
/// source-level keywords of one image sidecar. Mutations apply in flag order
/// as `BatchOp::{AddKeyword, RemoveKeyword}` via `apply_batch_op`.
#[derive(Debug, Args)]
pub(crate) struct KeywordsArgs {
    #[arg(long)]
    pub(crate) input: PathBuf,
    /// Keywords to add (repeatable, applied in order).
    #[arg(long = "add")]
    pub(crate) add: Vec<String>,
    /// Keywords to remove (repeatable, applied in order after `--add`).
    #[arg(long = "remove")]
    pub(crate) remove: Vec<String>,
    #[arg(long)]
    pub(crate) json: bool,
}

/// G-15 META-MVP (Slice 2): apply exactly one `BatchOp` (JSON in the
/// `BatchOp` serde form) over every sidecar found under `--input`.
/// Exactly one of `--op` / `--op-file` is required.
#[derive(Debug, Args)]
pub(crate) struct BatchMetaArgs {
    #[arg(long)]
    pub(crate) input: PathBuf,
    /// The batch operation as inline JSON (e.g.
    /// `{"op":"add_keyword","keyword":"portrait"}`).
    #[arg(long)]
    pub(crate) op: Option<String>,
    /// Path to a file containing the batch operation JSON.
    #[arg(long)]
    pub(crate) op_file: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

/// LRPAR-G15-IPTC-S3: IPTC metadata draft commands (`meta inspect`,
/// `meta draft set|clear`, `meta history show|clear`). Normative contract:
/// `feature/product/iptc-metadata.md` §8.
#[derive(Debug, Args)]
pub(crate) struct MetaArgs {
    #[command(subcommand)]
    pub(crate) command: MetaCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum MetaCommand {
    /// Show embedded IPTC of the source (JPEG IIM/XMP; other formats report
    /// loudly "nicht verfügbar"), the draft overlay per field (draft vs.
    /// embedded), keywords and the history length. Read-only.
    Inspect(MetaInspectArgs),
    /// Mutate the source-level draft (`--field <id>=<wert>`, repeatable).
    Draft(MetaDraftArgs),
    /// Show or explicitly clear the draft history (diagnostic context, no undo).
    History(MetaHistoryArgs),
    /// Apply file-backed IPTC metadata presets (`<name>.lumina-meta-preset.json`,
    /// static + dynamic with `{placeholder}` variables).
    Preset(MetaPresetArgs),
    /// Copy selected draft fields (+ keywords) from one source image onto N
    /// targets (field-selective, mirror semantics). See [`MetaSyncArgs`].
    Sync(MetaSyncArgs),
    /// META-COPYPASTE-1: copy non-empty draft fields (+ keywords) into an
    /// explicit clipboard file (no sidecar mutation, no shared state).
    Copy(MetaCopyArgs),
    /// META-COPYPASTE-1: paste clipboard fields onto N targets through the
    /// normal sidecar commit path (additive; never deletes other fields).
    Paste(MetaPasteArgs),
}

#[derive(Debug, Args)]
pub(crate) struct MetaInspectArgs {
    /// Source image whose sidecar draft and embedded IPTC are shown.
    pub(crate) path: PathBuf,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct MetaDraftArgs {
    #[command(subcommand)]
    pub(crate) command: MetaDraftCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum MetaDraftCommand {
    /// Set draft fields (`--field <id>=<wert>`, repeatable). `keywords`
    /// routes onto the existing source-level keyword field and replaces the
    /// whole list (one `--field keywords=<eintrag>` per entry; an empty value
    /// clears the list). Unknown IDs/invalid values abort loudly with
    /// all-or-nothing semantics (nothing written).
    Set(MetaDraftSetArgs),
    /// Remove draft fields: `--field <id,…>` (comma-separated, `keywords`
    /// allowed) or `--all` (empties the draft; the history is kept and gains
    /// one entry). Exactly one of both is required.
    Clear(MetaDraftClearArgs),
}

#[derive(Debug, Args)]
pub(crate) struct MetaDraftSetArgs {
    /// Source image whose sidecar draft is mutated.
    pub(crate) path: PathBuf,
    /// Draft assignment as `<id>=<wert>` (repeatable, split at the first `=`).
    #[arg(long = "field", required = true, value_name = "ID=VALUE")]
    pub(crate) field: Vec<String>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct MetaDraftClearArgs {
    /// Source image whose sidecar draft is mutated.
    pub(crate) path: PathBuf,
    /// Draft field IDs to remove (comma-separated, repeatable;
    /// `keywords` clears the keyword list).
    #[arg(long, value_delimiter = ',')]
    pub(crate) field: Vec<String>,
    /// Remove every draft field (history is kept and gains one entry).
    #[arg(long)]
    pub(crate) all: bool,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct MetaHistoryArgs {
    #[command(subcommand)]
    pub(crate) command: MetaHistoryCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum MetaHistoryCommand {
    /// List history entries (newest first), optionally limited.
    Show(MetaHistoryShowArgs),
    /// Explicitly clear the whole history (draft values are kept).
    Clear(MetaHistoryClearArgs),
}

#[derive(Debug, Args)]
pub(crate) struct MetaHistoryShowArgs {
    /// Source image whose sidecar history is shown.
    pub(crate) path: PathBuf,
    /// Maximum number of entries (newest first).
    #[arg(long)]
    pub(crate) limit: Option<usize>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct MetaHistoryClearArgs {
    /// Source image whose sidecar history is cleared.
    pub(crate) path: PathBuf,
    #[arg(long)]
    pub(crate) json: bool,
}

/// LRPAR-G15-IPTC-S4: file-backed IPTC metadata presets (`preset list|show|
/// apply`). Normative contract: `feature/product/iptc-metadata.md` §5.
#[derive(Debug, Args)]
pub(crate) struct MetaPresetArgs {
    #[command(subcommand)]
    pub(crate) command: MetaPresetCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum MetaPresetCommand {
    /// List `<name>.lumina-meta-preset.json` files in `[dir]` (default: the
    /// user-global presets directory). Broken files are reported loudly as
    /// failed entries, never skipped silently.
    List(MetaPresetListArgs),
    /// Show one preset: a display `<name>` (resolved against the user-global
    /// presets directory) or an explicit file path.
    Show(MetaPresetShowArgs),
    /// Apply one preset to N targets (`--target`, repeatable; `--var
    /// name=wert`, repeatable for dynamic presets). All placeholder variables
    /// are required upfront; missing/unknown variables and limit violations
    /// abort everything (exit 1, nothing written). Each target is then handled
    /// in isolation (updated / unchanged / failed, exit 3 on partial failure)
    /// with its own CAS + atomic write and `preset:<name>` history entry.
    Apply(MetaPresetApplyArgs),
}

#[derive(Debug, Args)]
pub(crate) struct MetaPresetListArgs {
    /// Presets directory to list (default: the user-global presets directory).
    #[arg(value_name = "DIR")]
    pub(crate) dir: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct MetaPresetShowArgs {
    /// Preset display name or explicit `.lumina-meta-preset.json` file path.
    #[arg(value_name = "NAME|PATH")]
    pub(crate) preset: String,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct MetaPresetApplyArgs {
    /// Preset display name or explicit `.lumina-meta-preset.json` file path.
    #[arg(value_name = "NAME|PATH")]
    pub(crate) preset: String,
    /// Target image(s) receiving the preset draft (repeatable, min 1).
    #[arg(long, required = true, value_name = "TARGET")]
    pub(crate) target: Vec<PathBuf>,
    /// Placeholder variable as `name=wert` (repeatable; split at the first
    /// `=`; duplicate names are rejected loudly).
    #[arg(long = "var", value_name = "NAME=VALUE")]
    pub(crate) var: Vec<String>,
    #[arg(long)]
    pub(crate) json: bool,
}

/// LRPAR-G15-IPTC-S5: field-selective draft/keyword transfer (`meta sync`).
/// `--fields` is deliberately NOT clap-`required`: a missing list must fail
/// loudly with exit 1 (like `meta draft clear`'s manual arity check), not
/// with clap's usage exit 2 and never with a silent transfer-all.
#[derive(Debug, Args)]
pub(crate) struct MetaSyncArgs {
    /// Source image whose draft (+ keywords) is the sync source.
    #[arg(long, value_name = "SOURCE")]
    pub(crate) source: PathBuf,
    /// Target image(s) receiving the selected fields (repeatable, min 1).
    #[arg(long, required = true, value_name = "TARGET")]
    pub(crate) target: Vec<PathBuf>,
    /// Draft field IDs to transfer (comma-separated, repeatable; registry IDs
    /// from SOLL §4, `keywords` allowed). Required — an empty list is a loud
    /// error (exit 1), never a silent transfer-all.
    #[arg(long, value_delimiter = ',', value_name = "ID,...")]
    pub(crate) fields: Vec<String>,
    #[arg(long)]
    pub(crate) json: bool,
}

/// META-COPYPASTE-1: `meta copy` — writes the selected non-empty draft fields
/// (+ non-empty keywords) of `path` into an explicit clipboard file. Copy is
/// read-only w.r.t. the sidecar and never removes or normalizes values.
#[derive(Debug, Args)]
pub(crate) struct MetaCopyArgs {
    /// Source image whose non-empty draft fields (+ keywords) are copied.
    pub(crate) path: PathBuf,
    /// Draft field IDs to copy (comma-separated, repeatable; registry IDs from
    /// SOLL §4, `keywords` allowed). Absent = every non-empty draft field plus
    /// non-empty keywords.
    #[arg(long, value_delimiter = ',', value_name = "ID,...")]
    pub(crate) fields: Vec<String>,
    /// Clipboard file to write (default: `<OS-Temp>/lumina-meta-clipboard.json`).
    #[arg(long, value_name = "FILE")]
    pub(crate) out: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

/// META-COPYPASTE-1: `meta paste` — applies clipboard fields to N targets over
/// the normal sidecar commit path (CAS, atomic, one `origin = "cli"` history
/// entry per changed target). Purely additive: only clipboard fields are
/// written, no other field is ever removed.
#[derive(Debug, Args)]
pub(crate) struct MetaPasteArgs {
    /// Clipboard file to read (default: the same path `meta copy` writes).
    #[arg(value_name = "CLIPBOARD")]
    pub(crate) clipboard: Option<PathBuf>,
    /// Target image(s) receiving the clipboard fields (repeatable, min 1).
    #[arg(long, required = true, value_name = "TARGET")]
    pub(crate) target: Vec<PathBuf>,
    /// Clipboard field IDs to apply (comma-separated, repeatable; must be a
    /// subset of the IDs stored in the clipboard). Absent = all stored fields.
    #[arg(long, value_delimiter = ',', value_name = "ID,...")]
    pub(crate) fields: Vec<String>,
    #[arg(long)]
    pub(crate) json: bool,
}
/// of `--from` onto every `--to` target sidecar (same full-recipe Sync
/// mechanism, one `previous` history step per target, per-target failures
/// isolated and loud). Non-neutral local-mask state is refused rather than
/// silently omitted or copied; no-local-mask recipes retain the historical
/// copy path.
#[derive(Debug, Args)]
pub(crate) struct PreviousArgs {
    /// Reference image whose virtual-copy recipe is the Previous source.
    #[arg(long)]
    pub(crate) from: PathBuf,
    /// Target image(s) receiving the reference recipe (repeatable, min 1).
    #[arg(long, required = true)]
    pub(crate) to: Vec<PathBuf>,
    /// Virtual copy id of the reference recipe (default `vc-original`).
    #[arg(long)]
    pub(crate) from_copy: Option<String>,
    /// Virtual copy id receiving the recipe on each target (default
    /// `vc-original`).
    #[arg(long)]
    pub(crate) to_copy: Option<String>,
    #[arg(long)]
    pub(crate) json: bool,
}

/// F-042-N1: persist a dust-removal (or AI-replacement) repair region into the
/// source's `.lumina.zdata` bundle and record it as a recipe source action.
/// The original image is never modified.
#[derive(Debug, Args)]
pub(crate) struct DustRemovalArgs {
    #[arg(long)]
    pub(crate) input: PathBuf,
    /// Path to a repair-region definition JSON (region plane + replacement image
    /// path). See `RepairRegionInput` for the schema.
    #[arg(long)]
    pub(crate) repair_region: PathBuf,
    #[arg(long)]
    pub(crate) virtual_copy: Option<String>,
    /// Optional path to render the frame with the action applied, so the effect
    /// is verifiable headlessly. Never equals `--input`.
    #[arg(long)]
    pub(crate) render_out: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

/// G-02 Color-Parität (LRPAR-G02-COLOR): inspect and edit the color stages
/// of one virtual copy (tone curve per channel, HSL mixer, Point Color,
/// color grading incl. luminance/blending, vibrance/saturation). Reads and
/// writes are loud (unknown copies/channels/fields/ids, bad ranges abort
/// with exit 1); the original image is never modified. See
/// `feature/architecture/pipeline.md` §§ F-089, F-090, F-090b, F-091.
#[derive(Debug, Args)]
pub(crate) struct ColorArgs {
    #[arg(long)]
    pub(crate) input: PathBuf,
    #[arg(long)]
    pub(crate) virtual_copy: Option<String>,
    #[arg(long)]
    pub(crate) json: bool,
    /// List curve/HSL/Point-Color/grading values (default when no mutation
    /// flag is given).
    #[arg(long)]
    pub(crate) list: bool,
    /// Set parametric curve regions as `CHANNEL:S,D,L,H` with
    /// `CHANNEL = master|red|green|blue` and four `-1..=1` deltas
    /// (repeatable).
    #[arg(long, value_name = "CHANNEL:S,D,L,H")]
    pub(crate) set_curve_param: Vec<String>,
    /// Set free curve points as `CHANNEL:I,O;I,O;...` with 2..=32 points,
    /// strictly ascending inputs and `(0,0)`/`(1,1)` endpoints (repeatable).
    #[arg(long, value_name = "CHANNEL:I,O;...")]
    pub(crate) set_curve_points: Vec<String>,
    /// Remove the whole tone-curve stage (all channels).
    #[arg(long)]
    pub(crate) clear_curves: bool,
    /// Remove one tone-curve channel (`master|red|green|blue`; master
    /// resets to identity, channel lists are dropped).
    #[arg(long, value_name = "CHANNEL")]
    pub(crate) clear_curve_channel: Option<String>,
    /// Set one HSL mixer field as `CHANNEL:FIELD:VALUE` with
    /// `CHANNEL = red|orange|yellow|green|cyan|blue|violet|magenta` and
    /// `FIELD = hue|saturation|luminance` (repeatable).
    #[arg(long, value_name = "CHANNEL:FIELD:VALUE")]
    pub(crate) set_hsl: Vec<String>,
    /// Remove the whole HSL mixer stage.
    #[arg(long)]
    pub(crate) clear_hsl: bool,
    /// Add a Point Color entry (takes `--hue-center/--hue-range/
    /// --hue-shift/--sat-shift/--lum-shift`; the id is the next stable
    /// `pc-<n>`).
    #[arg(long)]
    pub(crate) add_point_color: bool,
    /// Hue center for `--add-point-color` (`0..=360`, default 0).
    #[arg(long, value_name = "0..=360")]
    pub(crate) hue_center: Option<f32>,
    /// Hue range for `--add-point-color` (`0..=180`, default 30).
    #[arg(long, value_name = "0..=180")]
    pub(crate) hue_range: Option<f32>,
    /// Hue shift for `--add-point-color` (`-1..=1`, default 0).
    #[arg(long, value_name = "-1..=1")]
    pub(crate) hue_shift: Option<f32>,
    /// Saturation shift for `--add-point-color` (`-1..=1`, default 0).
    #[arg(long, value_name = "-1..=1")]
    pub(crate) sat_shift: Option<f32>,
    /// Luminance shift for `--add-point-color` (`-1..=1`, default 0).
    #[arg(long, value_name = "-1..=1")]
    pub(crate) lum_shift: Option<f32>,
    /// Set one Point Color field as `ID:FIELD:VALUE` with
    /// `FIELD = hue_center|hue_range|hue_shift|saturation_shift|
    /// luminance_shift` (repeatable).
    #[arg(long, value_name = "ID:FIELD:VALUE")]
    pub(crate) set_point_color: Vec<String>,
    /// Remove one Point Color entry by stable id (repeatable).
    #[arg(long, value_name = "ID")]
    pub(crate) remove_point_color: Vec<String>,
    /// Remove the whole Point Color stage (identity).
    #[arg(long)]
    pub(crate) clear_point_color: bool,
    /// Set one color-grading field as `RANGE:FIELD:VALUE` with
    /// `RANGE = shadows|midtones|highlights` and
    /// `FIELD = hue_degrees|saturation|luminance` (repeatable).
    #[arg(long, value_name = "RANGE:FIELD:VALUE")]
    pub(crate) set_grading: Vec<String>,
    /// Set the color-grading balance (`-1..=1`).
    #[arg(long, value_name = "-1..=1")]
    pub(crate) set_grading_balance: Option<f32>,
    /// Set the color-grading blending (`0..=1`, 0.5 = legacy edges).
    #[arg(long, value_name = "0..=1")]
    pub(crate) set_grading_blending: Option<f32>,
    /// Remove the whole color-grading stage.
    #[arg(long)]
    pub(crate) clear_grading: bool,
    /// Set vibrance (`-1..=1`).
    #[arg(long, value_name = "-1..=1")]
    pub(crate) set_vibrance: Option<f64>,
    /// Set saturation (`-1..=1`).
    #[arg(long, value_name = "-1..=1")]
    pub(crate) set_saturation: Option<f64>,
}

/// CLI-facing `--mask-policy` selection (REVIEW-CLI-EXPORTMASK-1). `warn` is
/// the harmonized default everywhere: missing or stale masks produce a warning
/// and the command continues; `strict` aborts the command with an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum CliMaskPolicy {
    /// Missing/stale masks warn; the command continues.
    Warn,
    /// Missing/stale masks abort the command.
    Strict,
}

impl CliMaskPolicy {
    pub(crate) fn to_policy(self) -> MaskPolicy {
        match self {
            Self::Warn => MaskPolicy::Warn,
            Self::Strict => MaskPolicy::Strict,
        }
    }
}

/// Parsed `*.status.json` resume marker written by `batch` (REVIEW-CLI-N3).
/// Resume decisions read this struct instead of substring-matching raw text.
#[derive(Debug, Deserialize)]
pub(crate) struct BatchStatusFile {
    #[serde(default)]
    pub(crate) status: String,
}

#[derive(Debug, Args)]
pub(crate) struct ProcessArgs {
    #[arg(long)]
    pub(crate) input: PathBuf,
    #[arg(long)]
    pub(crate) output: PathBuf,
    #[arg(long)]
    pub(crate) preset: Option<PathBuf>,
    #[arg(long)]
    pub(crate) exposure: Option<f64>,
    #[arg(long)]
    pub(crate) contrast: Option<f64>,
    /// AUTO-TONE-CLI-6: explicit value for the `whites` slider. Explicit CLI
    /// values win last, for all six auto sliders; the `auto_features` mirror
    /// keeps the auto value while `adjustments` carries the effective one.
    #[arg(long)]
    pub(crate) whites: Option<f64>,
    /// AUTO-TONE-CLI-6: explicit value for the `blacks` slider (see `--whites`).
    #[arg(long)]
    pub(crate) blacks: Option<f64>,
    #[arg(long)]
    pub(crate) highlights: Option<f64>,
    #[arg(long)]
    pub(crate) shadows: Option<f64>,
    #[arg(long)]
    pub(crate) auto_tone: bool,
    #[arg(long)]
    pub(crate) match_total_exposure: bool,
    #[arg(long, default_value_t = 0.5)]
    pub(crate) target_luminance: f64,
    /// LRPAR-G15-IPTC-S6: opt-in IPTC bake-in into the exported file (JPEG
    /// only, IIM+XMP from the sidecar draft + keywords, SOLL §7). Without the
    /// flag the export stays exactly as today (no metadata, no silent
    /// assumptions).
    #[arg(long)]
    pub(crate) write_metadata: bool,
}

/// R2-CLI-03: `inspect` accepts `--json` for a machine-readable report
/// (RAW metadata, sidecar status, every virtual copy incl. auto-tone and
/// matching state). Free text remains the default output.
#[derive(Debug, Args)]
pub(crate) struct InspectArgs {
    pub(crate) input: PathBuf,
    /// Print a machine-readable JSON status instead of free text.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct RedEyeArgs {
    #[arg(long)]
    pub(crate) input: PathBuf,
    #[arg(long)]
    pub(crate) virtual_copy: Option<String>,
    #[arg(long)]
    pub(crate) json: bool,
    /// List the persisted red-eye regions (read-only; the default).
    #[arg(long)]
    pub(crate) list: bool,
    /// Mark one region as `ID:x,y,radius,desaturate,darken` (normalized
    /// coordinates; repeatable). Regions are identified by their stable id,
    /// never by list position; an existing id is replaced loudly.
    #[arg(long, value_name = "ID:X,Y,RADIUS,DESAT,DARKEN")]
    pub(crate) set: Vec<String>,
    /// Remove regions by id (comma-separated, repeatable).
    #[arg(long, value_delimiter = ',')]
    pub(crate) remove: Vec<String>,
    /// Remove the whole red-eye stage (identity).
    #[arg(long)]
    pub(crate) clear: bool,
    /// LRPAR-G14-REDEYE-AUTO-15: run the deterministic, model-free pupil
    /// detection on the decoded source pixels and list the candidates. This is
    /// read-only; nothing is persisted until `--detect-apply` is passed.
    #[arg(long)]
    pub(crate) detect: bool,
    /// Persist the candidates found by `--detect` (`auto-re-` regions).
    /// Requires `--detect`; replaces only previously auto-detected regions and
    /// leaves manual regions untouched. Loud when the 32-region cap would be
    /// exceeded (no silent truncation).
    #[arg(long)]
    pub(crate) detect_apply: bool,
}
