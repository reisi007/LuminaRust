//! MCP-LENSFUN-72: the Lensfun render-path boundary of `lumina-mcp`, pinned
//! by measurement.
//!
//! # The boundary (named, permanent)
//!
//! `lumina-mcp` never builds a Lensfun corrector. Every MCP render path
//! constructs its `RenderContext` with `lensfun: None`
//! (`crates/lumina-mcp/src/util.rs` — the `render_recipe` CPU and
//! camera-WB-fallback sites, and `tools/dust_removal.rs` render_out), and the
//! artifact paths hand `NoCorrector` through the shared `CorrectorSource`
//! port (`tools/generative.rs`, `tools/regenerate.rs`). The CLI
//! (`crates/lumina-cli/src/lensfun_cli.rs::build_lensfun_corrector`, driven by
//! real EXIF metadata) and the GUI resolve a profile when the metadata names
//! a camera the Lensfun database knows. With a resolved profile the two sides
//! therefore render **different pixels**; without one both sides are
//! byte-identical. Wiring a corrector into the MCP server would make its
//! pixels host-database-dependent and add liblensfun as a build dependency —
//! an architecture decision, not a bug fix. The boundary is deliberate and
//! documented in `feature/platform/mcp-server.md` (§ „Lensfun im
//! MCP-Renderpfad").
//!
//! # What this test pins
//!
//! The measured relationship, driven through the real MCP render entry
//! (`lumina_mcp::util::render_recipe`, the choke point behind
//! `lumina_preview`/`lumina_save`/`lumina_analyze`/`lumina_batch`) on one
//! side — which hard-codes `lensfun: None` — and through the shared
//! `lumina_core::render_frame` entry on the other, fed the corrector
//! `build_lensfun_corrector` would have built from the fixture's EXIF (the
//! `lumina-lensfun` call that function wraps). Both sides render the same
//! synthetic frame.
//!
//! * **No resolvable profile** (metadata names no camera): both sides are
//!   byte-identical. Unconditional — no host can resolve a profile from
//!   metadata that names no camera.
//! * **Resolvable profile** (metadata with `Nikon Corporation` / `Nikon D40`,
//!   18 mm, f/5.6 — the same identity `crates/lumina-cli/src/tests/lensfun.rs`
//!   uses and proves resolves against the system database): the sides differ
//!   **iff** this host's Lensfun database actually resolves the profile. The
//!   probe (`build_corrector`, i.e. `lumina_lensfun::LensfunDb::load_system_with`
//!   plus `for_camera`) decides which arm applies, so the test is
//!   machine-independent: on a host with the database it asserts the
//!   divergence, on a host without it the identity.
//!
//! If someone wires a corrector into the MCP render path without revisiting
//! the boundary, the divergence arm turns red on every host that has a
//! Lensfun database: the MCP side would then apply the same corrector as the
//! CLI and the bytes would match.
//!
//! # Why the comparison isolates the corrector
//!
//! Both sides render the same synthetic frame (a spatial gradient, not a
//! uniform frame — a uniform frame is invariant under lens correction), the
//! same identity recipe (`EditRecipe::default()` — what a fresh sidecar
//! stores), and pass no masks (an empty mask context is pixel-identical to
//! `None`; see `lumina-core` `render_frame`). The white-balance slot is
//! `None` on both sides. The Lensfun slot is the only difference. The
//! metadata is synthetic (the same `RawMetadata` shape
//! `crates/lumina-cli/src/tests/lensfun.rs::make_metadata` builds), carrying
//! the real EXIF field values the CLI's `build_lensfun_corrector` reads.

#![cfg(all(feature = "mcp", feature = "lensfun"))]

use lumina_mcp::util::render_recipe;
use lumina_raw::RawMetadata;
use std::path::Path;

