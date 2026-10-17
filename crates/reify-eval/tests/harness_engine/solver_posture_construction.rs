//! Every `Engine` constructor takes a leading [`SolverPosture`], and the engine
//! it builds holds exactly that posture (task #6689; PRD
//! `docs/prds/v0_6/solver-driver-parity.md` §4.1 I1, §4.2 I2).
//!
//! `PROBE_SRC` separates the three possible solver wirings in one eval: the
//! production registry resolves `a` to the centre of `[8mm, 12mm]` and leaves
//! the unconstrained `orphan` unresolved with only a warning; a bare
//! `DimensionalSolver` leaves both unresolved and emits a strict-resolution
//! error; no solver leaves both unresolved.
//!
//! The guards that keep a named posture true are pinned too: a `SolverFree`
//! engine refuses a solver, and a `Fresh` profile refuses a persistent cache.

use std::collections::{BTreeMap, HashMap};

use reify_core::{Diagnostic, Severity, ValueCellId};
use reify_eval::{Engine, SolverPosture};
use reify_ir::{ConstraintChecker, IterationBudget, ResolutionProfile, StalenessPolicy, Value};
use reify_test_support::{
    MockConstraintChecker, MockConstraintSolver, collect_errors, compile_source_with_stdlib, mm,
    prefixed_tempdir,
};

const PROBE_SRC: &str = r#"module solver_posture_probe

pub structure Probe {
    param a : Length = auto
    param orphan : Length = auto
    constraint a >= 8mm
    constraint a <= 12mm
}
"#;

/// The Chebyshev centre of `8mm <= a <= 12mm`, in metres.
const CENTRE_SI: f64 = 0.01;
const SOLVER_TOL: f64 = 1e-9;

const SOLVE_FRESH: SolverPosture = SolverPosture::Solve(ResolutionProfile::FRESH);

fn probe_a() -> ValueCellId {
    ValueCellId::new("Probe", "a")
}

fn probe_orphan() -> ValueCellId {
    ValueCellId::new("Probe", "orphan")
}

fn checker() -> Box<dyn ConstraintChecker> {
    Box::new(MockConstraintChecker::new())
}

struct ProbeOutcome {
    a: Value,
    orphan: Value,
    errors: Vec<Diagnostic>,
}

fn eval_probe(engine: &mut Engine) -> ProbeOutcome {
    let compiled = compile_source_with_stdlib(PROBE_SRC);
    let compile_errors = collect_errors(&compiled.diagnostics);
    assert!(
        compile_errors.is_empty(),
        "the probe source must compile without errors; got {compile_errors:#?}",
    );
    let result = engine.eval(&compiled);
    let cell = |id: &ValueCellId| {
        result
            .values
            .get(id)
            .cloned()
            .unwrap_or_else(|| panic!("no value cell {id:?} exists in the probe eval"))
    };
    ProbeOutcome {
        a: cell(&probe_a()),
        orphan: cell(&probe_orphan()),
        errors: result
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .cloned()
            .collect(),
    }
}

fn is_centre(value: &Value) -> bool {
    matches!(value, Value::Scalar { si_value, .. } if (si_value - CENTRE_SI).abs() <= SOLVER_TOL)
}

#[test]
fn solve_posture_wires_the_production_registry() {
    let mut engine = Engine::new(SOLVE_FRESH, checker(), None);
    let outcome = eval_probe(&mut engine);
    assert!(
        is_centre(&outcome.a),
        "Solve(..) must wire SolverRegistry::production(), which resolves `a` to \
         the centre of [8mm, 12mm] ({CENTRE_SI} m). A bare DimensionalSolver leaves \
         `a` Undef with a strict-resolution error, and no solver leaves it Undef; \
         got {:?}",
        outcome.a,
    );
    assert_eq!(
        outcome.orphan,
        Value::Undef,
        "the unconstrained `orphan` stays unresolved under the production registry",
    );
    assert!(
        outcome.errors.is_empty(),
        "the production registry reports an unconstrained auto as a warning, not an \
         error (a bare DimensionalSolver errors); got {:#?}",
        outcome.errors,
    );
}

#[test]
fn solver_free_posture_resolves_nothing() {
    let mut engine = Engine::new(SolverPosture::SolverFree, checker(), None);
    let outcome = eval_probe(&mut engine);
    assert_eq!(outcome.a, Value::Undef, "SolverFree must not resolve `a`");
    assert_eq!(
        outcome.orphan,
        Value::Undef,
        "SolverFree must not resolve `orphan`"
    );
}

