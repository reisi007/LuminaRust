//! File-backed user presets (F-009).
//!
//! SOLL: `feature/product/virtual-copies.md` § Presets (F-009). A preset is a
//! reusable, image-independent recipe template stored as a single
//! `<name>.lumina-preset.json` file in the user's global presets directory.
//! Files carry a dedicated versioned envelope (`format` + `schema_version`)
//! around the sidecar `Preset` structure, so the recipe model stays identical
//! to the sidecar while the preset schema evolves independently.
//!
//! Failure policy (Agents.md): invalid or foreign artifacts are rejected
//! loudly — nothing is silently normalized, skipped, or re-created. The
//! directory scan reports every failing file individually instead of hiding
//! it.
//!
//! Native-only capability (`platform/capability-matrix.md`, "Virtuelle Kopien
//! / Presets"): the whole module is compiled out on wasm32, where file-backed
//! presets remain post-MVP.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use lumina_sidecar::{EditRecipe, Preset};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Envelope discriminator of a preset file. A file with any other value is
/// rejected instead of guessed at.
pub const PRESET_FORMAT: &str = "lumina-preset";

/// Current envelope major version. Foreign versions are rejected loudly; there
/// is no silent migration (pre-MVP schema decision in `feature/README.md`).
pub const PRESET_SCHEMA_VERSION: u32 = 1;

/// Fixed file suffix including the extension. The display name is the stem.
pub const PRESET_FILE_SUFFIX: &str = ".lumina-preset.json";

/// MVP adjustment keys with their inclusive pipeline ranges
/// (`architecture/pipeline.md`). Any other key inside a preset recipe is
/// rejected on load — unknown keys are never carried silently into a target
/// recipe.
const PRESET_ADJUSTMENT_RANGES: [(&str, f64, f64); 4] = [
    ("exposure", -10.0, 10.0),
    ("contrast", -1.0, 1.0),
    ("highlights", -1.0, 1.0),
    ("shadows", -1.0, 1.0),
];

/// Every reason a preset file can be refused. Variants carry the path so UI
/// errors point at the exact offending file (no silent fallbacks).
#[derive(Debug, Error)]
pub enum PresetFileError {
    #[error("preset I/O failed while {operation} `{path}`: {message}")]
    Io {
        operation: &'static str,
        path: String,
        message: String,
    },
    #[error("preset `{path}` is not valid JSON: {message}")]
    Parse { path: String, message: String },
    #[error("preset `{path}` declares format `{found}`, expected `{expected}`")]
    Format {
        path: String,
        found: String,
        expected: &'static str,
    },
    #[error(
        "preset `{path}` uses unsupported schema version {found}; supported \
         major version is {supported}"
    )]
    SchemaVersion {
        path: String,
        found: u32,
        supported: u32,
    },
    #[error("preset `{path}` is invalid: {reason}")]
    Invalid { path: String, reason: String },
    #[error("preset name `{name}` cannot become a file name: {reason}")]
    Name { name: String, reason: String },
    #[error("preset file `{path}` already exists")]
    Collision { path: String },
}

/// Versioned JSON envelope of a `<name>.lumina-preset.json` file.
///
/// `preset` reuses the sidecar [`Preset`] structure verbatim (same recipe
/// model); no source identity, geometry, mask references, or binary payloads
/// are permitted inside.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresetFile {
    pub format: String,
    pub schema_version: u32,
    pub preset: Preset,
}

/// One entry of a scanned presets directory. A file that fails validation
/// stays visible as [`PresetEntry::Failed`] with its error text — the list
/// never skips broken files silently.
#[derive(Debug, Clone, PartialEq)]
pub enum PresetEntry {
    Available { path: PathBuf, preset: Box<Preset> },
    Failed { path: PathBuf, error: String },
}

/// User-global presets directory:
/// `<config base>/lumina/presets` (macOS `~/Library/Application Support`,
/// XDG `$XDG_CONFIG_HOME`/`~/.config`, Windows `%APPDATA%`).
///
/// `None` means the platform config base could not be determined; callers must
/// surface "unavailable" rather than falling back to some other directory.
pub fn default_presets_dir() -> Option<PathBuf> {
    config_base().map(|base| base.join("lumina").join("presets"))
}

