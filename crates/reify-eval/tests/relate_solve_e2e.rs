//! Per-scope relate-solve end-to-end tests — geometric-relations ζ (task 4386).
//!
//! This file is layered (design "Test layering"):
//!
//!   * **kernel-free unit slices** (this step, step-3) drive the pure
//!     scope-collection logic (`reify_eval::relate_solve::collect_relate_scope`)
//!     over a compiled `TopologyTemplate`. No geometry kernel is needed: ζ step-2
//!     already threaded the flat relation set + the per-sub auto-pose spec onto the
//!     compiled template, so classification reads structurally off the template.
//!   * **OCCT-gated e2e slices** (later steps 5/13/15/17) realize datums + drive
//!     the full build against the real kernel.
//!
//! ## step-3 (this slice) — RED
//!
//! `collect_relate_scope(template)` must, for the §1 `BoltPlate` scope, return a
//! `RelateScope` that partitions the scope into the relate-solve's three inputs:
//!
//!   (i)   the **auto Frame unknowns** — one per `at auto` sub, each carrying the
//!         sub id + the `free` flag + the ordered seed params (from step-2's
//!         threaded `auto_pose` spec);
//!   (ii)  the **flat ordered relation list** — the threaded per-scope relation set
//!         (each a `FunctionCall` retaining its name + operand exprs), in source
//!         order; and
//!   (iii) the **ground set** — the non-auto subs that serve as the fixed anchor.
//!
//! RED until step-4 creates `crates/reify-eval/src/relate_solve.rs` (declared in
//! `lib.rs`) with `collect_relate_scope` + the `RelateScope`/`AutoUnknown` types.
//! The file fails to compile against the missing module — the established
//! RED-by-missing-symbol convention (mirrors `relate_threading_tests.rs`).

use reify_compiler::{CompiledModule, TopologyTemplate};
use reify_eval::relate_solve::{RelateScope, collect_relate_scope};
use reify_ir::{CompiledExpr, CompiledExprKind};
use reify_test_support::compile_source_with_stdlib;

/// Read the committed §1 worked example so the kernel-free unit slice and the
/// step-17/18 e2e build exercise the SAME source — no drift between the collection
/// test and the example. `CARGO_MANIFEST_DIR` is `crates/reify-eval`;
/// `../../examples/...` is the workspace-root example dir.
fn bolt_plate_source() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/geometric_relations/bolt_plate.ri"
    );
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read §1 example {path}: {e}"))
}

/// Find the named template, panicking with the full diagnostics on miss.
fn template<'a>(module: &'a CompiledModule, name: &str) -> &'a TopologyTemplate {
    module.templates.iter().find(|t| t.name == name).unwrap_or_else(|| {
        panic!(
            "no template {name:?} in compiled module; diagnostics: {:#?}",
            module.diagnostics
        )
    })
}

/// The relation's function name + operand count. Each relation compiles to a
/// `FunctionCall` over its datum operands — γ types it `Relation` but keeps the
/// node a `FunctionCall` (no `Value::Relation`), so the name + arity are
/// recoverable here. Mirrors the helper in `relate_threading_tests.rs`.
fn relation_name_arity(expr: &CompiledExpr) -> (String, usize) {
    match &expr.kind {
        CompiledExprKind::FunctionCall { function, args } => (function.name.clone(), args.len()),
        other => panic!("a collected relation must be a FunctionCall, got {other:?}"),
    }
}

/// step-3 — the §1 `BoltPlate` scope collects into the relate-solve's three
/// inputs: one auto unknown (the bolt, strict `at auto` → free=false, no seed
/// params), the plate in the ground set, and the two relations in source order.
#[test]
fn collect_relate_scope_classifies_auto_ground_and_relations() {
    let module = compile_source_with_stdlib(&bolt_plate_source());
    let bp = template(&module, "BoltPlate");

    let scope: RelateScope = collect_relate_scope(bp);

    // (i) the auto Frame unknowns — exactly the bolt, carrying its id + free flag
    //     + (empty) seed params. Bare `at auto` is strict ⇒ free=false.
    assert_eq!(
        scope.auto_unknowns.len(),
        1,
        "§1 has exactly one `at auto` sub (the bolt), got {:?}",
        scope
            .auto_unknowns
            .iter()
            .map(|u| u.sub.as_str())
            .collect::<Vec<_>>()
    );
    let bolt = &scope.auto_unknowns[0];
    assert_eq!(bolt.sub, "bolt", "the lone auto unknown is the bolt sub");
    assert!(!bolt.free, "bare `at auto` is strict — free=false");
    assert!(
        bolt.seed_params.is_empty(),
        "bare `at auto` carries no seed/component-fix params, got {:?}",
        bolt.seed_params
    );

    // (iii) the ground set — the non-auto plate sub is the fixed anchor; the auto
    //       bolt is NOT in the ground set.
    assert_eq!(
        scope.ground,
        vec!["plate".to_string()],
        "the grounded `plate` sub (no `at auto`) is the sole anchor; the auto bolt \
         is an unknown, not ground"
    );

    // (ii) the flat relation list — both §1 relations, in source order, each
    //      retaining its name + two operand exprs.
    let rels: Vec<(String, usize)> = scope.relations.iter().map(relation_name_arity).collect();
    assert_eq!(
        rels,
        vec![("concentric".to_string(), 2), ("flush".to_string(), 2)],
        "the §1 scope collects both relations in source order (concentric then \
         flush), each retaining its name + 2 operand exprs"
    );
}
