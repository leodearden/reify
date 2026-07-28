//! Task 5360 — instance-nested sub-component elaboration.
//!
//! A `let` that reads across a sub boundary (`self.<sub>.<member>`) resolves
//! fine at TEMPLATE scope, because the top-level sub-elaboration loop in
//! `engine_eval.rs` elaborates every plain sub of every template. It does NOT
//! resolve at INSTANCE scope: `elaborate_child_instance` elaborates only the
//! child's own params + lets, never recursing into the child template's own
//! `sub_components`. So for `Parent { sub m = Mid() }` where `Mid` itself
//! declares `sub k = Kid()`, the grandchild entity `Parent.m.k` is never
//! materialised and `Parent.m.relay` / `Parent.echo` silently become `Undef`
//! with zero diagnostics.
//!
//! These tests pin the chained cross-sub read at instance scope (t1), the
//! constructor-arg threading through the new nesting recursion (t2), and the
//! never-silent-undef diagnostic for deliberately-unsupported nesting (t3).
//!
//! Round 2 (t4) covers the second half of the arg-threading story: an arg that
//! reads something which is NOT a param of the child template — a `let` of the
//! child, a sibling sub's member, or a let chained off an earlier sub. Phase 1
//! only populates params, so every such read used to fall through to the
//! TEMPLATE-scope entry in the global map and silently substitute the template
//! default for the instance value.

#![allow(clippy::mutable_key_type)]

use reify_core::{Diagnostic, ModulePath, Severity, Type, ValueCellId};
use reify_ir::{BinOp, CompiledExpr, Value};
use reify_test_support::builders::{binop, gt, literal, value_ref_typed};
use reify_test_support::{
    CompiledModuleBuilder, TopologyTemplateBuilder, make_simple_engine,
    parse_and_compile_with_stdlib,
};

/// Tolerance for SI-value comparisons (all fixtures below are exact binary
/// fractions of a metre scaled by small integers, so this is generous).
const EPS: f64 = 1e-12;

/// Assert that `id` is present in `values` and holds a `Scalar` whose
/// `si_value` matches `expected`, with a message naming the cell.
fn assert_scalar_si(values: &reify_ir::ValueMap, entity: &str, member: &str, expected: f64) {
    let id = ValueCellId::new(entity, member);
    let got = values
        .get(&id)
        .unwrap_or_else(|| panic!("cell {entity}.{member} is absent from the values map"));
    match got {
        Value::Scalar { si_value, .. } => assert!(
            (si_value - expected).abs() < EPS,
            "cell {entity}.{member}: expected si_value {expected}, got {si_value}",
        ),
        other => panic!("cell {entity}.{member}: expected Value::Scalar({expected}), got {other:?}"),
    }
}

/// Assert the eval produced no `Severity::Error` diagnostics.
fn assert_no_error_diagnostics(diagnostics: &[Diagnostic], what: &str) {
    let errors: Vec<&Diagnostic> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "{what}: expected no error diagnostics, got: {errors:?}",
    );
}