fn config_base() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library/Application Support"))
    }
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA").map(PathBuf::from)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        match std::env::var_os("XDG_CONFIG_HOME") {
            Some(value) if !value.is_empty() => Some(PathBuf::from(value)),
            _ => std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".config")),
        }
    }
}

/// Maps a validated display name to its file name. The name is not rewritten:
/// anything unusable as a single file name component is an error, so the
/// stored `name` always matches what the user typed.
pub fn preset_filename(name: &str) -> Result<String, PresetFileError> {
    let trimmed = name.trim();
    let reject = |reason: &str| PresetFileError::Name {
        name: name.to_string(),
        reason: reason.to_string(),
    };
    if trimmed.is_empty() {
        return Err(reject("the name is empty"));
    }
    if trimmed == "." || trimmed == ".." {
        return Err(reject("`.` and `..` are not usable names"));
    }
    if trimmed.ends_with('.') {
        return Err(reject("a trailing dot is not a usable file name"));
    }
    for character in trimmed.chars() {
        if matches!(character, '/' | '\\') {
            return Err(reject("path separators are not allowed"));
        }
        if character.is_control() || character == '\0' {
            return Err(reject("control characters are not allowed"));
        }
    }
    Ok(format!("{trimmed}{PRESET_FILE_SUFFIX}"))
}

/// Writes the preset as `<dir>/<name>.lumina-preset.json` using the shared
/// atomic write pattern ([`lumina_sidecar::write_atomically`], temp file +
/// rename), so an aborted write can never leave a half-written preset behind
/// that would later load as valid.
///
/// With `overwrite = false` an existing target is a [`PresetFileError::
/// Collision`]. With `overwrite = true` the file is replaced deliberately:
/// the display name is the preset's identity and same-name saves are
/// documented update semantics (`virtual-copies.md` § Dateinamen- und
/// Kollisionsregeln). The existence check plus rename has a theoretical race
/// window on concurrent writers; the GUI is single-user, and the flag makes
/// replacement an explicit caller decision either way.
pub fn save_preset_file(
    dir: &Path,
    preset: &Preset,
    overwrite: bool,
) -> Result<PathBuf, PresetFileError> {
    let filename = preset_filename(&preset.name)?;
    let path = dir.join(&filename);
    let path_string = path.display().to_string();

    validate_preset(preset).map_err(|reason| PresetFileError::Invalid {
        path: path_string.clone(),
        reason,
    })?;

    if !overwrite
        && fs::exists(&path).map_err(|error| PresetFileError::Io {
            operation: "probing",
            path: path_string.clone(),
            message: error.to_string(),
        })?
    {
        return Err(PresetFileError::Collision { path: path_string });
    }

    let envelope = PresetFile {
        format: PRESET_FORMAT.to_string(),
        schema_version: PRESET_SCHEMA_VERSION,
        preset: preset.clone(),
    };
    // Pretty-printed on purpose: presets are user-shareable files.
    let bytes = serde_json::to_vec_pretty(&envelope).map_err(|error| PresetFileError::Io {
        operation: "serializing",
        path: path_string.clone(),
        message: error.to_string(),
    })?;
    fs::create_dir_all(dir).map_err(|error| PresetFileError::Io {
        operation: "creating",
        path: dir.display().to_string(),
        message: error.to_string(),
    })?;
    lumina_sidecar::write_atomically(&path, &bytes).map_err(|error| PresetFileError::Io {
        operation: "atomically writing",
        path: path_string.clone(),
        message: error.to_string(),
    })?;
    Ok(path)
}

