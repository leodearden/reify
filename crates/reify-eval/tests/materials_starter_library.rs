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

use reify_core::ValueCellId;
use reify_ir::{PersistentMap, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

/// `PersistentMap<String, Value>::get` is keyed by `&String`; this lets tests
/// index `StructureInstance.fields` with a string literal.
///
/// Note: an identical helper exists in `structure_instance_e2e.rs` — this is
/// the second copy. A third consumer would justify moving it into
/// `reify_test_support`; a third consumer would justify moving it (see task-3542 review).
fn field<'a>(m: &'a PersistentMap<String, Value>, k: &str) -> Option<&'a Value> {
    m.get(&k.to_string())
}

// ── Shared assertion helper ───────────────────────────────────────────────────

/// Compile `structure def <fixture_name> { let mat = <type_name>(); … }`,
/// evaluate it, and assert the three-part contract:
///
/// (a) `<fixture_name>.mat` is a `Value::StructureInstance` whose `type_name`
///     equals `type_name`.
/// (b) The instance carries all four `ElasticMaterial` fields
///     (`youngs_modulus`, `poisson_ratio`, `density`, `yield_stress`),
///     all non-Undef.
/// (c) The member-access cells `e`, `nu`, `rho` resolve to
///     `Value::Scalar { .. }` / `Value::Real(_)` (non-Undef).
///
/// The three `#[test]` wrappers each call this helper independently, so a
/// failure in one material surfaces under its own test name without masking
/// the other two.
fn assert_material_round_trip(type_name: &str, fixture_name: &str) {
    let source = format!(
        "
structure def {fixture_name} {{
    let mat = {type_name}()
    let e   = self.mat.youngs_modulus
    let nu  = self.mat.poisson_ratio
    let rho = self.mat.density
}}
"
    );

    let compiled = parse_and_compile_with_stdlib(&source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // (a) mat cell must be a StructureInstance with the correct type_name.
    let mat = result
        .values
        .get(&ValueCellId::new(fixture_name, "mat"))
        .unwrap_or_else(|| panic!("{fixture_name}.mat cell missing from eval result"));

    match mat {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, type_name,
                "expected type_name={type_name:?}, got {:?}",
                data.type_name
            );
            // (b) all four fields must be present and non-Undef.
            for field_name in &["youngs_modulus", "poisson_ratio", "density", "yield_stress"] {
                assert!(
                    field(&data.fields, field_name).is_some(),
                    "{type_name} instance must carry field `{field_name}`; \
                     present fields: {:?}",
                    data.fields.iter().map(|(k, _)| k).collect::<Vec<_>>()
                );
                assert!(
                    !matches!(field(&data.fields, field_name), Some(Value::Undef)),
                    "field `{field_name}` must not be Undef in {type_name} defaults"
                );
            }
        }
        other => panic!("expected Value::StructureInstance for {fixture_name}.mat, got {other:?}"),
    }

    // (c) member-access cells must resolve to non-Undef scalars.
    let e = result
        .values
        .get(&ValueCellId::new(fixture_name, "e"))
        .unwrap_or_else(|| panic!("{fixture_name}.e cell missing"));
    assert!(
        matches!(e, Value::Scalar { .. }),
        "self.mat.youngs_modulus must resolve to a Scalar for {type_name}, got {e:?}"
    );

    let nu = result
        .values
        .get(&ValueCellId::new(fixture_name, "nu"))
        .unwrap_or_else(|| panic!("{fixture_name}.nu cell missing"));
    assert!(
        matches!(nu, Value::Real(_)),
        "self.mat.poisson_ratio must resolve to a Real for {type_name}, got {nu:?}"
    );

    let rho = result
        .values
        .get(&ValueCellId::new(fixture_name, "rho"))
        .unwrap_or_else(|| panic!("{fixture_name}.rho cell missing"));
    assert!(
        matches!(rho, Value::Scalar { .. }),
        "self.mat.density must resolve to a Scalar for {type_name}, got {rho:?}"
    );
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
    assert_material_round_trip("Aluminium_6061_T6", "AluminiumFixture");
}

