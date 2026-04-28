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
use reify_types::{DiagnosticCode, Severity, SourceSpan};

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

// ─── step-7: trait refinement chain ───────────────────────────────────────

/// A structure declaring conformance to trait `OilSeal` (which refines
/// `Seal`) must satisfy a required bound of `Seal` via the transitive
/// refinement chain. This pins reuse of `entity::satisfies_trait_bound`
/// (which delegates to `trait_satisfies` recursively at
/// `entity.rs:2146-2166`) and prevents future refactors from accidentally
/// bypassing the recursive walk with a flat string-equality check.
#[test]
fn enumerate_includes_candidate_via_trait_refinement_chain() {
    let source = r#"
trait Seal {}
trait OilSeal : Seal {}

structure def NitrileOilSeal : OilSeal {
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
        CandidateEnumeration::Found(vec!["NitrileOilSeal".to_string()]),
        "expected NitrileOilSeal to satisfy Seal via OilSeal refinement chain"
    );
    assert!(
        diagnostics.is_empty(),
        "Found path with refinement should emit no diagnostics, got: {:?}",
        diagnostics
    );
}

// ─── step-9: composite-bound intersection ─────────────────────────────────

/// Composite bounds (`bounds: &["Seal", "Cooled"]`) require a candidate
/// to satisfy ALL bounds (intersection), not any (union). Three
/// structures: `OnlySeal : Seal`, `OnlyCooled : Cooled`, and
/// `Both : Seal + Cooled`. Only `Both` qualifies.
///
/// Pins PRD §"Phase A": "Composite (`T: TraitA + TraitB`): intersection."
/// A regression that introduced `bounds.iter().any(...)` instead of
/// `bounds.iter().all(...)` would cause this test to fail with all
/// three structures appearing in the result.
#[test]
fn enumerate_returns_intersection_for_composite_bound() {
    let source = r#"
trait Seal {}
trait Cooled {}

structure def OnlySeal : Seal {
    param x : Real = 1.0
}

structure def OnlyCooled : Cooled {
    param x : Real = 1.0
}

structure def Both : Seal + Cooled {
    param x : Real = 1.0
}
"#;
    let module = compile_source(source);
    let (template_registry, trait_registry) = build_registries(&module);

    let mut diagnostics = Vec::new();
    let result = enumerate_candidates(
        &["Seal".to_string(), "Cooled".to_string()],
        &template_registry,
        &trait_registry,
        SourceSpan::empty(0),
        &mut diagnostics,
    );

    assert_eq!(
        result,
        CandidateEnumeration::Found(vec!["Both".to_string()]),
        "composite bound 'Seal + Cooled' must intersect — only `Both` qualifies"
    );
    assert!(
        diagnostics.is_empty(),
        "Found path should emit no diagnostics, got: {:?}",
        diagnostics
    );
}

// ─── step-11: exactly 10 candidates (boundary, no overflow) ───────────────

/// Build a Reify source defining `trait Seal {}` and `count` structures
/// `S00`..`S{count-1}` each declaring `: Seal`. Candidate names are
/// zero-padded to two digits so alphabetical ordering matches numeric
/// ordering up to S99.
fn build_n_seal_structures(count: usize) -> String {
    assert!(
        count <= 100,
        "build_n_seal_structures: zero-pad width is two digits; max count is 100"
    );
    let mut src = String::from("trait Seal {}\n");
    for i in 0..count {
        src.push_str(&format!(
            "structure def S{:02} : Seal {{\n    param x : Real = 1.0\n}}\n",
            i
        ));
    }
    src
}

/// Boundary semantics: 10 candidates is "ok" (no overflow), 11 is
/// "overflow" (step-13). With exactly 10 structures `S00..S09`
/// implementing `Seal`, the result is `Found([S00, .., S09])` and zero
/// diagnostics.
#[test]
fn enumerate_returns_found_at_exactly_max_candidates_no_overflow() {
    let source = build_n_seal_structures(MAX_AUTO_TYPE_PARAM_CANDIDATES);
    let module = compile_source(&source);
    let (template_registry, trait_registry) = build_registries(&module);

    let mut diagnostics = Vec::new();
    let result = enumerate_candidates(
        &["Seal".to_string()],
        &template_registry,
        &trait_registry,
        SourceSpan::empty(0),
        &mut diagnostics,
    );

    let expected: Vec<String> = (0..MAX_AUTO_TYPE_PARAM_CANDIDATES)
        .map(|i| format!("S{:02}", i))
        .collect();
    assert_eq!(
        result,
        CandidateEnumeration::Found(expected),
        "exactly {MAX_AUTO_TYPE_PARAM_CANDIDATES} candidates must be Found (boundary; not Overflow)"
    );
    assert!(
        diagnostics.is_empty(),
        "boundary case (exactly MAX) must emit no diagnostics, got: {:?}",
        diagnostics
    );
}