#[test]
fn every_constructor_honours_its_posture() {
    let constructors: [(&str, fn(SolverPosture) -> Engine); 6] = [
        ("new", |p| Engine::new(p, checker(), None)),
        ("with_prelude", |p| {
            Engine::with_prelude(
                p,
                checker(),
                None,
                reify_compiler::stdlib_loader::load_stdlib(),
            )
        }),
        ("with_registered_kernel", |p| {
            Engine::with_registered_kernel(p, checker())
        }),
        ("with_registered_kernels", |p| {
            Engine::with_registered_kernels(p, checker())
        }),
        ("with_registered_kernels_and_manifest", |p| {
            Engine::with_registered_kernels_and_manifest(p, checker(), None).0
        }),
        ("with_test_kernels_and_registry", |p| {
            Engine::with_test_kernels_and_registry(
                p,
                checker(),
                BTreeMap::new(),
                BTreeMap::new(),
                None,
            )
        }),
    ];
    for (name, construct) in constructors {
        for posture in [SOLVE_FRESH, SolverPosture::SolverFree] {
            let mut engine = construct(posture);
            assert_eq!(
                engine.solver_posture(),
                posture,
                "Engine::{name} must hold the posture it was given",
            );
            let outcome = eval_probe(&mut engine);
            assert_eq!(
                is_centre(&outcome.a),
                matches!(posture, SolverPosture::Solve(_)),
                "Engine::{name} under {posture:?}: `a` must resolve to the centre iff \
                 the posture is Solve; got {:?}",
                outcome.a,
            );
        }
    }
}

#[test]
fn with_solver_replaces_the_solver_but_keeps_the_profile() {
    let seven_mm = mm(7.0);
    let double = MockConstraintSolver::new_solved(HashMap::from([(probe_a(), seven_mm.clone())]));
    let mut engine = Engine::new(SOLVE_FRESH, checker(), None).with_solver(Box::new(double));
    assert_eq!(engine.solver_posture(), SOLVE_FRESH);
    let outcome = eval_probe(&mut engine);
    assert_eq!(
        outcome.a, seven_mm,
        "with_solver must route the solve through the injected double, not the \
         production registry (which would give the centre, 10mm)",
    );
}

fn solved_double() -> Box<MockConstraintSolver> {
    Box::new(MockConstraintSolver::new_solved(HashMap::from([(
        probe_a(),
        mm(7.0),
    )])))
}

fn may_serve_persisted() -> SolverPosture {
    SolverPosture::Solve(ResolutionProfile::new(
        IterationBudget::SolverDefault,
        StalenessPolicy::MayServePersisted,
    ))
}

#[test]
#[should_panic(expected = "SolverFree")]
fn with_solver_refuses_a_solver_free_engine() {
    let _ = Engine::new(SolverPosture::SolverFree, checker(), None).with_solver(solved_double());
}

#[test]
#[should_panic(expected = "SolverFree")]
fn register_solver_refuses_a_solver_free_engine() {
    let mut engine = Engine::new(SolverPosture::SolverFree, checker(), None);
    engine.register_solver("libslvs", solved_double());
}

#[test]
#[should_panic(expected = "MayServePersisted")]
fn persistent_cache_refused_under_a_fresh_profile() {
    let guard = prefixed_tempdir("reify-6689-posture-");
    let mut engine = Engine::new(SOLVE_FRESH, checker(), None);
    engine.set_persistent_cache_dir(Some(guard.path().to_path_buf()));
}

#[test]
fn persistent_cache_accepted_when_the_profile_permits_it() {
    let mut engine = Engine::new(may_serve_persisted(), checker(), None);
    let guard = prefixed_tempdir("reify-6689-posture-");
    engine.set_persistent_cache_dir(Some(guard.path().to_path_buf()));
    assert_eq!(engine.persistent_cache_dir(), Some(guard.path()));
}

#[test]
fn persistent_cache_detach_is_always_accepted() {
    for posture in [SOLVE_FRESH, SolverPosture::SolverFree] {
        let mut engine = Engine::new(posture, checker(), None);
        engine.set_persistent_cache_dir(None);
        assert_eq!(engine.persistent_cache_dir(), None);
    }
}

#[test]
fn solver_free_engine_may_carry_a_persistent_cache() {
    let mut engine = Engine::new(SolverPosture::SolverFree, checker(), None);
    let guard = prefixed_tempdir("reify-6689-posture-");
    engine.set_persistent_cache_dir(Some(guard.path().to_path_buf()));
    assert_eq!(engine.persistent_cache_dir(), Some(guard.path()));
}
