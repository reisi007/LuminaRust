use super::*;

#[test]
fn geometry_crop_rotation_and_mirror_are_applied_in_order() {
    let mut frame = ImageFrame::new(
        3,
        2,
        vec![
            1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255, 4, 0, 0, 255, 5, 0, 0, 255, 6, 0, 0, 255,
        ],
    )
    .unwrap();
    frame
        .apply_geometry(
            Some(&lumina_sidecar::Geometry {
                version: 1,
                crop: Some(lumina_sidecar::Crop::Free {
                    x: 1.0 / 3.0,
                    y: 0.0,
                    width: 2.0 / 3.0,
                    height: 1.0,
                }),
                rotation_degrees: 0.0,
                mirror_horizontal: true,
                mirror_vertical: false,
            }),
            None,
            None,
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    assert_eq!((frame.width, frame.height), (2, 2));
    assert_eq!(
        frame.pixels,
        vec![3, 0, 0, 255, 2, 0, 0, 255, 6, 0, 0, 255, 5, 0, 0, 255]
    );
}

#[test]
fn geometry_measurement_domain_tracks_crop_and_quarter_turn() {
    let frame = ImageFrame::new(4, 2, vec![0; 32]).unwrap();
    let domain = frame
        .measurement_domain(Some(&lumina_sidecar::Geometry {
            version: 1,
            crop: Some(lumina_sidecar::Crop::Free {
                x: 0.25,
                y: 0.0,
                width: 0.5,
                height: 1.0,
            }),
            rotation_degrees: 90.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        }))
        .unwrap();
    assert_eq!((domain.output_width, domain.output_height), (2, 2));
    assert_eq!((domain.source_x, domain.source_y), (0.25, 0.0));
    assert_eq!((domain.source_width, domain.source_height), (0.5, 1.0));
}

#[test]
fn perspective_identity_preserves_bytes_and_dimensions() {
    let pixels = vec![
        10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255,
    ];
    let mut rendered = ImageFrame::new(2, 2, pixels.clone()).unwrap();
    let p = test_perspective();
    rendered
        .apply_geometry(
            None,
            None,
            Some(&p),
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    assert_eq!((rendered.width, rendered.height), (2, 2));
    assert_eq!(rendered.pixels, pixels);
    let domain = rendered
        .measurement_domain_with_perspective(None, None, Some(&p))
        .unwrap();
    assert_eq!((domain.output_width, domain.output_height), (2, 2));
}

#[test]
fn perspective_scale_two_doubles_bounding_box_and_projects_corners() {
    let pixels = vec![10, 0, 0, 255, 20, 0, 0, 255, 30, 0, 0, 255, 40, 0, 0, 255];
    let mut rendered = ImageFrame::new(2, 2, pixels).unwrap();
    let mut p = test_perspective();
    p.scale = 2.0;
    rendered
        .apply_geometry(
            None,
            None,
            Some(&p),
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    assert_eq!((rendered.width, rendered.height), (4, 4));
    assert_eq!(rendered.pixels[0], 10);
    assert_eq!(rendered.pixels[3 * 4], 20);
    assert_eq!(rendered.pixels[(3 * 4) * rendered.width as usize], 30);
    assert_eq!(rendered.pixels[((4 * rendered.width - 1) * 4) as usize], 40);
    let domain = ImageFrame::new(2, 2, vec![0; 16])
        .unwrap()
        .measurement_domain_with_perspective(None, None, Some(&p))
        .unwrap();
    assert_eq!((domain.output_width, domain.output_height), (4, 4));
}

#[test]
fn perspective_shift_rotation_and_direction_change_rendered_geometry() {
    let base =
        ImageFrame::new(4, 2, (0..8).flat_map(|v| [v * 20, 100, 50, 255]).collect()).unwrap();
    let mut shifted = base.clone();
    let mut p = test_perspective();
    p.shift_x = 0.5;
    p.shift_y = -0.25;
    shifted
        .apply_perspective_stage(
            None,
            Some(&p),
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    assert_eq!((shifted.width, shifted.height), (4, 2));
    assert_ne!(shifted.pixels, base.pixels);

    let mut rotated = base.clone();
    p = test_perspective();
    p.rotation = 1.0;
    rotated
        .apply_perspective_stage(
            None,
            Some(&p),
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    assert!(rotated.width > base.width && rotated.height > base.height);

    for (vertical, horizontal) in [(0.5, 0.0), (0.0, 0.5), (-0.5, 0.0), (0.0, -0.5)] {
        let mut directional = base.clone();
        p = test_perspective();
        p.vertical = vertical;
        p.horizontal = horizontal;
        directional
            .apply_perspective_stage(
                None,
                Some(&p),
                #[cfg(feature = "lensfun")]
                None,
            )
            .unwrap();
        assert!(directional.width >= base.width && directional.height >= base.height);
    }
}

#[test]
fn perspective_combines_lens_then_perspective_then_crop_and_measurement() {
    let mut frame =
        ImageFrame::new(8, 4, (0..32).flat_map(|v| [v, 100, 50, 255]).collect()).unwrap();
    let lens = lumina_sidecar::LensCorrection {
        version: 1,
        profile: None,
        distortion_k1: Some(0.2),
        distortion_k2: None,
        distortion_k3: None,
        vignette_c0: Some(1.0),
        vignette_c1: None,
        vignette_c2: None,
        ca_red: None,
        ca_blue: None,
    };
    let mut p = test_perspective();
    p.scale = 2.0;
    p.aspect_ratio = 0.5;
    let geometry = lumina_sidecar::Geometry {
        version: 1,
        crop: Some(lumina_sidecar::Crop::Free {
            x: 0.25,
            y: 0.25,
            width: 0.5,
            height: 0.5,
        }),
        rotation_degrees: 0.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    };
    let expected = frame
        .measurement_domain_with_perspective(Some(&geometry), Some(&lens), Some(&p))
        .unwrap();
    frame
        .apply_geometry(
            Some(&geometry),
            Some(&lens),
            Some(&p),
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    assert_eq!(
        (frame.width, frame.height),
        (expected.output_width, expected.output_height)
    );
    assert!(frame.pixels.iter().any(|&v| v != 0));
}

#[test]
fn lens_explicit_zero_overrides_profile_and_invalid_geometry_is_rejected() {
    let source =
        ImageFrame::new(5, 5, (0..25).flat_map(|v| [v * 7, 20, 30, 255]).collect()).unwrap();
    let profile = lumina_sidecar::LensCorrection {
        version: 1,
        profile: Some("wide-light".into()),
        distortion_k1: None,
        distortion_k2: None,
        distortion_k3: None,
        vignette_c0: None,
        vignette_c1: None,
        vignette_c2: None,
        ca_red: None,
        ca_blue: None,
    };
    let mut explicit = profile.clone();
    explicit.distortion_k1 = Some(0.0);
    let mut a = source.clone();
    let mut b = source.clone();
    a.apply_geometry(
        None,
        Some(&profile),
        None,
        #[cfg(feature = "lensfun")]
        None,
    )
    .unwrap();
    b.apply_geometry(
        None,
        Some(&explicit),
        None,
        #[cfg(feature = "lensfun")]
        None,
    )
    .unwrap();
    assert_ne!(a.pixels, b.pixels);

    for bad in [
        lumina_sidecar::Perspective {
            version: 2,
            ..test_perspective()
        },
        lumina_sidecar::Perspective {
            scale: f32::NAN,
            ..test_perspective()
        },
    ] {
        assert!(source
            .clone()
            .apply_geometry(
                None,
                None,
                Some(&bad),
                #[cfg(feature = "lensfun")]
                None
            )
            .is_err());
    }
    let bad_lens = lumina_sidecar::LensCorrection {
        distortion_k1: Some(1.1),
        ..profile
    };
    assert!(source
        .clone()
        .apply_geometry(
            None,
            Some(&bad_lens),
            None,
            #[cfg(feature = "lensfun")]
            None
        )
        .is_err());
    let bad_ca = lumina_sidecar::LensCorrection {
        ca_red: Some(f32::NAN),
        ..bad_lens
    };
    assert!(source
        .clone()
        .apply_geometry(
            None,
            Some(&bad_ca),
            None,
            #[cfg(feature = "lensfun")]
            None
        )
        .is_err());
}

// GEN-PIPELINE-DECOUPLE: the extracted crop stage is byte-identical to
// the legacy 5-in-1 tail, and the legacy entry points delegate to the
// decoupled stages without changing a pixel.
#[test]
fn crop_stage_matches_legacy_apply_geometry_tail() {
    fn frame() -> ImageFrame {
        let mut pixels = Vec::with_capacity(12 * 8 * 4);
        for y in 0..8 {
            for x in 0..12 {
                let v = ((x * 17 + y * 31) % 256) as u8;
                pixels.extend_from_slice(&[v, 255 - v, v / 2, 255]);
            }
        }
        ImageFrame::new(12, 8, pixels).unwrap()
    }
    let geometry = lumina_sidecar::Geometry {
        version: 1,
        crop: Some(lumina_sidecar::Crop::Aspect {
            preset: lumina_sidecar::AspectPreset::OneToOne,
        }),
        rotation_degrees: 90.0,
        mirror_horizontal: true,
        mirror_vertical: false,
    };
    let mut legacy = frame();
    legacy
        .apply_geometry(
            Some(&geometry),
            None,
            None,
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    let mut staged = frame();
    staged
        .apply_lens_stage(
            None,
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    staged
        .apply_perspective_stage(
            None,
            None,
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    staged.apply_crop_stage(Some(&geometry), false).unwrap();
    assert_eq!(legacy.pixels, staged.pixels);
    assert_eq!((legacy.width, legacy.height), (8, 8));
    assert_eq!((staged.width, staged.height), (8, 8));
}

#[test]
fn geometry_with_auto_fill_matches_manual_stage_sequence() {
    fn frame() -> ImageFrame {
        ImageFrame::new(8, 8, vec![140u8; 8 * 8 * 4]).unwrap()
    }
    let lens = lumina_sidecar::LensCorrection {
        version: 1,
        profile: Some("wide-light".into()),
        distortion_k1: None,
        distortion_k2: None,
        distortion_k3: None,
        vignette_c0: None,
        vignette_c1: None,
        vignette_c2: None,
        ca_red: None,
        ca_blue: None,
    };
    let geometry = lumina_sidecar::Geometry {
        version: 1,
        crop: Some(lumina_sidecar::Crop::Free {
            x: 0.25,
            y: 0.25,
            width: 0.5,
            height: 0.5,
        }),
        rotation_degrees: 0.0,
        mirror_horizontal: false,
        mirror_vertical: true,
    };
    let mut legacy = frame();
    legacy
        .apply_geometry_with_auto_fill(
            Some(&geometry),
            Some(&lens),
            None,
            #[cfg(feature = "lensfun")]
            None,
            true,
            99,
        )
        .unwrap();
    let mut manual = frame();
    manual
        .apply_lens_stage(
            Some(&lens),
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    manual.apply_auto_fill_transparent(true, 99);
    manual
        .apply_perspective_stage(
            Some(&lens),
            None,
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    manual.apply_crop_stage(Some(&geometry), true).unwrap();
    assert_eq!(legacy.pixels, manual.pixels);
    assert_eq!((legacy.width, legacy.height), (4, 4));
}

#[test]
fn crop_stage_rejects_invalid_geometry() {
    let mut frame = ImageFrame::new(4, 4, vec![1u8; 4 * 4 * 4]).unwrap();
    assert!(frame.apply_crop_stage(None, false).is_ok());
    assert_eq!(frame.pixels, vec![1u8; 4 * 4 * 4]);
    for bad in [
        lumina_sidecar::Geometry {
            version: 2,
            crop: None,
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        },
        lumina_sidecar::Geometry {
            version: 1,
            crop: None,
            rotation_degrees: f32::NAN,
            mirror_horizontal: false,
            mirror_vertical: false,
        },
        lumina_sidecar::Geometry {
            version: 1,
            crop: None,
            rotation_degrees: 200.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        },
    ] {
        assert!(frame.apply_crop_stage(Some(&bad), false).is_err());
    }
}
