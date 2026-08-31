//! Compile-time surface pin for `reify-core`.
//!
//! Pins the full public API that `reify-core` MUST export after the atomic
//! module move (step-2), in both the flat form (`reify_core::SourceSpan`) and
//! the module-path form (`reify_core::diagnostics::SourceSpan`).
//!
//! Both spellings remain in sync because `reify-core/src/lib.rs` exports each
//! module as `pub mod` AND re-exports its symbols at the crate root.

// ── diagnostics (flat form) ──────────────────────────────────────────────────
use reify_core::{
    Diagnostic, DiagnosticCode, DiagnosticInfo, DiagnosticLabel, DiagnosticRef, Severity,
    SourceSpan,
};

// ── diagnostics (module-path form) ──────────────────────────────────────────
use reify_core::diagnostics::{
    Diagnostic as DiagMod, DiagnosticCode as DiagCodeMod, DiagnosticInfo as DiagInfoMod,
    DiagnosticLabel as DiagLabelMod, DiagnosticRef as DiagRefMod, Severity as SeverityMod,
    SourceSpan as SourceSpanMod,
};

// ── hash ─────────────────────────────────────────────────────────────────────
use reify_core::ContentHash;
use reify_core::hash::ContentHash as ContentHashMod;

// ── dimension ────────────────────────────────────────────────────────────────
use reify_core::{DimensionVector, NAMED_DIMENSIONS, Rational};
use reify_core::dimension::{DimensionVector as DimVecMod, NAMED_DIMENSIONS as NAMED_DIM_MOD, Rational as RationalMod};

// ── ty ───────────────────────────────────────────────────────────────────────
use reify_core::Type;
use reify_core::ty::Type as TypeMod;

// ── identity ─────────────────────────────────────────────────────────────────
use reify_core::{
    ComputeNodeId, ConstraintNodeId, EntityPath, FIELD_ENTITY_PREFIX, LOCATED_PORT_TRAIT,
    MemberName, ModulePath, ModulePathParseError, RealizationNodeId, ResolutionNodeId,
    ScopeId, SnapshotId, SourceNodeId, ValueCellId, VersionId,
};
use reify_core::identity::{
    ComputeNodeId as ComputeNodeMod, ConstraintNodeId as CNodeMod, EntityPath as EntityPathMod,
    FIELD_ENTITY_PREFIX as FIELD_PREFIX_MOD, LOCATED_PORT_TRAIT as LOCATED_PORT_MOD,
    MemberName as MemberNameMod, ModulePath as ModPathMod, ModulePathParseError as MPEMod,
    RealizationNodeId as RNodeMod, ResolutionNodeId as ResNodeMod, ScopeId as ScopeIdMod,
    SnapshotId as SnapMod, SourceNodeId as SourceNodeMod, ValueCellId as VCellMod,
    VersionId as VersionIdMod,
};

// ── source_location ──────────────────────────────────────────────────────────
use reify_core::{
    SourceLocationInfo, build_line_offsets, byte_offset_to_line_col,
    line_col_to_byte_offset_with_offsets,
};
use reify_core::source_location::{
    SourceLocationInfo as SLocMod, build_line_offsets as build_offsets_mod,
    byte_offset_to_line_col as byte_to_lc_mod,
    line_col_to_byte_offset_with_offsets as lc_to_byte_mod,
};

// ── spanned_ident ────────────────────────────────────────────────────────────
use reify_core::SpannedIdent;
use reify_core::spanned_ident::SpannedIdent as SpannedIdentMod;

// ── primitives ───────────────────────────────────────────────────────────────
use reify_core::{
    DEPRECATED_ANNOTATION, OPTIMIZED_ANNOTATION, SHELL_ANNOTATION, SOLID_ANNOTATION,
    SOLVER_HINT_ANNOTATION, TEST_ANNOTATION,
};
use reify_core::primitives::{
    DEPRECATED_ANNOTATION as DEPRECATED_MOD, OPTIMIZED_ANNOTATION as OPTIMIZED_MOD,
    PortDirection, SHELL_ANNOTATION as SHELL_MOD, SOLID_ANNOTATION as SOLID_MOD,
    SOLVER_HINT_ANNOTATION as SOLVER_MOD, TEST_ANNOTATION as TEST_MOD,
};

