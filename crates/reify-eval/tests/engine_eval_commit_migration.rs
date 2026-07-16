//! Integration tests for task γ (#5053): migrating engine_eval.rs's per-cell
//! eval-and-commit transaction/ctx sites onto `commit_cell_result` (task α,
//! `cell_commit.rs`) built with the free-function `cell_eval_ctx` (task β,
//! `cell_eval_ctx.rs`).
//!
//! CHARACTERIZATION-FIRST / behaviour-preserving: each migrated site keeps
//! its current determinacy rule and cache-recording decision. Every test
//! below is written RED-first against TODAY's baseline behaviour (no
//! `commit_cell_result` call at its target site yet), pins the preserved
//! value/determinacy, and turns GREEN only once the matching `impl-N` step
//! wires the site onto the primitive. See `docs/prds/v0_6/eval-cell-commit-substrate.md`
//! §2, §7 (B2/B3 — B1 is out of γ's scope per esc-5053-2 Option A).
//!
//! This file grows incrementally: this first commit (prerequisite `pre-1`)
//! adds shared scaffolding only — the `started_payload` provenance-reading
//! helper and the trigger `.ri` sources every subsequent RED test drives —
//! with no test functions yet. `test-1` onward each add one `#[test]` fn.

use reify_core::ValueCellId;
// Consumed starting at test-3/test-7 (`engine.eval_cached(&module, VersionId(..))`).
#[allow(unused_imports)]
use reify_core::VersionId;
use reify_eval::Engine;
use reify_eval::cache::NodeId;
use reify_eval::journal::{EventKind, EventPayload};
// Consumed starting at test-1 (compiling/evaluating the trigger sources;
// `mm` builds the guarded-group's rejected-override value).
#[allow(unused_imports)]
use reify_test_support::{make_engine, mm, parse_and_compile};

/// Returns the payload of the FIRST `EventKind::Started` event recorded for
/// `id` in `engine`'s journal, IF that payload is `EventPayload::Custom` —
/// `None` if there is no `Started` event for `id`, or if its payload isn't
/// `Custom` (e.g. today's baseline `payload: None` at the not-yet-migrated
/// sites).
///
/// This is the observability hook every RED provenance test in this file
/// asserts against: `commit_cell_result` (`cell_commit.rs`) records the
/// migration's additive `TraceSource` slug on the `Started` event's
/// `EventPayload::Custom` — verbatim on `CacheLeg::Record`, or suffixed
/// `|cache-skip=<reason>` on `CacheLeg::Skip` — so a test asserting
/// `started_payload(&engine, &cell) == Some("<slug>")` is RED before the
/// matching site is migrated (today's payload is `None` or absent) and GREEN
/// after.
#[allow(dead_code)] // first consumer lands at test-1
fn started_payload(engine: &Engine, id: &ValueCellId) -> Option<String> {
    let node_id = NodeId::Value(id.clone());
    let events = engine.journal().events_for_node(&node_id);
    let started = events
        .iter()
        .find(|event| matches!(event.kind, EventKind::Started))?;
    match &started.payload {
        Some(EventPayload::Custom(slug)) => Some(slug.clone()),
        _ => None,
    }
}

/// Guarded group with an accepted-default Param (`x_ok`) and a
/// rejected-override-no-default Param (`x_rejected`) in the SAME `where`
/// block — drives `eval_guarded_group_param_cell` (engine_eval.rs, ~@2132).
///
/// `x_rejected` has no default_expr; a test sets an incompatible-type-kind
/// override on it BEFORE eval (a `Value::Scalar` against its `Int`
/// cell_type) via `Engine::set_param_and_invalidate`, so
/// `validate_param_override` rejects it with `TypeKindMismatch` and the
/// helper's rejected-override-no-default arm fires — this is the parity
/// fixture's rejected-override cell (writes `(Undef, Undetermined)`).
/// `x_ok` has no override ever set, so its default_expr (`5mm`) evaluates
/// and the helper's override/default-accepted arm fires (writes
/// `(val, Determined)`).
///
/// Verified (scratch run, prerequisite pre-1): compiles+evals cleanly with
/// `x_ok` = `Scalar(0.005m, LENGTH)`, `x_rejected` = `Undef`, and exactly one
/// `Warning` diagnostic ("type-kind mismatch") — reaches the target site as
/// intended.
#[allow(dead_code)] // first consumer lands at test-1
const GUARDED_GROUP_SRC: &str = r#"
    structure S {
        param active : Bool = true
        where active {
            param x_ok : Length = 5mm
            param x_rejected : Int
        }
    }
"#;

/// A `Let` whose default_expr is a structural query (`self.members`) —
/// drives the structural-query post-pass (engine_eval.rs, ~@3894-3903).
/// Reaches the post-pass regardless of whether `S` declares any `sub`s: 0
/// subs evaluates `self.members` to `Value::List(vec![])`.
///
/// Verified (scratch run, prerequisite pre-1): compiles+evals cleanly with
/// `ms` = `List([])` and no diagnostics.
#[allow(dead_code)] // first consumer lands at test-4
const STRUCTURAL_QUERY_SRC: &str = r#"
    structure S {
        let ms = self.members
    }
"#;

/// A `Let` whose default_expr is a self-datum projection (`self.xy_plane`) —
/// drives the self-datum projection post-pass (engine_eval.rs, ~@3984-3993).
/// Kernel-free: `self.xy_plane` resolves to the intrinsic identity-frame
/// `xy` plane constant with no geometry kernel needed.
///
/// Verified (scratch run, prerequisite pre-1): compiles+evals cleanly with
/// `p` = a concrete `Value::Plane { .. }` (not `Undef`) and no diagnostics.
#[allow(dead_code)] // first consumer lands at test-5
const SELF_DATUM_SRC: &str = r#"
    structure S {
        let p = self.xy_plane
    }
"#;
