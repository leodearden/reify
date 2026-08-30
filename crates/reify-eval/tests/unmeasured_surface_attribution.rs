//! Over-reach guards for task 6667's surface-attribution emissions.
//!
//! # These tests are GREEN ON ARRIVAL — they are not RED drivers
//!
//! The RED/GREEN drivers for this work are the GUI-surface pins in
//! `gui/src-tauri/src/tests/unmeasured_attribution_tests.rs`. Every test in
//! THIS file is expected to pass the moment it is written; its job is to fail
//! if the emission over-reaches — if it attributes an Indeterminate on a
//! surface that DID measure, if it fires on a module that carries none of the
//! relevant constraint kinds, or if generalizing ζ's reason taxonomy (task
//! 6169) changed any of ζ's own strings. A reader who mistakes these for RED
//! drivers will misread a passing run as "nothing was implemented".
//!
//! # Why an engine-level file at all
//!
//! The GUI pins are the contract, but they are expensive to iterate on (the
//! `reify-gui` test binary links the whole GUI dep-tree). These are the cheap
//! loop over the same behaviour, and — more importantly — they can construct
//! surfaces the GUI cannot: a MEASURING one (`build()` with a mock kernel,
//! populating `realization_handles`) is exactly the shape the GUI never
//! reaches, so the over-reach half of the contract is only testable here.

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DiagnosticCode, Severity};
use reify_eval::{CheckResult, Engine};
use reify_ir::{ExportFormat, Satisfaction};
use reify_test_support::{MockGeometryKernel, parse_and_compile, parse_and_compile_with_stdlib};

// ── Fixtures ─────────────────────────────────────────────────────────────────

/// A geometry-bearing template (so `build()` populates `realization_handles`)
/// alongside a geometric `Conforms` whose own operands do not resolve to live
/// handles.
///
/// That combination is the whole point: it produces a MEASURING surface
/// (`realization_handles_len() > 0`) that nonetheless leaves this particular
/// callout Indeterminate — the exact analogue of the shape ζ guards with
/// `measuring_surface_indeterminate_carries_no_attribution_diagnostic`
/// (a non-empty `achieved_repr_tol` that holds no entry for THIS subject).
///
/// `Widget` is modelled on `reset_per_build_state_classification.rs`'s
/// `WIDGET_SRC`, which is already proven to populate `realization_handles`
/// under `MockGeometryKernel`.
const MEASURING_SURFACE_GDT_SOURCE: &str = r#"
structure def Widget {
    param width : Length = 10mm
    param geometry : Solid = box(width, 20mm, 30mm)
}

structure def Probe {
    param tol : Flatness = Flatness(tolerance_value: 0.1mm, feature: box(1mm, 1mm, 1mm))
    param act : Geometry = box(1mm, 1mm, 1mm)
    constraint Conforms(tolerance: tol, measured_deviation: 0mm, feature_departure: 0mm, actual: act)
}
"#;

/// A module with NO geometric `Conforms` — no constraint carries an `actual`
/// binding — and no DFM rule. `measure_gdt_conformance`'s fast no-op
/// early-return must fire and nothing may be emitted.
///
/// No stdlib needed: `mm` is a built-in length unit.
const NO_GEOMETRIC_CONFORMS_SOURCE: &str = r#"
structure Plain {
    param len : Length = 5mm
    constraint len >= 0mm
}
"#;

/// ζ's own fixture shape: a `RepresentationWithin` whose subject param carries
/// a **default**, so every operand is genuinely DEFINED at check time and the
/// peel does not decline the entry to the language-level checker.
///
/// Copied from `representation_within_assertion.rs`'s
/// `NON_MEASURING_SURFACE_SOURCE` — deliberately the same shape, because this
/// file's job is to prove ζ's answers did not move.
const ZETA_SOURCE: &str = r#"
structure MyGeom {
    param x : Real = 1.0
}

structure Checker {
    param subject : MyGeom = MyGeom()
    constraint RepresentationWithin(subject, 1mm)
}
"#;