/// The camera the CLI's own Lensfun tests use — guaranteed to resolve a
/// non-identity profile (distortion + vignetting) on any host with the
/// system Lensfun database installed.
const MAKE: &str = "Nikon Corporation";
const MODEL: &str = "Nikon D40";
/// 18 mm at f/5.6 — inside the D40 kit-lens calibration range.
const FOCAL_LENGTH: f32 = 18.0;
const APERTURE: f32 = 5.6;
/// The documented subject-distance default of the CLI wiring
/// (`crates/lumina-cli/src/lensfun_cli.rs`).
const DISTANCE: f32 = 10.0;

/// Fixture geometry — the CLI's own Lensfun unit test
/// (`crates/lumina-cli/src/tests/lensfun.rs`) uses the same 1000x750. A
/// spatial gradient, not a uniform frame: a uniform frame is invariant
/// under lens correction (distortion remaps positions, uniform → uniform),
/// so the gradient is what makes the divergence measurable in 8-bit output.
/// (Measured on this host: the D40 profile changes 2,049,954 of 3,000,000
/// bytes with a max channel delta of 254.)
const WIDTH: u32 = 1000;
const HEIGHT: u32 = 750;

// ---------------------------------------------------------------------------
// Synthetic fixture: a gradient frame + EXIF metadata
// ---------------------------------------------------------------------------

/// Builds the gradient frame (the pixel content both sides render).
fn gradient_frame() -> lumina_core::ImageFrame {
    let mut pixels = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            pixels.push(((x * 4) % 256) as u8);
            pixels.push(((y * 5) % 256) as u8);
            pixels.push(((x + y) * 2 % 256) as u8);
            pixels.push(255);
        }
    }
    lumina_core::ImageFrame::new(WIDTH, HEIGHT, pixels).expect("frame")
}

/// Builds the EXIF metadata the CLI's `build_lensfun_corrector` reads.
/// Mirrors `crates/lumina-cli/src/tests/lensfun.rs::make_metadata`.
fn make_metadata(profiled: bool) -> RawMetadata {
    RawMetadata {
        width: WIDTH,
        height: HEIGHT,
        orientation: 1,
        camera_make: profiled.then(|| MAKE.to_string()),
        camera_model: profiled.then(|| MODEL.to_string()),
        iso: None,
        shutter: None,
        aperture: profiled.then_some(APERTURE),
        lens: None,
        focal_length: profiled.then_some(FOCAL_LENGTH),
        timestamp: None,
        artist: None,
        description: None,
        camera_matrix: [[0.0; 4]; 3],
        camera_white_balance: [1.0; 4],
        pre_multipliers: [1.0; 4],
        icc_profile: None,
    }
}

// ---------------------------------------------------------------------------
// The two render sides
// ---------------------------------------------------------------------------

/// The CLI side: build the corrector the same way
/// `crates/lumina-cli/src/lensfun_cli.rs::build_lensfun_corrector` does
/// (the public-equivalent `lumina_lensfun` call that function wraps) and
/// render the frame through the shared `lumina_core::render_frame` entry —
/// the same entry the CLI drives, on the full frame so the corrector's
/// geometry is not trimmed away by the default content crop.
fn cli_render(metadata: &RawMetadata) -> lumina_core::ImageFrame {
    let corrector = build_corrector(metadata);
    let recipe = lumina_sidecar::EditRecipe::default();
    let frame = lumina_core::render_frame(
        &gradient_frame(),
        &lumina_core::RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: corrector.as_ref().map(lumina_core::LensfunCorrectorRef),
            depth: None,
        },
    )
    .expect("render");
    frame.frame
}

/// Build a corrector from metadata, mirroring
/// `crates/lumina-cli/src/lensfun_cli.rs::build_lensfun_corrector` exactly.
fn build_corrector(metadata: &RawMetadata) -> Option<lumina_lensfun::Corrector> {
    let make = metadata.camera_make.as_deref()?;
    let model = metadata.camera_model.as_deref()?;
    let focal_length = metadata.focal_length.filter(|value| value.is_finite())?;
    let aperture = metadata.aperture.filter(|value| value.is_finite())?;
    let db = lumina_lensfun::LensfunDb::load_system_with(&mut SilentDiagnostics)?;
    db.for_camera(
        make,
        model,
        metadata.lens.as_deref(),
        metadata.width,
        metadata.height,
        focal_length,
        aperture,
        DISTANCE,
    )
}

