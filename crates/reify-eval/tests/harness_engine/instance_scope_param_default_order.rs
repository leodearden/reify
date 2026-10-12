//! Instance-scope counterpart of the template-scope ordering fix in
//! `param_default_sibling_let_order.rs` (task #4317).
//!
//! That file covers `engine_eval.rs`, where #4317 collapsed the kind-partitioned
//! two-pass into ONE dependency-ordered pass over all non-Auto body cells. THIS
//! file covers `unfold.rs::elaborate_child_params_only` — the phase-1 param loop
//! that runs when a template is instantiated under a `sub`.
//!
//! The property these tests pin is TWO-SCOPE AGREEMENT: each assertion compares
//! the instance cell against the template cell as an equality AND against the
//! expected literal. The equality alone would be satisfied by both scopes
//! degrading together; the literal alone would let a regression that degrades
//! both scopes pass. Together they pin what the ticket asks for.

use reify_compiler::CompiledModule;
use reify_core::{Severity, ValueCellId};
use reify_eval::Engine;
use reify_ir::{DeterminacyState, Value};
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::parse_and_compile;

/// Build an Engine with an empty prelude for self-contained tests.
fn fresh_engine() -> Engine {
    Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[])
}

/// Convenience: parse + compile a source string.
fn compile_source(source: &str) -> CompiledModule {
    parse_and_compile(source)
}

// ──────────────────────────────────────────────────────────────────────────────
// step-1: RED — core repro, a param default reading a later-declared sibling
// ──────────────────────────────────────────────────────────────────────────────

/// The canonical repro. `Inner.p` reads `seed`, declared after it.
///
/// MEASURED RED on base 5efc4c04ce: template `Inner.p == Int(4)`, instance
/// `Outer.a.p == Undef`, `Outer.a.seed == Int(3)`, and no diagnostic at all —
/// the divergence is silent. Plain arithmetic, no `@optimized` anywhere, which
/// is what makes this independent of #6662's reuse gate.
#[test]
fn param_default_reading_a_later_declared_sibling_matches_template_at_instance_scope() {
    let mut engine = fresh_engine();
    let module = compile_source(
        "structure Inner { \
            param p : Int = seed + 1 \
            param seed : Int = 3 \
        } \
        structure Outer { \
            sub a = Inner() \
        }",
    );

    let result = engine.eval(&module);

    let template_p = ValueCellId::new("Inner", "p");
    let instance_p = ValueCellId::new("Outer.a", "p");

    // The template cell is the reference point. Pinned to the literal too, so
    // this test cannot pass by BOTH scopes degrading together.
    assert_eq!(
        result.values.get(&template_p),
        Some(&Value::Int(4)),
        "template scope resolves the forward read (task #4317); if this is not \
         Int(4) the template-scope fix has regressed and the instance-scope \
         assertion below is measuring nothing"
    );

    assert_eq!(
        result.values.get(&instance_p),
        result.values.get(&template_p),
        "instance scope must agree with template scope on a forward-referencing \
         param default: `elaborate_child_params_only` visits params in dependency \
         order, so `seed` is in `child_values` before `p`'s default runs"
    );
    assert_eq!(
        result.values.get(&instance_p),
        Some(&Value::Int(4)),
        "Outer.a.p must be Int(4); Undef here is the declaration-order gap"
    );

    // `seed` itself was never the problem — it is the control that shows only
    // the forward-reading cell degraded.
    assert_eq!(
        result.values.get(&ValueCellId::new("Outer.a", "seed")),
        Some(&Value::Int(3)),
        "Outer.a.seed has no dependency and resolved even before the fix"
    );

    // Determinacy, not just the value: the gap left the pair (Undef, Undetermined).
    let snap = engine.snapshot().expect("snapshot after eval");
    let (p_val, p_det) = snap
        .values
        .get(&instance_p)
        .expect("snapshot must contain Outer.a.p after eval");
    assert_eq!(
        *p_det,
        DeterminacyState::Determined,
        "Outer.a.p must be Determined, not the (Undef, Undetermined) pair the \
         declaration-order gap produced. Got: ({p_val:?}, {p_det:?})"
    );

    assert_no_error_diagnostics(&result, "a valid forward param->param dependency");
}

