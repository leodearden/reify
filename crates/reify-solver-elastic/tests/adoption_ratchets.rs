//! Adoption-ratchet host for reify-solver-elastic (PRD
//! `docs/prds/v0_6/solver-crates-layering.md` D20, §9 α, BT14).
//!
//! Each row counts the inline copies of a primitive that a later leaf extracts,
//! and the count must equal the row's committed baseline. The engine, its
//! self-tests and the contract this host keeps are
//! `reify_test_support::adoption_ratchet`.

use reify_test_support::adoption_ratchet::{
    self, Host, Measure, Pattern, Ratchet, TEST_ITEMS, View,
};

const HOST: Host = Host {
    crate_root: env!("CARGO_MANIFEST_DIR"),
    file: file!(),
    path_in_crate: "tests/adoption_ratchets.rs",
    decision: "docs/prds/v0_6/solver-crates-layering.md D20",
};

const SOLVE_REDUCED_DEFINITION_AND_TEST_ITEMS: &[&str] = &[TEST_ITEMS[0], "fn solve_reduced"];

const SRC: &[&str] = &["src/"];
const SRC_AND_TESTS: &[&str] = &["src/", "tests/"];

const RATCHETS: &[Ratchet] = &[
    Ratchet {
        name: "shell-assembly-upward-imports",
        home: "elements/shell_frame.rs (solver-crates-layering D4, leaf β)",
        baseline: 0,
        measure: Measure::Occurrences(Pattern::production(
            "shell_assembly::",
            &[
                "src/elements/",
                "src/geometric_stiffness/",
                "src/shell_kinematics.rs",
                "src/membrane_load.rs",
            ],
            "use crate::shell_assembly::plane_stress_d;",
        )),
    },
    Ratchet {
        name: "bubble-condensation-copies",
        home: "the one guarded bubble-condensation fn (solver-crates-layering D6, leaf γ)",
        baseline: 1,
        measure: Measure::Occurrences(Pattern::production(
            "let det_bb =",
            SRC,
            "let det_bb = k00 * k11 - k01 * k10;",
        )),
    },
    Ratchet {
        name: "shear-correction-literal",
        home: "ShellSection::SHEAR_CORRECTION (solver-crates-layering D5, leaf γ)",
        baseline: 1,
        measure: Measure::Occurrences(Pattern::production(
            "5.0 / 6.0",
            SRC,
            "const KAPPA: f64 = 5.0 / 6.0;",
        )),
    },
    Ratchet {
        name: "inline-shear-modulus",
        home: "IsotropicElastic::shear_modulus (solver-crates-layering D5, leaf γ)",
        baseline: 1,
        measure: Measure::Occurrences(Pattern::whole_file(
            "/ (2.0 * (1.0 + nu))",
            SRC,
            "let g = e / (2.0 * (1.0 + nu));",
        )),
    },
    Ratchet {
        name: "thickness-scaled-d-blocks",
        home: "the shared scaled(d, s) helper (solver-crates-layering D6, leaf γ)",
        baseline: 0,
        measure: Measure::Occurrences(Pattern::production(
            "td[i][j] = ",
            SRC,
            "td[i][j] = t * d[i][j];",
        )),
    },
    Ratchet {
        name: "assembly-element-literals",
        home: "AssemblyElement::new (solver-crates-layering D7, leaf δ)",
        baseline: 39,
        measure: Measure::Occurrences(Pattern {
            exclude: &["src/assembly/global.rs"],
            ..Pattern::whole_file(
                "AssemblyElement {",
                SRC_AND_TESTS,
                "let el = AssemblyElement { id: 0, connectivity: &c, k_e: &k };",
            )
        }),
    },
    Ratchet {
        name: "element-stiffness-data-indexing",
        home: "the ElementStiffness accessors (solver-crates-layering D8, leaf ε)",
        baseline: 74,
        measure: Measure::Occurrences(Pattern {
            exclude: &["src/assembly/mod.rs", "src/resample.rs"],
            ..Pattern::whole_file(".data[", SRC_AND_TESTS, "let v = k_e.data[0];")
        }),
    },
    Ratchet {
        name: "member-force-delta-definitions",
        home: "the membrane_load helper shared with tensegrity_load (solver-crates-layering D9, leaf ζ)",
        baseline: 2,
        measure: Measure::Occurrences(Pattern::production(
            "fn member_force_delta(",
            SRC,
            "fn member_force_delta(x: f64) -> f64 { x }",
        )),
    },
    Ratchet {
        name: "bar-axial-force-delta-definitions",
        home: "the membrane_load helper shared with tensegrity_load (solver-crates-layering D9, leaf ζ)",
        baseline: 2,
        measure: Measure::Occurrences(Pattern::production(
            "fn bar_axial_force_delta(",
            SRC,
            "fn bar_axial_force_delta(x: f64) -> f64 { x }",
        )),
    },
    Ratchet {
        name: "grounding-diagonal-definitions",
        home: "the membrane_load constant shared with tensegrity_load (solver-crates-layering D9, leaf ζ)",
        baseline: 2,
        measure: Measure::Occurrences(Pattern::production(
            "const GROUNDING_DIAGONAL:",
            SRC,
            "const GROUNDING_DIAGONAL: f64 = 1.0;",
        )),
    },
    Ratchet {
        name: "cable-sign-rule-arms",
        home: "MemberKind::admits (solver-crates-layering D1, leaf η)",
        baseline: 1,
        measure: Measure::Occurrences(Pattern::production(
            "MemberKind::Cable =>",
            SRC,
            "MemberKind::Cable => qi > 0.0,",
        )),
    },
    Ratchet {
        name: "unchecked-anchor-partition",
        home: "ForceDensityProblem construction (solver-crates-layering D1, leaf η)",
        baseline: 1,
        measure: Measure::Occurrences(Pattern::production(
            "is_anchor[a] = true",
            SRC,
            "is_anchor[a] = true;",
        )),
    },
    // The next three rows are textual proxies: each needle spells the one copy's
    // local variable names, so a copy written with other names escapes it.
    Ratchet {
        name: "member-force-loops",
        home: "member_axial_forces in form_find.rs (solver-crates-layering D3, leaf η)",
        baseline: 1,
        measure: Measure::Occurrences(Pattern::production("qi * len", SRC, "let n_i = qi * len;")),
    },
    Ratchet {
        name: "surface-stencil-scatters",
        home: "scatter_surface_stencils in form_find.rs (solver-crates-layering D3, leaf η)",
        baseline: 1,
        measure: Measure::Occurrences(Pattern::production(
            "(idx[a], idx[b])] +=",
            SRC,
            "d[(idx[a], idx[b])] += l[a][b];",
        )),
    },
    Ratchet {
        name: "line-rank1-scatters",
        home: "scatter_line_members in form_find.rs (solver-crates-layering D3, leaf η)",
        baseline: 1,
        measure: Measure::Occurrences(Pattern::production(
            "[(j, k)] -= qi",
            SRC,
            "d[(j, k)] -= qi;",
        )),
    },
    // Counts every reference to `solve_reduced` outside its own definition, so a
    // second caller, a fn-pointer binding or an import reds whatever its
    // argument spelling; a hand-rolled loop that solves `D_ff` some other way
    // is not a reference, so this stays a proxy for "one driver".
    Ratchet {
        name: "anchored-fixed-point-drivers",
        home: "solve_reduced's one caller, the stencil-parameterised anchored \
               fixed-point driver in form_find.rs (solver-crates-layering D3, leaf η)",
        baseline: 1,
        measure: Measure::Occurrences(Pattern {
            skip_items: SOLVE_REDUCED_DEFINITION_AND_TEST_ITEMS,
            ..Pattern::production(
                "solve_reduced",
                SRC,
                "let solved = solve_reduced(&d, &x, &f, &a);",
            )
        }),
    },
    Ratchet {
        name: "mpc-row-literals",
        home: "MpcRow::new (solver-crates-layering D10, leaf θ)",
        baseline: 2,
        measure: Measure::Occurrences(Pattern {
            skip_items: &["impl MpcRow", "pub struct MpcRow"],
            view: View::CodeAndDocs,
            ..Pattern::whole_file(
                "MpcRow {",
                SRC_AND_TESTS,
                "let r = MpcRow { dofs: vec![0], coeffs: vec![1.0], rhs: 0.0 };",
            )
        }),
    },
    Ratchet {
        name: "duplicated-test-helpers",
        home: "tests/common/fea_harness.rs (solver-crates-layering D11, leaf ι)",
        baseline: 84,
        measure: Measure::DuplicateTopLevelFns {
            root: "tests/",
            shared_dir: "tests/common/",
        },
    },
];

#[test]
fn every_row_flags_and_clears_its_seeded_copies() {
    adoption_ratchet::assert_seeded_self_tests(RATCHETS, &HOST)
}

#[test]
fn every_ratchet_row_holds_at_its_committed_baseline() {
    adoption_ratchet::assert_crate_tree_at_baselines(RATCHETS, &HOST)
}
