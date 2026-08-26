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
//! # Thread safety
//!
//! lensfun 0.3.4's database path is **not thread-safe**: lens/model name
//! parsing compiles and uses process-global POSIX regexes without any lock
//! (`GuessParameters` → `_lf_parse_lens_name`; a global `_lf_lens_regex_refs`
//! counter triggers `regfree`), so concurrent database loads/searches from
//! several threads race on the same `regex_t` (undefined behaviour — observed
//! as SIGSEGV under glibc). The safe wrapper serializes database
//! construction, search and destruction behind a global mutex
//! ([`ffi::LENSFUN_GLOBAL_LOCK`]); per-corrector `geometry`/`color_gain` calls
//! stay lock-free. Concurrent use of the public API across threads is safe;
//! the crate's tests exercise exactly that (see
//! [`tests::concurrent_db_load_and_search_is_safe`]).
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
//!
//! # Vignetting-only profiles
//!
//! A profile may carry vignetting calibration but no distortion calibration
//! for the requested parameters (the `lf_modifier_initialize` bitmask then
//! contains `LF_MODIFY_VIGNETTING` only). For such modifiers
//! `lf_modifier_apply_geometry_distortion` returns false **without writing**
//! its output buffer, so [`Corrector::geometry`] passes the coordinates
//! through unchanged ([`Corrector::has_distortion`] reports `false`).
//! Ignoring the return value collapsed every destination pixel onto
//! `(0, 0)` — a single-colour image (review REVIEW-LENSFUN-VIGN-1). Callers
//! must gate geometric use of a corrector on
//! [`Corrector::has_distortion`]; the vignetting correction of such a
//! corrector remains fully usable.
//!
//! # Row-block API and numerical equivalence (R2-LENS-01)
//!
//! [`Corrector::geometry_row`] and [`Corrector::apply_vignetting_row`] push a
//! whole destination *row* through a single FFI call each (the per-pixel
//! wrappers cost two transitions per pixel). They are **not** drop-in
//! bit-identical to the per-pixel calls: lensfun 0.3.4's block entry points
//! accumulate coordinates incrementally across the block
//! (`ApplyGeometryDistortion`: `x += NormScale` per column;
//! `ModifyColor_DeVignetting_PA`: `r2 += d1*x + d2`, `x += param[3]`),
//! whereas a 1×1 call recomputes both axes directly
//! (`x = x·NormScale − CenterX`, `r2 = x² + y²`). The results therefore agree
//! only to floating-point accumulation noise (first column/pixel exactly,
//! later columns within measured drift — see the
//! `*_row_matches_per_pixel_*` tests). Measured against lensfun 0.3.4 on
//! arm64 macOS: ≤ 7.4e-4 px geometry / ≤ 8.4e-5 gain drift at 257 columns,
//! growing to ≈ 0.75 px / ≈ 2.8e-2 at 8192 columns. Any consumer switching
//! from the per-pixel API to the row API must either accept that sub-pixel
//! drift or re-baseline its goldens first; byte-exact rendering requires
//! staying on [`Corrector::geometry`] / [`Corrector::color_gain`] (which is
//! what the lumina-core loop does).

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
    use std::sync::Mutex;

    /// Serializes every call that touches lensfun 0.3.4's *process-global*
    /// state.
    ///
    /// lensfun 0.3.4 keeps the lens/model name regexes as lazily compiled
    /// global POSIX `regex_t`s guarded only by a plain `bool` and a global
    /// `_lf_lens_regex_refs` counter (`GuessParameters` → `_lf_parse_lens_name`,
    /// `lfLens` ctor/dtor). There is **no lock**: two threads concurrently
    /// loading a database or running a search will race on `regcomp`/`regexec`/
    /// `regfree` of the same `regex_t`, which is undefined behaviour (observed
    /// as a SIGSEGV under glibc; upstream fixed this after 0.3.4 by switching
    /// to `std::regex`). The safe wrapper therefore serializes database
    /// construction, search and destruction. Per-corrector modifier calls
    /// (`geometry`/`color_gain`) and `lf_modifier_destroy` touch no global
    /// state and stay lock-free.
    static LENSFUN_GLOBAL_LOCK: Mutex<()> = Mutex::new(());

    fn lensfun_global_lock() -> std::sync::MutexGuard<'static, ()> {
        LENSFUN_GLOBAL_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

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
    pub const LF_CR_NEXT: c_int = 1;
    /// Component exists but its colour role does not matter: the callback
    /// **skips** this slot without modifying it (needed to walk interleaved
    /// RGBA buffers over the alpha byte).
    pub const LF_CR_UNKNOWN: c_int = 2;
    pub const LF_CR_RED: c_int = 4;
    pub const LF_CR_GREEN: c_int = 5;
    pub const LF_CR_BLUE: c_int = 6;

    /// Build `LF_CR_3(a, b, c)` role mask: `a | (b << 4) | (c << 8)`.
    pub const fn lf_cr_3(a: c_int, b: c_int, c: c_int) -> c_int {
        a | (b << 4) | (c << 8)
    }

    /// Build `LF_CR_4(a, b, c, d)` role mask: `a | (b << 4) | (c << 8) | (d << 12)`.
    pub const fn lf_cr_4(a: c_int, b: c_int, c: c_int, d: c_int) -> c_int {
        a | (b << 4) | (c << 8) | (d << 12)
    }

    /// RGB component-role mask used for per-pixel colour modification on a
    /// **packed** RGB buffer (`LF_CR_3(R, G, B)`): consumes exactly three
    /// consecutive floats per pixel.
    pub const LF_CR_RGB: c_int = lf_cr_3(LF_CR_RED, LF_CR_GREEN, LF_CR_BLUE);

    /// Component-role mask for **interleaved RGBA** buffers
    /// (`LF_CR_4(R, G, B, UNKNOWN)`): multiplies R, G, B and skips the alpha
    /// slot unmodified (`LF_CR_UNKNOWN`), then stops at the implicit
    /// `LF_CR_END` padding.
    pub const LF_CR_RGBA: c_int = lf_cr_4(LF_CR_RED, LF_CR_GREEN, LF_CR_BLUE, LF_CR_UNKNOWN);

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
        /// Loads one XML database file into `db`, in addition to whatever is
        /// already loaded. Returns an `lfError` (0 == LF_NO_ERROR).
        pub fn lf_db_load_file(db: *mut lfDatabase, filename: *const c_char) -> c_int;
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
        ///
        /// The return value is lensfun's `cbool`, which 0.3.x `#define`s as a
        /// plain `int` (`#define cbool int` in lensfun.h), hence `c_int`.
        /// **False (0) leaves `res` completely untouched** — for example when
        /// the modifier was initialized without distortion calibration
        /// (vignetting-only profile; review REVIEW-LENSFUN-VIGN-1). Callers
        /// must treat false as "keep the coordinates unchanged".
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
        ///
        /// Thread safety: lensfun 0.3.4's database path is not thread-safe
        /// (global lazy regex compilation, see `LENSFUN_GLOBAL_LOCK`); the
        /// wrapper serializes it, so concurrent calls from several threads
        /// are safe.
        pub fn load_system() -> Option<LensfunDb> {
            let _guard = lensfun_global_lock();
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

        /// Load a Lensfun database from one XML file (instead of the system
        /// search paths). Returns `None` if the database cannot be initialised
        /// or the file does not exist or fails to parse.
        ///
        /// Thread safety: serialized behind [`LENSFUN_GLOBAL_LOCK`], like
        /// [`Self::load_system`].
        pub fn load_file(path: &std::path::Path) -> Option<LensfunDb> {
            // Runs before any C allocation, so `?` cannot leak the handle.
            let as_str = path.to_str()?;
            let c_path = match std::ffi::CString::new(as_str) {
                Ok(c) => c,
                Err(_) => return None,
            };
            let _guard = lensfun_global_lock();
            unsafe {
                let db = lf_db_new();
                if db.is_null() {
                    return None;
                }
                let err = lf_db_load_file(db, c_path.as_ptr());
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
            // The database destructor deletes its `lfLens` objects, which
            // decrements lensfun's global regex refcount (and may `regfree` the
            // shared regexes) — must not race with a concurrent load/search.
            let _guard = lensfun_global_lock();
            unsafe { lf_db_destroy(self.db) }
        }
    }

    /// A pre-configured Lensfun lens corrector for one image (camera/lens/focal/
    /// aperture/distance/size). Provides per-pixel geometry (distortion) and
    /// colour (vignetting) mappings.
    ///
    /// # Lifetime invariant / `'static` safety (R2-LENS-02)
    ///
    /// A `Corrector` owns nothing but a `*mut lfModifier`, yet it is built
    /// from an `lfLens` that is **owned by the database** and destroyed by
    /// `lf_db_destroy`. The struct is nevertheless valid for `'static` —
    /// outliving the [`LensfunDb`] handle — because lensfun 0.3.4 never
    /// retains the `lfLens` pointer inside the modifier. Verified against the
    /// v0.3.4 sources (`libs/lensfun/modifier.cpp`, tag `v0.3.4`):
    ///
    /// 1. `lfModifier::lfModifier(lens, crop, width, height)` reads only
    ///    *scalars* (`lens->CropFactor`, `lens->AspectRatio`,
    ///    `lens->CenterX/CenterY`) into its own members
    ///    (`NormScale`, `NormUnScale`, `NormalizedInMillimeters`,
    ///    `CenterX/CenterY`); the pointer itself is not stored.
    /// 2. `lfModifier::Initialize(...)` interpolates the calibration for the
    ///    requested focal/aperture/distance into **stack-local** structs
    ///    (`lfLensCalibVignetting lcv; lfLensCalibDistortion lcd;`) and hands
    ///    them to `AddColorCallbackVignetting` / `AddCoordCallbackDistortion`.
    /// 3. Both funnel through `lfModifier::AddCallback`, which deep-copies the
    ///    payload into modifier-owned memory:
    ///    `d->data = g_malloc(data_size); memcpy(d->data, data, data_size);`.
    ///    All registration sites pass non-zero `data_size`.
    /// 4. `lfModifier::~lfModifier` frees only those callback arrays.
    ///
    /// Consequently no pointer into database memory survives
    /// `lf_modifier_initialize`, and dropping the [`LensfunDb`] while a
    /// `Corrector` is alive cannot dangle. This invariant is pinned by the
    /// `corrector_remains_usable_after_database_is_dropped` test.
    pub struct Corrector {
        modifier: *mut lfModifier,
        width: u32,
        height: u32,
        /// True iff lensfun set up a geometry callback
        /// (`LF_MODIFY_DISTORTION`), i.e. the profile carries distortion
        /// calibration for the requested parameters. Only then may the
        /// corrector be used geometrically ([`Corrector::geometry`]);
        /// vignetting-only profiles keep [`Corrector::geometry`] at the
        /// identity mapping (review REVIEW-LENSFUN-VIGN-1).
        has_distortion: bool,
        /// True iff lensfun set up a colour callback (`LF_MODIFY_VIGNETTING`),
        /// i.e. the profile carries vignetting calibration for the requested
        /// parameters.
        has_vignetting: bool,
    }

    impl Corrector {
        /// Build a corrector for the given camera/lens, or `None` if no
        /// matching profile is found or the correction would be the identity
        /// (in which case the manual LuminaRust model is preferred — graceful
        /// fallback).
        ///
        /// A corrector may be *vignetting-only* (`has_distortion() == false`,
        /// `has_vignetting() == true`) when the profile has no distortion
        /// calibration for these parameters. Such a corrector must only be
        /// used for colour correction; its `geometry` is the identity mapping.
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
            // The search path compiles/uses lensfun's process-global regexes
            // (`GuessParameters`) and must not run concurrently with another
            // load/search/drop.
            let _guard = lensfun_global_lock();
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
                    has_distortion: enabled & LF_MODIFY_DISTORTION != 0,
                    has_vignetting: enabled & LF_MODIFY_VIGNETTING != 0,
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
        ///
        /// If the modifier has no distortion callback (vignetting-only
        /// profile, [`Self::has_distortion`] `== false`), lensfun reports
        /// false and does not write `res`; the coordinates are then passed
        /// through **unchanged** instead of collapsing onto `(0, 0)`
        /// (review REVIEW-LENSFUN-VIGN-1).
        pub fn geometry(&self, x: f64, y: f64) -> (f64, f64) {
            unsafe {
                // Prefill with the identity mapping: on a `false` return
                // lensfun leaves the buffer untouched, so the passthrough
                // values below are correct even before we check the flag.
                let mut res = [x as f32, y as f32];
                let ok = lf_modifier_apply_geometry_distortion(
                    self.modifier,
                    x as c_float,
                    y as c_float,
                    1,
                    1,
                    res.as_mut_ptr(),
                );
                if ok == 0 {
                    return (x, y);
                }
                (res[0] as f64, res[1] as f64)
            }
        }

        /// Whether this corrector performs geometric (distortion) correction.
        /// Geometric use is only valid when this returns `true`; for
        /// vignetting-only profiles [`Self::geometry`] is the identity
        /// mapping. See review REVIEW-LENSFUN-VIGN-1.
        pub fn has_distortion(&self) -> bool {
            self.has_distortion
        }

        /// Whether this corrector performs colour (vignetting) correction.
        pub fn has_vignetting(&self) -> bool {
            self.has_vignetting
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

        /// Map a whole destination row to source coordinates in **one** FFI
        /// call (R2-LENS-01). `out` must hold at least `2 * width` floats and
        /// receives `(sx, sy)` pairs for the columns `0..width` of destination
        /// row `y`.
        ///
        /// Returns `true` when lensfun applied its distortion callback and
        /// wrote all pairs; `false` means the modifier has no geometry
        /// callback (vignetting-only profile), in which case the prefilled
        /// identity mapping is left untouched — exactly like per-pixel
        /// [`Self::geometry`].
        ///
        /// # Panics
        /// Panics if `out` cannot hold `2 * width` floats (caller contract,
        /// never silently truncated).
        ///
        /// # Numerics
        /// NOT bit-identical to per-pixel [`Self::geometry`] calls beyond the
        /// first column: lensfun's block path steps `x += NormScale`
        /// incrementally across the row instead of computing each column
        /// directly. See the crate-level "Row-block API" section and the
        /// `geometry_row_matches_per_pixel_calls_within_documented_drift`
        /// test.
        pub fn geometry_row(&self, y: f64, out: &mut [f32]) -> bool {
            let w = self.width as usize;
            assert!(
                out.len() >= 2 * w,
                "geometry_row: out needs 2 * width = {} floats, got {}",
                2 * w,
                out.len()
            );
            // Prefill with the identity mapping: on a `false` return lensfun
            // leaves the buffer untouched (same passthrough contract as
            // `geometry`).
            for i in 0..w {
                out[2 * i] = i as f32;
                out[2 * i + 1] = y as f32;
            }
            unsafe {
                lf_modifier_apply_geometry_distortion(
                    self.modifier,
                    0.0,
                    y as c_float,
                    self.width as c_int,
                    1,
                    out.as_mut_ptr(),
                ) != 0
            }
        }

        /// Apply the vignetting correction to one full row of interleaved
        /// RGBA32F pixels in **one** FFI call (R2-LENS-01). `rgba` must hold
        /// at least `4 * width` floats (`R,G,B,A` per column); only the RGB
        /// components are modified (`LF_CR_RGBA` role mask = R,G,B multiply,
        /// alpha slot skipped), alpha is left untouched — same contract as
        /// per-pixel [`Self::color_gain`].
        ///
        /// Returns lensfun's `cbool` (`false` == no colour callback active,
        /// buffer untouched).
        ///
        /// # Panics
        /// Panics if `rgba` cannot hold `4 * width` floats.
        ///
        /// # Numerics
        /// NOT bit-identical to per-pixel [`Self::color_gain`] calls beyond
        /// the first pixel: lensfun's block path accumulates `r²` and `x`
        /// incrementally across the row. See the crate-level "Row-block API"
        /// section and the
        /// `vignetting_row_matches_per_pixel_calls_within_documented_drift`
        /// test.
        pub fn apply_vignetting_row(&self, y: f64, rgba: &mut [f32]) -> bool {
            let w = self.width as usize;
            assert!(
                rgba.len() >= 4 * w,
                "apply_vignetting_row: rgba needs 4 * width = {} floats, got {}",
                4 * w,
                rgba.len()
            );
            unsafe {
                lf_modifier_apply_color_modification(
                    self.modifier,
                    rgba.as_mut_ptr() as *mut c_void,
                    0.0,
                    y as c_float,
                    self.width as c_int,
                    1,
                    LF_CR_RGBA,
                    std::mem::size_of_val(rgba) as c_int,
                ) != 0
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
            // Geometry at centre + four corners. Only probed when lensfun
            // actually enabled a distortion callback — a vignetting-only
            // modifier maps coordinates through unchanged (`geometry`),
            // which would make every probe trivially pass anyway.
            if self.has_distortion {
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
                .field("has_distortion", &self.has_distortion)
                .field("has_vignetting", &self.has_vignetting)
                .finish_non_exhaustive()
        }
    }
}

#[cfg(all(test, feature = "native"))]
mod tests {
    use super::ffi::{lf_camera_crop_factor, Corrector, LensfunDb};

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

    // A vignetting-only profile (REVIEW-LENSFUN-VIGN-1): the pre-fix wrapper
    // ignored the false return of `lf_modifier_apply_geometry_distortion`,
    // kept `res = (0, 0)` for every pixel and collapsed the corrected image
    // onto a single colour.

    const FIXTURE_CAM_MAKE: &str = "Lumina Test Corp";
    const FIXTURE_CAM_MODEL: &str = "Lumina Test Body";

    /// Writes a minimal Lensfun *version_1* database XML containing one camera
    /// and one lens with ONLY vignetting calibration (attribute layout as in
    /// the system databases) and returns its path.
    fn write_vignetting_only_fixture(tag: &str) -> std::path::PathBuf {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<lensdatabase>
    <camera>
        <maker>Lumina Test Corp</maker>
        <model>Lumina Test Body</model>
        <mount>LuminaTestMount</mount>
        <cropfactor>1.5</cropfactor>
    </camera>
    <lens>
        <maker>Lumina Test Corp</maker>
        <model>Lumina Vignetting-Only 50mm f/2.8</model>
        <mount>LuminaTestMount</mount>
        <cropfactor>1.5</cropfactor>
        <calibration>
            <!-- No distortion, no TCA: vignetting only. -->
            <vignetting model="pa" focal="50" aperture="2.8" distance="10" k1="-0.08" k2="-0.03" k3="-0.01"/>
        </calibration>
    </lens>
</lensdatabase>
"#;
        write_fixture_xml(tag, xml)
    }

    /// Writes a minimal Lensfun *version_1* database XML with ONE lens that
    /// carries BOTH distortion (PTLens) and vignetting (PA) calibration, so a
    /// corrector built from it enables both callbacks.
    fn write_distortion_and_vignetting_fixture(tag: &str) -> std::path::PathBuf {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<lensdatabase>
    <camera>
        <maker>Lumina Test Corp</maker>
        <model>Lumina Test Body</model>
        <mount>LuminaTestMount</mount>
        <cropfactor>1.5</cropfactor>
    </camera>
    <lens>
        <maker>Lumina Test Corp</maker>
        <model>Lumina Distortion+Vignetting 50mm f/2.8</model>
        <mount>LuminaTestMount</mount>
        <cropfactor>1.5</cropfactor>
        <calibration>
            <distortion model="ptlens" focal="50" a="0.08" b="-0.10" c="0.02"/>
            <vignetting model="pa" focal="50" aperture="2.8" distance="10" k1="-0.08" k2="-0.03" k3="-0.01"/>
        </calibration>
    </lens>
</lensdatabase>
"#;
        write_fixture_xml(tag, xml)
    }

    fn write_fixture_xml(tag: &str, xml: &str) -> std::path::PathBuf {
        // Tests run in parallel threads of ONE process (`pid` alone is not
        // unique): add thread id + an atomic counter so concurrent fixtures
        // never share a file.
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "lumina-lensfun-{tag}-{}-{:?}-{seq}.xml",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, xml).expect("write fixture database");
        path
    }

    #[test]
    fn load_file_missing_file_yields_none() {
        assert!(LensfunDb::load_file(std::path::Path::new(
            "/nonexistent/lumina-lensfun-definitely-missing.xml"
        ))
        .is_none());
    }

    #[test]
    fn vignetting_only_profile_keeps_geometry_identity() {
        let path = write_vignetting_only_fixture("vignonly");
        let db = LensfunDb::load_file(&path).expect("fixture database must load");
        let _ = std::fs::remove_file(&path);

        let c = Corrector::for_camera(
            &db,
            FIXTURE_CAM_MAKE,
            FIXTURE_CAM_MODEL,
            None,
            400,
            300,
            50.0,
            2.8,
            10.0,
        )
        .expect("vignetting-only corrector must be built");

        // The profile corrects vignetting only — no distortion calibration.
        assert!(!c.has_distortion());
        assert!(c.has_vignetting());

        // Geometry must be an EXACT identity mapping for every pixel of the
        // image ("Bild unverändert"): the pre-fix behaviour returned (0, 0)
        // here and collapsed the whole image onto its top-left pixel.
        const W: u32 = 400;
        const H: u32 = 300;
        for y in 0..H {
            for x in 0..W {
                let (sx, sy) = c.geometry(f64::from(x), f64::from(y));
                assert_eq!(sx, f64::from(x), "x deviates at ({x}, {y})");
                assert_eq!(sy, f64::from(y), "y deviates at ({x}, {y})");
            }
        }

        // Fractional (off-grid) coordinates pass through unchanged as well.
        let (sx, sy) = c.geometry(123.456, 7.89);
        assert_eq!(sx, 123.456);
        assert_eq!(sy, 7.89);

        // The vignetting correction itself stays active — otherwise the
        // constructor would have dropped the corrector as an identity no-op
        // and there would be nothing left to protect.
        assert!(!c.is_identity());
        let centre = c.color_gain(100.0, 100.0, 100.0, 200.0, 150.0);
        let corner = c.color_gain(100.0, 100.0, 100.0, 0.0, 0.0);
        assert!(
            corner.0 > centre.0 + 1e-3,
            "corner gain {corner:?} should brighten vs centre {centre:?}"
        );
    }

    #[test]
    fn corrector_remains_usable_after_database_is_dropped() {
        // R2-LENS-02 guard: `Corrector` is valid for 'static although it is
        // built from an lfLens owned by the database. lensfun 0.3.4 copies
        // every calibration value into modifier-owned callback memory during
        // `lf_modifier_initialize` (see the lifetime-invariant doc on
        // `Corrector`), so destroying the database must leave the corrector
        // fully functional AND bit-for-bit unchanged. If a future lensfun
        // version ever retained a lens pointer, this test would catch it
        // (use-after-free / changed results) instead of corrupting renders
        // silently.
        let path = write_distortion_and_vignetting_fixture("afterdrop");
        let db = LensfunDb::load_file(&path).expect("fixture database must load");
        let _ = std::fs::remove_file(&path);

        let corrector = Corrector::for_camera(
            &db,
            FIXTURE_CAM_MAKE,
            FIXTURE_CAM_MODEL,
            None,
            400,
            300,
            50.0,
            2.8,
            10.0,
        )
        .expect("combined-profile corrector must be built");
        assert!(corrector.has_distortion(), "fixture enables distortion");
        assert!(corrector.has_vignetting(), "fixture enables vignetting");

        let geo_before = corrector.geometry(12.0, 34.0);
        let gain_before = corrector.color_gain(90.0, 110.0, 130.0, 350.0, 250.0);
        let identity_before = corrector.is_identity();

        drop(db); // destroys all db-owned lfLens objects

        // The corrector must still be fully usable — geometry, colour and
        // identity probe alike — with bit-identical results.
        let geo_after = corrector.geometry(12.0, 34.0);
        let gain_after = corrector.color_gain(90.0, 110.0, 130.0, 350.0, 250.0);
        assert_eq!(
            geo_before, geo_after,
            "geometry changed after the database was dropped"
        );
        assert_eq!(
            gain_before, gain_after,
            "colour gain changed after the database was dropped"
        );
        assert_eq!(identity_before, corrector.is_identity());

        // The post-drop FFI must have exercised real copied calibration data:
        // the distortion mapping deviates from the identity at the image
        // corner.
        let (sx, sy) = corrector.geometry(399.0, 299.0);
        assert!(
            (sx - 399.0).abs() > 1e-2 || (sy - 299.0).abs() > 1e-2,
            "distortion should deviate from identity at the corner, got ({sx}, {sy})"
        );
    }

    #[test]
    fn real_profile_reports_distortion_and_vignetting_flags() {
        let db = LensfunDb::load_system().expect("system lensfun db");
        let c = Corrector::for_camera(&db, MAKE, MODEL, Some(LENS), 1000, 750, 18.0, 5.6, 10.0)
            .expect("profile found");
        // The DX 18-55mm VR carries distortion AND vignetting calibration at
        // these parameters, so both callbacks must be enabled.
        assert!(c.has_distortion());
        assert!(c.has_vignetting());
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

    #[test]
    fn concurrent_db_load_and_search_is_safe() {
        // Regression test: lensfun 0.3.4's database path is NOT thread-safe
        // (global lazily compiled POSIX regexes + refcount without any lock —
        // `GuessParameters`/`_lf_parse_lens_name`, `lfLens` ctor/dtor). With
        // several threads loading a database and searching at the same time,
        // the library races on `regcomp`/`regexec`/`regfree` of the same
        // `regex_t`, which crashed the parallel test harness with a SIGSEGV
        // under glibc (CI, Ubuntu 24.04) while macOS/Homebrew was unaffected.
        // The safe wrapper serializes load/search/drop, so exercising the
        // exact same concurrency must succeed. Without the wrapper lock this
        // test crashes the whole test process on glibc.
        let _db = LensfunDb::load_system().expect("system lensfun db");
        let threads: Vec<_> = (0..6)
            .map(|_| {
                std::thread::spawn(|| {
                    let db = LensfunDb::load_system().expect("per-thread lensfun db");
                    let c = Corrector::for_camera(
                        &db,
                        MAKE,
                        MODEL,
                        Some(LENS),
                        1000,
                        750,
                        18.0,
                        5.6,
                        10.0,
                    )
                    .expect("profile found");
                    let _ = c.geometry(0.0, 0.0);
                    drop(c);
                    drop(db);
                })
            })
            .collect();
        for handle in threads {
            handle.join().expect("lensfun worker thread must not panic");
        }
    }

    // -----------------------------------------------------------------------
    // R2-LENS-03: null/clamp paths of `lf_camera_crop_factor`.
    //
    // The function reads `lfCamera::CropFactor` at a hard-coded byte offset
    // (`4 * pointer size`, layout pinned by the build.rs probe). To exercise
    // it without liblensfun objects we synthesise an aligned buffer of the
    // same shape and write the crop factor bits into the right slot.
    // -----------------------------------------------------------------------

    /// Word index of the CropFactor field inside a `u64`-aligned fake camera
    /// buffer (byte offset `4 * pointer size`).
    const CROP_FACTOR_WORD: usize =
        (4 * std::mem::size_of::<*const std::os::raw::c_void>()) / std::mem::size_of::<u64>();

    /// A `u64`-aligned stand-in for an `lfCamera` whose `CropFactor` carries
    /// `crop`. Alignment 8 ≥ any pointer/f32 alignment, and the probed read
    /// touches only the initialised bytes at `CROP_FACTOR_WORD`.
    struct FakeCamera {
        storage: [u64; CROP_FACTOR_WORD + 2],
    }

    impl FakeCamera {
        fn with_crop(crop: f32) -> Self {
            let mut storage = [0u64; CROP_FACTOR_WORD + 2];
            storage[CROP_FACTOR_WORD] = u64::from(crop.to_bits());
            FakeCamera { storage }
        }

        /// # Safety
        /// Borrow stays alive for `'a`; the buffer is fully initialised and
        /// correctly aligned, so reading the CropFactor offset is in bounds.
        unsafe fn as_ptr(&self) -> *const super::ffi::lfCamera {
            self.storage.as_ptr().cast()
        }
    }

    #[test]
    fn crop_factor_falls_back_to_one_on_null_pointer() {
        unsafe {
            assert_eq!(lf_camera_crop_factor(std::ptr::null()), 1.0);
        }
    }

    #[test]
    fn crop_factor_clamps_implausible_values_to_one() {
        // NaN, ±inf, non-positive, the excluded boundaries (0.1, 100.0 — the
        // validity predicate is strictly `> 0.1 && < 100.0`) and oversized
        // factors must all fall back to exactly 1.0.
        for bad in [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            0.0,
            -0.0,
            -1.5,
            0.1,
            100.0,
            250.0,
        ] {
            let cam = FakeCamera::with_crop(bad);
            let got = unsafe { lf_camera_crop_factor(cam.as_ptr()) };
            assert_eq!(got, 1.0, "crop factor {bad} must clamp to 1.0");
        }
    }

    #[test]
    fn crop_factor_passes_plausible_values_through_bit_unchanged() {
        for good in [0.11_f32, 0.15, 1.0, 1.5, 2.0, 42.0, 99.9] {
            let cam = FakeCamera::with_crop(good);
            let got = unsafe { lf_camera_crop_factor(cam.as_ptr()) };
            assert_eq!(got, good, "plausible crop factor {good} must pass through");
        }
    }

    // -----------------------------------------------------------------------
    // R2-LENS-01: row-block wrappers vs per-pixel calls.
    //
    // lensfun 0.3.4's block entry points accumulate coordinates INCREMENTALLY
    // across the block (`ApplyGeometryDistortion`: `x += NormScale` per
    // column; `ModifyColor_DeVignetting_PA`: `r2 += d1*x + d2`,
    // `x += param[3]`), whereas every 1×1 call recomputes both axes directly.
    // Row results therefore agree with per-pixel results only to fp
    // accumulation noise: bit-exact in the first column/pixel, drifting by
    // tiny amounts afterwards. These tests pin that relationship honestly:
    // exact equality where it provably holds, documented drift bounds where
    // it does not. Byte-exact rendering MUST keep using the per-pixel API
    // (the lumina-core loop does; see crate docs "Row-block API").
    // -----------------------------------------------------------------------

    const ROW_W: u32 = 257; // odd width → exercises ragged SIMD tails on x86
    const ROW_H: u32 = 61;

    /// Documented drift bound for row-vs-pixel geometry coordinates, in
    /// destination pixels, **at this test's probe width (`ROW_W = 257`)**.
    /// Measured worst case over the full probe grid below: ≤ 7.4e-4 px
    /// (synthetic fixture) / ≤ 2.3e-4 px (system DB profile). The bound
    /// carries one order of magnitude of headroom for platform dispatch
    /// differences (x86 SSE callback variants). NOTE the scaling: lensfun
    /// accumulates incrementally across the row, so drift grows with width
    /// (measured ≈ 0.75 px at 8192 columns) — see the crate-level
    /// "Row-block API" section for why byte-exact rendering must not switch.
    const GEOMETRY_ROW_DRIFT_PX: f64 = 1e-2;
    /// Documented drift bound for row-vs-pixel vignetting gains, applied to a
    /// 100.0 baseline (= 1/25500 of one 8-bit code value). Measured worst
    /// case at `ROW_W = 257`: ≤ 8.4e-5 (fixture) / ≤ 3.1e-4 (system profile);
    /// grows to ≈ 2.8e-2 at 8192 columns.
    const VIGNETTING_ROW_DRIFT: f32 = 1e-2;

    fn combined_fixture_corrector() -> Corrector {
        let path = write_distortion_and_vignetting_fixture("rowequiv");
        let db = LensfunDb::load_file(&path).expect("fixture database must load");
        let _ = std::fs::remove_file(&path);
        Corrector::for_camera(
            &db,
            FIXTURE_CAM_MAKE,
            FIXTURE_CAM_MODEL,
            None,
            ROW_W,
            ROW_H,
            50.0,
            2.8,
            10.0,
        )
        .expect("combined-profile corrector must be built")
    }

    fn system_profile_corrector() -> Option<Corrector> {
        let db = LensfunDb::load_system()?;
        Corrector::for_camera(&db, MAKE, MODEL, Some(LENS), ROW_W, ROW_H, 18.0, 5.6, 10.0)
    }

    #[test]
    fn geometry_row_first_column_is_bit_identical_to_per_pixel_call() {
        // Both call shapes evaluate the identical expression chain for
        // column 0 (`x·NormScale − CenterX` with no accumulated steps yet),
        // so the first pair must match EXACTLY for every row and every
        // corrector kind.
        for c in [
            combined_fixture_corrector(),
            system_profile_corrector().expect("system profile"),
        ] {
            assert!(c.has_distortion());
            let mut out = vec![0f32; 2 * ROW_W as usize];
            for y in 0..ROW_H {
                let y = f64::from(y);
                assert!(c.geometry_row(y, &mut out));
                let (px, py) = c.geometry(0.0, y);
                assert_eq!(out[0] as f64, px, "geometry row col-0 sx at y={y}");
                assert_eq!(out[1] as f64, py, "geometry row col-0 sy at y={y}");
            }
        }
    }

    #[test]
    fn geometry_row_matches_per_pixel_calls_within_documented_drift() {
        let mut max_drift = 0.0f64;
        for c in [
            combined_fixture_corrector(),
            system_profile_corrector().expect("system profile"),
        ] {
            let mut out = vec![0f32; 2 * ROW_W as usize];
            for y in 0..ROW_H {
                let y = f64::from(y);
                assert!(c.geometry_row(y, &mut out));
                for x in 0..ROW_W {
                    let x = f64::from(x);
                    let (px, py) = c.geometry(x, y);
                    let dx = (f64::from(out[2 * x as usize]) - px).abs();
                    let dy = (f64::from(out[2 * x as usize + 1]) - py).abs();
                    max_drift = max_drift.max(dx).max(dy);
                }
            }
        }
        assert!(
            max_drift <= GEOMETRY_ROW_DRIFT_PX,
            "geometry row/pixel drift {max_drift} px exceeds documented bound \
             {GEOMETRY_ROW_DRIFT_PX} px"
        );
    }

    #[test]
    fn vignetting_row_first_pixel_is_bit_identical_to_per_pixel_call() {
        for c in [
            combined_fixture_corrector(),
            system_profile_corrector().expect("system profile"),
        ] {
            assert!(c.has_vignetting());
            let mut row = vec![100.0f32; 4 * ROW_W as usize];
            for y in 0..ROW_H {
                let y = f64::from(y);
                // Fresh 100.0 baseline per row: the correction multiplies in
                // place, so a reused buffer would compound across rows.
                row.fill(100.0);
                assert!(c.apply_vignetting_row(y, &mut row));
                let (pr, pg, pb) = c.color_gain(100.0, 100.0, 100.0, 0.0, y);
                assert_eq!(row[0], pr, "vignetting row pixel-0 r at y={y}");
                assert_eq!(row[1], pg, "vignetting row pixel-0 g at y={y}");
                assert_eq!(row[2], pb, "vignetting row pixel-0 b at y={y}");
                // Alpha untouched by the LF_CR_RGB role mask.
                assert_eq!(row[3], 100.0, "alpha must stay untouched");
            }
        }
    }

    #[test]
    fn vignetting_row_matches_per_pixel_calls_within_documented_drift() {
        let mut max_drift = 0.0f32;
        for c in [
            combined_fixture_corrector(),
            system_profile_corrector().expect("system profile"),
        ] {
            let mut row = vec![100.0f32; 4 * ROW_W as usize];
            for y in 0..ROW_H {
                let y = f64::from(y);
                // Fresh 100.0 baseline per row (see first-pixel test).
                row.fill(100.0);
                assert!(c.apply_vignetting_row(y, &mut row));
                for x in 0..ROW_W {
                    let (pr, pg, pb) = c.color_gain(100.0, 100.0, 100.0, f64::from(x), y);
                    let i = 4 * x as usize;
                    for (got, want) in [(row[i], pr), (row[i + 1], pg), (row[i + 2], pb)] {
                        max_drift = max_drift.max((got - want).abs());
                    }
                }
            }
        }
        assert!(
            max_drift <= VIGNETTING_ROW_DRIFT,
            "vignetting row/pixel drift {max_drift} exceeds documented bound \
             {VIGNETTING_ROW_DRIFT}"
        );
    }
}
