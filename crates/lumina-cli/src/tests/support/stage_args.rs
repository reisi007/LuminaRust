use super::*;

// ---- G-05 Lens Blur CLI ----

pub(crate) fn lens_blur_base_args(input: PathBuf) -> LensBlurArgs {
    LensBlurArgs {
        input,
        virtual_copy: None,
        json: true,
        list: false,
        enable: false,
        disable: false,
        set_amount: None,
        set_focal_near: None,
        set_focal_far: None,
        set_bokeh: None,
        set_focus_rect: None,
        set_depth_artifact: None,
        clear_depth_artifact: false,
        clear: false,
    }
}

pub(crate) fn color_base_args(input: PathBuf) -> ColorArgs {
    ColorArgs {
        input,
        virtual_copy: None,
        json: true,
        list: false,
        set_curve_param: Vec::new(),
        set_curve_points: Vec::new(),
        clear_curves: false,
        clear_curve_channel: None,
        set_hsl: Vec::new(),
        clear_hsl: false,
        add_point_color: false,
        hue_center: None,
        hue_range: None,
        hue_shift: None,
        sat_shift: None,
        lum_shift: None,
        set_point_color: Vec::new(),
        remove_point_color: Vec::new(),
        clear_point_color: false,
        set_grading: Vec::new(),
        set_grading_balance: None,
        set_grading_blending: None,
        clear_grading: false,
        set_vibrance: None,
        set_saturation: None,
    }
}

// ---- G-06 Geometrie-Parität CLI ----

pub(crate) fn geometry_base_args(input: PathBuf) -> GeometryArgs {
    GeometryArgs {
        input,
        virtual_copy: None,
        json: true,
        list: false,
        set_crop_aspect: None,
        set_crop_free: None,
        clear_crop: false,
        set_rotation: None,
        straighten: None,
        set_mirror: None,
        clear_geometry: false,
        set_lens_profile: None,
        set_lens: Vec::new(),
        clear_lens: false,
        set_perspective: Vec::new(),
        clear_perspective: false,
        lensfun_status: false,
    }
}

pub(crate) fn upright_base_args(input: PathBuf) -> UprightArgs {
    UprightArgs {
        input,
        virtual_copy: None,
        json: true,
        list: false,
        analyze: false,
        enable: false,
        disable: false,
        clear: false,
    }
}

pub(crate) fn red_eye_base_args(input: PathBuf) -> RedEyeArgs {
    RedEyeArgs {
        input,
        virtual_copy: None,
        json: true,
        list: false,
        set: Vec::new(),
        remove: Vec::new(),
        clear: false,
        detect: false,
        detect_apply: false,
    }
}

/// A `size`×`size` grid (bright bars on dark) rotated by `angle_deg`, so the
/// `upright-lines-v1` detector has a real line signal to measure.
pub(crate) fn tilted_png_frame(size: u32, angle_deg: f32) -> ImageFrame {
    let (sa, ca) = angle_deg.to_radians().sin_cos();
    let period = 0.4f32;
    let mut pixels = vec![0u8; (size as usize) * (size as usize) * 4];
    for y in 0..size {
        for x in 0..size {
            let nx = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let ny = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let rx = ca * nx + sa * ny;
            let dy = (rx / period - (rx / period).round()).abs() * period;
            let value = if dy < 0.05 { 235u8 } else { 20u8 };
            let i = ((y * size + x) as usize) * 4;
            pixels[i] = value;
            pixels[i + 1] = value;
            pixels[i + 2] = value;
            pixels[i + 3] = 255;
        }
    }
    ImageFrame::new(size, size, pixels).unwrap()
}

/// Grey RGBA PNG with solid red rectangles at the given pixel bounds — the
/// deterministic fixture for LRPAR-G14-REDEYE-AUTO-15.
pub(crate) fn red_pupil_png(width: u32, height: u32, pupils: &[(u32, u32, u32, u32)]) -> Vec<u8> {
    let mut frame = ImageFrame::new(
        width,
        height,
        [120u8, 120, 120, 255]
            .iter()
            .copied()
            .cycle()
            .take((width * height * 4) as usize)
            .collect(),
    )
    .unwrap();
    for &(x0, y0, x1, y1) in pupils {
        for y in y0..y1 {
            for x in x0..x1 {
                let index = ((y * width + x) as usize) * 4;
                frame.pixels[index..index + 4].copy_from_slice(&[220, 30, 40, 255]);
            }
        }
    }
    frame.encode(ImageFileFormat::Png).unwrap()
}

