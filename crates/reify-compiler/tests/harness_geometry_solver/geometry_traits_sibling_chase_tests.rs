//! Trait inference resolving `GeomRef::Sub` operands through sibling realizations.

use reify_compiler::geometry_traits_inference::{
    InferredTraits, SubRefEnv, combine_pattern, infer_traits_for_op, infer_traits_for_op_in_env,
};
use reify_compiler::{
    BooleanOp, CompiledGeometryOp, GeomRef, PatternKind, PrimitiveKind, SweepKind, TransformKind,
};
use reify_core::DiagnosticCode;
use reify_test_support::{compile_source_with_stdlib, errors_only};
use std::panic;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

// ─── SubRefEnv: op-array Sub-ref resolution seam (#7060) ────────────────────

/// Test stub for the op-array env seam: resolves the single sibling name `"h"`
/// to an unbounded half-space trait set, and declines every other name.
///
/// Mirrors the `LetBindingEnv` stubs used by the `infer_traits_for_expr_in_env`
/// tests in `geometry_traits_inference_tests.rs` — the two entry points share
/// one env idiom.
struct StubSubRefEnv;

impl SubRefEnv for StubSubRefEnv {
    fn lookup_sub(&self, name: &str) -> Option<InferredTraits> {
        match name {
            "h" => Some(InferredTraits::unbounded_convex()),
            _ => None,
        }
    }
}

/// A `GeomRef::Sub(name)` the env RESOLVES propagates the referent's real
/// traits through `combine_transform` (an identity passthrough), so an
/// unbounded sibling yields `bounded == false` instead of the `all()`
/// safe-default.
///
/// This is the seam that makes the sibling-let → `GeomRef::Sub` lowering
/// (#4668) visible to trait inference at all.
#[test]
fn infer_traits_for_op_in_env_resolves_sub_ref_through_env() {
    let ops = vec![CompiledGeometryOp::Transform {
        kind: TransformKind::Translate,
        target: GeomRef::Sub("h".to_string()),
        args: vec![],
    }];
    let inferred = infer_traits_for_op_in_env(&ops, &StubSubRefEnv);
    assert!(
        !inferred.bounded,
        "Sub(\"h\") resolves through the env to unbounded_convex(); combine_transform is an \
         identity passthrough, so translate(h, ..) must infer bounded=false, got {inferred:?}"
    );
    assert_eq!(
        inferred,
        InferredTraits::unbounded_convex(),
        "combine_transform preserves the operand's full trait set unchanged"
    );
}

/// The `Pattern` arm resolves its `target` through the env too, so an unbounded
/// sibling stays unbounded under `combine_pattern`.
///
/// `env` is threaded through every recursive `infer_op` arm; this pins the one
/// that no end-to-end case reaches, so a future refactor that drops the `env`
/// argument on this arm alone reds here rather than silently reverting `Pattern`
/// operands to the `all()` safe default.
#[test]
fn infer_traits_for_op_in_env_resolves_sub_ref_on_pattern_target() {
    let ops = vec![CompiledGeometryOp::Pattern {
        kind: PatternKind::Linear,
        target: GeomRef::Sub("h".to_string()),
        args: vec![],
    }];
    let inferred = infer_traits_for_op_in_env(&ops, &StubSubRefEnv);
    assert!(
        !inferred.bounded,
        "Sub(\"h\") resolves to unbounded_convex() and combine_pattern inherits `bounded` \
         from its target, so patterning an unbounded sibling must stay unbounded, \
         got {inferred:?}"
    );
    assert_eq!(
        inferred,
        combine_pattern(InferredTraits::unbounded_convex()),
        "the Pattern arm must apply combine_pattern to the RESOLVED operand — no other \
         propagation rule and no safe-default short-circuit"
    );
}

