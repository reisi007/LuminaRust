//! GPU-PARITY-MASKGATE-1: the VRAM mask plane of the readback-free present
//! path.
//!
//! The present composite ([`lumina_gpu::GpuContext::copy_vram_to_texture`])
//! reads the tone result and the `R16Uint` mask plane of the **active** VRAM
//! pool entry, so that entry's plane is the only mask coverage a presented
//! frame can show. This module owns one invariant:
//!
//! > After every render, the active entry's mask plane holds exactly the
//! > coverage the presented frame is allowed to show — the combined evaluated
//! > mask layers, the incrementally uploaded live brush plane while a live
//! > brush is composited, or **nothing at all**.
//!
//! The third case is the one that used to be missing. `lumina-gpu` pools
//! entries per `(width, height)` and does not clear a reactivated entry, and
//! the old upload path returned early when no layer was evaluated — so deleting
//! the **last** mask left the deleted mask's coverage in the pool while the
//! frame carried no layer at all. Since that state is exactly GPU-PARITY-
//! MASKGATE-1 case 1 (no evaluated layer, editorial gate closed), the VRAM
//! present path was allowed and presented a stale artifact as the current
//! frame; measured on a real adapter: `maxAbsDiff=67` over 127 707 of 603 904
//! presented photo bytes, and unreachable for the user to switch off because
//! `mask_overlay_allowed` demands the selection that no longer exists.
//!
//! Writing zeros instead of skipping the upload is what makes the SOLL
//! condition *honestly satisfiable*. GPU-CLEAR-TRANSITION
//! (GPU-MASKPLANE-CLEAR-PERF-51) then narrows that write to the one moment it
//! can change something — the transition "non-null plane resident → no layer" —
//! because a maskless render repeated it on every frame, and F-074-N10 measured
//! what that costs: 37 µs / 722 µs / 15,21 ms (160×120 / 1280×853 / 6000×4000),
//! volume-driven, ~13× a full render at 24 MP. The bookkeeping that decision
//! needs ([`VramMaskPlaneClear`]) cannot drift, and for a structural reason
//! rather than by convention: every write of the plane is witnessed (a pushed
//! combined plane, a written brush upload, the clear below), and every entry
//! that no witness has ever described was created with a zeroed mask texture by
//! `lumina-gpu`'s `create_vram_state`. The verdict additionally carries the
//! pool entry it was made in, because the pool re-points its active entry on
//! every `ensure_vram` and does not clear a reactivated entry. See
//! `feature/architecture/pipeline.md`, section "Present-Pfad".
//!
//! The clear is written in bounded row bands (see [`zero_band_rows`]) so the
//! transient buffer stays small even for a full-resolution pool entry, and it
//! targets the active entry only — the sync never re-points the pool, so it
//! cannot disturb which entry the present path composites.

use super::*;
use log::warn;

/// Upper bound for the transient zero buffer of the mask-plane clear (1 MiB).
/// The clear writes full-width row bands, so the peak allocation is this value
/// regardless of the entry's resolution.
pub(crate) const ZERO_BAND_BYTES: usize = 1 << 20;

/// What the presented frame may show in the VRAM mask plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MaskPlaneIntent {
    /// Push the combined evaluated mask planes (the historical path).
    Push,
    /// Leave the incrementally uploaded live brush plane untouched: it is
    /// uploaded tile by tile, so a full-plane write would destroy everything
    /// outside the current dab.
    KeepLiveBrush,
    /// The frame carries no mask coverage at all — the plane must read as
    /// zero, so a previously uploaded plane cannot be presented as current.
    /// The zero write itself only runs while
    /// [`VramMaskPlaneResidency::needs_zero_write`] says the active entry can
    /// still hold coverage (GPU-CLEAR-TRANSITION).
    Clear,
}

