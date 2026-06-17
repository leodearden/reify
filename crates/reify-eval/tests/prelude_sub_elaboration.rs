//! Eval-time elaboration of sub-components whose templates live in the stdlib
//! prelude (io-export δ scope expansion; esc-4287-15 ruling, option A).
//!
//! Stdlib occurrence templates (`STLOutput` et al., `stdlib/io.ri`) live in
//! `Engine::prelude` and are deliberately NOT merged into
//! `CompiledModule::templates`, so a module-only `find_template` lookup
//! reported them as unknown structures and `sub o = STLOutput(...)` never
//! elaborated into a `Value::StructureInstance`. Sub-component resolution must
//! consult the user module's templates FIRST (shadowing wins) and fall back to
//! the prelude — at BOTH resolver sites: the elaborating loop in `eval()` and
//! the validation-only mirror in `eval_cached()` (the two must agree on which
//! names are unknown, or cached re-evals emit false errors).

use reify_core::{ModulePath, ValueCellId, VersionId};
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::*;

/// Mirrors crates/reify-cli/tests/fixtures/output_driver_single.ri — the B5
/// driver shape: one stdlib `STLOutput` occurrence on a module-local solid.
const SINGLE_STL_OUTPUT: &str = r#"
structure def D {
    let part = box(10mm, 20mm, 5mm)

    sub o = STLOutput(subject: part, resolution: 0.2mm, path: "o.stl")
}
"#;

/// `eval()` must resolve a stdlib occurrence sub via the prelude fallback and
/// elaborate it into a `Value::StructureInstance` at `ValueCellId("D", "o")`
/// with the instance fields populated: `path` from the call-site arg and
/// `format` from the prelude template's default (`OutputFormat.STL`).
#[test]
fn stdlib_occurrence_sub_elaborates_via_prelude_fallback() {
    let compiled = parse_and_compile_with_stdlib(SINGLE_STL_OUTPUT);
    let mut engine = reify_eval::Engine::new(Box::new(MockConstraintChecker::new()), None);
    let result = engine.eval(&compiled);

    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("references unknown structure")),
        "stdlib occurrence sub must resolve via the prelude fallback, got: {:?}",
        result.diagnostics
    );
    assert_eq!(
        engine.last_sub_component_unknown_structure_errors(),
        0,
        "prelude-resolved sub must not count as an unknown-structure miss"
    );

    let instance_id = ValueCellId::new("D", "o");
    let instance = result.values.get(&instance_id).unwrap_or_else(|| {
        panic!(
            "eval must elaborate `sub o = STLOutput(...)` into a value at {:?}. \
             Available values: {:?}",
            instance_id,
            result
                .values
                .iter()
                .map(|(k, _)| k.to_string())
                .collect::<Vec<_>>()
        )
    });
    match instance {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "STLOutput",
                "instance type_name must be the occurrence template name"
            );
            assert_eq!(
                data.fields.get("path"),
                Some(&Value::String("o.stl".to_string())),
                "instance `path` field must carry the call-site arg"
            );
            match data.fields.get("format") {
                Some(Value::Enum { type_name, variant }) => {
                    assert_eq!(type_name, "OutputFormat");
                    assert_eq!(
                        variant, "STL",
                        "format default from the prelude template must be applied"
                    );
                }
                other => panic!(
                    "instance `format` field must be the defaulted \
                     Enum(OutputFormat, STL), got: {:?}",
                    other
                ),
            }
        }
        other => panic!("expected StructureInstance for D.o, got: {:?}", other),
    }
}

/// The `eval_cached()` validation mirror must agree with `eval()` on which
/// structure names are unknown: a stdlib occurrence sub must NOT produce a
/// false "references unknown structure" error on the cached path.
#[test]
fn eval_cached_no_unknown_structure_error_for_stdlib_occurrence_sub() {
    let compiled = parse_and_compile_with_stdlib(SINGLE_STL_OUTPUT);
    let mut engine = reify_eval::Engine::new(Box::new(MockConstraintChecker::new()), None);
    let result = engine.eval_cached(&compiled, VersionId(1));

    assert!(
        !result
            .eval_result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("references unknown structure")),
        "eval_cached must not emit a false unknown-structure error for a \
         prelude-resolvable occurrence sub, got: {:?}",
        result.eval_result.diagnostics
    );
}

/// Guard: the prelude fallback must not make genuinely unknown structures
/// resolve — a sub referencing a name in neither the module nor the prelude
/// still emits the Error diagnostic and increments the instrumentation
/// counter, on both resolver sites.
#[test]
fn genuinely_unknown_structure_still_errors_with_prelude_loaded() {
    let template = TopologyTemplateBuilder::new("Parent")
        .sub_component("rib", "DoesNotExist", vec![])
        .build();
    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    // Engine::new loads the real stdlib prelude — the fallback has plenty of
    // templates to search and must still come up empty for "DoesNotExist".
    let mut engine = reify_eval::Engine::new(Box::new(MockConstraintChecker::new()), None);
    let result = engine.eval(&module);
    assert!(
        result.diagnostics.iter().any(|d| {
            d.message.contains("sub-component")
                && d.message.contains("references unknown structure")
                && d.message.contains("DoesNotExist")
        }),
        "genuinely unknown structure must still error under the prelude \
         fallback, got: {:?}",
        result.diagnostics
    );
    assert_eq!(engine.last_sub_component_unknown_structure_errors(), 1);

    let mut cached_engine =
        reify_eval::Engine::new(Box::new(MockConstraintChecker::new()), None);
    let cached = cached_engine.eval_cached(&module, VersionId(1));
    assert!(
        cached.eval_result.diagnostics.iter().any(|d| {
            d.message.contains("references unknown structure")
                && d.message.contains("DoesNotExist")
        }),
        "eval_cached must still error for genuinely unknown structures, got: {:?}",
        cached.eval_result.diagnostics
    );
}
