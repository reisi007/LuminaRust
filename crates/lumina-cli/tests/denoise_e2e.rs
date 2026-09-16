//! LRPAR-G14-DENOISE-IMPL-20 (CLI slice): end-to-end tests for `lumina denoise`
//! through the built binary.
//!
//! SOLL: `feature/decisions/LRPAR-G14-DENOISE-20.md` §6 (visible status, no
//! silent fallback, no automatic recomputation) and
//! `feature/architecture/pipeline.md` §F-096a. Fixtures are generated PNGs (no
//! user photos, no network). The bundle is written and read through the real
//! `save_denoise_rgb`/`load_zdata` codec (B4): the recipe checksum, the sidecar
//! record checksum and the core artifact checksum must be identical.

use lumina_core::{
    denoise_producer_provenance, DenoiseRgbArtifact as CoreDenoiseRgbArtifact, ImageFileFormat,
    ImageFrame,
};
use lumina_sidecar::{load_sidecar, load_zdata, sidecar_path_for, zdata_path_for, RecordSpec};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

const W: u32 = 8;
const H: u32 = 6;

/// Non-periodic RGBA gradient fixture (distinct from the flat artifact below).
fn gradient_png(dir: &tempfile::TempDir, name: &str) -> PathBuf {
    let mut rgba = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        for x in 0..W {
            let v = (x * 17 + y * 29) as u8;
            rgba.extend_from_slice(&[v, v.wrapping_add(40), v.wrapping_add(80), 255]);
        }
    }
    let path = dir.path().join(name);
    let frame = ImageFrame::new(W, H, rgba).unwrap();
    fs::write(&path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    path
}

