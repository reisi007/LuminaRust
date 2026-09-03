//! Standalone instrumenting GUI harness for the open LuminaRust GUI tasks.
//!
//! Scope: this crate lives in `agent-harness/` only and never touches
//! `crates/`, `feature/`, `Agents.todo.md` or the workspace `Cargo.toml`.
//! It drives `lumina-gui` in-process via `egui_kittest` (headless wgpu,
//! 1280x800, `pixels_per_point = 1`), following the
//! `crates/lumina-gui/tests/kittest_snapshots.rs` builder pattern.
//!
//! Design note on PASS/FAIL: the scenario checks encode the SHOULD state of
//! the open tasks, several of which are currently buggy. Failing `assert!`s
//! would turn the crate gates red, so each scenario records structured
//! **verdicts** (`PASS` / `FAIL` / `OPEN`) into
//! `artifacts/<scenario>/verdict.json` (plus `shot.png` + `tree.json`) and
//! the test itself only asserts harness-level invariants (artifacts exist,
//! app ran). The verdicts are the verification input for the fix decision.

use std::path::{Path, PathBuf};

use egui_kittest::kittest::{NodeT, Queryable};
use lumina_gui::{LuminaApp, Module};

/// Painter-home helpers (AGENT-HARNESS-2): pixel evidence for Painter-composited
/// content that AccessKit cannot see (badge chips, navigator rect stroke,
/// in-app preview frames). See `painter.rs` for the analysis.
pub mod painter;

/// Window size for all scenarios (matches AGENT-HARNESS-1).
pub const WINDOW_W: f32 = 1280.0;
pub const WINDOW_H: f32 = 800.0;

/// Headless probe around the real Lumina app.
pub struct GuiProbe {
    harness: egui_kittest::Harness<'static, LuminaApp>,
}

impl Default for GuiProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl GuiProbe {
    /// Fresh headless app at 1280x800, `pixels_per_point = 1`, wgpu backend.
    pub fn new() -> Self {
        let harness = egui_kittest::Harness::builder()
            .with_size([WINDOW_W, WINDOW_H])
            .with_pixels_per_point(1.0)
            .wgpu()
            .build_eframe(|cc| LuminaApp::new(cc.egui_ctx.clone()));
        Self { harness }
    }

    /// Fresh probe with the bundled sample image loaded in Develop.
    pub fn develop_with_sample() -> Self {
        let mut probe = Self::new();
        probe.app_mut().set_module(Module::Develop);
        probe
            .app_mut()
            .load_bytes(LuminaApp::sample_image_png(), "sample.png")
            .expect("sample image loads");
        probe.run_steps(2);
        probe
    }

    pub fn app(&self) -> &LuminaApp {
        self.harness.state()
    }

    pub fn app_mut(&mut self) -> &mut LuminaApp {
        self.harness.state_mut()
    }

