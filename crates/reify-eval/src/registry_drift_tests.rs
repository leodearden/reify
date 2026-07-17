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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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

// ═══════════════════════════════════════════════════════════════════════
// Divergence ledger
//
// Single consolidated record of every intentional cross-registry naming
// divergence this module's assertions carve out of what would otherwise be
// strict equality. Each entry names the (family, oracle) pair, the
// diverging name(s), and why. Populated incrementally — task 5055 γ steps
// 2, 4, 6, 8. Empty at scaffold time (pre-2).
// ═══════════════════════════════════════════════════════════════════════
