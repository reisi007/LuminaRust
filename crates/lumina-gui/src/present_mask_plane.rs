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
//! condition *honestly satisfiable*: there is no residency bookkeeping that
//! can drift, because the plane is rewritten on the very render that empties
//! the layer list. See
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
    Clear,
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
            MaskPlaneIntent::KeepLiveBrush => {}
            MaskPlaneIntent::Push => self.push_combined_mask_plane_to_vram(),
            MaskPlaneIntent::Clear => self.clear_active_vram_mask_plane(),
        }
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
                            Ok(()) => self.vram_mask_is_evaluated = true,
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
    /// A failed write is **loud in routing terms, not only in the log**: the
    /// plane would keep whatever it held, and case 1 would present it, so
    /// `vram_fresh` is dropped and the frame falls back to the exact CPU
    /// present. The next successful VRAM render sets the flag again — a
    /// self-healing conservative route, not a silent divergence.
    fn clear_active_vram_mask_plane(&mut self) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        if !gpu.is_available() {
            return;
        }
        let Some((width, height)) = gpu.vram_dimensions() else {
            // Nothing is resident, so nothing can be stale.
            return;
        };
        if width == 0 || height == 0 {
            return;
        }
        let rows_per_band = zero_band_rows(width, ZERO_BAND_BYTES);
        let mut band = vec![0u16; width as usize * rows_per_band as usize];
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
        }
    }
}
