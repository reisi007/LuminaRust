//! GUI-REFACTOR-W1-20 S1.2a: render-source identity and mask-plane loading,
//! extracted verbatim from `lib.rs`.
//!
//! [`LuminaApp::resolved_source_hash`] is the memoized blake3 content hash of
//! the loaded source (PERF-GUI-1 — hashing the whole file per render tick was
//! part of the old hot path) and [`LuminaApp::load_mask_planes`] loads the
//! active copy's `.lumina.zdata` matte tiles (composite-key first, logged
//! legacy bare-mask-id fallback). Both feed `render_from`; the app root and the
//! sibling feature modules (face/denoise/cull) call them, hence `pub(crate)`.

use super::*;
use log::debug;

impl LuminaApp {
    /// PERF-GUI-1: content hash of the currently loaded source bytes, computed
    /// at most once per loaded file. The memo is cleared together with
    /// `source_bytes` in [`Self::apply_decoded_frame`]; callers therefore never
    /// re-hash the (potentially ~50 MB) RAW file per interactive render tick.
    pub(crate) fn resolved_source_hash(&mut self) -> String {
        if let Some(hash) = &self.source_hash_memo {
            return hash.clone();
        }
        let hash = self
            .source_bytes
            .as_ref()
            .map(|bytes| format!("blake3:{}", blake3::hash(bytes).to_hex()))
            .unwrap_or_else(|| "blake3:unknown".into());
        self.source_hash_memo = Some(hash.clone());
        hash
    }

    /// Loads mask artifact planes from the optional `.lumina.zdata` sidecar for
    /// the active virtual copy (native only).  Missing/unreadable zdata yields
    /// an empty map; affected layers are handled by the `MaskPolicy::Warn`
    /// path in [`render_frame`].
    ///
    /// KONSISTENZ (REVIEW-CLI-N1): tile records are addressed by the composite
    /// id [`Self::zdata_tile_record_id`] (`"{copy_id}/{mask_id}"`) so two
    /// virtual copies that happen to share a mask id never share a matte.
    /// Containers written before that convention carry the bare `mask_id`;
    /// those records stay readable through an explicitly logged legacy lookup
    /// (documented read compatibility — not a silent fallback).
    pub(crate) fn load_mask_planes(&self) -> BTreeMap<(String, String), MaskPlane> {
        let mut planes = BTreeMap::new();
        let Some(document) = &self.document else {
            return planes;
        };
        let Some(copy) = document
            .virtual_copies
            .iter()
            .find(|c| c.id == self.virtual_copy_id)
        else {
            return planes;
        };
        let zdata_path = zdata_path_for(Path::new(&self.path));
        if !zdata_path.exists() {
            return planes;
        }
        let Ok(container) = load_zdata(&zdata_path) else {
            return planes;
        };
        for mask in copy
            .mask_library
            .iter()
            .filter(|m| matches!(m.status, MaskStatus::Valid))
        {
            let tile = match container.tile(&Self::zdata_tile_record_id(&copy.id, &mask.id), 0, 0) {
                Ok(tile) => Some(tile),
                Err(_) => match container.tile(&mask.id, 0, 0) {
                    Ok(tile) => {
                        debug!(
                            "mask plane copy `{}` / mask `{}` loaded under the legacy bare \
                             mask-id zdata key",
                            copy.id, mask.id
                        );
                        Some(tile)
                    }
                    Err(_) => None,
                },
            };
            if let Some(tile) = tile {
                if let Ok(plane) = MaskPlane::new(tile.width, tile.height, tile.values) {
                    planes.insert((copy.id.clone(), mask.id.clone()), plane);
                }
            }
        }
        planes
    }
}
