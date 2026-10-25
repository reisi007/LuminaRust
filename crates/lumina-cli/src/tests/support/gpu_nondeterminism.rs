use super::*;
use lumina_gpu::RenderExecutionReport;
use std::path::{Path, PathBuf};

struct RenderCall {
    png: Vec<u8>,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    sidecar_text: String,
    sidecar_hash: String,
    slider_literals: [String; 6],
    route_is_gpu: bool,
    gpu_report: Option<RenderExecutionReport>,
}

struct PixelComparison {
    equal: bool,
    differing_pixels: usize,
    first_different_pixel: Option<usize>,
}

struct PairComparison {
    label: &'static str,
    png_equal: bool,
    pixels: PixelComparison,
}

/// Runs the 40-member Auto-Tone fixture family through the production CLI
/// process path and retains mismatch evidence before each tempdir is dropped.
pub(crate) fn run_adapter_family_diagnostic(member_count: u32) {
    let artifact_root = std::env::var_os("LUMINA_GPU_NONDETERMINISM_ARTIFACT_DIR")
        .map(PathBuf::from)
        .expect("set LUMINA_GPU_NONDETERMINISM_ARTIFACT_DIR for the adapter diagnostic");
    fs::create_dir_all(&artifact_root).expect("create diagnostic artifact directory");

    let mut all_calls = Vec::with_capacity(member_count as usize * 3);
    let mut failures = Vec::new();
    for member in 0..member_count {
        let directory = tempfile::tempdir().unwrap();
        let input = auto_tone_family_input(directory.path(), member);
        let sidecar_path = sidecar_path_for(&input);
        let mut calls = Vec::with_capacity(3);
        for run in 0..3 {
            let output = directory.path().join(format!("run-{run}.png"));
            let rendered = process_auto_tone_with_report(&input, &output, |_| {});
            let png = fs::read(&output).unwrap();
            let (decoded, _) = decode_input(&output, &png).unwrap();
            let sidecar_text = fs::read_to_string(&sidecar_path).unwrap();
            let sidecar_hash = blake3::hash(sidecar_text.as_bytes()).to_hex().to_string();
            let slider_literals = slider_literals(&sidecar_text);
            let report = rendered.gpu_report;
            let route_is_gpu = rendered.route.is_gpu();
            let completed = matches!(&report, Some(RenderExecutionReport::GpuPipelineCompleted));
            if !route_is_gpu || !completed {
                failures.push(format!(
                    "member {member:02} run {run}: cli_gpu_route={route_is_gpu}, gpu_report={report:?}"
                ));
            }
            calls.push(RenderCall {
                png,
                width: decoded.width(),
                height: decoded.height(),
                pixels: decoded.pixels().to_vec(),
                sidecar_text,
                sidecar_hash,
                slider_literals,
                route_is_gpu,
                gpu_report: report,
            });
        }

        let comparisons = [
            compare(&calls[0], &calls[1], "1-vs-2"),
            compare(&calls[1], &calls[2], "2-vs-3"),
        ];
        let slider_literals_equal = calls[0].slider_literals == calls[1].slider_literals
            && calls[1].slider_literals == calls[2].slider_literals;
        let mismatch = comparisons
            .iter()
            .any(|pair| !pair.png_equal || !pair.pixels.equal)
            || !slider_literals_equal
            || calls.iter().any(|call| {
                !call.route_is_gpu
                    || !matches!(
                        &call.gpu_report,
                        Some(RenderExecutionReport::GpuPipelineCompleted)
                    )
            });
        if mismatch {
            write_member_artifacts(
                &artifact_root,
                member,
                &calls,
                &comparisons,
                slider_literals_equal,
            );
            for pair in &comparisons {
                if !pair.png_equal || !pair.pixels.equal {
                    failures.push(format!(
                        "member {member:02} {}: png_equal={}, decoded_pixels_equal={}, differing_pixels={}, first_different_pixel={:?}",
                        pair.label,
                        pair.png_equal,
                        pair.pixels.equal,
                        pair.pixels.differing_pixels,
                        pair.pixels.first_different_pixel
                    ));
                }
            }
            if !slider_literals_equal {
                failures.push(format!(
                    "member {member:02}: persisted Auto-Tone slider literals changed"
                ));
            }
        }
        all_calls.extend(calls);
    }

    let completed_calls = all_calls
        .iter()
        .filter(|call| {
            matches!(
                &call.gpu_report,
                Some(RenderExecutionReport::GpuPipelineCompleted)
            )
        })
        .count();
    assert_eq!(
        all_calls.len(),
        member_count as usize * 3,
        "every fixture member must render exactly three times"
    );
    assert_eq!(
        completed_calls,
        member_count as usize * 3,
        "all process_selected renders must carry a real GpuPipelineCompleted report; failures: {failures:#?}"
    );
    assert!(
        failures.is_empty(),
        "Auto-Tone adapter diagnostic failed; artifacts: {}\n{failures:#?}",
        artifact_root.display()
    );
}

