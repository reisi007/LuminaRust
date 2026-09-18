use super::*;

#[test]
fn all_aspect_crop_presets_have_expected_dimensions_and_centering() {
    let presets = [
        (lumina_sidecar::AspectPreset::Original, 200, 100, 0),
        (lumina_sidecar::AspectPreset::OneToOne, 100, 100, 50),
        (lumina_sidecar::AspectPreset::FourToFive, 80, 100, 60),
        (lumina_sidecar::AspectPreset::FiveToFour, 125, 100, 38),
        (lumina_sidecar::AspectPreset::ThreeToTwo, 150, 100, 25),
        (lumina_sidecar::AspectPreset::TwoToThree, 67, 100, 67),
        (lumina_sidecar::AspectPreset::FourToThree, 133, 100, 33),
        (lumina_sidecar::AspectPreset::ThreeToFour, 75, 100, 63),
        (lumina_sidecar::AspectPreset::SixteenToNine, 178, 100, 11),
        (lumina_sidecar::AspectPreset::NineToSixteen, 56, 100, 72),
    ];
    for (preset, expected_width, expected_height, expected_x) in presets {
        let frame = ImageFrame::new(200, 100, vec![0; 200 * 100 * 4]).unwrap();
        let domain = frame
            .measurement_domain(Some(&lumina_sidecar::Geometry {
                version: 1,
                crop: Some(lumina_sidecar::Crop::Aspect { preset }),
                rotation_degrees: 0.0,
                mirror_horizontal: false,
                mirror_vertical: false,
            }))
            .unwrap();
        assert_eq!(
            (domain.output_width, domain.output_height),
            (expected_width, expected_height)
        );
        assert_eq!((domain.source_x * 200.0).round() as u32, expected_x);
        assert_eq!(domain.source_y, 0.0);
    }
}

#[test]
fn free_crop_50_by_25_percent_is_exactly_centered() {
    let frame = ImageFrame::new(200, 100, vec![0; 200 * 100 * 4]).unwrap();
    let domain = frame
        .measurement_domain(Some(&lumina_sidecar::Geometry {
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
        }))
        .unwrap();
    assert_eq!((domain.output_width, domain.output_height), (100, 50));
    assert_eq!(
        (
            domain.source_x,
            domain.source_y,
            domain.source_width,
            domain.source_height
        ),
        (0.25, 0.25, 0.5, 0.5)
    );
}

#[test]
fn rotation_90_and_180_have_exact_non_square_dimensions_and_pattern() {
    let geometry = |degrees| lumina_sidecar::Geometry {
        version: 1,
        crop: None,
        rotation_degrees: degrees,
        mirror_horizontal: false,
        mirror_vertical: false,
    };
    let mut quarter =
        ImageFrame::new(2, 2, (1..=4).flat_map(|v| [v, 0, 0, 255]).collect()).unwrap();
    quarter
        .apply_geometry(
            Some(&geometry(90.0)),
            None,
            None,
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    assert_eq!((quarter.width, quarter.height), (2, 2));
    assert_eq!(
        quarter
            .pixels
            .iter()
            .step_by(4)
            .copied()
            .collect::<Vec<_>>(),
        vec![3, 1, 4, 2]
    );
    let mut half = ImageFrame::new(2, 2, (1..=4).flat_map(|v| [v, 0, 0, 255]).collect()).unwrap();
    half.apply_geometry(
        Some(&geometry(180.0)),
        None,
        None,
        #[cfg(feature = "lensfun")]
        None,
    )
    .unwrap();
    assert_eq!((half.width, half.height), (2, 2));
    assert_eq!(
        half.pixels.iter().step_by(4).copied().collect::<Vec<_>>(),
        vec![4, 3, 2, 1]
    );
}

#[test]
fn non_quarter_rotation_keeps_black_pixels_outside_non_square_source() {
    let mut frame = ImageFrame::new(3, 2, vec![255; 3 * 2 * 4]).unwrap();
    frame
        .apply_geometry(
            Some(&lumina_sidecar::Geometry {
                version: 1,
                crop: None,
                rotation_degrees: 45.0,
                mirror_horizontal: false,
                mirror_vertical: false,
            }),
            None,
            None,
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    assert_eq!((frame.width, frame.height), (4, 4));
    for index in [0, 3, 12, 15] {
        assert_eq!(&frame.pixels[index * 4..index * 4 + 4], &[0, 0, 0, 0]);
    }
}

#[test]
fn horizontal_and_vertical_mirrors_are_exact() {
    let geometry = |horizontal, vertical| lumina_sidecar::Geometry {
        version: 1,
        crop: None,
        rotation_degrees: 0.0,
        mirror_horizontal: horizontal,
        mirror_vertical: vertical,
    };
    let source = || ImageFrame::new(2, 2, (1..=4).flat_map(|v| [v, 0, 0, 255]).collect()).unwrap();
    let mut horizontal = source();
    horizontal
        .apply_geometry(
            Some(&geometry(true, false)),
            None,
            None,
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    assert_eq!(
        horizontal
            .pixels
            .iter()
            .step_by(4)
            .copied()
            .collect::<Vec<_>>(),
        vec![2, 1, 4, 3]
    );
    let mut vertical = source();
    vertical
        .apply_geometry(
            Some(&geometry(false, true)),
            None,
            None,
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    assert_eq!(
        vertical
            .pixels
            .iter()
            .step_by(4)
            .copied()
            .collect::<Vec<_>>(),
        vec![3, 4, 1, 2]
    );
}

#[test]
fn geometry_none_is_identity_and_core_validates_version_rotation_and_free_crop() {
    let frame = ImageFrame::new(4, 2, vec![7; 32]).unwrap();
    let mut unchanged = frame.clone();
    unchanged
        .apply_geometry(
            Some(&lumina_sidecar::Geometry {
                version: 1,
                crop: None,
                rotation_degrees: 0.0,
                mirror_horizontal: false,
                mirror_vertical: false,
            }),
            None,
            None,
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    assert_eq!(unchanged, frame);
    let identity = frame.measurement_domain(None).unwrap();
    assert_eq!(
        (
            identity.output_width,
            identity.output_height,
            identity.source_x,
            identity.source_y,
            identity.source_width,
            identity.source_height
        ),
        (4, 2, 0.0, 0.0, 1.0, 1.0)
    );
    for geometry in [
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
            rotation_degrees: 181.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        },
        lumina_sidecar::Geometry {
            version: 1,
            crop: Some(lumina_sidecar::Crop::Free {
                x: 0.8,
                y: 0.0,
                width: 0.3,
                height: 0.5,
            }),
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        },
    ] {
        let mut candidate = frame.clone();
        assert!(candidate
            .apply_recipe(&EditRecipe {
                geometry: Some(geometry),
                ..Default::default()
            })
            .is_err());
    }
}
