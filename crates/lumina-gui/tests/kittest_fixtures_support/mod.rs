//! GOLDEN-FIXT-31: the shared **real-RAW fixture** contract for the headless
//! kittest suites (`kittest_snapshots`, `kittest_library_stack`).
//!
//! Normative SOLL: [`feature/quality/golden-fixtures.md`](../../../../feature/quality/golden-fixtures.md).
//!
//! Before this module the Library fixtures were 11 committed `.arw` files of 18
//! literal bytes (`lumina-raw-fixture`). LibRaw rejected them, so the committed
//! Library goldens captured a *decode-failure banner* plus colour-block
//! placeholders — a picture that can never reveal an image-pipeline regression.
//!
//! The contract implemented here:
//!
//! * **R1 — real RAW fixture.** The two licensed Canon EOS R1 CR3s in
//!   `sample-data/raw/` (provenance/licence: `sample-data/raw/README.md` §4/§8)
//!   are the only RAW bytes a fixture may contain. They are committed
//!   **once**; a test stages a byte-identical copy into its own (relative,
//!   gitignored) fixture directory at setup time. Nothing is duplicated into
//!   the GUI test tree and no fixture needs network access.
//! * The staged copy is the *whole* contract: the Library/filmstrip cells get
//!   their pixels from the production thumbnail worker, which decodes the real
//!   CR3 and renders the real pipeline output. Nothing is pre-seeded into the
//!   preview cache any more, so a cache hit can no longer hide a broken decode.
//! * `sample_image_png()` (4x3 synthetic PNG) stays for **smoke/layout** tests
//!   only and is never a render invariant (classification in the SOLL).

use egui_kittest::{kittest::Queryable, Harness};
use lumina_core::cache::{disk::DiskFolderCache, PreviewKind};
use lumina_core::ImageFrame;
use lumina_gui::LuminaApp;
use lumina_sidecar::{DecodeFingerprint, GeometryFingerprint, SourceIdentity};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// Committed, licensed RAW fixture directory (package-relative).
///
/// Resolved from `CARGO_MANIFEST_DIR`, never from the process CWD, so a fixture
/// is found from every working directory Cargo may use.
pub(crate) const RAW_FIXTURE_DIR: &str = "../../sample-data/raw";

/// The two licensed RAW fixtures: `(file name, visible width, visible height,
/// EXIF orientation)`. Both are Canon EOS R1 captures at 6032x4024 sensor
/// output; the portrait frame carries EXIF orientation 8, so LibRaw's
/// orientation promotion makes the *visible* frame 4024x6032.
pub(crate) const RAW_FIXTURES: &[(&str, u32, u32, u8)] = &[
    ("aircraft-landscape.cr3", 6032, 4024, 1),
    ("aircraft-portrait.cr3", 4024, 6032, 8),
];

/// Longest-edge cap of the production thumbnail downscale
/// (`lumina_gui::filmstrip::THUMBNAIL_MAX_DIM`). Mirrored on purpose: the
/// golden guard must not reuse the constant it verifies.
const THUMBNAIL_MAX_DIM: u32 = 200;

/// Lower bound on distinct RGB values in a worker-produced thumbnail.
///
/// The removed synthetic fixture painted a vertical ramp around one base colour,
/// which can only reach ~192 distinct triples (64 ramp steps x 3 channels). A
/// nearest-neighbour downscale of a real photograph reaches thousands, so 400
/// separates "a real decode happened" from "synthetic placeholder" by two
/// orders of magnitude and cannot flip on a renderer detail.
const MIN_THUMBNAIL_COLORS: usize = 400;

/// One staged fixture entry: the path that was staged plus the RAW fixture it
/// was staged from (the source is needed for its geometry).
pub(crate) type StagedEntry = (PathBuf, &'static str);

/// Absolute path of a committed RAW fixture.
pub(crate) fn raw_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(RAW_FIXTURE_DIR)
        .join(name)
}

/// Visible geometry of a committed RAW fixture, by file name.
pub(crate) fn raw_geometry(name: &str) -> (u32, u32, u8) {
    RAW_FIXTURES
        .iter()
        .find(|(fixture, ..)| *fixture == name)
        .map(|(_, width, height, orientation)| (*width, *height, *orientation))
        .unwrap_or_else(|| panic!("{name} is not a declared RAW fixture (GOLDEN-FIXT-31)"))
}

