//! Phase A tests for `auto` type-parameter resolution candidate enumeration.
//!
//! Targets `crates/reify-compiler/src/auto_type_param.rs`'s public
//! `enumerate_candidates` function and its three-arm result enum
//! [`CandidateEnumeration`]. The PRD that drives this work is
//! `docs/prds/auto-type-param-resolution.md` and language spec §3.9.
//!
//! Phase A only covers candidate enumeration: walking the in-scope name
//! table at the use site and collecting every concrete structure whose
//! declared trait bounds satisfy a required trait bound, capped at 10.
//! Phases B (per-candidate feasibility), C (selection), and D (topology
//! trigger) are out of scope here and live in follow-up tasks.

use std::collections::HashMap;

use reify_compiler::auto_type_param::{
    CandidateEnumeration, MAX_AUTO_TYPE_PARAM_CANDIDATES, enumerate_candidates,
};
use reify_compiler::{CompiledModule, CompiledTrait, TopologyTemplate};
use reify_test_support::compile_source;
use reify_types::SourceSpan;

/// Build the `(template_registry, trait_registry)` pair that
/// `enumerate_candidates` consumes, borrowing from a single compiled module.
///
/// Mirrors the construction shape used internally by
/// `compile_builder::entities_phase::phase_pending_bound_checks` (lines
/// 246-253 in entities_phase.rs).
fn build_registries(
    module: &CompiledModule,
) -> (
    HashMap<String, &TopologyTemplate>,
    HashMap<String, &CompiledTrait>,
) {
    let template_registry: HashMap<String, &TopologyTemplate> = module
        .templates
        .iter()
        .map(|t| (t.name.clone(), t))
        .collect();
    let trait_registry: HashMap<String, &CompiledTrait> = module
        .trait_defs
        .iter()
        .map(|t| (t.name.clone(), t))
        .collect();
    (template_registry, trait_registry)
}

// ─── step-1: empty result when no template satisfies the bound ────────────

/// When the only structure in scope does not declare conformance to the
/// required trait, `enumerate_candidates` returns
/// [`CandidateEnumeration::Empty`] without pushing any diagnostic. The
/// selection phase (future task) is responsible for emitting
/// `E_AUTO_TYPE_PARAM_NO_CANDIDATE` on the empty path; Phase A is silent.
#[test]
fn enumerate_returns_empty_when_no_template_satisfies_bound() {
    let source = r#"
trait Seal {}

structure def Bracket {
    param x : Real = 1.0
}
"#;
    let module = compile_source(source);
    let (template_registry, trait_registry) = build_registries(&module);

    // Sanity check the fixture: the template exists but does NOT declare Seal.
    let bracket = template_registry
        .get("Bracket")
        .expect("expected 'Bracket' template in compiled module");
    assert!(
        !bracket.trait_bounds.contains(&"Seal".to_string()),
        "Bracket should not declare Seal conformance, got: {:?}",
        bracket.trait_bounds
    );

    let mut diagnostics = Vec::new();
    let result = enumerate_candidates(
        &["Seal".to_string()],
        &template_registry,
        &trait_registry,
        SourceSpan::empty(0),
        &mut diagnostics,
    );

    assert!(
        matches!(result, CandidateEnumeration::Empty),
        "expected CandidateEnumeration::Empty, got: {:?}",
        result
    );
    assert!(
        diagnostics.is_empty(),
        "Empty path should emit NO diagnostics (selection phase handles E_AUTO_TYPE_PARAM_NO_CANDIDATE), got: {:?}",
        diagnostics
    );

    // Cap is exposed at 10 (sanity-check the public constant).
    assert_eq!(MAX_AUTO_TYPE_PARAM_CANDIDATES, 10);
}

// ─── step-3: single-candidate result ──────────────────────────────────────

/// When exactly one in-scope structure declares conformance to the
/// required trait, the result is `Found(vec!["ORingSeal"])` and no
/// diagnostic is emitted. Pins both the variant choice and the exact
/// contents of the candidate Vec.
#[test]
fn enumerate_returns_found_with_single_candidate_for_trait_bound() {
    let source = r#"
trait Seal {}

structure def ORingSeal : Seal {
    param diameter : Real = 10.0
}
"#;
    let module = compile_source(source);
    let (template_registry, trait_registry) = build_registries(&module);

    let mut diagnostics = Vec::new();
    let result = enumerate_candidates(
        &["Seal".to_string()],
        &template_registry,
        &trait_registry,
        SourceSpan::empty(0),
        &mut diagnostics,
    );

    assert_eq!(
        result,
        CandidateEnumeration::Found(vec!["ORingSeal".to_string()]),
        "expected exactly one candidate 'ORingSeal'"
    );
    assert!(
        diagnostics.is_empty(),
        "Found path with no overflow should emit no diagnostics, got: {:?}",
        diagnostics
    );
}

// ─── step-5: multi-candidate alphabetical determinism ─────────────────────

/// When multiple in-scope structures declare conformance to the required
/// trait, the candidate Vec is sorted alphabetically by template name —
/// NOT in source-declaration order. Three structures `Zeta`, `Alpha`,
/// `Mike` are declared in non-alphabetical source order; the result must
/// be `["Alpha", "Mike", "Zeta"]`.
///
/// This pins the determinism guarantee from PRD acceptance criterion 11
/// ("same source produces same resolution choice across runs and across
/// machines"). Asserting `assert_eq!` on the exact ordered Vec makes any
/// regression to source order or to HashMap iteration order fail loudly.
#[test]
fn enumerate_returns_found_sorted_alphabetically_for_multiple_candidates() {
    let source = r#"
trait Seal {}

structure def Zeta : Seal {
    param x : Real = 1.0
}

structure def Alpha : Seal {
    param x : Real = 1.0
}

structure def Mike : Seal {
    param x : Real = 1.0
}
"#;
    let module = compile_source(source);
    let (template_registry, trait_registry) = build_registries(&module);

    let mut diagnostics = Vec::new();
    let result = enumerate_candidates(
        &["Seal".to_string()],
        &template_registry,
        &trait_registry,
        SourceSpan::empty(0),
        &mut diagnostics,
    );

    assert_eq!(
        result,
        CandidateEnumeration::Found(vec![
            "Alpha".to_string(),
            "Mike".to_string(),
            "Zeta".to_string(),
        ]),
        "expected alphabetical order ['Alpha','Mike','Zeta'], NOT source order"
    );
    assert!(
        diagnostics.is_empty(),
        "Found path with no overflow should emit no diagnostics, got: {:?}",
        diagnostics
    );
}
