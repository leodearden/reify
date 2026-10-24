//! The force-density input contract shared by every form-finding entry
//! (solver-crates-layering D1/D2, leaf η), driven through public seams only.

use reify_solver_elastic::{
    AnisoFormFindError, AnisotropicSurfaceStress, ForceDensityInputError, ForceDensitySpec,
    FormFindError, FreeFormError, GroupSpecDefect, IndexedInput, MemberKind, form_find_anchored,
    form_find_anchored_surfaces, form_find_anchored_surfaces_aniso, form_find_free,
    form_find_free_surfaces,
};
use reify_test_support::tensegrity_fixtures::{TRIPLEX_MEMBERS, TRIPLEX_SEEDS, TRIPLEX_STRUTS};

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

// ── free-standing family ─────────────────────────────────────────────────────

/// Group ids in `TRIPLEX_MEMBERS` order: struts, horizontal cables, vertical cables.
const FD_TRIPLEX_GROUP_IDS: [usize; 12] = [0, 0, 0, 1, 1, 1, 1, 1, 1, 2, 2, 2];

/// One free-standing form-find input, posed identically to the line-only and
/// the surface-aware entry.
struct FdFreeCase {
    nodes: Vec<[f64; 3]>,
    members: Vec<(usize, usize)>,
    kinds: Vec<MemberKind>,
    spec: ForceDensitySpec,
    surfaces: Vec<(usize, usize, usize)>,
    sigma: Vec<f64>,
}

impl FdFreeCase {
    /// The triplex topology at a non-degenerate initial guess (a free-standing
    /// solve only gauge-fixes against it), with one membrane triangle over the
    /// top ring and the triplex's line-only self-stress as explicit densities.
    fn explicit() -> Self {
        let sqrt3 = 3.0_f64.sqrt();
        let q = FD_TRIPLEX_GROUP_IDS
            .iter()
            .map(|&group| [-sqrt3, 1.0, sqrt3][group])
            .collect();
        Self {
            nodes: vec![
                [1.0, 0.0, 1.0],
                [-0.5, 0.9, 1.0],
                [-0.5, -0.9, 1.0],
                [0.9, 0.5, 0.0],
                [-0.9, 0.5, 0.0],
                [0.0, -1.0, 0.0],
            ],
            members: TRIPLEX_MEMBERS
                .iter()
                .map(|&[j, k]| (j as usize, k as usize))
                .collect(),
            kinds: (0..TRIPLEX_MEMBERS.len())
                .map(|i| {
                    if i < TRIPLEX_STRUTS {
                        MemberKind::Strut
                    } else {
                        MemberKind::Cable
                    }
                })
                .collect(),
            spec: ForceDensitySpec::Explicit(q),
            surfaces: vec![(0, 1, 2)],
            sigma: vec![0.05],
        }
    }

    /// [`FdFreeCase::explicit`] searching the triplex groups from its seeds instead.
    fn group_ratios() -> Self {
        Self {
            spec: ForceDensitySpec::GroupRatios {
                group_ids: FD_TRIPLEX_GROUP_IDS.to_vec(),
                seed_ratios: TRIPLEX_SEEDS.to_vec(),
                reference_group: 1,
            },
            ..Self::explicit()
        }
    }

    fn node_count(&self) -> usize {
        self.nodes.len()
    }

    fn explicit_q(&mut self) -> &mut Vec<f64> {
        match &mut self.spec {
            ForceDensitySpec::Explicit(q) => q,
            ForceDensitySpec::GroupRatios { .. } => panic!("not an explicit-q case"),
        }
    }

    fn line_only(&self) -> Result<(), FreeFormError> {
        form_find_free(&self.nodes, &self.members, &self.kinds, &self.spec).map(|_| ())
    }

    fn with_surfaces(&self) -> Result<(), FreeFormError> {
        form_find_free_surfaces(
            &self.nodes,
            &self.members,
            &self.kinds,
            &self.surfaces,
            &self.sigma,
            &self.spec,
        )
        .map(|_| ())
    }

    /// Both free-standing entries reject this input with the same `expected`.
    fn assert_both_entries_reject(&self, expected: ForceDensityInputError) {
        assert_eq!(
            self.line_only(),
            Err(FreeFormError::Input(expected)),
            "line-only"
        );
        assert_eq!(
            self.with_surfaces(),
            Err(FreeFormError::Input(expected)),
            "with surfaces"
        );
    }
}

#[test]
fn the_free_fixtures_satisfy_the_input_contract() {
    for case in [FdFreeCase::explicit(), FdFreeCase::group_ratios()] {
        assert_eq!(case.line_only(), Ok(()));
        let combined = case.with_surfaces();
        assert!(
            !matches!(combined, Err(FreeFormError::Input(_))),
            "the unperturbed fixture is valid input, got {combined:?}"
        );
    }
}

#[test]
fn a_free_member_endpoint_past_the_nodes_is_an_indexed_input_error() {
    for mut case in [FdFreeCase::explicit(), FdFreeCase::group_ratios()] {
        let n = case.node_count();
        case.members[1] = (1, n);
        case.assert_both_entries_reject(ForceDensityInputError::IndexOutOfRange {
            what: IndexedInput::MemberEndpoint { member: 1 },
            index: n,
            len: n,
        });
    }
}