/// The `Sweep` arm infers from `profiles.first()` ONLY, so a `loft` whose first
/// profile resolves bounded infers `bounded=true` however unbounded its later
/// profiles are.
///
/// The approximation applies equally to inlined `GeomRef::Step` profiles. Both
/// orderings are asserted: the second is the one that fails if `infer_op` ever
/// starts folding every profile.
#[test]
fn infer_traits_for_op_in_env_sweep_consults_only_the_first_profile() {
    let unbounded_first = vec![CompiledGeometryOp::Sweep {
        kind: SweepKind::Loft,
        profiles: vec![
            GeomRef::Sub("h".to_string()),
            GeomRef::Sub("declined".to_string()),
        ],
        args: vec![],
    }];
    assert!(
        !infer_traits_for_op_in_env(&unbounded_first, &StubSubRefEnv).bounded,
        "first profile Sub(\"h\") resolves to unbounded_convex(), and combine_sweep inherits \
         `bounded` from the profile, so the loft must infer bounded=false"
    );

    let unbounded_second = vec![CompiledGeometryOp::Sweep {
        kind: SweepKind::Loft,
        profiles: vec![
            GeomRef::Sub("declined".to_string()),
            GeomRef::Sub("h".to_string()),
        ],
        args: vec![],
    }];
    assert!(
        infer_traits_for_op_in_env(&unbounded_second, &StubSubRefEnv).bounded,
        "KNOWN APPROXIMATION, asserted deliberately: only profiles.first() is consulted, so \
         an unbounded profile in any later position is invisible to inference. If a future \
         change folds every profile, this assertion is the one to flip — and the \
         end-to-end `loft` behaviour changes with it"
    );
}

/// A `GeomRef::Sub(name)` the env DECLINES keeps today's `InferredTraits::all()`
/// safe-default — the `None` → `all()` collapse is the same convention every
/// other defensive arm in the module uses.
#[test]
fn infer_traits_for_op_in_env_unresolved_sub_ref_keeps_safe_default() {
    let ops = vec![CompiledGeometryOp::Transform {
        kind: TransformKind::Translate,
        target: GeomRef::Sub("nope".to_string()),
        args: vec![],
    }];
    assert_eq!(
        infer_traits_for_op_in_env(&ops, &StubSubRefEnv),
        InferredTraits::all(),
        "an unresolved Sub name must safe-default to all(), not leak a partial trait set"
    );
}

/// The env-less `infer_traits_for_op` alias is behaviour-preserving: over the
/// very op-array that resolves to `bounded=false` under `StubSubRefEnv`, it
/// still returns `InferredTraits::all()`.
///
/// This locks the blast radius of the seam to call sites that explicitly opt in
/// by passing an env.
#[test]
fn infer_traits_for_op_without_env_still_defaults_sub_ref_to_all() {
    let ops = vec![CompiledGeometryOp::Transform {
        kind: TransformKind::Translate,
        target: GeomRef::Sub("h".to_string()),
        args: vec![],
    }];
    assert_eq!(
        infer_traits_for_op(&ops),
        InferredTraits::all(),
        "infer_traits_for_op must stay a thin alias over the empty env: Sub(\"h\") is \
         unresolvable without an env, so the safe default all() stands"
    );
}

// ─── Sibling-let GeomRef::Sub chasing, end to end (#7060) ───────────────────
//
// A bare sibling-let geometry operand lowers to `GeomRef::Sub(<name>)` rather
// than an inlined `GeomRef::Step` (#4668). The inference walker chases that
// `Sub` into the sibling realization, so an unbounded producer reached through
// a `let` is diagnosed at a `Bounded` slot exactly as the inline shape is.
// These cases pin the end-to-end behaviour in both directions.

/// Count the `GeometryUnbounded` diagnostics a source produces, returning the
/// full error list alongside so a failure message can show what DID fire.
fn geometry_unbounded_count(source: &str) -> (usize, Vec<String>) {
    let compiled = compile_source_with_stdlib(source);
    let all_diags = errors_only(&compiled);
    let count = all_diags
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::GeometryUnbounded))
        .count();
    let rendered = all_diags.iter().map(|d| format!("{d:?}")).collect();
    (count, rendered)
}

/// Assert `source` compiles with NO errors at all — which subsumes "emits no
/// `GeometryUnbounded`" while also failing if the fixture stops compiling for
/// an unrelated reason (a renamed stdlib builtin, a grammar change).
///
/// The weaker `assert_eq!(geometry_unbounded_count(..).0, 0)` passes VACUOUSLY
/// in that case: the count of a diagnostic that never got a chance to fire is
/// also 0, so a must-stay-silent test would go green while proving nothing.
/// Every zero-expected case in this section therefore goes through here, and
/// `why` carries its per-case rationale into the failure message.
fn assert_compiles_with_no_geometry_unbounded(source: &str, why: &str) {
    let (count, diags) = geometry_unbounded_count(source);
    assert!(
        diags.is_empty(),
        "fixture must compile clean, so that its zero GeometryUnbounded count means the \
         check ran and stayed silent rather than never running at all — {why}. \
         GeometryUnbounded count: {count}. All errors: {diags:?}"
    );
}

