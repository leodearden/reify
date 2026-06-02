//! End-to-end tests for `imported` field sources (task 3576).
//!
//! # Structure
//!
//! Five test groups:
//!
//! 1. **Compile smoke** (`imported_field_compiles_without_errors`) — verifies
//!    that a well-formed imported block produces no `Severity::Error` diagnostics
//!    and populates `CompiledFieldSource::Imported { .. }`. cfg-independent.
//!
//! 2. **Error-path** (`imported_field_with_bad_path_returns_undef_and_emits_field_import_failed`) —
//!    eval with a nonexistent file path; asserts `lambda == Value::Undef` AND a
//!    runtime `DiagnosticCode::FieldImportFailed` error in `EvalResult.diagnostics`.
//!    cfg-independent (the stub `read_vdb_file` returns `Err(FfiNotImplemented)`;
//!    the real body returns `Err(FileReadError)`; both surface as `FieldImportFailed`).
//!
//! 3. **Success e2e** (`imported_field_e2e_vdb_cube_sdf`) — generates a cube
//!    SDF fixture at test-time via `OpenVdbKernel`, compiles an embedded `.ri`
//!    source, evals, and asserts:
//!    - (G2#1) no `Severity::Error` compile diagnostics;
//!    - (G2#2) `lambda` is `Value::SampledField` (not Undef);
//!    - (G2#3) SDF sign probe: inside face → sample < 0, outside → sample > 0;
//!    - (G2#4) exact cross-validation against direct `read_vdb_file` + `sample_at_point`.
//!      Guarded: `cfg(has_openvdb)` real test + `cfg(not(has_openvdb))` skip-stub.
//!
//! 4. **Cache hash records and updates** (`imported_field_cache_hash_records_and_updates_on_content_change`)
//!    — writes non-VDB bytes to a tempfile, evals, and asserts the content-hash is
//!    recorded via `Engine::imported_file_content_hash`; overwrites with different bytes,
//!    evals again, asserts the recorded hash changed. cfg-independent.
//!
//! 5. **Cache hash triggers re-ingest** (`imported_field_cache_hash_triggers_reingest_on_content_change`)
//!    — generates two differently-sized cube SDF fixtures to the same tempfile path and
//!    asserts that the evaluated `SampledField` and recorded content-hash both change after
//!    overwriting. Guarded: `cfg(has_openvdb)` real test + `cfg(not(has_openvdb))` skip-stub.
//!
//! # Why `compile_source_with_stdlib`
//!
//! `parse_and_compile_with_stdlib` panics on any `Severity::Error`. Using
//! `compile_source_with_stdlib` lets tests assert the absence of errors
//! explicitly, which is clearer for test-failure diagnosis.
//!
//! # Embedded source convention
//!
//! Source strings are embedded (not loaded from `examples/`) to keep tests
//! self-contained and avoid contaminating the `all_examples_parse_and_compile_with_stdlib`
//! sweep (`e2e_meta.rs`), which forbids `Severity::Error`.

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DiagnosticCode, FIELD_ENTITY_PREFIX, Severity, ValueCellId};
use reify_ir::{FieldSourceKind, Value};
use reify_test_support::{compile_source_with_stdlib, errors_only};

// ── Test 1: Compile smoke ─────────────────────────────────────────────────────

/// Well-formed imported block compiles without errors and populates
/// `CompiledFieldSource::Imported { path: Some(_), format: Some(_), grid: Some(_) }`.
#[test]
fn imported_field_compiles_without_errors() {
    let source = r#"
field def pressure_map : Point3 -> Scalar {
    source = imported {
        path = "fea_results.vdb"
        format = OpenVDB
        grid = "pressure"
    }
}
"#;
    let compiled = compile_source_with_stdlib(source);

    // No FieldImportedV02 and no Severity::Error.
    let errors = errors_only(&compiled);
    assert!(
        errors
            .iter()
            .all(|d| d.code != Some(DiagnosticCode::FieldImportedV02)),
        "unexpected FieldImportedV02, got: {:?}",
        errors.iter().map(|d| d.code).collect::<Vec<_>>()
    );
    assert!(
        errors.is_empty(),
        "expected no Severity::Error diagnostics, got: {:?}",
        errors
            .iter()
            .map(|d| (d.code, &d.message))
            .collect::<Vec<_>>()
    );

    // Exactly one compiled field with the struct-variant Imported source.
    assert_eq!(
        compiled.fields.len(),
        1,
        "expected exactly 1 compiled field"
    );
    let field = &compiled.fields[0];
    assert!(
        matches!(
            field.source,
            reify_compiler::CompiledFieldSource::Imported { .. }
        ),
        "expected CompiledFieldSource::Imported, got {:?}",
        field.source
    );
}

