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
//! # C7 drift-guard registrations — answered here, not deferred
//!
//! **`tests/infra/harness-layout-baseline.manifest`: nothing owed, because this
//! file is not standalone.** A NEW top-level `crates/reify-eval/tests/*.rs`
//! binary is a C1 layout violation, not a baseline candidate: the manifest is a
//! shrinking ratchet, so `scripts/check-harness-baseline-registration.sh` routes
//! a new test into the consolidated harness for its subsystem instead. This
//! guard therefore lives at `tests/harness_geometry/units_length_closure_guard.rs`,
//! declared from `harness_geometry.rs` with the mandatory `#[path]`, alongside
//! the Contract C length-units siblings (`geometry_length_args_units_e2e`,
//! `primitive_profile_length_units_e2e`, `modify_sweep_length_units_e2e`,
//! `transform_translation_length_units_e2e`) it generalizes. Consequences to
//! know: its tests are named `units_length_closure_guard::<test>` in the
//! `reify-eval::harness_geometry` binary, not bare in a binary of their own, and
//! the whole `harness_geometry` compile unit measures ~11.8 kLOC against
//! `test_harness_kloc_cap.sh`'s `CAP_LINES=20000` — well under the 90% advisory
//! warn line, so no `_KLOC_WARN_KNOWN` row is owed either.
//!
//! **`.config/nextest.toml`: no override, deliberately.** That file is read by
//! `cargo nextest`, so the measurement that decides the question is the nextest
//! one: on this tree the slowest test of this module measured 5.0s, 7.1s and
//! 8.4s across three runs — the sweep is IR-build-only and never constructs a
//! kernel — against `[profile.default]`'s `120s x 10` = 1200s ceiling. That is
//! over two orders of magnitude of headroom, so the run-to-run variance that
//! makes the figure a range rather than a number cannot threaten the
//! conclusion. (Plain `cargo test` reports 5.3s for these tests as a group, at
//! the bottom of that range, because it runs them as threads of ONE process
//! so they share the sweep cache, whereas nextest gives each test its own
//! process and every Step-3 test pays the sweep itself. Quoting nextest is
//! what keeps the basis matched to the runner the config governs.) Those
//! figures were measured before the C1 move, on the tests themselves rather
//! than on the enclosing binary, which is the quantity a per-test nextest
//! `slow-timeout` governs either way. `harness_geometry` carries no override
//! block today, and adding one would be dead config AND would owe a paired row
//! in `GATE_RESIDENT_FILTERS` (`tests/infra/test_nextest_slow_priority.sh`),
//! whose Assertion K reds on an override classifying as neither heavy nor
//! gate-resident. A block that does not exist cannot red.
//!
//! **`tests/infra/run-all-classification.manifest`: nothing owed.** No
//! `tests/infra/test_*.sh` is added — this is a Rust integration test, reached
//! by `cargo test -p reify-eval` (the `test-instrumentation` self-dev-dep
//! activates the probe seam, so no `--features` flag is needed).
//!
//! **`tests/infra/test_no_new_wallclock_upper_bounds.sh`: nothing owed.** This
//! file asserts no elapsed-time bound at all, which is what C7 prefers and what
//! `version_id_discipline_gate.rs` set the precedent for. The counts it DOES
//! bound — observations, rejections, universe size — are FLOORS on evidence,
//! not deadlines, so they cannot flake with machine load.
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

// -- Step-2: the registry and the classifier --

/// One numeric argument position of one builtin at one arity.
///
/// This tuple is the ONLY suppression key in this file. Nothing is waved
/// through by builtin name alone, because a builtin's gated and un-gated slots
/// routinely sit next to each other — `half_space` gates its `px`/`py`/`pz`
/// POINT while its `nx`/`ny`/`nz` NORMAL stays bare, and `mirror` draws the same
/// origin-vs-direction split.
///
/// `arity` is part of the key because Contract C's gated spans are
/// arity-dependent: `nurbs` gates `2 .. 2 + 3·n_points`, a range computed from
/// an argument, and `circular_pattern`'s short form has no origin triple to gate
/// at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Position {
    builtin: &'static str,
    /// Count of NUMERIC arguments supplied after any geometry target operand.
    arity: usize,
    /// 0-based index into those numeric arguments.
    index: usize,
}

/// Why a dimensionless position is legitimately dimensionless — decision D14's
/// CLOSED taxonomy.
///
/// Closed on purpose. A free-text rationale would let the next un-gated position
/// be waved through with improvised prose; a variant list forces a new reading
/// to be argued once, here, where every existing entry is visible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Justification {
    /// A component of a DIRECTION vector. Normalised by the consumer, so the
    /// vector's scale is meaningless and a bare component is correct `.ri`.
    UnitVectorComponent,
    /// A cardinality — how many instances, points or spans.
    Count,
    /// A rational blending weight.
    Weight,
    /// A knot: a parameter-space coordinate, not a coordinate in metres.
    Knot,
    /// A polynomial degree.
    Degree,
    /// A dimensionless scale factor.
    Factor,
    /// An index or handle selecting an entity — a face, an edge, a step, a
    /// datum, a transform — rather than measuring a quantity.
    Index,
    /// A mode or flag selecting BEHAVIOUR: it neither measures a quantity nor
    /// names an entity.
    ///
    /// The ONE reading this guard's sweep added to D14's original seven, and it
    /// was added because two measured positions fit none of them —
    /// `isosurface`'s boolean `adaptive` and `extrude_infinite`'s `direction`
    /// side selector. Recorded here rather than bent into `Index` so the
    /// extension is visible: a variant is the unit in which a new reading gets
    /// argued.
    ModeFlag,
}

impl std::fmt::Display for Justification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let reading = match self {
            Justification::UnitVectorComponent => "a unit-vector component",
            Justification::Count => "a count",
            Justification::Weight => "a blending weight",
            Justification::Knot => "a parameter-space knot",
            Justification::Degree => "a polynomial degree",
            Justification::Factor => "a dimensionless factor",
            Justification::Index => "an entity selector",
            Justification::ModeFlag => "a behaviour mode flag",
        };
        f.write_str(reading)
    }
}

/// A position that is dimensionless BY DESIGN, with the dimension it expects and
/// the D14 reading that licenses it.
///
/// `expected` is carried even though every shipped row is dimensionless today:
/// it is what lets PRD 3 add ANGLE rows without touching the classifier.
#[derive(Clone, Copy, Debug)]
struct AllowEntry {
    position: Position,
    expected: reify_core::DimensionVector,
    justification: Justification,
}

impl std::fmt::Display for AllowEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}/{}[{}] expects {:?} because it is {}",
            self.position.builtin,
            self.position.arity,
            self.position.index,
            self.expected,
            self.justification
        )
    }
}

