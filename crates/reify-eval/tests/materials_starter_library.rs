//! SIR-β-mat (task 3542) — wave-2 material field-read round-trip tests.
//!
//! Verifies that `Aluminium_6061_T6`, `Titanium_Ti6Al4V`, and `ABS_Plastic`
//! are reachable via the SIR-α lowering path and that their engineering
//! defaults round-trip through member-access expressions as `Value::Scalar`.
//!
//! Also contains the `reify eval` CLI golden test for
//! `examples/materials_starter_library.ri` (the wave-2 user-observable signal).
//!
//! PRD reference: docs/prds/v0_3/structural-analysis-fea.md §8 SIR-β-mat,
//! GR-019 (cluster C-16 Material starter library).

#![allow(clippy::mutable_key_type)]

use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};
use reify_types::{PersistentMap, Value, ValueCellId};

/// `PersistentMap<String, Value>::get` is keyed by `&String`; this lets tests
/// index `StructureInstance.fields` with a string literal.
fn field<'a>(m: &'a PersistentMap<String, Value>, k: &str) -> Option<&'a Value> {
    m.get(&k.to_string())
}

// ── Per-material unit tests ───────────────────────────────────────────────────

/// `Aluminium_6061_T6` round-trip: the constructor evaluates to a
/// `Value::StructureInstance` carrying the three primary engineering defaults,
/// and member-access expressions resolve to `Value::Scalar` (non-Undef).
///
/// Expected defaults (from `materials_fea.ri`):
///   youngs_modulus = 68.9 GPa,  poisson_ratio = 0.33,  density = 2700 kg/m³.
#[test]
fn aluminium_6061_t6_field_read_round_trip() {
    const SOURCE: &str = r#"
structure def AluminiumFixture {
    let mat = Aluminium_6061_T6()
    let e   = self.mat.youngs_modulus
    let nu  = self.mat.poisson_ratio
    let rho = self.mat.density
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // (a) mat cell must be a StructureInstance with the correct type_name.
    let mat = result
        .values
        .get(&ValueCellId::new("AluminiumFixture", "mat"))
        .unwrap_or_else(|| panic!("AluminiumFixture.mat cell missing from eval result"));

    match mat {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "Aluminium_6061_T6",
                "expected type_name=\"Aluminium_6061_T6\", got {:?}",
                data.type_name
            );
            // (b) all four fields must be present and non-Undef.
            for field_name in &["youngs_modulus", "poisson_ratio", "density", "yield_stress"] {
                assert!(
                    field(&data.fields, field_name).is_some(),
                    "Aluminium_6061_T6 instance must carry field `{field_name}`; \
                     present fields: {:?}",
                    data.fields.iter().map(|(k, _)| k).collect::<Vec<_>>()
                );
                assert!(
                    !matches!(field(&data.fields, field_name), Some(Value::Undef)),
                    "field `{field_name}` must not be Undef in Aluminium_6061_T6 defaults"
                );
            }
        }
        other => panic!(
            "expected Value::StructureInstance for AluminiumFixture.mat, got {other:?}"
        ),
    }

    // (c) member-access cells must resolve to non-Undef scalars.
    let e = result
        .values
        .get(&ValueCellId::new("AluminiumFixture", "e"))
        .unwrap_or_else(|| panic!("AluminiumFixture.e cell missing"));
    assert!(
        matches!(e, Value::Scalar { .. }),
        "self.mat.youngs_modulus must resolve to a Scalar (68.9 GPa), got {e:?}"
    );

    let nu = result
        .values
        .get(&ValueCellId::new("AluminiumFixture", "nu"))
        .unwrap_or_else(|| panic!("AluminiumFixture.nu cell missing"));
    assert!(
        matches!(nu, Value::Real(_)),
        "self.mat.poisson_ratio must resolve to a Real (0.33), got {nu:?}"
    );

    let rho = result
        .values
        .get(&ValueCellId::new("AluminiumFixture", "rho"))
        .unwrap_or_else(|| panic!("AluminiumFixture.rho cell missing"));
    assert!(
        matches!(rho, Value::Scalar { .. }),
        "self.mat.density must resolve to a Scalar (2700 kg/m³), got {rho:?}"
    );
}