fn compare(first: &RenderCall, second: &RenderCall, label: &'static str) -> PairComparison {
    PairComparison {
        label,
        png_equal: first.png == second.png,
        pixels: compare_pixels(first, second),
    }
}

fn compare_pixels(first: &RenderCall, second: &RenderCall) -> PixelComparison {
    if first.width != second.width || first.height != second.height {
        return PixelComparison {
            equal: false,
            differing_pixels: first.pixels.len().max(second.pixels.len()) / 4,
            first_different_pixel: Some(0),
        };
    }
    let mut differing_pixels = 0;
    let mut first_different_pixel = None;
    let first_pixels = first.pixels.as_chunks::<4>().0;
    let second_pixels = second.pixels.as_chunks::<4>().0;
    for (index, (left, right)) in first_pixels.iter().zip(second_pixels).enumerate() {
        if left != right {
            differing_pixels += 1;
            first_different_pixel.get_or_insert(index);
        }
    }
    PixelComparison {
        equal: differing_pixels == 0 && first.pixels.len() == second.pixels.len(),
        differing_pixels,
        first_different_pixel,
    }
}

fn write_member_artifacts(
    root: &Path,
    member: u32,
    calls: &[RenderCall],
    comparisons: &[PairComparison; 2],
    slider_literals_equal: bool,
) {
    let directory = root.join(format!("family-{member:02}"));
    fs::create_dir_all(&directory).expect("create family diagnostic directory");
    for (run, call) in calls.iter().enumerate() {
        fs::write(directory.join(format!("run-{run}.png")), &call.png)
            .expect("write raw PNG diagnostic");
        fs::write(directory.join(format!("run-{run}.rgba8")), &call.pixels)
            .expect("write decoded RGBA8 diagnostic");
        fs::write(
            directory.join(format!("run-{run}.lumina.json")),
            &call.sidecar_text,
        )
        .expect("write sidecar-text diagnostic");
        fs::write(
            directory.join(format!("run-{run}.sidecar.blake3")),
            format!("{}\n", call.sidecar_hash),
        )
        .expect("write sidecar-hash diagnostic");
        fs::write(
            directory.join(format!("run-{run}.slider-literals.txt")),
            call.slider_literals.join("\n") + "\n",
        )
        .expect("write Auto-Tone slider-literal diagnostic");
        fs::write(
            directory.join(format!("run-{run}.report.txt")),
            format!(
                "cli_route_gpu={}\ngpu_report={:?}\nwidth={}\nheight={}\n",
                call.route_is_gpu, call.gpu_report, call.width, call.height
            ),
        )
        .expect("write per-call render report");
    }
    let mut comparison_text = format!("slider_literals_equal={slider_literals_equal}\n");
    for pair in comparisons {
        use std::fmt::Write as _;
        writeln!(
            comparison_text,
            "{} png_equal={} decoded_pixels_equal={} differing_pixels={} first_different_pixel={:?}",
            pair.label,
            pair.png_equal,
            pair.pixels.equal,
            pair.pixels.differing_pixels,
            pair.pixels.first_different_pixel
        )
        .unwrap();
    }
    fs::write(directory.join("comparisons.txt"), comparison_text)
        .expect("write pairwise comparison diagnostic");
}
