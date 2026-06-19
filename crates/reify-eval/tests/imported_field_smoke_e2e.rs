//! Consolidated end-to-end smoke test and diagnostic coverage for imported
//! field sources (task 2669).
//!
//! # Structure
//!
//! Three test groups (in addition to the step-3 provenance-wiring test):
//!
//! 1. **Provenance wiring** (`imported_field_provenance_wiring_cfg_independent`) —
//!    cfg-independent integration test. Writes readable (non-VDB) bytes to a
//!    NamedTempFile, compiles+evals an embedded imported `.ri`, and asserts that
//!    `Engine::imported_field_provenance(path)` returns `Some` with the correct
//!    `path`, `format`, `content_hash`, `ingestion_timestamp_secs > 0`, and
//!    `declared_tolerance_si == None`. Exercises provenance wiring independent of
//!    the OpenVDB kernel (both stub and real FFI paths hash the file before
//!    attempting to parse it, so `imported_hash` is `Some` even on a parse failure).
//!
//! 2. **Consolidated end-to-end smoke** (`imported_field_smoke_e2e_grammar_to_provenance`) —
//!    `cfg(has_openvdb)` real test + `cfg(not(has_openvdb))` skip-stub.
//!    Generates a cube-SDF VDB fixture, compiles+evals, and asserts the full
//!    grammar→ingestion→cache→provenance path: no errors, `Value::SampledField`
//!    lambda, SDF sign probe, cache-hash recorded, provenance recorded.
//!
//! 3. **Malformed-file diagnostic** (`imported_field_malformed_file_emits_field_import_failed`) —
//!    cfg-independent. Writes readable garbage bytes, compiles+evals, asserts
//!    `lambda == Value::Undef` and `DiagnosticCode::FieldImportFailed`.
//!
//! 4. **Grid-not-in-file diagnostic** (`imported_field_grid_not_in_file_emits_field_import_failed`) —
//!    `cfg(has_openvdb)` real test + `cfg(not(has_openvdb))` skip-stub.
//!    Generates a valid cube-SDF VDB under grid "density", but requests grid
//!    "missing_grid"; asserts `Value::Undef` and `DiagnosticCode::FieldImportFailed`.
//!
//! # Reuse
//!
//! The cube-SDF fixture recipe and `cfg(has_openvdb)` / skip-stub pattern are
//! reused from `imported_field_e2e.rs`. The step-3 test is cfg-independent and
//! uses non-VDB bytes to avoid any dependency on the OpenVDB kernel.

use reify_constraints::SimpleConstraintChecker;
use reify_core::{ContentHash, DiagnosticCode, FIELD_ENTITY_PREFIX, Severity, ValueCellId};
use reify_ir::{FieldSourceKind, Value};
use reify_test_support::{compile_source_with_stdlib, errors_only};

// ── Shared fixture helpers ────────────────────────────────────────────────────

