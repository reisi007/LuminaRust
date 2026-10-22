// ---------------------------------------------------------------------------
// GPU-LENSFUN-PARITY-1: the strict Lensfun corrector renders on the GPU through
// a caller-bound `LensfunMap` (CPU-precomputed warp/gain map) and must match the
// CPU oracle (`render_frame` with the corrector) per F-043 (maxAbsDiff/PSNR).
// ---------------------------------------------------------------------------

// SPLICE-GPU-PARITY-92 (Slice 6): reiner Move aus `parity.rs`; `use super::*` holt die Imports der Test-Wurzel.
use super::*;
#[cfg(feature = "lensfun")]
use lumina_core::{LensfunCorrectorRef, LensfunMap};
#[cfg(feature = "lensfun")]
use lumina_lensfun::{Corrector, LensfunDb};

/// Hermetic version_1 fixture: one camera + one lens with distortion (PTLens)
/// and vignetting (PA), optionally TCA (poly3) — so the map exercises both the
/// shared-coordinate and the per-channel resample path. Returns the loaded
/// database alongside the corrector (the db must outlive it).
#[cfg(feature = "lensfun")]
fn lensfun_parity_corrector(tag: &str, with_tca: bool, w: u32, h: u32) -> (Corrector, LensfunDb) {
    let tca = if with_tca {
        r#"<tca model="poly3" focal="50" vr="1.006" vb="0.994"/>"#
    } else {
        "<!-- no TCA calibration -->"
    };
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<lensdatabase>
    <camera><maker>Lumina GPU Corp</maker><model>Lumina GPU Body</model>
        <mount>LuminaGpuMount</mount><cropfactor>1.5</cropfactor></camera>
    <lens><maker>Lumina GPU Corp</maker><model>Lumina GPU 50mm f/2.8</model>
        <mount>LuminaGpuMount</mount><cropfactor>1.5</cropfactor>
        <calibration>
            <distortion model="ptlens" focal="50" a="0.06" b="-0.08" c="0.015"/>
            <vignetting model="pa" focal="50" aperture="2.8" distance="10" k1="-0.08" k2="-0.03" k3="-0.01"/>
            {tca}
        </calibration></lens>
</lensdatabase>
"#
    );
    let path = std::env::temp_dir().join(format!(
        "lumina-gpu-lensfun-{tag}-{}.xml",
        std::process::id()
    ));
    std::fs::write(&path, xml).expect("write fixture database");
    let db = LensfunDb::load_file(&path).expect("fixture database must load");
    let _ = std::fs::remove_file(&path);
    let corrector = Corrector::for_camera(
        &db,
        "Lumina GPU Corp",
        "Lumina GPU Body",
        None,
        w,
        h,
        50.0,
        2.8,
        10.0,
    )
    .expect("fixture corrector must be built");
    (corrector, db)
}

/// A distortion corrector without an explicit crop would trigger the CPU
/// oracle's content-based default crop, so the recipe carries an explicit
/// full-frame crop — the contract the GPU map binding requires.
#[cfg(feature = "lensfun")]
fn lensfun_recipe() -> EditRecipe {
    EditRecipe {
        geometry: Some(Geometry {
            version: 1,
            crop: Some(Crop::Free {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            }),
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        }),
        ..Default::default()
    }
}

#[cfg(feature = "lensfun")]
#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
fn lensfun_corrector_map_matches_cpu_oracle() {
    let mut ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped Lensfun parity");
            return;
        }
    };
    if !support::require_adapter(&ctx) {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    let (w, h) = (96u32, 72u32);
    let recipe = lensfun_recipe();
    for (variant, with_tca) in [("distortion_vignetting", false), ("tca", true)] {
        let (corrector, _db) = lensfun_parity_corrector(variant, with_tca, w, h);
        let map = LensfunMap::from_corrector(&corrector, w, h).expect("map builds");
        assert_eq!(map.has_tca(), with_tca, "fixture TCA flag");
        ctx.set_lensfun_map(Some(&map)).expect("bind map");
        for (frame_name, frame) in [
            ("gradient", gradient_frame(w, h)),
            ("noise", noise_frame(w, h, 0x5151_2222_3333_4444)),
        ] {
            let cpu = render_frame(
                &frame,
                &RenderContext {
                    recipe: &recipe,
                    camera_white_balance: None,
                    source_actions: &[],
                    masks: None,
                    lensfun: Some(LensfunCorrectorRef(&corrector)),
                    depth: None,
                },
            )
            .expect("CPU oracle render")
            .frame;
            let gpu = ctx
                .render_with_gpu(&frame, &recipe)
                .unwrap_or_else(|e| panic!("{variant}/{frame_name}: GPU render: {e}"));
            assert_eq!(
                (cpu.width(), cpu.height()),
                (gpu.width, gpu.height),
                "{variant}/{frame_name}: dimensions"
            );
            let diff = max_abs_diff(cpu.pixels(), &gpu.pixels);
            let psnr = psnr_db(cpu.pixels(), &gpu.pixels);
            eprintln!("lensfun[{variant}/{frame_name}]: maxAbsDiff={diff} psnr={psnr:.2} dB");
            assert_eq!(
                diff, 0,
                "{variant}/{frame_name}: Lensfun map resample must be byte-identical"
            );
            // GPU-LENSFUN-PARITY-1: the interactive VRAM present path (the
            // GUI's route) must render the bound map into the resident output
            // without refusing, and must refuse a distortion map without an
            // explicit crop (the unplannable content default crop) loudly.
            ctx.render_to_vram(&frame, &recipe)
                .expect("vram lensfun render");
            if !with_tca && frame_name == "gradient" {
                assert!(
                    ctx.render_to_vram(&frame, &EditRecipe::default()).is_err(),
                    "a distortion map without an explicit crop must be refused by the VRAM path"
                );
            }
        }
        ctx.set_lensfun_map(None).expect("clear map");
    }
}
