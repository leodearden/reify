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
use reify_test_support::{compile_source, errors_only};

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
