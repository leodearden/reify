//! Integration tests for task 3974 (trait associated types ιₑ): verifies that a
//! qualified associated-type access used as a type-expr — `Beam::Material` and the
//! `Beam::(HasMaterial::Material)` paren disambiguator (FORK-G) — resolves to the
//! base structure's bound associated type, and that a bare access ambiguous across
//! two conformed traits raises `E_AMBIGUOUS_ASSOC_TYPE`.
//!
//! PRD: docs/prds/v0_6/trait-associated-functions.md §5.3a, §8 Phase 10, FORK-G.
//!
//! Resolution reads iota-β's resolved associated-type table
//! (`TopologyTemplate.assoc_types` / `.trait_bounds`); the base structure must be
//! declared BEFORE its consumer (source-order compilation).

use reify_core::{DiagnosticCode, Type};
use reify_test_support::{compile_source, compile_source_with_stdlib, errors_only};

// ─── Step-3 RED: bare-unique qualified access ─────────────────────────────────

/// `param m : Beam::Material`, where `Beam` conforms to a single trait declaring
/// `type Material` and binds `type Material = Steel`, must resolve to
/// `Type::StructureRef("Steel")` with no diagnostics.
///
/// Fails today: the `QualifiedAssoc` type-expr resolves to `None` in the
/// registry-less generic resolver, so the entity.rs param arm emits
/// `UnresolvedType` and falls back to `Type::Real`.
#[test]
fn bare_unique_qualified_assoc_resolves_in_param_annotation() {
    let source = r#"
structure Steel {}
trait HasMaterial { type Material }
structure def Beam : HasMaterial {
    type Material = Steel
}
structure def UseBeam {
    param m : Beam::Material
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for bare-unique Beam::Material; got: {:?}",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "UseBeam")
        .expect("UseBeam template should be compiled");

    let m_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "m")
        .expect("value cell 'm' should exist");

    assert_eq!(
        m_cell.cell_type,
        Type::StructureRef("Steel".to_string()),
        "param typed `Beam::Material` should resolve to Type::StructureRef(\"Steel\"); \
         got: {:?}",
        m_cell.cell_type
    );
}

// ─── Step-5 RED: two-trait ambiguity ──────────────────────────────────────────

/// Collect the message plus every label message of a diagnostic into one
/// haystack, so substring assertions are agnostic to whether a name lands in the
/// top-level message or in an attached label.
fn diag_haystack(d: &reify_core::Diagnostic) -> String {
    let mut s = d.message.clone();
    for label in &d.labels {
        s.push('\n');
        s.push_str(&label.message);
    }
    s
}

/// When `Beam` conforms to TWO traits that each declare `type Material`, a bare
/// `param m : Beam::Material` is ambiguous: exactly one `AmbiguousAssocType`
/// diagnostic must be raised, naming the structure, the member, and both
/// candidate traits. The structure `Beam` itself must compile clean (no
/// `ConflictingTraitAssocType`) — this pins the achievability premise (a second
/// same-name *required* assoc type dedups silently; conflict fires only for
/// differing trait *defaults*).
///
/// Fails after step-4: the helper returns `None` for `declaring_traits.len() >= 2`
/// without emitting any diagnostic, so zero `AmbiguousAssocType` are produced.
#[test]
fn two_trait_bare_qualified_assoc_is_ambiguous() {
    let source = r#"
structure Steel {}
trait HasMaterial { type Material }
trait HasSkin { type Material }
structure def Beam : HasMaterial + HasSkin {
    type Material = Steel
}
structure def UseBeam {
    param m : Beam::Material
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    // Achievability premise: Beam compiles clean — two same-name *required* assoc
    // types do not conflict (conflict fires only for differing trait defaults).
    let conflicts: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ConflictingTraitAssocType))
        .collect();
    assert!(
        conflicts.is_empty(),
        "Beam : HasMaterial, HasSkin with `type Material = Steel` must compile clean \
         (no ConflictingTraitAssocType); got: {:?}",
        conflicts
    );

    let ambiguous: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AmbiguousAssocType))
        .collect();
    assert_eq!(
        ambiguous.len(),
        1,
        "expected exactly one AmbiguousAssocType for bare `Beam::Material`; all errors: {:?}",
        errors
    );

    let haystack = diag_haystack(ambiguous[0]);
    for needle in ["Beam", "Material", "HasMaterial", "HasSkin"] {
        assert!(
            haystack.contains(needle),
            "AmbiguousAssocType diagnostic should name `{}`; full text: {:?}",
            needle,
            haystack
        );
    }
}