/// Generate a unit-cube SDF VDB fixture and write it to a temporary file.
///
/// Creates a unit-cube mesh (half-extent = 1.0, outward normals), voxelises it
/// via `OpenVdbKernel::realize_voxel_from_mesh` (voxel_size = 0.1,
/// half_width = 3.0), and writes the resulting SDF grid to a
/// [`tempfile::NamedTempFile`] under the grid name `"density"`.
///
/// Returns `(tempfile, path_str)`.  The caller **must** keep the returned
/// [`tempfile::NamedTempFile`] alive for the duration of the test — dropping it
/// would delete the file and make the path invalid.
///
/// # Cross-file duplication note
///
/// The same fixture recipe exists in `imported_field_e2e.rs` (task 3576,
/// `imported_field_e2e_vdb_cube_sdf`).  Deduplicating across test files would
/// require moving this helper into `reify-test-support` (outside the scope of
/// task 2669); this amendment eliminates the within-file duplication between
/// tests 2 and 4.
#[cfg(has_openvdb)]
fn make_cube_sdf_vdb_fixture() -> (tempfile::NamedTempFile, String) {
    use reify_kernel_openvdb::OpenVdbKernel;

    let verts: Vec<[f32; 3]> = vec![
        [-1.0, -1.0, -1.0],
        [1.0, -1.0, -1.0],
        [1.0, 1.0, -1.0],
        [-1.0, 1.0, -1.0],
        [-1.0, -1.0, 1.0],
        [1.0, -1.0, 1.0],
        [1.0, 1.0, 1.0],
        [-1.0, 1.0, 1.0],
    ];
    #[rustfmt::skip]
    let tris: Vec<[u32; 3]> = vec![
        [0, 2, 1], [0, 3, 2],
        [4, 5, 6], [4, 6, 7],
        [0, 1, 5], [0, 5, 4],
        [2, 3, 7], [2, 7, 6],
        [3, 0, 4], [3, 4, 7],
        [1, 2, 6], [1, 6, 5],
    ];

    let mut kernel = OpenVdbKernel::new();
    let handle = kernel
        .realize_voxel_from_mesh(&verts, &tris, 0.1, 3.0)
        .expect("realize_voxel_from_mesh should succeed for unit cube");

    let tmp = tempfile::NamedTempFile::new().expect("tempfile creation");
    kernel
        .write_vdb_grid(handle, tmp.path(), "density")
        .expect("write_vdb_grid should succeed");

    let path_str = tmp
        .path()
        .to_str()
        .expect("tempfile path utf-8")
        .to_owned();

    (tmp, path_str)
}

// ── Test 1: Provenance wiring (cfg-independent) ──────────────────────────────

/// Provenance is recorded for an Imported field source whenever the source
/// file is readable (content-hash available), regardless of VDB-parse success.
///
/// Writes non-VDB bytes to a NamedTempFile, compiles+evals an embedded imported
/// `.ri` source, and asserts that `Engine::imported_field_provenance(path)`
/// returns `Some` with:
/// - `path` == the tempfile path string,
/// - `format` == "OpenVDB",
/// - `content_hash` == `ContentHash::of(bytes)`,
/// - `ingestion_timestamp_secs > 0`,
/// - `declared_tolerance_si` == `None`.
///
/// cfg-independent: both the stub (`FfiNotImplemented`) and real (`FileReadError`)
/// FFI paths hash the file before attempting to parse it, so `imported_hash` is
/// `Some` even when the VDB parse fails.  Provenance recording is gated on
/// `imported_hash` being `Some`, not on parse success.
///
/// Fails to compile until step-4 adds:
/// - `Engine::imported_field_provenance` (engine_admin.rs), and
/// - the provenance-recording call in the Engine::eval field loop (engine_eval.rs).
#[test]
fn imported_field_provenance_wiring_cfg_independent() {
    // Write readable (non-VDB) bytes to a tempfile.
    let tmp = tempfile::NamedTempFile::new().expect("tempfile creation");
    let bytes: &[u8] = b"not a valid vdb file, just readable bytes for provenance test";
    std::fs::write(tmp.path(), bytes).expect("write bytes to tempfile");
    let path_str = tmp.path().to_str().expect("tempfile path utf-8").to_owned();

    // Embed the path in a .ri source.
    let source = format!(
        r#"
field def prov_test : Point3 -> Length {{
    source = imported {{
        path = "{path}"
        format = OpenVDB
        grid = "density"
    }}
}}
"#,
        path = path_str.replace('\\', "\\\\").replace('"', "\\\"")
    );

    // Compile.
    let compiled = compile_source_with_stdlib(&source);
    let compile_errors = errors_only(&compiled);
    assert!(
        compile_errors.is_empty(),
        "expected no compile errors for well-formed imported block, got: {:?}",
        compile_errors
            .iter()
            .map(|d| (d.code, &d.message))
            .collect::<Vec<_>>()
    );

    // Eval on a fresh Engine.
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let _ = engine.eval(&compiled);

    // Provenance must be recorded.
    let prov = engine
        .imported_field_provenance(&path_str)
        .unwrap_or_else(|| {
            panic!(
                "imported_field_provenance({:?}) must be Some after eval; got None",
                path_str
            )
        });

    assert_eq!(
        prov.path, path_str,
        "provenance.path must match the eval'd import path"
    );
    assert_eq!(
        prov.format, "OpenVDB",
        "provenance.format must be \"OpenVDB\" (default when format=OpenVDB in source)"
    );
    assert_eq!(
        prov.content_hash,
        ContentHash::of(bytes),
        "provenance.content_hash must equal ContentHash::of(file bytes)"
    );
    assert!(
        prov.ingestion_timestamp_secs > 0,
        "provenance.ingestion_timestamp_secs must be > 0 (Unix epoch seconds from SystemTime::now())"
    );
    assert_eq!(
        prov.declared_tolerance_si, None,
        "provenance.declared_tolerance_si must be None for a field-def imported source (no tolerance param)"
    );
}