// ─── step-13: overflow at MAX+1 — full diagnostic shape ───────────────────

/// With exactly 11 structures `S00..S10` implementing `Seal`, the result
/// is `Overflow([S00..S09])` (alphabetical first 10, NOT `S10`). One
/// diagnostic is pushed onto the supplied vector with:
///   - severity: Error
///   - code: AutoTypeParamPoolOverflow
///   - candidates: ["S00", .., "S09"] (machine-readable)
///   - labels[0].span: the use-site span passed in
///   - message: contains the bound name "Seal"
///
/// This is the single canonical pin of the overflow contract end-to-end.
#[test]
fn enumerate_overflows_at_eleven_candidates_emits_diagnostic_with_first_ten() {
    let source = build_n_seal_structures(MAX_AUTO_TYPE_PARAM_CANDIDATES + 1);
    let module = compile_source(&source);
    let (template_registry, trait_registry) = build_registries(&module);

    let use_site_span = SourceSpan::new(100, 110);
    let mut diagnostics = Vec::new();
    let result = enumerate_candidates(
        &["Seal".to_string()],
        &template_registry,
        &trait_registry,
        use_site_span,
        &mut diagnostics,
    );

    let expected_first_ten: Vec<String> = (0..MAX_AUTO_TYPE_PARAM_CANDIDATES)
        .map(|i| format!("S{:02}", i))
        .collect();
    assert_eq!(
        result,
        CandidateEnumeration::Overflow(expected_first_ten.clone()),
        "MAX+1 candidates must produce Overflow with first MAX alphabetically (excluding S{:02})",
        MAX_AUTO_TYPE_PARAM_CANDIDATES
    );

    // Exactly one diagnostic — not one per excess candidate.
    assert_eq!(
        diagnostics.len(),
        1,
        "exactly one overflow diagnostic expected, got {}",
        diagnostics.len()
    );
    let d = &diagnostics[0];
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(d.code, Some(DiagnosticCode::AutoTypeParamPoolOverflow));
    assert_eq!(
        d.candidates, expected_first_ten,
        "diagnostic.candidates must contain the first 10 alphabetically"
    );
    assert!(
        !d.labels.is_empty(),
        "diagnostic must carry at least one label at the use-site span"
    );
    assert_eq!(
        d.labels[0].span, use_site_span,
        "primary label must point at the supplied use-site span"
    );
    assert!(
        d.message.contains("Seal"),
        "diagnostic message must mention the bound 'Seal', got: {:?}",
        d.message
    );
}

// ─── step-15: overflow with many extra candidates still terminates ────────

/// With 25 structures `S00..S24` implementing `Seal`, the result is
/// still `Overflow(["S00".."S09"])` — exactly 10 entries, alphabetically
/// the first 10 of the entire pool — and exactly one diagnostic is
/// emitted (not one per excess candidate).
///
/// This pins the early-termination correctness from step-14: even with
/// many extra matches, the diagnostic content is exactly the first 10
/// alphabetically because of sorted iteration, and the iteration breaks
/// after the 11th match.
#[test]
fn enumerate_overflow_with_many_candidates_still_terminates_at_eleven() {
    let source = build_n_seal_structures(25);
    let module = compile_source(&source);
    let (template_registry, trait_registry) = build_registries(&module);

    let mut diagnostics = Vec::new();
    let result = enumerate_candidates(
        &["Seal".to_string()],
        &template_registry,
        &trait_registry,
        SourceSpan::empty(0),
        &mut diagnostics,
    );

    let expected_first_ten: Vec<String> = (0..MAX_AUTO_TYPE_PARAM_CANDIDATES)
        .map(|i| format!("S{:02}", i))
        .collect();
    assert_eq!(
        result,
        CandidateEnumeration::Overflow(expected_first_ten),
        "with 25 candidates, Overflow vector must contain exactly S00..S09 (NOT S15/S20/etc.)"
    );
    // Exactly one diagnostic — emitting one per excess candidate would
    // flood the diagnostic stream and is explicitly not the contract.
    assert_eq!(
        diagnostics.len(),
        1,
        "exactly one overflow diagnostic expected (not one per excess candidate), got {}",
        diagnostics.len()
    );
}