/// `blake3:`-prefixed content hash of a committed RAW fixture, memoized per
/// process: a 12 MB file is hashed once, not once per staged copy.
fn source_hash(name: &str) -> String {
    static CACHE: OnceLock<Mutex<BTreeMap<String, String>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Some(hash) = cache.lock().expect("RAW hash cache").get(name) {
        return hash.clone();
    }
    let bytes = std::fs::read(raw_fixture(name))
        .unwrap_or_else(|error| panic!("committed RAW fixture {name} is unreadable: {error}"));
    let hash = format!("blake3:{}", blake3::hash(&bytes).to_hex());
    cache
        .lock()
        .expect("RAW hash cache")
        .insert(name.to_owned(), hash.clone());
    hash
}

/// Stage one committed RAW fixture into `dir` as `staged_name` and return the
/// staged path.
///
/// The staged bytes are byte-identical to the committed original, so the staged
/// copy has the same content hash, the same decode and the same render.
/// Idempotent and cheap: the files live on the same volume, so the copy is a
/// filesystem clone (measured well under a millisecond per file).
pub(crate) fn stage_raw(dir: &Path, staged_name: &str, source: &'static str) -> PathBuf {
    let (width, height, orientation) = raw_geometry(source);
    assert!(
        width > 0 && height > 0 && (1..=8).contains(&orientation),
        "RAW fixture {source} must declare a decodable geometry and orientation"
    );
    let target = dir.join(staged_name);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).expect("create staged RAW fixture dir");
    }
    std::fs::copy(raw_fixture(source), &target).unwrap_or_else(|error| {
        panic!(
            "stage RAW fixture {source} as {}: {error}",
            target.display()
        )
    });
    target
}

/// Sidecar `SourceIdentity` for a staged RAW fixture, matching what production
/// writes for a RAW source: the exact content hash, the uppercase extension as
/// `raw_format`, the LibRaw decoder identity/version and the decoded geometry.
///
/// A sidecar whose content hash or geometry disagreed with the staged bytes
/// would be *stale*, and the thumbnail worker would (correctly) refuse it — so
/// this is a fixture requirement, not decoration.
pub(crate) fn staged_source_identity(staged_name: &str, source: &str) -> SourceIdentity {
    let (width, height, orientation) = raw_geometry(source);
    SourceIdentity {
        relative_name: staged_name.to_owned(),
        content_hash: source_hash(source),
        byte_length: std::fs::metadata(raw_fixture(source))
            .expect("committed RAW fixture metadata")
            .len(),
        modified_at: None,
        raw_format: "CR3".to_owned(),
        orientation,
        decode_fingerprint: DecodeFingerprint {
            decoder: "libraw".to_owned(),
            version: lumina_raw::libraw_decode_version(),
            parameters: BTreeMap::new(),
            extras: BTreeMap::new(),
        },
        geometry_fingerprint: GeometryFingerprint {
            width,
            height,
            orientation,
            pixel_aspect_ratio: 1.0,
            extras: BTreeMap::new(),
        },
        extras: BTreeMap::new(),
    }
}

/// Pre-create the (gitignored) `.lumina/previews` folder cache of a fixture
/// directory.
///
/// The grid's thumbnail probe creates it asynchronously; pre-creating it keeps
/// the folder-tree rows of every Library golden identical between a cold and a
/// warm run. No preview is stored here: a seeded record would defeat the whole
/// point of the contract, because a cache hit would hide a broken decode.
pub(crate) fn prepare_folder_cache(dir: &Path) {
    DiskFolderCache::in_folder(dir).expect("fixture folder cache");
}

