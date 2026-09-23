//! Full-resolution render seam for the test-only GUI recipe matrix.
//!
//! The interactive preview deliberately applies the R3 viewport cap. Matrix
//! verification must instead render the decoded source at its original
//! geometry before the documented comparison downscale, matching the CLI CPU
//! reference and committed goldens.

use crate::{GuiError, LuminaApp};

/// Render through [`LuminaApp::render`] without the interactive preview cap and
/// fail loudly if the pipeline consumed anything other than the full source.
///
/// This is test-only because the enclosing `matrix` module is `#[cfg(test)]`.
/// It changes only the synthetic headless viewport; rendering still goes
/// through the app's shared CPU `render_from` pipeline and `RenderContext`.
pub(super) fn full_resolution(app: &mut LuminaApp) -> Result<(), GuiError> {
    let source = app
        .original
        .as_ref()
        .ok_or_else(|| GuiError::Io("GUI matrix has no decoded source".into()))?;
    let expected = (source.width, source.height);

    // `preview_size_cap` multiplies this test viewport by DPR and the preview
    // margin, so a source-sized pane cannot trigger the R3 downscale. Keep DPR
    // deterministic and discard any stale capped source before rendering.
    app.preview_pane_w = source.width as f32;
    app.preview_pane_h = source.height as f32;
    app.preview_cap_state.dpr = 1.0;
    app.preview_cap_state.capped_src = None;
    app.render()?;

    let actual = app.preview_render_src;
    if actual != Some(expected) {
        return Err(GuiError::Io(format!(
            "GUI matrix render source {actual:?} is not full source geometry {expected:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::run_matrix;
    use super::full_resolution;
    use crate::LuminaApp;
    use eframe::egui;
    use lumina_core::{
        downscale_bilinear, render_frame, ImageFileFormat, ImageFrame, RenderContext,
    };
    use lumina_sidecar::EditRecipe;
    use std::fs;
    use std::path::PathBuf;

    const SAMPLE_WIDTH: u32 = 1200;
    const SAMPLE_HEIGHT: u32 = 900;

    /// Deterministic source larger than the default 800×600 viewport, so the
    /// R3 cap would visibly shrink it if the matrix seam regressed.
    fn sample_png() -> Vec<u8> {
        let mut pixels = Vec::with_capacity((SAMPLE_WIDTH * SAMPLE_HEIGHT * 4) as usize);
        for y in 0..SAMPLE_HEIGHT {
            for x in 0..SAMPLE_WIDTH {
                pixels.push((x * 4) as u8);
                pixels.push((y * 5) as u8);
                pixels.push(((x + y) * 2) as u8);
                pixels.push(255);
            }
        }
        ImageFrame::new(SAMPLE_WIDTH, SAMPLE_HEIGHT, pixels)
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap()
    }

    fn write_set(directory: &tempfile::TempDir) -> PathBuf {
        fs::write(directory.path().join("sample.png"), sample_png()).unwrap();
        let path = directory.path().join("recipe-set.json");
        fs::write(
            &path,
            r#"{
  "schema_version": 1,
  "pipeline_version": "raster-mvp-1",
  "comparison_width": 48,
  "samples": [{ "id": "sample", "path": "sample.png" }],
  "recipes": [
    {
      "id": "tone",
      "goals": ["G-01"],
      "stages": ["exposure", "contrast"],
      "tolerance": "standard",
      "expected_route": "gpu",
      "recipe": { "adjustments": { "exposure": 0.4, "contrast": 0.2 } }
    }
  ]
}"#,
        )
        .unwrap();
        path
    }

    /// The GUI matrix path renders the full decoded source and exactly matches
    /// `lumina_core::render_frame` with the matrix `RenderContext`. The
    /// comparison downscale and unchanged tolerance gate still run end to end.
    #[test]
    fn matrix_render_is_full_resolution_shared_core_and_keeps_tolerance_gate() {
        let directory = tempfile::tempdir().unwrap();
        let recipe_set = write_set(&directory);
        let golden_dir = directory.path().join("golden");
        fs::create_dir_all(&golden_dir).unwrap();

        let frame = ImageFrame::decode(&sample_png()).unwrap();
        let recipe: EditRecipe =
            serde_json::from_str(r#"{ "adjustments": { "exposure": 0.4, "contrast": 0.2 } }"#)
                .unwrap();
        let context = RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        };
        let core = render_frame(&frame, &context).unwrap().frame;

        let mut app = LuminaApp::new(egui::Context::default());
        app.load_bytes(sample_png(), "sample.png").unwrap();
        app.recipe = recipe;
        full_resolution(&mut app).unwrap();
        let full_preview = app.preview().unwrap();
        assert_eq!(
            (full_preview.width, full_preview.height),
            (SAMPLE_WIDTH, SAMPLE_HEIGHT),
            "GUI matrix must render before comparison downscaling"
        );
        assert_eq!(
            app.preview_render_src,
            Some((SAMPLE_WIDTH, SAMPLE_HEIGHT)),
            "GUI matrix must not feed the R3-capped source to the shared pipeline"
        );
        assert_eq!(
            full_preview.pixels, core.pixels,
            "GUI matrix must use the same full-resolution core render context"
        );

        let preview = downscale_bilinear(full_preview, 48).unwrap();
        fs::write(
            golden_dir.join("sample__tone.png"),
            preview.encode(ImageFileFormat::Png).unwrap(),
        )
        .unwrap();

        let report = run_matrix(&recipe_set, Some(&golden_dir), None, &[]).unwrap();
        assert!(
            report.failed().is_empty(),
            "green run must pass: {}",
            report.summary()
        );
        assert!(report.pairs[0].psnr_db.unwrap().is_infinite());
        assert_eq!(report.pairs[0].expected_route, "gpu");

        let mut perturbed = preview.clone();
        for (index, byte) in perturbed.pixels.iter_mut().enumerate() {
            if index % 4 != 3 {
                *byte = 255 - *byte;
            }
        }
        fs::write(
            golden_dir.join("sample__tone.png"),
            perturbed.encode(ImageFileFormat::Png).unwrap(),
        )
        .unwrap();
        let report = run_matrix(&recipe_set, Some(&golden_dir), None, &[]).unwrap();
        assert_eq!(report.failed().len(), 1, "{}", report.summary());
        let message = report.failed()[0].message.clone().unwrap_or_default();
        assert!(message.contains("requires PSNR"), "message: {message}");
    }
}
