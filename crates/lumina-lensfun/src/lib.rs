//! LuminaRust Lensfun integration (F-098-N1).
//!
//! A safe Rust wrapper around the system `liblensfun` (LGPL-3.0) C library
//! for automatic lens correction (distortion + vignetting) when a matching
//! camera/lens profile is found in the installed database.
//!
//! The crate is intentionally feature-gated: without the `native` feature it
//! compiles to an **empty** library with no link dependencies, so default
//! builds stay green without `liblensfun` present. Linking is dynamic
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
//! inverse-bilinear resampling loop of the caller. [`Corrector::subpixel`]
//! (G-06 Lensfun-Vollausbau) additionally maps each destination pixel to
//! per-channel source pixels when the profile carries TCA calibration
//! ([`Corrector::has_tca`]), so transverse chromatic aberration is corrected
//! in the same geometric resampling pass (green = reference, as in the
//! manual CA model).
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
//! # Where the profile database comes from (LENSFUN-DB-33)
//!
//! lensfun 0.3.4 has no runtime override, so [`LensfunDb::resolve_system`]
//! resolves the location and loads the XML files itself. A missing database is a
//! named [`SystemDbError`], never a silent no-op, and every event goes through a
//! caller-supplied [`Diagnostics`] sink — this crate has no logger.
//!
//! The system layer is upstream's `lfDatabase::Load()` competition over three
//! `version_1` directories, where "newer" is the **content of
//! `timestamp.txt`**, never a file mtime ([`db_timestamp`]). The winner is
//! [`db_path::Resolved::primary`], which is not necessarily
//! [`db_path::Resolved::dir`] — and the gap is always reported. Details and the
//! documented deviations: [`db_path`], [`db_layers`], [`db_error`], [`db_sinks`].
//!
#![cfg_attr(not(feature = "native"), allow(dead_code))]

#[cfg(feature = "native")]
pub use system_load::LensfunDb;

/// Per-image lens corrector. Re-exported under the `native` feature.
#[cfg(feature = "native")]
pub use ffi::Corrector;

/// Named, loud failures of the lookup (LENSFUN-DB-33); see [`db_error`].
#[cfg(feature = "native")]
pub use db_error::{MissReason, ProbeMiss, SystemDbError};

/// Sink for database-load events: the caller routes these and picks the level
/// (this crate has no logger); see [`db_sinks::ReportOnce`].
#[cfg(feature = "native")]
pub use db_sinks::report_once;
#[cfg(feature = "native")]
pub use system_load::{report_with, Diagnostics};

/// Row-batch FFI wrappers of [`Corrector`] (R2-LENS-01); see the module docs.
#[cfg(feature = "native")]
mod corrector_rows;

// The LENSFUN-DB-33 modules; each file carries its own module docs.
#[cfg(feature = "native")]
pub mod db_error;
#[cfg(feature = "native")]
pub mod db_layers;
#[cfg(feature = "native")]
pub mod db_path;
#[cfg(feature = "native")]
pub mod db_sinks;
#[cfg(feature = "native")]
pub mod db_timestamp;
#[cfg(feature = "native")]
pub mod db_user_dir;
#[cfg(feature = "native")]
pub mod system_load;

#[cfg(feature = "native")]
mod ffi {
    //! Hand-written `extern "C"` bindings to the system `liblensfun` (0.3.x).
    //!
    //! We deliberately avoid `bindgen`/`cc`: the C API surface we need is
    //! small and stable, and the build only invokes `pkg-config` to locate the
    //! dylib. All unsafe FFI is encapsulated here; the safe wrapper lives in the
    //! parent module. The signatures follow `/opt/homebrew/include/lensfun/lensfun.h`.
    #![allow(non_camel_case_types, non_snake_case, dead_code)]

    use super::system_load::LensfunDb;
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
    pub(crate) static LENSFUN_GLOBAL_LOCK: Mutex<()> = Mutex::new(());

    pub(crate) fn lensfun_global_lock() -> std::sync::MutexGuard<'static, ()> {
        LENSFUN_GLOBAL_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    // ---- modifier / correction flags (lfModifier flag bitmask) ----
    pub const LF_MODIFY_DISTORTION: c_int = 0x0000_0008;
    pub const LF_MODIFY_VIGNETTING: c_int = 0x0000_0002;
    pub const LF_MODIFY_TCA: c_int = 0x0000_0001;

    // ---- pixel format (lfPixelFormat): U8=0, U16=1, U32=2, F32=3, F64=4 ----
    pub const LF_PF_F32: c_int = 3;

    // ---- lens type (lfLensType): UNKNOWN=0, RECTILINEAR=1, ... ----
    pub const LF_RECTILINEAR: c_int = 1;

