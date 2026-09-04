//! LRPAR-G15-IPTC-S4: file-backed IPTC metadata presets (static + dynamic).
//!
//! SOLL: `feature/product/iptc-metadata.md` §5. A preset is a portable,
//! image-independent draft template stored as a single
//! `<name>.lumina-meta-preset.json` file in the **same** user-global presets
//! directory as edit presets (`<config>/lumina/presets`). Files carry a
//! versioned envelope (`format = "lumina-meta-preset"`, `version = 1`).
//!
//! - **Static:** `placeholders` is empty; `fields` holds fixed registry values.
//! - **Dynamic:** field values embed `{placeholder}` references (names
//!   `[a-z][a-z0-9_]*`); `{{` / `}}` escape literal braces. Every application
//!   requires **all** placeholder variables; undeclared references, missing or
//!   unknown variables and values violating registry limits are loud errors.
//!
//! Failure policy (Agents.md): every deviation is rejected loudly — never
//! silently normalized, skipped, or defaulted. No absolute paths are stored.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{is_metadata_field, validate_metadata_field_value, validate_metadata_origin};

/// Envelope discriminator of a meta-preset file. Any other value is rejected
/// instead of guessed at.
pub const META_PRESET_FORMAT: &str = "lumina-meta-preset";

/// Current envelope version. Foreign versions are rejected loudly; there is
/// no silent migration (pre-MVP schema decision in `feature/README.md`).
pub const META_PRESET_VERSION: u8 = 1;

/// Fixed file suffix including the extension. The display name is the stem.
pub const META_PRESET_FILE_SUFFIX: &str = ".lumina-meta-preset.json";

/// Every reason a meta-preset file (or its application) can be refused.
/// Variants carry the path where one exists so errors point at the exact
/// offending file (no silent fallbacks).
#[derive(Debug, Error)]
pub enum MetaPresetError {
    #[error("meta-preset I/O failed while {operation} `{path}`: {message}")]
    Io {
        operation: &'static str,
        path: String,
        message: String,
    },
    #[error("meta-preset `{path}` is not valid JSON: {message}")]
    Parse { path: String, message: String },
    #[error("meta-preset `{path}` declares format `{found}`, expected `{expected}`")]
    Format {
        path: String,
        found: String,
        expected: &'static str,
    },
    #[error(
        "meta-preset `{path}` uses unsupported version {found}; supported version is {supported}"
    )]
    Version {
        path: String,
        found: u8,
        supported: u8,
    },
    #[error("meta-preset `{path}` is invalid: {reason}")]
    Invalid { path: String, reason: String },
    #[error("meta-preset `{path}` cannot be rendered: {reason}")]
    Render { path: String, reason: String },
    #[error("meta-preset name `{name}` cannot become a file name: {reason}")]
    Name { name: String, reason: String },
    #[error("meta-preset directory is unavailable: {reason}")]
    NoPresetsDir { reason: String },
}

/// One declared placeholder of a dynamic preset. `description` documents the
/// variable for the prompt dialog (CLI `--var`, GUI prompt, MCP `vars`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaPresetPlaceholder {
    pub name: String,
    pub description: String,
}

/// Versioned JSON envelope of a `<name>.lumina-meta-preset.json` file.
///
/// `fields` maps registry field IDs (SOLL §4) to value templates; a static
/// preset carries fixed values with empty `placeholders`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaPresetFile {
    pub format: String,
    pub version: u8,
    pub name: String,
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    #[serde(default)]
    pub placeholders: Vec<MetaPresetPlaceholder>,
}

/// One entry of a scanned meta-presets directory. A file that fails
/// validation stays visible as [`MetaPresetEntry::Failed`] with its error
/// text — the list never skips broken files silently.
#[derive(Debug, Clone, PartialEq)]
pub enum MetaPresetEntry {
    Available {
        path: PathBuf,
        preset: Box<MetaPresetFile>,
    },
    Failed {
        path: PathBuf,
        error: String,
    },
}