//
// Documented boundary (F-042-N1): the CLI still passes an empty
// source-action list (`source_actions: &[]` in `process_selected`).
// Source actions reach the CLI only with F-042-N1 (persistence +
// CLI command); no CLI source-action test is written yet.

// ---- LRPAR-G04-REMOVE: `spot` command ----
pub(crate) fn spot_base_args(input: PathBuf) -> SpotArgs {
    SpotArgs {
        input,
        virtual_copy: None,
        json: true,
        list: false,
        add_heuristic: false,
        center_x: None,
        center_y: None,
        radius: None,
        feather: None,
        offset_dx: None,
        offset_dy: None,
        opacity: None,
        clear: false,
        spot_id: None,
        set_radius: None,
        set_feather: None,
        set_opacity: None,
        set_offset_dx: None,
        set_offset_dy: None,
        remove_spot: None,
        set_visualize_threshold: None,
        clear_visualize: false,
        set_distraction: None,
        detect_objects: false,
        detect_apply: false,
        detect_threshold: None,
        detect_max: None,
        regenerate_variant: None,
        variant: None,
        seed: None,
    }
}

pub(crate) fn import_sidecar_for(input: &Path) {
    let bytes = fs::read(input).unwrap();
    let frame = ImageFrame::decode(&bytes).unwrap();
    save_sidecar(
        &sidecar_path_for(input),
        &SidecarDocument::new(
            source_identity(input, &bytes, &frame, None).unwrap(),
            "raster-mvp-1",
        ),
    )
    .unwrap();
}

// ---- F-019: CLI `--migrate` delegates to the library migration path ----

/// Writes a legacy schema-version-0 sidecar next to `input` (the historical
/// pre-release stamp `migrate_json` bumps 0 → 1 → 2) and returns its path.
/// The bytes are written verbatim, bypassing `validate`, so the on-disk file
/// really is a v0 document that only an explicit migration may change.
pub(crate) fn write_legacy_sidecar(input: &Path) -> PathBuf {
    let path = sidecar_path_for(input);
    let bytes = fs::read(input).unwrap();
    let frame = ImageFrame::decode(&bytes).unwrap();
    let document = SidecarDocument::new(
        source_identity(input, &bytes, &frame, None).unwrap(),
        "raster-mvp-1",
    );
    let mut value: serde_json::Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
    value["schema_version"] = serde_json::Value::from(0);
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    path
}

// ---- GUI-GEN-GRANULAR-10 (F-100): explicit per-module regeneration ----

/// 4x4 gray-gradient PNG used by the regenerate tests (non-uniform so
/// Auto-Tone and Exposure Matching produce real, non-identity values).
pub(crate) fn regenerate_input(directory: &Path, name: &str) -> (PathBuf, ImageFrame) {
    let input = directory.join(name);
    let mut pixels = Vec::with_capacity(4 * 4 * 4);
    for y in 0..4u32 {
        for x in 0..4u32 {
            let value = ((x * 40 + y * 20) % 256) as u8;
            pixels.extend_from_slice(&[value, value, value, 255]);
        }
    }
    let frame = ImageFrame::new(4, 4, pixels).unwrap();
    fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    (input, frame)
}

pub(crate) fn regenerate_args(input: &Path, modules: Vec<RegenerateModule>) -> RegenerateArgs {
    RegenerateArgs {
        input: input.to_path_buf(),
        virtual_copy: None,
        modules: modules.into_iter().map(ModuleArg::from).collect(),
        target_luminance: 0.5,
        json: true,
    }
}

// ------------------------------------------------------------------
// GEN-ONNX-1 Welle 1: `lumina generative` end-to-end.
// ------------------------------------------------------------------

pub(crate) fn generative_args(input: &Path) -> GenerativeArgs {
    GenerativeArgs {
        input: input.to_path_buf(),
        virtual_copy: None,
        status: false,
        generate: false,
        force: false,
        remove: false,
        prompt: None,
        negative_prompt: None,
        seed: None,
        auto_fill: false,
        expand: false,
        canvas: None,
        keep: None,
        json: false,
    }
}

pub(crate) fn process_args(input: &Path, output: &Path) -> ProcessArgs {
    ProcessArgs {
        input: input.to_path_buf(),
        output: output.to_path_buf(),
        preset: None,
        exposure: None,
        contrast: None,
        whites: None,
        blacks: None,
        highlights: None,
        shadows: None,
        auto_tone: false,
        match_total_exposure: false,
        target_luminance: 0.5,
        write_metadata: false,
    }
}
