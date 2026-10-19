//! RESAMPLER-GATE-SCOPE-78 — der **Workspace-Scan** des Resampler-Gates.
//!
//! Der Scanner besteht aus zwei Teilen: dem Lexikon in `resampler_lex.rs`
//! (die Frage „ist diese Zeile eine Funktionsdefinition, und wie lautet ihre
//! Signatur?“) und diesem Teil (die Frage „welche Dateien werden ueberhaupt
//! gelesen?“).
//!
//! **Was gescannt wird:** der **gesamte Workspace** — alle Crates, je
//! `src/`, `tests/`, `benches/`, `examples/`, `bench/`, nur `.rs`. `target/`
//! (dort liegen Kopien der Quellen) und `.git/` sind ausgeschlossen. Der
//! Workspace-Root kommt aus `env!("CARGO_MANIFEST_DIR")`, **nicht** aus dem
//! relativen CWD.
//!
//! **Die Datei des Gates selbst** (`GATE_OWN_FILES`) ist **nicht** Teil des
//! Ergebnisses: der Gate-Test nennt die Altnamen selbst und traegt ein
//! Detektor-Fixture mit echten Definitionszeilen. Die uebrigen Testdateien
//! (auch `downscale_anchor.rs` selbst, dessen Doc-Kommentare die Altnamen
//! nennen) bleiben im Suchbereich — das Lexikon ignoriert Kommentar- und
//! String-Zeilen.
//!
//! ## Die Selbstreferenz des Gate-Tests
//!
//! Der Gate-Test nennt die Altnamen selbst (String-Literale) **und** traegt
//! ein Detektor-Fixture, das echte Definitionszeilen enthaelt. Der
//! **Ausschluss der Gate-Datei** (`GATE_OWN_FILES`) ist der **tragende**
//! Mechanismus: ohne ihn liefert der Scan **drei** Treffer in genau dieser
//! Datei (das Fixture mit drei Altnamen-Definitionen). Der zweite Mechanismus
//! — der Detektor erkennt nur `fn <name>`-**Definitionen**, nicht blosse
//! Nennungen — ist eine **zusaetzliche** Absicherung: ein Hinweis im Kommentar
//! oder in einem String (etwa die Konstante
//! `"srgb_rgba8_downscale_bilinear"` in `lumina-cull`) ist keine Definition.
//!
//! **Benannte Grenze des Detektors:** der Inhalt von **Raw-Strings** wird
//! zeilenweise mitgelesen (Shader-Quelltext in einem `r#"…"#`-Literal zaehlt
//! mit). Ein WGSL-Funktionsname, der auf einem Altnamen oder einem Stiel
//! beruht, koennte deshalb ein Falsch-Rot erzeugen — das ist die bewusst in
//! Kauf genommene Kehrseite davon, dass der Ausschluss der Gate-Datei der
//! einzige Ort ist, an dem ein solches Literal steht.
//!
//! **Weitere benannte Grenzen:**
//!
//! * Der Detektor arbeitet **zeilenweise**. Der Name einer Definition muss in
//!   derselben Zeile stehen wie das `fn`; ein Kommentar zwischen `fn` und dem
//!   Namen wird nicht erkannt.
//! * Der Typ-Anker ist der **Typname** `ImageFrame`.
//! * Die Altnamen werden **exakt** verglichen: `downscale_bilinear_v2` ist

use std::path::{Path, PathBuf};

use crate::resampler_lex::{definition_at, impl_header, signature_of};

// ---------------------------------------------------------------------------
// Der durchsuchte Bereich — im Code benannt, nicht nur im Kommentar.
// ---------------------------------------------------------------------------