/// User-global meta-presets directory: `<config base>/lumina/presets` — the
/// **same** directory as edit presets (SOLL §5). `None` means the platform
/// config base could not be determined; callers must surface "unavailable"
/// rather than falling back to some other directory.
pub fn default_meta_presets_dir() -> Option<PathBuf> {
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
/// stored `name` always matches what the user typed. The rules mirror edit
/// presets (`lumina-gui` `preset_filename`): no separators, no control
/// characters, no `.` / `..`, no trailing dot.
pub fn meta_preset_filename(name: &str) -> Result<String, MetaPresetError> {
    let trimmed = name.trim();
    let reject = |reason: &str| MetaPresetError::Name {
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
    Ok(format!("{trimmed}{META_PRESET_FILE_SUFFIX}"))
}

/// Resolves a CLI `show` / `apply` preset spec to a file path: an existing
/// file, anything looking like a path (contains a separator or ends with the
/// meta-preset suffix), is used directly; anything else is a display name
/// resolved against `explicit_dir` (or the user-global directory when `None`).
/// A name that cannot become a file name, or a missing global directory, is a
/// loud error.
pub fn resolve_meta_preset_path(
    spec: &str,
    explicit_dir: Option<&Path>,
) -> Result<PathBuf, MetaPresetError> {
    let as_path = Path::new(spec);
    if as_path.exists()
        || spec.contains('/')
        || spec.contains('\\')
        || spec.ends_with(META_PRESET_FILE_SUFFIX)
    {
        return Ok(as_path.to_path_buf());
    }
    let filename = meta_preset_filename(spec)?;
    let dir = match explicit_dir {
        Some(dir) => dir.to_path_buf(),
        None => default_meta_presets_dir().ok_or_else(|| MetaPresetError::NoPresetsDir {
            reason: "the platform configuration directory could not be determined; \
                     pass an explicit preset file path instead"
                .to_string(),
        })?,
    };
    Ok(dir.join(filename))
}

/// Reads and fully validates one meta-preset file. Any deviation from the
/// SOLL (parse error, wrong format, foreign version, invalid content) is a
/// loud error.
pub fn load_meta_preset_file(path: &Path) -> Result<MetaPresetFile, MetaPresetError> {
    let path_string = path.display().to_string();
    let bytes = fs::read(path).map_err(|error| MetaPresetError::Io {
        operation: "reading",
        path: path_string.clone(),
        message: error.to_string(),
    })?;
    let envelope: MetaPresetFile =
        serde_json::from_slice(&bytes).map_err(|error| MetaPresetError::Parse {
            path: path_string.clone(),
            message: error.to_string(),
        })?;
    if envelope.format != META_PRESET_FORMAT {
        return Err(MetaPresetError::Format {
            path: path_string,
            found: envelope.format,
            expected: META_PRESET_FORMAT,
        });
    }
    if envelope.version != META_PRESET_VERSION {
        return Err(MetaPresetError::Version {
            path: path_string,
            found: envelope.version,
            supported: META_PRESET_VERSION,
        });
    }
    validate_meta_preset(&envelope).map_err(|reason| MetaPresetError::Invalid {
        path: path_string,
        reason,
    })?;
    Ok(envelope)
}

/// Lists the meta-presets directory sorted by file name. A missing directory
/// means "no presets saved yet" (first run), not an error; an unreadable
/// directory surfaces as a single failed entry. Invalid files appear as
/// failed entries — they are never skipped silently.
pub fn scan_meta_presets_dir(dir: &Path) -> Vec<MetaPresetEntry> {
    let read = match fs::read_dir(dir) {
        Ok(read) => read,
        Err(error) if error.kind() == ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            return vec![MetaPresetEntry::Failed {
                path: dir.to_path_buf(),
                error: format!("meta-presets directory unreadable: {error}"),
            }];
        }
    };
    let mut files: Vec<PathBuf> = read
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|name| name.ends_with(META_PRESET_FILE_SUFFIX))
        })
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|path| match load_meta_preset_file(&path) {
            Ok(preset) => MetaPresetEntry::Available {
                path,
                preset: Box::new(preset),
            },
            Err(error) => MetaPresetEntry::Failed {
                path,
                error: error.to_string(),
            },
        })
        .collect()
}

