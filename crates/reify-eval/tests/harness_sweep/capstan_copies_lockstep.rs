//! Lockstep gate for printer_v01's two copies of the `Capstan` structure
//! (#7215): `prj/printer_v01/printer.ri` embeds a copy of the drum because v0.1
//! has no cross-file import.
//!
//! **dev_capstan.ri is the ORIGINAL, printer.ri the copy** — the reverse of
//! `IdlerPulley`, whose lockstep gate lives in [`super::idler_seat_e2e`]. So
//! every failure here says to bring printer.ri to dev_capstan.ri.
//!
//! **Why member by member, not one template hash.** The sketch deliberately
//! carries cells the copy must not ([`SKETCH_ONLY_CELLS`],
//! [`NOT_YET_PORTED_CELLS`]), so the two `TopologyTemplate::content_hash`es
//! differ by construction. What the compiler folds into that hash for this
//! structure is each value cell's default-expression hash and each constraint's
//! expression hash, plus collections that are empty in both copies (claim 0 of
//! [`capstan_copies_declare_the_same_cells`] holds them empty). Comparing those
//! same per-member hashes over the SHARED cells makes the template-hash claim
//! restricted to what the copies share — span- and doc-independent — and names
//! the cell or relation that forked instead of just the file.
//!
//! **What it licenses.** printer.ri's `Capstan` equals the sketch's minus the
//! named omissions, and [`super::capstan_groove_e2e`] gates the sketch's copy
//! (the DIN 15061 ratio, the `seat_c` identity, the mouth, the mesh and the
//! removed volume), so printer.ri's drum inherits those gates. An edit applied
//! to BOTH copies in lockstep keeps this module green; the sketch gates are
//! what catch it.
//!
//! Kernel-free throughout: it reads compiled templates and printer.ri's check
//! surface only, because printer.ri must not be tessellated (#7383).

use std::collections::BTreeSet;

use reify_compiler::{CompiledConstraint, SolverHintKind, TopologyTemplate, ValueCellDecl};

use super::capstan_groove_e2e::{CAPSTAN_ENTITY, DEV_CAPSTAN, dev_capstan_compiled};
use super::design_fixture::{Strictness, assert_constraints_ok, entity_template};
use super::idler_seat_e2e::{PRINTER_RI, printer_checked, printer_compiled};

/// Sketch cells printer.ri's copy must never carry. `volume()` is the sketch's
/// machining metric: on printer.ri's kernel-free surface these would have to
/// join `idler_seat_e2e`'s `PRINTER_VOLUME_UNRESOLVED`, and under a kernel they
/// would cost two drum-volume evaluations per `CapstanUnit`.
const SKETCH_ONLY_CELLS: &[&str] = &["blank_volume", "body_volume"];

/// Sketch cells printer.ri has not ported yet. #7238 owns porting `band`
/// together with printer.ri's stale 88 mm "groove band" Sizing prose; whoever
/// ports it deletes the entry here in the same diff.
const NOT_YET_PORTED_CELLS: &[&str] = &["band"];

fn sketch_capstan() -> &'static TopologyTemplate {
    entity_template(dev_capstan_compiled(), DEV_CAPSTAN, CAPSTAN_ENTITY)
}

fn printer_capstan() -> &'static TopologyTemplate {
    entity_template(printer_compiled(), PRINTER_RI, CAPSTAN_ENTITY)
}

fn cell_names(template: &TopologyTemplate) -> BTreeSet<String> {
    template
        .value_cells
        .iter()
        .map(|c| c.id.member.clone())
        .collect()
}

/// A cell's solver hints without their source spans, which differ between the
/// two files by construction.
fn solver_hints(cell: &ValueCellDecl) -> impl Iterator<Item = (SolverHintKind, &str)> {
    cell.solver_hints
        .iter()
        .map(|h| (h.kind, h.collection.as_str()))
}

fn is_omitted(cell: &str) -> bool {
    SKETCH_ONLY_CELLS.contains(&cell) || NOT_YET_PORTED_CELLS.contains(&cell)
}

