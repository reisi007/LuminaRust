//! LuminaRust Lensfun integration (F-098-N1).
//!
//! A safe Rust wrapper around the system `liblensfun` (LGPL-3.0) C library
//! for automatic lens correction (distortion + vignetting) when a matching
//! camera/lens profile is found in the installed database.
//!
//! The crate is intentionally feature-gated: without the `native` feature it
//! compiles to an **empty** library with no link dependencies, so default and
//! WASM builds stay green without `liblensfun` present. Linking is dynamic
//! (`-llensfun`, performed by `build.rs` via `pkg-config`) and only happens
//! when `native` is enabled.
//!
//! Licensing: Lensfun is LGPL-3.0 and its profile database CC-BY-SA. Because
//! we link dynamically, the LGPL does not extend to the whole work; the
//! license text and a source offer are shipped in the release bundle (see
//! `THIRD-PARTY-NOTICES.md`).
//!
//! # Coordinate conventions
//!
//! Lensfun's coordinate system centres the image and works in pixel
//! coordinates `0 ..= width-1` / `0 ..= height-1`. [`Corrector::geometry`]
//! maps a *destination* (corrected) pixel to the *source* (distorted) pixel to
//! sample, and [`Corrector::color_gain`] applies the (position-dependent)
//! vignetting correction to a single pixel's RGB. Both are wrapped in the
//! inverse-bilinear resampling loop of the caller.

#![cfg_attr(not(feature = "native"), allow(dead_code))]

#[cfg(feature = "native")]
pub use ffi::LensfunDb;

/// Per-image lens corrector. Re-exported under the `native` feature.
#[cfg(feature = "native")]
pub use ffi::Corrector;

#[cfg(feature = "native")]
mod ffi {
    //! Hand-written `extern "C"` bindings to the system `liblensfun` (0.3.x).
    //!
    //! We deliberately avoid `bindgen`/`cc`: the C API surface we need is
    //! small and stable, and the build only invokes `pkg-config` to locate the
    //! dylib. All unsafe FFI is encapsulated here; the safe wrapper lives in the
    //! parent module. The signatures follow `/opt/homebrew/include/lensfun/lensfun.h`.
    #![allow(non_camel_case_types, non_snake_case, dead_code)]

    use std::os::raw::{c_char, c_float, c_int, c_void};

    // ---- modifier / correction flags (lfModifier flag bitmask) ----
    pub const LF_MODIFY_DISTORTION: c_int = 0x0000_0008;
    pub const LF_MODIFY_VIGNETTING: c_int = 0x0000_0002;
    pub const LF_MODIFY_TCA: c_int = 0x0000_0001;

    // ---- database search flags ----
    pub const LF_SEARCH_LOOSE: c_int = 1;

    // ---- pixel format (lfPixelFormat): U8=0, U16=1, U32=2, F32=3, F64=4 ----
    pub const LF_PF_F32: c_int = 3;

    // ---- lens type (lfLensType): UNKNOWN=0, RECTILINEAR=1, ... ----
    pub const LF_RECTILINEAR: c_int = 1;

    // ---- component roles (lfComponentRole) ----
    pub const LF_CR_END: c_int = 0;
    pub const LF_CR_RED: c_int = 4;
    pub const LF_CR_GREEN: c_int = 5;
    pub const LF_CR_BLUE: c_int = 6;

    /// Build `LF_CR_3(a, b, c)` role mask: `a | (b << 4) | (c << 8)`.
    pub const fn lf_cr_3(a: c_int, b: c_int, c: c_int) -> c_int {
        a | (b << 4) | (c << 8)
    }

    /// RGB component-role mask used for per-pixel colour modification.
    pub const LF_CR_RGB: c_int = lf_cr_3(LF_CR_RED, LF_CR_GREEN, LF_CR_BLUE);

    #[repr(C)]
    pub struct lfDatabase {
        _private: [u8; 0],
    }
    #[repr(C)]
    pub struct lfCamera {
        _private: [u8; 0],
    }
    #[repr(C)]
    pub struct lfLens {
        _private: [u8; 0],
    }
    #[repr(C)]
    pub struct lfModifier {
        _private: [u8; 0],
    }

