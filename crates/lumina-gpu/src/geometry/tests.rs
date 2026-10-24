use super::*;

#[test]
fn geometry_pipelines_validate() {
    // Guards the WGSL sources against keyword/syntax regressions (e.g. a
    // reserved word): an invalid shader would produce a pipeline that
    // silently writes nothing, which the pixel parity test would catch as
    // all-zero output — this test makes the root cause explicit.
    let ctx = match crate::GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("geometry pipeline validation skipped (no GPU context: {err})");
            return;
        }
    };
    if !ctx.is_available() {
        eprintln!("geometry pipeline validation skipped (no GPU adapter)");
        return;
    }
    let device = ctx.device().expect("device");
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let _pipelines = build_geometry_pipelines(device);
    let err = pollster::block_on(scope.pop());
    assert!(err.is_none(), "geometry shaders must validate: {err:?}");
}

fn perspective(vertical: f32) -> Perspective {
    Perspective {
        version: 1,
        vertical,
        horizontal: 0.0,
        rotation: 0.0,
        scale: 1.0,
        aspect_ratio: 1.0,
        shift_x: 0.0,
        shift_y: 0.0,
    }
}

fn lens() -> LensCorrection {
    LensCorrection {
        version: 1,
        profile: None,
        distortion_k1: Some(0.1),
        distortion_k2: None,
        distortion_k3: None,
        vignette_c0: None,
        vignette_c1: None,
        vignette_c2: None,
        ca_red: Some(0.006),
        ca_blue: Some(-0.006),
    }
}

fn recipe_with(
    perspective: Option<Perspective>,
    lens: Option<LensCorrection>,
    geometry: Option<lumina_sidecar::Geometry>,
) -> lumina_sidecar::EditRecipe {
    lumina_sidecar::EditRecipe {
        perspective,
        lens_correction: lens,
        geometry,
        ..Default::default()
    }
}

#[test]
fn no_geometry_is_none() {
    let recipe = lumina_sidecar::EditRecipe::default();
    assert!(GeometryPlan::from_recipe(&recipe, 32, 24)
        .unwrap()
        .is_none());
}

#[test]
fn neutral_perspective_is_skipped() {
    let recipe = recipe_with(Some(perspective(0.0)), None, None);
    assert!(GeometryPlan::from_recipe(&recipe, 32, 24)
        .unwrap()
        .is_none());
}

#[test]
fn perspective_changes_dims_like_oracle() {
    let frame = lumina_core::ImageFrame::new(40, 30, vec![0u8; 40 * 30 * 4]).unwrap();
    for vertical in [0.2f32, 0.8] {
        let p = perspective(vertical);
        let domain = frame
            .measurement_domain_with_perspective(None, None, Some(&p))
            .expect("oracle dimensions");
        let recipe = recipe_with(Some(p), None, None);
        let plan = GeometryPlan::from_recipe(&recipe, 40, 30)
            .unwrap()
            .expect("active");
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(
            (plan.output_width, plan.output_height),
            (domain.output_width, domain.output_height),
            "plan dimensions must match the CPU oracle's measurement domain"
        );
    }
}

/// Cross-check the planned output dimensions for a full crop + rotation +
/// perspective chain against the public oracle
/// (`ImageFrame::measurement_domain_with_perspective`), which is exactly
/// what `render_frame` produces.
#[test]
fn geometry_plan_dims_match_measurement_domain() {
    let frame = lumina_core::ImageFrame::new(64, 48, vec![0u8; 64 * 48 * 4]).unwrap();
    let geometry = lumina_sidecar::Geometry {
        version: 1,
        crop: Some(Crop::Free {
            x: 0.1,
            y: 0.15,
            width: 0.6,
            height: 0.5,
        }),
        rotation_degrees: 23.0,
        mirror_horizontal: true,
        mirror_vertical: true,
    };
    let p = perspective(0.2);
    let domain = frame
        .measurement_domain_with_perspective(Some(&geometry), None, Some(&p))
        .expect("oracle dimensions");
    let recipe = recipe_with(Some(p), None, Some(geometry));
    let plan = GeometryPlan::from_recipe(&recipe, 64, 48)
        .unwrap()
        .expect("active");
    assert_eq!(
        (plan.output_width, plan.output_height),
        (domain.output_width, domain.output_height)
    );
}

