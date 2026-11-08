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

/// A `Solid` PARAM default must mint at instance scope too, so an override-free
/// sub's selector over it reuses the template's dispatched value.
#[test]
fn override_free_sub_reuses_through_a_selector_over_a_solid_param_default() {
    let source = r#"
        @optimized("test::sel777")
        fn sel_probe(s : FaceSelector) -> Int {
            42
        }

        structure ParamBody {
            param len : Length = 10mm
            param body : Solid = box(len, 10mm, 10mm)
            let r = sel_probe(face(body, "x_max"))
        }

        structure ParamOuter {
            sub same = ParamBody()
        }
    "#;
    let result = eval_with_const777(source, "test::sel777");

    assert_eq!(
        cell(&result, "ParamBody", "r"),
        &Value::Int(777),
        "premise: template-scope ParamBody.r must be the dispatched 777"
    );

    let template_body = cell(&result, "ParamBody", "body");
    let instance_body = cell(&result, "ParamOuter.same", "body");
    assert!(
        matches!(instance_body, Value::GeometryHandle { .. }),
        "ParamOuter.same.body must be a symbolic GeometryHandle minted from the \
         param default, got {instance_body:?}"
    );
    assert_eq!(
        instance_body, template_body,
        "ParamOuter.same.body must equal the template's ParamBody.body"
    );
    assert_eq!(
        cell(&result, "ParamOuter.same", "r"),
        cell(&result, "ParamBody", "r"),
        "ParamOuter.same.r must reuse the template's dispatched 777 (Undef = the \
         param default never minted `body`; Int(42) = the reuse gate declined)"
    );
}

/// Phase 1's scratch let must mint the same selector phase 2 commits.
///
/// Since #7511, `elaborate_child_params_only` evaluates the lets a param default
/// reads as SCRATCH values (`Phase1Step::ScratchLet`). `p`'s default is a
/// `ValueRef` to the selector let `s`, not a selector constructor, so the
/// param-default mint cannot rescue it: only a minted scratch `body`/`s` makes
/// `ScratchOuter.same.p` the template's Selector, and so lets the `@optimized`
/// read of it reuse the template's dispatched value.
#[test]
fn phase1_scratch_let_mints_so_a_param_default_reading_a_selector_let_reuses() {
    let source = r#"
        @optimized("test::sel777")
        fn sel_probe(s : FaceSelector) -> Int {
            42
        }

        structure ScratchInner {
            param len : Length = 10mm
            let body = box(len, 10mm, 10mm)
            let s = face(body, "x_max")
            param p : FaceSelector = s
            let r = sel_probe(p)
        }

        structure ScratchOuter {
            sub same = ScratchInner()
        }
    "#;
    let result = eval_with_const777(source, "test::sel777");

    let template_p = cell(&result, "ScratchInner", "p");
    assert!(
        matches!(template_p, Value::Selector(_)),
        "premise: template-scope ScratchInner.p must be a Selector, got {template_p:?}"
    );
    assert_eq!(
        cell(&result, "ScratchInner", "r"),
        &Value::Int(777),
        "premise: template-scope ScratchInner.r must be the dispatched 777"
    );

    let instance_p = cell(&result, "ScratchOuter.same", "p");
    assert!(
        matches!(instance_p, Value::Selector(_)),
        "ScratchOuter.same.p must be a Selector: Undef means phase 1's scratch \
         `body`/`s` were not minted, got {instance_p:?}"
    );
    assert_eq!(
        instance_p, template_p,
        "ScratchOuter.same.p must equal the template's ScratchInner.p"
    );
    assert_eq!(
        cell(&result, "ScratchOuter.same", "r"),
        &Value::Int(777),
        "ScratchOuter.same.r must reuse the template's dispatched 777 (Int(42) = \
         the reuse gate declined on `p`)"
    );
    let warnings = decline_warnings(&result, "test::sel777");
    assert!(
        warnings.is_empty(),
        "an override-free sub must not report a decline for test::sel777, got: \
         {warnings:?}"
    );
}

