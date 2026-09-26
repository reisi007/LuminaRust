//! Named, loud failures of the Lensfun system-database lookup (LENSFUN-DB-33).
//!
//! Split out of `db_path.rs` (file-size ratchet, User-Vorgabe 2026-09-17).
//!
//! Every state a candidate directory can be in has its own variant and its own
//! wording, because the operator's next action differs per case: a missing
//! directory means "install the package", a permission error means "fix the
//! permissions", a full directory of rejected XML means "the files are
//! corrupt", and an empty one means "the package shipped no profiles". Folding
//! them into a single "not found" is what makes a database lookup silently
//! degrade into a byte-identical no-op.
//!
//! SOLL: `feature/platform/capability-matrix.md`, section
//! „Lensfun-Profil-Datenbank (plattformabhängige Auflösung, LENSFUN-DB-33)“,
//! subsection „Kein stiller Ersatz, harte Fehler“.

use std::fmt;
use std::path::PathBuf;

use crate::db_path::{Source, DB_DIR_ENV};

/// Why a candidate directory does not hold a usable database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissReason {
    /// The directory (or its `version_1` subdirectory) does not exist.
    Absent,
    /// The path exists but is not a directory (e.g. a relative override).
    NotADirectory,
    /// The directory exists but could not be read (permissions, I/O error).
    Unreadable,
    /// `version_1/` exists but holds no `*.xml` file.
    NoXmlFiles,
    /// `*.xml` files were found, but liblensfun rejected **every** one of them.
    AllFilesRejected,
}

impl MissReason {
    /// A stable, machine-friendly label (used in test output and diagnostics).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "Absent",
            Self::NotADirectory => "NotADirectory",
            Self::Unreadable => "Unreadable",
            Self::NoXmlFiles => "NoXmlFiles",
            Self::AllFilesRejected => "AllFilesRejected",
        }
    }
}

impl fmt::Display for MissReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absent => f.write_str("Verzeichnis nicht vorhanden"),
            Self::NotADirectory => f.write_str("kein absolutes, lesbares Verzeichnis"),
            Self::Unreadable => f.write_str("Verzeichnis nicht lesbar"),
            Self::NoXmlFiles => f.write_str("version_1/ ohne XML-Datei"),
            Self::AllFilesRejected => {
                f.write_str("XML-Dateien vorhanden, aber alle von liblensfun abgelehnt")
            }
        }
    }
}

/// One rejected candidate, kept so the error can name every probed location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeMiss {
    /// Candidate directory that was probed.
    pub dir: PathBuf,
    /// Resolution step that proposed it.
    pub source: Source,
    /// Why it was not usable.
    pub reason: MissReason,
}

/// The named, loud failure of the system-database resolution.
///
/// Never a silent fallback: the `Display` lists every probed location with its
/// source and reason, plus how to fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemDbError {
    /// `LUMINA_LENSFUN_DB` is set but unusable. Hard failure by design — an
    /// explicit operator intent is never silently replaced by another source.
    OverrideUnusable {
        /// The value taken from the environment.
        dir: PathBuf,
        /// Why it is unusable.
        reason: MissReason,
    },
    /// No candidate in the documented order held a database.
    NotFound {
        /// Every rejected candidate, in probe order.
        misses: Vec<ProbeMiss>,
    },
}

impl fmt::Display for SystemDbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OverrideUnusable { dir, reason } => {
                writeln!(
                    f,
                    "lumina-lensfun: LENSFUN-DB-33 SystemDbError/OverrideUnusable: \
                     die Umgebungs-Override {DB_DIR_ENV}={} zeigt auf {} [{}].",
                    dir.display(),
                    reason,
                    MissReason::as_str(*reason),
                )?;
                f.write_str(
                    "  Eine explizit gesetzte Override wird bewusst NICHT stillschweigend \
                     durch eine andere Quelle ersetzt.\n\
                     Abhilfe: auf ein absolutes Verzeichnis mit version_1/*.xml zeigen \
                     lassen (z. B. /usr/share/lensfun oder /opt/homebrew/share/lensfun) \
                     oder die Variable entfernen.",
                )
            }
            Self::NotFound { misses } => {
                writeln!(
                    f,
                    "lumina-lensfun: LENSFUN-DB-33 SystemDbError/NotFound: \
                     keine Lensfun-Profil-Datenbank gefunden ({} Kandidaten geprüft).",
                    misses.len()
                )?;
                for miss in misses {
                    writeln!(
                        f,
                        "  - {} [{}]: {}",
                        miss.dir.display(),
                        miss.source.as_str(),
                        miss.reason
                    )?;
                }
                f.write_str(
                    "  Abhilfe: liblensfun samt Profil-Datenbank installieren \
                     (apt-get install liblensfun-dev / brew install lensfun) \
                     oder die Umgebungs-Override LUMINA_LENSFUN_DB explizit setzen.\n\
                     Es wird KEIN stiller Ersatz verwendet: ohne Datenbank ist die \
                     Linsenkorrektur nicht verfügbar, nicht byte-identisch geraten.",
                )
            }
        }
    }
}

impl std::error::Error for SystemDbError {}
