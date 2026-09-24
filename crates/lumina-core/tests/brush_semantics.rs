//! Normative R5-BRUSH-24 CPU kernel regressions.

use lumina_core::mask_tiles::{blend_brush_value, brush_mark_alpha};
use lumina_core::masks::rasterize_prompt;
use lumina_sidecar::{BrushMark, BrushMarkSign, MaskPrompt, PromptTransform};

#[test]
fn softness_uses_exact_smoothstep_and_flow_scaling() {
    assert_eq!(brush_mark_alpha(0.0, 1.0, 1.0, 0.5), 32_768);
    assert_eq!(brush_mark_alpha(0.25, 1.0, 1.0, 0.5), 27_648);
    assert_eq!(brush_mark_alpha(0.5, 1.0, 1.0, 0.5), 16_384);
    assert_eq!(brush_mark_alpha(1.0, 1.0, 1.0, 0.5), 0);
    assert_eq!(brush_mark_alpha(0.5, 1.0, 1.0, 0.0), 0);
}

#[test]
fn positive_and_negative_dabs_select_max_and_min_exactly() {
    assert_eq!(
        blend_brush_value(20_000, BrushMarkSign::Positive, 30_000),
        30_000
    );
    assert_eq!(
        blend_brush_value(20_000, BrushMarkSign::Negative, 30_000),
        20_000
    );
    assert_eq!(
        blend_brush_value(u16::MAX, BrushMarkSign::Negative, 32_768),
        32_767
    );
}

#[test]
fn cpu_rasterizer_applies_softness_flow_and_negative_minimum() {
    let soft = MaskPrompt::Brush {
        marks: vec![BrushMark {
            x: 31.5 / 64.0,
            y: 31.5 / 64.0,
            radius: 0.25,
            sign: BrushMarkSign::Positive,
            softness: 1.0,
            flow: 0.5,
        }],
        resolution: (64, 64),
        transformation: PromptTransform::default(),
    };
    let soft_plane = rasterize_prompt(&soft, 64, 64).unwrap();
    let row = 31 * 64;
    assert_eq!(soft_plane.values[row + 31], 32_768);
    assert_eq!(soft_plane.values[row + 39], 16_384);
    assert_eq!(soft_plane.values[row + 47], 0);

    let erase = MaskPrompt::Brush {
        marks: vec![
            BrushMark {
                x: 31.5 / 64.0,
                y: 31.5 / 64.0,
                radius: 0.25,
                sign: BrushMarkSign::Positive,
                softness: 0.0,
                flow: 1.0,
            },
            BrushMark {
                x: 31.5 / 64.0,
                y: 31.5 / 64.0,
                radius: 0.1,
                sign: BrushMarkSign::Negative,
                softness: 0.0,
                flow: 0.5,
            },
        ],
        resolution: (64, 64),
        transformation: PromptTransform::default(),
    };
    let erase_plane = rasterize_prompt(&erase, 64, 64).unwrap();
    assert_eq!(
        erase_plane.values[row + 31],
        32_767,
        "negative min wins inside the eraser"
    );
    assert_eq!(
        erase_plane.values[row + 38],
        u16::MAX,
        "outside the eraser disc is a no-op"
    );
}

#[test]
fn incremental_negative_stamp_selects_minimum() {
    assert_eq!(brush_mark_alpha(0.5, 1.0, 0.0, 1.0), u16::MAX);
    let mut values = vec![u16::MAX; 2];
    let bbox = lumina_core::mask_tiles::stamp_brush_mark_with_options(
        &mut values,
        2,
        1,
        BrushMark {
            x: 0.5,
            y: 0.5,
            radius: 1.0,
            sign: BrushMarkSign::Negative,
            softness: 0.0,
            flow: 1.0,
        },
    );
    assert_eq!(bbox, (0, 0, 2, 1));
    assert_eq!(values, vec![0, 0]);
}

#[test]
fn zero_flow_and_outside_samples_do_not_modify_the_cpu_plane() {
    let prompt = MaskPrompt::Brush {
        marks: vec![
            BrushMark {
                x: 31.5 / 64.0,
                y: 31.5 / 64.0,
                radius: 0.1,
                sign: BrushMarkSign::Positive,
                softness: 0.0,
                flow: 0.0,
            },
            BrushMark {
                x: 0.0,
                y: 0.0,
                radius: 0.1,
                sign: BrushMarkSign::Negative,
                softness: 0.0,
                flow: 1.0,
            },
        ],
        resolution: (64, 64),
        transformation: PromptTransform::default(),
    };
    let plane = rasterize_prompt(&prompt, 64, 64).unwrap();
    assert!(plane.values.iter().all(|&value| value == 0));
}