/// One hop: `let h = half_space(..)` then `let t = translate(h, ..)`, with `t`
/// passed to a `param g : Bounded` slot.
///
/// `t` lowers to `[Transform { Translate, target: Sub("h") }]` via the #4668
/// sibling-let pre-check. `combine_transform` is an identity passthrough, so
/// once the `Sub("h")` operand resolves to `half_space`'s `bounded=false` the
/// slot must be diagnosed — exactly as the inline `Foo(g: half_space(..))`
/// form already is.
#[test]
fn translate_of_unbounded_sibling_let_at_bounded_slot_emits_geometry_unbounded() {
    let source = r#"
        structure def Foo {
            param g : Bounded
        }
        structure def Top {
            let h = half_space(0mm, 0mm, 0mm, 0, 0, 1)
            let t = translate(h, 1mm, 0mm, 0mm)
            sub x = Foo(g: t)
        }
    "#;
    let (count, diags) = geometry_unbounded_count(source);
    assert_eq!(
        count, 1,
        "expected EXACTLY ONE GeometryUnbounded diagnostic for `translate(h, ..)` where `h` \
         is an unbounded sibling let: the operand lowers to GeomRef::Sub(\"h\") (#4668) and \
         must be chased through the sibling realization. The fixture has exactly one \
         geometry slot, so a count above 1 means the chase re-reported one operand. \
         Got {count}. All diagnostics: {diags:?}"
    );
}

/// Two hops: `h` → `let t1 = translate(h, ..)` → `let t2 = translate(t1, ..)`,
/// with `t2` at the `Bounded` slot.
///
/// Each hop is its own realization holding a `GeomRef::Sub` back to the
/// previous one, so this fails if resolution stops at depth 1 rather than
/// recursing through the sibling-let chain (#4668).
#[test]
fn two_hop_transform_chain_of_unbounded_sibling_let_emits_geometry_unbounded() {
    let source = r#"
        structure def Foo {
            param g : Bounded
        }
        structure def Top {
            let h = half_space(0mm, 0mm, 0mm, 0, 0, 1)
            let t1 = translate(h, 1mm, 0mm, 0mm)
            let t2 = translate(t1, 0mm, 1mm, 0mm)
            sub x = Foo(g: t2)
        }
    "#;
    let (count, diags) = geometry_unbounded_count(source);
    assert_eq!(
        count, 1,
        "expected EXACTLY ONE GeometryUnbounded diagnostic for a two-hop sibling-let chain \
         (h -> t1 -> t2): each hop lowers to its own GeomRef::Sub (#4668), so chasing must \
         recurse rather than stop at depth 1 — and must report the one slot once, not once \
         per hop. Got {count}. All diagnostics: {diags:?}"
    );
}

/// Modify family: `let s = shell(h, 1mm)` at the `Bounded` slot.
///
/// `shell` lowers to `CompiledGeometryOp::Modify`, a different `infer_op` arm
/// from `Transform`, so this pins that the `GeomRef::Sub` (#4668) chasing is a
/// property of `infer_geom_ref` rather than of one op variant.
#[test]
fn shell_of_unbounded_sibling_let_at_bounded_slot_emits_geometry_unbounded() {
    let source = r#"
        structure def Foo {
            param g : Bounded
        }
        structure def Top {
            let h = half_space(0mm, 0mm, 0mm, 0, 0, 1)
            let s = shell(h, 1mm)
            sub x = Foo(g: s)
        }
    "#;
    let (count, diags) = geometry_unbounded_count(source);
    assert_eq!(
        count, 1,
        "expected EXACTLY ONE GeometryUnbounded diagnostic for `shell(h, ..)` where `h` is an \
         unbounded sibling let: the Modify arm resolves its target through the same \
         GeomRef::Sub (#4668) path as Transform, and the fixture has one geometry slot. \
         Got {count}. All diagnostics: {diags:?}"
    );
}