// ── ζ's three reason strings, transcribed verbatim ───────────────────────────
//
// These are the ONLY full-sentence assertions in this file. Everywhere else the
// pinning contract is stable TOKENS only (`representation_within_assertion.rs`
// :185). The exception is deliberate and is the entire point of
// `zeta_representation_within_reasons_are_byte_identical_after_the_refactor`:
// token matching would not catch a reworded clause, and "ζ's strings are
// unchanged" is precisely a byte-level claim.

const ZETA_ARM_1_NO_CAPABLE_KERNEL: &str = "this run does not measure representation tolerance \
     because no geometry kernel able to tessellate the subject is registered; such a geometry \
     kernel is required — build with OCCT to evaluate this RepresentationWithin bound";

const ZETA_ARM_2_CAPTURE_OFF: &str = "this evaluation surface does not measure representation \
     tolerance (no tessellation ran, so no achieved deviation exists for the subject); run \
     `reify check` to evaluate this RepresentationWithin bound";

const ZETA_ARM_3_NOTHING_MEASURED: &str = "no realization of the subject was tessellated on this \
     run, so no achieved deviation exists for it and this run does not measure representation \
     tolerance; check that the subject declares a realization";

// ── Helpers ──────────────────────────────────────────────────────────────────

/// A kernel that fails every operation — registered, but nothing it is asked to
/// do succeeds, so no realization is ever produced.
///
/// Redefined here rather than shared: `representation_within_assertion.rs`
/// declares its own copy privately, and integration tests are separate crates.
struct NoRealizationStubKernel;

impl reify_ir::GeometryKernel for NoRealizationStubKernel {
    fn execute(
        &mut self,
        _op: &reify_ir::GeometryOp,
    ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
        Err(reify_ir::GeometryError::OperationFailed("stub".into()))
    }
    fn query(&self, _q: &reify_ir::GeometryQuery) -> Result<reify_ir::Value, reify_ir::QueryError> {
        Err(reify_ir::QueryError::QueryFailed("stub".into()))
    }
    fn export(
        &self,
        _h: reify_ir::GeometryHandleId,
        _f: reify_ir::ExportFormat,
        _w: &mut dyn std::io::Write,
    ) -> Result<(), reify_ir::ExportError> {
        Err(reify_ir::ExportError::FormatError("stub".into()))
    }
    fn tessellate(
        &self,
        _h: reify_ir::GeometryHandleId,
        _t: f64,
    ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
        Err(reify_ir::TessError::TessellationFailed("stub".into()))
    }
}

/// The surface-attribution diagnostics in a check result: ζ's channel, which
/// task 6667 reuses rather than duplicating — `Severity::Info` +
/// [`DiagnosticCode::ConstraintIndeterminate`].
fn attribution_messages(result: &CheckResult) -> Vec<&str> {
    result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Info && d.code == Some(DiagnosticCode::ConstraintIndeterminate)
        })
        .map(|d| d.message.as_str())
        .collect()
}

/// The reason clause of the single attribution naming `id`, with the shared
/// `"constraint {id} indeterminate: "` prefix stripped.
fn sole_reason_for<'a>(result: &'a CheckResult, id: &str) -> &'a str {
    let matching: Vec<&str> = attribution_messages(result)
        .into_iter()
        .filter(|m| m.contains(id))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one surface attribution naming {id}; got {matching:#?}"
    );
    let prefix = format!("constraint {id} indeterminate: ");
    matching[0]
        .strip_prefix(prefix.as_str())
        .unwrap_or_else(|| {
            panic!(
                "the attribution must keep ζ's `constraint {{id}} indeterminate: {{reason}}` \
                 grammar so `labeled_diagnostics` can substitute a user-facing label; got: {}",
                matching[0]
            )
        })
}

// ── (a) Over-reach guard: a MEASURING surface gains nothing ──────────────────