/// Die je Crate durchsuchten Unterverzeichnisse (relativ zur Crate-Wurzel).
///
/// `bench/` deckt die **Custom-Harness-Bench-Ziele** ab (z. B.
/// `crates/lumina-bench/bench/core.rs`, per `Cargo.toml` `path = "bench/core.rs"`)
/// — sie liegen nicht unter `benches/` und waeren sonst ausserhalb des
/// Bereichs.
///
/// **Nicht** Teil des Bereichs (bewusst benannt):
///
/// * `build.rs` auf der Crate-Wurzel (`crates/lumina-lensfun/build.rs`) — ein
///   Build-Skript, kein Produkt- oder Testquelltext.
/// * `agent-harness/` — ein eigenes Crate mit eigenem `Cargo.lock`, **kein**
///   Workspace-Mitglied; es liegt zudem nicht unter `crates/`.
pub const SCANNED_SUBDIRECTORIES: [&str; 5] = ["src", "tests", "benches", "examples", "bench"];

/// Die durchsuchte Dateiendung (ohne Punkt).
pub const SCANNED_EXTENSION: &str = "rs";

/// Verzeichnisnamen, die nie durchsucht werden: Build-Artefakte (dort liegen
/// **Kopien** der Quellen) und VCS-Metadaten.
pub const EXCLUDED_DIRECTORY_NAMES: [&str; 2] = ["target", ".git"];

/// Die **Datei** des Gates selbst. Sie ist **nicht** Teil des Suchbereichs:
/// der Gate-Test nennt die Altnamen selbst und traegt ein Detektor-Fixture
/// mit echten Definitionszeilen. Die uebrigen Testdateien (auch
/// `downscale_anchor.rs` selbst, dessen Doc-Kommentare die Altnamen nennen)
/// bleiben im Suchbereich — das Lexikon ignoriert Kommentar- und
/// String-Zeilen.
pub const GATE_OWN_FILES: [&str; 1] =
    ["crates/lumina-core/tests/downscale_anchor/resampler_gate.rs"];

// ---------------------------------------------------------------------------
// Die gefundenen Definitionen.
// ---------------------------------------------------------------------------

/// Eine gefundene Definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    /// Pfad relativ zum Workspace-Root.
    pub path: String,
    /// 1-basierte Zeilennummer der Definitionszeile.
    pub line: usize,
    /// Der Name der Definition.
    pub name: String,
    /// Der umgebende `impl`-Typ (`""` = frei stehende Funktion).
    pub impl_type: String,
    /// Die Parameter der Signatur.
    pub params: String,
    /// Der Rueckgabetyp der Signatur.
    pub returns: String,
}

impl Definition {
    /// Beruehrt diese Definition das `ImageFrame`? Das ist der **Typ-Anker**
    /// der begrenzten semantischen Suche: eine Definition, die Frame-Pixel
    /// verbraucht **oder** erzeugt, ist im Frame-Bereich.
    #[must_use]
    pub fn touches_frame(&self) -> bool {
        self.params.contains("ImageFrame") || self.returns.contains("ImageFrame")
    }

    /// Der Ort als `datei:zeile` — die Form, in der ein Treffer gemeldet wird.
    #[must_use]
    pub fn location(&self) -> String {
        format!("{}:{}", self.path, self.line)
    }
}