/// What the mask plane of the **active** VRAM pool entry holds
/// (GPU-MASKPLANE-CLEAR-PERF-51, owner decision `GPU-CLEAR-TRANSITION`).
///
/// A maskless render used to write zeros on every frame. That write is the
/// expensive half of a frame without coverage — 37 µs / 722 µs / 15,21 ms at
/// 160×120 / 1280×853 / 6000×4000, volume-driven (F-074-N10) — so it now runs
/// only on the transition "non-null plane resident → no layer". This enum is
/// the state that transition is decided from, and it is conservative by
/// construction: every state that merely *might* hold coverage answers
/// "write it".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum VramMaskPlaneResidency {
    /// Reads as zero, globally: no write has ever been described in this
    /// session, or no plane exists at all (no adapter, no pooled entry). That
    /// is a sound zero verdict for *every* pooled entry, because
    /// `lumina-gpu`'s `create_vram_state` zeroes the mask texture when it
    /// creates one.
    #[default]
    Zero,
    /// Holds non-zero coverage: a combined plane that was pushed, a brush
    /// upload that wrote, or a zero write that stopped part-way and left
    /// earlier bands behind.
    NonZero,
    /// A completed zero write left it zero **in the entry with these
    /// dimensions**. The pool keys entries by dimensions and re-points its
    /// active entry on every `ensure_vram` (source or resolution switch, the
    /// draft↔full switch) without clearing the reactivated entry, so the
    /// verdict is dropped as soon as a different entry becomes the active one.
    ZeroedIn((u32, u32)),
}

impl VramMaskPlaneResidency {
    /// Whether a maskless frame still has to write zeros. `active` is the pool
    /// key of the entry the present path composites — the only entry whose
    /// leftover coverage a presented frame could show; `None` when no adapter
    /// is bound or no entry exists, in which case no plane exists at all.
    pub(crate) fn needs_zero_write(&self, active: Option<(u32, u32)>) -> bool {
        match self {
            // Never written, or nothing to write in: there is no coverage to
            // remove, on any entry.
            Self::Zero => false,
            // Coverage is resident, or may be (a partial clear left some).
            Self::NonZero => true,
            // Zero as of the last write — but only for the entry that write
            // covered. A different active entry carries no verdict, so the
            // conservative answer is to write it.
            Self::ZeroedIn(entry) => active != Some(*entry),
        }
    }
}

/// What a completed [`LuminaApp::clear_active_vram_mask_plane`] leaves behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ZeroWriteOutcome {
    /// The plane reads as zero: either every row was written, or there was no
    /// plane to write at all (no adapter / no pooled entry), in which case zero
    /// is the only state that can exist.
    Complete,
    /// The write stopped part-way, so the plane keeps whatever it held. The
    /// residency must stay non-zero and the next render retries the write.
    Incomplete,
}

/// How the mask plane of the active VRAM pool entry stands, and how often the
/// zero write has run (GPU-MASKPLANE-CLEAR-PERF-51).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct VramMaskPlaneClear {
    /// What that plane holds — the state the transition condition reads.
    pub(crate) residency: VramMaskPlaneResidency,
    /// Number of times [`LuminaApp::clear_active_vram_mask_plane`] has run. The
    /// write itself needs an adapter, so this counter is what makes the
    /// transition condition observable without one — the same pattern as
    /// `lumina_core::MaskGraph`'s executed-node counter. It is also a plain
    /// diagnostic: a session that paints no mask at all must show `0` here.
    pub(crate) zero_writes: u32,
}

impl VramMaskPlaneClear {
    /// Whether the entry `active` still needs the zero write. The entry's pool
    /// key is passed in rather than read from the device, so the entry-keyed
    /// half of the condition is testable without an adapter.
    pub(crate) fn needs_zero_write(&self, active: Option<(u32, u32)>) -> bool {
        self.residency.needs_zero_write(active)
    }
}

/// Rows of a full-width zero band whose byte size fits `byte_budget`.
///
/// At least one row is always returned, so a width larger than the budget
/// still produces progress instead of an empty band.
pub(crate) fn zero_band_rows(width: u32, byte_budget: usize) -> u32 {
    if width == 0 {
        return 1;
    }
    let bytes_per_row = (width as usize).saturating_mul(2);
    (byte_budget / bytes_per_row).max(1) as u32
}

impl LuminaApp {
    /// What the VRAM mask plane must hold for the frame that is about to be
    /// presented. Pure session/render-state logic — the headless tests drive it
    /// without an adapter, and [`Self::sync_mask_plane_to_vram`] is its only
    /// production caller.
    pub(crate) fn vram_mask_plane_intent(&self) -> MaskPlaneIntent {
        if !self.render_mask_layers.is_empty() {
            return MaskPlaneIntent::Push;
        }
        // GPU-MASKPLANE-BRUSH-50: a live brush plane only survives while the
        // upload that would have replaced it actually wrote. Every failure path
        // in `gpu_upload_brush_tile` leaves the previous plane resident and
        // still-resident-looking, because `preview_masks` sets `drawing = true`
        // *before* the upload is attempted. Keeping that plane would present
        // another mask's coverage, which is the very thing the clear exists to
        // prevent — so a failed upload demotes the intent to `Clear` instead of
        // silently keeping stale residency. The demo normal case is unaffected:
        // a successful upload clears the flag and the plane survives.
        if self.live_brush_plane_stale {
            return MaskPlaneIntent::Clear;
        }
        // A live brush plane is the one coverage that is *not* reproducible by a
        // full-plane write, and it is composited exactly while the present
        // gate's live-gesture branch allows it. Mirroring that triple here
        // keeps the two decisions from drifting apart: whenever the VRAM path
        // would composite the live plane, the plane survives, and otherwise the
        // plane is cleared.
        if self.drawing && self.mask_tool == MaskTool::Brush && self.mask_overlay_allowed() {
            return MaskPlaneIntent::KeepLiveBrush;
        }
        MaskPlaneIntent::Clear
    }