/// GREEN ON ARRIVAL. The direct analogue of ζ's
/// `measuring_surface_indeterminate_carries_no_attribution_diagnostic`, and the
/// one test that would catch a step-2 gate keyed on the wrong engine field.
///
/// `build()` populates `realization_handles`, so this run demonstrably DID
/// realize. The `Probe` callout is still Indeterminate — its own operands do
/// not resolve to live handles — but that is a different fact needing a
/// different (existing) answer, and attributing it to "the surface does not
/// measure" would be a false statement of exactly the INV-SF-4 kind this work
/// removes. It would also change `reify check` output, which runs
/// `realize_for_check` before `check()` and therefore always reaches this pass
/// with a populated map.
#[test]
fn measuring_surface_gdt_indeterminate_carries_no_surface_attribution() {
    let compiled = parse_and_compile_with_stdlib(MEASURING_SURFACE_GDT_SOURCE);
    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    );

    // Populate `realization_handles` — the build surface, not the GUI's.
    let _ = engine.build(&compiled, ExportFormat::Step);

    // Asserted UP FRONT so this test cannot silently degrade into a second copy
    // of the empty-handle case it exists to distinguish itself from.
    let handles = engine.realization_handles_len();
    assert!(
        handles > 0,
        "premise: build() must have realized something, otherwise this is the \
         NON-measuring surface and the assertion below is vacuous; got len={handles}"
    );

    let result = engine.check(&compiled);

    let probe = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Probe" && e.id.index == 0)
        .expect("must have Probe#constraint[0] (the geometric Conforms)");
    assert_eq!(
        probe.satisfaction,
        Satisfaction::Indeterminate,
        "premise: this callout must still be Indeterminate on the measuring \
         surface — if it resolved to a verdict, the fixture no longer exercises \
         the shape this guard is about"
    );

    let attributions = attribution_messages(&result);
    assert!(
        attributions.is_empty(),
        "the surface attribution must NOT reach a surface that DID realize. \
         This one realized; the callout's own operands simply do not resolve, \
         which is a different fact with a different existing answer. Attaching \
         it here would also change `reify check` output. Got: {attributions:#?}"
    );
}

// ── (b) Refactor guard: ζ's three strings are byte-identical ─────────────────

/// GREEN ON ARRIVAL. Task 6667 generalized `Engine::unmeasured_reason` into
/// `unmeasured_reason_for(kind)`; this asserts the generalization was a pure
/// delegation and moved not one byte of ζ's answers.
///
/// `unmeasured_reason_for` is private, so all three arms are driven through the
/// public `check()` seam exactly as `representation_within_assertion.rs` does.
/// The arm-SELECTION coverage (which engine state picks which arm, and why the
/// ordering is what it is) belongs to ζ's own 22 pins and is deliberately not
/// duplicated here; what only THIS test can catch is a reworded clause, which
/// token matching would sail straight past. Hence the full-sentence `==` — the
/// one deliberate exception to the token-only pinning contract.
#[test]
fn zeta_representation_within_reasons_are_byte_identical_after_the_refactor() {
    const ID: &str = "Checker#constraint[0]";
    let compiled = parse_and_compile(ZETA_SOURCE);

    // Arm 1 — no capability. `Engine::new(checker, None)` leaves
    // `geometry_kernels` empty, which is the one shape that yields `false`
    // without a recorded capability.
    let mut arm1 = Engine::new(Box::new(SimpleConstraintChecker), None);
    let r1 = arm1.check(&compiled);
    assert_eq!(
        sole_reason_for(&r1, ID),
        ZETA_ARM_1_NO_CAPABLE_KERNEL,
        "ζ arm 1 must be byte-identical after the task-6667 refactor"
    );

    // Arm 2 — capable kernel (an UNRECORDED adapter gets the benefit of the
    // doubt, per `has_repr_capable_kernel`'s documented rule), capture OFF.
    let mut arm2 = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(NoRealizationStubKernel)),
    );
    let r2 = arm2.check(&compiled);
    assert_eq!(
        sole_reason_for(&r2, ID),
        ZETA_ARM_2_CAPTURE_OFF,
        "ζ arm 2 must be byte-identical after the task-6667 refactor"
    );

    // Arm 3 — capable kernel, capture ON, still nothing measured: the subject
    // declares no realization, so the map stays empty for a reason that is
    // neither the kernel's nor the surface's.
    let mut arm3 = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(NoRealizationStubKernel)),
    );
    arm3.set_capture_repr_tol(true);
    let r3 = arm3.check(&compiled);
    assert_eq!(
        sole_reason_for(&r3, ID),
        ZETA_ARM_3_NOTHING_MEASURED,
        "ζ arm 3 must be byte-identical after the task-6667 refactor"
    );

    // The three arms must remain DISTINCT — a delegation bug that collapsed the
    // match to one arm would satisfy each `==` above only if the fixtures
    // happened to agree, so state the discrimination itself.
    assert_ne!(ZETA_ARM_1_NO_CAPABLE_KERNEL, ZETA_ARM_2_CAPTURE_OFF);
    assert_ne!(ZETA_ARM_2_CAPTURE_OFF, ZETA_ARM_3_NOTHING_MEASURED);
}