    extern "C" {
        pub fn lf_db_new() -> *mut lfDatabase;
        pub fn lf_db_destroy(db: *mut lfDatabase);
        /// Returns an `lfError` (0 == LF_NO_ERROR).
        pub fn lf_db_load(db: *mut lfDatabase) -> c_int;
        /// Frees memory allocated by the database search functions.
        pub fn lf_free(data: *mut c_void);

        pub fn lf_db_find_cameras_ext(
            db: *const lfDatabase,
            maker: *const c_char,
            model: *const c_char,
            sflags: c_int,
        ) -> *mut *const lfCamera;

        /// High-level lens search: lenses matching `camera` (mount/crop) and
        /// the human `lens` description. Returns a NULL-terminated array.
        pub fn lf_db_find_lenses_hd(
            db: *const lfDatabase,
            camera: *const lfCamera,
            maker: *const c_char,
            lens: *const c_char,
            sflags: c_int,
        ) -> *mut *const lfLens;

        pub fn lf_modifier_new(
            lens: *const lfLens,
            crop: c_float,
            width: c_int,
            height: c_int,
        ) -> *mut lfModifier;

        pub fn lf_modifier_destroy(modifier: *mut lfModifier);

        /// Returns the bitmask of actually-enabled corrections (0 == nothing).
        pub fn lf_modifier_initialize(
            modifier: *mut lfModifier,
            lens: *const lfLens,
            format: c_int,
            focal: c_float,
            aperture: c_float,
            distance: c_float,
            scale: c_float,
            targeom: c_int,
            flags: c_int,
            reverse: c_int,
        ) -> c_int;

        /// Maps a destination (corrected) pixel to the source (distorted)
        /// pixel. `res` must hold `width * height * 2` floats.
        pub fn lf_modifier_apply_geometry_distortion(
            modifier: *mut lfModifier,
            xu: c_float,
            yu: c_float,
            width: c_int,
            height: c_int,
            res: *mut c_float,
        ) -> c_int;

        /// Applies the enabled colour callbacks (here: vignetting) to `pixels`.
        pub fn lf_modifier_apply_color_modification(
            modifier: *mut lfModifier,
            pixels: *mut c_void,
            x: c_float,
            y: c_float,
            width: c_int,
            height: c_int,
            comp_role: c_int,
            row_stride: c_int,
        ) -> c_int;
    }

    /// Read `lfCamera::CropFactor`.
    ///
    /// `lfCamera` is a plain-old-data struct (its destructor is **not** virtual,
    /// so there is no vtable); its data members are declared in order:
    /// `Maker`, `Model`, `Variant` (`char*`), `Mount` (`char*`), `CropFactor`
    /// (`float`), `Score` (`int`). The crop factor therefore sits at byte offset
    /// `4 * size_of::<*const c_void>()` (after the four pointer fields).
    ///
    /// `crop` is in the *denominator* of lensfun's `coordinate_correction`, so
    /// a zero/invalid value would divide by zero. We clamp to a sane range and
    /// fall back to `1.0` if the read is implausible.
    ///
    /// # Safety
    /// `cam` must be a valid, non-dangling `lfCamera` pointer (or null).
    pub unsafe fn lf_camera_crop_factor(cam: *const lfCamera) -> f32 {
        if cam.is_null() {
            return 1.0;
        }
        let offset = 4 * std::mem::size_of::<*const c_void>();
        let crop_ptr = (cam as *const u8).add(offset) as *const f32;
        let crop = *crop_ptr;
        if crop.is_finite() && crop > 0.1 && crop < 100.0 {
            crop
        } else {
            1.0
        }
    }

    /// Handle to the system Lensfun database, loaded once.
    pub struct LensfunDb {
        db: *mut lfDatabase,
    }

    impl LensfunDb {
        /// Load the system Lensfun database (via the standard search paths).
        /// Returns `None` if the library or database cannot be initialised.
        pub fn load_system() -> Option<LensfunDb> {
            unsafe {
                let db = lf_db_new();
                if db.is_null() {
                    return None;
                }
                // `lf_db_load` searches the system database directories
                // (e.g. /opt/homebrew/share/lensfun/version_1).
                let err = lf_db_load(db);
                if err != 0 {
                    lf_db_destroy(db);
                    return None;
                }
                Some(LensfunDb { db })
            }
        }

        /// Build a lens corrector for the given camera/lens, or `None` if no
        /// matching, non-identity profile is found.
        ///
        /// `lens_name` is an optional human-readable lens description; when
        /// `None` only the camera is used to pick a lens. `width`/`height` are
        /// the image dimensions the correction is computed for (must be > 0).
        #[allow(clippy::too_many_arguments)]
        pub fn for_camera(
            &self,
            make: &str,
            model: &str,
            lens_name: Option<&str>,
            width: u32,
            height: u32,
            focal_length: f32,
            aperture: f32,
            distance: f32,
        ) -> Option<Corrector> {
            Corrector::for_camera(
                self,
                make,
                model,
                lens_name,
                width,
                height,
                focal_length,
                aperture,
                distance,
            )
        }
    }

    impl Drop for LensfunDb {
        fn drop(&mut self) {
            unsafe { lf_db_destroy(self.db) }
        }
    }