// ── Test 2: Consolidated e2e smoke (cfg(has_openvdb) real + skip-stub) ───────

/// End-to-end smoke: grammar → ingestion → cache → provenance.
///
/// Generates a unit-cube SDF fixture, compiles+evals an embedded `.ri` source
/// pointing at it, and asserts the full chain:
/// - no `Severity::Error` diagnostics;
/// - `Value::Field.lambda` is `Value::SampledField`;
/// - SDF sign probe: inside x=0.85 → sample < 0;
/// - cache: `Engine::imported_file_content_hash(path)` == `ContentHash::of(file bytes)`;
/// - provenance: `Engine::imported_field_provenance(path)` `Some` with correct
///   `format`, `content_hash`, `declared_tolerance_si == None`, `ingestion_timestamp_secs > 0`.
#[cfg(has_openvdb)]
#[test]
fn imported_field_smoke_e2e_grammar_to_provenance() {
    use reify_expr::{EvalContext, sampled};
    use reify_ir::ValueMap;
    use reify_core::Type;

    // Fixture: unit-cube SDF VDB (shared via `make_cube_sdf_vdb_fixture`; same
    // recipe used by the grid-not-in-file test below).
    //
    // NOTE (redundant-coverage): the SampledField / SDF-sign-probe / cache-hash
    // assertions below also appear in `imported_field_e2e.rs`'s
    // `imported_field_e2e_vdb_cube_sdf` (task 3576).  A full dedup — adding the
    // four provenance assertions to that test and removing this one — would
    // require modifying `imported_field_e2e.rs` (outside task 2669 scope).  The
    // task plan explicitly calls for these assertions in this file, so they are
    // retained; the cross-file overlap is documented here for future cleanup.
    let (tmp, path_str) = make_cube_sdf_vdb_fixture();

    // Read file bytes for hash comparison.
    let file_bytes = std::fs::read(tmp.path()).expect("read file bytes for hash");
    let expected_hash = ContentHash::of(&file_bytes);

    let source = format!(
        r#"
field def smoke_sdf : Point3 -> Length {{
    source = imported {{
        path = "{path}"
        format = OpenVDB
        grid = "density"
    }}
}}
"#,
        path = path_str.replace('\\', "\\\\").replace('"', "\\\"")
    );

    let compiled = compile_source_with_stdlib(&source);
    let compile_errors = errors_only(&compiled);
    assert!(
        compile_errors.is_empty(),
        "expected no compile errors; got: {:?}",
        compile_errors
            .iter()
            .map(|d| (d.code, &d.message))
            .collect::<Vec<_>>()
    );

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // No runtime errors.
    let runtime_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        runtime_errors.is_empty(),
        "expected no runtime errors; got: {:?}",
        runtime_errors
            .iter()
            .map(|d| (d.code, &d.message))
            .collect::<Vec<_>>()
    );

    // Field lambda must be SampledField.
    let field_def = &compiled.fields[0];
    let cell_id = ValueCellId::new(FIELD_ENTITY_PREFIX, &field_def.name);
    let val = result
        .values
        .get(&cell_id)
        .unwrap_or_else(|| panic!("no value for field cell {:?}", cell_id));

    let sf = match val {
        Value::Field { source, lambda, .. } => {
            assert_eq!(
                *source,
                FieldSourceKind::Imported,
                "expected FieldSourceKind::Imported, got {:?}",
                source
            );
            match lambda.as_ref() {
                Value::SampledField(sf) => sf.clone(),
                other => panic!("expected lambda = Value::SampledField, got {:?}", other),
            }
        }
        other => panic!("expected Value::Field, got {:?}", other),
    };

    // SDF sign probe: inside x=0.85 < 0.
    let codomain_type = Type::dimensionless_scalar();
    let inside_probe = Value::Point(vec![Value::Real(0.85), Value::Real(0.0), Value::Real(0.0)]);
    let empty_values = ValueMap::new();
    let ctx = EvalContext::simple(&empty_values);
    let inside_val = sampled::sample_at_point(&sf, &inside_probe, &codomain_type, &ctx);
    let inside_f = match &inside_val {
        Value::Real(v) => *v,
        other => panic!("inside probe returned non-Real: {:?}", other),
    };
    assert!(
        inside_f < 0.0,
        "SDF inside probe (x=0.85) must be < 0; got {}",
        inside_f
    );

    // Cache: content-hash is recorded.
    assert_eq!(
        engine.imported_file_content_hash(&path_str),
        Some(expected_hash),
        "imported_file_content_hash must equal ContentHash::of(file bytes)"
    );

    // Provenance: recorded and correct.
    let prov = engine
        .imported_field_provenance(&path_str)
        .unwrap_or_else(|| panic!("imported_field_provenance must be Some after e2e eval"));
    assert_eq!(prov.format, "OpenVDB", "provenance.format must be OpenVDB");
    assert_eq!(
        prov.content_hash, expected_hash,
        "provenance.content_hash must equal the file content hash"
    );
    assert_eq!(
        prov.declared_tolerance_si, None,
        "provenance.declared_tolerance_si must be None for field-def imported source"
    );
    assert!(
        prov.ingestion_timestamp_secs > 0,
        "provenance.ingestion_timestamp_secs must be > 0"
    );
}