/// `source` with `template_param` set to `edited`, evaluated two ways: WARM
/// (`eval`, then `edit_param`) and COLD (`set_param_and_invalidate`, then
/// `eval`). Returns the warm engine's pre-edit eval, its `edit_param` result,
/// and the cold eval.
fn warm_edit_and_cold_eval(
    source: &str,
    template_param: &ValueCellId,
    edited: Value,
) -> (EvalResult, EvalResult, EvalResult) {
    let compiled = parse_and_compile_with_stdlib(source);
    let mut warm = make_simple_engine();
    let before = warm.eval(&compiled);
    let after_edit = warm
        .edit_param(template_param.clone(), edited.clone())
        .unwrap_or_else(|e| panic!("edit_param({template_param}) must succeed after eval: {e:?}"));

    let mut cold = make_simple_engine();
    cold.set_param_and_invalidate(template_param, edited);
    let cold = cold.eval(&compiled);
    (before, after_edit, cold)
}

/// `instance.body` after an edit must be what a cold eval of the edited state
/// commits: a minted handle naming the template's realization decl, with the
/// upstream hash folded over the instance's OWN committed values.
///
/// Premise: the warm and cold engines commit the same `instance.len`, so the
/// handles are compared like for like. Whether instance params follow a
/// template param edit is #7184's question, not these tests'.
fn assert_edited_body_like_cold(after_edit: &EvalResult, cold: &EvalResult, instance: &str) {
    assert_eq!(
        cell(after_edit, instance, "len"),
        cell(cold, instance, "len"),
        "premise: the warm and cold engines must commit the same {instance}.len, \
         or their instance cells are not comparable"
    );
    let edited_body = cell(after_edit, instance, "body");
    assert!(
        matches!(edited_body, Value::GeometryHandle { .. }),
        "{instance}.body must be a minted GeometryHandle after the edit: Undef \
         means the edit path evaluated the instance cell without the \
         instance-scope mint, got {edited_body:?}"
    );
    assert_eq!(
        edited_body,
        cell(cold, instance, "body"),
        "{instance}.body after the edit must equal a cold eval's: the \
         template's realization, hashed over the instance's own values"
    );
}

/// The premise every edit-path pin starts from: `eval()` minted `instance.body`.
fn assert_minted_at_eval(before: &EvalResult, instance: &str) {
    let minted_at_eval = cell(before, instance, "body");
    assert!(
        matches!(minted_at_eval, Value::GeometryHandle { .. }),
        "premise: eval() mints {instance}.body, got {minted_at_eval:?}"
    );
}

/// `edit_param` on a TEMPLATE param re-evaluates an override-free sub's
/// geometry cell, and the edit path must mint it by the same instance-scope
/// rule as `eval()`: the template's realization decl, with the upstream hash
/// folded over the instance's OWN committed values. So the edited cell must
/// equal what a cold eval of the edited state commits for it.
#[test]
fn edit_param_on_a_template_param_mints_an_override_free_instance_like_a_cold_eval() {
    let source = r#"
        structure EditInner {
            param len : Length = 10mm
            let body = box(len, 10mm, 10mm)
        }

        structure EditOuter {
            sub same = EditInner()
        }
    "#;
    let (before, after_edit, cold) = warm_edit_and_cold_eval(
        source,
        &ValueCellId::new("EditInner", "len"),
        Value::length(0.03),
    );

    assert_minted_at_eval(&before, "EditOuter.same");
    assert_edited_body_like_cold(&after_edit, &cold, "EditOuter.same");
}

/// A rack of two bolts whose count is static, so `eval()` stamps both slots.
const BOLT_RACK: &str = r#"
    structure CBolt {
        param len : Length = 10mm
        let body = box(len, 10mm, 10mm)
    }

    structure CRack {
        param n : Int = 2
        sub bolts : List<CBolt>
        constraint bolts.count == n
    }
"#;