// ── Test 2: Error-path (cfg-independent) ────────────────────────────────────

/// Eval an imported field whose path points at a nonexistent file; assert:
/// - `Value::Field.lambda == Value::Undef` (graceful failure, not a panic), and
/// - `EvalResult.diagnostics` contains a `Severity::Error` with
///   `DiagnosticCode::FieldImportFailed`.
///
/// cfg-independent: the `cfg(not(has_openvdb))` stub returns
/// `Err(IngestError::FfiNotImplemented)` for any path, so the error path is
/// exercised in both build environments.  The `cfg(has_openvdb)` real body
/// returns `Err(IngestError::FileReadError)` for a nonexistent file — both
/// surface as `FieldImportFailed` at the elaborate_field wire site.
#[test]
fn imported_field_with_bad_path_returns_undef_and_emits_field_import_failed() {
    let source = r#"
field def phantom : Point3 -> Scalar {
    source = imported {
        path = "/nonexistent/path/that/does/not/exist.vdb"
        format = OpenVDB
        grid = "density"
    }
}
"#;
    let compiled = compile_source_with_stdlib(source);

    // Compile must succeed without errors.
    let compile_errors = errors_only(&compiled);
    assert!(
        compile_errors.is_empty(),
        "expected no compile errors for well-formed imported block, got: {:?}",
        compile_errors
            .iter()
            .map(|d| (d.code, &d.message))
            .collect::<Vec<_>>()
    );

    // Eval.
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Field cell must exist and its lambda must be Value::Undef (graceful failure).
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
                "expected FieldSourceKind::Imported on error path, got {:?}",
                source
            );
            assert_eq!(
                **lambda,
                Value::Undef,
                "error path: lambda must be Value::Undef (graceful failure), got {:?}",
                **lambda
            );
        }
        other => panic!("expected Value::Field on error path, got: {:?}", other),
    }

    // EvalResult.diagnostics must contain a FieldImportFailed Severity::Error.
    //
    // NOTE: DiagnosticCode::FieldImportFailed is added in step-8. This
    // reference causes a compile error (RED) until step-8 adds the variant.
    let has_import_failed = result.diagnostics.iter().any(|d| {
        d.code == Some(DiagnosticCode::FieldImportFailed) && d.severity == Severity::Error
    });
    assert!(
        has_import_failed,
        "expected a Severity::Error FieldImportFailed diagnostic in EvalResult.diagnostics; \
         got: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.severity, d.code, &d.message))
            .collect::<Vec<_>>()
    );
}

// ── Test 3: Success e2e (cfg(has_openvdb) real + cfg(not) skip-stub) ─────────