/// The MCP side: the real MCP render entry — `render_recipe`, the choke
/// point behind `lumina_preview`/`lumina_save`/`lumina_analyze`/
/// `lumina_batch`. The Lensfun slot is whatever `render_recipe` hard-codes:
/// `None`.
fn mcp_render() -> lumina_core::ImageFrame {
    let recipe = lumina_sidecar::EditRecipe::default();
    render_recipe(&gradient_frame(), &recipe, None, None).expect("the MCP render path renders")
}

// ---------------------------------------------------------------------------
// The host probe: does this host's Lensfun database resolve the profile?
// ---------------------------------------------------------------------------

struct SilentDiagnostics;

impl lumina_lensfun::system_load::Diagnostics for SilentDiagnostics {
    fn resolved(&mut self, _: &lumina_lensfun::db_path::Resolved) {}
    fn file_rejected(&mut self, _: &Path, _: &Path) {}
    fn layer_skipped(&mut self, _: &lumina_lensfun::db_layers::SkippedLayer) {}
    fn pin_displaced(&mut self, _: &lumina_lensfun::db_path::Resolved) {}
    fn failed(&mut self, _: &lumina_lensfun::db_error::SystemDbError) {}
}

fn profile_resolves_on_this_host(metadata: &RawMetadata) -> bool {
    build_corrector(metadata).is_some()
}

// ---------------------------------------------------------------------------
// Divergence stats (used in the assert messages; the measurement record)
// ---------------------------------------------------------------------------

fn divergence_stats(a: &[u8], b: &[u8]) -> (usize, u8) {
    assert_eq!(
        a.len(),
        b.len(),
        "the two renders must have the same length"
    );
    let mut bytes = 0usize;
    let mut max_delta = 0u8;
    for (x, y) in a.iter().zip(b) {
        if x != y {
            bytes += 1;
            max_delta = max_delta.max(x.abs_diff(*y));
        }
    }
    (bytes, max_delta)
}

// ---------------------------------------------------------------------------
// The two arms of the boundary
// ---------------------------------------------------------------------------

/// No resolvable profile (the metadata names no camera): both sides are
/// byte-identical — unconditionally, on every host.
#[test]
fn without_a_resolvable_profile_both_render_paths_are_byte_identical() {
    let metadata = make_metadata(false);
    let cli = cli_render(&metadata);
    let mcp = mcp_render();
    assert_eq!(
        cli, mcp,
        "without a resolvable Lensfun profile the CLI and the MCP render \
         path must be byte-identical"
    );
}

/// Resolvable profile (the metadata names the D40): the sides differ iff
/// this host's Lensfun database resolves the profile. On a host with the
/// database the CLI applies the corrector and the MCP side does not — the
/// measured divergence. On a host without it neither side corrects and the
/// bytes match. Wiring a corrector into the MCP render path turns the
/// divergence arm red on every host that has a database.
#[test]
fn with_a_resolvable_profile_only_the_cli_applies_the_corrector() {
    let metadata = make_metadata(true);
    let cli = cli_render(&metadata);
    let mcp = mcp_render();

    if profile_resolves_on_this_host(&metadata) {
        let (bytes, max_delta) = divergence_stats(cli.pixels(), mcp.pixels());
        println!(
            "MEASUREMENT profiled: cli={} bytes, mcp={} bytes, differing_bytes={bytes}, max_channel_delta={max_delta}",
            cli.pixels().len(),
            mcp.pixels().len(),
        );
        assert_ne!(
            cli, mcp,
            "with a resolved Lensfun profile the CLI (corrector applied) and \
             the MCP render path (lensfun: None) must render different \
             pixels: {bytes} bytes differ, max channel delta {max_delta}"
        );
    } else {
        assert_eq!(
            cli, mcp,
            "this host's Lensfun database resolves no profile for \
             {MAKE} {MODEL}, so neither side corrects and the bytes match"
        );
    }
}
