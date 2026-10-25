//! [`ImageFrame`] identity: the frame type itself, its process-local instance
//! ID and its content-based equality.
//!
//! GPU-UPLOAD-IDENTITY-92: the GPU upload skip is keyed on the **instance**, not
//! on the allocation address or the pixel content. This module owns that
//! identity: [`ImageFrame::new`] takes a fresh ID from a process-local
//! [`AtomicU64`], `Clone` keeps it, and equality stays content-based so every
//! existing `assert_eq!` on frames keeps its meaning.
//!
//! Extracted from `lib.rs` (file-size ratchet §8) together with the type: the
//! struct's seven-line doc comment moved verbatim, only the field-privacy
//! sentence now says `pub(crate)` because the sibling modules keep reading
//! `width`/`height`/`pixels` directly.

use std::sync::atomic::{AtomicU64, Ordering};

/// Source of the process-local [`ImageFrame`] instance IDs.
///
/// Starts at 1 so `0` is never a valid identity; the value is only ever
/// compared for equality. `fetch_add(1, Relaxed)` gives every constructor call a
/// distinct value without needing cross-thread ordering. A wrap after `2^64`
/// constructions is not handled: it would require more frames than a process
/// can allocate, and even then only a cache-miss/cache-hit choice would be
/// wrong — never pixel content.
static NEXT_INSTANCE_ID: AtomicU64 = AtomicU64::new(1);

/// Row-major RGBA8 frame.
///
/// Invariant: `pixels().len() == width() * height() * 4` (checked
/// multiplication), sealed by construction. [`ImageFrame::new`] is the only
/// **public** constructor and enforces the relation up front; the crate-internal
/// `with_new_identity` exists solely so [`ImageFrame::new`] assigns the instance
/// id at the one place that also validates the buffer. [`ImageFrame::pixels`],
/// [`ImageFrame::pixels_mut`] and [`ImageFrame::into_pixels`] can never change
/// the buffer length, and `width`/`height` are read-only accessors outside the
/// crate (`pub(crate)` fields) — so neither the buffer side nor the dimensions
/// side of the relation can be broken from outside the crate after
/// construction.
#[derive(Debug, Clone)]
pub struct ImageFrame {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u8>,
    instance_id: u64,
}

impl ImageFrame {
    /// Constructs the frame body with a **fresh** instance ID.
    ///
    /// Crate-internal half of [`ImageFrame::new`], which does the length
    /// validation and is the only public constructor. Keeping the ID assignment
    /// here lets `new` stay one statement.
    pub(crate) fn with_new_identity(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        Self {
            width,
            height,
            pixels,
            instance_id: NEXT_INSTANCE_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// Process-local identity of this frame **instance**.
    ///
    /// The ID is monotone within the process (`ImageFrame::new` takes the next
    /// value; `Clone` keeps the source's value), is only meant for cache
    /// identity — e.g. the GPU's upload skip — and is never persisted: it does
    /// not appear in any sidecar, render key or JSON, and it is **not** part of
    /// [`PartialEq`]/[`Eq`]. It also says nothing about the content: two frames
    /// with identical pixels still have different IDs unless one is a clone of
    /// the other.
    #[must_use]
    pub fn instance_id(&self) -> u64 {
        self.instance_id
    }
}

/// Content equality: geometry plus pixels, **excluding** the instance ID.
///
/// Implemented manually (instead of derived) because `instance_id` must not
/// affect equality: two frames of equal size and equal pixels are `==` even
/// when they are different instances. There is deliberately no `Hash` impl.
impl PartialEq for ImageFrame {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width && self.height == other.height && self.pixels == other.pixels
    }
}

impl Eq for ImageFrame {}

#[cfg(test)]
mod tests {
    use super::ImageFrame;

    fn frame(value: u8) -> ImageFrame {
        ImageFrame::new(2, 1, vec![value; 8]).expect("valid 2x1 RGBA8 frame")
    }

    #[test]
    fn new_assigns_a_fresh_instance_id_per_call() {
        let first = frame(1);
        let second = frame(1);
        assert!(
            second.instance_id() > first.instance_id(),
            "each new frame takes the next ID: {} -> {}",
            first.instance_id(),
            second.instance_id()
        );
    }

    #[test]
    fn clone_keeps_the_instance_id() {
        let original = frame(7);
        let cloned = original.clone();
        assert_eq!(cloned.instance_id(), original.instance_id());
    }

    #[test]
    fn equality_ignores_the_instance_id_but_not_the_content() {
        let first = frame(3);
        let second = frame(3);
        assert_ne!(
            first.instance_id(),
            second.instance_id(),
            "two constructors calls are two instances"
        );
        assert_eq!(first, second, "equal geometry and pixels are equal");

        let different = frame(4);
        assert_ne!(first, different, "different pixels are not equal");
    }
}
