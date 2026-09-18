use super::*;

// ----- F-103-N5: `paths_resolve_equal` non-destructive export guard -----

#[test]
fn paths_resolve_equal_same_file_is_true() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    std::fs::write(&source, b"data").unwrap();
    // Identical path resolves equal to itself.
    assert!(paths_resolve_equal(&source, &source).unwrap());
    // A second reference to the very same file also resolves equal.
    let same = directory.path().join("photo.png");
    assert!(paths_resolve_equal(&source, &same).unwrap());
}

#[test]
fn paths_resolve_equal_different_files_is_false() {
    let directory = tempfile::tempdir().unwrap();
    let a = directory.path().join("a.png");
    let b = directory.path().join("b.png");
    std::fs::write(&a, b"data-a").unwrap();
    std::fs::write(&b, b"data-b").unwrap();
    assert!(!paths_resolve_equal(&a, &b).unwrap());
}

#[test]
fn paths_resolve_equal_missing_output_same_name_is_false() {
    // A not-yet-existing export target that shares the source's file name but
    // lives in a *different* directory resolves to a different location, so the
    // non-destructive guard must NOT reject it (it is a legitimate export).
    //
    // This genuinely exercises the `!output.exists()` (missing) branch of
    // `paths_resolve_equal`: `output` does not exist, so it is resolved against
    // its parent directory and compared to the source's canonical path.
    //
    // Note: a truly *missing* output whose resolved path equals the source is
    // impossible on a normal filesystem — if the resolved path named an existing
    // file (the source) the `output` would `exists()` and take the other branch.
    // The overwrite-rejection for the source's own name is therefore covered by
    // `paths_resolve_equal_same_file_is_true` (the `exists()` branch), which is
    // the realistic non-destructive contract: the original still occupies that
    // path when an export targets it.
    let directory = tempfile::tempdir().unwrap();
    let source_dir = directory.path().join("src");
    std::fs::create_dir_all(&source_dir).unwrap();
    let source = source_dir.join("photo.png");
    std::fs::write(&source, b"data").unwrap();
    // Output shares the file name but sits in the parent folder and does not
    // exist yet — the missing branch is taken.
    let missing = directory.path().join("photo.png");
    assert!(!missing.exists());
    assert!(!paths_resolve_equal(&source, &missing).unwrap());
}

#[test]
fn paths_resolve_equal_missing_output_other_name_is_false() {
    // A not-yet-existing output with a different name in the same folder is
    // a legitimate export target.
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    std::fs::write(&source, b"data").unwrap();
    let missing = directory.path().join("photo_export.png");
    assert!(!paths_resolve_equal(&source, &missing).unwrap());
}

#[test]
fn paths_resolve_equal_different_directories_is_false() {
    let parent = tempfile::tempdir().unwrap();
    let d1 = parent.path().join("d1");
    let d2 = parent.path().join("d2");
    std::fs::create_dir_all(&d1).unwrap();
    std::fs::create_dir_all(&d2).unwrap();
    let source = d1.join("photo.png");
    let other = d2.join("photo.png");
    std::fs::write(&source, b"data").unwrap();
    // Even with the same file name, different directories never resolve equal.
    assert!(!paths_resolve_equal(&source, &other).unwrap());
}
