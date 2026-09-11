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

/// Assert that `entity.member` holds a `Value::StructureInstance` carrying a
/// `Scalar` field `field` whose `si_value` matches `expected`.
///
/// Checks the collapsed structure value (SIR-α / the `__self` alias), not the
/// per-member scoped cells [`assert_scalar_si`] covers — the two are separate
/// commits and only the latter existed at instance scope before review #1.
fn assert_si_field_scalar(
    values: &reify_ir::ValueMap,
    entity: &str,
    member: &str,
    field: &str,
    expected: f64,
) {
    let id = ValueCellId::new(entity, member);
    let got = values
        .get(&id)
        .unwrap_or_else(|| panic!("cell {entity}.{member} is absent from the values map"));
    let Value::StructureInstance(data) = got else {
        panic!("cell {entity}.{member}: expected Value::StructureInstance, got {got:?}");
    };
    let field_val = data
        .fields
        .get(field)
        .unwrap_or_else(|| panic!("cell {entity}.{member}: instance has no field \"{field}\""));
    match field_val {
        Value::Scalar { si_value, .. } => assert!(
            (si_value - expected).abs() < EPS,
            "cell {entity}.{member}.{field}: expected si_value {expected}, got {si_value}",
        ),
        other => panic!(
            "cell {entity}.{member}.{field}: expected Value::Scalar({expected}), got {other:?}"
        ),
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

/// (t6) A nested PURE-let cycle must be reported once per scope — phase 1.5
/// must not pile a third diagnostic onto a defect that already has two owners.
///
/// `Mid`'s `a`/`b` cycle involves no sub at all, so it is already reported
/// twice: once at TEMPLATE scope by `engine_eval.rs`, once at INSTANCE scope by
/// `elaborate_child_lets_only`. Phase 1.5's node set includes every `let` with a
/// `default_expr`, so this cycle is dropped by BOTH topological sorts — and i5's
/// unconditional emission therefore adds a THIRD error, a SECOND one naming
/// `Parent.m`.
///
/// This pins "one owner per scope", not "suppress reporting": the instance-scope
/// count must be exactly 1 (a genuine duplicate-detector, not an `any()`
/// existence check), AND the template-scope diagnostic must still be present.
#[test]
fn nested_pure_let_cycle_is_reported_once_per_scope() {
    const SOURCE: &str = r#"
structure def Mid {
    let a = b * 2.0
    let b = a * 2.0
}

structure def Parent {
    sub m = Mid()
    let echo = self.m.a
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

    // Instance scope: exactly one owner. `elaborate_child_lets_only` already
    // covers a let-only cycle at instance scope, and phase 1.5 sees the very
    // same nodes, so an ungated phase-1.5 diagnostic double-reports it.
    let instance_scoped: Vec<&&Diagnostic> = errors
        .iter()
        .filter(|d| {
            let m = &d.message;
            (m.contains("circular") || m.contains("cyclic")) && m.contains("Parent.m")
        })
        .collect();
    assert_eq!(
        instance_scoped.len(),
        1,
        "a pure-let cycle must be reported exactly once at instance scope \
         \"Parent.m\"; got {} such errors: {instance_scoped:?}",
        instance_scoped.len(),
    );

    // Template scope must still fire, unchanged — this test pins one owner per
    // scope, never the removal of reporting.
    assert!(
        errors.iter().any(|d| {
            let m = &d.message;
            (m.contains("circular") || m.contains("cyclic"))
                && m.contains("template Mid")
                && !m.contains("Parent.m")
        }),
        "the template-scope cycle diagnostic for Mid must survive unchanged; got: {errors:?}",
    );
}

/// (t8a) The phase-1.5 cycle GATE must key on actual cycle membership, not on
/// "was omitted by Kahn".
///
/// `topological_sort` emits a node only once its in-degree reaches 0. A node
/// whose dependency sits in a cycle never sees that dependency placed, so it is
/// never emitted either — recursively. The omitted set is therefore a strict
/// SUPERSET of the cycle: it also holds everything TRANSITIVELY DOWNSTREAM.
///
/// Here the only real cycle is the pure-let `a <-> b`. `sub k` merely reads `a`,
/// and `tail` merely reads `self.k.off` — one hop further, to pin that the
/// over-approximation is transitive rather than one-deep. Gating on the omitted
/// set makes `touches_sub` fire anyway, so the "one owner per cycle class"
/// invariant that t6 pins is broken by a downstream sub: this fixture emits TWO
/// instance-scope errors naming `Parent.m` (phase 1.5's plus
/// `elaborate_child_lets_only`'s) where t6's sub-free variant emits one.
#[test]
fn nested_pure_let_cycle_with_downstream_sub_is_reported_once_per_scope() {
    const SOURCE: &str = r#"
structure def Kid {
    param w : Length = 1mm
    let off = w * 2.0
}

structure def Mid {
    let a = b * 2.0
    let b = a * 2.0
    sub k = Kid(w: a)
    let tail = self.k.off * 3.0
}

structure def Parent {
    sub m = Mid()
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

    // Same assertion shape as t6, so the two read as a pair: a pure-let cycle
    // has exactly one instance-scope owner, whether or not something downstream
    // of it happens to be a sub.
    let instance_scoped: Vec<&&Diagnostic> = errors
        .iter()
        .filter(|d| {
            let m = &d.message;
            (m.contains("circular") || m.contains("cyclic")) && m.contains("Parent.m")
        })
        .collect();
    assert_eq!(
        instance_scoped.len(),
        1,
        "a pure-let cycle with a merely-downstream sub must still be reported exactly \
         once at instance scope \"Parent.m\"; got {} such errors: {instance_scoped:?}",
        instance_scoped.len(),
    );

    // Precision, not just count: the survivor must name the real participants
    // only. `sub k` and `tail` are downstream of the cycle, not on it.
    let message = &instance_scoped[0].message;
    assert!(
        !message.contains("sub k"),
        "the instance-scope cycle diagnostic must not name \"sub k\", which is merely \
         downstream of the a<->b cycle; got: {message}",
    );
    assert!(
        !message.contains("tail"),
        "the instance-scope cycle diagnostic must not name \"tail\", which is merely \
         downstream of the a<->b cycle; got: {message}",
    );

    // Template scope must still fire. Deliberately an existence check, not a
    // count: this fixture draws TWO template-scope errors from two distinct
    // pre-existing emitters in `engine_eval.rs`, neither touched by this task.
    assert!(
        errors.iter().any(|d| {
            let m = &d.message;
            (m.contains("circular") || m.contains("cyclic"))
                && m.contains("template Mid")
                && !m.contains("Parent.m")
        }),
        "the template-scope cycle diagnostic for Mid must survive unchanged; got: {errors:?}",
    );
}

/// (t8b) The phase-1.5 cycle diagnostic's PARTICIPANT LIST must name only nodes
/// actually on the cycle.
///
/// The counterpart to t8a: here the cycle `relay <-> sub k` is genuine and does
/// contain a sub, so phase 1.5 still owns it and the diagnostic must still fire
/// — proving the fix TRIMS the participant list rather than merely suppressing
/// the diagnostic. `tail` reads `relay` and so is dropped by Kahn too, but it is
/// downstream, not a participant. A diagnostic whose whole job is to NAME the
/// unresolvable cells must not name innocents.
#[test]
fn nested_sub_cycle_diagnostic_names_only_real_participants() {
    const SOURCE: &str = r#"
structure def Kid {
    param v : Length = 1mm
    let off = v * 2.0
}

structure def Mid {
    sub k = Kid(v: relay)
    let relay = self.k.off
    let tail = relay * 3.0
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

    let instance_scoped: Vec<&&Diagnostic> = errors
        .iter()
        .filter(|d| {
            let m = &d.message;
            (m.contains("circular") || m.contains("cyclic")) && m.contains("Parent.m")
        })
        .collect();
    assert!(
        !instance_scoped.is_empty(),
        "a genuine cycle through a nested sub must still be diagnosed at instance scope \
         \"Parent.m\" — trimming the participant list must not silence it; got: {errors:?}",
    );

    assert!(
        instance_scoped
            .iter()
            .any(|d| d.message.contains("relay") && d.message.contains("sub k")),
        "the instance-scope cycle diagnostic must still name both real participants — the \
         let \"relay\" and the sub \"k\"; got: {instance_scoped:?}",
    );
    for d in &instance_scoped {
        assert!(
            !d.message.contains("tail"),
            "the instance-scope cycle diagnostic must not name \"tail\", which reads \
             \"relay\" but lies on no cycle; got: {}",
            d.message,
        );
    }
}

/// esc-5360-9 (review): a nested sub whose target structure lives in the STDLIB
/// PRELUDE must be elaborated at instance scope too.
///
/// Stdlib templates live in `Engine::prelude` and are deliberately NOT merged
/// into `CompiledModule::templates` (io-export δ / esc-4287-15). Phase 1.5
/// originally resolved a nested sub with a bare module-only `find_template`,
/// while the top-level sub-elaboration loop in `engine_eval.rs` resolves with
/// the prelude fallback. Two distinct defects followed from that asymmetry, and
/// this test pins both:
///
///  1. `Mid.tol` elaborated at template scope but `Parent.m.tol` did not —
///     re-opening, for every prelude-typed sub, the exact instance/template gap
///     this task exists to close. So `Parent.m.relay` / `Parent.echo` went
///     silently `Undef`.
///  2. The skip was recorded as "target structure not found in this module",
///     so `report_unresolvable_nested_reads` raised a NEW error naming a
///     structure that is in fact perfectly resolvable — a false positive where
///     the pre-change behaviour had at least been silent.
///
/// `DimensionalTolerance` (`stdlib/tolerancing.ri`) is the fixture type because
/// it carries its OWN derived let (`tolerance_band = upper_deviation -
/// lower_deviation`). The chain therefore exercises the full path — a prelude
/// template's params AND its lets must be elaborated at nested instance scope,
/// not just the entity's existence.
///
/// SCOPE NOTE (historical) — why this test stops at the nested entity's own
/// cells and does not also assert a relaying `let relay =
/// self.tol.tolerance_band` on `Mid`. When it was written, such a let WAS typed
/// `Geometry` by the compiler and evaluated to a `GeometryHandle`, not the
/// `Scalar` the member holds; that was a separate COMPILER-side gap which bit
/// identically at TEMPLATE scope (`Mid.relay`), so this eval-side change could
/// not have fixed it — measured at the time by re-running this fixture with the
/// `unfold.rs`/`engine_eval.rs` changes stashed: byte-identical failure.
/// Asserting it here would have pinned an unrelated defect to this test.
///
/// That gap is now CLOSED, by #5867: compile-side sub-target template
/// resolution is module-first with a prelude fallback
/// (`reify_compiler::types::find_template_with_prelude`), the mirror of
/// `unfold::find_template_in_scope` on this side. The relay assertion it made
/// impossible now lives in
/// [`prelude_typed_sub_member_read_relays_at_both_scopes`] below. This test's
/// own scope is deliberately unchanged — it remains #5360's regression pin for
/// the nested entity's existence and elaboration.
#[test]
fn prelude_typed_nested_sub_resolves_at_instance_scope() {
    // tolerance_band = 2mm - 1mm = 1mm, on a sub two levels down.
    const SOURCE: &str = r#"
structure def Mid {
    sub tol = DimensionalTolerance(nominal: 10mm, upper_deviation: 2mm, lower_deviation: 1mm)
}

structure def Parent {
    sub m = Mid()
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // Control: template scope elaborates the prelude-typed sub today — this is
    // the achievability basis for the instance-scope expectation below.
    assert_scalar_si(&result.values, "Mid.tol", "tolerance_band", 0.001);
    assert_scalar_si(&result.values, "Mid.tol", "nominal", 0.010);

    // (1) The nested prelude-typed entity must be materialised at instance
    // scope too — its constructor-threaded params AND its own derived let.
    // Pre-fix, the module-only lookup skipped it and these cells were absent.
    assert_scalar_si(&result.values, "Parent.m.tol", "nominal", 0.010);
    assert_scalar_si(&result.values, "Parent.m.tol", "tolerance_band", 0.001);

    // (2) No false-positive "not found" error for a name that IS resolvable.
    let errors: Vec<&Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors
            .iter()
            .any(|d| d.message.contains("not found") || d.message.contains("unknown structure")),
        "a prelude-typed nested sub must not be reported as unresolvable — the \
         module-only lookup is the bug; got: {errors:?}",
    );
    assert_no_error_diagnostics(
        &result.diagnostics,
        "prelude_typed_nested_sub_resolves_at_instance_scope",
    );
}

/// Task #5867 ACCEPTANCE — a `let` that RELAYS a member of a PRELUDE-typed sub
/// must carry the member's VALUE, at template scope AND at instance scope.
///
/// This is the half [`prelude_typed_nested_sub_resolves_at_instance_scope`]
/// deliberately did not assert: that test pinned the nested prelude entity's
/// OWN cells (`Mid.tol.tolerance_band`), because a relaying `let relay =
/// self.tol.tolerance_band` was at the time typed `Geometry` by the compiler
/// and evaluated to a `GeometryHandle { kernel_handle: None }` — a compiler-side
/// defect that bit identically at template scope and so was out of scope for
/// #5360's eval-side change.
///
/// The compiler-side fix is `types::find_template_with_prelude`: sub-target
/// template resolution is module-first with a prelude fallback, mirroring
/// `unfold::find_template_in_scope` on the eval side. No eval-side change was
/// needed — #5360's instance-scope elaboration already handles the scoped-id
/// rewrite once the compiler emits a value cell instead of a realization.
///
/// `tolerance_band = 2mm − 1mm = 1mm` is exact, so all four readings are 0.001
/// SI. The two-level `Parent.echo` relay is included because it is the shape
/// that composes both defects: an instance-scope read OF a relaying let.
#[test]
fn prelude_typed_sub_member_read_relays_at_both_scopes() {
    const SOURCE: &str = r#"
structure def Mid {
    sub tol = DimensionalTolerance(nominal: 10mm, upper_deviation: 2mm, lower_deviation: 1mm)
    let relay = self.tol.tolerance_band
}

structure def Parent {
    sub m = Mid()
    let echo = self.m.relay
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // Control / achievability basis: the prelude-typed sub's own derived let,
    // already pinned by `prelude_typed_nested_sub_resolves_at_instance_scope`.
    assert_scalar_si(&result.values, "Mid.tol", "tolerance_band", 0.001);

    // (1) TEMPLATE scope: the relay must be the Scalar, not a GeometryHandle.
    assert_scalar_si(&result.values, "Mid", "relay", 0.001);

    // (2) INSTANCE scope: the same relay, on the nested instance.
    assert_scalar_si(&result.values, "Parent.m", "relay", 0.001);

    // (3) Two-level: a parent let reading the child's relaying let.
    assert_scalar_si(&result.values, "Parent", "echo", 0.001);

    assert_no_error_diagnostics(
        &result.diagnostics,
        "prelude_typed_sub_member_read_relays_at_both_scopes",
    );
}

/// (t9a) An UNGUARDED SELF-recursive plain sub must be CUT at instance scope,
/// not recursed into forever.
///
/// The ancestor-chain guard in `elaborate_child_instance_nested` is the ONLY
/// thing that terminates the phase-1.5 recursion: plain sub nesting is not a
/// DAG. `structure def Node { sub child : Node }` is admitted by the compiler
/// (it reports "recursive sub has no termination condition" but still emits the
/// template), so eval genuinely sees a cyclic sub graph — an unguarded
/// recursion here is a stack overflow, not a hang. Nothing pinned that until
/// now.
///
/// Built via the template builders rather than `.ri` source on purpose:
/// `parse_and_compile_with_stdlib` asserts zero compile errors, and this shape
/// is exactly the one the compiler errors on. The builders model what eval
/// actually receives — the emitted-anyway template.
#[test]
fn self_recursive_plain_sub_is_cut_not_overflowed() {
    let node = TopologyTemplateBuilder::new("Node")
        .param(
            "Node",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(1), Type::Int)),
        )
        // UNGUARDED self-reference: `sub child : Node`.
        .sub_component("child", "Node", vec![])
        .let_binding(
            "Node",
            "echo",
            Type::Int,
            value_ref_typed("Node.child", "n", Type::Int),
        )
        .build();

    let parent = TopologyTemplateBuilder::new("Parent")
        .sub_component("node", "Node", vec![])
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(node)
        .template(parent)
        .build();
    let mut engine = make_simple_engine();
    // Reaching the next line at all IS the termination assertion.
    let result = engine.eval(&module);

    // The instance itself is elaborated — its own param lands.
    assert_eq!(
        result.values.get(&ValueCellId::new("Parent.node", "n")),
        Some(&Value::Int(1)),
        "Parent.node.n should be the Node param default (proves the instance was elaborated)",
    );

    // ...but the self-referential nested sub is cut, so the grandchild entity
    // never materialises and the read across it cannot resolve.
    assert!(
        !result
            .values
            .contains(&ValueCellId::new("Parent.node.child", "n")),
        "Parent.node.child.n must not exist — the cycle guard must cut the self-reference",
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("Parent.node", "echo")),
        Some(&Value::Undef),
        "Parent.node.echo is expected to be Undef — the point is that it must not be SILENTLY so",
    );

    // The cut is surfaced, and names itself as a cycle cut rather than as one
    // of the other three skip shapes.
    let errors: Vec<&Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.iter().any(|d| d.message.contains("Parent.node.echo")
            && d.message.contains("Parent.node.child.n")
            && d.message.contains("cyclic sub nesting cut")),
        "expected an error naming the starved let, the nested cell it reads, and the \
         \"cyclic sub nesting cut\" reason; got: {errors:?}",
    );
}

/// (t9b) MUTUALLY-recursive plain subs (`A { sub b : B }` / `B { sub a : A }`)
/// must terminate too — and the guard must cut at the RE-ENTRY, not earlier.
///
/// The companion to t9a: a self-reference is cut at depth 1, so it cannot tell
/// a correct ancestor-chain guard from one that simply refuses to nest at all.
/// Here the first level MUST be elaborated (`Parent.x.b.q` = 7, and `A`'s
/// cross-sub read of it resolves) and only the re-entry into `A` is cut.
#[test]
fn mutually_recursive_plain_subs_cut_at_reentry() {
    let a = TopologyTemplateBuilder::new("A")
        .sub_component("b", "B", vec![])
        .let_binding("A", "echo", Type::Int, value_ref_typed("A.b", "q", Type::Int))
        .build();

    let b = TopologyTemplateBuilder::new("B")
        .param(
            "B",
            "q",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(7), Type::Int)),
        )
        // Closes the A -> B -> A cycle.
        .sub_component("a", "A", vec![])
        .let_binding(
            "B",
            "back",
            Type::Int,
            value_ref_typed("B.a", "echo", Type::Int),
        )
        .build();

    let parent = TopologyTemplateBuilder::new("Parent")
        .sub_component("x", "A", vec![])
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(a)
        .template(b)
        .template(parent)
        .build();
    let mut engine = make_simple_engine();
    // Reaching the next line at all IS the termination assertion.
    let result = engine.eval(&module);

    // Depth 1 IS elaborated: the guard must not be over-eager.
    assert_eq!(
        result.values.get(&ValueCellId::new("Parent.x.b", "q")),
        Some(&Value::Int(7)),
        "Parent.x.b.q must exist — the A -> B step is not a cycle and must be nested",
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("Parent.x", "echo")),
        Some(&Value::Int(7)),
        "Parent.x.echo must relay the nested value (the task-5360 contract, one level in)",
    );

    // Depth 2 re-enters `A`, which is already on the chain — cut there.
    assert!(
        !result
            .values
            .contains(&ValueCellId::new("Parent.x.b.a", "echo")),
        "Parent.x.b.a.echo must not exist — re-entering A must be cut",
    );

    // ...and the cut at that deeper level is surfaced, proving the report runs
    // at every nesting level rather than only at the outermost one.
    let errors: Vec<&Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.iter().any(|d| d.message.contains("Parent.x.b.back")
            && d.message.contains("Parent.x.b.a.echo")
            && d.message.contains("cyclic sub nesting cut")),
        "expected an error naming the starved let at the NESTED scope \
         (\"Parent.x.b.back\"), the cell it reads, and the cycle-cut reason; got: {errors:?}",
    );
}

