//! Instance-scope symbolic geometry/selector mint (task #8102).
//!
//! Template scope retries an Undef geometry or selector cell with a kernel-free
//! symbolic mint; a `sub` instance must mint the SAME value for an override-free
//! instantiation, or every typed selector over that geometry differs between the
//! scopes and `@optimized` reuse (#6662) declines.
//!
//! The trampoline/body discriminator is the one `compute_dispatch_registry.rs`
//! uses: the trampoline returns 777, the `.ri` body returns 42, so the observed
//! value names which path ran.

use reify_core::{Severity, ValueCellId};
use reify_eval::{
    CancellationHandle, ComputeFn, ComputeOutcome, EvalResult, RealizationReadHandle,
};
use reify_ir::{OpaqueState, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

/// Trampoline returning a fixed sentinel (777) that can never be confused with
/// the `.ri` body literal (42).
fn const777_fn(
    _value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    ComputeOutcome::Completed {
        result: Value::Int(777),
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
        structured_detail: vec![],
    }
}

/// Compile `source` with the stdlib and `eval` it with `target` bound to the
/// 777 trampoline.
fn eval_with_const777(source: &str, target: &'static str) -> EvalResult {
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    engine.register_compute_fn(target, const777_fn as ComputeFn);
    engine.eval(&compiled)
}

fn cell<'a>(result: &'a EvalResult, entity: &str, member: &str) -> &'a Value {
    result
        .values
        .get(&ValueCellId::new(entity, member))
        .unwrap_or_else(|| panic!("cell {entity}.{member} not found in eval result"))
}

/// The Warnings naming `target`, i.e. the reuse declines reported for it.
fn decline_warnings<'a>(result: &'a EvalResult, target: &str) -> Vec<&'a str> {
    result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains(target))
        .map(|d| d.message.as_str())
        .collect()
}

/// An override-free sub must carry the template's selector over its geometry
/// let, so the `@optimized` read of it reuses the template's dispatched value;
/// an override that reaches ONLY the geometry's inputs must still decline.
///
/// The `resized` body pins the identity rule: an instance handle names the
/// TEMPLATE's realization decl and folds its upstream hash over the instance's
/// own inputs, so it shares `realization_ref` with the template yet compares
/// unequal once an override changes those inputs.
#[test]
fn override_free_sub_reuses_through_a_typed_selector_and_a_geometry_only_override_declines() {
    let source = r#"
        @optimized("test::sel777")
        fn sel_probe(s : FaceSelector) -> Int {
            42
        }

        structure SelInner {
            param len : Length = 10mm
            let body = box(len, 10mm, 10mm)
            let r = sel_probe(face(body, "x_max"))
        }

        structure SelOuter {
            sub same = SelInner()
            sub resized = SelInner(len: 20mm)
        }
    "#;
    let result = eval_with_const777(source, "test::sel777");

    assert_eq!(
        cell(&result, "SelInner", "r"),
        &Value::Int(777),
        "premise: template-scope SelInner.r must be the dispatched 777"
    );
    assert_eq!(
        cell(&result, "SelOuter.same", "r"),
        &Value::Int(777),
        "SelOuter.same.r must reuse the template's dispatched 777: the sub has \
         no overrides, so its selector over `body` must equal the template's \
         (Undef = the instance never minted `body`; Int(42) = the reuse gate \
         declined and the .ri body ran)"
    );
    assert_eq!(
        cell(&result, "SelOuter.resized", "r"),
        &Value::Int(42),
        "SelOuter.resized.r must be the body-inline sentinel 42: `len` reaches \
         the box's inputs, so the template's 777 is not this instance's answer"
    );

    let template_body = cell(&result, "SelInner", "body");
    assert_eq!(
        cell(&result, "SelOuter.same", "body"),
        template_body,
        "SelOuter.same.body must equal the template's symbolic SelInner.body"
    );
    let resized_body = cell(&result, "SelOuter.resized", "body");
    let (
        Value::GeometryHandle {
            realization_ref: resized_ref,
            ..
        },
        Value::GeometryHandle {
            realization_ref: template_ref,
            ..
        },
    ) = (resized_body, template_body)
    else {
        panic!(
            "SelOuter.resized.body and SelInner.body must both be symbolic \
             GeometryHandles, got {resized_body:?} and {template_body:?}"
        );
    };
    assert_eq!(
        resized_ref, template_ref,
        "an instance handle names the TEMPLATE's realization decl"
    );
    assert_ne!(
        resized_body, template_body,
        "SelOuter.resized.body must differ from SelInner.body: the override \
         changes the realization's inputs, so the upstream values hash differs"
    );

    let warnings = decline_warnings(&result, "test::sel777");
    assert_eq!(
        warnings.len(),
        1,
        "expected exactly ONE decline Warning for test::sel777 (the resized \
         instance), got: {:?}",
        result.diagnostics
    );
    assert!(
        warnings[0].contains("SelOuter.resized"),
        "the decline Warning must name SelOuter.resized, got: {}",
        warnings[0]
    );
    assert!(
        !warnings[0].contains("SelOuter.same"),
        "SelOuter.same must reuse, so it must not be named in a decline \
         Warning, got: {}",
        warnings[0]
    );
}