/// False-positive guard: a BOUNDED sibling let must stay silent.
///
/// `let b = box(..)` / `let t = translate(b, ..)` lowers to the identical
/// `GeomRef::Sub("b")` (#4668) shape as the must-fire cases above. Chasing it
/// resolves to `box`'s `bounded=true`, so the slot is satisfied and NO
/// diagnostic may be emitted. This is what distinguishes chasing the Sub from
/// pessimising it.
#[test]
fn translate_of_bounded_sibling_let_at_bounded_slot_emits_no_geometry_unbounded() {
    let source = r#"
        structure def Foo {
            param g : Bounded
        }
        structure def Top {
            let b = box(20mm, 20mm, 20mm)
            let t = translate(b, 1mm, 0mm, 0mm)
            sub x = Foo(g: t)
        }
    "#;
    assert_compiles_with_no_geometry_unbounded(
        source,
        "chasing GeomRef::Sub(\"b\") (#4668) must resolve to box's bounded=true, not \
         pessimise the operand",
    );
}

// ─── Regression locks for the #7060 Sub-chasing fix ────────────────────────

/// (a) Cycle termination. Two sibling lets that name each other are accepted
/// source — `let a = translate(b, ..)` / `let b = translate(a, ..)` lowers to
/// `a = [Xform(.., Sub("b"))]` / `b = [Xform(.., Sub("a"))]`, a genuine 2-cycle
/// in the sibling-let graph (#4668). Chasing those Subs without a visited set
/// recurses forever.
///
/// This test asserts only that compilation RETURNS and safe-defaults. It is the
/// executable proof that the `in_flight` guard sits above the realization arm:
/// remove that guard and this does not fail, it aborts the whole test binary
/// with a stack overflow.
#[test]
fn mutually_referential_sibling_lets_terminate_instead_of_overflowing() {
    let source = r#"
        structure def Foo {
            param g : Bounded
        }
        structure def Top {
            let a = translate(b, 1mm, 0mm, 0mm)
            let b = translate(a, 1mm, 0mm, 0mm)
            sub x = Foo(g: a)
        }
    "#;
    // Reaching the next line at all is the assertion: a missing cycle guard
    // overflows the stack here rather than returning.
    assert_compiles_with_no_geometry_unbounded(
        source,
        "a sibling-let cycle must terminate at the in_flight guard and safe-default to \
         InferredTraits::all() (bounded=true), not invent a diagnostic",
    );
}

/// (b) Compound cross-sub keys are still not chased.
///
/// `GeomRef::Sub` is overloaded: besides bare sibling names (#4668) it also
/// carries `"<sub>.<member>"` for cross-sub `self.<sub>.<member>` references,
/// whose referent lives on a DIFFERENT template. The consumer's `contains('.')`
/// carve-out declines those, and a declined name collapses to the `all()` safe
/// default here — so cross-sub references are never chased.
///
/// Op shape mirrors the lowering pinned by
/// `harness_type_checking::boolean_arg_cross_sub_diagnostic_tests::
/// boolean_op_with_working_path_cross_sub_arg_lowers_without_diagnostic`.
#[test]
fn compound_cross_sub_key_declined_by_env_keeps_safe_default() {
    let ops = vec![
        CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        },
        CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left: GeomRef::Sub("inner.body".to_string()),
            right: GeomRef::Step(0),
        },
    ];
    // StubSubRefEnv declines every name but "h", standing in for the consumer
    // declining every name containing a dot.
    assert_eq!(
        infer_traits_for_op_in_env(&ops, &StubSubRefEnv),
        InferredTraits::bounded_only(),
        "a compound cross-sub key must collapse to all() (making Union(all, Box) = \
         bounded_only), identical to the pre-#7060 behaviour — #7060 chases bare sibling \
         names only and must not widen into cross-entity resolution"
    );
}

/// (b, end to end) The cross-sub source shape itself is behaviourally untouched:
/// a `self.<sub>.<member>` geometry reference at a `Bounded` slot still emits no
/// `GeometryUnbounded`, because its compound `GeomRef::Sub` key is not chased.
#[test]
fn cross_sub_geometry_reference_at_bounded_slot_is_unchanged_by_sub_chasing() {
    let source = r#"
        structure def Foo {
            param g : Bounded
        }
        structure def Inner {
            let body = half_space(0mm, 0mm, 0mm, 0, 0, 1)
        }
        structure def Top {
            sub inner = Inner()
            let combined = union(self.inner.body, box(20mm, 20mm, 20mm))
            sub x = Foo(g: combined)
        }
    "#;
    assert_compiles_with_no_geometry_unbounded(
        source,
        "a cross-sub `self.inner.body` reference lowers to the compound key \
         GeomRef::Sub(\"inner.body\"), which #7060 deliberately does NOT chase — behaviour \
         must be identical to before the fix",
    );
}

