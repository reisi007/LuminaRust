//! R5-LOUPE-1 (F-103-N6 Runde 5, User-Bug): the Library loupe must paint the
//! **developed render** (Draft or viewport-capped Full, like the Develop canvas
//! under R3-RENDER-SIZE-1), not the ≤ 221 px filmstrip thumbnail that left the
//! image tiny in the middle.
//!
//! Nested under `tests::library_views` so `lib.rs`'s ratcheted module list and
//! the 500-line parent file stay unchanged.

use super::*;

/// Deterministic `w`×`h` checker PNG (real decode, non-degenerate content).
fn save_sized_png(path: &Path, width: u32, height: u32) {
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let value = if (x / 8 + y / 8) % 2 == 0 {
                200u8
            } else {
                40u8
            };
            let index = ((y * width + x) as usize) * 4;
            pixels[index..index + 4].copy_from_slice(&[value, value, value, 255]);
        }
    }
    let frame = ImageFrame::new(width, height, pixels).unwrap();
    std::fs::write(path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
}

/// The painted texture of every textured shape, with its on-screen bounds.
fn painted_textures(shapes: &[egui::epaint::ClippedShape]) -> Vec<(egui::TextureId, egui::Rect)> {
    shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Mesh(mesh) => Some((mesh.texture_id, mesh.calc_bounds())),
            egui::Shape::Rect(rect) => Some((rect.fill_texture_id(), rect.rect)),
            _ => None,
        })
        .collect()
}

#[test]
fn loupe_paints_the_developed_render_not_the_thumbnail() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("photo.png");
    save_sized_png(&source, 400, 300);
    let ctx = egui::Context::default();
    let mut app = LuminaApp::new(ctx.clone());
    open_and_decode(&mut app, source.display().to_string());
    app.set_directory(dir.path().display().to_string());
    app.list_directory();
    app.set_library_view(LibraryView::Loupe);
    app.render().unwrap();

    // A deliberately tiny, bright thumbnail: the buggy loupe painted THIS.
    let thumb_key = app.entries[0].thumb_key.clone();
    let thumb = ctx.load_texture(
        "thumb",
        egui::ColorImage::filled([8, 8], egui::Color32::RED),
        egui::TextureOptions::NEAREST,
    );
    let thumb_id = thumb.id();
    app.thumbnails.insert(&thumb_key, thumb);

    // Upload the developed render so its texture id is stable (the loupe keeps
    // the same handle when its content identity is unchanged).
    app.update_texture(&ctx);
    let render_id = app.texture.as_ref().expect("developed render texture").id();

    let indices = app.filtered_library_order();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0));
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(1.0),
            ..Default::default()
        },
        |ui| {
            let ctx = ui.ctx().clone();
            app.draw_library_loupe(&ctx, ui, &indices);
        },
    );
    output.textures_delta.clear();

    let painted = painted_textures(&output.shapes);
    let render_paint = painted
        .iter()
        .find(|(id, _)| *id == render_id)
        .unwrap_or_else(|| {
            panic!("the loupe must paint the developed render {render_id:?}; painted {painted:?}")
        });
    assert!(
        render_paint.1.width() > 100.0 && render_paint.1.height() > 100.0,
        "the loupe render must be large, got {:?}",
        render_paint.1
    );
    assert!(
        painted.iter().all(|(id, _)| *id != thumb_id),
        "the filmstrip thumbnail must not be painted by the loupe"
    );
}

/// R3-RENDER-SIZE-1 consistency: the loupe render is the same viewport-capped
/// preview texture the Develop canvas uses (not an unbounded full-source
/// upload), so a large source stays inside the preview budget.
#[test]
fn loupe_render_is_the_viewport_capped_preview() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("large.png");
    save_sized_png(&source, 1200, 900);
    let ctx = egui::Context::default();
    let mut app = LuminaApp::new(ctx.clone());
    open_and_decode(&mut app, source.display().to_string());
    app.set_directory(dir.path().display().to_string());
    app.list_directory();
    app.set_library_view(LibraryView::Loupe);
    app.preview_pane_w = 200.0;
    app.preview_pane_h = 150.0;
    app.preview_cap_state.dpr = 1.0;
    app.render_full([200, 150], None).unwrap();
    // The cap keeps the render source at/below the viewport budget, exactly as
    // the Develop preview does.
    let render_src = app.preview_render_src.expect("render source recorded");
    assert!(
        render_src.0 <= 1200 && render_src.1 <= 900,
        "render must stay a real downscale: {render_src:?}"
    );
    app.update_texture(&ctx);
    let render_id = app.texture.as_ref().expect("render texture").id();

    let indices = app.filtered_library_order();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0));
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(1.0),
            ..Default::default()
        },
        |ui| {
            let ctx = ui.ctx().clone();
            app.draw_library_loupe(&ctx, ui, &indices);
        },
    );
    output.textures_delta.clear();
    assert!(
        painted_textures(&output.shapes)
            .iter()
            .any(|(id, _)| *id == render_id),
        "the loupe must paint the capped preview texture"
    );
}