/// What is wrong with one observed position.
///
/// A structured reason rather than a message string: the renderer below is the
/// single place that turns it into prose, so no consumer has to parse one back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ViolationReason {
    /// The op compiled and the bare value was taken — the gate is absent.
    BareValueAccepted,
    /// The op did not compile, but no Contract C rejection was raised, so
    /// whatever stopped it was NOT the dimension gate.
    FailedWithoutDimensionRejection,
}

impl std::fmt::Display for ViolationReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ViolationReason::BareValueAccepted => "a bare number was ACCEPTED",
            ViolationReason::FailedWithoutDimensionRejection => {
                "op compile failed WITHOUT a Contract C rejection"
            }
        };
        f.write_str(s)
    }
}

/// One un-gated, unjustified position.
#[derive(Clone, Copy, Debug)]
struct Violation {
    position: Position,
    reason: ViolationReason,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}/{}[{}]: {}",
            self.position.builtin, self.position.arity, self.position.index, self.reason
        )
    }
}

/// What driving one position through the eval-side op compiler produced.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ProbeOutcome {
    /// A `DiagnosticCode::DimensionedArgRejected` diagnostic was raised — the
    /// Contract C gate fired. This is the only outcome that needs no entry.
    ContractCRejected,
    /// The op compiled and kept the bare value.
    Accepted,
    /// The op did NOT compile, but no dimension rejection was raised — so
    /// whatever stopped it was something other than the gate.
    ///
    /// Deliberately DISTINCT from [`ProbeOutcome::ContractCRejected`]. Folding
    /// the two together is the exact false-green this guard exists to prevent:
    /// an op that fails for an unrelated reason would be read as a gate firing,
    /// and a position whose gate had been deleted would stay green for as long
    /// as anything else about the call was broken.
    OpCompileFailed(String),
    /// The position was not consumed — no op compiled for this call, or the
    /// value never reached the op's argument list. Raises no violation; the
    /// residual rows are what bound this silence.
    NotReached,
}

/// Read the probe's result and diagnostics into an outcome.
///
/// A Contract C rejection is identified by
/// `reify_core::DiagnosticCode::DimensionedArgRejected` — the code task 5743
/// attached to every `ArgSpec`-backed rejection in `geometry_ops`, asserted some
/// twenty times across `crates/reify-eval/src/geometry_ops/tests.rs`. The code
/// is the structured contract; matching on message text instead would be the
/// ad-hoc parser this codebase bans, and would break the moment the shared
/// `ArgRejection::message` template is reworded.
fn outcome_from_probe(
    result: &Result<reify_ir::GeometryOp, String>,
    diagnostics: &[reify_core::Diagnostic],
    position: &Position,
) -> ProbeOutcome {
    let rejected = diagnostics
        .iter()
        .any(|d| d.code == Some(reify_core::DiagnosticCode::DimensionedArgRejected));
    if rejected {
        return ProbeOutcome::ContractCRejected;
    }
    match result {
        Ok(_) => ProbeOutcome::Accepted,
        Err(message) => ProbeOutcome::OpCompileFailed(format!(
            "{}/{}[{}]: {message}",
            position.builtin, position.arity, position.index
        )),
    }
}

/// One position, and what the probe saw at it.
#[derive(Clone, Debug)]
struct Observation {
    position: Position,
    outcome: ProbeOutcome,
}

/// The allowlist, keyed by [`Position`].
///
/// A VALUE passed to [`classify_all`], never a global. That is what makes both
/// the seeded self-tests below and the shrink-the-shipped-allowlist anti-vacuity
/// test expressible without mutating shipped state.
#[derive(Clone, Debug, Default)]
struct Registry {
    allowed: std::collections::BTreeMap<Position, AllowEntry>,
    residuals: Vec<Residual>,
}

impl Registry {
    fn from_allow(entries: impl IntoIterator<Item = AllowEntry>) -> Self {
        Registry::from_parts(entries, [])
    }

    fn from_parts(
        entries: impl IntoIterator<Item = AllowEntry>,
        residuals: impl IntoIterator<Item = Residual>,
    ) -> Self {
        Registry {
            allowed: entries.into_iter().map(|e| (e.position, e)).collect(),
            residuals: residuals.into_iter().collect(),
        }
    }

    fn allow(&self, position: &Position) -> Option<&AllowEntry> {
        self.allowed.get(position)
    }

    /// Whether some residual row OWNS `position`.
    ///
    /// Matching is on the structured [`ResidualSubject`] alone — a row's `note`
    /// is never searched, so a position cannot become "covered" by being named
    /// in someone else's prose.
    fn residual(&self, position: &Position) -> Option<&Residual> {
        self.residuals.iter().find(|r| match r.subject {
            ResidualSubject::Position(p) => p == *position,
            ResidualSubject::Builtin(name) => name == position.builtin,
            ResidualSubject::OutOfUniverse(_) => false,
        })
    }

    /// Whether `position` is accounted for and therefore raises no violation.
    fn covers(&self, position: &Position) -> bool {
        self.allow(position).is_some() || self.residual(position).is_some()
    }
}

/// Every observation that is neither Contract-C-rejected nor accounted for by
/// `registry`, in observation order.
///
/// Pure: no I/O, no globals, no interior mutation. Both inputs are borrowed
/// values, which is what lets the anti-vacuity tests re-run it against a
/// deliberately weakened registry or a deliberately stubbed observation.
fn classify_all(observations: &[Observation], registry: &Registry) -> Vec<Violation> {
    observations
        .iter()
        .filter_map(|obs| {
            let reason = match obs.outcome {
                ProbeOutcome::ContractCRejected | ProbeOutcome::NotReached => return None,
                ProbeOutcome::Accepted => ViolationReason::BareValueAccepted,
                ProbeOutcome::OpCompileFailed(_) => {
                    ViolationReason::FailedWithoutDimensionRejection
                }
            };
            (!registry.covers(&obs.position)).then_some(Violation {
                position: obs.position,
                reason,
            })
        })
        .collect()
}

#[cfg(test)]
mod seeded_classifier {
    use super::*;

    const SEED: Position = Position {
        builtin: "mirror",
        arity: 6,
        index: 3,
    };

    fn accepted_at(position: Position) -> Observation {
        Observation {
            position,
            outcome: ProbeOutcome::Accepted,
        }
    }

    fn allow(position: Position) -> AllowEntry {
        AllowEntry {
            position,
            expected: reify_core::DimensionVector::DIMENSIONLESS,
            justification: Justification::UnitVectorComponent,
        }
    }