/// The template collections the compiler folds into a structure's hash that
/// this module does NOT compare, each paired with whether it is empty.
fn uncompared_collections(t: &TopologyTemplate) -> [(&'static str, bool); 7] {
    [
        ("sub_components", t.sub_components.is_empty()),
        ("guarded_groups", t.guarded_groups.is_empty()),
        ("relations", t.relations.is_empty()),
        ("ports", t.ports.is_empty()),
        ("connections", t.connections.is_empty()),
        ("meta", t.meta.is_empty()),
        ("pragmas", t.pragmas.is_empty()),
    ]
}

/// Three claims, in the order that localizes a failure best:
///   0. coverage — both templates hold every collection this module does not
///      compare EMPTY, which is what makes the per-cell and per-constraint
///      comparisons in this module exhaustive over the structure's hash inputs;
///   1. the omission lists are live — each entry is a sketch cell, so a stale
///      entry cannot linger as a silent permission;
///   2. exact inventory — printer.ri's cells are the sketch's minus the
///      omission lists, in both directions.
#[test]
fn capstan_copies_declare_the_same_cells() {
    for (file, template) in [
        (DEV_CAPSTAN, sketch_capstan()),
        (PRINTER_RI, printer_capstan()),
    ] {
        let non_empty: Vec<&str> = uncompared_collections(template)
            .into_iter()
            .filter(|(_, empty)| !empty)
            .map(|(name, _)| name)
            .collect();
        assert!(
            non_empty.is_empty(),
            "`{CAPSTAN_ENTITY}` in {file} now has non-empty {non_empty:?}. The \
             compiler folds these into the structure's content hash, and this \
             module compares only value cells and constraints — so a fork inside \
             them would pass it. Extend this gate to compare them before relying \
             on it."
        );
    }

    let sketch = cell_names(sketch_capstan());
    let stale: Vec<&str> = SKETCH_ONLY_CELLS
        .iter()
        .chain(NOT_YET_PORTED_CELLS)
        .copied()
        .filter(|c| !sketch.contains(*c))
        .collect();
    assert!(
        stale.is_empty(),
        "{stale:?} are listed as omissions but {DEV_CAPSTAN}'s `{CAPSTAN_ENTITY}` \
         declares no such cell. A stale omission is a silent permission for \
         printer.ri to diverge; delete it."
    );

    let expected: BTreeSet<String> = sketch.into_iter().filter(|c| !is_omitted(c)).collect();
    let printer = cell_names(printer_capstan());
    let missing: Vec<&String> = expected.difference(&printer).collect();
    let extra: Vec<&String> = printer.difference(&expected).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "printer.ri's `{CAPSTAN_ENTITY}` has FORKED from dev_capstan.ri's, which is \
         the original: printer.ri must declare exactly the sketch's cells minus \
         SKETCH_ONLY_CELLS {SKETCH_ONLY_CELLS:?} and NOT_YET_PORTED_CELLS \
         {NOT_YET_PORTED_CELLS:?}.\n\
         In dev_capstan.ri, missing from printer.ri: {missing:?}\n\
         In printer.ri, not in dev_capstan.ri (or listed as omitted): {extra:?}\n\
         Bring printer.ri to dev_capstan.ri. If you have just ported an omitted \
         cell, delete its entry from the omission list in the same diff."
    );
}

