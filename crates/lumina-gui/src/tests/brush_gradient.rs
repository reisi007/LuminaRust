//! brush/gradient/ellipse mask geometry tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

// ---- F-103-N4: interactive mask tools (Brush / Linear / Radial) ----

#[test]
fn brush_marks_roundtrip_through_sidecar() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let id = app.create_mask("Subject").unwrap();
    let marks = vec![
        BrushMark {
            x: 0.2,
            y: 0.3,
            radius: 0.05,
            sign: BrushMarkSign::Positive,
        },
        BrushMark {
            x: 0.5,
            y: 0.6,
            radius: 0.05,
            sign: BrushMarkSign::Positive,
        },
    ];
    app.commit_brush_stroke(marks).unwrap();

    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    let mask = document.virtual_copies[0]
        .mask_library
        .iter()
        .find(|m| m.id == id)
        .unwrap();
    assert_eq!(
        mask.prompt,
        Some(MaskPrompt::Brush {
            marks: vec![
                BrushMark {
                    x: 0.2,
                    y: 0.3,
                    radius: 0.05,
                    sign: BrushMarkSign::Positive,
                },
                BrushMark {
                    x: 0.5,
                    y: 0.6,
                    radius: 0.05,
                    sign: BrushMarkSign::Positive,
                },
            ],
            resolution: (2, 1),
            transformation: PromptTransform::default(),
        })
    );
    // The active layer references the same mask.
    assert_eq!(document.virtual_copies[0].mask_layers[0].mask.mask_id, id);

    // The prompt survives a reopen.
    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    let reloaded = reopened.document.as_ref().unwrap();
    let reloaded_mask = reloaded.virtual_copies[0]
        .mask_library
        .iter()
        .find(|m| m.id == id)
        .unwrap();
    assert!(reloaded_mask.prompt.is_some());
    assert_eq!(reloaded_mask.status, MaskStatus::Valid);
}

#[test]
fn empty_brush_stroke_is_visible_error_and_writes_no_sidecar() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.create_mask("Subject").unwrap();
    // An empty stroke is rejected by the commit path (returns Err, no write).
    let result = app.commit_brush_stroke(vec![]);
    assert!(result.is_err());

    // The interactive UI path surfaces it as a visible GuiError via
    // `finish_drawing` (which the preview drag-stop calls) and writes
    // nothing to disk.
    app.set_mask_tool(MaskTool::Brush);
    app.pending_brush_marks.clear();
    app.drag_start = Some(Point2 { x: 0.3, y: 0.3 });
    app.drag_current = Some(Point2 { x: 0.3, y: 0.3 });
    app.finish_drawing();
    assert_eq!(app.status(), Str::Error.t());
    assert!(app.error().is_some());

    // No sidecar was written (the empty stroke never persisted).
    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    assert!(!sidecar.is_file());
}

#[test]
fn gradient_coordinate_calculation_from_drag() {
    // Helper exposes the prompt-building math directly.
    let p =
        LuminaApp::gradient_prompt_from_drag(Point2 { x: 0.1, y: 0.5 }, Point2 { x: 0.9, y: 0.5 });
    // Horizontal drag -> 0 degrees; start/end are the documented matte values.
    match p {
        MaskPrompt::Gradient {
            angle_deg,
            start,
            end,
            ..
        } => {
            assert!((angle_deg - 0.0).abs() < 1e-3);
            assert_eq!(start, 1.0);
            assert_eq!(end, 0.0);
        }
        _ => panic!("expected gradient prompt"),
    }

    match LuminaApp::gradient_prompt_from_drag(Point2 { x: 0.5, y: 0.1 }, Point2 { x: 0.5, y: 0.9 })
    {
        MaskPrompt::Gradient { angle_deg, .. } => {
            assert!((angle_deg - 90.0).abs() < 1e-3, "vertical drag -> 90°")
        }
        _ => panic!("expected gradient prompt"),
    }

    match LuminaApp::gradient_prompt_from_drag(Point2 { x: 0.9, y: 0.5 }, Point2 { x: 0.1, y: 0.5 })
    {
        MaskPrompt::Gradient { angle_deg, .. } => {
            assert!((angle_deg - 180.0).abs() < 1e-3, "right-to-left -> 180°")
        }
        _ => panic!("expected gradient prompt"),
    }

    match LuminaApp::gradient_prompt_from_drag(Point2 { x: 0.9, y: 0.9 }, Point2 { x: 0.1, y: 0.1 })
    {
        MaskPrompt::Gradient { angle_deg, .. } => assert!(
            (angle_deg - 225.0).abs() < 1e-3,
            "up-left -> 225° (negative direction normalized to [0,360))"
        ),
        _ => panic!("expected gradient prompt"),
    }

    // Points outside 0..=1 are clamped before the angle is computed.
    match LuminaApp::gradient_prompt_from_drag(
        Point2 { x: 2.0, y: 0.5 },
        Point2 { x: -1.0, y: 0.5 },
    ) {
        // After clamping: (1.0,0.5)->(0.0,0.5) -> dx=-1.0 -> 180°.
        MaskPrompt::Gradient { angle_deg, .. } => {
            assert!((angle_deg - 180.0).abs() < 1e-3)
        }
        _ => panic!("expected gradient prompt"),
    }

    // A zero-length drag is rejected by the commit path.
    let mut app = new_app();
    assert!(app
        .commit_gradient(Point2 { x: 0.5, y: 0.5 }, Point2 { x: 0.5001, y: 0.5 })
        .is_err());
}

#[test]
fn ellipse_generated_from_center_and_radii() {
    match LuminaApp::ellipse_prompt_from_drag(Point2 { x: 0.2, y: 0.2 }, Point2 { x: 0.8, y: 0.6 })
    {
        MaskPrompt::Ellipse { center, radii, .. } => {
            assert!((center.x - 0.5).abs() < 1e-6);
            assert!((center.y - 0.4).abs() < 1e-6);
            assert!((radii.x - 0.3).abs() < 1e-6);
            assert!((radii.y - 0.2).abs() < 1e-6);
        }
        _ => panic!("expected ellipse prompt"),
    }

    // A gradient/radial prompt also persists through the sidecar.
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let id = app.create_mask("Sky").unwrap();
    app.commit_gradient(Point2 { x: 0.1, y: 0.5 }, Point2 { x: 0.9, y: 0.5 })
        .unwrap();
    app.create_mask("Sun").unwrap();
    app.commit_radial(Point2 { x: 0.2, y: 0.2 }, Point2 { x: 0.8, y: 0.6 })
        .unwrap();

    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    let sky = document.virtual_copies[0]
        .mask_library
        .iter()
        .find(|m| m.id == id)
        .unwrap();
    assert!(matches!(sky.prompt, Some(MaskPrompt::Gradient { .. })));

    let radial = document.virtual_copies[0]
        .mask_library
        .iter()
        .find(|m| m.name == "Sun")
        .unwrap();
    assert!(matches!(radial.prompt, Some(MaskPrompt::Ellipse { .. })));
}