    /// Make the active VRAM entry's mask plane agree with the rendered frame
    /// (GUI-WGPU-PRESENT-1 / GPU-STAGE-1, extended by GPU-PARITY-MASKGATE-1).
    /// Called at the end of every render, so the invariant holds whenever
    /// `gpu_present_if_ready` asks its questions.
    pub(crate) fn sync_mask_plane_to_vram(&mut self) {
        // The per-frame "a combined plane was pushed" flag: it describes *this*
        // frame only and is deliberately not a residency signal (every
        // `mark_dirty` resets it).
        self.vram_mask_is_evaluated = false;
        match self.vram_mask_plane_intent() {
            MaskPlaneIntent::KeepLiveBrush => {
                // The live brush plane is the coverage the present path
                // composites right now, so the active entry holds non-zero data
                // for as long as the gesture runs. Marking it here rather than
                // inferring it from the upload paths is what makes the state
                // robust: a gesture that wrote no dab, and one whose last tile
                // the upload path deliberately does not record, both still end
                // with a plane that must be zeroed once. The per-write
                // witnesses (`note_vram_mask_plane_coverage`) stay — they are
                // the exact record of a write and are what carries the verdict
                // when a tile lands without a `KeepLiveBrush` frame after it.
                self.note_vram_mask_plane_coverage();
            }
            MaskPlaneIntent::Push => self.push_combined_mask_plane_to_vram(),
            MaskPlaneIntent::Clear => {
                // GPU-MASKPLANE-CLEAR-PERF-51 (`GPU-CLEAR-TRANSITION`): the
                // zero write only runs on the transition "non-null plane
                // resident → no layer". Every maskless render before that
                // decision paid it unconditionally, measured at 37 µs / 722 µs
                // / 15,21 ms (160×120 / 1280×853 / 6000×4000, F-074-N10) —
                // ~13× a full render at 24 MP for a plane that already reads
                // zero.
                if self.vram_mask_plane_needs_zero_write()
                    && self.clear_active_vram_mask_plane() == ZeroWriteOutcome::Complete
                {
                    // Either every row reads zero now, or there is no plane at
                    // all — both leave the active entry reading zero, and the
                    // verdict is only as wide as the entry it covers.
                    self.note_vram_mask_plane_zeroed();
                }
                // Incomplete: the plane keeps what it held, so the residency
                // stays non-zero and the next render retries the write.
            }
        }
    }

    /// GPU-CLEAR-TRANSITION: record that the active entry's mask plane reads as
    /// zero. Called after a completed zero write (or after finding no plane to
    /// write at all); the verdict names the entry it was made in, because the
    /// pool re-points its active entry on every `ensure_vram`.
    fn note_vram_mask_plane_zeroed(&mut self) {
        self.vram_mask_plane_clear.residency = match self.active_vram_mask_plane_entry() {
            Some(entry) => VramMaskPlaneResidency::ZeroedIn(entry),
            None => VramMaskPlaneResidency::Zero,
        };
    }

    /// Pool key of the VRAM entry the present path composites: the active
    /// (most-recently-used) one. `None` when no adapter is bound or no entry
    /// exists — in both cases no mask plane exists either.
    fn active_vram_mask_plane_entry(&self) -> Option<(u32, u32)> {
        self.gpu
            .as_ref()
            .and_then(lumina_gpu::GpuContext::vram_dimensions)
    }

    /// GPU-CLEAR-TRANSITION: coverage was written into the mask plane of the
    /// active entry — a combined plane that was pushed, or a brush upload that
    /// wrote. Both writers record it here, which is what keeps "non-null plane
    /// resident" from being *inferred* from session state (and therefore from
    /// drifting away from what the plane actually holds). A writer that wrote
    /// nothing must not call this: the previous verdict still describes the
    /// plane exactly.
    pub(crate) fn note_vram_mask_plane_coverage(&mut self) {
        self.vram_mask_plane_clear.residency = VramMaskPlaneResidency::NonZero;
    }