/// A GEOMETRY-consuming `@optimized` read reuses the template's dispatched value
/// for an override-free sub, and declines for one whose override reaches the
/// geometry's inputs.
///
/// This pins `eval()` only. Whether the reuse survives `build()`'s
/// `redispatch_geometry_consuming_compute_nodes` is #7184's to pin.
#[test]
fn geometry_consuming_optimized_cell_reuses_for_an_override_free_sub_at_eval() {
    let source = r#"
        @optimized("test::geom777")
        fn geom_probe(g : Solid) -> Int {
            42
        }

        structure GeomInner {
            param len : Length = 10mm
            let body = box(len, 10mm, 10mm)
            let r = geom_probe(body)
        }

        structure GeomOuter {
            sub same = GeomInner()
            sub resized = GeomInner(len: 20mm)
        }
    "#;
    let result = eval_with_const777(source, "test::geom777");

    assert_eq!(
        cell(&result, "GeomInner", "r"),
        &Value::Int(777),
        "premise: template-scope GeomInner.r must be the dispatched 777"
    );
    assert_eq!(
        cell(&result, "GeomOuter.same", "r"),
        cell(&result, "GeomInner", "r"),
        "GeomOuter.same.r must reuse the template's dispatched value: the sub \
         has no overrides, so its `body` equals the template's (Undef = the \
         instance never minted `body`)"
    );
    assert_eq!(
        cell(&result, "GeomOuter.resized", "r"),
        &Value::Int(42),
        "GeomOuter.resized.r must be the body-inline sentinel 42: `len` reaches \
         the box's inputs, so the template's value is not this instance's answer"
    );
}

/// Phase 1.5's scratch let must mint the same selector phase 2 commits.
///
/// `elaborate_child_instance_nested`'s `Phase15Node::Let` arm computes a SCRATCH
/// value for each let into the overlay that nested subs' constructor args are
/// pre-evaluated against, under the invariant "the scratch value equals the
/// value phase 2 will commit". `Top6.mid.sink.sel` is the one place a scratch
/// value that skipped the mint surfaces: phase 2 commits a minted
/// `Top6.mid.s`, while the overlay still held `body` (and so `s`) as Undef.
#[test]
fn phase15_scratch_overlay_carries_the_minted_selector_into_a_nested_sub_arg() {
    let source = r#"
        structure Leaf6 {
            param sel : Option<FaceSelector> = none
        }

        structure Mid6 {
            param len : Length = 10mm
            let body = box(len, 10mm, 10mm)
            let s = face(body, "x_max")
            sub sink = Leaf6(sel: s)
        }

        structure Top6 {
            sub mid = Mid6()
        }
    "#;
    let compiled = parse_and_compile_with_stdlib(source);
    let result = make_simple_engine().eval(&compiled);

    let committed = cell(&result, "Top6.mid", "s");
    assert!(
        matches!(committed, Value::Selector(_)),
        "phase 2 must commit Top6.mid.s as a minted Selector, got {committed:?}"
    );

    // The `Option<FaceSelector>` param may hold the selector wrapped in `Some`;
    // compare the payload so the assertion is about the value, not the wrapper.
    let passed = match cell(&result, "Top6.mid.sink", "sel") {
        Value::Option(Some(inner)) => inner.as_ref(),
        other => other,
    };
    assert_eq!(
        passed, committed,
        "Top6.mid.sink.sel (unwrapped from its Option) must equal the phase-2 \
         commit of Top6.mid.s: the ctor arg is pre-evaluated against phase \
         1.5's scratch overlay, so Undef here means the scratch let skipped the \
         symbolic mint phase 2 applies"
    );
}