/// Transitivity, not a one-hop special case: a 3-deep chain declared entirely in
/// reverse. A fix that only looks one cell ahead resolves `b` and leaves `c`
/// Undef; only a real topological order resolves all three.
///
/// MEASURED RED on base: instance `b` and `c` are both Undef.
#[test]
fn param_defaults_in_reverse_declaration_order_resolve_transitively_at_instance_scope() {
    let mut engine = fresh_engine();
    let module = compile_source(
        "structure Chain { \
            param c : Int = b + 1 \
            param b : Int = a + 1 \
            param a : Int = 1 \
        } \
        structure Holder { \
            sub s = Chain() \
        }",
    );

    let result = engine.eval(&module);

    for (member, expected) in [("a", 1), ("b", 2), ("c", 3)] {
        let template_id = ValueCellId::new("Chain", member);
        let instance_id = ValueCellId::new("Holder.s", member);
        assert_eq!(
            result.values.get(&template_id),
            Some(&Value::Int(expected)),
            "template scope must resolve the reverse chain at Chain.{member}"
        );
        assert_eq!(
            result.values.get(&instance_id),
            result.values.get(&template_id),
            "instance scope must agree with template scope at Holder.s.{member}"
        );
        assert_eq!(
            result.values.get(&instance_id),
            Some(&Value::Int(expected)),
            "Holder.s.{member} must be Int({expected}); a one-hop-only fix leaves \
             the far end of the chain Undef"
        );
    }

    assert_no_error_diagnostics(&result, "a valid reverse-declared param chain");
}

/// The symmetry control: params ALREADY in dependency order must stay resolved.
/// Green today and must stay green — this is what catches a "fix" that merely
/// inverts the visit order instead of ordering it.
#[test]
fn params_in_declaration_order_still_resolve_at_instance_scope() {
    let mut engine = fresh_engine();
    let module = compile_source(
        "structure Fwd { \
            param a : Int = 1 \
            param b : Int = a + 1 \
        } \
        structure FwdHolder { \
            sub s = Fwd() \
        }",
    );

    let result = engine.eval(&module);

    for (member, expected) in [("a", 1), ("b", 2)] {
        let template_id = ValueCellId::new("Fwd", member);
        let instance_id = ValueCellId::new("FwdHolder.s", member);
        assert_eq!(
            result.values.get(&instance_id),
            result.values.get(&template_id),
            "instance scope must agree with template scope at FwdHolder.s.{member}"
        );
        assert_eq!(
            result.values.get(&instance_id),
            Some(&Value::Int(expected)),
            "FwdHolder.s.{member} must be Int({expected}): dependency-ordering the \
             param loop must not break the already-working declaration order"
        );
    }

    assert_no_error_diagnostics(&result, "params declared in dependency order");
}

// ──────────────────────────────────────────────────────────────────────────────
// step-3: RED — ordering edges come from the expression that ACTUALLY runs
// ──────────────────────────────────────────────────────────────────────────────

/// A param supplied by an explicit ctor arg never evaluates its `default_expr` —
/// the arg is evaluated against the PARENT's `values` instead. Building that
/// param's dependency trace from the unused `default_expr` therefore invents
/// edges, and here the invented edge closes a 2-cycle: the sort omits both
/// cells, they fall into the declaration-order residue, and `p` evaluates
/// against a `child_values` that has no `q` yet — reproducing, for arg-supplied
/// instances, exactly the bug this task closes.
///
/// `q` is arg-supplied, so the only REAL edge is `p -> q`. Both cells must
/// resolve to Int(5).
#[test]
fn arg_supplied_param_contributes_no_default_expr_edge() {
    let mut engine = fresh_engine();
    let module = compile_source(
        "structure Pair { \
            param p : Int = q \
            param q : Int = p \
        } \
        structure PairHolder { \
            sub a = Pair(q: 5) \
        }",
    );

    let result = engine.eval(&module);

    assert_eq!(
        result.values.get(&ValueCellId::new("PairHolder.a", "q")),
        Some(&Value::Int(5)),
        "PairHolder.a.q comes from the ctor arg, evaluated in the parent's scope"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("PairHolder.a", "p")),
        Some(&Value::Int(5)),
        "PairHolder.a.p reads `q`, which the ctor arg supplies. An Undef here \
         means `q`'s unused `default_expr` contributed a phantom `q -> p` edge, \
         faking a cycle that dumped both cells into the declaration-order residue"
    );
}