// ── (c) Hot-path guard: an unrelated module gains nothing ────────────────────

/// GREEN ON ARRIVAL. The no-op guard mirroring ζ's
/// `non_assertion_module_hot_path_is_unchanged`.
///
/// No constraint here carries an `actual` binding, so
/// `measure_gdt_conformance`'s fast no-op early-return must fire before any of
/// task 6667's machinery is reached. A module that declares none of the
/// affected kinds must be byte-identical to its pre-6667 self — the emission is
/// for surfaces that could not measure something the user ASKED for, never a
/// blanket annotation on every Indeterminate.
#[test]
fn module_without_geometric_conforms_gains_no_diagnostic() {
    let compiled = parse_and_compile(NO_GEOMETRIC_CONFORMS_SOURCE);
    // The most degraded surface available — no kernel at all. If anything can
    // provoke a spurious attribution, it is this one.
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);

    let result = engine.check(&compiled);

    let attributions = attribution_messages(&result);
    assert!(
        attributions.is_empty(),
        "a module with no geometric Conforms and no DFM rule must gain nothing: \
         `measure_gdt_conformance`'s no-op early-return must fire. Got: {attributions:#?}"
    );
}

// ── DFM fixtures (task 6667, step-6) ─────────────────────────────────────────

/// The `DFMRule` conformer and the process it applies to, shared by the DFM
/// fixtures below.  Copied from
/// `crates/reify-cli/tests/fixtures/dfm_with_repr_within.ri`.
const DFM_RULE_DECLS: &str = r#"
import std.process

structure def FDM : Adding {
    param duration           : Time   = 60min
    param cost               : Money  = 5USD
    param layer_thickness    : Length = 0.2mm
    param min_feature_size   : Length = 0.4mm
    param build_volume       : Solid  = box(200mm, 200mm, 200mm)
    param max_overhang_angle : Angle  = 0deg
}

structure def OverhangRule : DFMRule {
    param rule_name  : String      = "overhang-check"
    param severity   : DFMSeverity = DFMSeverity.Warning
    param applies_to : Process     = FDM()
    param subject    : Solid       = box(50mm, 30mm, 20mm)
}
"#;

/// Geometry, but no `DFMRule` conformer: `Widget` carries no `severity` and no
/// `applies_to`, so `dfm_rule_spec` must reject it at its first two gates.
///
/// Doubles as the realization source for
/// [`measuring_surface_dfm_emits_no_surface_attribution`], which needs a
/// geometry-bearing module that carries no rule of its own.
const NO_DFM_RULE_SOURCE: &str = r#"
structure def Widget {
    param width : Length = 10mm
    param geometry : Solid = box(width, 20mm, 30mm)
}
"#;

/// The same rule reachable from BOTH discovery sources: source (A) finds the
/// top-level `OverhangRule` template, and source (B) finds the synthetic
/// `Value::StructureInstance` the evaluator emits at `Part.rule`.
fn both_discovery_sources_source() -> String {
    format!(
        "{DFM_RULE_DECLS}\n\
         structure def Part {{\n\
         \x20   let rule = OverhangRule()\n\
         }}\n"
    )
}

/// Both affected kinds in one module, on one non-realizing surface.
fn gdt_and_dfm_source() -> String {
    format!(
        "{DFM_RULE_DECLS}\n\
         structure def Probe {{\n\
         \x20   param tol : Flatness = Flatness(tolerance_value: 0.1mm, feature: box(1mm, 1mm, 1mm))\n\
         \x20   param act : Geometry = box(1mm, 1mm, 1mm)\n\
         \x20   constraint Conforms(tolerance: tol, measured_deviation: 0mm, feature_departure: 0mm, actual: act)\n\
         }}\n"
    )
}

