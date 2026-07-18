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
//! Verified by temporary mutation-then-revert (task 5055 γ item 4, step-8);
//! no mutating test is committed here — the strict-equality / ledgered
//! assertions below are the permanent regression guards this recipe proves
//! are not vacuously true.
//!
//! **Leg 1 — eval-side oracle, cross-family name.** Add a
//! `| "body_mass_props"` match arm to `is_geometry_consumer_call`
//! (`geometry_ops.rs`, in the `matches!` block that ends `| "angle"`) —
//! `body_mass_props` is a real, already-registered `DYNAMICS_QUERY_NAMES`
//! member (see the note below on why it must be an already-registered name).
//! Run `cargo test -p reify-eval --lib registry_drift`; two tests go RED:
//!   - `geometry_consumer_call_oracle_two_direction_agreement` — fails at its
//!     leading recognized-set pin (`left` gains `"body_mass_props"`).
//!   - `non_geometry_families_are_disjoint_from_eval_geometry_oracles` —
//!     fails naming `DYNAMICS_QUERY_NAMES member(s) {"body_mass_props"}
//!     recognized by is_geometry_consumer_call`, the direct
//!     no-silent-`Value::Undef` guard.
//!     Revert: delete the added match arm.
//!
//! **Leg 2 — compiler-side registry, family slice.** Add
//! `"zzz_drift_probe"` as a new element of `GEOMETRY_QUERY_NAMES`
//! (`reify-compiler/src/units.rs`). Run the same command; three tests go
//! RED, all naming `"zzz_drift_probe"` as the drifted name:
//!   - `geometry_query_call_oracle_agrees_with_geometry_query_names_family`
//!     (the `QUERY_CALL_LEDGER`-adjusted family-agreement assertion)
//!   - `geometry_consumer_call_oracle_two_direction_agreement` (Direction A)
//!   - `result_type_maps_agree_with_their_name_families_cross_crate`
//!     (`reify_compiler::geometry_query_result_type`'s `Some`-set no longer
//!     matches `GEOMETRY_QUERY_NAMES`)
//!     Revert: delete the added slice element.
//!
//! **Why Leg 1 must reuse an already-registered name, not a wholly novel
//! string:** every test in this module draws its candidate names from
//! [`all_family_names`] — the union of the *compiler-side* registries only.
//! A name recognized by an eval-side oracle that is not a member of ANY
//! compiler family is never even constructed into a probe `CompiledExpr`
//! (confirmed: adding a fabricated `| "zzz_drift_probe"` arm to
//! `is_geometry_consumer_call` alone, with no matching compiler-side
//! registration, leaves all 5 tests GREEN). This module detects drift
//! **among already-registered names** — an eval oracle recognizing a name
//! nothing on the compiler side ever registers is a dead-code/typo
//! question the type checker's unresolved-function-call diagnostics
//! already answer, not this module's job.

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

