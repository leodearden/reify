//! Task #5467 (PRD2 α, step-15(b)): `detect_underdetermined` must not
//! false-positive on an INSTANCE-PATH-keyed auto.
//!
//! Registered from `harness_engine.rs` with an explicit `#[path]` — see the
//! anti-re-accretion rationale there.
//!
//! # Why this is compiled from `.ri` source and not built with a builder
//!
//! The defect lives in a mismatch between TWO ID NAMESPACES: the compiler mints
//! an auto `ValueCellDecl` keyed by INSTANCE PATH at
//! `crates/reify-compiler/src/entity.rs:3025` (construction named-arg) and
//! `:3130` (sub-override), while a constraint's read of that same cell
//! normalises to the declaring TEMPLATE's id. A `TopologyTemplateBuilder`
//! fixture would only imitate the spelling `entity.rs` happens to emit today,
//! so it would keep passing if that spelling ever changed — pinning the
//! imitation instead of the fact. Compiling real source makes the COMPILER the
//! source of truth for both ids.
//!
//! # Why the assertion is a COUNT of `Underdetermined`, not "no errors"
//!
//! `W_UNDERDETERMINED` is a WARNING. The nearest existing e2e over the same
//! binding sites (`tests/auto_binding_sites_remaining_resolution.rs`) filters
//! with `errors_only(...)`, so two brand-new user-visible warnings passed
//! through it silently — which is precisely how this regression reached a
//! branch tip with a green suite.
//!
//! # Why the count assertion is not ENOUGH on its own (review #5467-12)
//!
//! Zero `Underdetermined` diagnostics is a NECESSARY but not a SUFFICIENT
//! signal, and asserting only the count green-lights the exact defect the
//! widening can introduce: layer 4 (`detect_underdetermined`) reads the FORWARD
//! walk while layer 1 (`filter_constraints_reading_autos`) reads the REVERSE
//! one, so widening one direction alone SUPPRESSES the warning without ever
//! solving the auto — strictly worse than the un-widened `main`, which at
//! least said so loudly. `no_underdetermined_for_either_instance_path_minting_site`
//! below also cannot reach the reverse direction at all, because it pins its
//! autos with a DIRECT read.
//!
//! `let_indirected_instance_path_autos_resolve_through_their_lets` therefore
//! carries the load-bearing half: the SAME two minting sites, pinned only
//! THROUGH a `let`, asserted on their RESOLVED VALUES. The zero-warning check
//! on that fixture is kept as a separate, secondary assertion
//! (`..._emit_no_underdetermined_warning`) so a regression reports which of the
//! two halves broke.
//!
//! # Why no connect-param site
//!
//! Deliberately excluded. `AllFourSites.__connector_0.gain` carries a SEPARATE,
//! PRE-EXISTING false positive (by design D5 the parent cannot name
//! `__connector_N`, so no constraint reads it under any spelling — true on
//! `main` too), and folding it in would make this count track two unrelated
//! defects at once. It is covered, and pinned as the expected baseline, by
//! `tests/auto_binding_sites_remaining_resolution.rs`.

use reify_core::ValueCellId;
use reify_eval::{Engine, EvalResult};
use reify_test_support::{MockConstraintChecker, collect_errors, compile_source_with_stdlib};

// Shared with the sibling `let_tracing_transitive_e2e` module — same test
// binary, same three helpers (review suggestion 5). `SOLVER_TOL` deliberately
// stays local: it is derived from THIS fixture's exact-arithmetic residuals,
// below.
use crate::underdetermined_support::{
    eval_through_production_registry, scalar_si, underdetermined,
};

/// Both instance-path minting sites side by side, each pinned by a constraint
/// that reads the SAME instance-path spelling the declaration uses.
///
/// Shape lifted from `examples/auto_binding_sites.ri` sites (1) and (2), minus
/// the connect-param site (see the module header).
const BOTH_MINTING_SITES: &str = r#"
structure Bearing {
    param bore : Length = 10mm
}

structure Bolt {
    param length : Length = 5mm
}

// (1) SUB-OVERRIDE site — `entity.rs:3130` mints the decl
//     `InstancePathAutos.b.bore`.
// (2) CONSTRUCTION named-arg site — `entity.rs:3025` mints the decl
//     `InstancePathAutos.bolt.length`.
structure InstancePathAutos {
    sub b : Bearing { bore = auto }
    constraint self.b.bore == 10mm

    sub bolt = Bolt(length: auto)
    constraint self.bolt.length == 10mm
}
"#;

