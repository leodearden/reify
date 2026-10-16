//! The force-density input contract shared by every form-finding entry
//! (solver-crates-layering D1/D2, leaf η), driven through public seams only.

use reify_solver_elastic::{
    AnisoFormFindError, AnisotropicSurfaceStress, ForceDensityInputError, FormFindError,
    IndexedInput, MemberKind, form_find_anchored, form_find_anchored_surfaces,
    form_find_anchored_surfaces_aniso,
};

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

// ── anchored family ──────────────────────────────────────────────────────────

/// One anchored form-find input, posed identically to the line-only, isotropic
/// and anisotropic entries.
struct FdAnchoredCase {
    nodes: Vec<[f64; 3]>,
    members: Vec<(usize, usize)>,
    kinds: Vec<MemberKind>,
    q: Vec<f64>,
    surfaces: Vec<(usize, usize, usize)>,
    sigma: Vec<f64>,
    aniso: Vec<AnisotropicSurfaceStress>,
    anchors: Vec<usize>,
}

impl FdAnchoredCase {
    /// Three anchors and a free apex tied to each by a cable, with one membrane
    /// triangle spanning the anchors.
    fn valid() -> Self {
        Self {
            nodes: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.3, 0.3, 0.5],
            ],
            members: vec![(3, 0), (3, 1), (3, 2)],
            kinds: vec![MemberKind::Cable; 3],
            q: vec![1.0; 3],
            surfaces: vec![(0, 1, 2)],
            sigma: vec![1.0],
            aniso: vec![fd_aniso_stress(1.0, 1.0)],
            anchors: vec![0, 1, 2],
        }
    }

    fn node_count(&self) -> usize {
        self.nodes.len()
    }

    fn line_only(&self) -> Result<(), FormFindError> {
        form_find_anchored(
            &self.nodes,
            &self.members,
            &self.kinds,
            &self.q,
            &self.anchors,
        )
        .map(|_| ())
    }

    fn isotropic(&self) -> Result<(), FormFindError> {
        form_find_anchored_surfaces(
            &self.nodes,
            &self.members,
            &self.kinds,
            &self.q,
            &self.surfaces,
            &self.sigma,
            &self.anchors,
        )
        .map(|_| ())
    }

    fn anisotropic(&self) -> Result<(), AnisoFormFindError> {
        form_find_anchored_surfaces_aniso(
            &self.nodes,
            &self.members,
            &self.kinds,
            &self.q,
            &self.surfaces,
            &self.aniso,
            &self.anchors,
        )
        .map(|_| ())
    }

    /// Both surface-aware entries reject this input with the same `expected`.
    fn assert_surface_entries_reject(&self, expected: ForceDensityInputError) {
        assert_eq!(
            self.isotropic(),
            Err(FormFindError::Input(expected)),
            "isotropic"
        );
        assert_eq!(
            self.anisotropic(),
            Err(AnisoFormFindError::Input(expected)),
            "anisotropic"
        );
    }

    /// All three anchored entries reject this input with the same `expected`.
    fn assert_every_entry_rejects(&self, expected: ForceDensityInputError) {
        assert_eq!(
            self.line_only(),
            Err(FormFindError::Input(expected)),
            "line-only"
        );
        self.assert_surface_entries_reject(expected);
    }
}

fn fd_aniso_stress(sigma_warp: f64, sigma_weft: f64) -> AnisotropicSurfaceStress {
    AnisotropicSurfaceStress {
        warp_dir: [1.0, 0.0, 0.0],
        sigma_warp,
        sigma_weft,
    }
}

#[test]
fn the_anchored_fixture_solves_on_every_entry() {
    let case = FdAnchoredCase::valid();
    assert_eq!(case.line_only(), Ok(()));
    assert_eq!(case.isotropic(), Ok(()));
    assert_eq!(case.anisotropic(), Ok(()));
}

#[test]
fn a_member_endpoint_past_the_nodes_is_an_indexed_input_error() {
    let mut case = FdAnchoredCase::valid();
    let n = case.node_count();
    case.members[1] = (3, n);
    case.assert_every_entry_rejects(ForceDensityInputError::IndexOutOfRange {
        what: IndexedInput::MemberEndpoint { member: 1 },
        index: n,
        len: n,
    });
}