/// The residue guard for the leg the dependency sort introduced.
///
/// `topological_sort` (Kahn) omits cycle members, so a topo-sort-only loop would
/// DELETE these two cells. They are committed as Undef today, and deleting a
/// committed cell is a worse member of the same stale-Undef family this task is
/// fixing — hence the declaration-order residue append.
///
/// NON-VACUITY: this test is green both before and after the residue append, so
/// it was verified by temporarily deleting the append from `phase1_walk`
/// (unfold/phase1_walk.rs) and observing this test RED (both cells absent),
/// then restoring it.
///
/// The cycle is reported exactly ONCE, by template scope. `unfold.rs` stays
/// silent deliberately: a second report would double-report a cycle template
/// scope already owns.
#[test]
fn cyclic_param_defaults_are_still_committed_at_instance_scope() {
    let mut engine = fresh_engine();
    let module = compile_source(
        "structure Cyc { \
            param a : Int = b \
            param b : Int = a \
        } \
        structure CycHolder { \
            sub s = Cyc() \
        }",
    );

    let result = engine.eval(&module);

    for member in ["a", "b"] {
        let instance_id = ValueCellId::new("CycHolder.s", member);
        assert_eq!(
            result.values.get(&instance_id),
            Some(&Value::Undef),
            "CycHolder.s.{member} must be PRESENT and Undef. `None` here means \
             the topological sort dropped the cycle member instead of appending \
             it to the residue, deleting a cell that was committed before the \
             dependency-ordering fix"
        );
    }

    let cycle_reports: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("circular dependency"))
        .collect();
    assert_eq!(
        cycle_reports.len(),
        1,
        "the param cycle must be reported exactly once, by template scope \
         (`circular dependency in template Cyc: [a, b]`). A second report means \
         `phase1_walk` grew its own diagnostic and is \
         double-reporting a cycle template scope already owns. Got: {cycle_reports:?}"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// #7511: a param default reading a sibling LET
// ──────────────────────────────────────────────────────────────────────────────

/// The #7511 repro. `p` reads the let `dbl`, which reads the later-declared
/// param `seed2`. Phase 1 must evaluate `dbl` before `p`'s default runs, even
/// though phase 2 is what commits `dbl`.
///
/// MEASURED RED on base b8e8c32a05: template `Mixed.p == Int(6)`, instance
/// `MixedHolder.a.p == Undef`, and `MixedHolder.a.dbl == Int(6)`, with no
/// diagnostic.
#[test]
fn param_default_reading_a_sibling_let_matches_template_at_instance_scope() {
    let mut engine = fresh_engine();
    let module = compile_source(
        "structure Mixed { \
            let dbl = seed2 * 2 \
            param p : Int = dbl \
            param seed2 : Int = 3 \
        } \
        structure MixedHolder { \
            sub a = Mixed() \
        }",
    );

    let result = engine.eval(&module);

    let template_p = ValueCellId::new("Mixed", "p");
    let instance_p = ValueCellId::new("MixedHolder.a", "p");

    assert_eq!(
        result.values.get(&template_p),
        Some(&Value::Int(6)),
        "template scope resolves a param default reading a sibling let (#4317)"
    );
    assert_eq!(
        result.values.get(&instance_p),
        result.values.get(&template_p),
        "instance scope must agree with template scope on a param default that \
         reads a sibling let"
    );
    assert_eq!(
        result.values.get(&instance_p),
        Some(&Value::Int(6)),
        "MixedHolder.a.p must be Int(6); Undef means phase 1 evaluated `p` \
         without first evaluating the let `dbl` it reads"
    );

    let snap = engine.snapshot().expect("snapshot after eval");
    let (p_val, p_det) = snap
        .values
        .get(&instance_p)
        .expect("snapshot must contain MixedHolder.a.p after eval");
    assert_eq!(
        *p_det,
        DeterminacyState::Determined,
        "MixedHolder.a.p must be Determined. Got: ({p_val:?}, {p_det:?})"
    );

    assert_eq!(
        result.values.get(&ValueCellId::new("MixedHolder.a", "dbl")),
        Some(&Value::Int(6)),
        "phase 2 still commits the let itself"
    );

    assert_no_error_diagnostics(&result, "a param default reading a sibling let");
}

/// Transitivity ACROSS KINDS: the chain alternates param -> let -> param -> let,
/// with the params declared in reverse dependency order. A fix that pulls in
/// only the lets a param reads directly, without the params those lets read in
/// turn, leaves `out` Undef.
///
/// Each let sits just above the param that reads it because the compiler types
/// a param initializer only against lets declared earlier.
#[test]
fn param_let_chain_in_reverse_declaration_order_resolves_at_instance_scope() {
    let mut engine = fresh_engine();
    let module = compile_source(
        "structure Zig { \
            let l2 = mid * 2 \
            param out : Int = l2 + 1 \
            let l1 = base * 10 \
            param mid : Int = l1 + 1 \
            param base : Int = 1 \
        } \
        structure ZigHolder { \
            sub z = Zig() \
        }",
    );

    let result = engine.eval(&module);

    for (member, expected) in [
        ("base", 1),
        ("l1", 10),
        ("mid", 11),
        ("l2", 22),
        ("out", 23),
    ] {
        let template_id = ValueCellId::new("Zig", member);
        let instance_id = ValueCellId::new("ZigHolder.z", member);
        assert_eq!(
            result.values.get(&template_id),
            Some(&Value::Int(expected)),
            "template scope must resolve the alternating chain at Zig.{member}"
        );
        assert_eq!(
            result.values.get(&instance_id),
            result.values.get(&template_id),
            "instance scope must agree with template scope at ZigHolder.z.{member}"
        );
        assert_eq!(
            result.values.get(&instance_id),
            Some(&Value::Int(expected)),
            "ZigHolder.z.{member} must be Int({expected})"
        );
    }

    assert_no_error_diagnostics(&result, "a reverse-declared param/let chain");
}

/// Deliberately NOT template agreement. `seed` is arg-supplied, so the scratch
/// `dbl` that `p` reads must be computed from THIS instance's `seed` (10), not
/// from the template default (3). `ArgHolder.a.p == Int(6)` would mean the
/// scratch let read the template's value.
#[test]
fn arg_supplied_param_reaches_a_param_default_through_a_sibling_let() {
    let mut engine = fresh_engine();
    let module = compile_source(
        "structure ArgMix { \
            param seed : Int = 3 \
            let dbl = seed * 2 \
            param p : Int = dbl \
        } \
        structure ArgHolder { \
            sub a = ArgMix(seed: 10) \
        }",
    );

    let result = engine.eval(&module);

    assert_eq!(
        result.values.get(&ValueCellId::new("ArgMix", "p")),
        Some(&Value::Int(6)),
        "template scope uses the default seed = 3"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("ArgHolder.a", "p")),
        Some(&Value::Int(20)),
        "ArgHolder.a.p must read dbl computed from the ctor-arg seed = 10. \
         Int(6) means the scratch let read the template's default seed"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("ArgHolder.a", "dbl")),
        Some(&Value::Int(20)),
        "phase 2 commits dbl from the ctor-arg seed"
    );

    assert_no_error_diagnostics(&result, "an arg-supplied param read through a let");
}