    /// (a) With an EMPTY registry an accepted position produces exactly one
    /// violation, and that violation names the builtin, the arity and the index.
    ///
    /// This is the guard FIRING. Every green result the Step-3 tests report is
    /// only meaningful because this path exists.
    #[test]
    fn an_accepted_position_with_an_empty_registry_yields_one_named_violation() {
        let violations = classify_all(&[accepted_at(SEED)], &Registry::default());

        assert_eq!(violations.len(), 1, "expected exactly one violation");
        let rendered = violations[0].to_string();
        assert!(
            rendered.contains("mirror") && rendered.contains('6') && rendered.contains('3'),
            "violation must name builtin, arity and index; got {rendered:?}"
        );
    }

    /// (b) An entry for EXACTLY the observed position suppresses it, and
    /// removing that entry restores the violation.
    ///
    /// The shrink-by-one shape at seed scale: suppression is not a one-way
    /// switch that could be stuck on.
    #[test]
    fn an_entry_suppresses_its_position_and_removing_it_restores_the_violation() {
        let observations = [accepted_at(SEED)];

        let with = Registry::from_allow([allow(SEED)]);
        assert!(
            classify_all(&observations, &with).is_empty(),
            "an entry for the observed position must suppress it"
        );

        let without = Registry::default();
        assert_eq!(
            classify_all(&observations, &without).len(),
            1,
            "removing the only entry must restore the violation"
        );
    }

    /// (c) An entry for a NEIGHBOURING index of the SAME builtin does not
    /// suppress.
    ///
    /// Pins that suppression is keyed by the whole `(builtin, arity, index)`
    /// tuple. A name-keyed allowlist would wave through `mirror`'s gated
    /// `ox`/`oy`/`oz` origin along with its bare `nx`/`ny`/`nz` normal.
    #[test]
    fn an_entry_for_a_neighbouring_index_does_not_suppress() {
        let neighbour = Position {
            index: SEED.index + 1,
            ..SEED
        };
        let registry = Registry::from_allow([allow(neighbour)]);

        let violations = classify_all(&[accepted_at(SEED)], &registry);

        assert_eq!(
            violations.len(),
            1,
            "an entry for {neighbour:?} must not suppress {SEED:?}"
        );
        assert_eq!(violations[0].position, SEED);
    }
}

// -- Step-2b: residual cites --

/// A reference to a Taskmaster task, in the ONE spelling the PTODO detector
/// accepts.
///
/// The canonical form (`docs/prds/reify-audit-ptodo-detector.md` §8) is `#NNNN`.
/// Holding the number and rendering it is what makes the banned spellings —
/// `task ε`, `task-5`, `task 4553` — UNREPRESENTABLE here rather than merely
/// rejected at runtime: there is no way to construct a `TaskCite` that renders
/// as any of them.
///
/// Liveness is the half this guard cannot check: a test cannot query
/// Taskmaster. It is delegated deliberately. Each shipped [`Residual`] also
/// carries a marker comment on its entry in the canonical PTODO form, so the
/// PTODO detector — which DOES resolve task state — performs the liveness
/// check, and a residual whose
/// owner closes orphans its cite and reds the fingerprint ratchet in
/// `tests/infra/test_reify_audit_ptodo.sh`. That is INV-SF-5
/// (`placeholders-owned-and-loud`) working, not a defect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct TaskCite(u32);

impl std::fmt::Display for TaskCite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// What a residual is ABOUT, distinguished structurally rather than by prose.
///
/// The three shapes are genuinely different kinds of gap and a reader must not
/// have to infer which one a note means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResidualSubject {
    /// One numeric position the sweep reaches but cannot read cleanly.
    Position(Position),
    /// A builtin of the universe for which the probe synthesizes no compiling
    /// call at any arity, so none of its positions are swept.
    Builtin(&'static str),
    /// A length-semantic site OUTSIDE this guard's universe entirely. Recorded
    /// so nobody reads a green sweep as covering it.
    OutOfUniverse(&'static str),
}

/// A gap that is OWNED rather than justified.
///
/// An [`AllowEntry`] says "this position is dimensionless and here is why". A
/// `Residual` says "this is not settled, and here is who settles it".
#[derive(Clone, Copy, Debug)]
struct Residual {
    subject: ResidualSubject,
    cite: TaskCite,
    note: &'static str,
}

impl std::fmt::Display for Residual {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let subject = match self.subject {
            ResidualSubject::Position(p) => format!("{}/{}[{}]", p.builtin, p.arity, p.index),
            ResidualSubject::Builtin(name) => format!("{name} (whole builtin)"),
            ResidualSubject::OutOfUniverse(what) => format!("{what} (outside the universe)"),
        };
        write!(f, "{subject} — {} [{}]", self.note, self.cite)
    }
}

/// This file, read from disk, so the shipped residual cites can be cross-checked
/// against the owner-cite marker comments that make them visible to the PTODO
/// detector.
///
/// Located from `CARGO_MANIFEST_DIR` rather than from the process's working
/// directory, mirroring `version_id_discipline_gate.rs`.
fn this_file() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/harness_geometry/units_length_closure_guard.rs");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

// -- Step-2c: the shipped residual rows --

/// Builtins of the universe for which the probe synthesizes no compiling call at
/// any arity or target template, so NONE of their positions are swept.
///
/// Measured, not assumed: each of these yields no `CompiledGeometryOp` from
/// `<name>(<scalar literals>)` under any of the three target templates. Their
/// arguments are geometry operands, not quantities — the booleans take two
/// solids, `sweep`/`sweep_guided` a profile and a path — or they desugar into a
/// shape the probe's last-op attribution cannot read. `arc` is here for a
/// different reason: its baseline keeps one dimension rejection under every
/// filler combination, because its angle slots are not satisfiable until
/// #5783's ANGLE rows land.
// TODO(#7714): deepen the probe's argument synthesis (geometry operands, Int
// counts, coordinate lists, grids) so these builtins are swept rather than
// recorded, then delete the rows they own here.
const UNSWEPT_BUILTINS: &[(&str, TaskCite)] = &[
    ("union", TaskCite(7714)),
    ("intersection", TaskCite(7714)),
    ("difference", TaskCite(7714)),
    ("union_all", TaskCite(7714)),
    ("intersection_all", TaskCite(7714)),
    ("sweep", TaskCite(7714)),
    ("sweep_guided", TaskCite(7714)),
    ("zone_annulus", TaskCite(7714)),
    ("zone_profile", TaskCite(7714)),
    ("rounded_box", TaskCite(7714)),
    ("rounded_rect", TaskCite(7714)),
    ("arc", TaskCite(5783)),
];