/// Neither instance-path auto is free: each is pinned by a constraint reading
/// the same instance-path spelling. Zero `Underdetermined` diagnostics.
///
/// RED until step-17 makes `CellReadIndex::read_closure` a genuine SUPERSET.
/// Today the closure keeps only the NORMALISED spelling (`Bearing.bore`,
/// `Bolt.length`) while `detect_underdetermined` matches the RAW declaration id
/// (`InstancePathAutos.b.bore`, `InstancePathAutos.bolt.length`), so both are
/// reported free on every `reify check`.
#[test]
fn no_underdetermined_for_either_instance_path_minting_site() {
    // `eval_through_reify_check` is `Engine::new(.., None)`, the literal
    // `reify check` entry point: `detect_underdetermined` runs OUTSIDE the
    // `has_active_solver` gate, so no amount of solver-side fixing can clear
    // what it emits.
    let result = eval_through_reify_check(BOTH_MINTING_SITES, "instance-path");

    let under = underdetermined(&result);

    assert_eq!(
        under.len(),
        0,
        "both autos here are pinned by a constraint reading the SAME \
         instance-path spelling the compiler used to declare them \
         (entity.rs:3130 sub-override, :3025 construction named-arg), so \
         NEITHER is underdetermined. Each diagnostic below is a false positive \
         caused by the read closure discarding the raw spelling; got {under:#?}",
    );
}

/// Tolerance for the resolved autos.
///
/// Each auto below is pinned by a single linear `Eq` residual with a unique
/// root, and `DimensionalSolver` accepts only a summed-squared-residual
/// `<= FEASIBILITY_THRESHOLD = 1e-12` (`crates/reify-constraints/src/solver.rs`),
/// which bounds each `|delta|` well under this. IMPLIED BY THE SOLVE SUCCEEDING,
/// not fitted to an observed run — a failure here is a convergence signal to
/// investigate, never an invitation to widen the constant.
const SOLVER_TOL: f64 = 1e-9;

/// The SAME two minting sites as [`BOTH_MINTING_SITES`], but with every auto
/// pinned ONLY THROUGH A `let`.
///
/// This is the shape that actually routes through `CellReadIndex::cells_reaching`
/// — the reverse direction of the index. The direct-read fixture above never
/// reaches it, because a constraint that names the auto itself is admitted by
/// layer 1's `auto_ids` disjunct without any reverse walk.
///
/// Each `let` has a unique root, so the assertions below are on exact values,
/// not on "some value was produced": `margin == 8mm` with `margin = bore - 2mm`
/// forces `bore = 10mm`; `slack == 9mm` with `slack = length - 1mm` forces
/// `length = 10mm`.
const BOTH_MINTING_SITES_LET_INDIRECTED: &str = r#"
structure Bearing {
    param bore : Length = 10mm
}

structure Bolt {
    param length : Length = 5mm
}

// (1) SUB-OVERRIDE site — `entity.rs:3130` mints the decl `LetIndirect.b.bore`.
// (2) CONSTRUCTION named-arg site — `entity.rs:3025` mints the decl
// `LetIndirect.bolt.length`.
structure LetIndirect {
    sub b : Bearing { bore = auto }
    let margin = self.b.bore - 2mm
    constraint margin == 8mm

    sub bolt = Bolt(length: auto)
    let slack = self.bolt.length - 1mm
    constraint slack == 9mm
}
"#;

