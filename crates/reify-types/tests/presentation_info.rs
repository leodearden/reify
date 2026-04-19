//! Integration tests for DiagnosticInfo and SourceLocationInfo presentation types.

#[test]
fn diagnostic_info_is_constructible_from_reify_types() {
    let d = reify_types::DiagnosticInfo {
        file_path: "a.ri".into(),
        line: 1,
        column: 0,
        end_line: 1,
        end_column: 5,
        severity: "error".into(),
        message: "bad".into(),
        code: None,
    };
    assert_eq!(d.file_path, "a.ri");
    assert_eq!(d.line, 1);
    assert_eq!(d.column, 0);
    assert_eq!(d.end_line, 1);
    assert_eq!(d.end_column, 5);
    assert_eq!(d.severity, "error");
    assert_eq!(d.message, "bad");
    assert_eq!(d.code, None);
}

#[test]
fn source_location_info_is_constructible_from_reify_types() {
    let loc = reify_types::SourceLocationInfo {
        file_path: "bracket.ri".into(),
        line: 3,
        column: 4,
        end_line: 3,
        end_column: 30,
    };
    assert_eq!(loc.file_path, "bracket.ri");
    assert_eq!(loc.line, 3);
    assert_eq!(loc.column, 4);
    assert_eq!(loc.end_line, 3);
    assert_eq!(loc.end_column, 30);
}

#[cfg(feature = "serde")]
#[test]
fn diagnostic_info_serde_roundtrip() {
    let d = reify_types::DiagnosticInfo {
        file_path: "a.ri".into(),
        line: 5,
        column: 2,
        end_line: 5,
        end_column: 20,
        severity: "warning".into(),
        message: "unused variable".into(),
        code: Some("W001".into()),
    };
    let v = serde_json::to_value(&d).unwrap();
    assert_eq!(v["file_path"], "a.ri");
    assert_eq!(v["line"], 5);
    assert_eq!(v["column"], 2);
    assert_eq!(v["end_line"], 5);
    assert_eq!(v["end_column"], 20);
    assert_eq!(v["severity"], "warning");
    assert_eq!(v["message"], "unused variable");
    assert_eq!(v["code"], "W001");
    // Verify round-trip deserialization
    let d2: reify_types::DiagnosticInfo = serde_json::from_value(v).unwrap();
    assert_eq!(d, d2);
}

#[cfg(feature = "serde")]
#[test]
fn severity_serde_roundtrip() {
    // Serialize each variant and confirm PascalCase wire strings.
    assert_eq!(
        serde_json::to_value(reify_types::Severity::Error).unwrap(),
        serde_json::Value::String("Error".into())
    );
    assert_eq!(
        serde_json::to_value(reify_types::Severity::Warning).unwrap(),
        serde_json::Value::String("Warning".into())
    );
    assert_eq!(
        serde_json::to_value(reify_types::Severity::Info).unwrap(),
        serde_json::Value::String("Info".into())
    );

    // Each PascalCase string must also deserialize back to the correct variant.
    let err: reify_types::Severity =
        serde_json::from_value(serde_json::Value::String("Error".into())).unwrap();
    assert_eq!(err, reify_types::Severity::Error);

    let warn: reify_types::Severity =
        serde_json::from_value(serde_json::Value::String("Warning".into())).unwrap();
    assert_eq!(warn, reify_types::Severity::Warning);

    let info: reify_types::Severity =
        serde_json::from_value(serde_json::Value::String("Info".into())).unwrap();
    assert_eq!(info, reify_types::Severity::Info);
}

#[cfg(feature = "serde")]
#[test]
fn severity_serde_unknown_string_is_error() {
    // The PascalCase contract must be fail-closed: lowercase or unknown strings
    // must not successfully deserialize.
    let result: Result<reify_types::Severity, _> =
        serde_json::from_value(serde_json::Value::String("error".into()));
    assert!(
        result.is_err(),
        "lowercase \"error\" must not deserialize as Severity (got {:?})",
        result
    );

    let result2: Result<reify_types::Severity, _> =
        serde_json::from_value(serde_json::Value::String("unknown".into()));
    assert!(
        result2.is_err(),
        "\"unknown\" must not deserialize as Severity (got {:?})",
        result2
    );
}

#[cfg(feature = "serde")]
#[test]
fn source_location_info_serde_has_file_path_key() {
    let loc = reify_types::SourceLocationInfo {
        file_path: "bracket.ri".into(),
        line: 3,
        column: 4,
        end_line: 3,
        end_column: 30,
    };
    let v = serde_json::to_value(&loc).unwrap();
    assert_eq!(v["file_path"], "bracket.ri");
    assert!(v.get("file").is_none(), "should not serialize as 'file'");
    // Verify all expected keys exist
    assert_eq!(v["line"], 3);
    assert_eq!(v["column"], 4);
    assert_eq!(v["end_line"], 3);
    assert_eq!(v["end_column"], 30);
}
