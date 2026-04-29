//! Field declaration tests.
//!
//! Tests for `field def name : DomainType -> CodomainType { source = kind { ... } }` declarations.

use reify_syntax::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("field_test"));
    (module.declarations, module.errors)
}

// ── Step 1: analytical field ─────────────────────────────────────────

#[test]
fn parse_analytical_field() {
    let (decls, errors) =
        parse_decls("field def temp : Point3 -> Scalar { source = analytical { |p| p } }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let field = match &decls[0] {
        Declaration::Field(f) => f,
        other => panic!("expected Field, got {:?}", other),
    };

    assert_eq!(field.name, "temp");
    assert!(!field.is_pub);
    assert_eq!(field.domain_type.to_string(), "Point3");
    assert_eq!(field.codomain_type.to_string(), "Scalar");

    match &field.source {
        FieldSource::Analytical { expr } => {
            // The expression should be a lambda: |p| p
            match &expr.kind {
                ExprKind::Lambda { params, .. } => {
                    assert_eq!(params.len(), 1);
                    assert_eq!(params[0].name, "p");
                }
                other => panic!("expected Lambda in analytical source, got {:?}", other),
            }
        }
        other => panic!("expected Analytical source, got {:?}", other),
    }
}

// ── Step 3: sampled field ────────────────────────────────────────────