/// Contiguous position spans the sweep REACHES but cannot read cleanly, each
/// with the reason and its owner.
///
/// `(builtin, arity, index range, cite, note)`.
// TODO(#5783): the ANGLE positions. angle-units ν extends this guard's
// allowlist with `expected: DimensionVector::ANGLE` rows; until it lands the
// two `circular_pattern` angle slots are recorded here rather than justified,
// because "dimensionless" is exactly what they are NOT.
const RESIDUAL_SPANS: &[(&str, usize, std::ops::Range<usize>, TaskCite, &str)] = &[
    (
        "linear_pattern_2d",
        10,
        0..10,
        TaskCite(7714),
        "count1/count2 need Int values no scalar filler produces, so the op \
         errors before spacing1/spacing2 reach the Contract C chokepoint; both \
         spacing slots ARE gated (task 5214) but the probe cannot show it",
    ),
    (
        "nurbs",
        10,
        0..10,
        TaskCite(7714),
        "the gated pole span is `2 .. 2 + 3·n_points`, computed FROM an \
         argument, so a valid call needs a small-integer n_points plus exactly \
         3·n_points pole coordinates",
    ),
    (
        "nurbs_surface",
        6,
        0..1,
        TaskCite(7714),
        "`control_points` is a GRID gated through the decoded-value route (task \
         5745); a scalar literal cannot occupy it",
    ),
    (
        "circular_pattern",
        3,
        2..3,
        TaskCite(5783),
        "the short form's `angle` — an ANGLE position, owned by PRD 3",
    ),
    (
        "circular_pattern",
        8,
        7..8,
        TaskCite(5783),
        "the long form's `angle` — an ANGLE position, owned by PRD 3",
    ),
];

/// Length-semantic sites OUTSIDE this guard's universe, recorded so a green
/// sweep is never read as covering them.
// TODO(#6089): the reify-stdlib `decompose_transform` consumers.
// TODO(#5810): PRD 5's second universe of reify-stdlib `eval_builtin` names.
// TODO(#7484): the five construction-datum constructors, in neither universe.
const OUT_OF_UNIVERSE: &[(&str, TaskCite, &str)] = &[
    (
        "reify_stdlib::geometry::affine_from_transform",
        TaskCite(6089),
        "discards the translation dimension into `_dim`; a pure VALUE-layer \
         stdlib builtin that mints an AffineMap and never produces a \
         `CompiledGeometryOp`, so this guard cannot reach it",
    ),
    (
        "reify_stdlib::geometry::transform_inverse",
        TaskCite(6089),
        "propagates whatever dimension arrived through \
         `make_dimensioned_component`; same value-layer reason",
    ),
    (
        "reify-stdlib eval_builtin names: plane_*/axis_*/point3, prb_*, joints, \
         trajectory, FEA",
        TaskCite(5810),
        "no compiler signature and no `.ri` declaration, so they are absent from \
         GEOMETRY_FUNCTION_NAMES; PRD 5 adds them as a SECOND universe",
    ),
    (
        "midplane / axis_through / plane_through / offset(arity 2) / frame_at",
        TaskCite(7484),
        "the five construction-datum constructors are in NEITHER this universe \
         nor C5's named second universe",
    ),
];

/// Every position the sweep reaches that is dimensionless BY DESIGN, with the
/// D14 reading that licenses it.
///
/// `(builtin, arity, index range, justification, the slots the range covers)`.
/// Authored from a measured sweep, never guessed: each row is a position the
/// probe observed accepting a bare number on a tree where its neighbours were
/// rejected for it.
const ALLOWLIST: &[(&str, usize, std::ops::Range<usize>, Justification, &str)] = &[
    // DIRECTIONS. The origin-vs-direction split, drawn the same way everywhere:
    // the ORIGIN triple of an axis or plane is gated, its DIRECTION triple is
    // not, because a unit vector legitimately has bare components and gating it
    // would reject correct `.ri`. This is the D3 adversary finding (BINDING).
    (
        "circular_pattern",
        8,
        3..6,
        Justification::UnitVectorComponent,
        "ax/ay/az — the axis direction, beside the gated ox/oy/oz origin",
    ),
    (
        "extrude_infinite",
        4,
        0..3,
        Justification::UnitVectorComponent,
        "dx/dy/dz — the extrusion direction; infinite extent has no length",
    ),
    (
        "half_space",
        6,
        3..6,
        Justification::UnitVectorComponent,
        "nx/ny/nz — the outward normal, beside the gated px/py/pz point",
    ),
    (
        "linear_pattern",
        5,
        0..3,
        Justification::UnitVectorComponent,
        "dx/dy/dz — the step direction; the magnitude is the gated `spacing`",
    ),
    (
        "mirror",
        6,
        3..6,
        Justification::UnitVectorComponent,
        "nx/ny/nz — the plane normal, beside the gated ox/oy/oz origin",
    ),
    (
        "offset_curve",
        2,
        1..2,
        Justification::UnitVectorComponent,
        "the third argument when it is not a reference Surface — its own \
         production diagnostic calls it \"a direction vec3\"",
    ),
    (
        "revolve",
        7,
        3..6,
        Justification::UnitVectorComponent,
        "ax/ay/az — the axis direction, beside the gated ox/oy/oz origin",
    ),
    (
        "revolve_full",
        6,
        3..6,
        Justification::UnitVectorComponent,
        "ax/ay/az — the axis direction, beside the gated ox/oy/oz origin",
    ),
    (
        "rotate",
        4,
        0..3,
        Justification::UnitVectorComponent,
        "ax/ay/az — the rotation axis direction",
    ),
    (
        "rotate_around",
        7,
        3..6,
        Justification::UnitVectorComponent,
        "ax/ay/az — the axis direction, beside the gated px/py/pz pivot",
    ),
    // COUNTS.
    (
        "circular_pattern",
        3,
        1..2,
        Justification::Count,
        "count — how many instances",
    ),
    (
        "linear_pattern",
        5,
        3..4,
        Justification::Count,
        "count — how many instances",
    ),
    // ENTITY SELECTORS. These slots carry a handle, a selector list or a datum
    // VALUE; they name something rather than measure it.
    (
        "affine_apply",
        1,
        0..1,
        Justification::Index,
        "map — an AffineMap value",
    ),
    (
        "apply_transform",
        1,
        0..1,
        Justification::Index,
        "transform — a Transform value",
    ),
    (
        "arbitrary_pattern",
        1,
        0..1,
        Justification::Index,
        "transform_list — the LIST form's per-element transforms",
    ),
    (
        "chamfer",
        2,
        0..1,
        Justification::Index,
        "edges — an edge selector",
    ),
    (
        "chamfer_asymmetric",
        3,
        0..1,
        Justification::Index,
        "edges — an edge selector",
    ),
    (
        "circular_pattern",
        3,
        0..1,
        Justification::Index,
        "axis — an Axis datum",
    ),
    (
        "draft",
        2,
        1..2,
        Justification::Index,
        "plane — the neutral Plane datum",
    ),
    (
        "draft",
        3,
        0..1,
        Justification::Index,
        "faces — a face selector",
    ),
    (
        "draft",
        3,
        2..3,
        Justification::Index,
        "plane — the neutral Plane datum",
    ),
    (
        "fillet",
        2,
        0..1,
        Justification::Index,
        "edges — an edge selector",
    ),
    (
        "loft",
        1,
        0..1,
        Justification::Index,
        "profile_1 — a profile step handle",
    ),
    (
        "loft_guided",
        2,
        0..2,
        Justification::Index,
        "profile_1 and guide — step handles",
    ),
    (
        "mirror",
        1,
        0..1,
        Justification::Index,
        "plane — a Plane datum, the short form of the six-argument call",
    ),
    (
        "rotate",
        1,
        0..1,
        Justification::Index,
        "orientation — an Orientation value, the short form of the four-\
         argument call",
    ),
    (
        "shell",
        2,
        1..2,
        Justification::Index,
        "face_0 — a face selector",
    ),
    (
        "shell_open",
        2,
        1..2,
        Justification::Index,
        "open_faces — a face selector",
    ),
    // THE REMAINING D14 READINGS.
    (
        "scale",
        1,
        0..1,
        Justification::Factor,
        "factor — a dimensionless scale factor",
    ),
    (
        "nurbs_surface",
        6,
        1..2,
        Justification::Weight,
        "weights — rational blending weights",
    ),
    (
        "nurbs_surface",
        6,
        2..4,
        Justification::Knot,
        "u_knots/v_knots — parameter-space values, not coordinates in metres",
    ),
    (
        "nurbs_surface",
        6,
        4..6,
        Justification::Degree,
        "u_degree/v_degree — polynomial degrees",
    ),
    (
        "extrude_infinite",
        4,
        3..4,
        Justification::ModeFlag,
        "direction — which side(s) of the profile the infinite extent covers",
    ),
    (
        "isosurface",
        3,
        2..3,
        Justification::ModeFlag,
        "adaptive — the boolean that picks adaptive meshing (default false)",
    ),
];