/// THE load-bearing assertion for both instance-path minting sites: the autos
/// must RESOLVE, not merely stop warning.
///
/// RED before the reverse map is made additive (task #5467 step-19): probed
/// directly on the pre-fix branch tip, both cells came back `Value::Undef` with
/// ZERO diagnostics — silently wrong, where `main` was loudly right.
#[test]
fn let_indirected_instance_path_autos_resolve_through_their_lets() {
    let result =
        eval_through_production_registry(BOTH_MINTING_SITES_LET_INDIRECTED, "let-indirected");

    // (1) SUB-OVERRIDE site. `margin = bore - 2mm`, `margin == 8mm` => 10mm.
    let bore = scalar_si(
        &result,
        &ValueCellId::new("LetIndirect.b", "bore"),
        "let-indirected",
    );
    assert!(
        (bore - 0.010).abs() < SOLVER_TOL,
        "sub-override site (entity.rs:3130): `let margin = self.b.bore - 2mm` \
         with `constraint margin == 8mm` has the unique solution bore = 10mm; \
         got {bore} m (|delta| = {})",
        (bore - 0.010).abs(),
    );

    // (2) CONSTRUCTION named-arg site. `slack = length - 1mm`, `slack == 9mm`
    // => 10mm.
    let length = scalar_si(
        &result,
        &ValueCellId::new("LetIndirect.bolt", "length"),
        "let-indirected",
    );
    assert!(
        (length - 0.010).abs() < SOLVER_TOL,
        "construction named-arg site (entity.rs:3025): `let slack = \
         self.bolt.length - 1mm` with `constraint slack == 9mm` has the unique \
         solution length = 10mm; got {length} m (|delta| = {})",
        (length - 0.010).abs(),
    );

    // The dependent `let`s must be re-materialized to match, or the post-solve
    // write-back list (`build_dependent_cells` stage (d), which consumes the
    // same reverse walk) dropped them while the solve itself succeeded.
    let margin = scalar_si(
        &result,
        &ValueCellId::new("LetIndirect", "margin"),
        "let-indirected",
    );
    assert!(
        (margin - 0.008).abs() < SOLVER_TOL,
        "the dependent `let` must be re-materialized from the solved auto; \
         expected 8mm, got {margin} m",
    );
    let slack = scalar_si(
        &result,
        &ValueCellId::new("LetIndirect", "slack"),
        "let-indirected",
    );
    assert!(
        (slack - 0.009).abs() < SOLVER_TOL,
        "the dependent `let` must be re-materialized from the solved auto; \
         expected 9mm, got {slack} m",
    );
}

/// The SECOND, separate half of the let-indirected signal — kept apart from the
/// value assertions on purpose (see the module header): a fix that resolved the
/// autos while still printing `W_UNDERDETERMINED`, or that silenced the warning
/// without solving, must fail exactly one of the two and name which.
#[test]
fn let_indirected_instance_path_autos_emit_no_underdetermined_warning() {
    let result =
        eval_through_production_registry(BOTH_MINTING_SITES_LET_INDIRECTED, "let-indirected");

    let flagged = underdetermined(&result);
    assert!(
        flagged.is_empty(),
        "both autos are pinned through their `let`s, so neither may be \
         reported underdetermined; got {flagged:#?}",
    );
}

// ---------------------------------------------------------------------------
// The CHILD-SIDE-`let` shape (task #5467 amendment round 2, review finding 1).
//
// Everything above pins the shape where the reading `let` lives in the PARENT
// (`LetIndirect.margin` reads `self.b.bore`), so the RAW instance-path spelling
// `LetIndirect.b.bore` enters `read_closure` directly from the seed sweep and
// layer 4 matches the declaration id without any further bridging.
//
// The shape below is the other one, and it is NOT covered by those: the reading
// `let` lives in the DECLARING CHILD (`Bearing.fit` reads `Bearing.bore`). That
// dep is ALREADY canonical, so `normalize_ref` answers `None` and the walk adds
// only `Bearing.bore` — never `Holder.b.bore`, which is what the compiler minted
// for `sub b : Bearing { bore = auto }` and what `detect_underdetermined`
// matches. Layer 1's ADDITIVE SEEDING (the round-1 amendment) already repaired
// the SOLVE for this shape; layer 4 was left behind, so the two disagree and the
// warning is now a NEW false positive that `main` did not have — on `main` the
// constraint was dropped and the warning was true.
// ---------------------------------------------------------------------------

/// Compile + eval `src` through the literal `reify check` entry point —
/// `Engine::new(.., None)`, no solver attached.
///
/// `detect_underdetermined` runs OUTSIDE the `has_active_solver` gate, so this
/// is the only path that observes what a `reify check` user actually sees; no
/// amount of solver-side fixing can clear what it emits. Factored out because
/// the count assertions below all need the same three lines AND the same
/// compile-error guard: each pins a DIAGNOSTIC COUNT, so a compile error would
/// silently change what is being counted rather than fail.
fn eval_through_reify_check(src: &str, what: &str) -> EvalResult {
    let compiled = compile_source_with_stdlib(src);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "the {what} fixture must compile without errors — this test pins a \
         DIAGNOSTIC COUNT, so a compile error would silently change what is \
         being counted; got {errors:#?}",
    );

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    engine.eval(&compiled)
}