/// Generate a cube SDF fixture at test-time, compile+eval an embedded `.ri`
/// source pointing at it, and assert the full G2 acceptance criteria.
///
/// Fixture: unit cube (half-extent 1.0) realized with voxel_size=0.1,
/// half_width=3.0, written to a `NamedTempFile` under grid name "density".
///
/// G2#1: compile produces no Severity::Error diagnostics.
/// G2#2: `Value::Field.lambda` is `Value::SampledField` (not Undef).
/// G2#3: SDF sign probe — inside near-surface point < 0, outside > 0.
/// G2#4: exact cross-validation of e2e sample vs direct `read_vdb_file` +
///       `sample_at_point` (both call the same code path → bit-identical).
#[cfg(has_openvdb)]
#[test]
fn imported_field_e2e_vdb_cube_sdf() {
    use reify_core::Type;
    use reify_expr::{EvalContext, sampled};
    use reify_ir::SampledField;
    use reify_ir::ValueMap;
    use reify_kernel_openvdb::{OpenVdbKernel, read_vdb_file};

    // ---------------------------------------------------------------------------
    // Fixture: unit cube mesh (8 verts, 12 tris, half-extent = 1.0).
    // Outward normals per face — mesh_to_volume_ffi uses OpenVDB's sign convention:
    // SDF < 0 inside, > 0 outside, ≈0 at surface.
    // ---------------------------------------------------------------------------
    let verts: Vec<[f32; 3]> = vec![
        // Bottom face (z = -1)
        [-1.0, -1.0, -1.0], // 0
        [1.0, -1.0, -1.0],  // 1
        [1.0, 1.0, -1.0],   // 2
        [-1.0, 1.0, -1.0],  // 3
        // Top face (z = +1)
        [-1.0, -1.0, 1.0], // 4
        [1.0, -1.0, 1.0],  // 5
        [1.0, 1.0, 1.0],   // 6
        [-1.0, 1.0, 1.0],  // 7
    ];
    #[rustfmt::skip]
    let tris: Vec<[u32; 3]> = vec![
        // Bottom (z=-1): outward = -Z → CCW from below
        [0, 2, 1], [0, 3, 2],
        // Top (z=+1): outward = +Z → CCW from above
        [4, 5, 6], [4, 6, 7],
        // Front (y=-1): outward = -Y
        [0, 1, 5], [0, 5, 4],
        // Back (y=+1): outward = +Y
        [2, 3, 7], [2, 7, 6],
        // Left (x=-1): outward = -X
        [3, 0, 4], [3, 4, 7],
        // Right (x=+1): outward = +X
        [1, 2, 6], [1, 6, 5],
    ];

    let voxel_size = 0.1_f64;
    let half_width = 3.0_f64;

    let mut kernel = OpenVdbKernel::new();
    let handle = kernel
        .realize_voxel_from_mesh(&verts, &tris, voxel_size, half_width)
        .expect("realize_voxel_from_mesh should succeed for unit cube");

    // Write to a tempfile — keep `tmp` alive so the file is not deleted until
    // the test ends.
    let tmp = tempfile::NamedTempFile::new().expect("tempfile creation should succeed");
    let vdb_path = tmp.path();
    kernel
        .write_vdb_grid(handle, vdb_path, "density")
        .expect("write_vdb_grid should succeed");
    let vdb_path_str = vdb_path
        .to_str()
        .expect("tempfile path should be valid UTF-8");

    // ---------------------------------------------------------------------------
    // G2#1: Compile the embedded .ri source (path interpolated from tempfile).
    // ---------------------------------------------------------------------------
    let codomain_type = Type::Real;
    let source = format!(
        r#"
field def cube_sdf : Point3 -> Scalar {{
    source = imported {{
        path = "{path}"
        format = OpenVDB
        grid = "density"
    }}
}}
"#,
        path = vdb_path_str.replace('\\', "\\\\").replace('"', "\\\"")
    );

    let compiled = compile_source_with_stdlib(&source);
    let compile_errors = errors_only(&compiled);
    assert!(
        compile_errors.is_empty(),
        "G2#1: expected no compile errors; got: {:?}",
        compile_errors
            .iter()
            .map(|d| (d.code, &d.message))
            .collect::<Vec<_>>()
    );

    // ---------------------------------------------------------------------------
    // Eval the module.
    // ---------------------------------------------------------------------------
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Assert no runtime errors.
    let runtime_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        runtime_errors.is_empty(),
        "G2#1: expected no runtime errors; got: {:?}",
        runtime_errors
            .iter()
            .map(|d| (d.code, &d.message))
            .collect::<Vec<_>>()
    );

    // Retrieve the field cell value.
    let field_def = &compiled.fields[0];
    let cell_id = ValueCellId::new(FIELD_ENTITY_PREFIX, &field_def.name);
    let val = result
        .values
        .get(&cell_id)
        .unwrap_or_else(|| panic!("no value for field cell {:?}", cell_id));

    // ---------------------------------------------------------------------------
    // G2#2: lambda is Value::SampledField (not Undef).
    // ---------------------------------------------------------------------------
    let sf_from_e2e = match val {
        Value::Field { source, lambda, .. } => {
            assert_eq!(
                *source,
                FieldSourceKind::Imported,
                "G2#2: expected FieldSourceKind::Imported, got {:?}",
                source
            );
            match lambda.as_ref() {
                Value::SampledField(sf) => sf.clone(),
                other => panic!(
                    "G2#2: expected lambda = Value::SampledField, got {:?}",
                    other
                ),
            }
        }
        other => panic!("expected Value::Field for cube_sdf, got: {:?}", other),
    };

    // ---------------------------------------------------------------------------
    // G2#3: SDF sign probe — near-surface in-band points on the +X face
    // (unit cube face at x = 1.0; narrow band width = half_width * voxel_size = 0.3).
    // inside_probe (x=0.85) is 0.15 inside the surface → SDF < 0.
    // outside_probe (x=1.15) is 0.15 outside the surface → SDF > 0.
    // Both are within the active bbox (bounds ≈ [-1.3, 1.3] on each axis).
    // ---------------------------------------------------------------------------
    let inside_probe = Value::Point(vec![Value::Real(0.85), Value::Real(0.0), Value::Real(0.0)]);
    let outside_probe = Value::Point(vec![Value::Real(1.15), Value::Real(0.0), Value::Real(0.0)]);

    let empty_values = ValueMap::new();
    let sample_ctx = EvalContext::simple(&empty_values);

    let inside_val =
        sampled::sample_at_point(&sf_from_e2e, &inside_probe, &codomain_type, &sample_ctx);
    let outside_val =
        sampled::sample_at_point(&sf_from_e2e, &outside_probe, &codomain_type, &sample_ctx);

    let inside_f64 = match &inside_val {
        Value::Real(v) => *v,
        other => panic!(
            "G2#3: inside probe returned non-Real: {:?} (probe may be out of narrow band)",
            other
        ),
    };
    let outside_f64 = match &outside_val {
        Value::Real(v) => *v,
        other => panic!(
            "G2#3: outside probe returned non-Real: {:?} (probe may be out of narrow band)",
            other
        ),
    };

    assert!(
        inside_f64 < 0.0,
        "G2#3: SDF inside probe should be < 0 (interior); got {}",
        inside_f64
    );
    assert!(
        outside_f64 > 0.0,
        "G2#3: SDF outside probe should be > 0 (exterior); got {}",
        outside_f64
    );

    // ---------------------------------------------------------------------------
    // G2#4: Exact cross-validation — e2e SampledField vs direct read_vdb_file.
    //
    // Both paths call read_vdb_file on the same file → bit-identical results.
    // Tolerance ~1e-9 guards against any accidental FP rounding.
    // ---------------------------------------------------------------------------
    let ref_outcome = read_vdb_file(vdb_path_str, "density", &codomain_type)
        .expect("direct read_vdb_file should succeed for the same fixture file");
    let sf_ref: &SampledField = &ref_outcome.field;

    for (label, probe) in [
        ("inside_probe", &inside_probe),
        ("outside_probe", &outside_probe),
    ] {
        let e2e_sample = sampled::sample_at_point(&sf_from_e2e, probe, &codomain_type, &sample_ctx);
        let ref_sample = sampled::sample_at_point(sf_ref, probe, &codomain_type, &sample_ctx);

        let e2e_f = match &e2e_sample {
            Value::Real(v) => *v,
            other => panic!("G2#4 {label}: e2e sample returned non-Real: {:?}", other),
        };
        let ref_f = match &ref_sample {
            Value::Real(v) => *v,
            other => panic!("G2#4 {label}: ref sample returned non-Real: {:?}", other),
        };

        assert!(
            (e2e_f - ref_f).abs() < 1e-9,
            "G2#4 {label}: e2e sample {e2e_f} differs from direct read_vdb_file sample {ref_f} \
             by {} (expected bit-identical; tolerance 1e-9)",
            (e2e_f - ref_f).abs()
        );
    }
}

