//! Cross-crate builtin-name **drift guard** between the compiler-side
//! builtin-name registries (`reify_compiler::units` — name families +
//! result-type maps) and the eval-side dispatch oracles
//! (`crate::geometry_ops` — the `pub(crate)` boolean name classifiers and
//! the `TopologySelectorHelper` build-dispatch map).
//!
//! ## Why this exists (task 5055 γ; PRD
//! `docs/prds/v0_6/compiler-type-hygiene.md` task γ §7.3; INV-COMP-2
//! interim — enforcement flips `proposed` → `enforced(test)`)
//!
//! A single selector/builtin name is registered in **>= 7 places** across
//! the two crates today. `closest_point` is a representative example:
//!
//!  1. `reify-compiler/src/units.rs` — `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` slice entry (compile-time recognition)
//!  2. `reify-compiler/src/units.rs` — `topology_selector_result_type` match arm (compile-time result type)
//!  3. `reify-compiler/src/units.rs` (`#[cfg(test)] mod tests`) — `is_geometry_topology_selector_recognises_closest_point` (compiler-internal pin)
//!  4. `reify-eval/src/geometry_ops.rs` — `is_geometry_consumer_call` match arm (eval consumer classification)
//!  5. `reify-eval/src/geometry_ops.rs` — `TopologySelectorHelper::ClosestPoint` name-map arm (`try_eval_topology_selector` build dispatch)
//!  6. `reify-eval/src/geometry_ops.rs` — `GeometryQuery::ClosestPointOnShape` kernel-wire request construction
//!  7. `reify-eval/src/geometry_ops.rs` — kernel response-parsing contract for the same call
//!
//! A missed registration at any one of these sites does not fail to
//! compile — it silently degrades the call to `Value::Undef` at eval time.
//! Until this module, cross-crate agreement between sites (1)-(2) and
//! sites (4)-(7) was enforced by **nothing but prose** — the "Maintenance
//! contract" doc-comment above `is_geometry_consumer_call`
//! (`geometry_ops.rs:3904`). The 32 existing `#[cfg(test)]` tests in
//! `reify-compiler/src/units.rs` pin only compiler-**internal** agreement
//! (family-vs-family disjointness, slice-vs-result-type parity) — none of
//! them cross the crate boundary.
//!
//! ## Why in-crate (not `tests/`), and not inside `geometry_ops.rs`
//!
//! The eval-side oracles this module probes (`is_geometry_query_call`,
//! `is_geometry_consumer_call`, `is_symbolic_eval_wired_selector_ctor`) are
//! `pub(crate)` — unreachable from an external integration test under
//! `reify-eval/tests/`. This module lives as a crate-root sibling of
//! `geometry_ops` (hooked from `lib.rs`, mirroring the `realization_read_gamma`
//! precedent) rather than inside `geometry_ops.rs` itself, to keep that
//! ~10k-line file — under active L5 refactor — unlocked.
//!
//! ## What this module does NOT do
//!
//! It does not implement the unified builtin-name registry (Wave-3 /
//! bookmark task δ). It is the first cross-crate validator that migration
//! will need to satisfy.
//!
//! ## Two-direction contract (PRD §7.3)
//!
//! For each (compiler family, eval oracle) pair this module asserts the
//! *correct* set relationship — equality, subset, or disjoint, in the
//! direction(s) that are meaningful for that pair — never blanket
//! equality. `is_geometry_consumer_call` in particular is a **union**
//! spanning several compiler families, so per-pair correctness must be
//! checked deliberately. Every intentional divergence from strict equality
//! is recorded in the single ledger below, seeded by the `angle` gap
//! (task 4952 α).
//!
//! ## Negative-test proof recipe
//!
//! PLACEHOLDER — replaced in step-8 with the exact reproducible recipe
//! (which registry to mutate, which named test goes RED, how to revert)
//! proving this module's assertions are not vacuously true.

use std::collections::BTreeSet;

// ═══════════════════════════════════════════════════════════════════════
// Shared probe harness
// ═══════════════════════════════════════════════════════════════════════

/// Build a synthetic `<name>(arg0, arg1, ..., arg{arity-1})` `FunctionCall`
/// expr whose args are dummy `Type::Geometry`-typed `ValueRef`s. Mirrors
/// `geometry_ops::tests::geom_query_call` / `outer_function_call`
/// (`geometry_ops/tests.rs:6507-6555`) — built entirely from public
/// `reify_ir` / `reify_core` API, so this sibling module can construct probe
/// exprs without reaching into `geometry_ops.rs`.
///
/// The oracles this module probes (`is_geometry_query_call` /
/// `is_geometry_consumer_call` / `is_symbolic_eval_wired_selector_ctor`)
/// match only on `expr.kind`'s function name and arg count — `result_type`
/// and `content_hash` are never inspected by them, so both are filled with
/// placeholder-but-valid values (a real `ContentHash` combine chain;
/// `Type::Geometry`).
fn call_expr(name: &str, arity: usize) -> reify_ir::CompiledExpr {
    let args: Vec<reify_ir::CompiledExpr> = (0..arity)
        .map(|i| {
            reify_ir::CompiledExpr::value_ref(
                reify_core::ValueCellId::new("registry_drift_probe", format!("arg{i}")),
                reify_core::Type::Geometry,
            )
        })
        .collect();
    let mut content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
        .combine(reify_core::ContentHash::of_str(name));
    for arg in &args {
        content_hash = content_hash.combine(arg.content_hash);
    }
    reify_ir::CompiledExpr {
        kind: reify_ir::CompiledExprKind::FunctionCall {
            function: reify_ir::ResolvedFunction {
                name: name.to_string(),
                qualified_name: name.to_string(),
            },
            args,
        },
        result_type: reify_core::Type::Geometry,
        content_hash,
    }
}