/// The auto is minted by the sub-override site (`entity.rs:3130`) as
/// `Holder.b.bore`, and the only thing that reads it is a `let` in the
/// DECLARING CHILD — so the read is spelled `Bearing.bore` at its source.
///
/// `fit = bore * 2` with `fit == 20mm` has the unique solution `bore = 10mm`.
const CHILD_SIDE_LET: &str = r#"
structure Bearing {
    param bore : Length = 10mm
    let fit = self.bore * 2.0
}

structure Holder {
    sub b : Bearing { bore = auto }
    constraint self.b.fit == 20mm
}
"#;

/// The NON-REGRESSION half, and what makes the warning below provably a FALSE
/// positive rather than a true one: the auto genuinely resolves.
///
/// Layer 1's additive seeding in `CellReadIndex::cells_reaching` is what makes
/// this pass — it seeds the reverse walk with BOTH `Holder.b.bore` and its
/// normalisation `Bearing.bore`, so `Bearing.fit` is found as a reader and the
/// pinning constraint is admitted. Kept SEPARATE from the warning assertion so
/// a fix that silences the warning by breaking the solve names which half broke.
#[test]
fn child_side_let_instance_path_auto_resolves_through_the_childs_let() {
    let result = eval_through_production_registry(CHILD_SIDE_LET, "child-side-let");

    let bore = scalar_si(
        &result,
        &ValueCellId::new("Holder.b", "bore"),
        "child-side-let",
    );
    assert!(
        (bore - 0.010).abs() < SOLVER_TOL,
        "`let fit = self.bore * 2.0` inside the DECLARING CHILD, with \
         `constraint self.b.fit == 20mm` in the parent, has the unique solution \
         bore = 10mm; got {bore} m (|delta| = {})",
        (bore - 0.010).abs(),
    );
}

/// The other half: `Holder.b.bore` is pinned, so `reify check` must not report
/// it free.
///
/// RED before step-30. `read_closure` only ever produces the CANONICAL spelling
/// of the child's dep (`Bearing.bore`) — `Bearing.fit`'s dep is already
/// canonical, so the walk's additive hop adds nothing — while
/// `detect_underdetermined` matches the RAW declaration id `Holder.b.bore`.
#[test]
fn child_side_let_instance_path_auto_emits_no_underdetermined_warning() {
    let result = eval_through_reify_check(CHILD_SIDE_LET, "child-side-let");

    let flagged = underdetermined(&result);
    assert!(
        flagged.is_empty(),
        "`Holder.b.bore` IS pinned — `child_side_let_instance_path_auto_resolves_\
         through_the_childs_let` shows it resolving to 10mm through the child's \
         `let`. A diagnostic here is therefore a NEW false positive this branch \
         introduced: on `main` layer 1 dropped the pinning constraint and the \
         warning was TRUE, so layers 1 and 4 now disagree. Got {flagged:#?}",
    );
}

/// THE FIX-SHAPE DISCRIMINATOR (task #5467 amendment round 2).
///
/// Two sibling instances share the child-side `let`; only `b1` is pinned. The
/// three verdicts a candidate fix can produce are all reachable, and only one is
/// correct — see the assertion message.
const TWO_SIBLINGS_SHARING_A_CHILD_SIDE_LET: &str = r#"
structure Bearing {
    param bore : Length = 10mm
    let fit = self.bore * 2.0
}

structure Sib {
    sub b1 : Bearing { bore = auto }
    sub b2 : Bearing { bore = auto }
    constraint self.b1.fit == 20mm
}
"#;

