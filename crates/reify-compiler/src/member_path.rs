//! The single compile-time **member-shape authority** for dotted member paths.
//!
//! Introduced by task 5424 (PRD `docs/prds/v0_6/uniform-member-access.md` task
//! α, §5 contract C1). One resolver answers, for any `<receiver>.<member>`
//! chain of arbitrary depth: *is this a member?*, *of which concrete structure?*,
//! *what kind of member?*, *is it visible from here?*, and *what is its static
//! type?*
//!
//! # Contract
//!
//! * **C1-i — purely static.** Resolution performs no evaluation and emits no
//!   diagnostics. Neither entry point takes a `&mut Vec<Diagnostic>`, so this
//!   is enforced by the signature rather than by convention: callers render
//!   diagnostics themselves from the returned typed error via
//!   `MemberPathError::to_diagnostic(span)`. This lets speculative consumers
//!   (PRD task β's type-driven geometry acceptance) ask "is this a
//!   Geometry-typed member path?" without side effects.
//! * **C1-ii — `priv` at every hop.** Visibility is enforced at each hop of the
//!   chain, not only at the terminal.
//! * **C1-iii — concrete attribution.** An unknown member at hop *k* names the
//!   concrete structure at hop *k*, never a generic sentence.
//! * **C1-iv — no lockstep duplication (INV-5).** No other site may re-match
//!   member AST shapes to decide membership or visibility. Sites that need that
//!   verdict call in here.
//!
//! # Visibility of this module's items
//!
//! `pub(crate)` throughout. The known future consumers — PRD task β (geometry
//! position acceptance) and task η (sub-matcher retirement) — are same-crate
//! callers, so no `pub` export is warranted yet.