/// The subset of `universe` for which the `fn(name: &str) -> Option<_>`-shaped
/// compiler-side result-type map `f` returns `Some`. Unlike [`recognized`],
/// these maps take a bare name (no synthetic `CompiledExpr` needed) — used to
/// probe `reify_compiler::topology_selector_result_type` /
/// `geometry_query_result_type` for cross-crate result-type-map parity
/// (step-7).
fn some_names<T>(f: impl Fn(&str) -> Option<T>, universe: &[&str]) -> BTreeSet<String> {
    universe
        .iter()
        .filter(|name| f(name).is_some())
        .map(|name| name.to_string())
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════
// Divergence ledger  (PRD docs/prds/v0_6/compiler-type-hygiene.md §7.3)
//
// Single consolidated record of every intentional cross-registry naming
// divergence this module's assertions carve out of what would otherwise be
// strict equality, per the two-direction contract: Direction A (compiler ⇒
// eval) asks whether every compiler-registered name in a family is
// recognized on the eval side; Direction B (eval ⇒ compiler) asks whether
// every eval-recognized name is claimed by some compiler family (the core
// no-silent-`Value::Undef` guard). Each entry below names the (family,
// oracle) pair, the diverging name(s), and why.
//
// Finalized at task 5055 γ step-8; populated across step-2
// (`QUERY_CALL_LEDGER`), step-4 (the `angle` seed entry +
// `CONSUMER_DIRECTION_A_LEDGER`), and step-6 (`SELECTOR_BUILD_PATH_ONLY`).
// Four divergences total. A fifth would only appear alongside a new
// compiler-side registration or eval-side oracle change — see the
// "Negative-test proof recipe" above for how to confirm this ledger is
// exhaustive, not vacuous.
// ═══════════════════════════════════════════════════════════════════════

/// `GEOMETRY_QUERY_NAMES` members `is_geometry_query_call` intentionally
/// does NOT recognize — it matches only the 4-name, 1-arg whole-handle-query
/// shape (`volume`/`area`/`centroid`/`bounding_box`, `geometry_ops.rs:3865`).
/// Each of the 11 names below reaches its result through a different
/// eval-time path. Step-2 (task 5055 γ).
const QUERY_CALL_LEDGER: &[&str] = &[
    // ── Kernel-bearing `TopologySelectorHelper` consumer path instead
    //    (name map at geometry_ops.rs:5314-5352, dispatched via
    //    `try_eval_topology_selector`'s build() case) ──────────────────────
    // `length` / `perimeter`: delivered via `dispatch_edge_length` /
    // `dispatch_perimeter` on the edge/face topology-selector path;
    // `GeometryQuery` has no whole-handle variant for either (module note
    // above `try_eval_geometry_query`, geometry_ops.rs:3626-3630).
    "length",
    "perimeter",
    // `distance` / `contains` / `intersects` / `geo_equiv` / `curvature` /
    // `normal`: each is a `TopologySelectorHelper` kernel-bearing consumer
    // name (`is_geometry_consumer_call`'s match arms, geometry_ops.rs:3915),
    // not a whole-handle 1-arg query.
    "distance",
    "contains",
    "intersects",
    "geo_equiv",
    "curvature",
    "normal",
    // `angle`: also dispatched via the build()-only `TopologySelectorHelper`
    // path despite being pure-math (acos/clamp/dot) — a pre-existing gap in
    // the consumer allow-list, not a deliberate exclusion (task 4952 α;
    // verbatim rationale at geometry_ops.rs:3942-3950). Re-ledgered in
    // step-4 as the seed entry for the consumer-oracle direction.
    "angle",
    // ── Distinct dispatch shapes, not `TopologySelectorHelper` routing ────
    // `max_deviation`: 2-arg DIRECT dispatch (`actual`, `nominal`) via
    // `try_eval_geometry_query`'s ζ/C4 case (geometry_ops.rs:3703-3741;
    // task 4479) — kept separate from the 1-arg `is_geometry_query_call`
    // invariant by design.
    "max_deviation",
    // `feature`: returns `Type::Feature`, not a scalar/point query result;
    // dispatched by the dedicated `try_eval_feature_accessor`
    // (geometry_ops.rs:3842), not the query-call oracle.
    "feature",
];

// ── Seed entry: the `angle` gap (task 4952 α) ───────────────────────────
//
// `angle` IS recognized by `is_geometry_consumer_call` and IS a
// `GEOMETRY_QUERY_NAMES` member, so it does NOT appear in
// `CONSUMER_DIRECTION_A_LEDGER` below — Direction A holds for it without
// relaxation. It is recorded here anyway because the *classification*
// itself is a known gap, not because any assertion needs it ledgered.
// Verbatim rationale from `geometry_ops.rs:3942-3950` (comment inside
// `is_geometry_consumer_call`):
//
//   task 3614 (KGQ-ε): dispatched via the same build()-only
//   TopologySelectorHelper::Angle path as `angle_between_surfaces`
//   (try_eval_topology_selector) even though its own computation
//   is pure-math (acos/clamp/dot) — a pre-existing gap in this
//   allow-list (task 4952 α), not a deliberate exclusion: no
//   classifier test asserted `angle` == false, and its own
//   pinning test (`kernel_queries_angle_smoke.rs`) resolves it
//   via `engine.build()`, not `engine.eval()`.
//
// task 5055 γ records the gap rather than silently baking it into a
// passing assertion; fixing the classification itself is out of scope
// (PRD §7.3 / Wave-3 bookmark task δ).

/// `GEOMETRY_QUERY_NAMES` members intentionally NOT routed through
/// `is_geometry_consumer_call` — Direction A (compiler ⇒ eval) divergences.
/// Step-4 (task 5055 γ).
const CONSUMER_DIRECTION_A_LEDGER: &[&str] = &[
    // 2-arg DIRECT dispatch (`actual`, `nominal`) via `try_eval_geometry_query`'s
    // ζ/C4 case (geometry_ops.rs:3703-3741; task 4479) — a GD&T 2-operand
    // query, not a name `is_geometry_consumer_call`'s match arms enumerate.
    "max_deviation",
    // Returns `Type::Feature`, not a scalar/point/selector consumer result;
    // dispatched by the dedicated `try_eval_feature_accessor`
    // (geometry_ops.rs:3842), not the consumer oracle.
    "feature",
];

/// `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` members recognized by NEITHER
/// `is_symbolic_eval_wired_selector_ctor` (kernel-free) NOR
/// `is_geometry_consumer_call` (kernel-bearing) — the named-leaf ctors
/// (`face`, `edge`, `solid_body`, `vertex`; task 4119 δ / 4368) plus `split`
/// (task 4190). All five resolve ONLY via `try_eval_topology_selector`'s
/// build() case (`geometry_ops.rs:6504`), which requires a realized kernel.
///
/// They are deliberately NOT probed directly: `try_eval_topology_selector`
/// returns `Option`, and without a kernel it returns `None` for BOTH an
/// unrecognized name and a recognized-but-eval-failed call — it cannot
/// distinguish "name unknown" from "name known, kernel absent", so it is not
/// a sound name oracle (see the module-level design-decision doc / PRD
/// §7.3). Their compiler-side presence is instead pinned indirectly via the
/// result-type-map parity check in step-7
/// (`reify_compiler::topology_selector_result_type`).
///
/// Step-6 (task 5055 γ).
const SELECTOR_BUILD_PATH_ONLY: &[&str] = &["face", "edge", "solid_body", "vertex", "split"];

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

/// `is_geometry_query_call` recognizes only the 4 whole-handle scalar/point
/// queries (`area`, `bounding_box`, `centroid`, `volume`) at arity 1 — a
/// STRICT SUBSET of the compiler's 15-member `GEOMETRY_QUERY_NAMES` family.
/// The remaining 11 members reach their result through other eval-time
/// paths, recorded in [`QUERY_CALL_LEDGER`]. The final "family-agreement
/// direction" assertion holds against `GEOMETRY_QUERY_NAMES minus
/// QUERY_CALL_LEDGER`, not raw `GEOMETRY_QUERY_NAMES` — the ledger is what
/// keeps this a real drift guard rather than a vacuous pass (step-2).
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

    // Family-agreement direction, ledgered: is_geometry_query_call-set must
    // equal GEOMETRY_QUERY_NAMES minus the 11 intentionally-excluded names
    // in QUERY_CALL_LEDGER (each with its own routing rationale above).
    let ledger = to_set(QUERY_CALL_LEDGER);
    let expected_family: BTreeSet<String> = geometry_query_names
        .difference(&ledger)
        .cloned()
        .collect();
    assert_eq!(
        query_call_set, expected_family,
        "is_geometry_query_call-set must equal GEOMETRY_QUERY_NAMES minus \
         QUERY_CALL_LEDGER — either the oracle's recognized set or the ledger \
         drifted out of sync with reify_compiler::GEOMETRY_QUERY_NAMES"
    );
}

