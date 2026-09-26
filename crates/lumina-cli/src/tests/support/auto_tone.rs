//! AUTO-TONE-CLI-6: shared deterministic fixture and inspection helpers for
//! the `process --auto-tone` / `regenerate` contract tests.
//!
//! The fixture is a documented pixel function (no external asset, no licence
//! obligation), like the `lumina-core` reference fixtures: 16x16 RGB where
//! `r = (13x + 5y + 1) % 179`, `g = (7x + 11y + 2) % 137`,
//! `b = (3x + 17y + 3) % 113`, alpha 255. It is deliberately **not** uniform
//! and not gray, so all six AUTO-TONE-2 sliders get non-trivial values and the
//! render is a real (non-identity) pixel result. All six values are also
//! **exactly representable in the sidecar JSON**: this workspace builds
//! `serde_json` without `float_roundtrip`, so a value like `0.44062500000000004`
//! would come back one ULP lower from a persisted sidecar. The fixture is
//! chosen so the pinned goldens are the same in memory and after a
//! `save`/`load` round trip — the test then needs no tolerance at all.
//!
//! The six auto values of this fixture at `target_luminance = 0.5` are
//! `0.8810589272764926`, `0.6533260183788233`, `0.4423828125`,
//! `-0.0107421875`, `0.16386718750000007`, `0.1677734375` (exposure, contrast,
//! whites, blacks, highlights, shadows) — taken from the `regenerate` writer on
//! HEAD a992f93, which already wrote the full six-slider contract.
//! `process --auto-tone` must produce exactly these.

use super::*;

/// The six AUTO-TONE-2 adjustment keys, in contract order.
pub(crate) const SLIDER_KEYS: [&str; 6] = [
    "exposure",
    "contrast",
    "whites",
    "blacks",
    "highlights",
    "shadows",
];

/// The six `auto_features` mirror field names, parallel to [`SLIDER_KEYS`].
pub(crate) const MIRROR_FIELDS: [&str; 6] = [
    "auto_exposure",
    "auto_contrast",
    "auto_whites",
    "auto_blacks",
    "auto_highlights",
    "auto_shadows",
];

/// Deterministic 16x16 RGB fixture (see the module docs for the formula).
pub(crate) fn auto_tone_frame() -> ImageFrame {
    let mut pixels = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16u32 {
        for x in 0..16u32 {
            pixels.push(((13 * x + 5 * y + 1) % 179) as u8);
            pixels.push(((7 * x + 11 * y + 2) % 137) as u8);
            pixels.push(((3 * x + 17 * y + 3) % 113) as u8);
            pixels.push(255);
        }
    }
    ImageFrame::new(16, 16, pixels).unwrap()
}

/// Writes the fixture as a PNG and imports it, so the sidecar exists and the
/// source identity is the one `process` validates against.
pub(crate) fn auto_tone_input(directory: &Path, name: &str) -> PathBuf {
    let input = directory.join(name);
    let frame = auto_tone_frame();
    fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    import_file(ImportArgs {
        input: input.clone(),
        json: true,
        migrate: false,
    })
    .unwrap();
    input
}

/// The six slider values of `recipe` as an array in [`SLIDER_KEYS`] order.
pub(crate) fn slider_values(recipe: &lumina_sidecar::EditRecipe) -> Vec<f64> {
    SLIDER_KEYS
        .iter()
        .map(|key| {
            recipe
                .adjustments
                .get(*key)
                .copied()
                .unwrap_or_else(|| panic!("missing slider `{key}`"))
        })
        .collect()
}

/// The six mirror values of `recipe` as an array in [`MIRROR_FIELDS`] order.
/// `None` the moment a single mirror is missing — the all-or-nothing view.
pub(crate) fn mirrored_values(recipe: &lumina_sidecar::EditRecipe) -> Option<Vec<Option<f64>>> {
    let auto = &recipe.auto_features;
    let values = vec![
        auto.auto_exposure,
        auto.auto_contrast,
        auto.auto_whites,
        auto.auto_blacks,
        auto.auto_highlights,
        auto.auto_shadows,
    ];
    (values.iter().all(Option::is_some)).then_some(values)
}

/// Decodes a rendered image file and returns its exact RGBA bytes.
pub(crate) fn rendered_pixels(path: &Path) -> Vec<u8> {
    let bytes = fs::read(path).unwrap();
    ImageFrame::decode(&bytes)
        .expect("the rendered output must be a decodable image")
        .pixels
}