/// Every cell both copies declare must be the same cell: the same kind,
/// visibility, aux flag, declared type and solver hints, and a default
/// expression with the same structural hash. Only the expression is a template
/// hash input; the rest are compared because the hash cannot see them, and a
/// `pub` on one copy's cell changes what a composing structure may read. The
/// expression hash carries no span and no doc, and hints are compared without
/// their spans, so a comment or formatting difference can never trip this — and
/// the copies deliberately differ in comments: printer.ri's are abridged,
/// pointing at dev_capstan.ri.
#[test]
fn capstan_copies_compute_every_shared_cell_alike() {
    let printer = printer_capstan();
    let mut shared = 0usize;
    let mut forked: Vec<String> = Vec::new();

    for s in &sketch_capstan().value_cells {
        let Some(p) = printer
            .value_cells
            .iter()
            .find(|p| p.id.member == s.id.member)
        else {
            continue;
        };
        shared += 1;
        let aspects: Vec<&str> = [
            ("kind", s.kind != p.kind),
            ("visibility", s.visibility != p.visibility),
            ("aux flag", s.is_aux != p.is_aux),
            ("declared type", s.cell_type != p.cell_type),
            ("solver hints", solver_hints(s).ne(solver_hints(p))),
            (
                "expression",
                s.default_expr.as_ref().map(|e| e.content_hash)
                    != p.default_expr.as_ref().map(|e| e.content_hash),
            ),
        ]
        .into_iter()
        .filter(|(_, differs)| *differs)
        .map(|(aspect, _)| aspect)
        .collect();
        if !aspects.is_empty() {
            forked.push(format!("{} ({})", s.id.member, aspects.join(", ")));
        }
    }

    assert!(
        shared > 0,
        "the two `{CAPSTAN_ENTITY}` copies share no cell at all, so this comparison \
         would pass vacuously — see `capstan_copies_declare_the_same_cells`."
    );
    assert!(
        forked.is_empty(),
        "printer.ri's `{CAPSTAN_ENTITY}` computes these cells differently from \
         dev_capstan.ri's, which is the original: {forked:#?}\n\
         The expression comparison is a structural hash with no span or doc in \
         it, so this is a CODE difference, not a comment or formatting one. \
         Bring printer.ri's cell to dev_capstan.ri's, token for token."
    );
}

/// Both copies must state the same constraints — compared as a multiset of
/// structural expression hashes, so order and comments are free — and
/// printer.ri's must actually HOLD on printer.ri's own check surface.
///
/// The second half is a direct claim on the production file rather than one
/// inherited through hash equality, so an evaluation-context difference the
/// hash cannot see is still caught. It is strict because every `Capstan`
/// constraint reads scalar cells only, which makes each one decidable without a
/// kernel; it is scoped to `Capstan` because printer.ri has unrelated
/// constraints that are not.
#[test]
fn capstan_copies_state_the_same_constraints() {
    let sketch = sketch_capstan();
    let printer = printer_capstan();
    let sketch_only = unmatched_constraints(sketch, printer);
    let printer_only = unmatched_constraints(printer, sketch);
    assert!(
        sketch_only.is_empty() && printer_only.is_empty(),
        "printer.ri's `{CAPSTAN_ENTITY}` states different constraints from \
         dev_capstan.ri's, which is the original ({} vs {}). Each unmatched \
         constraint is shown by the `{CAPSTAN_ENTITY}` cells it reads.\n\
         In dev_capstan.ri, not in printer.ri: {sketch_only:#?}\n\
         In printer.ri, not in dev_capstan.ri: {printer_only:#?}\n\
         Bring printer.ri's constraints to dev_capstan.ri's, token for token.",
        sketch.constraints.len(),
        printer.constraints.len(),
    );

    assert_constraints_ok(
        &printer_checked().constraint_results,
        PRINTER_RI,
        Some(CAPSTAN_ENTITY),
        Strictness::AllSatisfied,
        "the kernel-free check surface",
        "`Capstan`'s constraints read scalar cells only, so all of them are \
         decidable without a kernel: an `Indeterminate` here means an input cell \
         stopped evaluating, NOT that a kernel was needed.",
    );
}

/// The constraints of `from` with no hash-equal partner left in `against`,
/// matched as a multiset, each described by the `Capstan` cells it reads.
fn unmatched_constraints(from: &TopologyTemplate, against: &TopologyTemplate) -> Vec<String> {
    let mut unpaired: Vec<u128> = against
        .constraints
        .iter()
        .map(|c| c.expr.content_hash.0)
        .collect();
    let mut unmatched = Vec::new();
    for c in &from.constraints {
        match unpaired.iter().position(|h| *h == c.expr.content_hash.0) {
            Some(i) => {
                unpaired.swap_remove(i);
            }
            None => unmatched.push(describe_constraint(c)),
        }
    }
    unmatched
}

fn describe_constraint(c: &CompiledConstraint) -> String {
    let reads: BTreeSet<String> = c
        .expr
        .collect_value_refs()
        .into_iter()
        .filter(|id| id.entity == CAPSTAN_ENTITY)
        .map(|id| id.member)
        .collect();
    match &c.label {
        Some(label) => format!("`{label}` reading {reads:?}"),
        None => format!("constraint reading {reads:?}"),
    }
}