#[test]
fn an_anchor_past_the_nodes_is_an_indexed_input_error() {
    let mut case = FdAnchoredCase::valid();
    let n = case.node_count();
    case.anchors[2] = n;
    case.assert_every_entry_rejects(ForceDensityInputError::IndexOutOfRange {
        what: IndexedInput::Anchor { anchor: 2 },
        index: n,
        len: n,
    });
}

#[test]
fn a_surface_corner_past_the_nodes_is_an_indexed_input_error() {
    let mut case = FdAnchoredCase::valid();
    let n = case.node_count();
    case.surfaces[0] = (0, 1, n);
    case.assert_surface_entries_reject(ForceDensityInputError::IndexOutOfRange {
        what: IndexedInput::SurfaceCorner { surface: 0 },
        index: n,
        len: n,
    });
}

#[test]
fn a_density_against_its_kind_is_one_sign_violation_on_every_anchored_entry() {
    let violations = [
        (MemberKind::Cable, 0.0),
        (MemberKind::Cable, -1.0),
        (MemberKind::Cable, f64::NAN),
        (MemberKind::Strut, 0.0),
        (MemberKind::Strut, 1.0),
        (MemberKind::Strut, f64::NAN),
    ];
    for (kind, q) in violations {
        let mut case = FdAnchoredCase::valid();
        case.kinds[1] = kind;
        case.q[1] = q;
        case.assert_every_entry_rejects(ForceDensityInputError::SignViolation { member: 1 });
    }
}

#[test]
fn densities_shorter_than_the_members_are_a_dimension_mismatch() {
    let mut case = FdAnchoredCase::valid();
    case.q.pop();
    case.assert_every_entry_rejects(ForceDensityInputError::DimensionMismatch);
}

#[test]
fn surfaces_without_one_stress_each_are_a_surface_count_mismatch() {
    let mut case = FdAnchoredCase::valid();
    case.sigma.clear();
    case.aniso.clear();
    case.assert_surface_entries_reject(ForceDensityInputError::SurfaceCountMismatch);
}

#[test]
fn a_surface_stress_that_is_not_tension_is_rejected_including_nan() {
    let non_tension = ForceDensityInputError::NonTensionSurfaceStress { surface: 0 };
    for sigma in [0.0, -1.0, f64::NAN] {
        let mut case = FdAnchoredCase::valid();
        case.sigma = vec![sigma];
        assert_eq!(
            case.isotropic(),
            Err(FormFindError::Input(non_tension)),
            "σ = {sigma}"
        );
    }
    for stress in [fd_aniso_stress(0.0, 1.0), fd_aniso_stress(1.0, f64::NAN)] {
        let mut case = FdAnchoredCase::valid();
        case.aniso = vec![stress];
        assert_eq!(
            case.anisotropic(),
            Err(AnisoFormFindError::Input(non_tension))
        );
    }
}

#[test]
fn a_collinear_input_triangle_is_a_degenerate_triangle_input_error() {
    let mut case = FdAnchoredCase::valid();
    case.nodes[2] = [2.0, 0.0, 0.0];
    case.assert_surface_entries_reject(ForceDensityInputError::DegenerateTriangle { surface: 0 });
}

#[test]
fn a_triangle_that_degenerates_during_the_fixed_point_is_a_solver_error() {
    // Valid at input; the first reduced solve puts the free apex exactly at
    // (1, 0, 0), on the anchored edge, so the next assembly sees zero area.
    let case = FdAnchoredCase {
        nodes: vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
        members: vec![],
        kinds: vec![],
        q: vec![],
        surfaces: vec![(0, 1, 2)],
        sigma: vec![1.0],
        aniso: vec![fd_aniso_stress(1.0, 1.0)],
        anchors: vec![0, 1],
    };
    assert_eq!(case.isotropic(), Err(FormFindError::DegenerateTriangle));
    assert_eq!(
        case.anisotropic(),
        Err(AnisoFormFindError::DegenerateTriangle)
    );
}