/// The phase-1 value must reach phase 1.5: a nested sub's ctor arg reads `p`,
/// which reads the let `dbl`.
///
/// MEASURED RED on base b8e8c32a05: template `Mid2.k.w == Int(6)`, instance
/// `Outer2.m.k.w == Undef`.
#[test]
fn nested_sub_ctor_arg_reading_a_let_dependent_param_matches_template() {
    let mut engine = fresh_engine();
    let module = compile_source(
        "structure Kid2 { \
            param w : Int = 1 \
        } \
        structure Mid2 { \
            let dbl = seed * 2 \
            param p : Int = dbl \
            param seed : Int = 3 \
            sub k = Kid2(w: p) \
        } \
        structure Outer2 { \
            sub m = Mid2() \
        }",
    );

    let result = engine.eval(&module);

    for (template_id, instance_id) in [
        (
            ValueCellId::new("Mid2", "p"),
            ValueCellId::new("Outer2.m", "p"),
        ),
        (
            ValueCellId::new("Mid2.k", "w"),
            ValueCellId::new("Outer2.m.k", "w"),
        ),
    ] {
        assert_eq!(
            result.values.get(&template_id),
            Some(&Value::Int(6)),
            "template scope must resolve {template_id:?}"
        );
        assert_eq!(
            result.values.get(&instance_id),
            result.values.get(&template_id),
            "instance {instance_id:?} must agree with template {template_id:?}"
        );
        assert_eq!(
            result.values.get(&instance_id),
            Some(&Value::Int(6)),
            "{instance_id:?} must be Int(6); Undef means the nested ctor arg \
             read a `p` that phase 1 left unresolved"
        );
    }

    assert_no_error_diagnostics(&result, "a nested ctor arg reading a let-dependent param");
}