/// Skip-stub: `has_openvdb` is not set in this build environment.
#[cfg(not(has_openvdb))]
#[test]
fn imported_field_smoke_e2e_grammar_to_provenance() {
    eprintln!("SKIP: has_openvdb not set — skipping grammar→provenance e2e smoke test");
}

// ── Test 3: Malformed-file diagnostic coverage (cfg-independent) ─────────────

/// Asserts that a readable-but-invalid file produces `lambda == Value::Undef`
/// and a `DiagnosticCode::FieldImportFailed` runtime error.
///
/// Characterizes the existing uniform error path on the "file readable but
/// content invalid" trigger.  cfg-independent: the stub returns
/// `FfiNotImplemented`, the real body returns `FileReadError` for garbage
/// bytes; both surface as `FieldImportFailed`.
#[test]
fn imported_field_malformed_file_emits_field_import_failed() {
    // Write garbage bytes (readable but not a valid VDB file).
    let tmp = tempfile::NamedTempFile::new().expect("tempfile creation");
    let bytes: &[u8] = b"GARBAGE_NOT_VDB\x00\x01\x02\x03";
    std::fs::write(tmp.path(), bytes).expect("write garbage bytes");

    let path_str = tmp.path().to_str().expect("tempfile path utf-8").to_owned();

    let source = format!(
        r#"
field def malformed : Point3 -> Length {{
    source = imported {{
        path = "{path}"
        format = OpenVDB
        grid = "density"
    }}
}}
"#,
        path = path_str.replace('\\', "\\\\").replace('"', "\\\"")
    );

    let compiled = compile_source_with_stdlib(&source);
    let compile_errors = errors_only(&compiled);
    assert!(
        compile_errors.is_empty(),
        "expected no compile errors, got: {:?}",
        compile_errors
            .iter()
            .map(|d| (d.code, &d.message))
            .collect::<Vec<_>>()
    );

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Field lambda must be Undef (graceful failure, not a panic).
    let field_def = &compiled.fields[0];
    let cell_id = ValueCellId::new(FIELD_ENTITY_PREFIX, &field_def.name);
    let val = result
        .values
        .get(&cell_id)
        .unwrap_or_else(|| panic!("no value for field cell {:?}", cell_id));

    match val {
        Value::Field { source, lambda, .. } => {
            assert_eq!(
                *source,
                FieldSourceKind::Imported,
                "expected FieldSourceKind::Imported on malformed-file path, got {:?}",
                source
            );
            assert_eq!(
                **lambda,
                Value::Undef,
                "malformed-file: lambda must be Value::Undef (graceful failure), got {:?}",
                **lambda
            );
        }
        other => panic!(
            "expected Value::Field on malformed-file path, got: {:?}",
            other
        ),
    }

    // EvalResult.diagnostics must contain a FieldImportFailed Severity::Error.
    let has_import_failed = result.diagnostics.iter().any(|d| {
        d.code == Some(DiagnosticCode::FieldImportFailed) && d.severity == Severity::Error
    });
    assert!(
        has_import_failed,
        "expected a Severity::Error FieldImportFailed diagnostic; got: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.severity, d.code, &d.message))
            .collect::<Vec<_>>()
    );
}

