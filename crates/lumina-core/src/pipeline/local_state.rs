//! Local mask-state contributions to the render/mask identity.

use super::RenderKey;
use blake3::Hasher;

impl RenderKey {
    /// Attach the canonical ordered mask-layer state. This is part of both
    /// the mask-stage and final render identities; changing a local value or
    /// moving one overlapping layer therefore cannot reuse stale pixels.
    #[must_use]
    pub fn with_mask_local_state_digest(mut self, digest: impl Into<String>) -> Self {
        self.mask_local_state_digest = Some(digest.into());
        self
    }
}

pub(super) fn update_digest(hasher: &mut Hasher, digest: Option<&str>) {
    match digest {
        None => {
            hasher.update(&[0]);
        }
        Some(value) => {
            hasher.update(&[1]);
            hasher.update(value.as_bytes());
            hasher.update(&[0]);
        }
    }
}
