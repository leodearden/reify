//! Meaning-free primitive vocabulary for Reify.
//!
//! This is the leaf crate introduced in Phase 1 of
//! `docs/prds/core-ast-ir-layering.md` (task γ). It contains the eight
//! modules that carry no semantic meaning (no `reify-*` dependencies) and
//! can therefore sit at the bottom of the dependency graph.
//!
//! # B1 invariant
//!
//! This crate MUST have zero `reify-*` dependencies. The structural invariant
//! is locked in by `crates/reify-core/tests/dag_invariant.rs`, which shells
//! out to `cargo metadata` and asserts that no dependency name starts with
//! `"reify-"`. The workspace-wide assertion (`scripts/assert-crate-dag.sh`)
//! arrives under task η per PRD §10.

// `BTreeMap<Value, _>` in downstream crates can trigger this lint; we copy
// the attribute from reify-types to keep the two crates' lint preludes
// structurally identical for downstream reasoning.
#![allow(clippy::mutable_key_type)]

pub mod diagnostics;
pub mod dimension;
pub mod hash;
pub mod identity;
pub mod primitives;
pub mod source_location;
pub mod spanned_ident;
pub mod ty;

// Root re-exports — mirror the flat surface that previously lived at the
// reify-types lib root for these eight modules.  Mirroring them here keeps
// the moved modules' internal `use crate::SourceSpan;` (spanned_ident.rs),
// `crate::SourceSpan::PRELUDE_SENTINEL_OFFSET` (source_location.rs), and
// `use crate::ContentHash;` (dimension.rs) compiling without per-file edits.
pub use diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticInfo, DiagnosticLabel, DiagnosticRef, Severity,
    SourceSpan,
};
pub use dimension::{DimensionVector, NAMED_DIMENSIONS, Rational};
pub use hash::ContentHash;
pub use identity::*;
pub use primitives::{
    DEPRECATED_ANNOTATION, OPTIMIZED_ANNOTATION, PortDirection, SHELL_ANNOTATION, SOLID_ANNOTATION,
    SOLVER_HINT_ANNOTATION, TEST_ANNOTATION,
};
pub use source_location::{
    SourceLocationInfo, build_line_offsets, byte_offset_to_line_col,
    line_col_to_byte_offset_with_offsets,
};
pub use spanned_ident::SpannedIdent;
pub use ty::Type;