#[test]
fn parse_sampled_field() {
    let (decls, errors) = parse_decls(
        "field def pressure : Point3 -> Scalar { source = sampled { resolution = 100  interpolation = linear } }",
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let field = match &decls[0] {
        Declaration::Field(f) => f,
        other => panic!("expected Field, got {:?}", other),
    };

    assert_eq!(field.name, "pressure");
    assert_eq!(field.domain_type.to_string(), "Point3");
    assert_eq!(field.codomain_type.to_string(), "Scalar");

    match &field.source {
        FieldSource::Sampled { config } => {
            assert_eq!(config.len(), 2);
            assert_eq!(config[0].0, "resolution");
            assert_eq!(config[1].0, "interpolation");
        }
        other => panic!("expected Sampled source, got {:?}", other),
    }
}

// ── Step 5: composed field ──────────────────────────────────────────

#[test]
fn parse_composed_field() {
    let (decls, errors) = parse_decls(
        "field def combined : Point3 -> Vector3 { source = composed { |f, g| |p| f(g(p)) } }",
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let field = match &decls[0] {
        Declaration::Field(f) => f,
        other => panic!("expected Field, got {:?}", other),
    };

    assert_eq!(field.name, "combined");
    assert_eq!(field.domain_type.to_string(), "Point3");
    assert_eq!(field.codomain_type.to_string(), "Vector3");

    match &field.source {
        FieldSource::Composed { expr } => {
            // The expression should be a lambda: |f, g| |p| f(g(p))
            match &expr.kind {
                ExprKind::Lambda { params, .. } => {
                    assert_eq!(params.len(), 2);
                    assert_eq!(params[0].name, "f");
                    assert_eq!(params[1].name, "g");
                }
                other => panic!("expected Lambda in composed source, got {:?}", other),
            }
        }
        other => panic!("expected Composed source, got {:?}", other),
    }
}

// ── Step 2665-1: imported field full key=value block ────────────────

#[test]
fn parse_imported_field_with_full_block() {
    let (decls, errors) = parse_decls(
        r#"field def fea : Point3 -> Scalar { source = imported { path = "fea.vdb" format = OpenVDB grid = "vonMises" } }"#,
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let field = match &decls[0] {
        Declaration::Field(f) => f,
        other => panic!("expected Field, got {:?}", other),
    };
    assert_eq!(field.name, "fea");

    match &field.source {
        FieldSource::Imported { path, format, grid } => {
            assert_eq!(path.as_deref(), Some("fea.vdb"));
            assert_eq!(format.as_deref(), Some("OpenVDB"));
            assert_eq!(grid.as_deref(), Some("vonMises"));
        }
        other => panic!("expected Imported source, got {:?}", other),
    }
}

// ── Step 2665-3: imported field keys in any order ───────────────────

#[test]
fn parse_imported_field_keys_any_order() {
    // Keys reordered: grid → path → format (vs the canonical path → format → grid)
    let (decls, errors) = parse_decls(
        r#"field def fea : Point3 -> Scalar { source = imported { grid = "vonMises" path = "fea.vdb" format = OpenVDB } }"#,
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let field = match &decls[0] {
        Declaration::Field(f) => f,
        other => panic!("expected Field, got {:?}", other),
    };

    match &field.source {
        FieldSource::Imported { path, format, grid } => {
            assert_eq!(path.as_deref(), Some("fea.vdb"), "path mismatch");
            assert_eq!(format.as_deref(), Some("OpenVDB"), "format mismatch");
            assert_eq!(grid.as_deref(), Some("vonMises"), "grid mismatch");
        }
        other => panic!("expected Imported source, got {:?}", other),
    }
}

// ── Step 2665-5: imported field with partial keys ────────────────────

#[test]
fn parse_imported_field_partial_keys() {
    // Only path provided; format and grid are absent.
    let (decls, errors) = parse_decls(
        r#"field def fea : Point3 -> Scalar { source = imported { path = "fea.vdb" } }"#,
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let field = match &decls[0] {
        Declaration::Field(f) => f,
        other => panic!("expected Field, got {:?}", other),
    };

    match &field.source {
        FieldSource::Imported { path, format, grid } => {
            assert_eq!(path.as_deref(), Some("fea.vdb"));
            assert_eq!(*format, None, "format should be None when absent");
            assert_eq!(*grid, None, "grid should be None when absent");
        }
        other => panic!("expected Imported source, got {:?}", other),
    }
}

// ── Step 2665-7: format identifier captured verbatim ─────────────────

#[test]
fn parse_imported_field_format_identifier_captured_verbatim() {
    // Deliberately uses HDF5 (not yet supported in v0.2) to pin that the parser
    // captures identifiers verbatim and does NOT validate against an allowlist.
    let (decls, errors) = parse_decls(
        r#"field def fea : Point3 -> Scalar { source = imported { path = "x.vdb" format = HDF5 grid = "g" } }"#,
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let field = match &decls[0] {
        Declaration::Field(f) => f,
        other => panic!("expected Field, got {:?}", other),
    };

    match &field.source {
        FieldSource::Imported { format, .. } => {
            assert_eq!(format.as_deref(), Some("HDF5"), "format should be captured verbatim");
        }
        other => panic!("expected Imported source, got {:?}", other),
    }
}

// ── Step 2665-9: extra keys don't break known-key parsing ───────────

#[test]
fn parse_imported_field_extra_keys_do_not_break_known_keys() {
    // Includes `units` and `interpolation` keys that v0.2 explicitly does NOT support.
    // The test name reflects what the body can actually assert: extra keys don't break
    // parsing of the three known keys. Unknown keys are silently dropped at parse time
    // with no extras field; the compiler can only observe None for absent/dropped keys.
    let (decls, errors) = parse_decls(
        r#"field def fea : Point3 -> Scalar { source = imported { path = "x.vdb" format = OpenVDB grid = "g" units = MPa interpolation = trilinear } }"#,
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let field = match &decls[0] {
        Declaration::Field(f) => f,
        other => panic!("expected Field, got {:?}", other),
    };

    match &field.source {
        FieldSource::Imported { path, format, grid } => {
            assert_eq!(path.as_deref(), Some("x.vdb"), "path should be populated");
            assert_eq!(format.as_deref(), Some("OpenVDB"), "format should be populated");
            assert_eq!(grid.as_deref(), Some("g"), "grid should be populated");
        }
        other => panic!("expected Imported source, got {:?}", other),
    }
}

// ── Step 2665-amend: duplicate key handling (last-write-wins) ────────

/// Verify that duplicate keys in an imported block are handled with last-write-wins semantics.
/// The parser makes no attempt to detect or diagnose duplicates — that is intentional behaviour,
/// pinned here so any future change (e.g., promoting duplicates to a parse error) is explicit.
#[test]
fn parse_imported_field_duplicate_keys_last_write_wins() {
    let (decls, errors) = parse_decls(
        r#"field def fea : Point3 -> Scalar { source = imported { path = "first.vdb" path = "second.vdb" format = OpenVDB grid = "g" } }"#,
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let field = match &decls[0] {
        Declaration::Field(f) => f,
        other => panic!("expected Field, got {:?}", other),
    };

    match &field.source {
        FieldSource::Imported { path, .. } => {
            // Last-write-wins: "second.vdb" overwrites "first.vdb".
            assert_eq!(path.as_deref(), Some("second.vdb"), "last path value should win");
        }
        other => panic!("expected Imported source, got {:?}", other),
    }
}

// ── Step 7: pub field ───────────────────────────────────────────────

#[test]
fn parse_pub_field() {
    let (decls, errors) =
        parse_decls("pub field def temp : Point3 -> Scalar { source = analytical { |p| p } }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let field = match &decls[0] {
        Declaration::Field(f) => f,
        other => panic!("expected Field, got {:?}", other),
    };

    assert_eq!(field.name, "temp");
    assert!(field.is_pub);
}

// ── Step 2689-1: imported field wrong-type values silently dropped ──

/// Verify that type-mismatched values for known keys are silently dropped to `None`.
/// `path` expects a string literal but receives an identifier → None.
/// `format` expects an identifier but receives a string literal → None.
/// `grid` expects a string literal but receives an identifier → None.
/// This pins the contract documented in `lib.rs:706-728` and the `_ =>` arms in
/// `ts_parser.rs:779-784`: wrong-ExprKind values are silently ignored.
#[test]
fn parse_imported_field_wrong_type_values_dropped() {
    let (decls, errors) = parse_decls(
        r#"field def fea : Point3 -> Scalar { source = imported { path = OpenVDB format = "openvdb" grid = bare } }"#,
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let field = match &decls[0] {
        Declaration::Field(f) => f,
        other => panic!("expected Field, got {:?}", other),
    };

    match &field.source {
        FieldSource::Imported { path, format, grid } => {
            assert_eq!(*path, None, "path with non-string value should be dropped");
            assert_eq!(*format, None, "format with non-ident value should be dropped");
            assert_eq!(*grid, None, "grid with non-string value should be dropped");
        }
        other => panic!("expected Imported source, got {:?}", other),
    }
}