fn shipped_allowlist() -> Vec<AllowEntry> {
    ALLOWLIST
        .iter()
        .flat_map(|(builtin, arity, indices, justification, _slots)| {
            indices.clone().map(move |index| AllowEntry {
                position: Position {
                    builtin,
                    arity: *arity,
                    index,
                },
                expected: reify_core::DimensionVector::DIMENSIONLESS,
                justification: *justification,
            })
        })
        .collect()
}

fn shipped_residuals() -> Vec<Residual> {
    let unswept = UNSWEPT_BUILTINS.iter().map(|&(builtin, cite)| Residual {
        subject: ResidualSubject::Builtin(builtin),
        cite,
        note: "no compiling call is synthesizable from scalar literals",
    });
    let spans = RESIDUAL_SPANS
        .iter()
        .flat_map(|(builtin, arity, indices, cite, note)| {
            indices.clone().map(move |index| Residual {
                subject: ResidualSubject::Position(Position {
                    builtin,
                    arity: *arity,
                    index,
                }),
                cite: *cite,
                note,
            })
        });
    let outside = OUT_OF_UNIVERSE
        .iter()
        .map(|&(subject, cite, note)| Residual {
            subject: ResidualSubject::OutOfUniverse(subject),
            cite,
            note,
        });
    unswept.chain(spans).chain(outside).collect()
}

/// The registry this guard ships: the justified allowlist plus the owned
/// residuals.
fn shipped_registry() -> Registry {
    Registry::from_parts(shipped_allowlist(), shipped_residuals())
}

#[cfg(test)]
mod seeded_cites {
    use super::*;

    /// (i) A cite renders in the canonical PTODO form, and nothing else is
    /// representable.
    #[test]
    fn a_cite_renders_as_hash_followed_by_digits_only() {
        assert_eq!(TaskCite(6089).to_string(), "#6089");

        for cite in [TaskCite(1), TaskCite(5783), TaskCite(7714)] {
            let rendered = cite.to_string();
            let mut chars = rendered.chars();
            assert_eq!(chars.next(), Some('#'), "{rendered:?} must start with '#'");
            assert!(
                chars.clone().count() > 0 && chars.all(|c| c.is_ascii_digit()),
                "{rendered:?} must be '#' followed by digits only — the Greek-letter, \
                 hyphenated and prose spellings the PTODO grammar rejects are \
                 unrepresentable by construction, not filtered at runtime"
            );
        }
    }

    /// (ii) A cite that appears only in a residual's free-text `note` registers
    /// nothing.
    ///
    /// Coverage comes from a structured [`ResidualSubject`], never from prose.
    /// Without this, a residual could be "created" by mentioning a position in
    /// someone else's note.
    #[test]
    fn a_position_named_only_in_free_text_is_not_covered() {
        let position = Position {
            builtin: "nurbs",
            arity: 10,
            index: 0,
        };
        let decoy = Residual {
            subject: ResidualSubject::OutOfUniverse("a different site entirely"),
            cite: TaskCite(7714),
            note: "prose mentioning nurbs arity 10 index 0 and #7714",
        };
        let registry = Registry::from_parts([], [decoy]);

        assert!(
            !registry.covers(&position),
            "a position named only in another row's note must not be covered"
        );
        assert_eq!(
            classify_all(
                &[Observation {
                    position,
                    outcome: ProbeOutcome::Accepted,
                }],
                &registry
            )
            .len(),
            1
        );
    }

    /// (iii) Every shipped residual's cite is ALSO written as a PTODO marker
    /// comment in this file, so the PTODO detector performs the liveness check
    /// this test cannot.
    ///
    /// This is a cite-liveness mechanism, not a docstring-wording pin: it checks
    /// that a structured identifier is visible to another gate.
    #[test]
    fn every_shipped_residual_cite_has_a_todo_comment_in_this_file() {
        let residuals = shipped_registry().residuals;
        assert!(
            !residuals.is_empty(),
            "the shipped residual list is empty — this check would be vacuous"
        );

        for residual in &residuals {
            assert!(
                !residual.note.trim().is_empty(),
                "residual {residual} states no reason — a row with a cite but no \
                 stated gap is a placeholder with nothing in it"
            );
        }

        let source = this_file();
        let mut cites: Vec<TaskCite> = residuals.iter().map(|r| r.cite).collect();
        cites.sort_unstable();
        cites.dedup();

        for cite in cites {
            let marker = format!("// TODO({cite}):"); // ptodo:allow — the matcher, not a marker
            assert!(
                source.contains(&marker),
                "residual cite {cite} has no `{marker}` comment in this file. \
                 Without it the PTODO detector never sees the cite, so nothing \
                 checks that the owning task is still live."
            );
        }
    }
}

// -- Step-3: the real tree --