/// `is_geometry_consumer_call` recognizes 22 names — the `is_geometry_query_call`
/// family plus every kernel-bearing `TopologySelectorHelper` consumer (module
/// doc above `is_geometry_consumer_call`, `geometry_ops.rs:3877-3902`). Both
/// directions of the two-direction contract (PRD §7.3) are checked:
///
/// - Direction B (eval ⇒ compiler): every name the oracle recognizes must be
///   claimed by at least one geometry compiler family (`GEOMETRY_QUERY_NAMES`
///   ∪ `GEOMETRY_TOPOLOGY_SELECTOR_NAMES`) — the core no-silent-`Value::Undef`
///   guard. Holds today.
/// - Direction A (compiler ⇒ eval): every `GEOMETRY_QUERY_NAMES` member
///   outside [`CONSUMER_DIRECTION_A_LEDGER`] must be a recognized consumer.
///   `max_deviation` and `feature` are in `GEOMETRY_QUERY_NAMES` but not
///   recognized — each dispatches via a distinct path (see both the ledger
///   entries and [`QUERY_CALL_LEDGER`]'s entries for the same two names) —
///   so they are ledgered rather than asserted equal (step-4).
///
/// Also probes that `GEOMETRY_FUNCTION_NAMES` constructors (`box`,
/// `cylinder`, `union`) are never misclassified as consumers.
#[test]
fn geometry_consumer_call_oracle_two_direction_agreement() {
    let universe = all_family_names();
    let consumer_set = recognized(crate::geometry_ops::is_geometry_consumer_call, &universe, 1);

    // The recognized set is exactly the documented 22 names.
    let expected = to_set(&[
        "volume",
        "area",
        "centroid",
        "bounding_box",
        "adjacent_faces",
        "normal",
        "closest_point",
        "shared_edges",
        "siblings_of_face",
        "ancestor_faces_of_edge",
        "length",
        "perimeter",
        "curvature",
        "center_of_mass",
        "moment_of_inertia",
        "distance",
        "contains",
        "intersects",
        "geo_equiv",
        "is_on",
        "angle_between_surfaces",
        "angle",
    ]);
    assert_eq!(
        consumer_set, expected,
        "is_geometry_consumer_call's recognized name-set changed; update this pin \
         if the change is intentional"
    );

    // Direction B (eval => compiler): every recognized name is claimed by
    // >=1 geometry compiler family. Core no-silent-Undef guard.
    let geometry_query_names = to_set(reify_compiler::GEOMETRY_QUERY_NAMES);
    let geometry_selector_names = to_set(reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES);
    let geometry_family_union: BTreeSet<String> = geometry_query_names
        .union(&geometry_selector_names)
        .cloned()
        .collect();
    assert!(
        consumer_set.is_subset(&geometry_family_union),
        "is_geometry_consumer_call recognizes a name no geometry compiler family \
         (GEOMETRY_QUERY_NAMES union GEOMETRY_TOPOLOGY_SELECTOR_NAMES) claims — \
         such a call would silently degrade to Value::Undef: {:?}",
        consumer_set
            .difference(&geometry_family_union)
            .collect::<Vec<_>>()
    );

    // Direction A (compiler => eval), ledgered: every GEOMETRY_QUERY_NAMES
    // member minus CONSUMER_DIRECTION_A_LEDGER must be a recognized consumer.
    let consumer_direction_a_ledger = to_set(CONSUMER_DIRECTION_A_LEDGER);
    let expected_query_consumers: BTreeSet<String> = geometry_query_names
        .difference(&consumer_direction_a_ledger)
        .cloned()
        .collect();
    let query_consumer_subset: BTreeSet<String> = consumer_set
        .intersection(&geometry_query_names)
        .cloned()
        .collect();
    assert_eq!(
        expected_query_consumers, query_consumer_subset,
        "every GEOMETRY_QUERY_NAMES member outside CONSUMER_DIRECTION_A_LEDGER \
         must be recognized by is_geometry_consumer_call — either the oracle's \
         recognized set or the ledger (CONSUMER_DIRECTION_A_LEDGER) drifted out \
         of sync"
    );

    // GEOMETRY_FUNCTION_NAMES constructors are never consumers.
    for ctor in ["box", "cylinder", "union"] {
        assert!(
            !crate::geometry_ops::is_geometry_consumer_call(&call_expr(ctor, 1)),
            "{ctor:?} is a GEOMETRY_FUNCTION_NAMES constructor and must not be \
             recognized as a geometry consumer"
        );
    }
}

