//! Shared fixture helpers for interrogating a compiled design file's surface —
//! the three jobs every `prj/`-backed gate in this compile unit does, with the
//! design FILE a parameter rather than a constant.
//!
//!   * [`entity_cell`] / [`entity_real`] read one dimension-checked cell out of
//!     an evaluated [`ValueMap`];
//!   * [`CellSpans`] answers WHICH cell a diagnostic is about, structurally, by
//!     label span — never by message text, which belongs to another crate;
//!   * [`Strictness`] / [`assert_constraints_ok`] assert a `constraint_results`
//!     set is non-empty and then that it holds.
//!
//! All of it was written for [`super::capstan_groove_e2e`] against
//! `prj/printer_v01/dev_capstan.ri`, and then wanted verbatim by
//! [`super::idler_seat_e2e`] against `prj/printer_v01/printer.ri`. The only
//! thing that differed between the two copies was which file a failure message
//! named, so `file: &str` is the parameter that replaced the fork. It lives
//! here rather than in either gate because it belongs to NEITHER design: both
//! files declare structures of the same name, and a gate for a third file
//! should reach for these three jobs without importing them out of the
//! capstan's module.

use reify_core::{Diagnostic, DimensionVector, SourceSpan, ValueCellId};
use reify_eval::ConstraintCheckEntry;
use reify_ir::{Satisfaction, Value, ValueMap};

/// Read a `Value::Scalar` cell of `entity` out of a value map, asserting its
/// dimension, and return its SI value (m / m³ / dimensionless).
///
/// Entity-parameterised so a cross-structure gate can read another structure's
/// cells — and an instance-scoped `<assembly>.<sub>` form — through the same
/// dimension-checked path, rather than carrying a second copy of the
/// `Value::Scalar` match. Map-parameterised (rather than taking a
/// `&TessellateResult`) so a kernel-free check surface reads its cells through
/// the same helper too.
///
/// Values in the map are SI; every failure message in this unit formats lengths
/// in mm, the unit the design files are written in.
///
/// `file` is message text only, and it is the reason the panic reads as "this
/// file does not declare this cell" — which is exactly what a RED step against
/// a not-yet-written cell means, and which of the two files carrying a
/// structure of this name it is about.
pub(super) fn entity_cell(
    values: &ValueMap,
    file: &str,
    entity: &str,
    cell: &str,
    expected_dim: DimensionVector,
) -> f64 {
    let id = ValueCellId::new(entity, cell);
    match values.get(&id) {
        Some(Value::Scalar {
            si_value,
            dimension,
        }) => {
            assert_eq!(
                *dimension, expected_dim,
                "{entity}.{cell}: expected dimension {expected_dim:?}, got {dimension:?}"
            );
            *si_value
        }
        other => panic!(
            "{entity}.{cell} must be a Value::Scalar with dimension {expected_dim:?}, \
             got {other:?} — is the cell declared in {file}?"
        ),
    }
}

/// Read a dimensionless (`: Real`) cell of `entity` out of a value map — a pure
/// count or a ratio.
///
/// Separate from [`entity_cell`] because the evaluator does NOT wrap a
/// dimensionless quantity in `Value::Scalar { dimension: DIMENSIONLESS }`: a
/// `: Real` cell comes back as a bare `Value::Real`. Both spellings are accepted
/// anyway — they denote the same mathematical object, and this unit's assertions
/// are about the DESIGN, not about which representation the evaluator picks for
/// a unitless number. A `Value::Scalar` carrying any real dimension is still
/// rejected: that would mean the cell had silently acquired units.
pub(super) fn entity_real(values: &ValueMap, file: &str, entity: &str, cell: &str) -> f64 {
    let id = ValueCellId::new(entity, cell);
    match values.get(&id) {
        Some(Value::Real(v)) => *v,
        Some(Value::Scalar {
            si_value,
            dimension,
        }) if *dimension == DimensionVector::DIMENSIONLESS => *si_value,
        other => panic!(
            "{entity}.{cell} must be a dimensionless real (a count or a ratio), i.e. a \
             `Value::Real` or a DIMENSIONLESS `Value::Scalar`, got {other:?} — is the \
             cell declared in {file}, and is it still `: Real`?"
        ),
    }
}

/// A compiled module's value cells indexed by SOURCE SPAN — the table that maps
/// a diagnostic back to the `<entity>.<cell>` it is about.
///
/// The emission carries the offending cell's `span` as its label
/// (`crates/reify-eval/src/engine_eval.rs`), so the identity is recovered
/// structurally. Nothing here reads a message: the prose belongs to another
/// crate, and a rewording of it must not move a diagnostic between a caller's
/// allowlist arms. Build this from the SAME compilation the diagnostics came
/// from and no caller hard-codes a byte offset, so an edit elsewhere in the file
/// cannot shift an identity.
///
/// Two lookups, because the two diagnostic populations this unit allowlists
/// label differently — see [`Self::exact`] and [`Self::smallest_containing`].
/// Both scan: the table is a few hundred cells and the Error diagnostics a
/// handful, so an index would cost more to build than it saves.
pub(super) struct CellSpans(Vec<(SourceSpan, String)>);

impl CellSpans {
    pub(super) fn of(module: &reify_compiler::CompiledModule) -> Self {
        Self(
            module
                .templates
                .iter()
                .flat_map(|t| t.value_cells.iter())
                .map(|c| (c.span, format!("{}.{}", c.id.entity, c.id.member)))
                .collect(),
        )
    }