/// (c) #6449 widening lock. `intersection(cutter, box(..))` where `cutter` is an
/// unbounded sibling let IS bounded, so the `Bounded` slot must stay silent.
///
/// Green today for a reason that is about to change: boolean operands still
/// inline as `GeomRef::Step`, so this never touches the #7060 chasing path. Once
/// #6449 converts bare-ident boolean operands to `GeomRef::Sub` — the same
/// lowering transform/modify already use (#4668) — `cutter` becomes a chased Sub
/// and `combine_intersection` is what must keep this green by masking the
/// unbounded operand against the bounded one.
///
/// Landing #7060 first is what stops #6449 from silently widening the dropped
/// diagnostic; this test is the executable half of that claim, and must be
/// re-read (not merely re-run) when #6449 lands.
#[test]
fn intersection_of_unbounded_sibling_let_with_box_emits_no_geometry_unbounded() {
    let source = r#"
        structure def Foo {
            param g : Bounded
        }
        structure def Top {
            let cutter = half_space(0mm, 0mm, 0mm, 0, 0, 1)
            let upper = intersection(cutter, box(20mm, 20mm, 20mm))
            sub x = Foo(g: upper)
        }
    "#;
    assert_compiles_with_no_geometry_unbounded(
        source,
        "intersection with a bounded operand IS bounded, so no GeometryUnbounded may fire \
         even though `cutter` is unbounded",
    );
}

/// (d) Depth termination. Build `let t0 = half_space(..)` followed by `n` hops
/// of `let t{i} = translate(t{i-1}, ..)`, and pass the last one to a `Bounded`
/// slot.
fn sibling_let_chain_source(n: usize) -> String {
    let mut body = String::from("            let t0 = half_space(0mm, 0mm, 0mm, 0, 0, 1)\n");
    for i in 1..=n {
        body.push_str(&format!(
            "            let t{i} = translate(t{}, 1mm, 0mm, 0mm)\n",
            i - 1
        ));
    }
    format!(
        "        structure def Foo {{\n\
         \x20           param g : Bounded\n\
         \x20       }}\n\
         \x20       structure def Top {{\n\
         {body}\
         \x20           sub x = Foo(g: t{n})\n\
         \x20       }}\n"
    )
}

/// A sibling-let chain LONGER than `MAX_SIBLING_CHASE_DEPTH` terminates at the
/// depth cap and safe-defaults, instead of overflowing the stack.
///
/// The cycle guard bounds revisits, not chain length: this chain is perfectly
/// acyclic, so every hop is a fresh `ValueCellId` the `in_flight` membership
/// check waves through, and the chase recurses once per hop. The compiler's own
/// `recursion_guard` cap does not cover it either — each let is depth-1 for
/// `compile_expr`/`compile_geometry_call`, so `MAX_COMPILE_RECURSION_DEPTH`
/// never fires. Without the cap, this source at n=2000 overflows the stack and
/// aborts the whole test binary.
///
/// Like the cycle case, the cap trades the diagnostic for termination: past
/// 256 hops the chase declines and the operand collapses to
/// `InferredTraits::all()`, so an unbounded producer at the far end of a
/// pathological chain goes unreported. The companion test below pins that
/// realistic depths are unaffected.
#[test]
fn sibling_let_chain_past_the_depth_cap_terminates_instead_of_overflowing() {
    // Reaching the next line at all is the assertion: without the depth cap
    // this aborts the process rather than returning.
    assert_compiles_with_no_geometry_unbounded(
        &sibling_let_chain_source(2000),
        "a chain past MAX_SIBLING_CHASE_DEPTH must decline at the cap and safe-default to \
         InferredTraits::all(), which reports nothing",
    );
}

