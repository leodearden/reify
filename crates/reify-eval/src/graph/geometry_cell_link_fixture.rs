//! A template holding each shape of geometry cell the scalar-geometry-cell →
//! realization link must tell apart, shared by the tests of
//! `EvaluationGraph::scalar_geometry_cell_realization` and of the instance mint
//! that resolves through it (task #9846).

use reify_core::ValueCellId;

use super::EvaluationGraph;

/// `T` holds two scalar geometry lets and two geometry-list lets, and `P`
/// stamps it as the sub instance `P.s`. `a` is a box over literals. `b`
/// consumes the guarded let `core`, which the compiler inlines into `b`'s
/// realization, so `b`'s operations read `w` while `b`'s expression does not.
/// `one` and `holes` are lists of length 1 and 3.
pub(crate) const SRC: &str = r#"structure T {
    param w : Length = 10mm
    param on : Bool = true
    where on {
        let core = box(w, 10mm, 10mm)
    }
    let a = box(10mm, 10mm, 10mm)
    let b = translate(core, 1mm, 0mm, 0mm)
    let one = generate(1, |i| cylinder(5mm, 20mm))
    let holes = generate(3, |i| cylinder(5mm, 20mm))
}
structure P {
    sub s = T()
}"#;

/// The members of `T` in [`SRC`] that are scalar geometry lets.
pub(crate) const SCALAR_GEOMETRY_MEMBERS: [&str; 2] = ["a", "b"];

/// The members of `T` in [`SRC`] that are geometry-list lets.
pub(crate) const GEOMETRY_LIST_MEMBERS: [&str; 2] = ["one", "holes"];

/// Panics unless some realization of `graph` links `cell`. A geometry-list cell
/// is linked N:1 (task #5385), and a claim that it resolves to no realization
/// is vacuous unless it is linked.
pub(crate) fn assert_some_realization_links(graph: &EvaluationGraph, cell: &ValueCellId) {
    assert!(
        graph
            .realizations
            .values()
            .any(|realization| realization.geometry_cell.as_ref() == Some(cell)),
        "precondition: some realization must link `{cell}` (task #5385's list link), \
         or a claim that it resolves to no realization is vacuous"
    );
}