/// A static-count collection's slots are sub instances too, so a template
/// param edit must leave every slot's geometry cell minted like a cold eval.
#[test]
fn edit_param_on_a_template_param_mints_static_collection_slots_like_a_cold_eval() {
    let (before, after_edit, cold) = warm_edit_and_cold_eval(
        BOLT_RACK,
        &ValueCellId::new("CBolt", "len"),
        Value::length(0.03),
    );

    for slot in ["CRack.bolts[0]", "CRack.bolts[1]"] {
        assert_minted_at_eval(&before, slot);
        assert_edited_body_like_cold(&after_edit, &cold, slot);
    }
}

/// Growing a collection re-creates its slots in the edit path's resize loop,
/// whose plain evaluation of a geometry cell returns Undef. Every slot,
/// the new one included, must still mint like a cold eval of the grown count.
#[test]
fn edit_param_growing_a_collection_mints_every_slot_like_a_cold_eval() {
    let (before, after_edit, cold) =
        warm_edit_and_cold_eval(BOLT_RACK, &ValueCellId::new("CRack", "n"), Value::Int(3));

    for slot in ["CRack.bolts[0]", "CRack.bolts[1]"] {
        assert_minted_at_eval(&before, slot);
    }
    for slot in ["CRack.bolts[0]", "CRack.bolts[1]", "CRack.bolts[2]"] {
        assert_edited_body_like_cold(&after_edit, &cold, slot);
    }
}

/// `edit_source`'s collection resize re-creates a grown collection's slots as
/// `edit_param`'s does, so a source edit that grows the count must leave every
/// slot's geometry cell minted like a cold eval of the edited source.
#[test]
fn edit_source_growing_a_collection_mints_every_slot_like_a_cold_eval() {
    let grown_source = BOLT_RACK.replace("param n : Int = 2", "param n : Int = 3");
    assert_ne!(
        grown_source, BOLT_RACK,
        "premise: the source edit must change the count default"
    );
    let grown = parse_and_compile_with_stdlib(&grown_source);
    let mut warm = make_simple_engine();
    let before = warm.eval(&parse_and_compile_with_stdlib(BOLT_RACK));
    let after_edit = warm
        .edit_source(&grown)
        .unwrap_or_else(|e| panic!("edit_source must succeed after eval: {e:?}"));
    let cold = make_simple_engine().eval(&grown);

    for slot in ["CRack.bolts[0]", "CRack.bolts[1]"] {
        assert_minted_at_eval(&before, slot);
    }
    for slot in ["CRack.bolts[0]", "CRack.bolts[1]", "CRack.bolts[2]"] {
        assert_edited_body_like_cold(&after_edit, &cold, slot);
    }
}

/// A rack whose element declares each cell BEFORE the sibling it reads.
const DECL_ORDER_RACK: &str = r#"
    structure DBolt {
        let s = face(body, "x_max")
        let body = box(len, 10mm, 10mm)
        param len : Length = 10mm
    }

    structure DRack {
        param n : Int = 2
        sub bolts : List<DBolt>
        constraint bolts.count == n
    }
"#;

/// `instance.s` after an edit must be the selector a cold eval commits.
fn assert_edited_selector_like_cold(after_edit: &EvalResult, cold: &EvalResult, instance: &str) {
    let edited_selector = cell(after_edit, instance, "s");
    assert!(
        matches!(edited_selector, Value::Selector(_)),
        "{instance}.s must be a minted Selector after the edit: Undef means the \
         resize minted `s` before the slot's `body` was committed, got \
         {edited_selector:?}"
    );
    assert_eq!(
        edited_selector,
        cell(cold, instance, "s"),
        "{instance}.s after the edit must equal a cold eval's: an unequal \
         selector means the resize minted `s` before the slot's `body` was \
         committed"
    );
}