/// The recursive path: `unfold_recursive_sub` calls `elaborate_child_params_only`
/// directly, and each level's `st` feeds the next level's ctor arg and guard.
///
/// MEASURED RED on base b8e8c32a05: `R.child.st` and `R.child.child.depth` are
/// Undef, so the Undef flowed into the next level's arg.
#[test]
fn recursive_level_param_reading_a_let_matches_template_and_feeds_the_next_level() {
    let mut engine = fresh_engine();
    let module = compile_source(
        "structure R { \
            param depth : Int = 2 \
            let next = 1 \
            param st : Int = next \
            sub child = R(depth: depth - st) where depth > 0 \
        }",
    );

    let result = engine.eval(&module);

    assert_eq!(
        result.values.get(&ValueCellId::new("R", "st")),
        Some(&Value::Int(1)),
        "template scope resolves R.st through the let `next`"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("R.child", "st")),
        result.values.get(&ValueCellId::new("R", "st")),
        "the first recursive level must agree with the template at st"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("R.child", "st")),
        Some(&Value::Int(1)),
        "R.child.st must be Int(1)"
    );
    assert_eq!(
        result
            .values
            .get(&ValueCellId::new("R.child.child", "depth")),
        Some(&Value::Int(0)),
        "R.child.child.depth is R.child's `depth - st` = 1 - 1. Undef means \
         R.child.st was unresolved when the next level's ctor arg ran"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("R.child.child", "st")),
        Some(&Value::Int(1)),
        "R.child.child.st must be Int(1)"
    );
    assert_eq!(
        result
            .values
            .get(&ValueCellId::new("R.child.child.child", "depth")),
        None,
        "the guard `depth > 0` is false at R.child.child, so recursion stops there"
    );

    assert_no_error_diagnostics(&result, "a recursive param reading a let");
}

// ──────────────────────────────────────────────────────────────────────────────
// amendment: the Auto-precedence branch under the dependency-ordered loop
// ──────────────────────────────────────────────────────────────────────────────

/// What a sibling read of an Auto-OVERRIDDEN param sees, made executable.
///
/// `derived` is declared BEFORE the `base` it reads, so the dependency order
/// visits `base` first — and `base` is the task-3806-γ case: the parent pushed
/// an `= auto` override, so the snapshot entry carries `DeterminacyState::Auto`
/// and the loop takes the Auto-precedence branch. That branch does NOT skip
/// `child_values` entirely: it inserts the snapshot's `Undef` placeholder before
/// its `continue`, so `derived`'s default reads `Some(Undef)` — not `None`, and
/// not the template's evaluated `6mm`.
///
/// MEASURED 2026-09-14: instance `base` is `(Undef, Auto)` in the snapshot and
/// `derived` commits `Undef`, while template `AutoInner.derived` is `6mm`. The
/// Auto state reaching the solver as `(Undef, Auto)` is what the ordering change
/// had to leave undisturbed, so it is asserted here rather than described.
#[test]
fn sibling_read_of_an_auto_overridden_param_sees_the_solver_placeholder() {
    let mut engine = fresh_engine();
    let module = compile_source(
        "structure AutoInner { \
            param derived : Length = base + 1mm \
            param base : Length = 5mm \
        } \
        structure AutoOuter { \
            sub b : AutoInner { base = auto } \
            constraint self.b.base == 10mm \
        }",
    );

    let result = engine.eval(&module);
    let snap = engine.snapshot().expect("snapshot after eval");

    assert_eq!(
        snap.values.get(&ValueCellId::new("AutoOuter.b", "base")),
        Some(&(Value::Undef, DeterminacyState::Auto)),
        "the parent's `= auto` override must survive the dependency-ordered \
         param loop as the (Undef, Auto) pair `build_solver_problem` consumes. \
         Anything else means the reorder let the child default overwrite it"
    );

    assert_eq!(
        result
            .values
            .get(&ValueCellId::new("AutoOuter.b", "derived")),
        Some(&Value::Undef),
        "`derived` reads an Auto-overridden sibling, so it sees the `Some(Undef)` \
         placeholder the Auto branch inserted. A `None` read and this placeholder \
         both yield Undef here, but they are different inputs to the `@optimized` \
         reuse gate's read comparison, which is why the branch inserts at all"
    );
    let template_derived = result
        .values
        .get(&ValueCellId::new("AutoInner", "derived"))
        .expect("template AutoInner.derived must exist");
    assert!(
        matches!(template_derived, Value::Scalar { si_value, .. } if *si_value == 0.006),
        "template scope has no Auto override, so it resolves `derived` to 6mm — \
         which is what makes the instance Undef above the OVERRIDE's consequence \
         and not an ordering failure. Got: {template_derived:?}"
    );

    assert_no_error_diagnostics(&result, "an Auto-overridden param read by a sibling");
}

// ──────────────────────────────────────────────────────────────────────────────
// shared assertion
// ──────────────────────────────────────────────────────────────────────────────

fn assert_no_error_diagnostics(result: &reify_eval::EvalResult, what: &str) {
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "eval must produce no Error diagnostics for {what}; got: {errors:?}"
    );
}