    /// GPU-MASKPLANE-CLEAR-PERF-51 (`GPU-CLEAR-TRANSITION`): whether the zero
    /// write of a maskless frame is still *needed* — i.e. whether the active
    /// entry's mask plane can hold coverage that the frame must not show.
    /// Exposed so the headless tests can pin the transition without an adapter;
    /// [`Self::sync_mask_plane_to_vram`] is its only production caller.
    pub(crate) fn vram_mask_plane_needs_zero_write(&self) -> bool {
        self.vram_mask_plane_clear
            .needs_zero_write(self.active_vram_mask_plane_entry())
    }

    /// GUI-WGPU-PRESENT-1 / GPU-STAGE-1: push the pipeline-evaluated combined
    /// mask planes into the VRAM present composite. Failures are loud but never
    /// break the CPU preview path.
    fn push_combined_mask_plane_to_vram(&mut self) {
        let planes: Vec<lumina_core::MaskPlane> = self
            .render_mask_layers
            .iter()
            .map(|layer| layer.plane.clone())
            .collect();
        match lumina_gpu::combine_mask_planes(&planes) {
            Ok(Some(combined))
                if combined.width == self.preview.as_ref().map(|p| p.width).unwrap_or(0)
                    && combined.height == self.preview.as_ref().map(|p| p.height).unwrap_or(0) =>
            {
                if let Some(gpu) = self.gpu.as_ref() {
                    if gpu.is_available()
                        && gpu.ensure_vram(combined.width, combined.height).is_ok()
                    {
                        match gpu.upload_mask_plane(
                            combined.width,
                            combined.height,
                            &combined.values,
                        ) {
                            Ok(()) => {
                                self.vram_mask_is_evaluated = true;
                                // GPU-CLEAR-TRANSITION: a written combined plane
                                // is coverage, so the entry it landed in no
                                // longer reads as zero.
                                self.note_vram_mask_plane_coverage();
                            }
                            Err(err) => {
                                warn!("gpu evaluated-mask upload failed: {err}");
                            }
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(err) => {
                warn!("gpu evaluated-mask combination failed: {err}");
            }
        }
    }

    /// Write zeros over the active entry's mask plane, in bounded row bands.
    ///
    /// The active entry is the pool's most-recently-used one, i.e. the entry
    /// the present composite reads, so it is the only one whose leftover
    /// coverage could ever be presented. `vram_dimensions` is read instead of
    /// `ensure_vram` on purpose: the sync must not re-point the pool, or it
    /// could make a different entry active than the one the render filled.
    ///
    /// Returns whether the plane reads as zero afterwards. A failed write is
    /// **loud in routing terms, not only in the log**: the plane would keep
    /// whatever it held, and case 1 would present it, so `vram_fresh` is
    /// dropped and the frame falls back to the exact CPU present. The next
    /// successful VRAM render sets the flag again — a self-healing conservative
    /// route, not a silent divergence. The residency answer matters exactly as
    /// much: a plane that only reads as *partly* zero is still coverage, so the
    /// caller must keep asking for the write.
    fn clear_active_vram_mask_plane(&mut self) -> ZeroWriteOutcome {
        self.vram_mask_plane_clear.zero_writes += 1;
        let Some(gpu) = self.gpu.as_ref() else {
            return ZeroWriteOutcome::Complete;
        };
        if !gpu.is_available() {
            return ZeroWriteOutcome::Complete;
        }
        let Some((width, height)) = gpu.vram_dimensions() else {
            // Nothing is resident, so nothing can be stale.
            return ZeroWriteOutcome::Complete;
        };
        if width == 0 || height == 0 {
            return ZeroWriteOutcome::Complete;
        }
        let rows_per_band = zero_band_rows(width, ZERO_BAND_BYTES);
        let band = vec![0u16; width as usize * rows_per_band as usize];
        let mut y = 0u32;
        let mut failed = false;
        while y < height {
            let rows = rows_per_band.min(height - y);
            let slice = &band[..width as usize * rows as usize];
            if let Err(err) = gpu.upload_mask_tile(0, y, width, rows, bytemuck::cast_slice(slice)) {
                warn!("gpu mask-plane clear failed at row {y} ({width}x{rows}): {err}");
                failed = true;
                break;
            }
            y += rows;
        }
        if failed {
            self.vram_fresh = false;
            return ZeroWriteOutcome::Incomplete;
        }
        ZeroWriteOutcome::Complete
    }
}