/// The premise both declaration-order pins start from: `eval()` minted the
/// slot's `body` and `s`.
fn assert_body_and_selector_minted_at_eval(before: &EvalResult, instance: &str) {
    assert_minted_at_eval(before, instance);
    let selector_at_eval = cell(before, instance, "s");
    assert!(
        matches!(selector_at_eval, Value::Selector(_)),
        "premise: eval() mints {instance}.s, got {selector_at_eval:?}"
    );
}

/// A resize re-creates EVERY slot's cells, so a cell declared before the
/// sibling it reads must still mint over that sibling's committed value, in
/// the surviving slots as well as the grown one.
#[test]
fn edit_param_growing_a_collection_mints_cells_declared_before_their_reads_like_a_cold_eval() {
    let (before, after_edit, cold) = warm_edit_and_cold_eval(
        DECL_ORDER_RACK,
        &ValueCellId::new("DRack", "n"),
        Value::Int(3),
    );

    for slot in ["DRack.bolts[0]", "DRack.bolts[1]"] {
        assert_body_and_selector_minted_at_eval(&before, slot);
    }
    for slot in ["DRack.bolts[0]", "DRack.bolts[1]", "DRack.bolts[2]"] {
        assert_edited_body_like_cold(&after_edit, &cold, slot);
        assert_edited_selector_like_cold(&after_edit, &cold, slot);
    }
}

/// `edit_source`'s resize must commit a slot's cells in the same order as
/// `edit_param`'s, so a grown count leaves every slot minted like a cold eval.
#[test]
fn edit_source_growing_a_collection_mints_cells_declared_before_their_reads_like_a_cold_eval() {
    let grown_source = DECL_ORDER_RACK.replace("param n : Int = 2", "param n : Int = 3");
    assert_ne!(
        grown_source, DECL_ORDER_RACK,
        "premise: the source edit must change the count default"
    );
    let grown = parse_and_compile_with_stdlib(&grown_source);
    let mut warm = make_simple_engine();
    let before = warm.eval(&parse_and_compile_with_stdlib(DECL_ORDER_RACK));
    let after_edit = warm
        .edit_source(&grown)
        .unwrap_or_else(|e| panic!("edit_source must succeed after eval: {e:?}"));
    let cold = make_simple_engine().eval(&grown);

    for slot in ["DRack.bolts[0]", "DRack.bolts[1]"] {
        assert_body_and_selector_minted_at_eval(&before, slot);
    }
    for slot in ["DRack.bolts[0]", "DRack.bolts[1]", "DRack.bolts[2]"] {
        assert_edited_body_like_cold(&after_edit, &cold, slot);
        assert_edited_selector_like_cold(&after_edit, &cold, slot);
    }
}

/// A rack whose element's `body` consumes a GUARDED geometry let. The compiler
/// inlines `core`'s box into `body`'s realization, so `body`'s handle hash
/// folds `len`, which `body`'s own expression never reads.
const GUARDED_CORE_RACK: &str = r#"
    structure GBolt {
        param len : Length = 10mm
        param on : Bool = true
        where on {
            let core = box(len, 10mm, 10mm)
        }
        let body = translate(core, 1mm, 0mm, 0mm)
    }

    structure GRack {
        param n : Int = 2
        sub bolts : List<GBolt>
        constraint bolts.count == n
    }
"#;

/// The edit path's instance mint must fold every input of the realization's
/// operations, not only the reads of the cell's expression: a slot re-created
/// by a resize mints `body` over the slot's own `len`, as a cold eval does.
#[test]
fn edit_param_growing_a_collection_mints_a_body_over_a_guarded_geometry_let_like_a_cold_eval() {
    let (before, after_edit, cold) = warm_edit_and_cold_eval(
        GUARDED_CORE_RACK,
        &ValueCellId::new("GRack", "n"),
        Value::Int(3),
    );

    for slot in ["GRack.bolts[0]", "GRack.bolts[1]"] {
        assert_minted_at_eval(&before, slot);
    }
    for slot in ["GRack.bolts[0]", "GRack.bolts[1]", "GRack.bolts[2]"] {
        assert_edited_body_like_cold(&after_edit, &cold, slot);
    }
}