/// `Titanium_Ti6Al4V` round-trip: the constructor evaluates to a
/// `Value::StructureInstance` carrying the three primary engineering defaults,
/// and member-access expressions resolve to `Value::Scalar` (non-Undef).
///
/// Expected defaults (from `materials_fea.ri`):
///   youngs_modulus = 113.8 GPa,  poisson_ratio = 0.342,  density = 4430 kg/m³.
#[test]
fn titanium_ti6al4v_field_read_round_trip() {
    assert_material_round_trip("Titanium_Ti6Al4V", "TitaniumFixture");
}

/// `ABS_Plastic` round-trip: the constructor evaluates to a
/// `Value::StructureInstance` carrying the three primary engineering defaults,
/// and member-access expressions resolve to `Value::Scalar` (non-Undef).
///
/// Expected defaults (from `materials_fea.ri`):
///   youngs_modulus = 2.3 GPa,  poisson_ratio = 0.35,  density = 1050 kg/m³.
#[test]
fn abs_plastic_field_read_round_trip() {
    assert_material_round_trip("ABS_Plastic", "AbsFixture");
}

// ── CLI golden test ───────────────────────────────────────────────────────────

/// `reify eval examples/materials_starter_library.ri` must print inspectable
/// structure-shaped values (not `undef`) for all three wave-2 materials, and
/// its stdout must match the committed golden. Regenerate with
/// `REIFY_REGENERATE_GOLDEN=1`.
///
/// `CARGO_BIN_EXE_reify` is only injected for `reify-cli`'s own integration
/// tests, so this cross-crate test drives the pre-built `reify` binary
/// directly. It deliberately does NOT use `cargo run`: even when the binary
/// is already compiled, `cargo run` re-fingerprints the entire workspace
/// before exec, and under high build concurrency that overhead can push the
/// test suite past its time budget (esc-4340-32, exit 124). `cargo test -p
/// reify-cli` builds all `[[bin]]` targets, including `reify`, so the binary
/// is present at `<target>/debug/reify`. The cargo runner
/// (`.cargo/run-with-occt.sh`) exports `LD_LIBRARY_PATH` into this test
/// process's environment, which the spawned child inherits, so OCCT shared
/// libraries resolve without going through cargo.
#[test]
fn cli_reify_eval_prints_inspectable_material_values() {
    let manifest = env!("CARGO_MANIFEST_DIR"); // .../crates/reify-eval
    let workspace_root = std::path::Path::new(manifest)
        .ancestors()
        .nth(2)
        .expect("workspace root is two levels above crates/reify-eval")
        .to_path_buf();
    let example = workspace_root.join("examples/materials_starter_library.ri");
    let golden = std::path::Path::new(manifest).join("tests/golden/materials_starter_library.txt");

    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| workspace_root.join("target"));
    let reify_bin = target_dir.join("debug").join("reify");
    let output = std::process::Command::new(&reify_bin)
        .current_dir(&workspace_root)
        .arg("eval")
        .arg(&example)
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "failed to spawn pre-built reify binary at {}: {e}; \
                 is it built? run `cargo test -p reify-cli`",
                reify_bin.display()
            )
        });

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
    // Defence-in-depth: assert the committed golden itself names all three
    // materials. Checked against `expected` (not `stdout`) so the intent is
    // explicit — this fires if someone regenerated the golden against a
    // regressed binary before the `assert_eq` above is reached.
    assert!(
        expected.contains("Aluminium_6061_T6 {"),
        "committed golden must mention Aluminium_6061_T6 — golden may have been \
         regenerated against a regressed binary; re-run with REIFY_REGENERATE_GOLDEN=1 \
         after fixing the regression.\ngolden:\n{expected}"
    );
    assert!(
        expected.contains("Titanium_Ti6Al4V {"),
        "committed golden must mention Titanium_Ti6Al4V — golden may have been \
         regenerated against a regressed binary; re-run with REIFY_REGENERATE_GOLDEN=1 \
         after fixing the regression.\ngolden:\n{expected}"
    );
    assert!(
        expected.contains("ABS_Plastic {"),
        "committed golden must mention ABS_Plastic — golden may have been \
         regenerated against a regressed binary; re-run with REIFY_REGENERATE_GOLDEN=1 \
         after fixing the regression.\ngolden:\n{expected}"
    );
}