/// Flat solid RGB artifact fixture of the same geometry (the "externally
/// produced denoise result").
fn solid_png(dir: &tempfile::TempDir, name: &str, rgb: [u8; 3]) -> PathBuf {
    let rgba = [rgb[0], rgb[1], rgb[2], 255].repeat((W * H) as usize);
    let path = dir.path().join(name);
    let frame = ImageFrame::new(W, H, rgba).unwrap();
    fs::write(&path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    path
}

fn hash(byte: u8) -> String {
    format!("sha256:{}", format!("{byte:02x}").repeat(32))
}

fn import(input: &Path) {
    let output = cli()
        .args(["import", "--input", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "import failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn record_output(
    input: &Path,
    rgb: &Path,
    model_hash: &str,
    extra: &[&str],
) -> std::process::Output {
    let mut args = vec![
        "denoise".to_string(),
        "--input".to_string(),
        input.to_str().unwrap().to_string(),
        "--record-rgb".to_string(),
        rgb.to_str().unwrap().to_string(),
        "--model-name".to_string(),
        "fixture-srgb".to_string(),
        "--model-version".to_string(),
        "1".to_string(),
        "--model-hash".to_string(),
        model_hash.to_string(),
        "--input-spec-digest".to_string(),
        hash(0x22),
        "--strength".to_string(),
        "1.0".to_string(),
        "--preserve-detail".to_string(),
        "0".to_string(),
        "--json".to_string(),
    ];
    args.extend(extra.iter().map(|value| (*value).to_string()));
    cli().args(&args).output().unwrap()
}

fn record(input: &Path, rgb: &Path, model_hash: &str, extra: &[&str]) {
    let output = record_output(input, rgb, model_hash, extra);
    assert!(
        output.status.success(),
        "record failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn read_rgb_artifact(input: &Path) -> (String, CoreDenoiseRgbArtifact) {
    let container = load_zdata(&zdata_path_for(input)).unwrap();
    let record = container
        .decode_all()
        .unwrap()
        .into_iter()
        .find_map(|spec| match spec {
            RecordSpec::DenoiseRgb(artifact) => Some(artifact),
            _ => None,
        })
        .expect("denoise_rgb record present");
    let core =
        CoreDenoiseRgbArtifact::new(record.width, record.height, record.pixels.clone()).unwrap();
    (record.checksum(), core)
}

/// B4: recipe reference, sidecar record and core artifact checksums agree; the
/// producer provenance round-trips and status resolves `ready`.
#[test]
fn record_status_and_render_roundtrip_and_checksum_agrees_across_crates() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir, "input.png");
    let rgb = solid_png(&dir, "denoised.png", [5, 9, 13]);
    import(&input);
    record(&input, &rgb, &hash(0x11), &[]);

    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let denoise = document.virtual_copies[0]
        .recipe
        .denoise_ai
        .as_ref()
        .expect("recipe carries denoise_ai");
    let reference_checksum = denoise.artifact.as_ref().unwrap().checksum.clone();
    let provenance = denoise_producer_provenance(denoise).expect("producer provenance persisted");
    assert_eq!(provenance.artifact_checksum, reference_checksum);
    assert_eq!(provenance.source_content_hash, document.source.content_hash);

    // B4: cross-crate checksum agreement over the real codec.
    let (record_checksum, core_artifact) = read_rgb_artifact(&input);
    assert_eq!(record_checksum, reference_checksum);
    assert_eq!(core_artifact.checksum(), reference_checksum);

    // Status is `ready` and read-only.
    let status = cli()
        .args([
            "denoise",
            "--input",
            input.to_str().unwrap(),
            "--status",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("\"status\":\"ready\""));

    // Render applies the artifact (strength 1.0 → the denoised RGB replaces the
    // source RGB; alpha is preserved).
    let out = dir.path().join("out.png");
    let rendered = cli()
        .args([
            "denoise",
            "--input",
            input.to_str().unwrap(),
            "--render",
            "--output",
            out.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        rendered.status.success(),
        "render failed: {}",
        String::from_utf8_lossy(&rendered.stderr)
    );
    let frame = ImageFrame::decode(&fs::read(&out).unwrap()).unwrap();
    assert_eq!((frame.width, frame.height), (W, H));
    for (pixel, rgb) in frame
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(core_artifact.pixels.as_chunks::<3>().0)
    {
        assert_eq!(&pixel[..3], rgb, "strength 1.0 must yield the artifact RGB");
        assert_eq!(pixel[3], 255, "alpha is untouched");
    }

    // Determinism: a second run is byte-identical.
    let out2 = dir.path().join("out2.png");
    let rendered2 = cli()
        .args([
            "denoise",
            "--input",
            input.to_str().unwrap(),
            "--render",
            "--output",
            out2.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(rendered2.status.success());
    assert_eq!(fs::read(&out).unwrap(), fs::read(&out2).unwrap());
}

/// §6/R2: `pending-integration` is visibly `unavailable`; the default `warn`
/// policy exits 0 with a stderr warning, `strict` aborts loudly.
#[test]
fn unavailable_stage_warns_and_exits_zero_but_strict_aborts() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir, "input.png");
    let rgb = solid_png(&dir, "denoised.png", [1, 2, 3]);
    import(&input);
    record(&input, &rgb, "pending-integration", &[]);

    let status = cli()
        .args([
            "denoise",
            "--input",
            input.to_str().unwrap(),
            "--status",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("\"status\":\"unavailable\""));

    let warn_out = dir.path().join("warn.png");
    let warned = cli()
        .args([
            "denoise",
            "--input",
            input.to_str().unwrap(),
            "--render",
            "--output",
            warn_out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(warned.status.success(), "warn policy must exit 0");
    assert!(
        String::from_utf8_lossy(&warned.stderr).contains("denoise_ai is unavailable"),
        "stderr must surface the visible fallback: {}",
        String::from_utf8_lossy(&warned.stderr)
    );
    assert!(warn_out.is_file());

    let strict_out = dir.path().join("strict.png");
    let strict = cli()
        .args([
            "denoise",
            "--input",
            input.to_str().unwrap(),
            "--render",
            "--output",
            strict_out.to_str().unwrap(),
            "--denoise-policy",
            "strict",
        ])
        .output()
        .unwrap();
    assert!(
        !strict.status.success(),
        "strict must abort on a non-ready stage"
    );
    assert!(!strict_out.exists(), "strict abort leaves no output");
}

/// §6: a missing artifact is a visible `missing`, never silently recreated; the
/// render falls back to the manual noise reduction (warn, exit 0).
#[test]
fn missing_artifact_is_visible_and_never_auto_recomputed() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir, "input.png");
    let rgb = solid_png(&dir, "denoised.png", [9, 9, 9]);
    import(&input);
    record(&input, &rgb, &hash(0x11), &[]);
    fs::remove_file(zdata_path_for(&input)).unwrap();

    let status = cli()
        .args([
            "denoise",
            "--input",
            input.to_str().unwrap(),
            "--status",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("\"status\":\"missing\""));
    assert!(
        !zdata_path_for(&input).exists(),
        "status must never recompute the artifact"
    );

    let out = dir.path().join("out.png");
    let rendered = cli()
        .args([
            "denoise",
            "--input",
            input.to_str().unwrap(),
            "--render",
            "--output",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(rendered.status.success());
    assert!(out.is_file());
}

/// §6: an unreadable bundle is a hard `corrupt` status (exit 1).
#[test]
fn corrupt_bundle_is_a_hard_corrupt_status() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir, "input.png");
    let rgb = solid_png(&dir, "denoised.png", [7, 7, 7]);
    import(&input);
    record(&input, &rgb, &hash(0x11), &[]);
    fs::write(zdata_path_for(&input), b"not-a-bundle").unwrap();

    let status = cli()
        .args([
            "denoise",
            "--input",
            input.to_str().unwrap(),
            "--status",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(!status.status.success(), "corrupt must exit non-zero");
    assert!(String::from_utf8_lossy(&status.stdout).contains("\"status\":\"corrupt\""));
}

/// `--record-rgb` refuses a geometry mismatch loudly (no half-state).
#[test]
fn record_rgb_rejects_geometry_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir, "input.png");
    import(&input);
    let wrong = ImageFrame::new(4, 4, [0, 0, 0, 255].repeat(16)).unwrap();
    let wrong_path = dir.path().join("wrong.png");
    fs::write(&wrong_path, wrong.encode(ImageFileFormat::Png).unwrap()).unwrap();

    let output = cli()
        .args([
            "denoise",
            "--input",
            input.to_str().unwrap(),
            "--record-rgb",
            wrong_path.to_str().unwrap(),
            "--model-name",
            "fixture-srgb",
            "--model-version",
            "1",
            "--model-hash",
            &hash(0x11),
            "--input-spec-digest",
            &hash(0x22),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("source geometry"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!zdata_path_for(&input).exists());
}

/// A missing sidecar is a loud error (nothing to inspect).
#[test]
fn status_without_sidecar_is_loud() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir, "input.png");
    let output = cli()
        .args(["denoise", "--input", input.to_str().unwrap(), "--status"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

/// `None`/identity recipe is `inactive` (no model, no error) — the identity
/// class of §6.
#[test]
fn inactive_stage_is_reported_without_requiring_an_artifact() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir, "input.png");
    import(&input);
    let status = cli()
        .args([
            "denoise",
            "--input",
            input.to_str().unwrap(),
            "--status",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("\"status\":\"inactive\""));
}

/// A changed source/decode context is `stale` (the persisted producer
/// provenance no longer matches the live identity) — never silently `ready`.
#[test]
fn changed_decode_context_is_stale() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir, "input.png");
    let rgb = solid_png(&dir, "denoised.png", [3, 3, 3]);
    import(&input);
    record(&input, &rgb, &hash(0x11), &[]);

    let path = sidecar_path_for(&input);
    let mut document = load_sidecar(&path).unwrap();
    document.source.decode_fingerprint.version = "other-decoder-version".into();
    lumina_sidecar::save_sidecar(&path, &document).unwrap();

    let status = cli()
        .args([
            "denoise",
            "--input",
            input.to_str().unwrap(),
            "--status",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("\"status\":\"stale\""));
}

/// Number of persisted `denoise_rgb` records in the bundle (F4).
fn denoise_record_count(input: &Path) -> usize {
    load_zdata(&zdata_path_for(input))
        .unwrap()
        .decode_all()
        .unwrap()
        .into_iter()
        .filter(|spec| matches!(spec, RecordSpec::DenoiseRgb(_)))
        .count()
}

/// F4: `--record-rgb` is the non-destructive append path (a duplicate content
/// id is rejected without touching the sidecar), while `--force` is the
/// explicit replace path — a re-record of the same content stays a single
/// record, and a new content replaces the recipe reference.
#[test]
fn record_rgb_duplicate_requires_force_and_force_replaces() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir, "input.png");
    let rgb = solid_png(&dir, "denoised.png", [5, 9, 13]);
    import(&input);
    record(&input, &rgb, &hash(0x11), &[]);
    assert_eq!(denoise_record_count(&input), 1);

    // Same content again without `--force`: the content-derived record id
    // already exists → loud failure, sidecar and bundle byte-identical.
    let sidecar = sidecar_path_for(&input);
    let sidecar_before = fs::read(&sidecar).unwrap();
    let zdata_before = fs::read(zdata_path_for(&input)).unwrap();
    let duplicate = record_output(&input, &rgb, &hash(0x11), &[]);
    assert_eq!(
        duplicate.status.code(),
        Some(1),
        "duplicate record without --force must be a loud failure"
    );
    assert!(
        String::from_utf8_lossy(&duplicate.stderr).contains("denoise_rgb"),
        "stderr must name the failing bundle: {}",
        String::from_utf8_lossy(&duplicate.stderr)
    );
    assert_eq!(fs::read(&sidecar).unwrap(), sidecar_before);
    assert_eq!(fs::read(zdata_path_for(&input)).unwrap(), zdata_before);
    assert_eq!(denoise_record_count(&input), 1);

    // `--force` replaces the existing record instead of appending a second one.
    record(&input, &rgb, &hash(0x11), &["--force"]);
    assert_eq!(denoise_record_count(&input), 1);
    let (checksum, _) = read_rgb_artifact(&input);
    let document = load_sidecar(&sidecar).unwrap();
    assert_eq!(
        document.virtual_copies[0]
            .recipe
            .denoise_ai
            .as_ref()
            .unwrap()
            .artifact
            .as_ref()
            .unwrap()
            .checksum,
        checksum
    );

    // Re-recording different content with `--force` re-points the recipe to
    // the new artifact (the previous record stays in the bundle under its own
    // content-derived id); status resolves `ready` against the new reference.
    let recipe_checksum_before = checksum.clone();
    let other = solid_png(&dir, "denoised-2.png", [200, 100, 50]);
    record(&input, &other, &hash(0x11), &["--force"]);
    let document = load_sidecar(&sidecar).unwrap();
    let recipe_checksum_after = document.virtual_copies[0]
        .recipe
        .denoise_ai
        .as_ref()
        .unwrap()
        .artifact
        .as_ref()
        .unwrap()
        .checksum
        .clone();
    assert_ne!(
        recipe_checksum_after, recipe_checksum_before,
        "force must re-point the recipe at the newly recorded artifact"
    );
    let status = cli()
        .args([
            "denoise",
            "--input",
            input.to_str().unwrap(),
            "--status",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("\"status\":\"ready\""));
}

/// F5: the three KI-Denoise actions are mutually exclusive; combining them is
/// a usage error (exit 2) instead of a silent precedence pick.
#[test]
fn multiple_actions_are_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir, "input.png");
    let rgb = solid_png(&dir, "denoised.png", [1, 2, 3]);
    import(&input);
    let out = dir.path().join("out.png");
    let input_arg = input.to_str().unwrap().to_string();
    let out_arg = out.to_str().unwrap().to_string();
    let rgb_arg = rgb.to_str().unwrap().to_string();

    let combos: Vec<Vec<String>> = vec![
        vec!["--status".into(), "--render".into()],
        vec!["--status".into(), "--record-rgb".into(), rgb_arg.clone()],
        vec!["--render".into(), "--record-rgb".into(), rgb_arg],
    ];
    for combo in combos {
        let mut args = vec![
            "denoise".to_string(),
            "--input".to_string(),
            input_arg.clone(),
        ];
        args.extend(combo);
        // Otherwise-valid invocation: the usage error must come from the
        // conflicting actions, not from a missing render output.
        args.push("--output".to_string());
        args.push(out_arg.clone());
        let output = cli().args(&args).output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "combination {args:?} must be a usage error: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("mutually exclusive"),
            "stderr must explain the conflict: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    // The rejected invocations must not have written anything.
    assert!(!out.exists());
    assert!(!zdata_path_for(&input).exists());
}

/// F1: `denoise --render` refuses a recipe that carries mask layers loudly
/// (never a silent render without masks) and writes no output. The bundle is a
/// real, loadable one: the mask library entry is created by the `mask` CLI.
#[test]
fn render_refuses_masked_recipe_loudly_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir, "input.png");
    import(&input);

    // Build a loadable library mask, then attach it as a layer.
    let added = cli()
        .args([
            "mask",
            "--input",
            input.to_str().unwrap(),
            "--add-luminance-range",
            "--name",
            "lum",
            "--range-min",
            "0.0",
            "--range-max",
            "0.5",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        added.status.success(),
        "mask add failed: {}",
        String::from_utf8_lossy(&added.stderr)
    );
    let listed = cli()
        .args([
            "mask",
            "--input",
            input.to_str().unwrap(),
            "--list",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(listed.status.success());
    let listed: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    let mask_id = listed["copies"][0]["masks"][0]["id"]
        .as_str()
        .expect("mask id")
        .to_string();
    let attached = cli()
        .args([
            "mask",
            "--input",
            input.to_str().unwrap(),
            "--attach-layer",
            &mask_id,
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        attached.status.success(),
        "mask attach failed: {}",
        String::from_utf8_lossy(&attached.stderr)
    );
    // The bundle is loadable and really carries the layer the render must see.
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(document.virtual_copies[0].mask_layers.len(), 1);

    let out = dir.path().join("out.png");
    let rendered = cli()
        .args([
            "denoise",
            "--input",
            input.to_str().unwrap(),
            "--render",
            "--output",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        rendered.status.code(),
        Some(1),
        "masked render must be refused (exit 1): {}",
        String::from_utf8_lossy(&rendered.stderr)
    );
    assert!(
        String::from_utf8_lossy(&rendered.stderr).contains("mask layers"),
        "stderr must name the unsupported mask layers: {}",
        String::from_utf8_lossy(&rendered.stderr)
    );
    assert!(!out.exists(), "a refused render writes no output");
}

/// M1: `denoise --render` honors `--format`/`--quality` exactly like the
/// neighboring `render` command — the requested format drives the output
/// extension and the encoder (never a silently mislabelled file), and the
/// quality reaches the encoder.
#[test]
fn render_honors_format_and_quality_like_render_command() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir, "input.png");
    let rgb = solid_png(&dir, "denoised.png", [40, 120, 200]);
    import(&input);
    record(&input, &rgb, &hash(0x11), &[]);
    let input_arg = input.to_str().unwrap().to_string();

    // `--format jpeg` on a `.png` output target produces a real JPEG at the
    // adjusted `.jpg` path; the original `.png` target is never written.
    let png_target = dir.path().join("formatted.png");
    let rendered = cli()
        .args([
            "denoise",
            "--input",
            &input_arg,
            "--render",
            "--output",
            png_target.to_str().unwrap(),
            "--format",
            "jpeg",
            "--quality",
            "85",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        rendered.status.success(),
        "render failed: {}",
        String::from_utf8_lossy(&rendered.stderr)
    );
    let jpg = dir.path().join("formatted.jpg");
    assert!(
        jpg.is_file(),
        "--format must drive the output extension (expected {})",
        jpg.display()
    );
    assert!(!png_target.exists(), "no mislabelled .png output");
    let bytes = fs::read(&jpg).unwrap();
    assert_eq!(
        &bytes[..3],
        &[0xFF, 0xD8, 0xFF],
        "payload must be a real JPEG"
    );

    // `--quality` is honored: two qualities produce different bytes.
    for (target, quality) in [("low.png", "10"), ("high.png", "95")] {
        let output = cli()
            .args([
                "denoise",
                "--input",
                &input_arg,
                "--render",
                "--output",
                dir.path().join(target).to_str().unwrap(),
                "--format",
                "jpeg",
                "--quality",
                quality,
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "render {quality} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_ne!(
        fs::read(dir.path().join("low.jpg")).unwrap(),
        fs::read(dir.path().join("high.jpg")).unwrap(),
        "--quality must reach the encoder"
    );
}

/// M1 negative case: an unsupported `--format` or an out-of-range `--quality`
/// is refused loudly (exit 1) before anything is written — the render never
/// falls back to a guessed format.
#[test]
fn render_rejects_invalid_format_and_quality_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir, "input.png");
    let rgb = solid_png(&dir, "denoised.png", [3, 6, 9]);
    import(&input);
    record(&input, &rgb, &hash(0x11), &[]);
    let input_arg = input.to_str().unwrap().to_string();
    let out = dir.path().join("out.png");

    let bad_format = cli()
        .args([
            "denoise",
            "--input",
            &input_arg,
            "--render",
            "--output",
            out.to_str().unwrap(),
            "--format",
            "tiff",
        ])
        .output()
        .unwrap();
    assert_eq!(bad_format.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&bad_format.stderr).contains("unsupported format"),
        "stderr must name the invalid format: {}",
        String::from_utf8_lossy(&bad_format.stderr)
    );
    assert!(!out.exists());

    let bad_quality = cli()
        .args([
            "denoise",
            "--input",
            &input_arg,
            "--render",
            "--output",
            out.to_str().unwrap(),
            "--format",
            "png",
            "--quality",
            "0",
        ])
        .output()
        .unwrap();
    assert_eq!(bad_quality.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&bad_quality.stderr).contains("quality must be in 1..=100"),
        "stderr must name the invalid quality: {}",
        String::from_utf8_lossy(&bad_quality.stderr)
    );
    assert!(!out.exists());
}