    /// A pre-configured Lensfun lens corrector for one image (camera/lens/focal/
    /// aperture/distance/size). Provides per-pixel geometry (distortion) and
    /// colour (vignetting) mappings.
    pub struct Corrector {
        modifier: *mut lfModifier,
        width: u32,
        height: u32,
    }

    impl Corrector {
        /// Build a corrector for the given camera/lens, or `None` if no
        /// matching profile is found or the correction would be the identity
        /// (in which case the manual LuminaRust model is preferred — graceful
        /// fallback).
        #[allow(clippy::too_many_arguments)]
        pub fn for_camera(
            db: &LensfunDb,
            make: &str,
            model: &str,
            lens_name: Option<&str>,
            width: u32,
            height: u32,
            focal_length: f32,
            aperture: f32,
            distance: f32,
        ) -> Option<Corrector> {
            if width == 0 || height == 0 {
                return None;
            }
            unsafe {
                use std::ffi::CString;
                use std::ptr;

                let make_c = CString::new(make).ok()?;
                let model_c = CString::new(model).ok()?;
                let cameras = lf_db_find_cameras_ext(
                    db.db,
                    make_c.as_ptr(),
                    model_c.as_ptr(),
                    LF_SEARCH_LOOSE,
                );
                if cameras.is_null() {
                    return None;
                }
                let camera = *cameras;
                lf_free(cameras as *mut c_void);
                if camera.is_null() {
                    return None;
                }
                let crop = lf_camera_crop_factor(camera);

                let lens_c = lens_name.and_then(|n| CString::new(n).ok());
                let lenses = lf_db_find_lenses_hd(
                    db.db,
                    camera,
                    ptr::null(),
                    lens_c.as_ref().map(|c| c.as_ptr()).unwrap_or(ptr::null()),
                    LF_SEARCH_LOOSE,
                );
                if lenses.is_null() {
                    return None;
                }
                let lens = *lenses;
                lf_free(lenses as *mut c_void);
                if lens.is_null() {
                    return None;
                }

                let modifier = lf_modifier_new(lens, crop, width as c_int, height as c_int);
                if modifier.is_null() {
                    return None;
                }

                // Build a *correction* (reverse = false) modifier:
                //  - distortion maps a corrected (destination) pixel to the
                //    distorted (source) pixel to sample, and
                //  - vignetting divides by the falloff (flattens edges).
                // CA (TCA) is intentionally NOT enabled — it stays manual in
                // LuminaRust (documented MVP limit, F-098-N1).
                let flags = LF_MODIFY_DISTORTION | LF_MODIFY_VIGNETTING;
                let enabled = lf_modifier_initialize(
                    modifier,
                    lens,
                    LF_PF_F32,
                    focal_length as c_float,
                    aperture as c_float,
                    distance as c_float,
                    1.0,
                    LF_RECTILINEAR,
                    flags,
                    0,
                );
                // `enabled` is the bitmask of actually set-up corrections; 0
                // means the profile had no usable calibration for these
                // parameters → treat as a no-op (fall back to manual model).
                if enabled == 0 {
                    lf_modifier_destroy(modifier);
                    return None;
                }

                let corrector = Corrector {
                    modifier,
                    width,
                    height,
                };
                // No-op detection: if the correction is ~identity at
                // representative points, fall back to the manual model.
                if corrector.is_identity() {
                    return None;
                }
                Some(corrector)
            }
        }

        /// Map a destination pixel `(x, y)` (in `[0, width-1] × [0, height-1]`)
        /// to the source pixel to sample (the Lensfun correction mapping).
        pub fn geometry(&self, x: f64, y: f64) -> (f64, f64) {
            unsafe {
                let mut res = [0.0_f32; 2];
                lf_modifier_apply_geometry_distortion(
                    self.modifier,
                    x as c_float,
                    y as c_float,
                    1,
                    1,
                    res.as_mut_ptr(),
                );
                (res[0] as f64, res[1] as f64)
            }
        }

        /// Apply the Lensfun vignetting correction to a single pixel's RGB.
        /// `x`/`y` are the destination pixel coordinates used for the radial
        /// position. Returns the corrected RGB (same scale as the input).
        pub fn color_gain(&self, r: f32, g: f32, b: f32, x: f64, y: f64) -> (f32, f32, f32) {
            unsafe {
                let mut px = [r, g, b];
                lf_modifier_apply_color_modification(
                    self.modifier,
                    px.as_mut_ptr() as *mut c_void,
                    x as c_float,
                    y as c_float,
                    1,
                    1,
                    LF_CR_RGB,
                    0,
                );
                (px[0], px[1], px[2])
            }
        }