/// A non-realizing engine that nonetheless HAS a registered kernel.
///
/// Load-bearing for every DFM test here: `measure_dfm_rules` early-returns on
/// `default_kernel_name.is_none()`, and task 6667 deliberately left that guard
/// in place, so `Engine::new(checker, None)` would produce silence for reasons
/// that have nothing to do with the attribution under test.
fn non_realizing_engine_with_a_kernel() -> Engine {
    Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(NoRealizationStubKernel)),
    )
}

// ── (a) Over-reach guard: a MEASURING surface gains no DFM attribution ───────

/// GREEN ON ARRIVAL. The DFM counterpart of
/// [`measuring_surface_gdt_indeterminate_carries_no_surface_attribution`], and
/// the test that would catch a step-5 gate keyed on the wrong engine field.
///
/// # Why two modules, and why the obvious one-module shape is VACUOUS
///
/// The naive version — build the DFM module itself, then check it — proves
/// nothing about the gate, and this was MEASURED rather than assumed: with the
/// gate forced permanently open, that version still passed. The reason is
/// `measure_dfm_rules` source (A), which injects a live handle into the
/// synthesized `subject` field for every NAMED REALIZATION of the template.
/// `param subject : Solid = box(...)` is exactly such a realization, so after a
/// build the rule is measurable, never lands in the skipped set, and the gate is
/// never consulted. A test that cannot fail is worse than no test.
///
/// So the engine here is made MEASURING by a DIFFERENT module: `build()` on a
/// geometry-only module populates `realization_handles` (and `check()` does not
/// clear it), then the DFM module is checked against that same engine without a
/// build of its own. Its rule is therefore DISCOVERED but SKIPPED — its
/// realization ids are not in the map — while the map is non-empty. Only the
/// gate stands between that state and a spurious attribution.
///
/// That is the precise analogue of what ζ's own guard constructs: a non-empty
/// `achieved_repr_tol` that holds no entry for the subject under test. ζ reaches
/// it through the `set_achieved_repr_tol_for_test` seam; `realization_handles`
/// has no such setter, so the state is reached through the public build/check
/// API instead.
#[test]
fn measuring_surface_dfm_emits_no_surface_attribution() {
    let dfm_module = parse_and_compile_with_stdlib(DFM_RULE_DECLS);
    let geometry_module = parse_and_compile_with_stdlib(NO_DFM_RULE_SOURCE);
    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    );

    // Populate `realization_handles` from a module that is NOT the one under
    // test, so the map is non-empty while holding no entry for the rule.
    let _ = engine.build(&geometry_module, ExportFormat::Step);

    // Asserted UP FRONT so this test cannot silently degrade into the empty case.
    let handles = engine.realization_handles_len();
    assert!(
        handles > 0,
        "premise: build() must have realized something, otherwise this is the \
         NON-measuring surface and the assertion below is vacuous; got len={handles}"
    );

    let result = engine.check(&dfm_module);

    let attributions = attribution_messages(&result);
    assert!(
        attributions.is_empty(),
        "the surface attribution must NOT reach an engine that DID realize. \
         This one realized; the rule's own subject simply has no entry in the \
         map, which is a different fact with a different (existing) answer. \
         Attaching it here would change `reify check` output, which runs \
         realize_for_check before check(). Got: {attributions:#?}"
    );
}

// ── (b) Recognition guard: a module with no DFMRule gains nothing ────────────