/// `is_symbolic_eval_wired_selector_ctor` recognizes the 17 kernel-free leaf
/// selector ctors (`symbolic_eval_helper_for_name`'s match arms,
/// `geometry_ops.rs:5318-5344`) — the kernel-free half of the eval-side
/// selector-recognition surface. The other half is the kernel-bearing
/// selector names `is_geometry_consumer_call` also recognizes (the subset of
/// its 22 names — `closest_point`, `is_on`, `angle_between_surfaces`,
/// `adjacent_faces`, `shared_edges`, `siblings_of_face`,
/// `ancestor_faces_of_edge`, `center_of_mass`, `moment_of_inertia` — that are
/// `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` members rather than
/// `GEOMETRY_QUERY_NAMES` members).
///
/// - (a)/(b) Direction B (eval ⇒ compiler): the kernel-free set alone, and
///   the full eval-side selector-recognition UNION, are both subsets of
///   `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` — no eval-side selector oracle
///   recognizes a name the compiler doesn't claim.
/// - Direction A (compiler ⇒ eval): the union covers 26 of 31
///   `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` members directly; the remaining 5 —
///   `face`, `edge`, `solid_body`, `vertex`, `split` — are recognized by
///   NEITHER boolean oracle (they dispatch only via
///   `try_eval_topology_selector`'s build() path) and are ledgered in
///   [`SELECTOR_BUILD_PATH_ONLY`] rather than asserted equal (step-6).
#[test]
fn topology_selector_eval_recognition_agrees_with_selector_names_family() {
    let universe = all_family_names();
    let selector_names = to_set(reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES);

    // The kernel-free leaf-ctor set is exactly the documented 17 names.
    let kernel_free_ctor_set = recognized(
        crate::geometry_ops::is_symbolic_eval_wired_selector_ctor,
        &universe,
        1,
    );
    let expected_kernel_free = to_set(&[
        "faces",
        "edges",
        "mid_surface",
        "vertices",
        "edges_by_length",
        "faces_by_area",
        "faces_by_normal",
        "edges_parallel_to",
        "edges_at_height",
        "faces_perpendicular_to",
        "edges_perpendicular_to",
        "faces_by_surface_kind",
        "edges_by_curve_kind",
        "extremal_by_bbox",
        "extremal_by_centroid",
        "created_by_feature",
        "split_by_feature",
    ]);
    assert_eq!(
        kernel_free_ctor_set, expected_kernel_free,
        "is_symbolic_eval_wired_selector_ctor's recognized name-set changed; \
         update this pin if the change is intentional"
    );

    // (a) the kernel-free set is a subset of GEOMETRY_TOPOLOGY_SELECTOR_NAMES.
    assert!(
        kernel_free_ctor_set.is_subset(&selector_names),
        "is_symbolic_eval_wired_selector_ctor recognizes a name \
         reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES does not claim: {:?}",
        kernel_free_ctor_set
            .difference(&selector_names)
            .collect::<Vec<_>>()
    );

    // The kernel-bearing selector names is_geometry_consumer_call recognizes
    // — the subset of its 22 names that are selectors, not queries.
    let consumer_set = recognized(crate::geometry_ops::is_geometry_consumer_call, &universe, 1);
    let kernel_bearing_selector_subset: BTreeSet<String> = consumer_set
        .intersection(&selector_names)
        .cloned()
        .collect();

    // The eval-side selector-recognition UNION: kernel-free ctors plus
    // kernel-bearing selector consumers.
    let eval_selector_union: BTreeSet<String> = kernel_free_ctor_set
        .union(&kernel_bearing_selector_subset)
        .cloned()
        .collect();

    // (b) Direction B (eval => compiler): the full union is a subset too.
    assert!(
        eval_selector_union.is_subset(&selector_names),
        "an eval-side selector oracle recognizes a name \
         reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES does not claim: {:?}",
        eval_selector_union
            .difference(&selector_names)
            .collect::<Vec<_>>()
    );

    // Direction A (compiler => eval), ledgered: the eval-side selector union
    // must equal GEOMETRY_TOPOLOGY_SELECTOR_NAMES minus SELECTOR_BUILD_PATH_ONLY.
    let selector_build_path_only = to_set(SELECTOR_BUILD_PATH_ONLY);
    let expected_selector_family: BTreeSet<String> = selector_names
        .difference(&selector_build_path_only)
        .cloned()
        .collect();
    assert_eq!(
        eval_selector_union, expected_selector_family,
        "eval-side selector recognition (kernel-free ctors union kernel-bearing \
         selector consumers) must equal GEOMETRY_TOPOLOGY_SELECTOR_NAMES minus \
         SELECTOR_BUILD_PATH_ONLY — either the oracles' recognized sets or the \
         ledger drifted out of sync"
    );
}