/// Wait until the production thumbnail worker has produced a Standard preview
/// for every listed entry, then pump frames so the textures are uploaded and
/// painted. Fails loudly when a real decode did not happen.
///
/// The worker persists its rendered thumbnail *after* `lumina_raw::decode_bytes`
/// succeeded, so a present record is evidence of a successful real RAW decode
/// and a missing one is a hard failure — never a silently painted placeholder.
///
/// # Why this does not spin frames
///
/// The worker pool runs on its own threads, so waiting for its records needs no
/// UI frame at all. That matters because every frame over a staged 12 MB CR3
/// costs a full BLAKE3 of the source file *on the UI thread*
/// (`FilmstripManager::refresh_source` -> `FileContentIdentity::from_path`,
/// measured ~105 MB/s here) for each visible cell — roughly 315 ms per frame
/// for a three-cell fixture. Stepping in a tight poll loop would therefore spend
/// minutes hashing and only milliseconds waiting. The frame budget below is
/// deliberate and minimal: enqueue, wait without frames, then drain and paint.
pub(crate) fn settle_thumbnails(harness: &mut Harness<'_, LuminaApp>, entries: &[StagedEntry]) {
    let mut probes: Vec<(DiskFolderCache, String, String, &'static str)> =
        Vec::with_capacity(entries.len());
    for (path, source) in entries {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("staged fixture file name")
            .to_owned();
        probes.push((
            DiskFolderCache::for_image(path).expect("staged fixture cache"),
            file_name,
            source_hash(source),
            source,
        ));
    }
    let all_present = |probes: &[(DiskFolderCache, String, String, &'static str)]| {
        probes.iter().all(|(cache, file_name, hash, _)| {
            cache
                .load_preview_with_source_hash(
                    file_name,
                    "vc-original",
                    PreviewKind::Standard,
                    hash,
                )
                .map(|preview| preview.is_some())
                .unwrap_or(false)
        })
    };
    // A few layout frames enqueue the visible cells (the grid, the filmstrip and
    // the Compare/Loupe/Survey panes schedule on different frames), spaced out
    // so a scheduled warm-up job is not missed.
    for _ in 0..2 {
        harness.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    // Wait for the worker without stepping the UI at all.
    let deadline = std::time::Instant::now() + Duration::from_secs(300);
    while !all_present(&probes) {
        assert!(
            std::time::Instant::now() < deadline,
            "the thumbnail worker never produced the staged RAW previews — a real \
             decode did not happen (GOLDEN-FIXT-31: no placeholder may be painted)"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    for (cache, file_name, hash, source) in &probes {
        assert_thumbnail_is_real(cache, file_name, hash, source);
    }
    // Drain the finished results, upload the textures and paint them.
    for _ in 0..5 {
        harness.step();
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Decode a worker-produced thumbnail and prove it carries real photographic
/// content, not the removed synthetic colour ramp.
fn assert_thumbnail_is_real(cache: &DiskFolderCache, file_name: &str, hash: &str, source: &str) {
    let (width, height, _) = raw_geometry(source);
    let scale = f64::from(THUMBNAIL_MAX_DIM) / width.max(height) as f64;
    let expected = (
        (width as f64 * scale).round() as u32,
        (height as f64 * scale).round() as u32,
    );
    let bytes = cache
        .load_preview_with_source_hash(file_name, "vc-original", PreviewKind::Standard, hash)
        .expect("load worker preview")
        .unwrap_or_else(|| panic!("worker preview disappeared for {file_name}"));
    let frame = ImageFrame::decode(&bytes).expect("worker preview decodes");
    assert_eq!(
        (frame.width, frame.height),
        expected,
        "{file_name} thumbnail must be the production downscale of {source}"
    );
    let mut colors: HashSet<[u8; 3]> = HashSet::new();
    let (chunks, _) = frame.pixels.as_chunks::<4>();
    for pixel in chunks {
        colors.insert([pixel[0], pixel[1], pixel[2]]);
    }
    assert!(
        colors.len() >= MIN_THUMBNAIL_COLORS,
        "{file_name} thumbnail carries only {} distinct colours — that is a synthetic \
         placeholder, not a real {source} decode",
        colors.len()
    );
}

/// Non-vacuous guard: the staged RAW fixtures reached the UI without a decode
/// failure.
///
/// This is the in-code form of the GOLDEN-FIXT-31 acceptance criterion: the
/// committed Library goldens used to *capture* the red
/// `LibRaw opening input failed (-100009)` banner as their intended content.
/// With real CR3 fixtures the banner must be gone, so both the app error state
/// and the rendered dialog are asserted empty here.
pub(crate) fn assert_no_raw_decode_failure(harness: &mut Harness<'_, LuminaApp>) {
    assert!(
        harness.state().error().is_none(),
        "a RAW fixture failed to decode: {:?}",
        harness.state().error()
    );
    assert!(
        harness
            .query_all_by_label_contains("LibRaw")
            .next()
            .is_none(),
        "the raw-decode failure banner must never be rendered (GOLDEN-FIXT-31)"
    );
    assert!(
        !harness.state().status().contains("LibRaw"),
        "the status line must not report a raw-decode failure, got {:?}",
        harness.state().status()
    );
}