// ─── Step-7 RED: paren disambiguator (FORK-G) ─────────────────────────────────

/// The FORK-G paren disambiguator `Beam::(Trait::Material)` resolves the
/// otherwise-ambiguous two-trait case distinctly, with no `AmbiguousAssocType`.
/// Because the structure binds `Material` exactly once, BOTH qualifiers
/// (`HasMaterial` and `HasSkin`) resolve to the same `Type::StructureRef("Steel")`
/// — the qualifier is disambiguation-only.
///
/// Fails after step-6: the helper does not yet handle `trait_name = Some(..)`,
/// so it returns `None` and the params fall back to `Type::Real`.
#[test]
fn paren_disambiguated_qualified_assoc_resolves_both_qualifiers() {
    let source = r#"
structure Steel {}
trait HasMaterial { type Material }
trait HasSkin { type Material }
structure def Beam : HasMaterial + HasSkin {
    type Material = Steel
}
structure def UseBeam {
    param m : Beam::(HasMaterial::Material)
    param m2 : Beam::(HasSkin::Material)
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for paren-disambiguated qualifiers; got: {:?}",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "UseBeam")
        .expect("UseBeam template should be compiled");

    let cell_type = |member: &str| {
        template
            .value_cells
            .iter()
            .find(|vc| vc.id.member == member)
            .unwrap_or_else(|| panic!("value cell `{member}` should exist"))
            .cell_type
            .clone()
    };

    assert_eq!(
        cell_type("m"),
        Type::StructureRef("Steel".to_string()),
        "`Beam::(HasMaterial::Material)` should resolve to Type::StructureRef(\"Steel\")"
    );
    assert_eq!(
        cell_type("m2"),
        Type::StructureRef("Steel".to_string()),
        "`Beam::(HasSkin::Material)` should resolve to the same Type::StructureRef(\"Steel\") \
         (the structure binds Material once; the qualifier is disambiguation-only)"
    );
}

// ─── Step-9 RED: error / edge paths (each: no panic + a clear diagnostic) ──────

/// Returns true if any diagnostic's combined text contains `needle`.
fn any_diag_mentions(errors: &[&reify_core::Diagnostic], needle: &str) -> bool {
    errors.iter().any(|d| diag_haystack(d).contains(needle))
}

/// Returns true if any diagnostic carries `code`.
fn any_diag_has_code(errors: &[&reify_core::Diagnostic], code: DiagnosticCode) -> bool {
    errors.iter().any(|d| d.code == Some(code))
}

/// (a) Unknown member: `Beam::Bogus` where no conformed trait declares `Bogus`.
/// Must yield an `UnresolvedType` (or equivalent) error naming the member, and
/// must NOT be reported as `AmbiguousAssocType`.
///
/// Fails after step-8: bare `declaring_traits.len() == 0` returns `None` with no
/// diagnostic, so the param silently falls back to `Type::Real`.
#[test]
fn unknown_member_qualified_assoc_is_unresolved_not_ambiguous() {
    let source = r#"
structure Steel {}
trait HasMaterial { type Material }
structure def Beam : HasMaterial {
    type Material = Steel
}
structure def UseBeam {
    param m : Beam::Bogus
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !any_diag_has_code(&errors, DiagnosticCode::AmbiguousAssocType),
        "unknown member must NOT be reported as ambiguous; got: {:?}",
        errors
    );
    assert!(
        any_diag_has_code(&errors, DiagnosticCode::UnresolvedType)
            && any_diag_mentions(&errors, "Bogus"),
        "expected an UnresolvedType error naming `Bogus`; got: {:?}",
        errors
    );
}

/// (b) Bad disambiguator: `Beam::(HasSkin::Material)` where `Beam` does NOT
/// conform to `HasSkin`. Must yield a 'does not conform' diagnostic and not an
/// `AmbiguousAssocType`.
#[test]
fn bad_disambiguator_nonconforming_trait_diagnoses() {
    let source = r#"
structure Steel {}
trait HasMaterial { type Material }
trait HasSkin { type Material }
structure def Beam : HasMaterial {
    type Material = Steel
}
structure def UseBeam {
    param m : Beam::(HasSkin::Material)
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !any_diag_has_code(&errors, DiagnosticCode::AmbiguousAssocType),
        "a bad disambiguator must NOT be reported as ambiguous; got: {:?}",
        errors
    );
    assert!(
        any_diag_mentions(&errors, "does not conform") && any_diag_mentions(&errors, "HasSkin"),
        "expected a 'does not conform' diagnostic naming `HasSkin`; got: {:?}",
        errors
    );
}

/// (b′) Bad disambiguator, conformed-but-non-declaring trait:
/// `Beam::(HasOther::Material)` where `Beam` conforms to BOTH `HasOther` and
/// `HasMaterial`, but only `HasMaterial` declares `type Material`. This pins the
/// SECOND error branch of the `Some(t)` disambiguator arm — the qualifier IS a
/// bound of `Beam` (so the 'does not conform' branch is skipped) yet does NOT
/// declare the member — distinct from `bad_disambiguator_nonconforming_trait_diagnoses`
/// above (where the qualifier is not conformed at all). Must yield a 'does not
/// declare associated type' diagnostic naming the qualifier trait and the member,
/// and must NOT be reported as `AmbiguousAssocType`.
///
/// Without this test a regression that swapped or dropped the branch-2 diagnostic
/// (e.g. folded it into the 'does not conform' message, or mis-routed it through
/// the ambiguity path) would go uncaught.
#[test]
fn disambiguator_conformed_trait_lacking_member_diagnoses() {
    let source = r#"
structure Steel {}
trait HasMaterial { type Material }
trait HasOther { type Other }
structure def Beam : HasMaterial + HasOther {
    type Material = Steel
    type Other = Steel
}
structure def UseBeam {
    param m : Beam::(HasOther::Material)
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !any_diag_has_code(&errors, DiagnosticCode::AmbiguousAssocType),
        "a conformed-but-non-declaring qualifier must NOT be reported as ambiguous; got: {:?}",
        errors
    );
    assert!(
        any_diag_mentions(&errors, "does not declare associated type")
            && any_diag_mentions(&errors, "HasOther")
            && any_diag_mentions(&errors, "Material"),
        "expected a 'does not declare associated type' diagnostic naming the qualifier \
         trait `HasOther` and the member `Material`; got: {:?}",
        errors
    );
}

/// (c) Type-parameter base: `T::Material` at a definition site, where `T` is an
/// unbound type parameter. Must yield a clear 'type parameter' diagnostic (no
/// concrete `StructureRef` exists at definition time) and must not panic.
///
/// Fails after step-8: the registry lookup misses for `T` and the helper returns
/// `None` silently, so the param falls back to `Type::Real` with no diagnostic.
#[test]
fn type_parameter_base_qualified_assoc_diagnoses() {
    let source = r#"
structure def UseT<T> {
    param m : T::Material
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !any_diag_has_code(&errors, DiagnosticCode::AmbiguousAssocType),
        "a type-parameter base must NOT be reported as ambiguous; got: {:?}",
        errors
    );
    assert!(
        any_diag_mentions(&errors, "type parameter") && any_diag_mentions(&errors, "T"),
        "expected a clear 'type parameter' diagnostic naming `T`; got: {:?}",
        errors
    );
}

// ─── Step-11 RED: end-to-end via the CI example file ──────────────────────────

/// Absolute path to the workspace `examples/` directory, resolved at compile
/// time from this crate's manifest directory (two levels up) — the same scheme
/// `examples_smoke.rs` uses.
const EXAMPLES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");

/// End-to-end: the CI example `examples/trait_assoc_type_qualified.ri` compiles
/// clean WITH the stdlib prelude (the same path `examples_smoke` exercises), and
/// its consumer structure's bare (`Beam::Material`) and paren-disambiguated
/// (`Beam::(IotaHasMaterial::Material)`) params both resolve to the structure's
/// bound material `Type::StructureRef("IotaSteel")`.
///
/// RED until step-12 creates the file: `read_to_string` fails and the test panics.
#[test]
fn example_file_qualified_assoc_compiles_and_resolves() {
    let path = std::path::Path::new(EXAMPLES_DIR).join("trait_assoc_type_qualified.ri");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    let module = compile_source_with_stdlib(&source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "examples/trait_assoc_type_qualified.ri must compile clean with stdlib; got: {:?}",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "UseBeam")
        .expect("UseBeam template should be compiled");

    let cell_type = |member: &str| {
        template
            .value_cells
            .iter()
            .find(|vc| vc.id.member == member)
            .unwrap_or_else(|| panic!("value cell `{member}` should exist"))
            .cell_type
            .clone()
    };

    // Bare-unique access: `Beam` conforms to a single trait declaring `Material`.
    assert_eq!(
        cell_type("m"),
        Type::StructureRef("IotaSteel".to_string()),
        "bare `Beam::Material` should resolve to Type::StructureRef(\"IotaSteel\")"
    );
    // FORK-G paren disambiguator resolves to the same single binding.
    assert_eq!(
        cell_type("n"),
        Type::StructureRef("IotaSteel".to_string()),
        "`Beam::(IotaHasMaterial::Material)` should resolve to Type::StructureRef(\"IotaSteel\")"
    );
}

// ─── Amendment: declared-but-unbound poison path (anti-cascade) ───────────────

/// Declared-but-unbound: `Beam` conforms to `HasMaterial` (which declares
/// `type Material`) but never binds `type Material = …`. The producer side
/// already reports this once as `TraitAssocTypeNotBound`; the consumer
/// `param m : Beam::Material` must NOT pile on a second diagnostic. Inside
/// `resolve_qualified_assoc_type`, exactly one conformed trait declares
/// `Material` (so the bare path is taken), but `template.assoc_types` has no
/// entry for it — `resolved_member()`'s `.unwrap_or(Type::Error)` branch fires
/// and the helper returns `Some(Type::Error)`, which flows through the entity.rs
/// `Some(t) => t` arm so `m` types as `Type::Error` (the poison sentinel), not
/// `Type::Real`.
///
/// This pins the most heavily-documented-yet-previously-untested behaviour of the
/// helper: a future refactor that emitted a duplicate Unresolved/Ambiguous
/// diagnostic at the consumer, or poisoned to a concrete `Type::Real`, would fail
/// here.
#[test]
fn declared_but_unbound_qualified_assoc_poisons_to_error_without_cascade() {
    let source = r#"
trait HasMaterial { type Material }
structure def Beam : HasMaterial {
}
structure def UseBeam {
    param m : Beam::Material
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    // Producer root cause: Beam fails to bind the required associated type. This
    // is the ONLY error the program should produce.
    assert!(
        any_diag_has_code(&errors, DiagnosticCode::TraitAssocTypeNotBound),
        "expected the producer-side TraitAssocTypeNotBound for Beam's unbound \
         `type Material`; got: {:?}",
        errors
    );
    // Anti-cascade: the consumer site (`Beam::Material`) adds NO second diagnostic
    // — neither a spurious UnresolvedType nor a (wrong) AmbiguousAssocType.
    assert!(
        !any_diag_has_code(&errors, DiagnosticCode::AmbiguousAssocType)
            && !any_diag_has_code(&errors, DiagnosticCode::UnresolvedType),
        "the consumer `Beam::Material` must not emit a second Unresolved/Ambiguous \
         diagnostic for the declared-but-unbound case (anti-cascade); got: {:?}",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "UseBeam")
        .expect("UseBeam template should be compiled");
    let m_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "m")
        .expect("value cell 'm' should exist");
    assert_eq!(
        m_cell.cell_type,
        Type::Error,
        "declared-but-unbound `Beam::Material` should poison `m` to Type::Error \
         (the anti-cascade sentinel), not fall back to Type::Real; got: {:?}",
        m_cell.cell_type
    );
}