/// Cross-crate **disjointness** regression pin — a surface the 32 in-crate
/// `units.rs` tests do not span (those pin compiler-internal family-vs-family
/// disjointness only). A member of any non-geometry compiler family
/// (`DYNAMICS_QUERY_NAMES`, `DYNAMICS_CONSTRUCTOR_NAMES`, `FEA_ENVELOPE_NAMES`,
/// `FIELD_OP_NAMES`, `GEOMETRY_FUNCTION_NAMES`) must never be recognized by
/// any of the three eval-side geometry oracles this module probes
/// (`is_geometry_query_call`, `is_geometry_consumer_call`,
/// `is_symbolic_eval_wired_selector_ctor`) — a dynamics/FEA/field-op/constructor
/// name can never masquerade as a geometry query, consumer, or kernel-free
/// selector ctor. Step-7 (task 5055 γ).
#[test]
fn non_geometry_families_are_disjoint_from_eval_geometry_oracles() {
    let non_geometry_families: [(&str, &[&str]); 5] = [
        ("DYNAMICS_QUERY_NAMES", reify_compiler::DYNAMICS_QUERY_NAMES),
        (
            "DYNAMICS_CONSTRUCTOR_NAMES",
            reify_compiler::DYNAMICS_CONSTRUCTOR_NAMES,
        ),
        ("FEA_ENVELOPE_NAMES", reify_compiler::FEA_ENVELOPE_NAMES),
        ("FIELD_OP_NAMES", reify_compiler::FIELD_OP_NAMES),
        (
            "GEOMETRY_FUNCTION_NAMES",
            reify_compiler::GEOMETRY_FUNCTION_NAMES,
        ),
    ];

    for (family_name, family) in non_geometry_families {
        let query_hits = recognized(crate::geometry_ops::is_geometry_query_call, family, 1);
        assert!(
            query_hits.is_empty(),
            "{family_name} member(s) {query_hits:?} recognized by is_geometry_query_call \
             — a {family_name} name must never masquerade as a whole-handle geometry query"
        );

        let consumer_hits = recognized(crate::geometry_ops::is_geometry_consumer_call, family, 1);
        assert!(
            consumer_hits.is_empty(),
            "{family_name} member(s) {consumer_hits:?} recognized by \
             is_geometry_consumer_call — a {family_name} name must never masquerade as a \
             geometry consumer"
        );

        let selector_ctor_hits = recognized(
            crate::geometry_ops::is_symbolic_eval_wired_selector_ctor,
            family,
            1,
        );
        assert!(
            selector_ctor_hits.is_empty(),
            "{family_name} member(s) {selector_ctor_hits:?} recognized by \
             is_symbolic_eval_wired_selector_ctor — a {family_name} name must never \
             masquerade as a kernel-free selector ctor"
        );
    }
}