/// BOTH siblings are flagged, and the diagnostics agree with the VALUES.
///
/// `Bearing` is reachable by TWO instance paths here, so
/// `build_single_instance_alias_paths` drops it and `build_dependent_cells`
/// stage (g) emits NO alias entry for `Sib.b1.fit` — layer 1 therefore cannot
/// drive `Sib.b1.bore` through the child-side `let`, and neither auto is solved.
/// `Sib.b1.bore` is nonetheless *semantically* pinned by
/// `constraint self.b1.fit == 20mm`, so layer 4's re-projection disjunct WOULD
/// unflag it — which is why that disjunct is gated on the same alias oracle
/// stage (g) uses (review esc-5467-16 finding 2).
///
/// The three verdicts a candidate fix can produce are all reachable, and only
/// one is correct:
///
/// * TWO warnings, both autos `Undef` — CORRECT and self-consistent. It is what
///   `main` reported, and it is honest: nothing here is solved, and the user is
///   told so for both. The v1 boundary that makes it true is documented on
///   `build_single_instance_alias_paths`.
/// * ONE warning (`Sib.b2.bore`), both autos `Undef` — the esc-5467-16 finding-2
///   regression: `Sib.b1.bore` is silently `Undef` with NO diagnostic at all.
///   Strictly worse than `main`.
/// * ZERO warnings, both autos `Undef` — the reverse probe spelled as a plain
///   `global_reads.contains(reader)`, letting b1's pin mask the genuinely free
///   b2 through `cells_reaching`'s many-to-one normalised seeding.
///
/// The VALUE assertions are the load-bearing half and are not optional: a count
/// alone cannot tell verdict 1 from verdict 2 turning into "one warning, b1
/// solved", which WOULD be an improvement. Asserting `Undef` is what pins the
/// warning to the solve.
#[test]
fn two_sibling_instances_sharing_a_child_side_let_are_flagged_independently() {
    let result = eval_through_production_registry(
        TWO_SIBLINGS_SHARING_A_CHILD_SIDE_LET,
        "two-sibling child-side-let",
    );

    let flagged: Vec<&str> = underdetermined(&result)
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    for auto in ["Sib.b1.bore", "Sib.b2.bore"] {
        assert!(
            flagged.iter().any(|m| m.contains(auto)),
            "`{auto}` is NOT solved here (asserted below), so it MUST be \
             flagged. A missing diagnostic for `Sib.b1.bore` is the \
             esc-5467-16 finding-2 regression: layer 4's re-projection \
             unflagged it on an instance path stage (g) never aliased, so the \
             loud correct warning became a silent `Undef`. Got {flagged:#?}",
        );
    }
    assert_eq!(
        flagged.len(),
        2,
        "exactly the two autos, once each. Got {flagged:#?}",
    );

    // The half a count cannot express: the warnings are TRUE.
    for (entity, member) in [
        ("Sib.b1", "bore"),
        ("Sib.b2", "bore"),
        ("Sib.b1", "fit"),
    ] {
        let got = result.values.get(&ValueCellId::new(entity, member));
        assert!(
            matches!(got, None | Some(reify_ir::Value::Undef)),
            "`{entity}.{member}` is expected to stay unresolved at this \
             fixture's documented v1 boundary (`Bearing` has two instance \
             paths, so stage (g) emits no alias). If it now RESOLVES, that is \
             good news and the alias emitter was widened — drop the matching \
             W_UNDERDETERMINED expectation above in the same change, and never \
             the other way round. Got {got:?}",
        );
    }
}

/// The shape a pure instance re-projection provably CANNOT reach: the pinning
/// constraint lives INSIDE the declaring child, so the reader `Bearing2.fit` is
/// template-keyed at its source and `Own.b.fit` appears nowhere in the closure.
///
/// It is ALSO the shape layer 1 does not solve — see the test below — so layer 4
/// must keep WARNING about it, not quietly unflag it.
const CONSTRAINT_INSIDE_THE_DECLARING_CHILD: &str = r#"
structure Bearing2 {
    param bore : Length = 10mm
    let fit = self.bore * 2.0
    constraint self.fit == 20mm
}

structure Own {
    sub b : Bearing2 { bore = auto }
}
"#;