    pub fn harness_mut(&mut self) -> &mut egui_kittest::Harness<'static, LuminaApp> {
        &mut self.harness
    }

    pub fn run_steps(&mut self, steps: usize) {
        self.harness.run_steps(steps);
    }

    /// Run frames until `preview_generation` is stable for `stable` consecutive
    /// frames or `max_steps` is exhausted. Returns `(frames_run, generation)`.
    pub fn wait_quiescent(&mut self, max_steps: usize, stable: usize) -> (usize, u64) {
        let mut last = self.app().preview_generation();
        let mut steady = 0usize;
        let mut run = 0usize;
        for _ in 0..max_steps {
            self.harness.run_steps(1);
            run += 1;
            let gen = self.app().preview_generation();
            if gen == last {
                steady += 1;
                if steady >= stable {
                    break;
                }
            } else {
                steady = 0;
                last = gen;
            }
        }
        (run, last)
    }

    /// Render the current frame and save it as PNG. Returns `(w, h)`.
    pub fn render_png(&mut self, path: &Path) -> Result<(u32, u32), String> {
        let img = self.harness.render()?;
        let (w, h) = (img.width(), img.height());
        img.save(path).map_err(|e| e.to_string())?;
        Ok((w, h))
    }

    /// Serialize the AccessKit tree (via the kittest query API) as JSON:
    /// a flat array of `{role, label, value, rect:{x0,y0,x1,y1}}`.
    /// Static text lives in `value` (Label/TextRun nodes carry no `label`).
    pub fn ui_tree_json(&self) -> serde_json::Value {
        serde_json::Value::Array(
            self.tree_nodes()
                .iter()
                .map(|n| {
                    serde_json::json!({
                        "role": n.role,
                        "label": n.label,
                        "value": n.value,
                        "rect": {"x0": n.x0, "y0": n.y0, "x1": n.x1, "y1": n.y1},
                    })
                })
                .collect(),
        )
    }

    /// Flat owned snapshot of the tree for geometric analysis.
    pub fn tree_nodes(&self) -> Vec<TreeNode> {
        self.harness
            .query_all_by(|_| true)
            .filter(|n| n.accesskit_node().bounding_box().is_some())
            .map(|n| {
                let an = n.accesskit_node();
                let r = n.rect();
                TreeNode {
                    role: format!("{:?}", an.role()),
                    label: an.label().map(|s| s.to_string()).unwrap_or_default(),
                    value: n.value().unwrap_or_default(),
                    x0: r.min.x,
                    y0: r.min.y,
                    x1: r.max.x,
                    y1: r.max.y,
                }
            })
            .collect()
    }

    /// Click the first node whose label contains `needle`, then run a frame.
    /// Returns whether a node was found.
    pub fn click_label_contains(&mut self, needle: &str) -> bool {
        let clicked = self
            .harness
            .query_all_by_label_contains(needle)
            .next()
            .is_some();
        if clicked {
            if let Some(node) = self.harness.query_all_by_label_contains(needle).next() {
                node.click();
            }
            self.harness.run_steps(1);
        }
        clicked
    }
}

/// Owned, serializable AccessKit node record.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TreeNode {
    pub role: String,
    pub label: String,
    pub value: String,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl TreeNode {
    pub fn width(&self) -> f32 {
        (self.x1 - self.x0).max(0.0)
    }
    pub fn height(&self) -> f32 {
        (self.y1 - self.y0).max(0.0)
    }
    pub fn area(&self) -> f32 {
        self.width() * self.height()
    }
    pub fn label_contains(&self, needle: &str) -> bool {
        self.label.contains(needle)
    }
    /// Static text lives in `value`, widget names in `label` — match both.
    pub fn text_contains(&self, needle: &str) -> bool {
        self.label.contains(needle) || self.value.contains(needle)
    }
}

/// Intersection area of two axis-aligned rects (0 when disjoint).
pub fn intersection_area(a: &TreeNode, b: &TreeNode) -> f32 {
    let x0 = a.x0.max(b.x0);
    let y0 = a.y0.max(b.y0);
    let x1 = a.x1.min(b.x1);
    let y1 = a.y1.min(b.y1);
    ((x1 - x0).max(0.0)) * ((y1 - y0).max(0.0))
}

/// Mean luminance, std-dev and near-gray fraction of an sRGB frame.
pub fn frame_stats(img: &image::RgbaImage) -> FrameStats {
    let n = (img.width() * img.height()).max(1) as f64;
    let mut sum = 0.0;
    let mut sum2 = 0.0;
    let mut gray = 0u64;
    for px in img.pixels() {
        let [r, g, b, _] = px.0;
        let luma = (0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b)) / 255.0;
        sum += luma;
        sum2 += luma * luma;
        let spread = r.abs_diff(g).max(g.abs_diff(b)).max(r.abs_diff(b));
        if spread <= 6 {
            gray += 1;
        }
    }
    let mean = sum / n;
    let var = (sum2 / n - mean * mean).max(0.0);
    FrameStats {
        width: img.width(),
        height: img.height(),
        mean_luma: mean,
        std_luma: var.sqrt(),
        gray_fraction: gray as f64 / n,
    }
}

