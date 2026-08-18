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
//! # Why no connect-param site
//!
//! Deliberately excluded. `AllFourSites.__connector_0.gain` carries a SEPARATE,
//! PRE-EXISTING false positive (by design D5 the parent cannot name
//! `__connector_N`, so no constraint reads it under any spelling — true on
//! `main` too), and folding it in would make this count track two unrelated
//! defects at once. It is covered, and pinned as the expected baseline, by
//! `tests/auto_binding_sites_remaining_resolution.rs`.

use reify_core::DiagnosticCode;
use reify_eval::Engine;
use reify_test_support::{MockConstraintChecker, collect_errors, compile_source_with_stdlib};

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
    let compiled = compile_source_with_stdlib(BOTH_MINTING_SITES);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "the instance-path fixture must compile without errors — this test \
         pins a DIAGNOSTIC COUNT, so a compile error would silently change \
         what is being counted; got {errors:#?}",
    );

    // `Engine::new(.., None)` is the literal `reify check` entry point:
    // `detect_underdetermined` runs OUTSIDE the `has_active_solver` gate, so no
    // amount of solver-side fixing can clear what it emits.
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    let result = engine.eval(&compiled);

    let under: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Underdetermined))
        .collect();

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