/// A constraint written inside the declaring child does NOT reach the
/// instance-path-keyed auto `Own.b.bore` through any layer this task builds, so
/// the warning must survive — matching `main` (review esc-5467-18).
///
/// # Why this is an expectation and not a bug left in place
///
/// An earlier round of this branch unflagged this shape via a TEMPLATE-LOCAL
/// PROVENANCE disjunct in `auto_is_pinned_through_a_reader`, arguing that a read
/// already template-keyed at its source pins every instance of `Bearing2`. That
/// is a true statement about the CONSTRAINT and a false one about the SOLVE:
/// `filter_constraints_reading_autos` (layer 1) only filters the constraints of
/// the scope that OWNS the auto — `Own` — so `Bearing2`'s constraint never enters
/// `Own`'s `ResolutionProblem`, while `Bearing2`'s own scope holds no autos and
/// yields no problem at all. Nothing solves `Own.b.bore`; the disjunct only
/// stopped layer 4 from saying so.
///
/// Widening layer 1 to admit a child's own constraints and re-project them onto
/// the alias path is a real feature and is OUT OF SCOPE for task #5467. Until it
/// lands, the honest verdict is `main`'s: one loud, TRUE `W_UNDERDETERMINED`.
///
/// # Why the VALUE half is not optional
///
/// A count alone cannot distinguish "still unsolved and correctly warned" from
/// "solved, and the warning is now a false positive" — the second would be GOOD
/// news and would demand the opposite edit. Asserting `Undef` through the real
/// `SolverRegistry::production()` is what makes the two halves unable to disagree
/// silently, exactly as
/// `two_sibling_instances_sharing_a_child_side_let_are_flagged_independently`
/// does one seam over.
#[test]
fn an_auto_pinned_only_by_a_constraint_inside_its_declaring_child_is_flagged() {
    let result = eval_through_reify_check(
        CONSTRAINT_INSIDE_THE_DECLARING_CHILD,
        "child-internal constraint",
    );

    let flagged = underdetermined(&result);
    assert_eq!(
        flagged.len(),
        1,
        "`Own.b.bore` is minted by the sub-override and is solved by NO layer \
         (asserted below), so exactly one W_UNDERDETERMINED must name it. ZERO \
         here is the esc-5467-18 regression: layer 4 unflagged an auto layer 1 \
         never drives, turning `main`'s loud correct warning into a silent \
         `Undef`. Got {flagged:#?}",
    );
    assert!(
        flagged[0].message.contains("Own.b.bore"),
        "the diagnostic must name the instance-path-keyed auto `Own.b.bore`; \
         got: {}",
        flagged[0].message,
    );

    // The half a count cannot express: the warning is TRUE.
    let solved = eval_through_production_registry(
        CONSTRAINT_INSIDE_THE_DECLARING_CHILD,
        "child-internal constraint",
    );
    for member in ["bore", "fit"] {
        let got = solved.values.get(&ValueCellId::new("Own.b", member));
        assert!(
            matches!(got, None | Some(reify_ir::Value::Undef)),
            "`Own.b.{member}` is expected to stay unresolved: layer 1 admits \
             only `Own`'s OWN constraints, and `Own` has none. If it now \
             RESOLVES, layer 1 was widened to re-project child-side constraints \
             — that is good news; drop the W_UNDERDETERMINED expectation above \
             in the SAME change, and never the other way round. Got {got:?}",
        );
    }
}

/// The counterexample that forces the DECLARING-TEMPLATE guard on the
/// template-local-provenance disjunct: the reading `let` lives in the shared
/// CONTAINER, not in the declaring child, and reads only ONE of two siblings.
const CONTAINER_LOCAL_LET_READING_ONE_SIBLING: &str = r#"
structure Bearing {
    param bore : Length = 10mm
}

structure Cont {
    sub b1 : Bearing { bore = auto }
    sub b2 : Bearing { bore = auto }
    let fit = self.b1.bore * 2.0
    constraint self.fit == 20mm
}
"#;

