//! LRPAR-G15-STACK-15: source-level image-stack membership schema tests
//! (creation/validation, deterministic id, JSON roundtrip without a
//! `schema_version` bump, same-folder invariant).

use super::*;

fn member(name: &str) -> String {
    name.to_string()
}

#[test]
fn stack_new_sorts_members_and_requires_cover_in_members() {
    let stack = StackMembership::new(
        "stack-abc",
        "b.arw",
        vec![member("c.arw"), member("a.arw"), member("b.arw")],
    )
    .unwrap();
    assert_eq!(stack.version, STACK_SCHEMA_VERSION);
    assert_eq!(
        stack.members,
        vec![member("a.arw"), member("b.arw"), member("c.arw")]
    );
    assert!(stack.contains("a.arw"));
    assert!(stack.is_cover("b.arw"));
    assert!(!stack.collapsed);

    // Cover missing from members fails loudly.
    assert!(StackMembership::new(
        "stack-abc",
        "zz.arw",
        vec![member("a.arw"), member("b.arw")]
    )
    .is_err());
    // Duplicates fail loudly (no silent deduplication).
    assert!(
        StackMembership::new("stack-abc", "a.arw", vec![member("a.arw"), member("a.arw")]).is_err()
    );
    // Fewer than two members fails loudly.
    assert!(StackMembership::new("stack-abc", "a.arw", vec![member("a.arw")]).is_err());
}

#[test]
fn stack_rejects_paths_and_bad_ids_loudly() {
    // Same-folder invariant: separators and `..`/absolute paths are rejected.
    for bad in ["sub/a.arw", "sub\\a.arw", "../a.arw", "..", "."] {
        assert!(
            StackMembership::new("stack-abc", bad, vec![member(bad), member("b.arw")]).is_err(),
            "member/cover `{bad}` must be rejected"
        );
    }
    // Absolute path is a separator-bearing name and therefore rejected.
    assert!(StackMembership::new(
        "/abs/a.arw",
        "/abs/a.arw",
        vec![member("/abs/a.arw"), member("b.arw")]
    )
    .is_err());
    // Bad ids: empty, whitespace-padded and control characters.
    assert!(StackMembership::new("", "a.arw", vec![member("a.arw"), member("b.arw")]).is_err());
    assert!(StackMembership::new(" sp ", "a.arw", vec![member("a.arw"), member("b.arw")]).is_err());
    assert!(
        StackMembership::new("s\u{7}", "a.arw", vec![member("a.arw"), member("b.arw")]).is_err()
    );
}

#[test]
fn stack_id_is_deterministic_and_order_independent() {
    let a = StackMembership::stack_id_for_members(&[member("b.arw"), member("a.arw")]);
    let b = StackMembership::stack_id_for_members(&[member("a.arw"), member("b.arw")]);
    let c = StackMembership::stack_id_for_members(&[member("a.arw"), member("c.arw")]);
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert!(a.starts_with("stack-"));
    validate_stack_id(&a).unwrap();
}

#[test]
fn stack_invalid_version_is_rejected() {
    let mut stack =
        StackMembership::new("stack-abc", "a.arw", vec![member("a.arw"), member("b.arw")]).unwrap();
    stack.version = STACK_SCHEMA_VERSION + 1;
    assert!(stack.validate().is_err());
}

/// N-3: the schema length bounds are enforced loudly (id, member name, count).
#[test]
fn stack_length_limits_are_rejected() {
    // Stack id beyond MAX_STACK_ID_CHARS.
    let long_id = "s".repeat(MAX_STACK_ID_CHARS + 1);
    assert!(
        StackMembership::new(long_id, "a.arw", vec![member("a.arw"), member("b.arw")]).is_err()
    );

    // Member/cover name beyond MAX_STACK_MEMBER_CHARS.
    let long_name = format!("{}.arw", "x".repeat(MAX_STACK_MEMBER_CHARS));
    assert!(StackMembership::new(
        "stack-abc",
        long_name.clone(),
        vec![long_name, member("b.arw")]
    )
    .is_err());

    // More members than MAX_STACK_MEMBERS.
    let mut too_many: Vec<String> = (0..=MAX_STACK_MEMBERS)
        .map(|index| format!("m{index:04}.arw"))
        .collect();
    too_many.sort();
    let cover = too_many[0].clone();
    assert!(StackMembership::new("stack-abc", cover, too_many).is_err());
}

/// End-to-end: the `stack` section persists through a file save/load with an
/// unchanged `schema_version`; an absent stack stays absent (legacy-stable).
#[test]
fn stack_roundtrips_through_file_without_schema_bump() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("a.arw.lumina.json");
    let mut document = SidecarDocument::new(source(), "pipeline-1");
    assert_eq!(document.schema_version, SCHEMA_VERSION);
    let stack = StackMembership::new(
        StackMembership::stack_id_for_members(&[member("a.arw"), member("b.arw")]),
        "a.arw",
        vec![member("a.arw"), member("b.arw")],
    )
    .unwrap()
    .with_collapsed(true);
    document.stack = Some(stack.clone());
    save_sidecar(&path, &document).unwrap();
    let loaded = load_sidecar(&path).unwrap();
    assert_eq!(loaded.schema_version, SCHEMA_VERSION);
    assert_eq!(loaded.stack.as_ref(), Some(&stack));

    // Legacy document without a stack serializes back without the key.
    let legacy = SidecarDocument::new(source(), "pipeline-1");
    let json = legacy.to_json().unwrap();
    assert!(!json.contains("\"stack\""));
}

/// An unsupported (higher) `schema_version` is still rejected loudly by the
/// loader — the additive stack section never weakens that rule.
#[test]
fn unsupported_schema_version_still_rejected_with_stack_feature() {
    assert!(
        SidecarDocument::from_json(r#"{"format":"lumina-sidecar","schema_version":99}"#).is_err()
    );
}