/// The depth cap is a backstop, not a functional regression: a chain well
/// within `MAX_SIBLING_CHASE_DEPTH` still chases all the way to the unbounded
/// producer and diagnoses.
///
/// 200 hops sits below the 256 cap with headroom for the one `lookup` frame the
/// slot itself costs. Lower the cap below this and the test reds, which is the
/// intended signal — it means realistic designs stopped being diagnosed.
#[test]
fn sibling_let_chain_within_the_depth_cap_still_emits_geometry_unbounded() {
    let (count, diags) = geometry_unbounded_count(&sibling_let_chain_source(200));
    assert_eq!(
        count, 1,
        "a 200-hop sibling-let chain is inside MAX_SIBLING_CHASE_DEPTH (256), so chasing must \
         still reach `t0 = half_space(..)` and report the one Bounded slot exactly once. \
         Got {count}. All diagnostics: {diags:?}"
    );
}

/// Run `f` on a worker thread and fail if it has not returned within a 60s
/// class-T anti-hang budget (`docs/prds/infra-test-wallclock-deflake.md`): a
/// ceiling that only ever catches a true hang, never a slow run.
///
/// The worker keeps the default thread stack size, the same budget libtest gives
/// a test thread, so the depth cap is exercised against the stack it is sized
/// for. A panic inside `f` is forwarded as itself. On timeout the worker cannot
/// be cancelled and leaks until the test binary exits.
fn within_anti_hang_budget<T: Send + 'static>(
    what: &str,
    f: impl FnOnce() -> T + Send + 'static,
) -> T {
    let (tx, rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = tx.send(panic::catch_unwind(panic::AssertUnwindSafe(f)));
    });
    match rx.recv_timeout(Duration::from_secs(60)) {
        Ok(Ok(value)) => value,
        Ok(Err(payload)) => panic::resume_unwind(payload),
        Err(mpsc::RecvTimeoutError::Timeout) => panic!(
            "{what} did not finish within the 60s anti-hang budget: the sibling-let chase \
             re-resolved a sibling once per reference instead of once per env"
        ),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("{what}: the worker thread exited without sending a result")
        }
    }
}

/// A sibling-let chain in which every hop names its predecessor TWICE: `let x0 =
/// {x0_init}`, then `n` hops of `let x{i} = union(translate(x{i-1}, ..),
/// translate(x{i-1}, ..))`, with `x{n}` passed to a `Bounded` slot.
///
/// The boolean operands are inlined, but each nested `translate` target lowers
/// to `GeomRef::Sub("x{i-1}")`, so the chase meets every predecessor twice.
fn fan_out_sibling_let_chain_source(n: usize, x0_init: &str) -> String {
    let mut body = format!("            let x0 = {x0_init}\n");
    for i in 1..=n {
        let prev = i - 1;
        body.push_str(&format!(
            "            let x{i} = union(translate(x{prev}, 0mm, 0mm, 0mm), \
             translate(x{prev}, 1mm, 0mm, 0mm))\n"
        ));
    }
    format!(
        "        structure def Foo {{\n\
         \x20           param g : Bounded\n\
         \x20       }}\n\
         \x20       structure def Top {{\n\
         {body}\
         \x20           sub x = Foo(g: x{n})\n\
         \x20       }}\n"
    )
}

/// A 40-hop fan-out chain still reaches the unbounded `x0` and diagnoses the one
/// `Bounded` slot, within the anti-hang budget: each sibling is resolved once,
/// not once per path that reaches it.
#[test]
fn fan_out_sibling_let_chain_within_the_depth_cap_still_emits_geometry_unbounded_promptly() {
    let source = fan_out_sibling_let_chain_source(40, "half_space(0mm, 0mm, 0mm, 0, 0, 1)");
    within_anti_hang_budget("the 40-hop fan-out sibling-let chain", move || {
        let (count, diags) = geometry_unbounded_count(&source);
        assert_eq!(
            count, 1,
            "a 40-hop fan-out chain is inside MAX_SIBLING_CHASE_DEPTH, so chasing must reach \
             `x0 = half_space(..)` and report the one Bounded slot exactly once. \
             Got {count}. All diagnostics: {diags:?}"
        );
    });
}