/// The geometry operand a call needs before its numeric arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TargetTemplate {
    /// No operand — the builtin constructs geometry from numbers alone.
    None,
    /// A solid operand, for the modify / transform / pattern families.
    Solid,
    /// A planar profile operand, for the sweep family.
    Profile,
}

impl TargetTemplate {
    /// The table the sweep iterates, most-specific first.
    ///
    /// Every builtin is offered every template and keeps the ones that compile.
    /// Deliberately NOT a per-name map: a hand-written `name -> template` table
    /// would need maintaining alongside the compiler's signatures, and deriving
    /// one from the name's spelling would be exactly the ad-hoc string sniffing
    /// this codebase bans.
    const ALL: [TargetTemplate; 3] = [
        TargetTemplate::Solid,
        TargetTemplate::Profile,
        TargetTemplate::None,
    ];

    /// The operand's source text, including its trailing separator.
    fn prefix(self) -> &'static str {
        match self {
            TargetTemplate::None => "",
            TargetTemplate::Solid => "box(1mm, 1mm, 1mm), ",
            TargetTemplate::Profile => "circle(1mm), ",
        }
    }

    /// Geometry ops the operand itself contributes ahead of the call under test.
    fn contributed_ops(self) -> usize {
        match self {
            TargetTemplate::None => 0,
            TargetTemplate::Solid | TargetTemplate::Profile => 1,
        }
    }
}

/// Values tried at the positions NOT under test, while a rejection-free baseline
/// is sought.
///
/// A baseline must raise ZERO dimension rejections before any position can be
/// read, so a call whose other arguments are themselves being rejected cannot be
/// mistaken for evidence about the one position under test. The ladder spans the
/// dimensions and shapes those other positions expect: a length, an angle, small
/// integers for counts and degrees, and a fraction.
const BASELINE_FILLERS: [&str; 6] = ["1mm", "1deg", "2", "0.5", "3", "1"];

/// The bare, dimensionless value planted at the position under test.
///
/// Distinct per index so the value can be traced back to its position
/// structurally — by looking for it among the compiled op's literal arguments —
/// rather than by reading it out of a diagnostic's prose.
fn bare_sentinel(index: usize) -> String {
    format!("{}", 1000 + index)
}

/// `structure def S { let x = <name>(<operand>, <args>) }`.
fn probe_source(name: &str, template: TargetTemplate, args: &[String]) -> String {
    format!(
        "structure def S {{ let x = {}({}{}) }}",
        name,
        template.prefix(),
        args.join(", ")
    )
}

/// The geometry op the call under test compiled to, or `None` if it compiled to
/// no op of its own.
///
/// The call under test is the OUTERMOST one, so its op is the LAST emitted —
/// the operand's own ops come first. Requiring strictly more ops than the
/// template contributes is what keeps the operand's `box`/`circle` from being
/// mistaken for the call under test.
fn compile_call(
    source: &str,
    template: TargetTemplate,
) -> Option<reify_compiler::CompiledGeometryOp> {
    let compiled = reify_test_support::compile_source(source);
    let mut ops: Vec<reify_compiler::CompiledGeometryOp> = compiled
        .templates
        .iter()
        .flat_map(|t| t.realizations.iter())
        .flat_map(|r| r.operations.iter().cloned())
        .collect();
    (ops.len() > template.contributed_ops())
        .then(|| ops.pop())
        .flatten()
}

/// An op's named argument slots.
///
/// Exhaustive with no `_` arm: a new `CompiledGeometryOp` variant is a compile
/// error here until it is classified, rather than silently sweeping none of its
/// positions.
fn op_args(op: &reify_compiler::CompiledGeometryOp) -> &[(String, reify_ir::CompiledExpr)] {
    use reify_compiler::CompiledGeometryOp as Op;
    match op {
        Op::Primitive { args, .. }
        | Op::Modify { args, .. }
        | Op::Transform { args, .. }
        | Op::Pattern { args, .. }
        | Op::Sweep { args, .. }
        | Op::Curve { args, .. }
        | Op::Profile { args, .. }
        | Op::Surface { args, .. }
        | Op::Isosurface { args, .. } => args,
        // Two geometry operands and no numeric slot at all.
        Op::Boolean { .. } => &[],
    }
}

/// Whether `value` survived into one of the op's top-level literal arguments.
fn op_carries_literal(op: &reify_compiler::CompiledGeometryOp, value: f64) -> bool {
    op_args(op).iter().any(|(_, expr)| {
        matches!(&expr.kind, reify_ir::CompiledExprKind::Literal(v)
            if v.as_f64().is_some_and(|f| (f - value).abs() < 1e-9))
    })
}

/// One driven call: the op, and what the eval-side op compiler made of it.
struct Probed {
    op: reify_compiler::CompiledGeometryOp,
    result: Result<reify_ir::GeometryOp, String>,
    diagnostics: Vec<reify_core::Diagnostic>,
}

impl Probed {
    fn dimension_rejections(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.code == Some(reify_core::DiagnosticCode::DimensionedArgRejected))
            .count()
    }

    /// How unusable this call is as a BASELINE, worst first: dimension
    /// rejections, then any other diagnostic, then a failed op compile.
    fn baseline_badness(&self) -> (usize, usize, usize) {
        (
            self.dimension_rejections(),
            self.diagnostics.len(),
            usize::from(self.result.is_err()),
        )
    }
}

/// Compile `args` for `name` and drive the resulting op through the eval-side op
/// compiler.
///
/// `values`, `functions`, `meta_map` and `named_steps` are empty and the step
/// handles are synthetic — the `run()` shape from
/// `tests/compile_geometry_op_characterization.rs`. IR-BUILD ONLY: no geometry
/// kernel is constructed anywhere on this path.
fn probe_call(name: &str, template: TargetTemplate, args: &[String]) -> Option<Probed> {
    let op = compile_call(&probe_source(name, template, args), template)?;
    let values = reify_ir::ValueMap::new();
    let meta_map: std::collections::HashMap<String, std::collections::HashMap<String, String>> =
        std::collections::HashMap::new();
    let named_steps: std::collections::HashMap<String, reify_ir::KernelHandle> =
        std::collections::HashMap::new();
    let step_handles: Vec<reify_ir::GeometryHandleId> =
        (0..8).map(reify_ir::GeometryHandleId).collect();
    let mut diagnostics: Vec<reify_core::Diagnostic> = Vec::new();
    let result = reify_eval::geometry_op_characterization_probe::compile_geometry_op_probe(
        &op,
        &values,
        &step_handles,
        &[],
        &meta_map,
        &named_steps,
        &mut diagnostics,
    );
    Some(Probed {
        op,
        result,
        diagnostics,
    })
}