/// Amendment (review #1) — the COLLAPSED structure value, not just the member
/// cells.
///
/// Phase 1.5 materialises `Parent.m.k.off` but used to stop there. At TEMPLATE
/// scope `engine_eval.rs` additionally commits a `Value::StructureInstance` at
/// `ValueCellId("Mid", "k")` (SIR-α, task 3540) plus its `__self` alias at
/// `ValueCellId("Mid.k", "__self")` (task 3941 ζ) — the cell a bare sub
/// reference used as a WHOLE value lowers to
/// (`resolve_non_collection_sub_to_structure_ref`, expr.rs). Without the same
/// pair at instance scope, a whole-value use of `k` resolves under `Mid` and is
/// silently `Undef` under `Parent.m` — the exact template/instance asymmetry
/// this task exists to close, left half-closed for non-scalar reads.
///
/// The template-scope cells are asserted first as the achievability control.
#[test]
fn nested_sub_collapsed_instance_and_self_alias_exist_at_instance_scope() {
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
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // Control: template scope already commits both, and the member cell the
    // collapsed value is gathered from.
    assert_scalar_si(&result.values, "Mid.k", "off", 0.02);
    assert_si_field_scalar(&result.values, "Mid", "k", "off", 0.02);
    assert_si_field_scalar(&result.values, "Mid.k", "__self", "off", 0.02);

    // Instance scope must mirror it exactly.
    assert_scalar_si(&result.values, "Parent.m.k", "off", 0.02);
    assert_si_field_scalar(&result.values, "Parent.m", "k", "off", 0.02);
    assert_si_field_scalar(&result.values, "Parent.m.k", "__self", "off", 0.02);

    assert_no_error_diagnostics(
        &result.diagnostics,
        "nested_sub_collapsed_instance_and_self_alias_exist_at_instance_scope",
    );
}

