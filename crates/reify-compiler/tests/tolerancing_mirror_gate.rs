//! Mirror-reintroduction gate for the m8_tolerancing lineage (task #5582).
//!
//! Five example files used to carry eval-time copies of stdlib tolerancing
//! types. Task #5582 stripped them: every one of those types reaches eval
//! through the prelude (Regime B — docs/prds/v0_6/resolution-unification.md §1,
//! `find_template_with_prelude`), so a local copy is not a harmless duplicate.
//! It shadows the stdlib type's trait bounds (dropping `nominal_zone` /
//! `zone_shape`), becomes a standalone module root of its own, and is free to
//! drift — io_export.ri's `Flatness` mirror still declared
//! `param feature : Real = 0.0` years after stdlib tightened it to
//! `feature : Geometry` (#3116).
//!
//! Three of the five files re-declared only `enum MaterialCondition`, which was
//! byte-identical to stdlib's. There is therefore NO eval-level drift signal for
//! them — a value assertion could not tell the mirror from the real thing. The
//! durable signal is structural-but-behavioural, and is what this file gates:
//! **the compiled module for each lineage file must declare no name that
//! stdlib/tolerancing.ri owns.**
//!
//! ## Scope: deliberately NOT corpus-wide
//!
//! This gate covers (these five files) × (names declared by
//! crates/reify-compiler/stdlib/tolerancing.ri). It is not a general
//! "no example re-declares a stdlib name" rule, and must not be broadened
//! casually. That was measured before being rejected: 44 collisions across 25
//! example files, the large majority legitimate local pedagogy — e.g.
//! m9_trait_conformance.ri defines its own `Part`/`Physical`,
//! m6_generic_enum.ri its own `Result`, keyed_vents.ri a `Manifold`. A
//! corpus-wide gate would need a 20+ entry allow-list; building one is separate,
//! allow-list-bearing work.
//!
//! ## Derived, never transcribed
//!
//! The stdlib-owned name set is computed at test runtime by compiling
//! stdlib/tolerancing.ri, not written down as a literal. When stdlib grows a new
//! tolerancing type the gate covers it automatically instead of silently going
//! stale. Following examples_smoke.rs conventions, every offending name is
//! accumulated and reported in one panic rather than failing on the first.

use reify_compiler::{compile_with_stdlib, parse_with_stdlib};
use reify_core::ModulePath;
use std::collections::BTreeSet;

/// The stdlib module whose declared names the lineage files must not shadow.
const STDLIB_TOLERANCING: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/stdlib/tolerancing.ri"
));

const EXAMPLES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");

/// Every template + enum name declared by `source`, compiled as a module named
/// `module_name`.
///
/// A compiled-artifact read, not a source grep: it sees exactly the names the
/// user module contributes to `compiled.templates` / `compiled.enum_defs` —
/// which is precisely what the eval engine and the η pass resolve against.
fn declared_names(source: &str, module_name: &str) -> BTreeSet<String> {
    let parsed = parse_with_stdlib(source, ModulePath::single(module_name));
    assert!(
        parsed.errors.is_empty(),
        "{module_name} should parse cleanly, got: {:?}",
        parsed.errors
    );
    let compiled = compile_with_stdlib(&parsed);
    compiled
        .templates
        .iter()
        .map(|t| t.name.clone())
        .chain(compiled.enum_defs.iter().map(|e| e.name.clone()))
        .collect()
}

/// The names `crates/reify-compiler/stdlib/tolerancing.ri` owns.
fn stdlib_tolerancing_names() -> BTreeSet<String> {
    let names = declared_names(STDLIB_TOLERANCING, "tolerancing");
    // Sanity floor: if this ever comes back empty the gate would pass
    // vacuously for every file, which is the one failure mode a
    // derive-don't-transcribe set can have.
    assert!(
        names.contains("Flatness") && names.contains("MaterialCondition"),
        "stdlib/tolerancing.ri should declare at least Flatness and MaterialCondition; \
         got {names:?} — the derivation is broken, so the gate below is vacuous"
    );
    names
}

