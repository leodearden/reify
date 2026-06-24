//! Boundary tests pinning D1/INV-1: graph `Freshness::Failed` vs language
//! Option-recovery orthogonality (result-and-fallback PRD §8 Phase 3 / §9.6).
//!
//! Two sides:
//!   (a) Determined-`none` IS recovered: `unwrap_or(none, dflt)` evaluates to
//!       `dflt` with `Freshness::Final`; `unwrap_or(some(x), dflt)` evaluates to
//!       `x` (tag-driven, not blanket-default).
//!   (b) Graph-`Failed` is NOT recovered — two facets:
//!       Facet 1: set_panic_on_eval on the consumer LET cell itself keeps the
//!                cell `Freshness::Failed`; the language-recovery combinator
//!                never fires.
//!       Facet 2 (C-4): a downstream `unwrap_or` consumer of a `Failed`
//!                upstream becomes `Freshness::Pending` rooted at the failed
//!                node — it is NOT recovered to the default.
//!
//! All assertions observe only the engine's public read paths:
//! `Engine::eval`, `Engine::cache_store().freshness()/.pending_cause()`,
//! `Engine::journal()`, `result.values`.  No production code is changed.
//!
//! Relies on the `test-instrumentation` feature, wired via the self-dev-dep in
//! `crates/reify-eval/Cargo.toml` — no Cargo change needed.

use reify_constraints::SimpleConstraintChecker;
use reify_core::ValueCellId;
use reify_eval::Engine;
use reify_eval::EvalResult;
use reify_eval::cache::NodeId;
use reify_eval::journal::EventKind;
use reify_ir::{Freshness, Value};
use reify_test_support::{collect_errors, mm, parse_and_compile_with_stdlib};

/// Side (a): `unwrap_or(none, 6mm)` recovers the determined-`none` to its
/// default `6mm` with `Freshness::Final`; the companion `unwrap_or(some(5mm),
/// 6mm)` evaluates to `5mm` (tag-driven, not the blanket default).
///
/// Pins INV-1 from the "language recovery works on determined-none" side.
#[test]
fn determined_none_is_recovered_by_unwrap_or_to_default_final() {
    let src = r#"
structure S {
    param o_none : Option<Length> = none
    param o_some : Option<Length> = some(5mm)
    let recovered = unwrap_or(o_none, 6mm)
    let kept = unwrap_or(o_some, 6mm)
}
"#;
    let (mut engine, result) = eval_module(src);

    // (1) Determined none recovers to the default 6mm.
    let recovered_id = ValueCellId::new("S", "recovered");
    let recovered = result.values.get(&recovered_id).unwrap_or_else(|| {
        panic!(
            "eval must produce S.recovered; available: {:?}",
            result.values.iter().map(|(k, _)| k.to_string()).collect::<Vec<_>>()
        )
    });
    assert_eq!(
        *recovered,
        mm(6.0),
        "unwrap_or(none, 6mm) must recover to 6mm; got {:?}",
        recovered
    );

    // (2) Freshness of the recovered cell is Final (clean eval, not stale).
    let recovered_node = NodeId::Value(recovered_id.clone());
    assert_eq!(
        engine.cache_store().freshness(&recovered_node),
        Freshness::Final,
        "S.recovered must have Freshness::Final after clean eval"
    );

    // (3) Tag-driven: some(5mm) yields 5mm, not the 6mm default.
    let kept_id = ValueCellId::new("S", "kept");
    let kept = result.values.get(&kept_id).expect("eval must produce S.kept");
    assert_eq!(
        *kept,
        mm(5.0),
        "unwrap_or(some(5mm), 6mm) must yield 5mm (tag-driven); got {:?}",
        kept
    );
}
