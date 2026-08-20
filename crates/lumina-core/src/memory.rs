//! Memory budgets for large RAW decodes and mask rasterizations (F-075).
//!
//! Before allocating large buffers (the processed RAW image or a mask matte),
//! callers check against a [`MemoryBudget`] so that oversized inputs fail fast
//! with a clear error instead of triggering an OOM/panic in the native decoder
//! or the mask rasterizer.
//!
//! Budgets are configurable via environment variables
//! (`LUMINA_MAX_RAW_PIXELS`, `LUMINA_MAX_MASK_PIXELS`, `LUMINA_MAX_ALLOC_BYTES`)
//! with a safe default applied for any variable that is missing or not a valid
//! positive integer.

/// Configurable memory budget for decode and mask allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryBudget {
    /// Maximum number of source pixels accepted for a single RAW decode.
    pub max_raw_pixels: u64,
    /// Maximum number of pixels accepted for a single mask rasterization.
    pub max_mask_pixels: u64,
    /// Maximum single-allocation size in bytes (covers the RAW processed image
    /// and the mask matte buffers).
    pub max_alloc_bytes: u64,
}

impl Default for MemoryBudget {
    fn default() -> Self {
        // 200 MP RAW, 100 MP masks, ~2.4 GiB single-allocation cap. These are
        // deliberately generous for desktop use and exist to prevent
        // pathological inputs from exhausting memory, not to constrain normal
        // photography.
        Self {
            max_raw_pixels: 200_000_000,
            max_mask_pixels: 100_000_000,
            max_alloc_bytes: 2_400_000_000,
        }
    }
}

impl MemoryBudget {
    /// Builds a budget from environment variables, falling back to [`Default`]
    /// for any variable that is missing or not a valid positive integer.
    pub fn from_env() -> Self {
        let mut budget = Self::default();
        if let Ok(value) = std::env::var("LUMINA_MAX_RAW_PIXELS") {
            if let Ok(parsed) = value.parse::<u64>() {
                budget.max_raw_pixels = parsed;
            }
        }
        if let Ok(value) = std::env::var("LUMINA_MAX_MASK_PIXELS") {
            if let Ok(parsed) = value.parse::<u64>() {
                budget.max_mask_pixels = parsed;
            }
        }
        if let Ok(value) = std::env::var("LUMINA_MAX_ALLOC_BYTES") {
            if let Ok(parsed) = value.parse::<u64>() {
                budget.max_alloc_bytes = parsed;
            }
        }
        budget
    }

    /// Checks that a RAW decode of `width × height` with `channels` ×
    /// `bytes_per_channel` per pixel fits the budget. Returns the required
    /// bytes on success.
    pub fn check_decode(
        &self,
        width: u64,
        height: u64,
        channels: u32,
        bytes_per_channel: u32,
    ) -> Result<u64, MemoryBudgetError> {
        let pixels = width
            .checked_mul(height)
            .ok_or(MemoryBudgetError::Overflow)?;
        if pixels > self.max_raw_pixels {
            return Err(MemoryBudgetError::RawPixelsExceeded {
                required: pixels,
                limit: self.max_raw_pixels,
            });
        }
        let bytes = pixels
            .checked_mul(channels as u64)
            .and_then(|value| value.checked_mul(bytes_per_channel as u64))
            .ok_or(MemoryBudgetError::Overflow)?;
        if bytes > self.max_alloc_bytes {
            return Err(MemoryBudgetError::AllocExceeded {
                required: bytes,
                limit: self.max_alloc_bytes,
            });
        }
        Ok(bytes)
    }

    /// Checks that a mask rasterization of `width × height` (u16 matte) fits
    /// the budget. Returns the required bytes on success.
    pub fn check_mask(&self, width: u64, height: u64) -> Result<u64, MemoryBudgetError> {
        let pixels = width
            .checked_mul(height)
            .ok_or(MemoryBudgetError::Overflow)?;
        if pixels > self.max_mask_pixels {
            return Err(MemoryBudgetError::MaskPixelsExceeded {
                required: pixels,
                limit: self.max_mask_pixels,
            });
        }
        let bytes = pixels
            .checked_mul(std::mem::size_of::<u16>() as u64)
            .ok_or(MemoryBudgetError::Overflow)?;
        if bytes > self.max_alloc_bytes {
            return Err(MemoryBudgetError::AllocExceeded {
                required: bytes,
                limit: self.max_alloc_bytes,
            });
        }
        Ok(bytes)
    }
}

