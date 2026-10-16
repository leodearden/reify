//! The force-density input contract shared by every form-finding entry
//! (solver-crates-layering D1/D2, leaf η), driven through public seams only.

use reify_solver_elastic::MemberKind;

#[test]
fn cable_admits_exactly_positive_densities() {
    for q in [1.0, f64::MIN_POSITIVE, f64::INFINITY] {
        assert!(MemberKind::Cable.admits(q), "a cable must admit q = {q}");
    }
    for q in [0.0, -0.0, -1.0, f64::NAN] {
        assert!(!MemberKind::Cable.admits(q), "a cable must reject q = {q}");
    }
}

#[test]
fn strut_admits_exactly_negative_densities() {
    for q in [-1.0, -f64::MIN_POSITIVE, f64::NEG_INFINITY] {
        assert!(MemberKind::Strut.admits(q), "a strut must admit q = {q}");
    }
    for q in [0.0, -0.0, 1.0, f64::NAN] {
        assert!(!MemberKind::Strut.admits(q), "a strut must reject q = {q}");
    }
}