    // ---- component roles (lfComponentRole) ----
    pub const LF_CR_END: c_int = 0;
    pub const LF_CR_NEXT: c_int = 1;
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
        ///
        /// **Deliberately unused** (LENSFUN-DB-33): its search path is
        /// compiled into the shared library, so it is neither influenceable by
        /// `LUMINA_LENSFUN_DB` nor testable. The wrapper loads the resolved
        /// files via [`lf_db_load_file`] instead. Kept declared so the binding
        /// set stays a faithful mirror of the C API.
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

        /// Maps a destination (corrected) pixel block to the source pixels
        /// per colour channel (transverse chromatic aberration, stage 2 & 3
        /// in one step). `res` must hold `width * height * 2 * 3` floats
        /// (X/Y per pixel per R/G/B channel, channel-major: all R pairs,
        /// then G, then B — matching `ApplySubpixelGeometryDistortion`).
        ///
        /// Return contract like `lf_modifier_apply_geometry_distortion`:
        /// false (0) leaves `res` untouched (no TCA callback installed).
        /// Callers must treat false as "keep the coordinates unchanged".
        pub fn lf_modifier_apply_subpixel_geometry_distortion(
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

    /// Per-channel destination→source mapping (red, green, blue) for the
    /// TCA subpixel path (G-06 Lensfun-Vollausbau). Green is the reference
    /// channel, as in the manual CA model.
    pub type SubpixelTriple = ((f64, f64), (f64, f64), (f64, f64));

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
    /// 1. `lfModifier::lfModifier(lens, crop, w, h)` reads only *scalars*
    ///    (`CropFactor`, `AspectRatio`, `CenterX/CenterY`); the pointer is not
    ///    stored.
    /// 2. `lfModifier::Initialize(...)` interpolates the calibration into
    ///    **stack-local** `lfLensCalibVignetting`/`lfLensCalibDistortion` and
    ///    hands them to `AddColorCallbackVignetting`/`AddCoordCallbackDistortion`.
    /// 3. Both funnel through `AddCallback`, which deep-copies the payload:
    ///    `d->data = g_malloc(size); memcpy(d->data, data, size);` — and every
    ///    registration site passes non-zero `size`.
    /// 4. `~lfModifier` frees only those callback arrays.
    ///
    /// Consequently no pointer into database memory survives
    /// `lf_modifier_initialize`, so dropping the [`LensfunDb`] while a
    /// `Corrector` is alive cannot dangle — pinned by
    /// `corrector_remains_usable_after_database_is_dropped`.
    pub struct Corrector {
        pub(crate) modifier: *mut lfModifier,
        pub(crate) width: u32,
        pub(crate) height: u32,
        /// True iff lensfun set up a geometry callback
        /// (`LF_MODIFY_DISTORTION`), i.e. the profile carries distortion
        /// calibration for the requested parameters. Only then may the
        /// corrector be used geometrically ([`Corrector::geometry`]);
        /// vignetting-only profiles keep [`Corrector::geometry`] at the
        /// identity mapping (review REVIEW-LENSFUN-VIGN-1).
        pub(crate) has_distortion: bool,
        /// True iff lensfun set up a colour callback (`LF_MODIFY_VIGNETTING`),
        /// i.e. the profile carries vignetting calibration for the requested
        /// parameters.
        pub(crate) has_vignetting: bool,
        /// True iff lensfun set up a transverse-chromatic-aberration
        /// correction (`LF_MODIFY_TCA`), i.e. the profile carries TCA
        /// calibration for the requested focal length. Only then may the
        /// corrector be used for per-channel sampling
        /// ([`Corrector::subpixel`]); without TCA calibration the manual
        /// `ca_red`/`ca_blue` model stays in effect (G-06 Lensfun-Vollausbau).
        pub(crate) has_tca: bool,
    }

    impl Corrector {
        /// Build a corrector for the given camera/lens, or `None` if no
        /// profile matches or the correction is the identity (the manual
        /// LuminaRust model then applies — graceful fallback).
        ///
        /// **Strict matching (GUI-ROUTING-N6):** no fabricated camera/lens
        /// profile. A corrector may be *vignetting-only* when the profile has
        /// no distortion calibration for these parameters; it must then be used
        /// for colour correction only, its `geometry` being the identity.
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
                // GUI-ROUTING-N6: strict matching — no fabricated camera/lens.
                let cameras = lf_db_find_cameras_ext(db.db, make_c.as_ptr(), model_c.as_ptr(), 0);
                if cameras.is_null() {
                    return None;
                }
                let camera = *cameras;
                lf_free(cameras as *mut c_void);
                if camera.is_null() {
                    return None;
                }
                let crop = lf_camera_crop_factor(camera);

                // A named lens must exist; `None` keeps the body/mount fallback.
                let lens_c = lens_name
                    .filter(|name| !name.trim().is_empty())
                    .and_then(|n| CString::new(n).ok());
                let lenses = lf_db_find_lenses_hd(
                    db.db,
                    camera,
                    ptr::null(),
                    lens_c.as_ref().map(|c| c.as_ptr()).unwrap_or(ptr::null()),
                    0,
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
                //  - TCA (transverse chromatic aberration) shifts the R/B
                //    sample coordinates relative to green (G-06
                //    Lensfun-Vollausbau; replaces the manual ca_red/ca_blue
                //    model whenever the profile carries TCA calibration).
                let flags = LF_MODIFY_DISTORTION | LF_MODIFY_VIGNETTING | LF_MODIFY_TCA;
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
                    has_tca: enabled & LF_MODIFY_TCA != 0,
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
        /// to the source pixel to sample. Without a distortion callback
        /// (vignetting-only profile, [`Self::has_distortion`] `== false`)
        /// lensfun reports false and leaves `res` untouched, so the coordinates
        /// pass through unchanged instead of collapsing onto `(0, 0)`
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

        /// Whether this corrector performs transverse-chromatic-aberration
        /// correction (G-06 Lensfun-Vollausbau). Per-channel sampling via
        /// [`Self::subpixel`] is only valid when this returns `true`;
        /// otherwise the manual `ca_red`/`ca_blue` model stays in effect.
        pub fn has_tca(&self) -> bool {
            self.has_tca
        }

        /// Map a destination pixel `(x, y)` to the per-channel source pixels
        /// to sample: `(red, green, blue)` mappings (G-06 Lensfun-Vollausbau).
        ///
        /// Green is the reference channel (as in the manual CA model); red
        /// and blue are shifted relative to it by the profile's TCA
        /// calibration. Single native width=1/height=1 call (scalar path —
        /// the multi-pixel SSE lane concern documented for `geometry_row`
        /// can never trigger).
        ///
        /// If the modifier has no TCA callback (`has_tca() == false`),
        /// lensfun reports false and leaves the buffer untouched; all three
        /// mappings are then the identity `(x, y)` (same passthrough
        /// contract as [`Self::geometry`], never a silent `(0, 0)`).
        pub fn subpixel(&self, x: f64, y: f64) -> SubpixelTriple {
            unsafe {
                // Prefill with the identity mapping (false-return passthrough).
                let mut res = [
                    x as c_float,
                    y as c_float,
                    x as c_float,
                    y as c_float,
                    x as c_float,
                    y as c_float,
                ];
                let ok = lf_modifier_apply_subpixel_geometry_distortion(
                    self.modifier,
                    x as c_float,
                    y as c_float,
                    1,
                    1,
                    res.as_mut_ptr(),
                );
                if ok == 0 {
                    return ((x, y), (x, y), (x, y));
                }
                (
                    (res[0] as f64, res[1] as f64),
                    (res[2] as f64, res[3] as f64),
                    (res[4] as f64, res[5] as f64),
                )
            }
        }

        /// Apply the Lensfun vignetting correction to one pixel's RGB; `x`/`y`
        /// are its destination coordinates (the radial position). Returns the
        /// corrected RGB at the input's scale.
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

        /// Probe whether the correction is approximately the identity mapping
        /// at the image centre and four corners. A near-identity correction is
        /// treated as a no-op and the caller falls back to the manual model.
        pub fn is_identity(&self) -> bool {
            let w = f64::from(self.width);
            let h = f64::from(self.height);
            let eps = 1e-3;
            // Distortion: sampled point must map back to itself.
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
            if self.has_vignetting {
                for (x, y) in [
                    (w / 2.0, h / 2.0),
                    (0.0, 0.0),
                    (w - 1.0, 0.0),
                    (0.0, h - 1.0),
                    (w - 1.0, h - 1.0),
                ] {
                    let (r, g, b) = self.color_gain(100.0, 100.0, 100.0, x, y);
                    if (r - 100.0).abs() > 1.0 || (g - 100.0).abs() > 1.0 || (b - 100.0).abs() > 1.0
                    {
                        return false;
                    }
                }
            }
            // TCA: red/blue mappings must coincide with green at the probed
            // points (G-06). Any calibrated lateral shift makes the
            // correction visibly non-identity.
            if self.has_tca {
                for (x, y) in [
                    (w / 2.0, h / 2.0),
                    (0.0, 0.0),
                    (w - 1.0, 0.0),
                    (0.0, h - 1.0),
                    (w - 1.0, h - 1.0),
                ] {
                    let (r, g, b) = self.subpixel(x, y);
                    if (r.0 - g.0).abs() > eps
                        || (r.1 - g.1).abs() > eps
                        || (b.0 - g.0).abs() > eps
                        || (b.1 - g.1).abs() > eps
                    {
                        return false;
                    }
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
                .field("has_tca", &self.has_tca)
                .finish_non_exhaustive()
        }
    }
}

#[cfg(all(test, feature = "native"))]
mod tests;