/// The subset of `universe` that `oracle` recognizes, probing each name at
/// the given `arity` via [`call_expr`]. Names are returned as owned
/// `String`s (not `&'static str`) so callers can freely set-compare against
/// `BTreeSet<String>`-shaped compiler-family universes without lifetime
/// friction.
fn recognized(
    oracle: impl Fn(&reify_ir::CompiledExpr) -> bool,
    universe: &[&str],
    arity: usize,
) -> BTreeSet<String> {
    universe
        .iter()
        .filter(|name| oracle(&call_expr(name, arity)))
        .map(|name| name.to_string())
        .collect()
}

/// Union of every compiler-side builtin-name family this module compares
/// against the eval-side oracles — the "candidate universe" each probing
/// test draws from. Deduplicated via `BTreeSet` (some families are
/// pairwise-disjoint by contract, but this helper does not assume it).
fn all_family_names() -> Vec<&'static str> {
    let set: BTreeSet<&'static str> = reify_compiler::GEOMETRY_FUNCTION_NAMES
        .iter()
        .chain(reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES)
        .chain(reify_compiler::GEOMETRY_QUERY_NAMES)
        .chain(reify_compiler::GEOMETRY_QUERY_HELPER_NAMES)
        .chain(reify_compiler::GEOMETRY_KINEMATIC_QUERY_NAMES)
        .chain(reify_compiler::DYNAMICS_QUERY_NAMES)
        .chain(reify_compiler::DYNAMICS_CONSTRUCTOR_NAMES)
        .chain(reify_compiler::FEA_ENVELOPE_NAMES)
        .chain(reify_compiler::FIELD_OP_NAMES)
        .copied()
        .collect();
    set.into_iter().collect()
}

/// Convert a compiler-side `&[&str]` name-family slice into an owned
/// `BTreeSet<String>`, so it set-compares directly against [`recognized`]'s
/// output. Shared by every per-family comparison test (steps 1, 3, 5, 7).
fn to_set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

// ═══════════════════════════════════════════════════════════════════════
// Divergence ledger
//
// Single consolidated record of every intentional cross-registry naming
// divergence this module's assertions carve out of what would otherwise be
// strict equality. Each entry names the (family, oracle) pair, the
// diverging name(s), and why. Populated incrementally — task 5055 γ steps
// 2, 4, 6, 8. Empty at scaffold time (pre-2).
// ═══════════════════════════════════════════════════════════════════════

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

/// RED until step-2: `is_geometry_query_call` recognizes only the 4
/// whole-handle scalar/point queries (`area`, `bounding_box`, `centroid`,
/// `volume`) at arity 1 — a STRICT SUBSET of the compiler's 15-member
/// `GEOMETRY_QUERY_NAMES` family (`length`/`perimeter`/the rest route via
/// other paths; see `geometry_ops.rs:3863`). The first three assertions
/// below hold today; the final "family-agreement direction" assertion is
/// deliberately naive strict equality between the two sets, which fails
/// (15 names vs. 4) until step-2 introduces `QUERY_CALL_LEDGER` and relaxes
/// it to `== (GEOMETRY_QUERY_NAMES minus QUERY_CALL_LEDGER)`.
#[test]
fn geometry_query_call_oracle_agrees_with_geometry_query_names_family() {
    let universe = all_family_names();
    let geometry_query_names = to_set(reify_compiler::GEOMETRY_QUERY_NAMES);

    // (a) the recognized set at arity 1 is exactly the 4 whole-handle queries.
    let query_call_set = recognized(crate::geometry_ops::is_geometry_query_call, &universe, 1);
    let expected = to_set(&["area", "bounding_box", "centroid", "volume"]);
    assert_eq!(
        query_call_set, expected,
        "is_geometry_query_call's recognized name-set changed; update this pin \
         (and the QUERY_CALL_LEDGER-based assertion below) if the change is intentional"
    );

    // (b) that set is a subset of the compiler's GEOMETRY_QUERY_NAMES family.
    assert!(
        query_call_set.is_subset(&geometry_query_names),
        "is_geometry_query_call recognizes a name reify_compiler::GEOMETRY_QUERY_NAMES \
         does not claim: {:?}",
        query_call_set
            .difference(&geometry_query_names)
            .collect::<Vec<_>>()
    );

    // (c) arity gate: the same 4 names must NOT be recognized at any arity != 1.
    for name in &expected {
        for &bad_arity in &[0usize, 2, 3] {
            assert!(
                !crate::geometry_ops::is_geometry_query_call(&call_expr(name, bad_arity)),
                "{name:?} must not be recognized by is_geometry_query_call at arity \
                 {bad_arity} (only arity 1 is the whole-handle-query shape)"
            );
        }
    }

    // Family-agreement direction — deliberately naive strict equality.
    // GEOMETRY_QUERY_NAMES has 15 names; the oracle recognizes only 4, so
    // this is RED until step-2 replaces it with the ledgered comparison.
    assert_eq!(
        query_call_set, geometry_query_names,
        "is_geometry_query_call-set must equal GEOMETRY_QUERY_NAMES minus the \
         intentional-divergence ledger (QUERY_CALL_LEDGER, added in step-2)"
    );
}