        /// Cheap check: does the modifier change anything at representative
        /// points? Used so a profile that is identity for the requested
        /// focal/aperture falls back to the manual model (graceful fallback).
        pub fn is_identity(&self) -> bool {
            let eps = 1e-3_f64;
            let (w, h) = (self.width as f64, self.height as f64);
            if w <= 0.0 || h <= 0.0 {
                return true;
            }
            // Geometry at centre + four corners.
            for (x, y) in [
                (w / 2.0, h / 2.0),
                (0.0, 0.0),
                (w - 1.0, 0.0),
                (0.0, h - 1.0),
                (w - 1.0, h - 1.0),
            ] {
                let (sx, sy) = self.geometry(x, y);
                if (sx - x).abs() > eps || (sy - y).abs() > eps {
                    return false;
                }
            }
            // Vignetting at the four corners (vs a flat 100 baseline).
            for (x, y) in [
                (0.0, 0.0),
                (w - 1.0, 0.0),
                (0.0, h - 1.0),
                (w - 1.0, h - 1.0),
            ] {
                let (r, g, b) = self.color_gain(100.0, 100.0, 100.0, x, y);
                if (r - 100.0).abs() > 0.5 || (g - 100.0).abs() > 0.5 || (b - 100.0).abs() > 0.5 {
                    return false;
                }
            }
            true
        }
    }

    impl Drop for Corrector {
        fn drop(&mut self) {
            unsafe { lf_modifier_destroy(self.modifier) }
        }
    }

    impl std::fmt::Debug for Corrector {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Corrector")
                .field("width", &self.width)
                .field("height", &self.height)
                .finish_non_exhaustive()
        }
    }
}

#[cfg(all(test, feature = "native"))]
mod tests {
    use super::ffi::{Corrector, LensfunDb};

    // These tests exercise the real system database. They only compile/run with
    // `--features native`, so the default `cargo test -p lumina-lensfun` (no
    // feature) runs nothing and stays green without liblensfun.

    // A real camera+lens pair present in the installed Lensfun DB
    // (version_1/slr-nikon.xml). The lens has both distortion and vignetting
    // calibration, so a non-identity corrector must be built.
    const MAKE: &str = "Nikon Corporation";
    const MODEL: &str = "Nikon D40";
    const LENS: &str = "Nikon AF-S DX Zoom-Nikkor 18-55mm f/3.5-5.6G VR";

    #[test]
    fn system_database_loads() {
        assert!(LensfunDb::load_system().is_some());
    }

    #[test]
    fn real_profile_yields_corrector() {
        let db = LensfunDb::load_system().expect("system lensfun db");
        let c = Corrector::for_camera(&db, MAKE, MODEL, Some(LENS), 1000, 750, 18.0, 5.6, 10.0);
        assert!(c.is_some(), "expected a matching Lensfun profile");
    }

    #[test]
    fn real_profile_distortion_deviates_at_corner() {
        let db = LensfunDb::load_system().expect("system lensfun db");
        let c = Corrector::for_camera(&db, MAKE, MODEL, Some(LENS), 1000, 750, 18.0, 5.6, 10.0)
            .expect("profile found");
        // Corner destination pixel (0, 0) must map to a different source pixel.
        let (sx, sy) = c.geometry(0.0, 0.0);
        assert!(
            (sx - 0.0).abs() > 1e-2 || (sy - 0.0).abs() > 1e-2,
            "geometry at corner should deviate from identity, got ({sx}, {sy})"
        );
    }

    #[test]
    fn real_profile_corrects_vignetting_by_brightening_corners() {
        let db = LensfunDb::load_system().expect("system lensfun db");
        let c = Corrector::for_camera(&db, MAKE, MODEL, Some(LENS), 1000, 750, 18.0, 5.6, 10.0)
            .expect("profile found");
        let centre = c.color_gain(100.0, 100.0, 100.0, 500.0, 375.0);
        let corner = c.color_gain(100.0, 100.0, 100.0, 0.0, 0.0);
        // Correction *flattens* the lens vignette, i.e. it brightens the
        // corner relative to the (already flat) centre.
        assert!(
            corner.0 > centre.0 + 1e-3,
            "corner gain {corner:?} should brighten vs centre {centre:?} (reverse=false correction)"
        );
    }

    #[test]
    fn unknown_camera_yields_none() {
        let db = LensfunDb::load_system().expect("system lensfun db");
        assert!(Corrector::for_camera(
            &db,
            "NoSuchMake__XYZ",
            "NoSuchModel__XYZ",
            None,
            1000,
            750,
            18.0,
            5.6,
            10.0,
        )
        .is_none());
    }

    #[test]
    fn zero_dimensions_yield_none() {
        let db = LensfunDb::load_system().expect("system lensfun db");
        assert!(Corrector::for_camera(&db, MAKE, MODEL, None, 0, 750, 18.0, 5.6, 10.0).is_none());
        assert!(Corrector::for_camera(&db, MAKE, MODEL, None, 1000, 0, 18.0, 5.6, 10.0).is_none());
    }
}