/// Alle Definitionen einer Datei.
pub fn definitions_in_file(rel_path: &str, source: &str) -> Vec<Definition> {
    let lines: Vec<&str> = source.lines().collect();
    // Klammer-Tiefe und `impl`-Kontext je physischer Zeile.
    let mut depth = 0i32;
    let mut impls: Vec<(i32, String)> = Vec::new();
    let mut context: Vec<String> = Vec::with_capacity(lines.len());
    for line in &lines {
        while impls.last().is_some_and(|(body, _)| *body > depth) {
            impls.pop();
        }
        let enclosing = impls
            .iter()
            .rev()
            .find(|(body, _)| *body == depth)
            .map(|(_, name)| name.clone())
            .unwrap_or_default();
        context.push(enclosing);
        if let Some(impl_type) = impl_header(line) {
            impls.push((depth + 1, impl_type));
        }
        for ch in line.chars() {
            match ch {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some((name, open)) = definition_at(line) else {
            continue;
        };
        let (params, returns) = signature_of(&lines, index, line, open);
        out.push(Definition {
            path: rel_path.to_string(),
            line: index + 1,
            name,
            impl_type: context[index].clone(),
            params,
            returns,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Der Workspace-Scan.
// ---------------------------------------------------------------------------

/// Der Workspace-Root. `CARGO_MANIFEST_DIR` ist `<root>/crates/<crate>`; der
/// Root ist dessen Grosselternteil. **Nicht** vom relativen CWD ausgehen.
#[must_use]
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("CARGO_MANIFEST_DIR is <workspace-root>/crates/<crate>")
        .to_path_buf()
}

/// Die Anzahl der `.rs`-Dateien unterhalb eines Verzeichnisses (ohne
/// weitere Ausschluesse) — zaehlt, was der `target/`-`.git/`-Ausschluss
/// wegnimmt.
fn count_rs_files(dir: &Path) -> usize {
    let mut count = 0;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            count += count_rs_files(&path);
        } else if path.extension().is_some_and(|ext| ext == SCANNED_EXTENSION) {
            count += 1;
        }
    }
    count
}

/// Alle `.rs`-Dateien unterhalb eines Verzeichnisses (ohne `target/`, `.git/`).
///
/// `excluded` zaehlt die Dateien, die der Verzeichnis-Ausschluss wegnimmt.
fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>, excluded: &mut usize) {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()));
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if EXCLUDED_DIRECTORY_NAMES.contains(&name.as_str()) {
                *excluded += count_rs_files(&path);
                continue;
            }
            collect_rs_files(&path, out, excluded);
        } else if path.extension().is_some_and(|ext| ext == SCANNED_EXTENSION) {
            out.push(path);
        }
    }
}

/// Der Workspace-Scan: die gescannten Dateien und ihre Definitionen.
pub struct Workspace {
    /// Die relativen Pfade der gescannten Dateien (sortiert).
    pub rel_paths: Vec<String>,
    /// Alle Definitionen der gescannten Dateien.
    pub definitions: Vec<Definition>,
    /// Wie viele `.rs`-Dateien der `target/`-`.git/`-Ausschluss weggenommen
    /// hat. Heute **0** — unter `crates/` gibt es kein solches Verzeichnis,
    /// der Ausschluss ist also defensiv, nicht lasttragend. Der Test pinnt
    /// die Zahl, damit ein stiller Bereichswechsel auffaellt.
    pub excluded_by_directory_rule: usize,
}

impl Workspace {
    /// Scannt den ganzen Workspace. Die Datei des Gates selbst
    /// (`GATE_OWN_FILES`) ist **nicht** Teil des Ergebnisses.
    #[must_use]
    pub fn scan(root: &Path) -> Self {
        let crates_dir = root.join("crates");
        let mut crate_dirs: Vec<PathBuf> = std::fs::read_dir(&crates_dir)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", crates_dir.display()))
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.is_dir())
            .collect();
        crate_dirs.sort();
        let mut files = Vec::new();
        let mut excluded = 0usize;
        for crate_dir in &crate_dirs {
            for sub in SCANNED_SUBDIRECTORIES {
                let dir = crate_dir.join(sub);
                if dir.is_dir() {
                    collect_rs_files(&dir, &mut files, &mut excluded);
                }
            }
        }
        files.sort();
        let mut rel_paths = Vec::with_capacity(files.len());
        let mut definitions = Vec::new();
        for path in &files {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(path.as_path())
                .to_string_lossy()
                .replace('\\', "/");
            if GATE_OWN_FILES.contains(&rel.as_str()) {
                continue;
            }
            rel_paths.push(rel.clone());
            let source = std::fs::read_to_string(path)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
            definitions.extend(definitions_in_file(&rel, &source));
        }
        Self {
            rel_paths,
            definitions,
            excluded_by_directory_rule: excluded,
        }
    }
}

/// Der Quelltext ohne Leerraum — damit ein reines Umformatieren den
/// kanonischen Pfad nicht brechen kann.
#[must_use]
pub fn squeeze(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}