#[test]
fn crop_rect_ports_oracle_clamping() {
    // Full-frame free rect is the identity crop.
    let crop = Crop::Free {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };
    assert_eq!(crop_rect(8, 6, Some(&crop)).unwrap(), (0, 0, 8, 6));
    // Half-width centred rect.
    let crop = Crop::Free {
        x: 0.25,
        y: 0.0,
        width: 0.5,
        height: 1.0,
    };
    assert_eq!(crop_rect(8, 6, Some(&crop)).unwrap(), (2, 0, 4, 6));
}

#[test]
fn rotate_plan_matches_oracle_cases() {
    assert!(rotate_params(0.0, 8, 6).is_none());
    let (params, ow, oh) = rotate_params(90.0, 8, 6).unwrap();
    assert_eq!((ow, oh), (6, 8));
    assert_eq!(params.mode_turn[0], 0);
    assert_eq!(params.mode_turn[1], 1);
    let (params, ow, oh) = rotate_params(33.0, 8, 6).unwrap();
    assert_eq!(params.mode_turn[0], 1);
    assert_eq!((ow, oh), rotate_dimensions(8, 6, 33.0));
}

#[test]
fn lens_unknown_profile_is_loud() {
    let l = LensCorrection {
        profile: Some("mystery".into()),
        ..lens()
    };
    assert!(lens_coefficients(&l).is_err());
}

#[test]
fn lens_profile_defaults_and_overrides() {
    let l = LensCorrection {
        profile: Some("standard-neutral".into()),
        distortion_k1: None,
        ..lens()
    };
    let c = lens_coefficients(&l).unwrap();
    assert_eq!(c[0], 0.0);
    assert_eq!(c[3], 1.0);
    // Explicit fields still override profile defaults.
    assert_eq!(c[6], 0.006);
}

#[test]
fn lens_only_plan_keeps_dims_and_adds_ca() {
    let recipe = recipe_with(None, Some(lens()), None);
    let plan = GeometryPlan::from_recipe(&recipe, 20, 10)
        .unwrap()
        .expect("active");
    // lens + CA.
    assert_eq!(plan.steps.len(), 2);
    assert_eq!((plan.output_width, plan.output_height), (20, 10));
}

#[test]
fn full_chain_orders_lens_perspective_ca_crop_rotate_mirror() {
    let recipe = recipe_with(
        Some(perspective(0.2)),
        Some(lens()),
        Some(lumina_sidecar::Geometry {
            version: 1,
            crop: Some(Crop::Free {
                x: 0.1,
                y: 0.1,
                width: 0.5,
                height: 0.5,
            }),
            rotation_degrees: 12.0,
            mirror_horizontal: true,
            mirror_vertical: false,
        }),
    );
    let plan = GeometryPlan::from_recipe(&recipe, 64, 48)
        .unwrap()
        .expect("active");
    let kinds: Vec<&str> = plan
        .steps
        .iter()
        .map(|s| match s {
            GeometryStep::Lens { .. } => "lens",
            GeometryStep::Perspective { .. } => "perspective",
            GeometryStep::Ca { .. } => "ca",
            GeometryStep::Substitute { .. } => "substitute",
            GeometryStep::Lensfun { .. } => "lensfun",
            GeometryStep::Crop { .. } => "crop",
            GeometryStep::Rotate { .. } => "rotate",
            GeometryStep::Mirror { .. } => "mirror",
        })
        .collect();
    assert_eq!(
        kinds,
        vec!["lens", "perspective", "ca", "crop", "rotate", "mirror"]
    );
}