/// An argument vector for `name` at `arity` that raises no dimension rejection
/// of its own, or `None` if no combination of [`BASELINE_FILLERS`] achieves one.
///
/// Greedy, one position at a time, which is enough because the fillers do not
/// interact: each position's acceptable dimension is independent of its
/// neighbours'. Two passes let a later repair unblock an earlier one.
fn baseline_args(name: &str, template: TargetTemplate, arity: usize) -> Option<Vec<String>> {
    let mut args: Vec<String> = vec![BASELINE_FILLERS[0].to_string(); arity];
    let mut badness = probe_call(name, template, &args)?.baseline_badness();
    for _ in 0..2 {
        if badness == (0, 0, 0) {
            break;
        }
        for index in 0..arity {
            for filler in BASELINE_FILLERS {
                let mut candidate = args.clone();
                candidate[index] = filler.to_string();
                let Some(probed) = probe_call(name, template, &candidate) else {
                    continue;
                };
                if probed.baseline_badness() < badness {
                    badness = probed.baseline_badness();
                    args = candidate;
                }
            }
        }
    }
    (badness.0 == 0).then_some(args)
}

/// The argument-slot FAMILIES an op exposes — slot names with their trailing
/// digits removed.
///
/// A higher arity that introduces no new family only lengthens a variadic run
/// (`profile_1`, `profile_2`, … ; `face_0`, `face_1`, …) whose first member the
/// sweep has already read, so it can exercise no rule the lower arity did not.
/// Skipping those arities is what keeps the sweep from generating hundreds of
/// indistinguishable positions. ACCEPTED LIMITATION: a variadic run whose LATER
/// members are governed differently from its first would be missed.
fn slot_families(op: &reify_compiler::CompiledGeometryOp) -> std::collections::BTreeSet<String> {
    op_args(op)
        .iter()
        .map(|(name, _)| name.chars().filter(|c| !c.is_ascii_digit()).collect())
        .collect()
}

/// Sweep the whole universe once.
///
/// For each builtin, target template and arity: establish a rejection-free
/// baseline, then re-probe once per position with a bare number planted there.
/// A dimension rejection that appears against that clean baseline can only be
/// about the position under test, which is what makes per-position attribution
/// structural rather than a matter of reading diagnostic prose.
fn sweep_universe() -> Vec<Observation> {
    let mut observations = Vec::new();
    for &builtin in probe_universe() {
        let mut families_seen: std::collections::BTreeSet<String> = Default::default();
        for template in TargetTemplate::ALL {
            for arity in 1..=MAX_PROBED_ARITY {
                let Some(args) = baseline_args(builtin, template, arity) else {
                    continue;
                };
                let Some(baseline) = probe_call(builtin, template, &args) else {
                    continue;
                };
                let families = slot_families(&baseline.op);
                if families.is_subset(&families_seen) {
                    continue;
                }
                families_seen.extend(families);

                for index in 0..arity {
                    let mut probe_args = args.clone();
                    probe_args[index] = bare_sentinel(index);
                    let position = Position {
                        builtin,
                        arity,
                        index,
                    };
                    let outcome = match probe_call(builtin, template, &probe_args) {
                        None => ProbeOutcome::NotReached,
                        Some(probed) if probed.dimension_rejections() > 0 => {
                            ProbeOutcome::ContractCRejected
                        }
                        Some(probed) if !op_carries_literal(&probed.op, (1000 + index) as f64) => {
                            ProbeOutcome::NotReached
                        }
                        Some(probed) => {
                            outcome_from_probe(&probed.result, &probed.diagnostics, &position)
                        }
                    };
                    observations.push(Observation { position, outcome });
                }
            }
        }
    }
    observations
}

/// The sweep, run once per PROCESS and shared by every Step-3 test in it.
///
/// Per process, not per binary: `cargo test` runs this binary's tests as
/// threads of one process, so one sweep serves all four Step-3 tests, while
/// `cargo nextest` — the gate's runner — gives each test its own process and
/// each pays its own sweep. That is why the binary costs 5.3s under the former
/// and 7-8s under the latter, both far inside the ceiling that keeps
/// `.config/nextest.toml` free of an override for it.
fn observe_universe() -> &'static [Observation] {
    static SWEEP: std::sync::OnceLock<Vec<Observation>> = std::sync::OnceLock::new();
    SWEEP.get_or_init(sweep_universe)
}

#[cfg(test)]
mod seeded_stubbed_gate {
    use super::*;

    /// `mirror`'s plane-origin `ox` — gated on this tree (measured), and the
    /// origin half of the D3 origin-vs-direction split whose `nx`/`ny`/`nz`
    /// sibling is deliberately bare.
    const GATED_TODAY: Position = Position {
        builtin: "mirror",
        arity: 6,
        index: 0,
    };

    /// (b) A shipped gate reported as ACCEPTED produces a violation naming that
    /// position, and the same position reported as rejected produces none.
    ///
    /// The seed stubs the gate in the OBSERVATION, touching no production code.
    /// What it pins is that `classify_all` reads the OUTCOME and not merely the
    /// registry: a gate deleted from `geometry_ops` would show up here, not
    /// merely a row deleted from the allowlist.
    #[test]
    fn a_stubbed_out_gate_is_caught_and_a_live_one_is_not() {
        let registry = shipped_registry();

        let stubbed = classify_all(
            &[Observation {
                position: GATED_TODAY,
                outcome: ProbeOutcome::Accepted,
            }],
            &registry,
        );
        assert_eq!(
            stubbed.len(),
            1,
            "stubbing out {GATED_TODAY:?} must produce exactly one violation"
        );
        assert_eq!(stubbed[0].position, GATED_TODAY);
        let rendered = stubbed[0].to_string();
        assert!(
            rendered.contains("mirror") && rendered.contains('6') && rendered.contains('0'),
            "violation must name builtin, arity and index; got {rendered:?}"
        );

        let live = classify_all(
            &[Observation {
                position: GATED_TODAY,
                outcome: ProbeOutcome::ContractCRejected,
            }],
            &registry,
        );
        assert!(
            live.is_empty(),
            "a Contract-C-rejected position needs no entry; got {live:?}"
        );
    }