/// Reads and fully validates one preset file, returning the inner [`Preset`].
/// Any deviation from the SOLL (parse error, wrong format, foreign schema
/// version, invalid content) is a loud error.
pub fn load_preset_file(path: &Path) -> Result<Preset, PresetFileError> {
    let path_string = path.display().to_string();
    let bytes = fs::read(path).map_err(|error| PresetFileError::Io {
        operation: "reading",
        path: path_string.clone(),
        message: error.to_string(),
    })?;
    let envelope: PresetFile =
        serde_json::from_slice(&bytes).map_err(|error| PresetFileError::Parse {
            path: path_string.clone(),
            message: error.to_string(),
        })?;
    if envelope.format != PRESET_FORMAT {
        return Err(PresetFileError::Format {
            path: path_string,
            found: envelope.format,
            expected: PRESET_FORMAT,
        });
    }
    if envelope.schema_version != PRESET_SCHEMA_VERSION {
        return Err(PresetFileError::SchemaVersion {
            path: path_string,
            found: envelope.schema_version,
            supported: PRESET_SCHEMA_VERSION,
        });
    }
    validate_preset(&envelope.preset).map_err(|reason| PresetFileError::Invalid {
        path: path_string,
        reason,
    })?;
    Ok(envelope.preset)
}

/// Lists the presets directory sorted by file name. A missing directory means
/// "no presets saved yet" (first run), not an error; an unreadable directory
/// surfaces as a single failed entry. Invalid files appear as failed entries —
/// they are never skipped silently.
pub fn scan_presets_dir(dir: &Path) -> Vec<PresetEntry> {
    let read = match fs::read_dir(dir) {
        Ok(read) => read,
        Err(error) if error.kind() == ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            return vec![PresetEntry::Failed {
                path: dir.to_path_buf(),
                error: format!("presets directory unreadable: {error}"),
            }]
        }
    };
    let mut files: Vec<PathBuf> = read
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|name| name.ends_with(PRESET_FILE_SUFFIX))
        })
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|path| match load_preset_file(&path) {
            Ok(preset) => PresetEntry::Available {
                path,
                preset: Box::new(preset),
            },
            Err(error) => PresetEntry::Failed {
                path,
                error: error.to_string(),
            },
        })
        .collect()
}

/// Content validation shared by save and load: non-empty name, only known MVP
/// adjustment keys within their pipeline ranges, only the
/// `exposure_semantics` option, and no leakage of other recipe fields
/// (auto features, curves, geometry, masks, …) into a preset.
fn validate_preset(preset: &Preset) -> Result<(), String> {
    if preset.name.trim().is_empty() {
        return Err("the preset name is empty".to_string());
    }
    for (key, value) in &preset.recipe.adjustments {
        let (_, minimum, maximum) = PRESET_ADJUSTMENT_RANGES
            .iter()
            .find(|(name, _, _)| name == key)
            .ok_or_else(|| {
                format!(
                    "unknown adjustment key `{key}` (allowed: exposure, contrast, \
                     highlights, shadows)"
                )
            })?;
        if !value.is_finite() || value < minimum || value > maximum {
            return Err(format!(
                "adjustment `{key}` = {value} is outside the pipeline range \
                 [{minimum}, {maximum}]"
            ));
        }
    }
    for (key, value) in &preset.recipe.options {
        if key != "exposure_semantics" {
            return Err(format!("unknown recipe option `{key}`"));
        }
        if value != "absolute" && value != "relative" {
            return Err(format!(
                "`exposure_semantics` must be `absolute` or `relative`, found \
                 `{value}`"
            ));
        }
    }
    if let Some(reason) = recipe_scope_violation(&preset.recipe) {
        return Err(reason);
    }
    Ok(())
}