/// (a) The exact two-level repro from the task description.
///
/// `Parent` reads `self.m.relay`; `Mid` reads `self.k.off`. At template scope
/// `Mid.relay` resolves (20mm) — the achievability basis for this acceptance
/// value. At instance scope the chain must resolve identically: the grandchild
/// instance `Parent.m.k` must exist and carry `off = 20mm`, `Parent.m.relay`
/// must relay it, and `Parent.echo` must echo it — with no diagnostics.
#[test]
fn two_level_chain_resolves() {
    const SOURCE: &str = r#"
structure def Kid {
    param w : Length = 10mm
    let off = w * 2.0
}

structure def Mid {
    sub k = Kid()
    let relay = self.k.off
}

structure def Parent {
    sub m = Mid()
    let echo = self.m.relay
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // Template scope already works today — pinned as the control.
    assert_scalar_si(&result.values, "Mid.k", "off", 0.02);
    assert_scalar_si(&result.values, "Mid", "relay", 0.02);

    // Instance scope: the grandchild entity must be elaborated, and both
    // cross-sub reads above it must resolve to the same 20mm.
    assert_scalar_si(&result.values, "Parent.m.k", "off", 0.02);
    assert_scalar_si(&result.values, "Parent.m", "relay", 0.02);
    assert_scalar_si(&result.values, "Parent", "echo", 0.02);

    assert_no_error_diagnostics(&result.diagnostics, "two_level_chain_resolves");
}

/// (b) The three-level (DriveTendons-shaped) chain: a single source value
/// relayed up through three nested sub boundaries.
///
/// Depth-general behaviour is the point — the fix must recurse, not
/// special-case one extra level. `L3.z.y.x` is the deepest instance entity;
/// `L3.d` is the top of the chain.
#[test]
fn three_level_chain_resolves() {
    const SOURCE: &str = r#"
structure def L0 {
    param w : Length = 10mm
    let a = w * 2.0
}

structure def L1 {
    sub x = L0()
    let b = self.x.a
}

structure def L2 {
    sub y = L1()
    let c = self.y.b
}

structure def L3 {
    sub z = L2()
    let d = self.z.c
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // One level already works today — the control that isolates the defect
    // to depth >= 2.
    assert_scalar_si(&result.values, "L1", "b", 0.02);

    // The deepest instance entity under L3 must exist, and every relay hop
    // above it must carry the same derived 20mm.
    assert_scalar_si(&result.values, "L3.z.y.x", "a", 0.02);
    assert_scalar_si(&result.values, "L3.z.y", "b", 0.02);
    assert_scalar_si(&result.values, "L3.z", "c", 0.02);
    assert_scalar_si(&result.values, "L3", "d", 0.02);

    assert_no_error_diagnostics(&result.diagnostics, "three_level_chain_resolves");
}

/// A parent override must thread through the nesting recursion into a nested
/// sub's constructor arg.
///
/// `Parent` overrides `Mid.scale` to 30mm; `Mid` forwards it to its own nested
/// sub as `Kid(w: scale)`. The arg expression is compiled in `Mid`'s scope, so
/// it references the cell `Mid.scale`. Evaluating it against the GLOBAL values
/// map reads the *template default* (1mm) and silently produces `w = 1mm` /
/// `echo = 2mm` — a wrong value, not an `Undef`, so nothing else would catch
/// it. Evaluating it against the instance's own `child_values` (which carries
/// `Parent.m.scale = 30mm` under the template-scoped key `Mid.scale`) gives the
/// correct 30mm / 60mm.
#[test]
fn nested_constructor_arg_threads_through_two_levels() {
    const SOURCE: &str = r#"
structure def Kid {
    param w : Length = 10mm
    let off = w * 2.0
}

structure def Mid {
    param scale : Length = 1mm
    sub k = Kid(w: scale)
    let relay = self.k.off
}

structure def Parent {
    sub m = Mid(scale: 30mm)
    let echo = self.m.relay
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // Control: the one-level override already lands on the instance's own
    // param cell today. This is the value the nested arg must be evaluated
    // against — the template default `Mid.scale` is 1mm.
    assert_scalar_si(&result.values, "Parent.m", "scale", 0.03);
    assert_scalar_si(&result.values, "Mid", "scale", 0.001);

    // The override must reach the nested sub's own param and everything
    // derived from it: 30mm -> off 60mm -> relay 60mm -> echo 60mm.
    assert_scalar_si(&result.values, "Parent.m.k", "w", 0.03);
    assert_scalar_si(&result.values, "Parent.m.k", "off", 0.06);
    assert_scalar_si(&result.values, "Parent.m", "relay", 0.06);
    assert_scalar_si(&result.values, "Parent", "echo", 0.06);

    assert_no_error_diagnostics(
        &result.diagnostics,
        "nested_constructor_arg_threads_through_two_levels",
    );
}

/// (t3) The nesting recursion deliberately does NOT descend into every sub
/// shape — collection, keyed and guarded subs, unknown structures, and cyclic
/// nesting cuts are all skipped. When a skipped sub is the thing an instance
/// `let` reads across, that `let` cannot resolve; it must say so rather than
/// resolve to `Value::Undef` in silence.
///
/// Fixture (hand-built IR, so the skipped shape is deterministic): `Rec` is a
/// recursive template with a GUARDED self-sub `child` and a let `echo_child`
/// carrying the `self.child.n` lowering — a `ValueRef` on the scoped cell
/// `Rec.child.n`. `Parent` wraps it in a PLAIN sub `m = Rec()` and reads
/// nothing itself.
///
/// At TEMPLATE scope this works: `Rec` is recursive, so `unfold_recursive_sub`
/// materialises `Rec.child` and `Rec.echo_child` reads it. At INSTANCE scope
/// `elaborate_child_instance` skips the guarded `child` (guards are only
/// meaningful in the recursive context `unfold_recursive_sub` owns at template
/// scope), so `Parent.m.child` never exists and `Parent.m.echo_child` has
/// nothing to read.
#[test]
fn unsupported_nested_read_emits_diagnostic_not_silent_undef() {
    // guard: n > 0  (references Rec.n)
    let guard = gt(
        value_ref_typed("Rec", "n", Type::Int),
        literal(Value::Int(0)),
    );
    // arg: n = n - 1  (references Rec.n)
    let n_minus_1 = binop(
        BinOp::Sub,
        value_ref_typed("Rec", "n", Type::Int),
        literal(Value::Int(1)),
    );

    let rec = TopologyTemplateBuilder::new("Rec")
        .param(
            "Rec",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(1), Type::Int)),
        )
        .is_recursive(true)
        .sub_component_with_guard("child", "Rec", vec![("n".to_string(), n_minus_1)], guard)
        // `self.child.n` lowers to a ValueRef on the scoped cell Rec.child.n.
        .let_binding(
            "Rec",
            "echo_child",
            Type::Int,
            value_ref_typed("Rec.child", "n", Type::Int),
        )
        .build();

    let parent = TopologyTemplateBuilder::new("Parent")
        .sub_component("m", "Rec", vec![])
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(rec)
        .template(parent)
        .build();
    let mut engine = make_simple_engine();
    let result = engine.eval(&module);

    // The instance itself IS elaborated — its own param lands.
    assert_eq!(
        result.values.get(&ValueCellId::new("Parent.m", "n")),
        Some(&Value::Int(1)),
        "Parent.m.n should be the Rec param default (proves the instance was elaborated)",
    );

    // The cross-sub read cannot resolve: the guarded `child` is skipped at
    // instance scope, so `Parent.m.child` was never materialised.
    let nested_n = ValueCellId::new("Parent.m.child", "n");
    assert!(
        !result.values.contains(&nested_n),
        "Parent.m.child.n must not exist (a guarded sub is not elaborated at instance scope)",
    );
    let instance_let = ValueCellId::new("Parent.m", "echo_child");
    assert_eq!(
        result.values.get(&instance_let),
        Some(&Value::Undef),
        "Parent.m.echo_child is expected to be Undef — the point of this test is that it must \
         not be SILENTLY so",
    );

    // The never-silent-undef contract: an error diagnostic must name both the
    // undefined instance let and the nested cell it could not read.
    let errors: Vec<&Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("Parent.m.echo_child")
                && d.message.contains("Parent.m.child.n")),
        "expected an error diagnostic naming both the unresolvable instance let \
         \"Parent.m.echo_child\" and the nested cell \"Parent.m.child.n\" it reads; got: {errors:?}",
    );
}

