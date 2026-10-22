use lumina_mcp::tools::edit::validate_adjustments;
use lumina_mcp::util::detect_format;
use std::collections::BTreeMap;
use std::fs;

#[test]
fn detect_format_accepts_supported_and_rejects_others() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("a.png");
    fs::write(&png, b"x").unwrap();
    assert_eq!(detect_format(&png).unwrap(), "png");

    let raw = dir.path().join("b.arw");
    fs::write(&raw, b"x").unwrap();
    assert_eq!(detect_format(&raw).unwrap(), "arw");

    let bad = dir.path().join("c.txt");
    fs::write(&bad, b"x").unwrap();
    assert!(matches!(
        detect_format(&bad),
        Err(lumina_mcp::McpError::UnsupportedFormat(_))
    ));
}

#[test]
fn validate_adjustments_rejects_out_of_range() {
    let mut map = BTreeMap::new();
    map.insert("exposure".to_string(), 100.0);
    assert!(matches!(
        validate_adjustments(&map),
        Err(lumina_mcp::McpError::InvalidAdjustment { .. })
    ));

    let mut valid = BTreeMap::new();
    valid.insert("contrast".to_string(), -0.5);
    valid.insert("wb_temperature".to_string(), 5500.0);
    assert!(validate_adjustments(&valid).is_ok());

    let mut unknown = BTreeMap::new();
    unknown.insert("not_a_key".to_string(), 0.0);
    assert!(matches!(
        validate_adjustments(&unknown),
        Err(lumina_mcp::McpError::InvalidAdjustment { .. })
    ));
}