/// A fan-out chain longer than `MAX_SIBLING_CHASE_DEPTH` safe-defaults at the cap
/// and still finishes promptly, so a result computed under a depth cut is reused
/// rather than re-chased at every level above the cut.
#[test]
fn fan_out_sibling_let_chain_past_the_depth_cap_terminates_promptly() {
    let source = fan_out_sibling_let_chain_source(300, "half_space(0mm, 0mm, 0mm, 0, 0, 1)");
    within_anti_hang_budget("the 300-hop fan-out sibling-let chain", move || {
        assert_compiles_with_no_geometry_unbounded(
            &source,
            "a fan-out chain past MAX_SIBLING_CHASE_DEPTH must decline at the cap and \
             safe-default to InferredTraits::all(), which reports nothing",
        );
    });
}

/// A fan-out chain closed into a cycle (`x0 = translate(x40, ..)`) terminates at
/// the cycle guard and finishes promptly, so a result computed under a cycle cut
/// is reused rather than re-chased below the cut.
#[test]
fn cyclic_fan_out_sibling_lets_terminate_promptly() {
    let source = fan_out_sibling_let_chain_source(40, "translate(x40, 0mm, 0mm, 0mm)");
    within_anti_hang_budget("the 40-hop cyclic fan-out sibling-let chain", move || {
        assert_compiles_with_no_geometry_unbounded(
            &source,
            "a cyclic fan-out chain must terminate at the in_flight guard and safe-default to \
             InferredTraits::all(), which reports nothing",
        );
    });
}

/// (e) A Solid-typed param's geometry DEFAULT is authoritative for trait
/// inference, at every hop count.
///
/// `geometry_realization_names` admits Solid params whose default is a geometry
/// call, and `compile_entity` lowers that default into a realization next to the
/// geometry lets — so `GeomRef::Sub("p")` resolves to the DEFAULT's traits, not
/// to whatever an instantiation might pass for `p`.
///
/// This pair pins that the policy is consistent across hop counts: the direct
/// form `Foo(g: p)` involves no `Sub`, the let-indirected form
/// `Foo(g: translate(p, ..))` reaches the same default through
/// `GeomRef::Sub("p")`, and both diagnose it.
#[test]
fn solid_param_default_direct_at_bounded_slot_emits_geometry_unbounded() {
    let source = r#"
        structure def Foo {
            param g : Bounded
        }
        structure def Top {
            param p : Solid = half_space(0mm, 0mm, 0mm, 0, 0, 1)
            sub x = Foo(g: p)
        }
    "#;
    let (count, diags) = geometry_unbounded_count(source);
    assert_eq!(
        count, 1,
        "PRE-EXISTING behaviour, pinned as the baseline for the indirected case below: an \
         unbounded param default at a Bounded slot is diagnosed even though an instantiation \
         could pass bounded geometry. Got {count}. All diagnostics: {diags:?}"
    );
}

#[test]
fn solid_param_default_through_sibling_let_at_bounded_slot_emits_geometry_unbounded() {
    let source = r#"
        structure def Foo {
            param g : Bounded
        }
        structure def Top {
            param p : Solid = half_space(0mm, 0mm, 0mm, 0, 0, 1)
            let t = translate(p, 1mm, 0mm, 0mm)
            sub x = Foo(g: t)
        }
    "#;
    let (count, diags) = geometry_unbounded_count(source);
    assert_eq!(
        count, 1,
        "WIDENED by #7060 and deliberately so: `GeomRef::Sub(\"p\")` (#4668) resolves to the \
         param's lowered default realization, so the indirected form now agrees with the \
         direct form pinned above instead of silently passing. Got {count}. \
         All diagnostics: {diags:?}"
    );
}

/// The param-default policy is about the DEFAULT's traits, not about params
/// being pessimised: a BOUNDED default at a `Bounded` slot stays silent through
/// the same `GeomRef::Sub` chase.
#[test]
fn bounded_solid_param_default_through_sibling_let_emits_no_geometry_unbounded() {
    let source = r#"
        structure def Foo {
            param g : Bounded
        }
        structure def Top {
            param p : Solid = box(20mm, 20mm, 20mm)
            let t = translate(p, 1mm, 0mm, 0mm)
            sub x = Foo(g: t)
        }
    "#;
    assert_compiles_with_no_geometry_unbounded(
        source,
        "chasing Sub(\"p\") to a BOUNDED param default must resolve to box's bounded=true — \
         the param arm reads the default's traits, it does not pessimise params",
    );
}