    /// An op that fails for a reason OTHER than the dimension gate is not read
    /// as the gate firing.
    ///
    /// Without this distinction a position could stay green purely because
    /// something unrelated about the call was broken.
    #[test]
    fn an_op_failure_without_a_dimension_rejection_is_not_a_gate_firing() {
        let failed = outcome_from_probe(
            &Err("unrelated failure".to_string()),
            &[reify_core::Diagnostic::error("some other problem")],
            &GATED_TODAY,
        );
        assert!(matches!(failed, ProbeOutcome::OpCompileFailed(_)));

        let violations = classify_all(
            &[Observation {
                position: GATED_TODAY,
                outcome: failed,
            }],
            &Registry::default(),
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].reason,
            ViolationReason::FailedWithoutDimensionRejection
        );
    }

    /// The rejection signal is the structured `DiagnosticCode`, not the message.
    #[test]
    fn the_rejection_signal_is_the_diagnostic_code() {
        let coded = reify_core::Diagnostic::error("wording that may change")
            .with_code(reify_core::DiagnosticCode::DimensionedArgRejected);
        assert_eq!(
            outcome_from_probe(&Err("dropped".to_string()), &[coded], &GATED_TODAY),
            ProbeOutcome::ContractCRejected
        );

        let uncoded = reify_core::Diagnostic::error(
            "argument 'ox' for mirror expects Length, got Int; pass a dimensioned \
             length such as `5mm`",
        );
        assert!(
            matches!(
                outcome_from_probe(&Err("dropped".to_string()), &[uncoded], &GATED_TODAY),
                ProbeOutcome::OpCompileFailed(_)
            ),
            "a rejection-shaped MESSAGE without the code must not count — the code \
             is the contract"
        );
    }
}

#[cfg(test)]
mod real_tree {
    use super::*;

    fn render(violations: &[Violation]) -> String {
        violations
            .iter()
            .map(|v| format!("  {v}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// THE GATE. Every position the sweep reaches is Contract-C-rejected, or
    /// allowlisted with a D14 reading, or owned by a residual row.
    ///
    /// The two tests below it are what make a green run here mean something:
    /// one proves no allowlist entry is dead weight, the other proves the
    /// rejections are observed rather than assumed.
    #[test]
    fn closure_guard_is_green_over_the_whole_universe() {
        let observations = observe_universe();
        let violations = classify_all(observations, &shipped_registry());

        assert!(
            violations.is_empty(),
            "{} un-gated, unjustified position(s):\n{}\n\n\
             Each one is a numeric argument slot that ACCEPTS a bare number. \
             Either gate it — route it through `accept_arg(&value, &length_spec())` \
             in `geometry_ops`, the Contract C chokepoint — or, if it is \
             legitimately dimensionless, add a row to ALLOWLIST with the D14 \
             `Justification` that licenses it. If it is neither settled nor \
             justified, add a `Residual` row with a LIVE task cite and its \
             matching PTODO marker comment. Do not add a row whose \
             justification you cannot defend: that is the prose completeness \
             claim this guard replaced.",
            violations.len(),
            render(&violations)
        );
    }

    /// The sweep actually swept something.
    ///
    /// A `classify_all` over an empty or tiny observation set is trivially
    /// green, so the green above is only worth having with a floor under the
    /// evidence it rests on. Floors, not equalities: adding builtins or arities
    /// must never red this.
    #[test]
    fn the_sweep_observes_a_substantial_gated_surface() {
        let observations = observe_universe();
        let rejected = observations
            .iter()
            .filter(|o| o.outcome == ProbeOutcome::ContractCRejected)
            .count();

        assert!(
            observations.len() >= 150,
            "the sweep produced {} observations; 187 were measured when this \
             guard was written and the floor is 150. A collapse means the probe \
             stopped compiling its synthesized calls, which would make the gate \
             above vacuously green.",
            observations.len()
        );
        assert!(
            rejected >= 80,
            "only {rejected} of {} observed positions are Contract-C-rejected; \
             107 were measured and the floor is 80. A drop means gates \
             disappeared from `geometry_ops`.",
            observations.len()
        );
    }

    /// ANTI-VACUITY I — no allowlist entry is dead weight.
    ///
    /// Removing any single entry must make the guard fire at exactly that
    /// position. An entry that survives its own removal is describing a
    /// position the sweep never reaches, and is documentation pretending to be
    /// a gate.
    #[test]
    fn shrinking_the_shipped_allowlist_by_one_entry_makes_the_guard_fire() {
        let observations = observe_universe();
        let entries = shipped_allowlist();
        assert!(!entries.is_empty(), "the shipped allowlist is empty");

        for dropped in &entries {
            let weakened = Registry::from_parts(
                entries
                    .iter()
                    .filter(|e| e.position != dropped.position)
                    .copied(),
                shipped_residuals(),
            );
            let violations = classify_all(observations, &weakened);

            assert!(
                violations.iter().any(|v| v.position == dropped.position),
                "dropping the allowlist entry `{dropped}` produced no violation \
                 naming it, so that entry suppresses nothing. Either the sweep \
                 no longer reaches the position — in which case delete the row \
                 — or the position is now gated, in which case delete it too. \
                 Violations seen:\n{}",
                render(&violations)
            );
        }
    }

    /// Every shipped allowlist row claims DIMENSIONLESS, and names its reading.
    ///
    /// This is the extension point held open rather than left implicit: an
    /// ANGLE row belongs to PRD 3 (#5783) and must arrive with its own observed
    /// evidence, not by re-dimensioning a row that was authored from a
    /// dimensionless observation.
    #[test]
    fn every_allowlist_row_expects_dimensionless_and_names_its_reading() {
        for entry in shipped_allowlist() {
            assert_eq!(
                entry.expected,
                reify_core::DimensionVector::DIMENSIONLESS,
                "`{entry}` expects a non-dimensionless dimension. Every row this \
                 guard ships was authored from an observation of a BARE value \
                 being accepted, which is a dimensionless reading. A row for \
                 another dimension needs its own observation."
            );
        }
    }

    /// ANTI-VACUITY II — the gates are OBSERVED, not assumed.
    ///
    /// Flipping one observed rejection to an acceptance — a gate stubbed out in
    /// the observation, with no edit to production code — must be caught. This
    /// is what distinguishes the green above from "the registry happens to
    /// cover everything the sweep noticed".
    #[test]
    fn stubbing_out_one_shipped_gate_makes_the_guard_fire() {
        let observations = observe_universe();
        let registry = shipped_registry();

        let gated: Vec<&Observation> = observations
            .iter()
            .filter(|o| o.outcome == ProbeOutcome::ContractCRejected)
            .collect();
        assert!(
            !gated.is_empty(),
            "no observed position is Contract-C-rejected — there is no gate to stub"
        );

        for target in gated {
            let stubbed: Vec<Observation> = observations
                .iter()
                .map(|o| Observation {
                    position: o.position,
                    outcome: if o.position == target.position {
                        ProbeOutcome::Accepted
                    } else {
                        o.outcome.clone()
                    },
                })
                .collect();
            let violations = classify_all(&stubbed, &registry);

            assert!(
                violations.iter().any(|v| v.position == target.position),
                "stubbing out the gate at {:?} produced no violation naming it — \
                 a registry row is shadowing a live gate, so that gate could be \
                 deleted without this guard noticing. Violations seen:\n{}",
                target.position,
                render(&violations)
            );
        }
    }
}