/// `Titanium_Ti6Al4V` round-trip: the constructor evaluates to a
/// `Value::StructureInstance` carrying the three primary engineering defaults,
/// and member-access expressions resolve to `Value::Scalar` (non-Undef).
///
/// Expected defaults (from `materials_fea.ri`):
///   youngs_modulus = 113.8 GPa,  poisson_ratio = 0.342,  density = 4430 kg/m³.
#[test]
fn titanium_ti6al4v_field_read_round_trip() {
    const SOURCE: &str = r#"
structure def TitaniumFixture {
    let mat = Titanium_Ti6Al4V()
    let e   = self.mat.youngs_modulus
    let nu  = self.mat.poisson_ratio
    let rho = self.mat.density
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // (a) mat cell must be a StructureInstance with the correct type_name.
    let mat = result
        .values
        .get(&ValueCellId::new("TitaniumFixture", "mat"))
        .unwrap_or_else(|| panic!("TitaniumFixture.mat cell missing from eval result"));

    match mat {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "Titanium_Ti6Al4V",
                "expected type_name=\"Titanium_Ti6Al4V\", got {:?}",
                data.type_name
            );
            // (b) all four fields must be present and non-Undef.
            for field_name in &["youngs_modulus", "poisson_ratio", "density", "yield_stress"] {
                assert!(
                    field(&data.fields, field_name).is_some(),
                    "Titanium_Ti6Al4V instance must carry field `{field_name}`; \
                     present fields: {:?}",
                    data.fields.iter().map(|(k, _)| k).collect::<Vec<_>>()
                );
                assert!(
                    !matches!(field(&data.fields, field_name), Some(Value::Undef)),
                    "field `{field_name}` must not be Undef in Titanium_Ti6Al4V defaults"
                );
            }
        }
        other => panic!(
            "expected Value::StructureInstance for TitaniumFixture.mat, got {other:?}"
        ),
    }

    // (c) member-access cells must resolve to non-Undef scalars.
    let e = result
        .values
        .get(&ValueCellId::new("TitaniumFixture", "e"))
        .unwrap_or_else(|| panic!("TitaniumFixture.e cell missing"));
    assert!(
        matches!(e, Value::Scalar { .. }),
        "self.mat.youngs_modulus must resolve to a Scalar (113.8 GPa), got {e:?}"
    );

    let nu = result
        .values
        .get(&ValueCellId::new("TitaniumFixture", "nu"))
        .unwrap_or_else(|| panic!("TitaniumFixture.nu cell missing"));
    assert!(
        matches!(nu, Value::Real(_)),
        "self.mat.poisson_ratio must resolve to a Real (0.342), got {nu:?}"
    );

    let rho = result
        .values
        .get(&ValueCellId::new("TitaniumFixture", "rho"))
        .unwrap_or_else(|| panic!("TitaniumFixture.rho cell missing"));
    assert!(
        matches!(rho, Value::Scalar { .. }),
        "self.mat.density must resolve to a Scalar (4430 kg/m³), got {rho:?}"
    );
}

