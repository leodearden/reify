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

use reify_core::Type;
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
