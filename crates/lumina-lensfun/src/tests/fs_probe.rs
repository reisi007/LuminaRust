//! LENSFUN-DB-33: the **real filesystem** side of the load plan.
//!
//! Split out of `tests/db_layers.rs` (file-size ratchet, User-Vorgabe
//! 2026-09-17). That file pins the *algorithm* hermetically with the in-memory
//! probe; this one runs the same claims against the real `FsProbe` on a real
//! temp directory, because "the fixture is right" and "the filesystem probe is
//! right" are two different failure modes:
//!
//! - `timestamp.txt` semantics on disk: a non-empty `version_1` without the
//!   file scores `0`, a missing directory and an **empty** directory both score
//!   `-1` (review finding F1-BRUTK);
//! - every `MissReason` has a distinct message **and** a distinct label;
//! - `MissReason::Unreadable` is reachable (review finding MITTEL-4) — a
//!   mode-`000` directory, with an honest root detection instead of a silent
//!   pass.

use crate::db_path::*;
use crate::db_timestamp::DatabaseTimestamp;

#[test]
fn fs_probe_reports_sorted_xml_files_and_real_timestamps() {
    let dir = std::env::temp_dir().join(format!(
        "lumina-lensfun-probe-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let schema = dir.join(SCHEMA_SUBDIR);
    std::fs::create_dir_all(&schema).expect("create probe fixture");
    for name in ["b.xml", "a.xml", "notes.txt"] {
        std::fs::write(schema.join(name), "<lensdatabase/>").expect("write probe file");
    }
    // Flat directory semantics: `schema_files` is `version_1`, `dir_files` is not.
    let flat = FsProbe
        .dir_files(&dir)
        .expect("flat dir")
        .expect("dir exists");
    assert!(flat.is_empty(), "no XML directly in {dir:?}");
    let files = FsProbe
        .schema_files(&dir)
        .expect("probe ok")
        .expect("schema dir exists");
    let names: Vec<String> = files
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["a.xml", "b.xml"]);

    // `timestamp.txt` semantics on the real filesystem: present and non-empty
    // but without the file ⇒ 0, not "newest mtime".
    let stamp = FsProbe.database_timestamp(&schema);
    assert_eq!(
        stamp,
        DatabaseTimestamp::NoTimestampFile,
        "a non-empty version_1 without timestamp.txt scores 0 upstream"
    );
    std::fs::write(schema.join("timestamp.txt"), "1700000000\n").expect("write stamp");
    assert_eq!(
        FsProbe.database_timestamp(&schema),
        DatabaseTimestamp::At(1_700_000_000)
    );
    // A missing directory and an EMPTY directory both score -1.
    assert_eq!(
        FsProbe.database_timestamp(&dir.join("absent")),
        DatabaseTimestamp::DirectoryAbsent
    );
    let empty = dir.join("empty");
    std::fs::create_dir_all(&empty).expect("create empty dir");
    assert_eq!(
        FsProbe.database_timestamp(&empty),
        DatabaseTimestamp::DirectoryAbsent,
        "an empty directory scores -1 upstream, exactly like a missing one"
    );
    assert!(FsProbe
        .dir_files(&dir.join("absent"))
        .expect("probe ok")
        .is_none());
    // A file where a directory is expected is not silently "absent".
    let not_a_dir = dir.join("plain.txt");
    std::fs::write(&not_a_dir, "x").expect("write plain file");
    assert_eq!(
        xml_files_in(&not_a_dir).unwrap_err(),
        MissReason::NotADirectory,
        "a non-directory must not be reported as merely empty/absent"
    );
    assert_eq!(
        FsProbe.database_timestamp(&not_a_dir),
        DatabaseTimestamp::DirectoryAbsent,
        "`g_dir_open` on a regular file fails upstream, so the value is -1"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A readable directory that simply holds no XML is `NoXmlFiles`, and every
/// miss reason has a distinct, non-lying message and label.
#[test]
fn every_miss_reason_has_a_distinct_message() {
    let reasons = [
        MissReason::Absent,
        MissReason::NotADirectory,
        MissReason::Unreadable,
        MissReason::NoXmlFiles,
        MissReason::AllFilesRejected,
    ];
    let texts: Vec<String> = reasons.iter().map(|r| r.to_string()).collect();
    let mut sorted = texts.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), texts.len(), "duplicate wording in {texts:?}");
    let labels: Vec<&str> = reasons.iter().map(|r| MissReason::as_str(*r)).collect();
    assert_eq!(labels.len(), 5, "{labels:?}");
    assert!(texts[4].contains("abgelehnt"), "{texts:?}");
}

/// **MITTEL-4: `Unreadable` must be reachable.** A mode-`000` directory exists,
/// so reporting it as `Absent` would send the operator to the wrong fix. Root
/// ignores the permission bits, so the case is *detected* and reported as such
/// rather than passing silently.
#[test]
fn an_unreadable_directory_is_reported_as_unreadable_not_absent() {
    if running_as_root() {
        eprintln!(
            "MITTEL-4: skipping the permission-denied case — this process is root, \
             so a mode-000 directory is still readable and the assertion would be vacuous"
        );
        return;
    }
    let root = std::env::temp_dir().join(format!(
        "lumina-lensfun-unreadable-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let schema = root.join(SCHEMA_SUBDIR);
    std::fs::create_dir_all(&schema).expect("create fixture");
    std::fs::write(schema.join("a.xml"), "<lensdatabase/>").expect("write fixture");

    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o000)).expect("chmod 000");
    // Confirm the fixture really is unreadable before asserting on the probe.
    if std::fs::read_dir(&root).is_ok() {
        let _ = std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755));
        let _ = std::fs::remove_dir_all(&root);
        eprintln!(
            "MITTEL-4: skipping the permission-denied case — {} is still readable \
             despite mode 000 (ACLs?); asserting it would be vacuous",
            root.display()
        );
        return;
    }
    assert_eq!(
        xml_files_in(&root).unwrap_err(),
        MissReason::Unreadable,
        "an existing but unreadable directory must not be reported as absent"
    );
    assert_eq!(
        FsProbe.schema_files(&root).unwrap_err(),
        MissReason::Unreadable
    );
    // …and the resolution names it as such.
    let err = resolve_with(Some(root.as_os_str()), None, None, None, &FsProbe)
        .expect_err("an unreadable override is a hard, named error");
    assert_eq!(
        err,
        SystemDbError::OverrideUnusable {
            dir: root.clone(),
            reason: MissReason::Unreadable,
        }
    );
    assert!(err.to_string().contains("nicht lesbar"), "{err}");

    // …and in the `NotFound` list of a machine where it is merely a candidate.
    let err = resolve_with(None, Some(root.as_os_str()), None, None, &FsProbe)
        .expect_err("still a hard error");
    let SystemDbError::NotFound { misses } = &err else {
        panic!("expected NotFound, got {err:?}");
    };
    assert!(
        misses.iter().any(|m| m.reason == MissReason::Unreadable),
        "{err}"
    );

    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).expect("chmod back");
    let _ = std::fs::remove_dir_all(&root);
}

/// Root detection without a `libc` dependency: the owner of `/` is the
/// effective uid of every process on the machine.
fn running_as_root() -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/").is_ok_and(|meta| meta.uid() == 0)
}