#[test]
fn a_free_surface_corner_past_the_nodes_is_an_indexed_input_error() {
    let mut case = FdFreeCase::explicit();
    let n = case.node_count();
    case.surfaces[0] = (0, 1, n);
    assert_eq!(
        case.with_surfaces(),
        Err(FreeFormError::Input(
            ForceDensityInputError::IndexOutOfRange {
                what: IndexedInput::SurfaceCorner { surface: 0 },
                index: n,
                len: n,
            }
        ))
    );
}

#[test]
fn an_explicit_density_against_its_kind_is_one_sign_violation_on_both_free_entries() {
    let violations = [
        (MemberKind::Cable, 0.0),
        (MemberKind::Cable, -1.0),
        (MemberKind::Cable, f64::NAN),
        (MemberKind::Strut, 0.0),
        (MemberKind::Strut, 1.0),
        (MemberKind::Strut, f64::NAN),
    ];
    for (kind, q) in violations {
        let mut case = FdFreeCase::explicit();
        case.kinds[1] = kind;
        case.explicit_q()[1] = q;
        case.assert_both_entries_reject(ForceDensityInputError::SignViolation { member: 1 });
    }
}

#[test]
fn a_seed_against_its_group_kind_is_a_sign_violation_before_any_search() {
    // Group 0 holds the struts, members 0..3: a tensile or NaN seed for it
    // contradicts the first strut's kind.
    for strut_seed in [1.0, f64::NAN] {
        let mut case = FdFreeCase::group_ratios();
        if let ForceDensitySpec::GroupRatios { seed_ratios, .. } = &mut case.spec {
            seed_ratios[0] = strut_seed;
        }
        case.assert_both_entries_reject(ForceDensityInputError::SignViolation { member: 0 });
    }
}

#[test]
fn explicit_densities_shorter_than_the_members_are_a_dimension_mismatch() {
    let mut case = FdFreeCase::explicit();
    case.explicit_q().pop();
    case.assert_both_entries_reject(ForceDensityInputError::DimensionMismatch);
}

#[test]
fn a_malformed_group_spec_names_its_defect_on_both_free_entries() {
    let ids = FD_TRIPLEX_GROUP_IDS.to_vec();
    let seeds = TRIPLEX_SEEDS.to_vec();
    let unseeded_group = seeds.len();
    let short_ids = ids[..ids.len() - 1].to_vec();
    let mut ids_naming_an_unseeded_group = ids.clone();
    ids_naming_an_unseeded_group[11] = unseeded_group;
    let mut zero_seed = seeds.clone();
    zero_seed[2] = 0.0;
    let malformed = [
        (
            (short_ids, seeds.clone(), 1),
            GroupSpecDefect::GroupIdsNotPerMember,
        ),
        (
            (ids_naming_an_unseeded_group, seeds.clone(), 1),
            GroupSpecDefect::UnseededGroup { member: 11 },
        ),
        (
            (ids.clone(), seeds, unseeded_group),
            GroupSpecDefect::UnseededReferenceGroup,
        ),
        ((ids, zero_seed, 1), GroupSpecDefect::ZeroSeed { group: 2 }),
    ];
    for ((group_ids, seed_ratios, reference_group), defect) in malformed {
        let case = FdFreeCase {
            spec: ForceDensitySpec::GroupRatios {
                group_ids,
                seed_ratios,
                reference_group,
            },
            ..FdFreeCase::explicit()
        };
        let expected = Err(FreeFormError::MalformedGroupSpec(defect));
        assert_eq!(case.line_only(), expected, "line-only");
        assert_eq!(case.with_surfaces(), expected, "with surfaces");
    }
}

#[test]
fn a_group_spec_with_no_seeds_has_an_unseeded_reference_group() {
    let case = FdFreeCase {
        spec: ForceDensitySpec::GroupRatios {
            group_ids: Vec::new(),
            seed_ratios: Vec::new(),
            reference_group: 0,
        },
        members: Vec::new(),
        kinds: Vec::new(),
        surfaces: Vec::new(),
        sigma: Vec::new(),
        ..FdFreeCase::explicit()
    };
    let expected = Err(FreeFormError::MalformedGroupSpec(
        GroupSpecDefect::UnseededReferenceGroup,
    ));
    assert_eq!(case.line_only(), expected, "line-only");
    assert_eq!(case.with_surfaces(), expected, "with surfaces");
}

#[test]
fn free_surfaces_without_one_stress_each_are_a_surface_count_mismatch() {
    let mut case = FdFreeCase::explicit();
    case.sigma.push(0.05);
    assert_eq!(
        case.with_surfaces(),
        Err(FreeFormError::Input(
            ForceDensityInputError::SurfaceCountMismatch
        ))
    );
}

#[test]
fn a_free_surface_stress_that_is_not_tension_is_rejected_including_nan() {
    for sigma in [0.0, f64::NAN] {
        let mut case = FdFreeCase::explicit();
        case.sigma = vec![sigma];
        assert_eq!(
            case.with_surfaces(),
            Err(FreeFormError::Input(
                ForceDensityInputError::NonTensionSurfaceStress { surface: 0 }
            )),
            "σ = {sigma}"
        );
    }
}

#[test]
fn a_collinear_free_input_triangle_is_a_degenerate_triangle_input_error() {
    let mut case = FdFreeCase::explicit();
    let [a, b] = [case.nodes[0], case.nodes[1]];
    case.nodes[2] = [2.0 * b[0] - a[0], 2.0 * b[1] - a[1], 2.0 * b[2] - a[2]];
    assert_eq!(
        case.with_surfaces(),
        Err(FreeFormError::Input(
            ForceDensityInputError::DegenerateTriangle { surface: 0 }
        ))
    );
}