/// Cross-crate **result-type-map parity** regression pin — now reachable
/// post-pre-1's export widening. `topology_selector_result_type` /
/// `geometry_query_result_type` are the compile-time halves of the >= 7
/// registration-site contract documented in the module header (sites (1)-(2)
/// for `closest_point`); this pins that their `Some`-set exactly equals the
/// corresponding compiler-side name family, from the eval crate's vantage
/// point — the same parity the 32 in-crate `units.rs` tests already check one
/// level down (slice-vs-result-type, WITHIN `units.rs`). Step-7 (task 5055 γ).
#[test]
fn result_type_maps_agree_with_their_name_families_cross_crate() {
    let universe = all_family_names();

    let topology_selector_result_set =
        some_names(reify_compiler::topology_selector_result_type, &universe);
    let topology_selector_names = to_set(reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES);
    assert_eq!(
        topology_selector_result_set, topology_selector_names,
        "reify_compiler::topology_selector_result_type's Some-set must equal \
         GEOMETRY_TOPOLOGY_SELECTOR_NAMES (31/31) — a name added to one without the \
         other silently falls through to the first-arg type-inference fallback"
    );

    let geometry_query_result_set =
        some_names(reify_compiler::geometry_query_result_type, &universe);
    let geometry_query_names = to_set(reify_compiler::GEOMETRY_QUERY_NAMES);
    assert_eq!(
        geometry_query_result_set, geometry_query_names,
        "reify_compiler::geometry_query_result_type's Some-set must equal \
         GEOMETRY_QUERY_NAMES (15/15) — a name added to one without the other \
         silently falls through to the first-arg type-inference fallback"
    );
}