// ── Test 4: Grid-not-in-file diagnostic coverage (cfg(has_openvdb)) ──────────

/// Asserts that a valid VDB file with the wrong grid name produces
/// `lambda == Value::Undef` and a `DiagnosticCode::FieldImportFailed` error.
///
/// Generates a unit-cube SDF fixture under grid "density", but requests
/// grid "missing_grid" in the embedded `.ri` source.  The VDB read succeeds
/// in opening the file but fails to find the requested grid name, surfacing
/// `IngestError::FileReadError` → `FieldImportFailed`.
#[cfg(has_openvdb)]
#[test]
fn imported_field_grid_not_in_file_emits_field_import_failed() {
    // Fixture: unit-cube SDF VDB under grid "density" (shared via
    // `make_cube_sdf_vdb_fixture`; same recipe used by the e2e smoke test above).
    let (_tmp, path_str) = make_cube_sdf_vdb_fixture();

    // Request a grid name that does NOT exist in the file.
    let source = format!(
        r#"
field def wrong_grid : Point3 -> Length {{
    source = imported {{
        path = "{path}"
        format = OpenVDB
        grid = "missing_grid"
    }}
}}
"#,
        path = path_str.replace('\\', "\\\\").replace('"', "\\\"")
    );

    let compiled = compile_source_with_stdlib(&source);
    let compile_errors = errors_only(&compiled);
    assert!(
        compile_errors.is_empty(),
        "expected no compile errors, got: {:?}",
        compile_errors
            .iter()
            .map(|d| (d.code, &d.message))
            .collect::<Vec<_>>()
    );

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Field lambda must be Undef.
    let field_def = &compiled.fields[0];
    let cell_id = ValueCellId::new(FIELD_ENTITY_PREFIX, &field_def.name);
    let val = result
        .values
        .get(&cell_id)
        .unwrap_or_else(|| panic!("no value for field cell {:?}", cell_id));

    match val {
        Value::Field { lambda, .. } => {
            assert_eq!(
                **lambda,
                Value::Undef,
                "grid-not-in-file: lambda must be Value::Undef; got {:?}",
                **lambda
            );
        }
        other => panic!(
            "expected Value::Field on grid-not-in-file path, got: {:?}",
            other
        ),
    }

    // Must emit FieldImportFailed.
    let has_import_failed = result.diagnostics.iter().any(|d| {
        d.code == Some(DiagnosticCode::FieldImportFailed) && d.severity == Severity::Error
    });
    assert!(
        has_import_failed,
        "expected a Severity::Error FieldImportFailed diagnostic for grid-not-in-file; got: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.severity, d.code, &d.message))
            .collect::<Vec<_>>()
    );
}

/// Skip-stub: `has_openvdb` is not set in this build environment.
#[cfg(not(has_openvdb))]
#[test]
fn imported_field_grid_not_in_file_emits_field_import_failed() {
    eprintln!("SKIP: has_openvdb not set — skipping grid-not-in-file diagnostic test");
}
