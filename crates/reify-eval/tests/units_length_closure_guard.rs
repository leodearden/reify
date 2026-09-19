//! Closure guard for the Contract C length-dimension gate over every geometry
//! builtin (PRD `docs/prds/v0_6/units-length-gate-completion.md` task ι,
//! contracts C5 + C7, decisions D6 / D14, INV-SF-5 `placeholders-owned-and-loud`).
//!
//! Contract C's completeness claim used to be PROSE — a hand-maintained
//! enumeration in `reify_ir::arg_acceptance`'s module doc of which positions are
//! gated, which are deliberately not, and who owns the rest. D6 replaces that
//! enumeration with the machine check in this file: a behavioural probe over the
//! WHOLE builtin universe that asserts every numeric position it reaches is
//! EITHER rejected by Contract C when handed a bare number, OR carries a
//! justified allowlist entry or an owned residual row. A prose list cannot go
//! red; this can.
//!
//! # The universe, and why it is read at test time
//!
//! [`probe_universe`] returns `reify_compiler::GEOMETRY_FUNCTION_NAMES`
//! verbatim. The names are NOT restated here: a copied list is a second source
//! of truth that drifts silently the first time a builtin is added. The const is
//! reachable cross-crate through the crate-ROOT re-export in
//! `reify-compiler/src/lib.rs` (`pub use units::{… GEOMETRY_FUNCTION_NAMES …}`,
//! widened from `pub(crate)` by task 5055 γ for exactly this purpose). `mod
//! units` itself is PRIVATE, so the `reify_compiler::units::…` spelling used in
//! the PRD text does not resolve — `reify_compiler::GEOMETRY_FUNCTION_NAMES`
//! does.
//!
//! # The vacuity lesson C5 encodes
//!
//! The probe universe and the assertion target are INDEPENDENT. The universe
//! comes from the COMPILER's builtin-name registry; what is asserted is the
//! EVAL-side gate behaviour observed through
//! `reify_eval::geometry_op_characterization_probe::compile_geometry_op_probe`.
//! Neither list is derived from the other, so a builtin cannot hide from this
//! guard by being absent from the thing the guard checks.
//!
//! Contrast `arg_slot_keys_are_registered_builtin_names`
//! (`reify-compiler/src/builtin_signatures.rs:2208`), which draws its universe
//! from the very list it asserts over and therefore cannot fail for an entry
//! that is missing from both. That shape is the vacuity C5 was written against.
//!
//! # Methodology
//!
//! Three banners segment the body, and they are also the order in which the
//! machinery is introduced:
//!
//! * **Step-1 — the universe.** [`probe_universe`] plus the arity bound, and the
//!   seeded floor test that keeps the sweep from silently collapsing.
//! * **Step-2 — the registry and the classifier.** `Position`, `Justification`,
//!   `AllowEntry`, `Residual`, `Registry`, `Violation` and the pure
//!   `classify_all`, exercised by SEEDED in-memory self-tests that prove the
//!   classifier FIRES, that an entry suppresses EXACTLY its own position, and
//!   that a stubbed-out gate is caught. These tests build their observations by
//!   hand; they compile no source and touch no filesystem.
//! * **Step-3 — the real tree.** The sweep over the shipped universe and the
//!   shipped registry, plus its two anti-vacuity companions (shrink the
//!   allowlist by one entry; stub one observed gate) which prove the green
//!   result in between them is load-bearing.
//!
//! # Extension points — PRD 3 and PRD 5 are ADDITIONS, never a rewrite
//!
//! `Registry` is a PARAMETER of [`classify_all`], never a global, and
//! [`AllowEntry`] carries the `DimensionVector` it expects. So the ANGLE
//! positions of `docs/prds/v0_6/angle-units-surface-convergence.md` (PRD 3) join
//! by adding rows with `expected: DimensionVector::ANGLE`, and a SECOND universe
//! (PRD 5's `plane_*` / `axis_*` / `point3`, `prb_*`, joints and solver readers)
//! joins by adding a second `probe_universe`-shaped source. Neither touches the
//! classifier, and both inherit the anti-vacuity tests for free.
//!
//! # ACCEPTED LIMITATIONS — read these before trusting a green run
//!
//! **This guard is IR-BUILD ONLY.** It compiles `.ri` source to
//! `CompiledGeometryOp`s and drives them through the eval-side op compiler. No
//! geometry kernel is ever constructed, so behaviour BELOW that boundary — what
//! OCCT, Manifold or OpenVDB do with an accepted value — is out of its reach
//! entirely, and a green run says nothing about it.
//!
//! **A position this guard cannot REACH is recorded, never silently passed.**
//! The probe synthesizes calls from scalar literals. A builtin whose arguments
//! are geometry operands, lists or grids has positions no scalar literal can
//! occupy; an arity whose baseline cannot be made rejection-free has no clean
//! reading at all. Every such case becomes a [`Residual`] row with a live task
//! cite — which is what makes the gap visible to the PTODO detector, and
//! therefore loud when its owner closes. A `ProbeOutcome::NotReached` position
//! raises no violation; that is the one place this guard is deliberately silent,
//! and it is bounded by the residual rows that name the builtins it covers.

// -- Step-1: the universe --

/// Every geometry builtin this guard sweeps, read from the compiler's registry.
///
/// Returning the const itself (rather than a copy) is the point: there is no
/// second list here to fall out of step with the first.
fn probe_universe() -> &'static [&'static str] {
    reify_compiler::GEOMETRY_FUNCTION_NAMES
}

/// Inclusive upper bound on the arity swept for each builtin.
///
/// Matches the sibling bound in `reify-compiler/src/builtin_signatures.rs:1308`,
/// which guards the arity sweeps of the arg-slot table. That one lives INSIDE
/// `mod tests` and so is test-private and not importable across the crate
/// boundary; this const is therefore a deliberate restatement, not an oversight.
/// Both leave headroom above the largest arity any builtin arm guards on today
/// (11, `linear_pattern_2d`). Raise them together.
const MAX_PROBED_ARITY: usize = 14;

/// The headroom claim above, enforced rather than asserted in prose: an
/// arity-guarded builtin arm cannot hide from the sweep by sitting just above
/// the bound.
const _: () = assert!(MAX_PROBED_ARITY > 11);

/// (e) The probe universe is READ from the compiler-side registry, never
/// restated here, and is asserted against a FLOOR rather than an equality.
///
/// An equality would turn every new geometry builtin into a failure of this
/// guard instead of a new position for it to sweep — the opposite of what a
/// closure guard is for. The floor still catches the failure mode that matters:
/// a universe that has silently collapsed to a handful of names, or to none,
/// would make every downstream assertion vacuous.
#[test]
fn probe_universe_is_the_compiler_registry_and_clears_the_floor() {
    let universe = probe_universe();

    assert!(
        !universe.is_empty(),
        "the probe universe is empty — every assertion in this file would be vacuous"
    );
    assert!(
        universe.len() >= 60,
        "probe universe shrank to {} names; the floor is 60 and the count measured \
         when this guard was written was 65. A DROP below the floor means geometry \
         builtins disappeared from `reify_compiler::GEOMETRY_FUNCTION_NAMES` and this \
         guard has stopped sweeping them. ADDING builtins must never break this guard, \
         which is why the assertion is a floor and not an equality.",
        universe.len()
    );
    assert_eq!(
        universe,
        reify_compiler::GEOMETRY_FUNCTION_NAMES,
        "the universe must be the compiler's registry itself, read at test time — \
         a literal list copied into this file would drift silently"
    );
}