/// Lowercase hex of a byte slice (for the byte-level goldens).
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The AUTO-TONE-CLI-6 clause-(9) fixture family, member `k` (`k = 0..40`):
/// 16x16, alpha 255, with
///
/// ```text
/// r = ((3 + 7k)·x + (5 + 3k)·y + (1 + k)) mod (97 + 4k)
/// g = ((5 + 11k)·x + (3 + 5k)·y + (2 + 2k)) mod (89 + 4k)
/// b = ((7 + 13k)·x + (2 + 7k)·y + (3 + 3k)) mod (83 + 4k)
/// ```
///
/// Every coefficient is an integer, so the family is deterministic and needs no
/// external asset. It is deliberately *not* the [`auto_tone_frame`] fixture:
/// about half its members produce at least one auto-tone value that does not
/// survive this workspace's `serde_json` round trip, which is exactly the case
/// AUTO-TONE-CLI-6 clause (9) has to cover.
pub(crate) fn auto_tone_family_frame(k: u32) -> ImageFrame {
    let mut pixels = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16u32 {
        for x in 0..16u32 {
            let r = ((3 + 7 * k) * x + (5 + 3 * k) * y + (1 + k)) % (97 + 4 * k);
            let g = ((5 + 11 * k) * x + (3 + 5 * k) * y + (2 + 2 * k)) % (89 + 4 * k);
            let b = ((7 + 13 * k) * x + (2 + 7 * k) * y + (3 + 3 * k)) % (83 + 4 * k);
            pixels.push(r as u8);
            pixels.push(g as u8);
            pixels.push(b as u8);
            pixels.push(255);
        }
    }
    ImageFrame::new(16, 16, pixels).unwrap()
}

/// Writes and imports family member `k`.
pub(crate) fn auto_tone_family_input(directory: &Path, k: u32) -> PathBuf {
    let input = directory.join(format!("family-{k:02}.png"));
    let frame = auto_tone_family_frame(k);
    fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    import_file(ImportArgs {
        input: input.clone(),
        json: true,
        migrate: false,
    })
    .unwrap();
    input
}

/// The signed distance between two `f64` in units in the last place. Used to
/// state the serde_json round-trip boundary as a measured number instead of a
/// tolerance.
pub(crate) fn ulp_distance(a: f64, b: f64) -> i64 {
    (a.to_bits() as i64) - (b.to_bits() as i64)
}

/// Reads a value back the way **Rust's own** `str::parse` does, i.e. correctly
/// rounded — the exact `f64` a sidecar's decimal text denotes. `serde_json`'s
/// parser is the one that can lose a bit, the text itself is always exact.
pub(crate) fn parse_exact(literal: &str) -> f64 {
    literal.trim().parse().unwrap()
}

/// The `serde_json` round trip of one value, as this workspace performs it.
pub(crate) fn serde_json_round_trip(value: f64) -> f64 {
    serde_json::from_str(&serde_json::to_string(&value).unwrap()).unwrap()
}

/// The exact decimal token `serde_json` wrote for `key` in the **live** copy's
/// `recipe.adjustments`, parsed with Rust's correctly rounded `str::parse`.
///
/// This deliberately bypasses `serde_json::Value::as_f64`, which would apply the
/// very lossy parser under test and hide the drift: the text is the shortest
/// representation of the value that was actually stored, and `str::parse`
/// recovers exactly that value.
pub(crate) fn slider_literal(sidecar_text: &str, key: &str) -> String {
    let adjustments_at = sidecar_text
        .find("\"adjustments\":")
        .expect("the live copy has an `adjustments` object");
    let object = &sidecar_text[adjustments_at..];
    let end = object.find('}').expect("`adjustments` is a flat object");
    let object = &object[..end];
    let needle = format!("\"{key}\":");
    let at = object
        .find(&needle)
        .unwrap_or_else(|| panic!("`{key}` is missing from `adjustments`"))
        + needle.len();
    let rest = object[at..].trim_start();
    let length = rest
        .find(|c: char| {
            !(c.is_ascii_digit() || c == '-' || c == '+' || c == '.' || c == 'e' || c == 'E')
        })
        .unwrap_or(rest.len());
    rest[..length].to_string()
}

/// The six slider literals of the live copy, in [`SLIDER_KEYS`] order, exactly
/// as written to the sidecar.
pub(crate) fn slider_literals(sidecar_text: &str) -> [String; 6] {
    std::array::from_fn(|index| slider_literal(sidecar_text, SLIDER_KEYS[index]))
}

/// A `process --auto-tone` run on `input` with the given argument tweaks.
pub(crate) fn process_auto_tone(
    input: &Path,
    output: &Path,
    mutate: impl FnOnce(&mut ProcessArgs),
) {
    let mut args = process_args(input, output);
    args.auto_tone = true;
    mutate(&mut args);
    let mut warnings = Vec::new();
    process_selected(args, 90, None, MaskPolicy::Warn, &mut warnings).unwrap();
    assert!(
        warnings.is_empty(),
        "the Auto-Tone fixture carries no mask work: {warnings:?}"
    );
}

/// Writes a `--preset` file whose recipe sets exactly `adjustments`.
pub(crate) fn write_preset(path: &Path, adjustments: &[(&str, f64)]) {
    let entries: Vec<String> = adjustments
        .iter()
        .map(|(key, value)| format!("\"{key}\": {value}"))
        .collect();
    let json = format!(
        r#"{{"id": "at6", "name": "at6", "recipe": {{"adjustments": {{{}}}}}}}"#,
        entries.join(", ")
    );
    fs::write(path, json).unwrap();
}