/// True for placeholder names (`[a-z][a-z0-9_]*`, SOLL §5).
pub fn is_meta_preset_placeholder_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => (),
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

enum TemplatePart {
    Literal(String),
    Var(String),
}

/// Parses one field value template: `{name}` references a declared
/// placeholder, `{{` / `}}` escape literal braces. A lone `{` / `}`, an empty
/// `{}` or a syntactically invalid reference is a loud error — never guessed.
fn parse_meta_template(value: &str) -> Result<Vec<TemplatePart>, String> {
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut chars = value.chars().peekable();
    while let Some(char) = chars.next() {
        match char {
            '{' => match chars.next() {
                Some('{') => literal.push('{'),
                Some('}') => {
                    return Err("empty placeholder `{}`; expected `{name}` with \
                              `[a-z][a-z0-9_]*` or `{{` for a literal brace"
                        .to_string());
                }
                Some(next) => {
                    let mut name = String::new();
                    name.push(next);
                    loop {
                        match chars.next() {
                            Some('}') => break,
                            Some(c) => name.push(c),
                            None => {
                                return Err(format!(
                                    "unclosed placeholder `{{{name}`; expected `}}` or \
                                     `{{{{` for a literal brace"
                                ));
                            }
                        }
                    }
                    if !is_meta_preset_placeholder_name(&name) {
                        return Err(format!(
                            "invalid placeholder `{{{name}}}`; names must match \
                             `[a-z][a-z0-9_]*`"
                        ));
                    }
                    if !literal.is_empty() {
                        parts.push(TemplatePart::Literal(std::mem::take(&mut literal)));
                    }
                    parts.push(TemplatePart::Var(name));
                }
                None => {
                    return Err(
                        "lone `{` at end of value; use `{{` for a literal brace".to_string()
                    );
                }
            },
            '}' => match chars.next() {
                Some('}') => literal.push('}'),
                _ => {
                    return Err("lone `}` in value; use `}}` for a literal brace".to_string());
                }
            },
            other => literal.push(other),
        }
    }
    if !literal.is_empty() {
        parts.push(TemplatePart::Literal(literal));
    }
    Ok(parts)
}

/// Content validation shared by load (and, through it, every consumer):
/// non-empty display name usable as a file name and as a `preset:<name>`
/// history origin, registry-only field IDs (`keywords` is rejected with its
/// routing hint, never duplicated into the draft), well-formed templates,
/// every reference declared and every declaration used.
fn validate_meta_preset(preset: &MetaPresetFile) -> Result<(), String> {
    meta_preset_filename(&preset.name)
        .map(|_| ())
        .map_err(|error| error.to_string())?;
    validate_metadata_origin(&format!("preset:{}", preset.name))
        .map_err(|error| error.to_string())?;
    for id in preset.fields.keys() {
        if is_metadata_field(id) {
            continue;
        }
        if id == "keywords" {
            return Err(
                "keywords is not a metadata preset field; keywords stay the document \
                 `keywords` field and are carried by sync, not by presets"
                    .to_string(),
            );
        }
        return Err(format!("unknown metadata field `{id}`"));
    }
    let mut declared = BTreeSet::new();
    for placeholder in &preset.placeholders {
        if !is_meta_preset_placeholder_name(&placeholder.name) {
            return Err(format!(
                "invalid placeholder name `{}`; names must match `[a-z][a-z0-9_]*`",
                placeholder.name
            ));
        }
        if !declared.insert(&placeholder.name) {
            return Err(format!("duplicate placeholder `{}`", placeholder.name));
        }
    }
    let mut used = BTreeSet::new();
    for (id, value) in &preset.fields {
        let parts = parse_meta_template(value)
            .map_err(|reason| format!("field `{id}` has an invalid template: {reason}"))?;
        for part in &parts {
            if let TemplatePart::Var(name) = part {
                if !declared.contains(name) {
                    return Err(format!(
                        "field `{id}` references undeclared placeholder `{{{name}}}`"
                    ));
                }
                used.insert(name.clone());
            }
        }
    }
    for name in &declared {
        if !used.contains(*name) {
            return Err(format!(
                "placeholder `{name}` is declared but never used in `fields`"
            ));
        }
    }
    Ok(())
}