/// Error returned when an allocation would exceed the configured
/// [`MemoryBudget`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MemoryBudgetError {
    #[error("memory budget overflow while computing allocation size")]
    Overflow,
    #[error("RAW decode pixels {required} exceed budget limit {limit}")]
    RawPixelsExceeded { required: u64, limit: u64 },
    #[error("mask pixels {required} exceed budget limit {limit}")]
    MaskPixelsExceeded { required: u64, limit: u64 },
    #[error("allocation {required} bytes exceeds budget limit {limit}")]
    AllocExceeded { required: u64, limit: u64 },
}

impl MemoryBudgetError {
    /// The allocation size (pixels or bytes) that triggered the rejection.
    pub fn required(&self) -> u64 {
        match self {
            Self::Overflow => 0,
            Self::RawPixelsExceeded { required, .. }
            | Self::MaskPixelsExceeded { required, .. }
            | Self::AllocExceeded { required, .. } => *required,
        }
    }

    /// The budget limit that was exceeded.
    pub fn limit(&self) -> u64 {
        match self {
            Self::Overflow => 0,
            Self::RawPixelsExceeded { limit, .. }
            | Self::MaskPixelsExceeded { limit, .. }
            | Self::AllocExceeded { limit, .. } => *limit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // `from_env` reads process-global environment variables; the two tests
    // below set and remove `LUMINA_MAX_*` vars. A global lock serializes them
    // so they never run concurrently: the unsynchronized env mutation is a
    // data race that intermittently breaks `from_env_falls_back_to_default_on_
    // missing_vars` (see F-075 "latente Flakiness in parallelen Tests").
    static ENV_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn default_budget_accepts_normal_sizes() {
        let budget = MemoryBudget::default();
        let bytes = budget
            .check_decode(6000, 4000, 3, 2)
            .expect("a 24 MP 16-bit RAW fits the default budget");
        assert_eq!(bytes, 6000 * 4000 * 3 * 2);
    }

    #[test]
    fn decode_rejects_too_many_pixels() {
        let budget = MemoryBudget::default();
        let error = budget
            .check_decode(20_000, 20_000, 3, 2)
            .expect_err("400 MP exceeds the 200 MP raw-pixel cap");
        assert!(matches!(error, MemoryBudgetError::RawPixelsExceeded { .. }));
        assert_eq!(error.limit(), 200_000_000);
    }

    #[test]
    fn decode_rejects_too_many_bytes() {
        let budget = MemoryBudget {
            max_raw_pixels: u64::MAX,
            max_mask_pixels: u64::MAX,
            max_alloc_bytes: 1_000,
        };
        let error = budget
            .check_decode(100, 100, 3, 2)
            .expect_err("60 KB exceeds the 1 KB alloc cap");
        assert!(matches!(error, MemoryBudgetError::AllocExceeded { .. }));
        assert_eq!(error.required(), 60_000);
    }

    #[test]
    fn mask_rejects_too_many_pixels() {
        let budget = MemoryBudget::default();
        let error = budget
            .check_mask(20_000, 20_000)
            .expect_err("400 MP exceeds the 100 MP mask-pixel cap");
        assert!(matches!(
            error,
            MemoryBudgetError::MaskPixelsExceeded { .. }
        ));
        assert_eq!(error.limit(), 100_000_000);
    }

    #[test]
    fn mask_accepts_normal_sizes() {
        let budget = MemoryBudget::default();
        let bytes = budget.check_mask(1920, 1080).expect("HD mask fits");
        assert_eq!(bytes, 1920 * 1080 * 2);
    }

    #[test]
    fn from_env_falls_back_to_default_on_missing_vars() {
        // No LUMINA_* vars are guaranteed in CI; the default must apply
        // cleanly without panicking. Serialized against `from_env_parses_
        // valid_vars`, which sets the same env vars (see ENV_TEST_LOCK).
        let _guard = ENV_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let budget = MemoryBudget::from_env();
        assert_eq!(budget, MemoryBudget::default());
    }

    #[test]
    fn from_env_parses_valid_vars() {
        let _guard = ENV_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("LUMINA_MAX_RAW_PIXELS", "12345");
        std::env::set_var("LUMINA_MAX_MASK_PIXELS", "not-a-number");
        std::env::set_var("LUMINA_MAX_ALLOC_BYTES", "999");
        let budget = MemoryBudget::from_env();
        assert_eq!(budget.max_raw_pixels, 12345);
        // Invalid value falls back to the default component.
        assert_eq!(budget.max_mask_pixels, 100_000_000);
        assert_eq!(budget.max_alloc_bytes, 999);
        std::env::remove_var("LUMINA_MAX_RAW_PIXELS");
        std::env::remove_var("LUMINA_MAX_MASK_PIXELS");
        std::env::remove_var("LUMINA_MAX_ALLOC_BYTES");
    }
}