/// A preset recipe may differ from the default recipe only in `adjustments`
/// and `options`. Anything else (enabled auto features, curves, geometry, …)
/// would silently change targets on apply, so it is rejected. Implemented as
/// a serialized comparison against the default so future additive recipe
/// fields stay covered without touching this check.
fn recipe_scope_violation(recipe: &EditRecipe) -> Option<String> {
    let mut actual = serde_json::to_value(recipe).ok()?;
    let mut default = serde_json::to_value(EditRecipe::default()).ok()?;
    for value in [&mut actual, &mut default] {
        if let serde_json::Value::Object(map) = value {
            map.remove("adjustments");
            map.remove("options");
        }
    }
    if actual == default {
        None
    } else {
        Some(
            "the recipe sets fields outside adjustments/exposure_semantics \
             (presets must not carry auto features, curves, geometry, masks, \
             etc.)"
                .to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn sample_preset(name: &str) -> Preset {
        let mut recipe = EditRecipe::default();
        recipe.adjustments.insert("exposure".into(), 0.35);
        recipe.adjustments.insert("contrast".into(), -0.2);
        recipe
            .options
            .insert("exposure_semantics".into(), "absolute".into());
        Preset {
            id: format!("preset-{name}"),
            name: name.into(),
            recipe,
            extras: BTreeMap::new(),
        }
    }

    fn write_raw(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn preset_filename_rules_reject_unusable_names_loudly() {
        assert_eq!(
            preset_filename("Warm Look").unwrap(),
            "Warm Look.lumina-preset.json"
        );
        for bad in [
            "",
            "   ",
            ".",
            "..",
            "a/b",
            "a\\b",
            "trailing.",
            "\u{7}bell",
        ] {
            assert!(
                preset_filename(bad).is_err(),
                "name {bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn save_load_roundtrip_returns_identical_preset() {
        let directory = tempfile::tempdir().unwrap();
        let original = sample_preset("Roundtrip");
        let path = save_preset_file(directory.path(), &original, false).unwrap();
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            "Roundtrip.lumina-preset.json"
        );
        let loaded = load_preset_file(&path).unwrap();
        assert_eq!(loaded, original);
    }

    #[test]
    fn collision_is_an_error_until_overwrite_is_explicit() {
        let directory = tempfile::tempdir().unwrap();
        let first = sample_preset("Same Name");
        save_preset_file(directory.path(), &first, false).unwrap();

        let second = sample_preset("Same Name");
        let mut second_recipe = EditRecipe::default();
        second_recipe.adjustments.insert("shadows".into(), 0.5);
        second_recipe
            .options
            .insert("exposure_semantics".into(), "absolute".into());
        let second = Preset {
            recipe: second_recipe,
            ..second
        };

        let collision = save_preset_file(directory.path(), &second, false).unwrap_err();
        assert!(matches!(collision, PresetFileError::Collision { .. }));

        let path = save_preset_file(directory.path(), &second, true).unwrap();
        let loaded = load_preset_file(&path).unwrap();
        assert_eq!(loaded.recipe.adjustments, second.recipe.adjustments);
    }

    #[test]
    fn corrupt_json_fails_loudly_as_parse_error() {
        let directory = tempfile::tempdir().unwrap();
        let path = write_raw(
            directory.path(),
            "Broken.lumina-preset.json",
            b"{ definitely not json",
        );
        let error = load_preset_file(&path).unwrap_err();
        assert!(matches!(error, PresetFileError::Parse { .. }));
    }

    #[test]
    fn foreign_schema_version_is_rejected_not_migrated() {
        let directory = tempfile::tempdir().unwrap();
        let mut envelope = PresetFile {
            format: PRESET_FORMAT.into(),
            schema_version: PRESET_SCHEMA_VERSION,
            preset: sample_preset("Future"),
        };
        envelope.schema_version = 2;
        let path = write_raw(
            directory.path(),
            "Future.lumina-preset.json",
            &serde_json::to_vec(&envelope).unwrap(),
        );
        let error = load_preset_file(&path).unwrap_err();
        assert!(matches!(
            error,
            PresetFileError::SchemaVersion {
                found: 2,
                supported: 1,
                ..
            }
        ));
    }

    #[test]
    fn wrong_format_discriminator_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let mut envelope = PresetFile {
            format: PRESET_FORMAT.into(),
            schema_version: PRESET_SCHEMA_VERSION,
            preset: sample_preset("Other"),
        };
        envelope.format = "something-else".into();
        let path = write_raw(
            directory.path(),
            "Other.lumina-preset.json",
            &serde_json::to_vec(&envelope).unwrap(),
        );
        let error = load_preset_file(&path).unwrap_err();
        assert!(
            matches!(error, PresetFileError::Format { ref found, .. } if found == "something-else")
        );
    }

    #[test]
    fn out_of_range_and_unknown_adjustment_keys_are_rejected() {
        let mut over_range = sample_preset("Too Bright");
        over_range
            .recipe
            .adjustments
            .insert("exposure".into(), 42.0);
        let error = validate_preset(&over_range).unwrap_err();
        assert!(error.contains("`exposure`"), "got: {error}");

        let mut unknown = sample_preset("Vibrant");
        unknown.recipe.adjustments.insert("vibrance".into(), 0.3);
        let error = validate_preset(&unknown).unwrap_err();
        assert!(error.contains("unknown adjustment key"), "got: {error}");
    }

    #[test]
    fn recipe_scope_leakage_is_rejected() {
        let mut leaky = sample_preset("Leaky");
        leaky.recipe.auto_features.enable_auto_tone = true;
        let error = validate_preset(&leaky).unwrap_err();
        assert!(error.contains("outside adjustments"), "got: {error}");
    }

    #[test]
    fn invalid_exposure_semantics_value_is_rejected() {
        let mut bogus = sample_preset("Bogus");
        bogus
            .recipe
            .options
            .insert("exposure_semantics".into(), "banana".into());
        let error = validate_preset(&bogus).unwrap_err();
        assert!(error.contains("absolute"), "got: {error}");
    }

    #[test]
    fn missing_exposure_semantics_defaults_to_absolute_and_loads() {
        let directory = tempfile::tempdir().unwrap();
        let mut minimal = sample_preset("Minimal");
        minimal.recipe.options.clear();
        let path = save_preset_file(directory.path(), &minimal, false).unwrap();
        let loaded = load_preset_file(&path).unwrap();
        assert_eq!(loaded, minimal);
    }

    #[test]
    fn relative_exposure_preset_roundtrips_for_apply_time_check() {
        let directory = tempfile::tempdir().unwrap();
        let mut relative = sample_preset("Relative");
        relative
            .recipe
            .options
            .insert("exposure_semantics".into(), "relative".into());
        let path = save_preset_file(directory.path(), &relative, false).unwrap();
        assert_eq!(load_preset_file(&path).unwrap(), relative);
    }

    #[test]
    fn scan_reports_sorted_available_entries_and_failed_files_individually() {
        let directory = tempfile::tempdir().unwrap();
        save_preset_file(directory.path(), &sample_preset("Beta"), false).unwrap();
        save_preset_file(directory.path(), &sample_preset("Alpha"), false).unwrap();
        write_raw(directory.path(), "Broken.lumina-preset.json", b"{ broken");

        let entries = scan_presets_dir(directory.path());
        assert_eq!(entries.len(), 3);
        match (&entries[0], &entries[1]) {
            (
                PresetEntry::Available { preset: first, .. },
                PresetEntry::Available { preset: second, .. },
            ) => {
                // Sorted by FILE name, so Alpha before Beta despite creation order.
                assert_eq!(first.name, "Alpha");
                assert_eq!(second.name, "Beta");
            }
            other => panic!("expected two available entries, got {other:?}"),
        }
        match &entries[2] {
            PresetEntry::Failed { path, error } => {
                assert!(path
                    .to_string_lossy()
                    .ends_with("Broken.lumina-preset.json"));
                assert!(!error.is_empty());
            }
            other => panic!("expected the corrupt file as failed entry, got {other:?}"),
        }
    }

    #[test]
    fn missing_directory_scans_as_empty_not_error() {
        // The tempdir binding only keeps the parent directory alive.
        let _directory = tempfile::tempdir().unwrap();
        let missing = _directory.path().join("does-not-exist");
        assert!(scan_presets_dir(&missing).is_empty());
    }

    #[test]
    fn empty_name_fails_at_save_before_touching_the_filesystem() {
        let directory = tempfile::tempdir().unwrap();
        let mut empty = sample_preset("");
        empty.name = "   ".into();
        let error = save_preset_file(directory.path(), &empty, false).unwrap_err();
        assert!(matches!(error, PresetFileError::Name { .. }));
        // Nothing was written: the directory is still empty.
        assert!(fs::read_dir(directory.path()).unwrap().next().is_none());
    }
}