/// Renders a preset to concrete draft values: every declared placeholder
/// requires exactly one variable (missing = loud error), unknown variables
/// are rejected loudly, substitution is a single pass (variable values are
/// never re-expanded) and every resolved value must satisfy the registry
/// limits ([`validate_metadata_field_value`]) — a limit violation rejects the
/// whole render, nothing is partially applied.
pub fn render_meta_preset(
    preset: &MetaPresetFile,
    vars: &BTreeMap<String, String>,
    path_display: &str,
) -> Result<BTreeMap<String, String>, MetaPresetError> {
    let declared: BTreeSet<&str> = preset
        .placeholders
        .iter()
        .map(|placeholder| placeholder.name.as_str())
        .collect();
    for name in vars.keys() {
        if !declared.contains(name.as_str()) {
            return Err(MetaPresetError::Render {
                path: path_display.to_string(),
                reason: format!(
                    "unknown variable `{name}` (declared placeholders: {})",
                    declared
                        .iter()
                        .map(|name| format!("{{{name}}}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            });
        }
    }
    for name in &declared {
        if !vars.contains_key(*name) {
            return Err(MetaPresetError::Render {
                path: path_display.to_string(),
                reason: format!("missing value for placeholder `{{{name}}}`"),
            });
        }
    }
    let mut resolved = BTreeMap::new();
    for (id, value) in &preset.fields {
        let parts = parse_meta_template(value).map_err(|reason| MetaPresetError::Render {
            path: path_display.to_string(),
            reason: format!("field `{id}` has an invalid template: {reason}"),
        })?;
        let mut rendered = String::new();
        for part in &parts {
            match part {
                TemplatePart::Literal(text) => rendered.push_str(text),
                TemplatePart::Var(name) => {
                    // Checked present above; single pass — the variable value
                    // itself is literal and never re-expanded.
                    rendered.push_str(&vars[name]);
                }
            }
        }
        validate_metadata_field_value(id, &rendered).map_err(|error| MetaPresetError::Render {
            path: path_display.to_string(),
            reason: format!("rendered field `{id}` is invalid: {error}"),
        })?;
        resolved.insert(id.clone(), rendered);
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, fields: &[(&str, &str)], placeholders: &[(&str, &str)]) -> MetaPresetFile {
        MetaPresetFile {
            format: META_PRESET_FORMAT.to_string(),
            version: META_PRESET_VERSION,
            name: name.to_string(),
            fields: fields
                .iter()
                .map(|(id, value)| ((*id).to_string(), (*value).to_string()))
                .collect(),
            placeholders: placeholders
                .iter()
                .map(|(name, description)| MetaPresetPlaceholder {
                    name: (*name).to_string(),
                    description: (*description).to_string(),
                })
                .collect(),
        }
    }

    fn write_raw(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn static_preset_without_placeholders_validates() {
        let preset = file(
            "Veranstaltung",
            &[("title", "Startschuss"), ("city", "Berlin")],
            &[],
        );
        assert!(validate_meta_preset(&preset).is_ok());
        let rendered = render_meta_preset(&preset, &BTreeMap::new(), "<test>").unwrap();
        assert_eq!(rendered["title"], "Startschuss");
        assert_eq!(rendered["city"], "Berlin");
    }

    #[test]
    fn wrong_format_and_version_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let mut preset = file("Ok", &[("title", "x")], &[]);
        preset.format = "something-else".into();
        let path = write_raw(
            directory.path(),
            "Other.lumina-meta-preset.json",
            &serde_json::to_vec(&preset).unwrap(),
        );
        assert!(matches!(
            load_meta_preset_file(&path).unwrap_err(),
            MetaPresetError::Format { .. }
        ));

        let mut preset = file("Ok", &[("title", "x")], &[]);
        preset.version = 2;
        let path = write_raw(
            directory.path(),
            "Future.lumina-meta-preset.json",
            &serde_json::to_vec(&preset).unwrap(),
        );
        assert!(matches!(
            load_meta_preset_file(&path).unwrap_err(),
            MetaPresetError::Version { found: 2, .. }
        ));
    }

    #[test]
    fn corrupt_json_fails_loudly_as_parse_error() {
        let directory = tempfile::tempdir().unwrap();
        let path = write_raw(
            directory.path(),
            "Broken.lumina-meta-preset.json",
            b"{ definitely not json",
        );
        assert!(matches!(
            load_meta_preset_file(&path).unwrap_err(),
            MetaPresetError::Parse { .. }
        ));
    }

    #[test]
    fn unknown_field_and_keywords_are_rejected() {
        let unknown = file("Bad", &[("nope", "x")], &[]);
        assert!(validate_meta_preset(&unknown)
            .unwrap_err()
            .contains("unknown metadata field"));
        let keywords = file("Bad", &[("keywords", "x")], &[]);
        assert!(validate_meta_preset(&keywords)
            .unwrap_err()
            .contains("keywords is not a metadata preset field"));
    }

    #[test]
    fn placeholder_name_syntax_is_enforced() {
        for bad in ["", "Event", "event-name", "1abc", "_lead", "mit leer"] {
            assert!(
                !is_meta_preset_placeholder_name(bad),
                "{bad:?} must be rejected"
            );
        }
        for good in ["ort", "event_name", "a1", "x_2_y"] {
            assert!(
                is_meta_preset_placeholder_name(good),
                "{good:?} must be accepted"
            );
        }
        let bad = file("Bad", &[("title", "{Event}")], &[("Event", "…")]);
        assert!(validate_meta_preset(&bad)
            .unwrap_err()
            .contains("invalid placeholder name"));
    }

    #[test]
    fn escapes_render_as_literal_braces() {
        let preset = file(
            "Esc",
            &[("title", "{{Fest}} {ort} {{{ort}}}")],
            &[("ort", "Stadt")],
        );
        assert!(validate_meta_preset(&preset).is_ok());
        let mut vars = BTreeMap::new();
        vars.insert("ort".to_string(), "Berlin".to_string());
        let rendered = render_meta_preset(&preset, &vars, "<test>").unwrap();
        assert_eq!(rendered["title"], "{Fest} Berlin {Berlin}");
    }

    #[test]
    fn lone_braces_and_unclosed_placeholders_are_rejected() {
        for bad in [
            "lone { brace",
            "lone } brace",
            "trailing {",
            "unclosed {ort",
            "{}",
        ] {
            let preset = file("Bad", &[("title", bad)], &[("ort", "…")]);
            let error = validate_meta_preset(&preset).unwrap_err();
            assert!(
                error.contains("invalid template"),
                "value {bad:?} must be rejected, got: {error}"
            );
        }
        // `{}` with no declared placeholders is an empty-placeholder error.
        let preset = file("Bad", &[("title", "{}")], &[]);
        assert!(validate_meta_preset(&preset).unwrap_err().contains("{}"));
    }

    #[test]
    fn undeclared_reference_and_unused_declaration_are_rejected() {
        let preset = file("Bad", &[("title", "{ort}")], &[]);
        assert!(validate_meta_preset(&preset)
            .unwrap_err()
            .contains("undeclared placeholder"));
        let preset = file("Bad", &[("title", "fest")], &[("ort", "ungenutzt")]);
        assert!(validate_meta_preset(&preset)
            .unwrap_err()
            .contains("never used"));
        let preset = file("Bad", &[("title", "{ort}")], &[("ort", "a"), ("ort", "b")]);
        assert!(validate_meta_preset(&preset)
            .unwrap_err()
            .contains("duplicate placeholder"));
    }

    #[test]
    fn render_requires_all_vars_and_rejects_unknown_vars() {
        let preset = file(
            "Dyn",
            &[("title", "{event_name} in {ort}")],
            &[("event_name", "Name"), ("ort", "Stadt")],
        );
        let mut vars = BTreeMap::new();
        vars.insert("event_name".to_string(), "Fest".to_string());
        let error = render_meta_preset(&preset, &vars, "<test>").unwrap_err();
        assert!(error.to_string().contains("missing value for placeholder"));

        vars.insert("ort".to_string(), "Berlin".to_string());
        vars.insert("extra".to_string(), "x".to_string());
        let error = render_meta_preset(&preset, &vars, "<test>").unwrap_err();
        assert!(error.to_string().contains("unknown variable"));

        vars.remove("extra");
        let rendered = render_meta_preset(&preset, &vars, "<test>").unwrap();
        assert_eq!(rendered["title"], "Fest in Berlin");
    }

    #[test]
    fn render_enforces_registry_limits() {
        let preset = file(
            "Dyn",
            &[("title", "{event_name}")],
            &[("event_name", "Name")],
        );
        let mut vars = BTreeMap::new();
        vars.insert("event_name".to_string(), "x".repeat(257));
        let error = render_meta_preset(&preset, &vars, "<test>").unwrap_err();
        assert!(error.to_string().contains("exceeds limit"));

        let preset = file("Dyn", &[("date_created", "{datum}")], &[("datum", "Datum")]);
        let mut vars = BTreeMap::new();
        vars.insert("datum".to_string(), "2026/09/04".to_string());
        assert!(render_meta_preset(&preset, &vars, "<test>").is_err());
    }

    #[test]
    fn variable_values_are_never_reexpanded() {
        let preset = file(
            "Dyn",
            &[("title", "{event_name}")],
            &[("event_name", "Name")],
        );
        let mut vars = BTreeMap::new();
        vars.insert("event_name".to_string(), "{ort}".to_string());
        let rendered = render_meta_preset(&preset, &vars, "<test>").unwrap();
        assert_eq!(rendered["title"], "{ort}");
    }

    #[test]
    fn file_roundtrip_and_scan_report_failed_files() {
        let directory = tempfile::tempdir().unwrap();
        let original = file(
            "Veranstaltung",
            &[("title", "{event_name}"), ("city", "{ort}")],
            &[("event_name", "Name"), ("ort", "Stadt")],
        );
        let path = directory
            .path()
            .join("Veranstaltung.lumina-meta-preset.json");
        fs::write(&path, serde_json::to_vec_pretty(&original).unwrap()).unwrap();
        assert_eq!(load_meta_preset_file(&path).unwrap(), original);
        write_raw(
            directory.path(),
            "Broken.lumina-meta-preset.json",
            b"{ broken",
        );

        let entries = scan_meta_presets_dir(directory.path());
        assert_eq!(entries.len(), 2);
        // Sorted by file name: Broken before Veranstaltung.
        assert!(matches!(entries[0], MetaPresetEntry::Failed { .. }));
        assert!(matches!(
            &entries[1],
            MetaPresetEntry::Available { preset, .. } if preset.name == "Veranstaltung"
        ));
        assert!(scan_meta_presets_dir(&directory.path().join("missing")).is_empty());
    }

    #[test]
    fn preset_names_must_be_filename_and_origin_safe() {
        for bad in ["", "   ", ".", "..", "a/b", "a\\b", "trailing.", "/lead"] {
            assert!(
                meta_preset_filename(bad).is_err(),
                "name {bad:?} must be rejected"
            );
        }
        assert_eq!(
            meta_preset_filename("Veranstaltung").unwrap(),
            "Veranstaltung.lumina-meta-preset.json"
        );
        // A name passing the file-name rules must also form a valid origin.
        let preset = file("/lead", &[("title", "x")], &[]);
        assert!(validate_meta_preset(&preset).is_err());
    }

    #[test]
    fn resolve_prefers_paths_over_names() {
        let directory = tempfile::tempdir().unwrap();
        let preset = file("Veranstaltung", &[("title", "x")], &[]);
        let path = directory
            .path()
            .join("Veranstaltung.lumina-meta-preset.json");
        fs::write(&path, serde_json::to_vec(&preset).unwrap()).unwrap();

        assert_eq!(
            resolve_meta_preset_path(path.to_str().unwrap(), None).unwrap(),
            path
        );
        assert_eq!(
            resolve_meta_preset_path("Veranstaltung", Some(directory.path())).unwrap(),
            path
        );
    }
}