// ── units ────────────────────────────────────────────────────────────────────
use reify_core::{BUILTIN_UNITS, ri_compound_unit_expr, ri_emittable_units, unit_symbol_to_si};
use reify_core::units::{
    BUILTIN_UNITS as BUILTIN_UNITS_MOD, ri_compound_unit_expr as ri_compound_unit_expr_mod,
    ri_emittable_units as ri_emittable_units_mod, unit_symbol_to_si as unit_symbol_to_si_mod,
};

// ── flat PortDirection ────────────────────────────────────────────────────────
use reify_core::PortDirection as PortDirectionFlat;

// ─────────────────────────────────────────────────────────────────────────────
// Surface assertions
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn diagnostics_flat_types_constructible() {
    let span: SourceSpan = SourceSpan::new(0, 5);
    assert_eq!(span.start, 0);
    assert_eq!(span.end, 5);

    let _span2: SourceSpanMod = SourceSpanMod::new(0, 5);

    let sev: Severity = Severity::Error;
    let _sev2: SeverityMod = SeverityMod::Error;
    assert_eq!(sev, Severity::Error);

    // Verify the Diagnostic, DiagnosticCode, etc. types are in scope.
    // We only assert the type names are importable (compiler verifies at use site).
    let _: fn() -> Option<DiagnosticCode> = || None;
    let _: fn() -> Option<DiagnosticLabel> = || None;
    let _: fn() -> Option<DiagnosticRef> = || None;
    let _: fn() -> Option<Diagnostic> = || None;
    let _: fn() -> Option<DiagnosticInfo> = || None;

    // Module-path aliases must resolve to the same type.
    let _: fn() -> Option<DiagCodeMod> = || None;
    let _: fn() -> Option<DiagLabelMod> = || None;
    let _: fn() -> Option<DiagRefMod> = || None;
    let _: fn() -> Option<DiagMod> = || None;
    let _: fn() -> Option<DiagInfoMod> = || None;
}

#[test]
fn hash_flat_and_module_path() {
    let h: ContentHash = ContentHash::of_str("test");
    let _h2: ContentHashMod = ContentHashMod::of_str("test");
    assert_eq!(h, ContentHash::of_str("test"));
}

#[test]
fn dimension_flat_and_module_path() {
    let _: fn() -> Option<DimensionVector> = || None;
    let _: fn() -> Option<DimVecMod> = || None;
    let _: Rational = Rational::ZERO;
    let _: RationalMod = RationalMod::ZERO;
    // NAMED_DIMENSIONS is a static slice — just check it's non-empty.
    assert!(!NAMED_DIMENSIONS.is_empty());
    assert!(!NAMED_DIM_MOD.is_empty());
}

#[test]
fn ty_flat_and_module_path() {
    let _: fn() -> Option<Type> = || None;
    let _: fn() -> Option<TypeMod> = || None;
}

