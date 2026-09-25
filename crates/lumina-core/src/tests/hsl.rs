use super::*;
use crate::color_stages::{hsl_to_rgb, rgb_to_hsl};

#[test]
fn hsl_violet_and_magenta_have_distinct_centres() {
    let mut frame = ImageFrame::new(1, 1, {
        let rgb = hsl_to_rgb(270.0, 1.0, 0.5);
        vec![
            (rgb[0] * 255.0).round() as u8,
            (rgb[1] * 255.0).round() as u8,
            (rgb[2] * 255.0).round() as u8,
            255,
        ]
    })
    .unwrap();
    frame
        .apply_recipe(&hsl_recipe(
            6,
            lumina_sidecar::HslChannel {
                hue: 1.0,
                ..Default::default()
            },
        ))
        .unwrap();
    let (hue, _, _) = rgb_to_hsl(
        frame.pixels[0] as f32 / 255.0,
        frame.pixels[1] as f32 / 255.0,
        frame.pixels[2] as f32 / 255.0,
    );
    assert!(
        (hue - 300.0).abs() < 1.0,
        "violet centre must be 270 degrees, got {hue}"
    );

    let mut frame = ImageFrame::new(1, 1, {
        let rgb = hsl_to_rgb(300.0, 1.0, 0.5);
        vec![
            (rgb[0] * 255.0).round() as u8,
            (rgb[1] * 255.0).round() as u8,
            (rgb[2] * 255.0).round() as u8,
            255,
        ]
    })
    .unwrap();
    frame
        .apply_recipe(&hsl_recipe(
            7,
            lumina_sidecar::HslChannel {
                hue: -1.0,
                ..Default::default()
            },
        ))
        .unwrap();
    let (hue, _, _) = rgb_to_hsl(
        frame.pixels[0] as f32 / 255.0,
        frame.pixels[1] as f32 / 255.0,
        frame.pixels[2] as f32 / 255.0,
    );
    assert!(
        (hue - 270.0).abs() < 1.0,
        "magenta centre must be 300 degrees, got {hue}"
    );
}

#[test]
fn hsl_neighbour_contributions_are_normalized() {
    let rgb = hsl_to_rgb(45.0, 0.6, 0.5);
    let mut frame = ImageFrame::new(
        1,
        1,
        vec![
            (rgb[0] * 255.0).round() as u8,
            (rgb[1] * 255.0).round() as u8,
            (rgb[2] * 255.0).round() as u8,
            255,
        ],
    )
    .unwrap();
    let mut recipe = hsl_recipe(
        1,
        lumina_sidecar::HslChannel {
            hue: 1.0,
            ..Default::default()
        },
    );
    recipe.hsl.as_mut().unwrap().yellow = Some(lumina_sidecar::HslChannel {
        hue: -1.0,
        ..Default::default()
    });
    frame.apply_recipe(&recipe).unwrap();
    assert_eq!(
        frame.pixels[0..3],
        [
            (rgb[0] * 255.0).round() as u8,
            (rgb[1] * 255.0).round() as u8,
            (rgb[2] * 255.0).round() as u8,
        ]
    );
}

#[test]
fn hsl_saturation_and_luminance_are_additive() {
    let rgb = hsl_to_rgb(0.0, 0.4, 0.5);
    let mut frame = ImageFrame::new(
        1,
        1,
        vec![
            (rgb[0] * 255.0).round() as u8,
            (rgb[1] * 255.0).round() as u8,
            (rgb[2] * 255.0).round() as u8,
            255,
        ],
    )
    .unwrap();
    frame
        .apply_recipe(&hsl_recipe(
            0,
            lumina_sidecar::HslChannel {
                saturation: 0.2,
                luminance: 0.1,
                ..Default::default()
            },
        ))
        .unwrap();
    let (_, saturation, luminance) = rgb_to_hsl(
        frame.pixels[0] as f32 / 255.0,
        frame.pixels[1] as f32 / 255.0,
        frame.pixels[2] as f32 / 255.0,
    );
    assert!((saturation - 0.6).abs() < 0.02);
    assert!((luminance - 0.6).abs() < 0.01);
}
