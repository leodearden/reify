//! Threading test for the per-scope relate-solve inputs — geometric-relations ζ
//! (task 4386), step-1/2.
//!
//! δ type-checks `relate { }` / `sub … where { }` relations and lowers `at auto`
//! to `ExprKind::Auto`, but DROPS both before eval: the compiled `TopologyTemplate`
//! has no relations field, and `compile_expr` lowers `ExprKind::Auto` to a
//! `Value::Undef` pose literal (the free/seed spec is lost). ζ's relate-solve needs
//! BOTH threaded onto the compiled template:
//!
//!   (a) the flat, source-ordered per-scope **relation set** — the member-level
//!       `relate { }` block ∪ each inline `sub … where { }` relation merged in
//!       source order (design §4: both desugar to one flat set; source order encodes
//!       "newest member" for conflict attribution), each relation retaining its
//!       relation name + compiled operand exprs; and
//!   (b) for each `at auto` sub, a preserved **auto-pose spec** (free flag + ordered
//!       params) instead of the `Value::Undef` pose literal.
//!
//! RED until step-2 adds `TopologyTemplate.relations` + `SubComponentDecl.auto_pose`
//! (+ the `AutoPoseSpec` type) and threads them in `entity.rs`. The file fails to
//! compile against the missing fields — the established RED-by-missing-symbol
//! convention (mirrors `relate_block_check_tests.rs`).

use reify_compiler::{CompiledModule, SubComponentDecl, TopologyTemplate};
use reify_ir::{CompiledExpr, CompiledExprKind};
use reify_test_support::compile_source_with_stdlib;

/// Read the committed §1 worked example so step-1 and the step-17/18 e2e build
/// exercise the SAME source — no drift between the threading test and the example.
/// `CARGO_MANIFEST_DIR` is `crates/reify-compiler`; `../../examples/...` is the
/// workspace-root example dir.
fn bolt_plate_source() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/geometric_relations/bolt_plate.ri"
    );
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read §1 example {path}: {e}"))
}

/// A cross-home variant: an inline `sub … at auto where { … }` relation declared
/// BEFORE a member-level `relate { }` block. The flat per-scope set must merge both
/// in source order — `concentric` (inline, textually first) precedes `flush`
/// (member block, textually later) — which pins that the merge is true source order,
/// not "member-block-first then inline". Self-contained: `cylinder`/`box`/`rectangle`
/// are built-in geometry constructors and `.axis`/`.plane` are ε feature→datum
/// projections (typed at compile time).
const INLINE_MERGE_SRC: &str = r#"
structure Bolt {
    let shank = cylinder(3mm, 20mm)
    let shank_axis : Axis = shank.axis
    let seat = rectangle(12mm, 12mm)
    let seat_plane : Plane = seat.plane
}

structure Plate {
    let body = box(40mm, 40mm, 5mm)
    let hole = cylinder(3.2mm, 5mm)
    let hole_axis : Axis = hole.axis
    let top = rectangle(40mm, 40mm)
    let top_plane : Plane = top.plane
}

structure InlineMerge {
    sub bolt : Bolt at auto where {
        concentric(bolt.shank_axis, plate.hole_axis)
    }
    sub plate : Plate
    relate {
        flush(bolt.seat_plane, plate.top_plane)
    }
}
"#;

/// Find the named template, panicking with the full diagnostics on miss.
fn template<'a>(module: &'a CompiledModule, name: &str) -> &'a TopologyTemplate {
    module.templates.iter().find(|t| t.name == name).unwrap_or_else(|| {
        panic!(
            "no template {name:?} in compiled module; diagnostics: {:#?}",
            module.diagnostics
        )
    })
}

/// Find the named sub-component of a template.
fn sub<'a>(template: &'a TopologyTemplate, name: &str) -> &'a SubComponentDecl {
    template
        .sub_components
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("no sub {name:?} in template {}", template.name))
}

/// The relation's function name + operand count. Each relation compiles to a
/// `FunctionCall` over its datum operands — γ types it `Relation` but keeps the node
/// a `FunctionCall` (no `Value::Relation`), so the name + arity are recoverable here.
fn relation_name_arity(expr: &CompiledExpr) -> (String, usize) {
    match &expr.kind {
        CompiledExprKind::FunctionCall { function, args } => (function.name.clone(), args.len()),
        other => panic!("a threaded relation must be a FunctionCall, got {other:?}"),
    }
}

/// (a) — the §1 `relate { }` block threads its two relations onto the `BoltPlate`
/// template in source order, each retaining its name + two compiled operand exprs.
#[test]
fn bolt_plate_threads_member_relations_in_source_order() {
    let module = compile_source_with_stdlib(&bolt_plate_source());
    let bp = template(&module, "BoltPlate");

    let rels: Vec<(String, usize)> = bp.relations.iter().map(relation_name_arity).collect();
    assert_eq!(
        rels,
        vec![("concentric".to_string(), 2), ("flush".to_string(), 2)],
        "§1 BoltPlate must thread the member `relate {{ }}` relations in source order \
         (concentric then flush), each retaining its name + 2 operand exprs"
    );
}

/// (b) — the `at auto` bolt sub carries a preserved auto-pose spec (bare `auto` is
/// strict: free=false, no seed/component-fix params), NOT a `Value::Undef` pose; the
/// grounded plate sub (no `at auto`) carries no auto-pose spec.
#[test]
fn bolt_plate_preserves_auto_pose_only_on_auto_sub() {
    let module = compile_source_with_stdlib(&bolt_plate_source());
    let bp = template(&module, "BoltPlate");

    let bolt = sub(bp, "bolt");
    let auto = bolt
        .auto_pose
        .as_ref()
        .expect("the `at auto` bolt sub must carry a preserved auto-pose spec, not a Undef pose");
    assert!(!auto.free, "bare `at auto` is strict — free=false");
    assert!(
        auto.params.is_empty(),
        "bare `at auto` carries no seed/component-fix params, got {:?}",
        auto.params
    );

    let plate = sub(bp, "plate");
    assert!(
        plate.auto_pose.is_none(),
        "the grounded `plate` sub (no `at auto`) must have no auto-pose spec"
    );
}

/// (a) cross-home merge — an inline `sub … where { concentric }` declared before a
/// member `relate { flush }` block threads BOTH onto the scope's flat relation set in
/// source order: `concentric` (inline, textually first) precedes `flush` (member
/// block, textually later). Pins that the merge is true source order.
#[test]
fn inline_where_and_member_relate_merge_in_source_order() {
    let module = compile_source_with_stdlib(INLINE_MERGE_SRC);
    let merge = template(&module, "InlineMerge");

    let rels: Vec<(String, usize)> = merge.relations.iter().map(relation_name_arity).collect();
    assert_eq!(
        rels,
        vec![("concentric".to_string(), 2), ("flush".to_string(), 2)],
        "the inline `where {{ }}` relation (concentric) must precede the member \
         `relate {{ }}` relation (flush) — both homes merge into one flat set in \
         source order"
    );
}