/// (t4a) A nested sub's constructor arg that reads a `let` of the child
/// template must see the INSTANCE's let value, not the template default.
///
/// `Mid.half` is derived from the overridden param (`scale = 30mm` -> `half =
/// 15mm`), and `sub k = Kid(w: half)` forwards it. Phase 1 of the instance
/// elaboration populates params only, so `half` is absent from the arg scope
/// and the read falls through to the global map — where `Mid.half` holds the
/// TEMPLATE default (0.5mm). The instance's own `half` IS computed correctly,
/// just later (phase 2), so nothing downstream notices the substitution: the
/// nested param lands wrong rather than `Undef`.
#[test]
fn nested_arg_reading_child_let_uses_instance_value() {
    const SOURCE: &str = r#"
structure def Kid {
    param w : Length = 10mm
    let off = w * 2.0
}

structure def Mid {
    param scale : Length = 1mm
    let half = scale * 0.5
    sub k = Kid(w: half)
    let relay = self.k.off
}

structure def Parent {
    sub m = Mid(scale: 30mm)
    let echo = self.m.relay
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // Control: template scope already resolves this chain against the default
    // (scale 1mm -> half 0.5mm -> w 0.5mm -> off 1mm). That correct row is the
    // exact value wrongly reused at instance scope today.
    assert_scalar_si(&result.values, "Mid", "half", 0.0005);
    assert_scalar_si(&result.values, "Mid.k", "w", 0.0005);

    // Control: the instance's own derived let is right (computed in phase 2).
    assert_scalar_si(&result.values, "Parent.m", "half", 0.015);

    // The nested sub's arg must be evaluated against THAT 15mm, not Mid.half.
    assert_scalar_si(&result.values, "Parent.m.k", "w", 0.015);
    assert_scalar_si(&result.values, "Parent.m.k", "off", 0.03);
    assert_scalar_si(&result.values, "Parent.m", "relay", 0.03);
    assert_scalar_si(&result.values, "Parent", "echo", 0.03);

    assert_no_error_diagnostics(
        &result.diagnostics,
        "nested_arg_reading_child_let_uses_instance_value",
    );
}

/// (t4b) A nested sub's constructor arg that reads a SIBLING sub's member must
/// see the sibling's instance value.
///
/// `Mid` declares `sub a = A(p: s)` and then `sub b = B(w: self.a.out)`. The
/// sibling `a` IS elaborated first at instance scope (declaration order happens
/// to agree with dependency order here), and `Parent.m.a.out` is correct — but
/// its cells are never projected into the arg scope, so `self.a.out` resolves
/// from the global map's TEMPLATE-scope `Mid.a.out` instead.
#[test]
fn nested_arg_reading_sibling_sub_member_uses_instance_value() {
    const SOURCE: &str = r#"
structure def A {
    param p : Length = 2mm
    let out = p * 3.0
}

structure def B {
    param w : Length = 1mm
    let res = w * 5.0
}

structure def Mid {
    param s : Length = 1mm
    sub a = A(p: s)
    sub b = B(w: self.a.out)
    let relay = self.b.res
}

structure def Parent {
    sub m = Mid(s: 10mm)
    let echo = self.m.relay
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // Control: the template-scope sibling chain (s 1mm -> a.out 3mm -> b.w
    // 3mm -> b.res 15mm) is the row silently reused at instance scope today.
    assert_scalar_si(&result.values, "Mid.a", "out", 0.003);
    assert_scalar_si(&result.values, "Mid.b", "w", 0.003);

    // Control: the sibling instance is already elaborated correctly — the gap
    // is purely that it is not visible to the consuming sub's arg.
    assert_scalar_si(&result.values, "Parent.m.a", "out", 0.03);

    assert_scalar_si(&result.values, "Parent.m.b", "w", 0.03);
    assert_scalar_si(&result.values, "Parent.m.b", "res", 0.15);
    assert_scalar_si(&result.values, "Parent.m", "relay", 0.15);
    assert_scalar_si(&result.values, "Parent", "echo", 0.15);

    assert_no_error_diagnostics(
        &result.diagnostics,
        "nested_arg_reading_sibling_sub_member_uses_instance_value",
    );
}

/// (t4c) The two shapes chained: a `let` that reads an earlier nested sub, feeding
/// a LATER nested sub's arg. Resolving this needs the child's lets and its subs
/// interleaved in one dependency order (k -> via -> j), not two separate passes.
#[test]
fn nested_arg_reading_let_derived_from_earlier_sub() {
    const SOURCE: &str = r#"
structure def Kid {
    param w : Length = 10mm
    let off = w * 2.0
}

structure def Mid {
    param s : Length = 1mm
    sub k = Kid(w: s)
    let via = self.k.off
    sub j = Kid(w: via)
    let relay = self.j.off
}

structure def Parent {
    sub m = Mid(s: 7mm)
    let echo = self.m.relay
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // Control: the first hop is already right at instance scope (its arg reads
    // a plain param), and `via` derives from it correctly in phase 2.
    assert_scalar_si(&result.values, "Parent.m.k", "w", 0.007);
    assert_scalar_si(&result.values, "Parent.m.k", "off", 0.014);
    assert_scalar_si(&result.values, "Parent.m", "via", 0.014);

    // The second hop consumes `via`; today it reads the template-scope
    // `Mid.via` (0.002) instead. Note template scope leaves `Mid.j.w` `Undef`
    // — the top-level sub loop in engine_eval.rs evaluates subs before lets, a
    // separate ordering gap not in this task's scope, hence no assertion on it.
    assert_scalar_si(&result.values, "Parent.m.j", "w", 0.014);
    assert_scalar_si(&result.values, "Parent.m.j", "off", 0.028);
    assert_scalar_si(&result.values, "Parent.m", "relay", 0.028);
    assert_scalar_si(&result.values, "Parent", "echo", 0.028);

    assert_no_error_diagnostics(
        &result.diagnostics,
        "nested_arg_reading_let_derived_from_earlier_sub",
    );
}

/// (t4d) Byte-identical to (t4b) except `sub b` is declared BEFORE the `sub a`
/// it reads. Pins that the ordering is derived from the dependency graph, not
/// from declaration order — a declaration-order loop that merely projected each
/// sub as it went would pass (t4b) and still fail here.
#[test]
fn nested_arg_sibling_reference_is_order_insensitive() {
    const SOURCE: &str = r#"
structure def A {
    param p : Length = 2mm
    let out = p * 3.0
}

structure def B {
    param w : Length = 1mm
    let res = w * 5.0
}

structure def Mid {
    param s : Length = 1mm
    sub b = B(w: self.a.out)
    sub a = A(p: s)
    let relay = self.b.res
}

structure def Parent {
    sub m = Mid(s: 10mm)
    let echo = self.m.relay
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // Control: the forward reference already leaves `Mid.b.w` `Undef` at
    // TEMPLATE scope (declaration-order elaboration there), so only the
    // instance rows are asserted — the instance path must not depend on order.
    assert_scalar_si(&result.values, "Parent.m.a", "out", 0.03);

    assert_scalar_si(&result.values, "Parent.m.b", "w", 0.03);
    assert_scalar_si(&result.values, "Parent.m.b", "res", 0.15);
    assert_scalar_si(&result.values, "Parent.m", "relay", 0.15);
    assert_scalar_si(&result.values, "Parent", "echo", 0.15);

    assert_no_error_diagnostics(
        &result.diagnostics,
        "nested_arg_sibling_reference_is_order_insensitive",
    );
}

/// (t5) A phase-1.5 dependency cycle routed THROUGH a nested sub must be
/// diagnosed, not left as silent `Undef`.
///
/// `Mid.relay` reads `self.k.off` while `sub k`'s constructor arg reads
/// `relay` — a cycle between a LET node and a SUB node in phase 1.5's
/// dependency graph. `topological_sort` (Kahn) silently omits cycle members,
/// and the walk then appends them back in declaration order so evaluation
/// terminates. That keeps values flowing but says nothing, so the whole chain
/// lands `Undef` with zero diagnostics.
///
/// The existing let-only detector in `elaborate_child_lets_only` cannot cover
/// this: its graph holds only `Mid`'s own `let` nodes, and this cycle routes
/// through a SUB node (`relay` reads `Mid.k.off`, which is not a let of `Mid`).
/// So phase 1.5 is the only place that can see it.
#[test]
fn nested_sub_arg_let_cycle_is_diagnosed_not_silent_undef() {
    const SOURCE: &str = r#"
structure def Kid {
    param v : Length = 1mm
    let off = v * 2.0
}

structure def Mid {
    sub k = Kid(v: relay)
    let relay = self.k.off
}

structure def Parent {
    sub m = Mid()
    let echo = self.m.relay
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let errors: Vec<&Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    // Substring matching, not full-message equality, so the exact prose stays
    // free to change: the contract is that the cycle is NAMED, at instance
    // scope, with both participants identified.
    assert!(
        errors.iter().any(|d| {
            let m = &d.message;
            (m.contains("circular") || m.contains("cyclic"))
                && m.contains("Parent.m")
                && m.contains("relay")
                // `sub k`, not a bare `k`: PHASE15_SUB_NODE_MEMBER is the EMPTY
                // string, so rendering a sub node the way the let-only detector
                // renders a let would print a dangling dot naming no cell.
                && m.contains("sub k")
        }),
        "expected an error diagnostic identifying a circular dependency at instance scope \
         \"Parent.m\" and naming both cycle participants — the let \"relay\" and the sub \
         \"k\"; got: {errors:?}",
    );
}