/// Amendment (review #1), behavioural half — a `let` that reads the nested sub
/// as a WHOLE value must resolve at instance scope.
///
/// `self.k` in a let of `Mid` compiles to `ValueRef(ValueCellId("Mid.k",
/// "__self"))`, and phase 2 evaluates `Mid`'s lets against `child_values`, NOT
/// against the phase-1.5 overlay. So committing the alias into the global map
/// alone is not enough: the collapsed pair must also reach phase 2's let scope,
/// or `Parent.m.whole` stays `Undef` while `Mid.whole` resolves.
#[test]
fn nested_sub_whole_value_let_resolves_at_instance_scope() {
    const SOURCE: &str = r#"
structure def Kid {
    param w : Length = 10mm
    let off = w * 2.0
}

structure def Mid {
    sub k = Kid()
    let whole = self.k
}

structure def Parent {
    sub m = Mid()
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // Control: the whole-value read resolves at template scope today.
    assert_si_field_scalar(&result.values, "Mid", "whole", "off", 0.02);

    // The instance-scope let must carry the same collapsed structure.
    assert_si_field_scalar(&result.values, "Parent.m", "whole", "off", 0.02);

    assert_no_error_diagnostics(
        &result.diagnostics,
        "nested_sub_whole_value_let_resolves_at_instance_scope",
    );
}