/// GREEN ON ARRIVAL. `Widget` carries a `subject`-shaped geometry cell but
/// neither `severity` nor `applies_to`, so `dfm_rule_spec` must reject it at
/// its first two gates and the new emission must never fire.
///
/// Run on the most provocative surface available for this kind: a registered
/// kernel (so `measure_dfm_rules` does NOT take its `default_kernel_name`
/// early return) that realizes nothing (so the attribution gate is OPEN). If
/// discovery over-matched, this is where it would show.
#[test]
fn module_without_dfm_rules_gains_no_diagnostic() {
    let compiled = parse_and_compile_with_stdlib(NO_DFM_RULE_SOURCE);
    let mut engine = non_realizing_engine_with_a_kernel();

    let result = engine.check(&compiled);

    assert_eq!(
        engine.realization_handles_len(),
        0,
        "premise: the attribution gate must be OPEN, so a spurious emission \
         would actually be observable here"
    );
    let attributions = attribution_messages(&result);
    assert!(
        attributions.is_empty(),
        "a module with no DFMRule conformer must gain nothing — dfm_rule_spec \
         rejects it at the severity/applies_to gates. Got: {attributions:#?}"
    );
}

// ── (c) Dedupe guard: one rule, two discovery sources, one attribution ───────

/// GREEN ON ARRIVAL. The dedupe pin for step-5(4).
///
/// `OverhangRule` is reachable twice: source (A) synthesizes a
/// `StructureInstance` from the top-level template's value cells, and source
/// (B) finds the synthetic instance the evaluator emits at `Part.rule`. The
/// measurement path's own `(kind, subject_handle)` dedupe is useless here —
/// every skipped rule has `subject_handle: None`, so that key collapses to
/// `(kind, None)` — which is exactly why step-5 dedupes the attribution by
/// `rule_name` instead.
///
/// Counted, not `find`-ed: a `find` would pass on the first match and never see
/// the duplicate this test exists to forbid.
#[test]
fn one_dfm_rule_discovered_from_both_sources_is_attributed_once() {
    let compiled = parse_and_compile_with_stdlib(&both_discovery_sources_source());
    let mut engine = non_realizing_engine_with_a_kernel();

    let result = engine.check(&compiled);

    let attributions = attribution_messages(&result);
    assert_eq!(
        attributions.len(),
        1,
        "one declared rule ⇒ exactly one attribution, however many discovery \
         sources reach it. Got: {attributions:#?}"
    );
    assert!(
        attributions[0].contains("overhang-check"),
        "the surviving attribution must name the rule by its `rule_name`; got: {}",
        attributions[0]
    );
}

// ── (d) Cross-kind guard: two kinds, two attributions, no collision ──────────

/// GREEN ON ARRIVAL. The analogue of ζ's
/// `non_measuring_mixed_batch_preserves_order_and_attributes_once`.
///
/// One module carrying BOTH affected kinds on ONE non-realizing surface must
/// yield exactly two attributions with DISTINCT subjects and DISTINCT kind
/// nouns. Two emissions sharing one gate and one diagnostic code is precisely
/// the shape where a collision or a double-count would hide, and the kind noun
/// is what stops a user reading two different unmeasured things as one repeated
/// sentence.
#[test]
fn gdt_and_dfm_in_one_module_each_get_their_own_attribution() {
    let compiled = parse_and_compile_with_stdlib(&gdt_and_dfm_source());
    let mut engine = non_realizing_engine_with_a_kernel();

    let result = engine.check(&compiled);

    let attributions = attribution_messages(&result);
    assert_eq!(
        attributions.len(),
        2,
        "one geometric Conforms + one DFM rule ⇒ exactly two attributions. \
         Got: {attributions:#?}"
    );

    let gdt: Vec<&&str> = attributions
        .iter()
        .filter(|m| m.contains("geometric conformance"))
        .collect();
    let dfm: Vec<&&str> = attributions
        .iter()
        .filter(|m| m.contains("does not measure DFM rules"))
        .collect();
    assert_eq!(
        gdt.len(),
        1,
        "exactly one attribution must name the GD&T kind. Got: {attributions:#?}"
    );
    assert_eq!(
        dfm.len(),
        1,
        "exactly one attribution must name the DFM kind. Got: {attributions:#?}"
    );

    // Distinct SUBJECTS, not merely distinct kind nouns: the Conforms is named
    // by its ConstraintNodeId, the rule by its `rule_name`.
    assert!(
        gdt[0].contains("Probe#constraint[0]"),
        "the GD&T attribution must name the constraint node; got: {}",
        gdt[0]
    );
    assert!(
        dfm[0].contains("overhang-check"),
        "the DFM attribution must name the rule; got: {}",
        dfm[0]
    );
}