/// Same stats restricted to the central `frac` (0..1) of the frame.
pub fn center_stats(img: &image::RgbaImage, frac: f32) -> FrameStats {
    let (w, h) = (img.width(), img.height());
    let mx = (w as f32 * (1.0 - frac) / 2.0) as u32;
    let my = (h as f32 * (1.0 - frac) / 2.0) as u32;
    let cw = (w as f32 * frac) as u32;
    let ch = (h as f32 * frac) as u32;
    let view = image::imageops::crop_imm(img, mx, my, cw.max(1), ch.max(1)).to_image();
    frame_stats(&view)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FrameStats {
    pub width: u32,
    pub height: u32,
    pub mean_luma: f64,
    pub std_luma: f64,
    pub gray_fraction: f64,
}

/// Verdict recorded per scenario check.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Verdict {
    pub check: String,
    pub result: &'static str,
    pub detail: String,
}

impl Verdict {
    pub fn pass(check: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            check: check.into(),
            result: "PASS",
            detail: detail.into(),
        }
    }
    pub fn fail(check: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            check: check.into(),
            result: "FAIL",
            detail: detail.into(),
        }
    }
    pub fn open(check: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            check: check.into(),
            result: "OPEN",
            detail: detail.into(),
        }
    }
}

/// Create (or reuse) `artifacts/<scenario>/` next to the crate root and return it.
pub fn artifact_dir(scenario: &str) -> PathBuf {
    let dir = PathBuf::from("artifacts").join(scenario);
    std::fs::create_dir_all(&dir).expect("artifact dir");
    dir
}

/// Write `verdicts` + `tree` into the scenario dir; prints a one-line summary.
pub fn save_report(dir: &Path, verdicts: &[Verdict], tree: &serde_json::Value) {
    std::fs::write(
        dir.join("verdict.json"),
        serde_json::to_string_pretty(verdicts).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.join("tree.json"),
        serde_json::to_string_pretty(tree).unwrap(),
    )
    .unwrap();
    for v in verdicts {
        println!("[{}] {} — {}", v.result, v.check, v.detail);
    }
    println!("artifacts: {}", dir.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_helpers_behave() {
        let a = TreeNode {
            role: String::new(),
            label: String::new(),
            value: String::new(),
            x0: 0.0,
            y0: 0.0,
            x1: 100.0,
            y1: 50.0,
        };
        let b = TreeNode {
            role: String::new(),
            label: String::new(),
            value: String::new(),
            x0: 50.0,
            y0: 25.0,
            x1: 150.0,
            y1: 75.0,
        };
        assert!((intersection_area(&a, &b) - 50.0 * 25.0).abs() < 1e-3);
        let c = TreeNode {
            x0: 200.0,
            y0: 200.0,
            x1: 300.0,
            y1: 300.0,
            ..a.clone()
        };
        assert_eq!(intersection_area(&a, &c), 0.0);
    }

    #[test]
    fn frame_stats_detect_flat_gray() {
        let flat = image::RgbaImage::from_pixel(64, 64, image::Rgba([128, 128, 128, 255]));
        let s = frame_stats(&flat);
        assert!((s.mean_luma - 128.0 / 255.0).abs() < 1e-6);
        assert!(s.std_luma < 1e-9);
        assert!((s.gray_fraction - 1.0).abs() < 1e-9);
    }

    #[test]
    fn frame_stats_detect_color_content() {
        let mut img = image::RgbaImage::new(64, 64);
        for (x, _, px) in img.enumerate_pixels_mut() {
            *px = if x < 32 {
                image::Rgba([240, 235, 220, 255])
            } else {
                image::Rgba([20, 25, 90, 255])
            };
        }
        let s = frame_stats(&img);
        assert!(s.std_luma > 0.1, "content must vary: {s:?}");
        assert!(s.gray_fraction < 0.1, "content is not gray: {s:?}");
    }
}