/// Skip-stub: `has_openvdb` is not set in this build environment.
#[cfg(not(has_openvdb))]
#[test]
fn imported_field_e2e_vdb_cube_sdf() {
    eprintln!("SKIP: has_openvdb not set — skipping OpenVDB e2e fixture test");
}

// ── Test 4: Cache hash records and updates (cfg-independent) ─────────────────

/// Asserts that `Engine::imported_file_content_hash` returns the content-hash
/// of the most recently eval'd imported file, and that the hash updates when
/// the file's content changes between evals on the same engine.
///
/// Uses arbitrary (non-VDB) bytes — the ingest will fail with `FieldImportFailed`
/// but hash recording is independent of ingest success.  cfg-independent.
#[test]
fn imported_field_cache_hash_records_and_updates_on_content_change() {
    use reify_core::ContentHash;

    // Write bytes X to a tempfile.
    let tmp = tempfile::NamedTempFile::new().expect("tempfile creation");
    let bytes_x: &[u8] = b"not a valid vdb file, just bytes for hashing X";
    std::fs::write(tmp.path(), bytes_x).expect("write bytes_x");

    let path_str = tmp.path().to_str().expect("tempfile path utf-8").to_owned();

    // Build an imported-field source with the tempfile path.
    let source = format!(
        r#"
field def hash_test : Point3 -> Scalar {{
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
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let _ = engine.eval(&compiled);

    // Hash must be recorded after eval.
    let expected_x = ContentHash::of(bytes_x);
    assert_eq!(
        engine.imported_file_content_hash(&path_str),
        Some(expected_x),
        "after first eval: expected hash of bytes_x to be recorded; \
         got {:?}",
        engine.imported_file_content_hash(&path_str)
    );

    // Overwrite the file with different bytes Y.
    let bytes_y: &[u8] = b"different content Y for cache invalidation test -- changed";
    std::fs::write(tmp.path(), bytes_y).expect("overwrite with bytes_y");

    // Eval again on the SAME engine.
    let _ = engine.eval(&compiled);

    let expected_y = ContentHash::of(bytes_y);
    assert_eq!(
        engine.imported_file_content_hash(&path_str),
        Some(expected_y),
        "after second eval: expected hash of bytes_y to be recorded; \
         got {:?}",
        engine.imported_file_content_hash(&path_str)
    );
    assert_ne!(
        expected_x, expected_y,
        "sanity: bytes_x and bytes_y must have different hashes (bytes differ)"
    );
}

// ── Test 5: Cache hash triggers re-ingest on content change (cfg(has_openvdb)) ─────────────

/// Generates two differently-sized cube SDF fixtures, writes them to the same
/// tempfile path in sequence, and asserts that after each eval the engine
/// returns a different `SampledField` and a different recorded content-hash.
///
/// Proves re-ingest on content change: if the `SampledField` were cached and
/// not re-read, bounds would be identical across the two evals.
///
/// Guarded: `cfg(has_openvdb)` real test + `cfg(not(has_openvdb))` skip-stub.
#[cfg(has_openvdb)]
#[test]
fn imported_field_cache_hash_triggers_reingest_on_content_change() {
    use reify_kernel_openvdb::OpenVdbKernel;

    /// Build a cube mesh with the given half-extent.
    fn cube_mesh(half: f32) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
        let h = half;
        let verts = vec![
            [-h, -h, -h],
            [h, -h, -h],
            [h, h, -h],
            [-h, h, -h],
            [-h, -h, h],
            [h, -h, h],
            [h, h, h],
            [-h, h, h],
        ];
        #[rustfmt::skip]
        let tris: Vec<[u32; 3]> = vec![
            // Bottom (z=-1): outward = -Z
            [0, 2, 1], [0, 3, 2],
            // Top (z=+1): outward = +Z
            [4, 5, 6], [4, 6, 7],
            // Front (y=-1): outward = -Y
            [0, 1, 5], [0, 5, 4],
            // Back (y=+1): outward = +Y
            [2, 3, 7], [2, 7, 6],
            // Left (x=-1): outward = -X
            [3, 0, 4], [3, 4, 7],
            // Right (x=+1): outward = +X
            [1, 2, 6], [1, 6, 5],
        ];
        (verts, tris)
    }

    let voxel_size = 0.1_f64;
    let half_width = 3.0_f64;

    // Create a NamedTempFile as the mutable VDB path (kept alive until end of test).
    let tmp = tempfile::NamedTempFile::new().expect("tempfile creation");
    let vdb_path = tmp.path().to_path_buf();
    let path_str = vdb_path.to_str().expect("tempfile path utf-8").to_owned();

    // ── Cube A: half-extent = 0.5 ────────────────────────────────────────────
    let mut kernel_a = OpenVdbKernel::new();
    let (verts_a, tris_a) = cube_mesh(0.5_f32);
    let handle_a = kernel_a
        .realize_voxel_from_mesh(&verts_a, &tris_a, voxel_size, half_width)
        .expect("realize cube A (half=0.5)");
    kernel_a
        .write_vdb_grid(handle_a, &vdb_path, "density")
        .expect("write cube A to tempfile");

    let source = format!(
        r#"
field def cache_test : Point3 -> Scalar {{
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
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result_a = engine.eval(&compiled);

    // Assert no runtime errors from eval A.
    let errors_a: Vec<_> = result_a
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors_a.is_empty(),
        "eval A: unexpected runtime errors: {:?}",
        errors_a
    );

    // Capture SampledField and content-hash from eval A.
    let field_def = &compiled.fields[0];
    let cell_id = ValueCellId::new(FIELD_ENTITY_PREFIX, &field_def.name);
    let sf_a = match result_a.values.get(&cell_id) {
        Some(Value::Field { lambda, .. }) => match lambda.as_ref() {
            Value::SampledField(sf) => sf.clone(),
            other => panic!("eval A: expected lambda = SampledField, got {:?}", other),
        },
        other => panic!("eval A: expected Value::Field, got {:?}", other),
    };
    let hash_a = engine
        .imported_file_content_hash(&path_str)
        .expect("hash must be recorded after eval A");

    // ── Cube B: half-extent = 1.0 ────────────────────────────────────────────
    let mut kernel_b = OpenVdbKernel::new();
    let (verts_b, tris_b) = cube_mesh(1.0_f32);
    let handle_b = kernel_b
        .realize_voxel_from_mesh(&verts_b, &tris_b, voxel_size, half_width)
        .expect("realize cube B (half=1.0)");
    // Overwrite the SAME path with the larger cube.
    kernel_b
        .write_vdb_grid(handle_b, &vdb_path, "density")
        .expect("write cube B to same tempfile path");

    // Eval again on the SAME engine, SAME compiled module, SAME path.
    let result_b = engine.eval(&compiled);

    // Assert no runtime errors from eval B.
    let errors_b: Vec<_> = result_b
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors_b.is_empty(),
        "eval B: unexpected runtime errors: {:?}",
        errors_b
    );

    // Capture SampledField and content-hash from eval B.
    let sf_b = match result_b.values.get(&cell_id) {
        Some(Value::Field { lambda, .. }) => match lambda.as_ref() {
            Value::SampledField(sf) => sf.clone(),
            other => panic!("eval B: expected lambda = SampledField, got {:?}", other),
        },
        other => panic!("eval B: expected Value::Field, got {:?}", other),
    };
    let hash_b = engine
        .imported_file_content_hash(&path_str)
        .expect("hash must be recorded after eval B");

    // Content-hash must change: cubes A and B have different byte content.
    assert_ne!(
        hash_a, hash_b,
        "content-hash must change after overwriting with a different cube (A half=0.5 → B half=1.0)"
    );

    // SampledField must change: cube B is larger so bounds_min/max differ.
    // Comparing bounds_min is sufficient (cube A has smaller active bbox than cube B).
    assert_ne!(
        sf_a.bounds_min, sf_b.bounds_min,
        "SampledField bounds_min must differ: cube A (half=0.5) vs cube B (half=1.0) \
         have different active bboxes; got sf_a.bounds_min={:?}, sf_b.bounds_min={:?}",
        sf_a.bounds_min, sf_b.bounds_min
    );
}

/// Skip-stub: `has_openvdb` is not set in this build environment.
#[cfg(not(has_openvdb))]
#[test]
fn imported_field_cache_hash_triggers_reingest_on_content_change() {
    eprintln!("SKIP: has_openvdb not set — skipping cache re-ingest test");
}
