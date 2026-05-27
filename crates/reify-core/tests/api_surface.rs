//! Compile-time surface pin for `reify-core`.
//!
//! Pins the full public API that `reify-core` MUST export after the atomic
//! module move (step-2), in both the flat form (`reify_core::SourceSpan`) and
//! the module-path form (`reify_core::diagnostics::SourceSpan`).
//!
//! This test is intentionally RED until step-2 lands the `pub mod` declarations
//! and root re-exports inside `crates/reify-core/src/lib.rs`.

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
    ConstraintNodeId, ModulePath, RealizationNodeId, ResolutionNodeId, SnapshotId, ValueCellId,
};
use reify_core::identity::{
    ConstraintNodeId as CNodeMod, ModulePath as ModPathMod, RealizationNodeId as RNodeMod,
    ResolutionNodeId as ResNodeMod, SnapshotId as SnapMod, ValueCellId as VCellMod,
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
    let _: DimensionVector;
    let _: DimVecMod;
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
