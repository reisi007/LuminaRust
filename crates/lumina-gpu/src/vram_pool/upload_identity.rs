//! GPU-UPLOAD-IDENTITY-92: identity of the source frame that is currently
//! uploaded into a cached input texture.
//!
//! The cached-upload skip (`R2-GPU-01` in the VRAM present path, `R2-GPU-04` in
//! the readback path) is an **instance** identity, never a content identity and
//! never an address identity:
//!
//! - a different frame instance is always uploaded, even when it has the same
//!   pixels, the same length or the same allocation address as the last one;
//! - the same frame object is not uploaded again, not even via a clone
//!   ([`lumina_core::ImageFrame::clone`] keeps the instance ID);
//! - a different target geometry is always uploaded.
//!
//! The former key `(pixels_ptr, pixels_len)` could not tell a new frame of equal
//! length from the previously uploaded one when the allocator reused the buffer,
//! so a stale upload stayed resident and the render showed the previous frame's
//! content. That defect is why the decision lives in this one pure function:
//! both upload sites call [`should_upload`], and no second comparison remains.

use lumina_core::ImageFrame;

/// Instance identity of the source frame last uploaded into a cached texture,
/// together with the geometry of the texture it was uploaded into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UploadIdentity {
    instance_id: u64,
    width: u32,
    height: u32,
}

impl UploadIdentity {
    /// Identity a successful upload of `frame` into a `width`x`height` texture
    /// leaves behind.
    pub(crate) fn of(frame: &ImageFrame, width: u32, height: u32) -> Self {
        Self {
            instance_id: frame.instance_id(),
            width,
            height,
        }
    }
}

/// Returns `true` when `frame` must be uploaded into the (cached) input texture.
///
/// The upload is skipped **only** when the previous upload came from the same
/// frame instance *and* used the same geometry. Everything else uploads: no
/// previous upload, another instance (independent of content and length), or a
/// different texture geometry.
pub(crate) fn should_upload(
    previous: Option<UploadIdentity>,
    width: u32,
    height: u32,
    frame: &ImageFrame,
) -> bool {
    previous != Some(UploadIdentity::of(frame, width, height))
}

#[cfg(test)]
mod tests {
    use super::{should_upload, UploadIdentity};
    use lumina_core::ImageFrame;

    fn frame(value: u8) -> ImageFrame {
        ImageFrame::new(2, 1, vec![value; 8]).expect("valid 2x1 RGBA8 frame")
    }

    fn uploaded(source: &ImageFrame) -> UploadIdentity {
        UploadIdentity::of(source, source.width(), source.height())
    }

    #[test]
    fn same_instance_and_geometry_skips_the_upload() {
        let source = frame(5);
        assert!(!should_upload(Some(uploaded(&source)), 2, 1, &source));
    }

    #[test]
    fn cloned_frame_keeps_the_upload() {
        let source = frame(5);
        let cloned = source.clone();
        assert!(
            !should_upload(Some(uploaded(&source)), 2, 1, &cloned),
            "a clone is the same instance and must not be re-uploaded"
        );
    }

    #[test]
    fn another_instance_with_identical_pixels_and_length_uploads() {
        let uploaded_frame = frame(9);
        let next = frame(9);
        assert_eq!(uploaded_frame.pixels(), next.pixels());
        assert_eq!(uploaded_frame.pixels().len(), next.pixels().len());
        assert_eq!(uploaded_frame, next, "content equality hides the instance");
        assert!(
            should_upload(Some(uploaded(&uploaded_frame)), 2, 1, &next),
            "a new instance with equal content and length is still a new upload"
        );
    }

    #[test]
    fn another_geometry_uploads() {
        let source = frame(1);
        assert!(
            should_upload(Some(UploadIdentity::of(&source, 4, 2)), 1, 2, &source),
            "a different stored geometry uploads even for the same instance"
        );
        assert!(
            should_upload(Some(uploaded(&source)), 1, 2, &source),
            "a different target geometry uploads"
        );
    }

    #[test]
    fn no_previous_upload_uploads() {
        let source = frame(1);
        assert!(should_upload(None, 2, 1, &source));
    }
}