#[test]
fn identity_flat_and_module_path() {
    let mp: ModulePath = ModulePath::single("bracket");
    let _mp2: ModPathMod = ModPathMod::single("bracket");
    assert_eq!(mp, ModulePath::single("bracket"));

    let vc: ValueCellId = ValueCellId::new("E", "p");
    let _vc2: VCellMod = VCellMod::new("E", "p");
    assert_eq!(vc, ValueCellId::new("E", "p"));

    let cn: ConstraintNodeId = ConstraintNodeId::new("E", 0);
    let _cn2: CNodeMod = CNodeMod::new("E", 0);
    assert_eq!(cn, ConstraintNodeId::new("E", 0));

    let rn: RealizationNodeId = RealizationNodeId::new("E", 0);
    let _rn2: RNodeMod = RNodeMod::new("E", 0);
    assert_eq!(rn, RealizationNodeId::new("E", 0));

    let rsn: ResolutionNodeId = ResolutionNodeId::new("E", 0);
    let _rsn2: ResNodeMod = ResNodeMod::new("E", 0);
    assert_eq!(rsn, ResolutionNodeId::new("E", 0));

    let snap: SnapshotId = SnapshotId(42);
    let _snap2: SnapMod = SnapMod(42);
    assert_eq!(snap, SnapshotId(42));

    // Items previously covered only by wildcard re-export.
    let cn: ComputeNodeId = ComputeNodeId::new("E", 0);
    let _cn2: ComputeNodeMod = ComputeNodeMod::new("E", 0);
    assert_eq!(cn, ComputeNodeId::new("E", 0));

    let _si: ScopeId = ScopeId(0);
    let _si2: ScopeIdMod = ScopeIdMod(0);

    let _vi: VersionId = VersionId(0);
    let _vi2: VersionIdMod = VersionIdMod(0);

    assert_eq!(FIELD_ENTITY_PREFIX, "__field");
    assert_eq!(FIELD_PREFIX_MOD, "__field");
    assert_eq!(LOCATED_PORT_TRAIT, "LocatedPort");
    assert_eq!(LOCATED_PORT_MOD, "LocatedPort");

    let _: fn() -> Option<ModulePathParseError> = || None;
    let _: fn() -> Option<MPEMod> = || None;
    let _: fn() -> Option<EntityPath> = || None;
    let _: fn() -> Option<EntityPathMod> = || None;
    let _: fn() -> Option<MemberName> = || None;
    let _: fn() -> Option<MemberNameMod> = || None;
    let _: fn() -> Option<SourceNodeId> = || None;
    let _: fn() -> Option<SourceNodeMod> = || None;
}

#[test]
fn source_location_flat_and_module_path() {
    let offsets = build_line_offsets("hello\nworld");
    assert_eq!(offsets, vec![5usize]);

    let offsets2 = build_offsets_mod("hello\nworld");
    assert_eq!(offsets2, vec![5usize]);

    let pos = byte_offset_to_line_col("hello\nworld", 6);
    assert_eq!(pos, (2, 1));

    let pos2 = byte_to_lc_mod("hello\nworld", 6);
    assert_eq!(pos2, (2, 1));

    let byte = line_col_to_byte_offset_with_offsets("hello\nworld", 2, 1, &offsets);
    assert_eq!(byte, 6);

    let byte2 = lc_to_byte_mod("hello\nworld", 2, 1, &offsets2);
    assert_eq!(byte2, 6);

    let sli: SourceLocationInfo = SourceLocationInfo {
        file_path: "test.ri".into(),
        line: 1,
        column: 1,
        end_line: 1,
        end_column: 5,
    };
    let _sli2: SLocMod = SLocMod {
        file_path: "test.ri".into(),
        line: 1,
        column: 1,
        end_line: 1,
        end_column: 5,
    };
    assert_eq!(sli.file_path, "test.ri");
}

#[test]
fn spanned_ident_flat_and_module_path() {
    let span = SourceSpan::new(0, 5);
    let si: SpannedIdent = SpannedIdent {
        name: "width".into(),
        span,
    };
    let _si2: SpannedIdentMod = SpannedIdentMod {
        name: "width".into(),
        span,
    };
    assert_eq!(si.name, "width");
}

#[test]
fn primitives_const_values() {
    assert_eq!(TEST_ANNOTATION, "test");
    assert_eq!(DEPRECATED_ANNOTATION, "deprecated");
    assert_eq!(OPTIMIZED_ANNOTATION, "optimized");
    assert_eq!(SOLVER_HINT_ANNOTATION, "solver_hint");
    assert_eq!(SHELL_ANNOTATION, "shell");
    assert_eq!(SOLID_ANNOTATION, "solid");

    // Module-path forms.
    assert_eq!(TEST_MOD, "test");
    assert_eq!(DEPRECATED_MOD, "deprecated");
    assert_eq!(OPTIMIZED_MOD, "optimized");
    assert_eq!(SOLVER_MOD, "solver_hint");
    assert_eq!(SHELL_MOD, "shell");
    assert_eq!(SOLID_MOD, "solid");
}