    /// The cell whose span IS the diagnostic's label span — for an emission
    /// labelled on the cell itself, which is how the evaluator reports an
    /// unresolvable cell.
    pub(super) fn exact(&self, d: &Diagnostic) -> Option<String> {
        let sp = d.labels.first()?.span;
        self.0
            .iter()
            .find(|(cs, _)| *cs == sp)
            .map(|(_, n)| n.clone())
    }

    /// The SMALLEST cell whose span CONTAINS the diagnostic's label span — for
    /// an emission labelled on an expression INSIDE a cell, which is how the
    /// compiler reports an unresolved name.
    pub(super) fn smallest_containing(&self, d: &Diagnostic) -> Option<String> {
        let sp = d.labels.first()?.span;
        self.0
            .iter()
            .filter(|(cs, _)| cs.start <= sp.start && sp.end <= cs.end)
            .min_by_key(|(cs, _)| cs.end - cs.start)
            .map(|(_, n)| n.clone())
    }
}

/// How strictly [`assert_constraints_ok`] reads a set of constraint results.
///
/// **This is the unit's one statement of why a constraint failure is not a
/// fixture's business**, and every other site links here rather than re-deriving
/// it — nothing executable checks a restatement, so copies go stale. This enum
/// is the natural home because it is the axis that exists *because* of it.
///
/// How `SimpleConstraintChecker` reports a failure alongside the typed
/// `Satisfaction` result is that crate's business: see
/// `crates/reify-constraints/src/lib.rs`. The one consequence the gates act on
/// is that a failure reaches the diagnostics as `ConstraintViolated`
/// (`Violated`) or as a mere WARNING (`Indeterminate`) — so every fixture in
/// this unit routes the former OUT of its Error filter and none can see the
/// latter at all.
///
/// Routed out because a violated constraint is a DESIGN failure, not a pipeline
/// one. Left in, it panics the shared fixture first — in every test at once,
/// including the pure-geometry ones that have nothing to do with the relation
/// that broke — under a message about evaluation or geometry that is false for
/// that failure, and it shadows the diagnosis [`assert_constraints_ok`] exists
/// to give (WHICH relation, at what strictness).
///
/// So NEITHER failure is a fixture's business: the satisfaction gates own both,
/// and they read `constraint_results` directly rather than the diagnostics —
/// which is what keeps their claims true however the checker chooses to report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Strictness {
    /// Every result must be `Satisfied` — `Indeterminate` fails too.
    ///
    /// `Indeterminate` is what a constraint whose inputs failed to EVALUATE
    /// reports: an undef leaf, or a cross-`sub` field reference that did not
    /// resolve. It is therefore the failure mode a `Violated`-only filter is
    /// blindest to — the constraint is still declared, still reported, and
    /// checking nothing — and per the enum doc above it reaches the diagnostics
    /// only as a WARNING, so no Error filter in this unit sees it either. This
    /// strictness is the only claim that catches it.
    AllSatisfied,
    /// Only `Violated` fails — the weaker statement `reify check` itself makes.
    NoneViolated,
}

/// Assert a `constraint_results` set is non-empty and holds at the strictness
/// asked for, optionally scoped to one entity; returns the entries examined.
///
/// One helper rather than a copy per site because every constraint claim in this
/// unit is the same two-step statement, and the first step is the one that
/// silently rots when copied:
///
///   * **Non-emptiness first.** A satisfaction filter over an empty set is
///     vacuously green, and an empty `constraint_results` emits NO diagnostic,
///     so nothing else in the unit can see it. That guard has to hold at every
///     site or the site that lost it stops asserting anything at all.
///   * **Then the satisfaction filter**, at [`Strictness`] — the axis that
///     genuinely differs between the sites, so it is a parameter rather than
///     several hand-written filters that could drift apart.
///
/// `file` names the design file, `surface` which evaluation surface the entries
/// came from and `note` carries the site's own mechanical reading of a failure;
/// all three are only ever message text.
pub(super) fn assert_constraints_ok<'a>(
    entries: &'a [ConstraintCheckEntry],
    file: &str,
    scope: Option<&str>,
    strictness: Strictness,
    surface: &str,
    note: &str,
) -> Vec<&'a ConstraintCheckEntry> {
    let scoped: Vec<&ConstraintCheckEntry> = match scope {
        Some(entity) => entries.iter().filter(|c| c.id.entity == entity).collect(),
        None => entries.iter().collect(),
    };
    let what = match scope {
        Some(entity) => format!("`{entity}` constraint results"),
        None => "constraint results".to_string(),
    };

    assert!(
        !scoped.is_empty(),
        "no {what} at all on {surface} of {file} — every scope this unit reads \
         declares constraints, so an empty set means the check never ran, or \
         stopped covering this scope, and the satisfaction filter would then pass \
         vacuously. {note} Entities checked: {:?}",
        entries.iter().map(|c| &c.id.entity).collect::<Vec<_>>()
    );

    let bad: Vec<_> = scoped
        .iter()
        .filter(|c| match strictness {
            Strictness::AllSatisfied => c.satisfaction != Satisfaction::Satisfied,
            Strictness::NoneViolated => c.satisfaction == Satisfaction::Violated,
        })
        .collect();
    assert!(
        bad.is_empty(),
        "{file} must satisfy {what} at its defaults on {surface} — {} of {} did \
         not, at {strictness:?} strictness. `Violated` means the design broke the \
         relation; `Indeterminate` means an input cell failed to EVALUATE, so the \
         constraint is present but checking nothing. {note} Results: {bad:#?}",
        bad.len(),
        scoped.len()
    );

    scoped
}
