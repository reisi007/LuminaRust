//! CROP-MAXRECT-1 (Release 1.0): default maximum-content crop.
//!
//! After a lens and/or perspective correction the resampled frame contains
//! transparent wedges (out-of-bounds source coordinates, `sample` returns
//! RGBA `0`). Unless the user sets an explicit crop, the default crop is the
//! **largest axis-aligned rectangle that contains only content** ("maximum
//! rectangle") and stays inside the frame ("constrain to image").
//!
//! # Identity of the default
//!
//! * A frame whose pixels are all content yields the full frame — the default
//!   crop is then the identity (`maximum_content_rect` returns
//!   `(0, 0, width, height)`).
//! * An all-transparent (degenerate) frame has no content rectangle; the
//!   caller keeps the full frame instead of collapsing to an empty crop.
//! * This function is content-based: it never invents coverage. A frame that
//!   is opaque everywhere (no correction, or a correction that introduced no
//!   transparent edge) is returned unchanged, so "no crop without a reason"
//!   holds by construction.
//!
//! The geometry resampling is binary with respect to alpha: `ImageFrame::sample`
//! returns either a fully opaque source pixel or `0` outside the source, so
//! `CONTENT_ALPHA_MIN` does not introduce a soft threshold on corrected frames
//! (see the `maximum_content_rect` tests).

use crate::ImageFrame;

/// Minimum alpha for a pixel to count as content. `0` is the transparent
/// out-of-bounds fill produced by lens/perspective resampling.
pub const CONTENT_ALPHA_MIN: u8 = 1;

/// Pixel rectangle `(x, y, width, height)` in frame coordinates.
pub type PixelRect = (u32, u32, u32, u32);

/// Largest-area axis-aligned rectangle of pixels that all carry content
/// (`alpha >= CONTENT_ALPHA_MIN`), constrained to the frame.
///
/// Returns `None` when the frame has no content at all (all pixels
/// transparent) or is degenerate (zero extent); callers then keep the full
/// frame rather than cropping to an empty rectangle.
pub fn maximum_content_rect(frame: &ImageFrame) -> Option<PixelRect> {
    let width = frame.width as usize;
    let height = frame.height as usize;
    if width == 0 || height == 0 {
        return None;
    }

    // `heights[x]` is the number of consecutive content pixels ending at the
    // current row. Recomputing the largest rectangle in that histogram for
    // every row yields the largest all-content axis-aligned rectangle.
    let mut heights = vec![0usize; width];
    let mut best: Option<(usize, usize, usize)> = None; // (left, width, height)
    let mut best_area = 0usize;
    let mut best_top = 0usize;

    for y in 0..height {
        let row = y * width;
        for (x, height) in heights.iter_mut().enumerate() {
            if frame.pixels[(row + x) * 4 + 3] >= CONTENT_ALPHA_MIN {
                *height += 1;
            } else {
                *height = 0;
            }
        }
        if let Some((left, rect_width, rect_height)) = largest_histogram_rect(&heights) {
            let area = rect_width * rect_height;
            if area > best_area {
                best_area = area;
                best_top = y + 1 - rect_height;
                best = Some((left, rect_width, rect_height));
            }
        }
    }

    best.map(|(left, rect_width, rect_height)| {
        (
            left as u32,
            best_top as u32,
            rect_width as u32,
            rect_height as u32,
        )
    })
}

/// Largest rectangle in a histogram (classic stack algorithm). Returns
/// `(left, width, height)` of the first maximum found scanning left to right;
/// integer arithmetic makes the choice deterministic across platforms. Returns
/// `None` for an all-zero histogram.
fn largest_histogram_rect(heights: &[usize]) -> Option<(usize, usize, usize)> {
    let mut stack: Vec<usize> = Vec::with_capacity(heights.len());
    let mut best: Option<(usize, usize, usize)> = None;
    let mut best_area = 0usize;

    for i in 0..=heights.len() {
        let current = if i < heights.len() { heights[i] } else { 0 };
        while let Some(&top) = stack.last() {
            if heights[top] > current {
                stack.pop();
                let height = heights[top];
                let left = stack.last().map_or(0, |&j| j + 1);
                let rect_width = i - left;
                let area = rect_width * height;
                if area > best_area {
                    best_area = area;
                    best = Some((left, rect_width, height));
                }
            } else {
                break;
            }
        }
        stack.push(i);
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_with<F: Fn(u32, u32) -> bool>(width: u32, height: u32, content: F) -> ImageFrame {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                if content(x, y) {
                    pixels.extend_from_slice(&[120, 130, 140, 255]);
                } else {
                    pixels.extend_from_slice(&[0, 0, 0, 0]);
                }
            }
        }
        ImageFrame::new(width, height, pixels).unwrap()
    }

    fn fully_opaque(width: u32, height: u32) -> ImageFrame {
        frame_with(width, height, |_, _| true)
    }

    #[test]
    fn opaque_frame_is_identity_full_frame() {
        let frame = fully_opaque(7, 4);
        assert_eq!(maximum_content_rect(&frame), Some((0, 0, 7, 4)));
    }

    #[test]
    fn transparent_border_is_excluded() {
        // Top and bottom rows, plus left column, transparent.
        let frame = frame_with(5, 5, |x, y| x > 0 && y > 0 && y < 4);
        assert_eq!(maximum_content_rect(&frame), Some((1, 1, 4, 3)));
    }

    #[test]
    fn picks_the_largest_all_content_rectangle_of_a_wedge() {
        // Lower-left triangle: content iff `x <= y` in a 4x4 frame. The
        // largest all-content rectangle has area 6 (`2 x 3` at x=0, y=1).
        let frame = frame_with(4, 4, |x, y| x <= y);
        assert_eq!(maximum_content_rect(&frame), Some((0, 1, 2, 3)));
    }

    #[test]
    fn largest_histogram_rect_is_deterministic_and_correct() {
        assert_eq!(largest_histogram_rect(&[0, 0, 0]), None);
        assert_eq!(largest_histogram_rect(&[4, 3, 2, 1]), Some((0, 2, 3)));
        // Width beats height on equal area (first maximum scanning right).
        assert_eq!(largest_histogram_rect(&[1, 1, 1, 1]), Some((0, 4, 1)));
        // A taller bar in the middle is the best rectangle.
        assert_eq!(largest_histogram_rect(&[1, 5, 1]), Some((1, 1, 5)));
    }

    #[test]
    fn fully_transparent_frame_has_no_content_rect() {
        let frame = frame_with(3, 3, |_, _| false);
        assert_eq!(maximum_content_rect(&frame), None);
    }

    #[test]
    fn content_rect_contains_no_transparent_pixel() {
        // A small transparent blob in one corner: the chosen rect must avoid
        // it while still maximizing area.
        let frame = frame_with(6, 4, |x, y| !(x == 0 && y == 0));
        let (x, y, w, h) = maximum_content_rect(&frame).unwrap();
        assert_eq!((x, y, w, h), (1, 0, 5, 4));
    }
}