#[test]
fn port_direction_flat_and_module_path() {
    let pd: PortDirectionFlat = PortDirectionFlat::In;
    let pd2: PortDirection = PortDirection::Out;
    assert_ne!(pd, pd2);
    assert_eq!(pd, PortDirectionFlat::In);
}

#[test]
fn units_flat_and_module_path() {
    // The built-in symbol → SI table, in both spellings.
    let (factor, dim) = unit_symbol_to_si("mm").expect("mm is a built-in symbol");
    assert_eq!(dim, DimensionVector::LENGTH);
    assert_eq!(factor, 0.001);
    assert_eq!(unit_symbol_to_si_mod("mm"), Some((factor, dim)));
    assert_eq!(unit_symbol_to_si("furlong"), None);

    // The reverse (.ri-emission) table, in both spellings. `reify-ir`'s
    // `value_to_ri_literal` depends on this surface, so pin it at compile
    // time rather than incidentally (task #5095).
    //
    // Deliberately NOT asserting the ladder's CONTENTS here. That is this
    // file's remit boundary: it pins reachability and the SIGNATURE (the
    // `&'static [&'static str]` annotation below is load-bearing — it fails to
    // compile if the return type moves to an owned or borrowed form). The
    // table's contents belong to `units.rs`, where the ladder and its two
    // drift guards live; duplicating `["mm", "cm", "m"]` here would add a
    // third edit site for a deliberate ladder change and no coverage.
    let ladder: &'static [&'static str] = ri_emittable_units(&DimensionVector::LENGTH);
    assert!(!ladder.is_empty(), "LENGTH must have an emission ladder");
    assert_eq!(ri_emittable_units_mod(&DimensionVector::LENGTH), ladder);
    assert!(ri_emittable_units(&DimensionVector::DIMENSIONLESS).is_empty());

    // The built-in table itself, in both spellings. `reify-compiler`'s
    // stdlib/built-in cross-guard iterates this slice to decide which symbols
    // to check for registry drift, so its reachability from OUTSIDE the crate
    // is load-bearing, not incidental (task #5095).
    //
    // Same remit boundary as the ladder above: the annotation below is the
    // assertion — it fails to compile if the entry shape moves (to a struct, an
    // owned form, or a different factor type), which would silently change what
    // every table-driven guard iterates. The table's CONTENTS stay in
    // `units.rs`, where the entries and the guards over them live — a count
    // here would be one more hand-maintained number to drift, which is the
    // exact defect this task removed.
    let table: &'static [(&'static str, f64, DimensionVector)] = BUILTIN_UNITS;
    assert!(!table.is_empty(), "the built-in unit table must be reachable");
    assert_eq!(BUILTIN_UNITS_MOD, table);

    // The COMPOUND emission builder, in both spellings (task #6400).
    // `reify-ir`'s `value_to_ri_literal_in_scope` depends on this surface under
    // `UnitScope::SiBaseUnitsSeeded`, so pin it at compile time.
    //
    // Same remit boundary as the ladder above: this pins reachability and the
    // SIGNATURE — the `fn(&DimensionVector) -> Option<String>` annotation below
    // is load-bearing, failing to compile if the return type moves to a
    // borrowed or infallible form. The expected emission STRINGS stay in
    // `units.rs`, where the builder and its shape/rejection guards live;
    // duplicating `"m^2"` here would add a third edit site for a deliberate
    // change and no coverage.
    let compound: fn(&DimensionVector) -> Option<String> = ri_compound_unit_expr;
    let area = compound(&DimensionVector::AREA);
    assert!(area.is_some(), "AREA must have a compound unit expression");
    assert_eq!(ri_compound_unit_expr_mod(&DimensionVector::AREA), area);
    assert_eq!(ri_compound_unit_expr(&DimensionVector::DIMENSIONLESS), None);
}