/// `ABS_Plastic` round-trip: the constructor evaluates to a
/// `Value::StructureInstance` carrying the three primary engineering defaults,
/// and member-access expressions resolve to `Value::Scalar` (non-Undef).
///
/// Expected defaults (from `materials_fea.ri`):
///   youngs_modulus = 2.3 GPa,  poisson_ratio = 0.35,  density = 1050 kg/m³.
#[test]
fn abs_plastic_field_read_round_trip() {
    const SOURCE: &str = r#"
structure def AbsFixture {
    let mat = ABS_Plastic()
    let e   = self.mat.youngs_modulus
    let nu  = self.mat.poisson_ratio
    let rho = self.mat.density
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // (a) mat cell must be a StructureInstance with the correct type_name.
    let mat = result
        .values
        .get(&ValueCellId::new("AbsFixture", "mat"))
        .unwrap_or_else(|| panic!("AbsFixture.mat cell missing from eval result"));

    match mat {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "ABS_Plastic",
                "expected type_name=\"ABS_Plastic\", got {:?}",
                data.type_name
            );
            // (b) all four fields must be present and non-Undef.
            for field_name in &["youngs_modulus", "poisson_ratio", "density", "yield_stress"] {
                assert!(
                    field(&data.fields, field_name).is_some(),
                    "ABS_Plastic instance must carry field `{field_name}`; \
                     present fields: {:?}",
                    data.fields.iter().map(|(k, _)| k).collect::<Vec<_>>()
                );
                assert!(
                    !matches!(field(&data.fields, field_name), Some(Value::Undef)),
                    "field `{field_name}` must not be Undef in ABS_Plastic defaults"
                );
            }
        }
        other => panic!("expected Value::StructureInstance for AbsFixture.mat, got {other:?}"),
    }

    // (c) member-access cells must resolve to non-Undef scalars.
    let e = result
        .values
        .get(&ValueCellId::new("AbsFixture", "e"))
        .unwrap_or_else(|| panic!("AbsFixture.e cell missing"));
    assert!(
        matches!(e, Value::Scalar { .. }),
        "self.mat.youngs_modulus must resolve to a Scalar (2.3 GPa), got {e:?}"
    );

    let nu = result
        .values
        .get(&ValueCellId::new("AbsFixture", "nu"))
        .unwrap_or_else(|| panic!("AbsFixture.nu cell missing"));
    assert!(
        matches!(nu, Value::Real(_)),
        "self.mat.poisson_ratio must resolve to a Real (0.35), got {nu:?}"
    );

    let rho = result
        .values
        .get(&ValueCellId::new("AbsFixture", "rho"))
        .unwrap_or_else(|| panic!("AbsFixture.rho cell missing"));
    assert!(
        matches!(rho, Value::Scalar { .. }),
        "self.mat.density must resolve to a Scalar (1050 kg/m³), got {rho:?}"
    );
}

// ── CLI golden test (step-3) ──────────────────────────────────────────────────

/// `reify eval examples/materials_starter_library.ri` must print inspectable
/// structure-shaped values (not `undef`) for all three wave-2 materials, and
/// its stdout must match the committed golden. Regenerate with
/// `REIFY_REGENERATE_GOLDEN=1`.
///
/// `CARGO_BIN_EXE_reify` is only injected for `reify-cli`'s own integration
/// tests, so this cross-crate test drives the binary through `cargo run`
/// (mirroring the established pattern from `structure_instance_e2e.rs`).
#[test]
fn cli_reify_eval_prints_inspectable_material_values() {
    let manifest = env!("CARGO_MANIFEST_DIR"); // .../crates/reify-eval
    let workspace_root = std::path::Path::new(manifest)
        .ancestors()
        .nth(2)
        .expect("workspace root is two levels above crates/reify-eval")
        .to_path_buf();
    let example = workspace_root.join("examples/materials_starter_library.ri");
    let golden =
        std::path::Path::new(manifest).join("tests/golden/materials_starter_library.txt");

    let output = std::process::Command::new(env!("CARGO"))
        .current_dir(&workspace_root)
        .args(["run", "-q", "-p", "reify-cli", "--bin", "reify", "--", "eval"])
        .arg(&example)
        .output()
        .expect("failed to spawn `cargo run -p reify-cli -- eval`");

    assert!(
        output.status.success(),
        "`reify eval` exited non-zero.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout must be valid UTF-8");

    if std::env::var("REIFY_REGENERATE_GOLDEN").is_ok() {
        std::fs::write(&golden, &stdout).expect("failed to write golden file");
        return;
    }

    let expected = std::fs::read_to_string(&golden).expect(
        "golden crates/reify-eval/tests/golden/materials_starter_library.txt missing; \
         run once with REIFY_REGENERATE_GOLDEN=1",
    );
    assert_eq!(
        stdout, expected,
        "`reify eval examples/materials_starter_library.ri` stdout drifted from the golden; \
         re-run with REIFY_REGENERATE_GOLDEN=1 to update"
    );
    assert!(
        stdout.contains("Aluminium_6061_T6 {"),
        "the SIR-b-mat signal requires an inspectable Aluminium_6061_T6 structure value \
         (not `undef`) in `reify eval` output; got:\n{stdout}"
    );
    assert!(
        stdout.contains("Titanium_Ti6Al4V {"),
        "the SIR-b-mat signal requires an inspectable Titanium_Ti6Al4V structure value \
         (not `undef`) in `reify eval` output; got:\n{stdout}"
    );
    assert!(
        stdout.contains("ABS_Plastic {"),
        "the SIR-b-mat signal requires an inspectable ABS_Plastic structure value \
         (not `undef`) in `reify eval` output; got:\n{stdout}"
    );
}