/// Names declared by `examples/<rel_path>`.
fn example_declared_names(rel_path: &str) -> BTreeSet<String> {
    let path = format!("{EXAMPLES_DIR}/{rel_path}");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{path} should exist and be readable: {e}"));
    let stem = rel_path
        .rsplit('/')
        .next()
        .unwrap_or(rel_path)
        .trim_end_matches(".ri");
    declared_names(&source, stem)
}

/// Assert `examples/<rel_path>` declares nothing stdlib/tolerancing.ri owns.
fn assert_no_tolerancing_mirrors(rel_path: &str) {
    let offenders: Vec<String> = example_declared_names(rel_path)
        .intersection(&stdlib_tolerancing_names())
        .cloned()
        .collect();

    assert!(
        offenders.is_empty(),
        "examples/{rel_path} re-declares {} name(s) that \
         crates/reify-compiler/stdlib/tolerancing.ri already owns: {offenders:?}. \
         These reach eval through the prelude (Regime B); a local copy shadows the \
         stdlib type's trait bounds, becomes a standalone module root, and is free to \
         drift out of sync. Consume them from the prelude instead — see task #5582.",
        offenders.len()
    );
}

// ── The five lineage files ───────────────────────────────────────────────────

#[test]
fn m8_tolerancing_declares_no_stdlib_tolerancing_mirrors() {
    assert_no_tolerancing_mirrors("m8_tolerancing.ri");
}

#[test]
fn io_export_declares_no_stdlib_tolerancing_mirrors() {
    assert_no_tolerancing_mirrors("io_export.ri");
}

#[test]
fn gdt_conformance_satisfied_declares_no_stdlib_tolerancing_mirrors() {
    assert_no_tolerancing_mirrors("gdt_conformance_satisfied.ri");
}

#[test]
fn gdt_conformance_violated_declares_no_stdlib_tolerancing_mirrors() {
    assert_no_tolerancing_mirrors("gdt_conformance_violated.ri");
}

#[test]
fn gdt_pass_weave_declares_no_stdlib_tolerancing_mirrors() {
    assert_no_tolerancing_mirrors("tolerancing/gdt_pass_weave.ri");
}

// ── The genuinely-local callout types must SURVIVE ───────────────────────────

/// `FlatnessCallout`, `WeaveScalarTol` and `WeaveFlatnessCallout` have NO stdlib
/// counterpart and are load-bearing: each carries an explicit
/// `: GeometricTolerance` bound, which is what the η geometric-conformance
/// pass's `satisfies_trait_bound` check keys on when it resolves a `Conforms`
/// callout's type against `module.templates` (PRD v0_6 C5).
///
/// Deleting them as "more mirrors" must be a test failure, not a silent
/// cleanup — this test is the counterweight to the five above.
#[test]
fn local_geometric_tolerance_callout_types_survive() {
    let expected: &[(&str, &[&str])] = &[
        ("gdt_conformance_satisfied.ri", &["FlatnessCallout"]),
        ("gdt_conformance_violated.ri", &["FlatnessCallout"]),
        (
            "tolerancing/gdt_pass_weave.ri",
            &["WeaveScalarTol", "WeaveFlatnessCallout"],
        ),
    ];

    let mut missing: Vec<String> = Vec::new();
    for (rel_path, required) in expected {
        let declared = example_declared_names(rel_path);
        for name in *required {
            if !declared.contains(*name) {
                missing.push(format!("examples/{rel_path} :: {name}"));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "these local `: GeometricTolerance` callout types have gone missing: {missing:?}. \
         They have no stdlib counterpart and are NOT mirrors — the η pass resolves a \
         `Conforms` callout's type against module.templates and checks the \
         GeometricTolerance bound there (PRD v0_6 C5), so removing them breaks the \
         conformance examples. If a stdlib type genuinely subsumes one, update this test \
         deliberately."
    );
}