/// `Cont.fit` is template-local to `Cont` AND is surfaced as a reader of BOTH
/// siblings — b2's probe reaches it through the shared normalised `Bearing.bore`
/// key — so an unguarded template-local-provenance disjunct masks the genuinely
/// free `Cont.b2.bore`.
///
/// MEASURED: with the `reader.entity == declaring` guard removed from
/// `auto_is_pinned_through_a_reader`, this fixture reports ZERO diagnostics.
/// The guard restricts that disjunct to readers living in the auto's own
/// DECLARING template (`Bearing` here, which owns no constraint at all), which
/// is the only population for which "already template-keyed at its source"
/// actually implies "pins every instance of the thing this auto belongs to".
#[test]
fn a_container_local_let_reading_one_sibling_does_not_mask_the_other() {
    let result = eval_through_reify_check(
        CONTAINER_LOCAL_LET_READING_ONE_SIBLING,
        "container-local let",
    );

    let flagged = underdetermined(&result);
    assert_eq!(
        flagged.len(),
        1,
        "only `Cont.b2.bore` is free: `Cont.b1.bore` is pinned through \
         `let fit = self.b1.bore * 2.0`, and nothing reads b2 at all. ZERO here \
         means the template-local-provenance disjunct fired on `Cont.fit`, a \
         reader that belongs to the CONTAINER rather than to the declaring \
         `Bearing` — b1's pin masking b2, the same false negative the \
         re-projection disjunct is shaped to avoid. Got {flagged:#?}",
    );
    assert!(
        flagged[0].message.contains("Cont.b2.bore"),
        "the surviving diagnostic must name the unread sibling `Cont.b2.bore`; \
         got: {}",
        flagged[0].message,
    );
}

/// The member-name COLLISION counterexample that forces the declaring-template
/// guard onto the RE-PROJECTION disjunct as well (review esc-5467-16 finding 1).
///
/// `Bearing.bore` is genuinely free: `Bearing.fit` is a literal `10mm` and
/// `constraint self.fit == 10mm` is a tautology that never touches `bore`, while
/// `Sib.fit` is an unconstrained `let`. The trap is that `Sib.fit` is REACHED as
/// a reader of `Bearing.bore` (`self.b1.bore` normalises to the `Bearing.bore`
/// reverse key), and an unguarded re-projection asks only
/// `global_reads.contains(Bearing.<reader.member>)` = `Bearing.fit` — which
/// `Bearing`'s own UNRELATED constraint put there. Nothing ties that id back to
/// `Sib.fit`; only the member NAME is shared.
const COLLIDING_MEMBER_NAME_IN_ANOTHER_ENTITY: &str = r#"
structure Bearing {
    param bore : Length = auto
    let fit : Length = 10mm
    constraint self.fit == 10mm
}

structure Sib {
    sub b1 : Bearing
    let fit : Length = self.b1.bore * 2.0
}
"#;

/// The SAME model with the colliding reader renamed `fit` -> `margin` and
/// nothing else changed. The verdict must not depend on that name.
const NON_COLLIDING_MEMBER_NAME_IN_ANOTHER_ENTITY: &str = r#"
structure Bearing {
    param bore : Length = auto
    let fit : Length = 10mm
    constraint self.fit == 10mm
}

structure Sib {
    sub b1 : Bearing
    let margin : Length = self.b1.bore * 2.0
}
"#;

/// A bare member-name collision in an UNRELATED entity must not unflag a
/// genuinely free auto.
///
/// MEASURED before the guard moved onto the re-projection disjunct: the
/// colliding spelling reported ZERO diagnostics while the renamed one reported
/// the correct single `Bearing.bore` diagnostic — a LOST WARNING relative to
/// `main`, whose one-hop read-set is `{Bearing.fit}` and which warns in both
/// spellings. The two fixtures are asserted TOGETHER, and the renamed one is the
/// control: asserting the colliding one alone could be satisfied by a fix that
/// simply stopped unflagging everything.
#[test]
fn a_colliding_member_name_in_another_entity_does_not_mask_a_free_auto() {
    for (src, what) in [
        (COLLIDING_MEMBER_NAME_IN_ANOTHER_ENTITY, "colliding `Sib.fit`"),
        (
            NON_COLLIDING_MEMBER_NAME_IN_ANOTHER_ENTITY,
            "renamed `Sib.margin`",
        ),
    ] {
        let result = eval_through_reify_check(src, what);
        let flagged = underdetermined(&result);
        assert_eq!(
            flagged.len(),
            1,
            "`Bearing.bore` is free in BOTH spellings — `Bearing.fit` is a \
             literal and its constraint is a tautology, and `Sib.fit`/`Sib.margin` \
             is an unconstrained `let`. ZERO for the {what} fixture means the \
             re-projection disjunct matched on the reader's MEMBER NAME alone \
             and honoured a reader that does not live in the declaring \
             `Bearing`. Got {flagged:#?}",
        );
        assert!(
            flagged[0].message.contains("Bearing.bore"),
            "the {what} diagnostic must name `Bearing.bore`; got: {}",
            flagged[0].message,
        );
    }
}

