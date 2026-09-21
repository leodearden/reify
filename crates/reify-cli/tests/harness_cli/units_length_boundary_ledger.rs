//! The BINDING between PRD `docs/prds/v0_6/units-length-gate-completion.md` §6 and the
//! tests that discharge it.
//!
//! §6 is a 21-row table (rows `1`..`20` plus `9b`) spread across eleven files in six
//! crates plus the GUI. "The §6 suite exists and is green" is therefore a claim about a
//! set, and no single test file can see that set. Without this ledger the claim lives in
//! prose: renaming `stubbing_out_one_shipped_gate_makes_the_guard_fire` in another crate,
//! or deleting it, would un-cover §6 row 12 while every remaining test stayed green and
//! every prose citation silently went stale. With it, that rename turns THIS gate red.
//!
//! Its ONE purpose is that binding. It holds no scenario assertions of its own — each row
//! is discharged where it is cited, which is why this is a separate file from
//! `units_length_boundary_gate.rs` rather than a section inside it.
//!
//! It is a registry-completeness check over test IDENTITIES, in the shape of
//! `units_length_closure_guard.rs::every_shipped_residual_cite_has_a_todo_comment_in_this_file`.
//! It asserts nothing about prose, doc comments or message wording: a row is bound when
//! the file exists and defines the named `fn`, and that is all.
//!
//! A row's discharge is a LIST of (file, test_fn) pairs, not a single pair, because §6's
//! own postcondition column gives several rows two halves that cannot live in one test or
//! even in one crate — row 1 (`reify eval`'s exit code at the process boundary, plus the
//! `DiagnosticCode` clause, which is not observable there), row 5 (the value form's
//! rejection, plus the scalar form's no-regression half), row 7 (the `.ri`-expressible
//! form, plus the `Scalar{DIMENSIONLESS}` form, which is not expressible from `.ri`),
//! row 13 (naming the op kind, AND naming the field), and rows 14/19/20. Binding only
//! one half of those rows would leave the other half un-bound, which is the exact failure
//! this file exists to prevent.

use std::path::PathBuf;

/// One test that discharges part or all of a §6 row.
#[derive(Clone, Copy, Debug)]
struct Discharge {
    /// Repo-relative path, resolved against [`workspace_root`].
    file: &'static str,
    /// The `#[test] fn` name, as defined in `file`.
    test_fn: &'static str,
}

/// One row of §6's boundary-test table, bound to the test(s) that discharge it.
#[derive(Clone, Copy, Debug)]
struct BoundaryRow {
    /// The row's §6 label. `9b` is a real label, which is why this is a string and not an
    /// index.
    row: &'static str,
    /// §6's own scenario, abbreviated — enough to recognise the row without the PRD open.
    scenario: &'static str,
    discharged_by: &'static [Discharge],
}

const BOUNDARY_GATE: &str = "crates/reify-cli/tests/harness_cli/units_length_boundary_gate.rs";
const CLOSURE_GUARD: &str =
    "crates/reify-eval/tests/harness_geometry/units_length_closure_guard.rs";
const GEOMETRY_OPS: &str = "crates/reify-eval/src/geometry_ops/tests.rs";
const OCCT_KERNEL: &str = "crates/reify-kernel-occt/src/lib.rs";
const FIDGET_KERNEL: &str = "crates/reify-kernel-fidget/src/kernel.rs";
const GUI_ENGINE: &str = "gui/src-tauri/src/tests/engine_tests.rs";

/// Every row of §6, bound. Populated from the map re-measured against this tree, not from
/// the PRD's prose.
const SECTION_6_ROWS: &[BoundaryRow] = &[
    BoundaryRow {
        row: "1",
        scenario: "box(20, 20, 10) — eval exits 1, naming every rejected argument, \
                   with a DiagnosticCode",
        discharged_by: &[
            Discharge {
                file: BOUNDARY_GATE,
                test_fn: "bare_box_dimensions_exit_1_naming_every_rejected_argument",
            },
            // The code half: `reify eval` exposes no structured-diagnostics flag, so the
            // `DiagnosticCode::DimensionedArgRejected` clause is discharged in-process.
            Discharge {
                file: "crates/reify-eval/tests/harness_geometry/\
                       primitive_profile_length_units_e2e.rs",
                test_fn: "bare_box_dimensions_drop_the_op_with_a_coded_error",
            },
        ],
    },
    BoundaryRow {
        row: "2",
        scenario: "box(20mm, 20mm, 10mm) (control) — exits 0, realization volume \
                   unchanged vs the pre-gate baseline",
        discharged_by: &[Discharge {
            file: BOUNDARY_GATE,
            test_fn: "dimensioned_box_exits_0_with_the_pre_gate_si_volume",
        }],
    },
    BoundaryRow {
        row: "3",
        scenario: "box(0, 0, 0) — bare zero is rejected too (D1)",
        discharged_by: &[Discharge {
            file: BOUNDARY_GATE,
            test_fn: "a_bare_zero_is_rejected_like_any_other_bare_number",
        }],
    },
    BoundaryRow {
        row: "4",
        scenario: "fillet(box(10mm,10mm,10mm), 1) — the units diagnostic names `radius`, \
                   replacing the span-less BRepFilletAPI_MakeFillet failure",
        discharged_by: &[Discharge {
            file: BOUNDARY_GATE,
            test_fn: "a_bare_fillet_radius_replaces_the_span_less_occt_failure",
        }],
    },
    BoundaryRow {
        row: "5",
        scenario: "mirror(b, plane_yz(10)) value form is rejected; the scalar form keeps \
                   its existing per-component wording (no regression)",
        discharged_by: &[
            Discharge {
                file: BOUNDARY_GATE,
                test_fn: "a_bare_plane_offset_is_rejected_and_mirror_fails_attributably",
            },
            Discharge {
                file: BOUNDARY_GATE,
                test_fn: "the_scalar_mirror_origin_keeps_its_per_component_wording",
            },
        ],
    },
    BoundaryRow {
        row: "6",
        scenario: "mirror(b, plane_yz(10mm)) (control) — exits 0, geometrically identical \
                   to the pre-gate 0.01 m offset",
        discharged_by: &[Discharge {
            file: BOUNDARY_GATE,
            test_fn: "a_dimensioned_plane_offset_mirrors_to_the_same_si_position",
        }],
    },
    BoundaryRow {
        row: "7",
        scenario: "apply_transform(box, transform3(…, vec3(5,0,0))) and the \
                   Scalar{DIMENSIONLESS} form — a units diagnostic (D11), not the generic \
                   `not a valid Transform<3>`",
        discharged_by: &[
            Discharge {
                file: BOUNDARY_GATE,
                test_fn: "a_bare_transform_translation_names_the_component_not_the_shape",
            },
            // The dimensionless twin is not expressible from `.ri` (`5mm / 1mm` collapses
            // to `Value::Real`), so its home is the compile-layer row.
            Discharge {
                file: GEOMETRY_OPS,
                test_fn: "compile_geometry_op_apply_transform_translation_follows_the_three_state_contract",
            },
        ],
    },
    BoundaryRow {
        row: "8",
        scenario: "affine_translate(5kg, 0kg, 0kg) — the MASS dimension is no longer \
                   silently discarded",
        discharged_by: &[Discharge {
            file: "crates/reify-cli/tests/harness_cli/cli_affine_eval.rs",
            test_fn: "eval_affine_translate_mass_exits_1_with_a_units_error",
        }],
    },
    BoundaryRow {
        row: "9",
        scenario: "a bare primitive dimension — `reify check` AND `reify eval` both exit 1",
        discharged_by: &[Discharge {
            file: BOUNDARY_GATE,
            test_fn: "check_and_eval_agree_on_a_bare_primitive_dimension",
        }],
    },
    BoundaryRow {
        row: "9b",
        scenario: "linear_pattern_2d(w, 1, 0, 0, 3, 20) at arity 6 — resolved as an arity \
                   error, not left silently un-slotted",
        discharged_by: &[
            Discharge {
                file: BOUNDARY_GATE,
                test_fn: "the_arity_6_linear_pattern_2d_site_is_an_arity_error_not_a_length_slot",
            },
            Discharge {
                file: "crates/reify-compiler/tests/harness_compilation_surface/\
                       compile_api_tests.rs",
                test_fn: "compile_linear_pattern_2d_wrong_arity_produces_diagnostic",
            },
        ],
    },
    BoundaryRow {
        row: "10",
        scenario: "closure guard over all builtins × arity 0..=14 — green; every reached \
                   numeric position is rejected or allowlisted-with-justification",
        discharged_by: &[Discharge {
            file: CLOSURE_GUARD,
            test_fn: "closure_guard_is_green_over_the_whole_universe",
        }],
    },
    BoundaryRow {
        row: "11",
        scenario: "guard anti-vacuity: shrink the allowlist by one entry — the guard fails",
        discharged_by: &[Discharge {
            file: CLOSURE_GUARD,
            test_fn: "shrinking_the_shipped_allowlist_by_one_entry_makes_the_guard_fire",
        }],
    },
    BoundaryRow {
        row: "12",
        scenario: "guard anti-vacuity: stub out one shipped gate — the guard fails naming \
                   that position",
        discharged_by: &[Discharge {
            file: CLOSURE_GUARD,
            test_fn: "stubbing_out_one_shipped_gate_makes_the_guard_fire",
        }],
    },
    BoundaryRow {
        row: "13",
        scenario: "debug-build kernel receives a bare length for a gated field — the \
                   assertion fires naming op kind AND field name",
        // Both shipped kernels carry the tripwire, so both are bound: a deletion on
        // either side un-covers half of this row.
        discharged_by: &[
            Discharge {
                file: OCCT_KERNEL,
                test_fn: "occt_armed_bare_length_panics_naming_the_op_kind",
            },
            Discharge {
                file: OCCT_KERNEL,
                test_fn: "occt_armed_bare_length_panics_naming_the_field",
            },
            Discharge {
                file: FIDGET_KERNEL,
                test_fn: "fidget_armed_bare_length_panics_naming_the_op_kind",
            },
            Discharge {
                file: FIDGET_KERNEL,
                test_fn: "fidget_armed_bare_length_panics_naming_the_field",
            },
        ],
    },
    BoundaryRow {
        row: "14",
        scenario: "release-build same injection — the kernel reports naming op kind and \
                   field, behaviour otherwise unchanged",
        discharged_by: &[
            Discharge {
                file: OCCT_KERNEL,
                test_fn: "occt_release_armed_bare_length_reports_without_panicking",
            },
            Discharge {
                file: FIDGET_KERNEL,
                test_fn: "fidget_release_armed_bare_length_reports_without_panicking",
            },
        ],
    },
    BoundaryRow {
        row: "15",
        scenario: "add a new SweepKind variant without registering it — the \
                   registry-completeness test fails via VARIANT_COUNT",
        discharged_by: &[Discharge {
            file: GEOMETRY_OPS,
            test_fn: "seeded_unregistered_sweep_variant_fails_registry_completeness",
        }],
    },
    BoundaryRow {
        row: "16",
        scenario: "GUI: typing `20` into a dimensioned param cell is rejected at input \
                   time with a message naming the expected dimension",
        discharged_by: &[Discharge {
            file: GUI_ENGINE,
            test_fn: "the_bare_number_refusal_names_a_rung_from_the_cells_own_ladder",
        }],
    },
    BoundaryRow {
        row: "17",
        scenario: "GUI: typing `3in` into a LENGTH cell is accepted — the DSL registry has \
                   `in`, the GUI table lacked it",
        discharged_by: &[Discharge {
            file: GUI_ENGINE,
            test_fn: "parse_value_string_accepts_the_ladder_labels_the_property_editor_admits",
        }],
    },
    BoundaryRow {
        row: "18",
        scenario: "the pattern diagnostic names `linear_pattern`, not `linear` (and \
                   `linear_pattern_2d`, not `linear_2d`)",
        discharged_by: &[Discharge {
            file: BOUNDARY_GATE,
            test_fn: "pattern_spacing_undef_names_the_builtin_the_author_typed",
        }],
    },
    BoundaryRow {
        row: "19",
        scenario: "isosurface(grid, iso: 5) is rejected; isosurface(grid) still takes its \
                   default with no units diagnostic (D12)",
        discharged_by: &[
            Discharge {
                file: BOUNDARY_GATE,
                test_fn: "the_iso_option_is_gated_but_its_absence_is_not",
            },
            // The absent-`iso` form cannot exit 0 under `reify eval` without an openvdb
            // kernel, so the exits-0 half is discharged where one is registered.
            Discharge {
                file: "crates/reify-eval/tests/isosurface_iso_option_e2e.rs",
                test_fn: "iso_option_changes_surfaced_mesh",
            },
        ],
    },
    BoundaryRow {
        row: "20",
        scenario: "the whole examples/**/*.ri corpus, examples/best_practices/ included, \
                   stays green — zero migrations were needed",
        discharged_by: &[
            Discharge {
                file: "crates/reify-eval/tests/harness_corpus_gates/\
                       units_length_corpus_end_state.rs",
                test_fn: "no_shipped_example_trips_a_length_gate",
            },
            Discharge {
                file: "crates/reify-eval/tests/harness_corpus_gates/\
                       units_length_corpus_end_state.rs",
                test_fn: "the_corpus_checker_fires_on_a_seeded_bare_length",
            },
        ],
    },
];

/// Resolve the workspace root from `CARGO_MANIFEST_DIR`.
///
/// `reify-cli` lives at `<root>/crates/reify-cli`, so the root is two levels up — the
/// same resolution `harness_cli/corpus_no_bare_scalar.rs` uses.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root must be accessible")
}

/// Whether `source` DEFINES `fn <test_fn>(`.
///
/// Anchored to the start of a line (after indentation) so that a mention of the name in a
/// doc comment, a citation, or a call site does not satisfy the binding — only a
/// definition does.
fn defines_fn(source: &str, test_fn: &str) -> bool {
    let opener = format!("fn {test_fn}(");
    source
        .lines()
        .any(|line| line.trim_start().starts_with(&opener))
}

/// Every (row, discharging test) pair in the ledger.
fn discharges() -> impl Iterator<Item = (&'static BoundaryRow, &'static Discharge)> {
    SECTION_6_ROWS
        .iter()
        .flat_map(|row| row.discharged_by.iter().map(move |d| (row, d)))
}

/// §6's row labels are exactly `1`..`20` plus `9b` — 21 of them, no gaps, no duplicates.
///
/// The count is asserted as a literal as well as derived, so adding a 22nd row (or
/// dropping one) cannot pass by moving the goalposts with it.
#[test]
fn every_section_6_row_is_bound_exactly_once() {
    const SECTION_6_ROW_COUNT: usize = 21;

    let mut expected: Vec<String> = (1..=20).map(|n| n.to_string()).collect();
    expected.push("9b".to_string());
    expected.sort();

    let mut bound: Vec<String> = SECTION_6_ROWS.iter().map(|r| r.row.to_string()).collect();
    bound.sort();

    let duplicates: Vec<&String> = bound
        .iter()
        .enumerate()
        .filter(|(i, label)| *i > 0 && bound[i - 1] == **label)
        .map(|(_, label)| label)
        .collect();
    assert!(
        duplicates.is_empty(),
        "row label(s) {duplicates:?} are bound more than once — each §6 row takes exactly \
         one entry, and a row's several halves belong in its `discharged_by` list"
    );

    let missing: Vec<&String> = expected.iter().filter(|r| !bound.contains(r)).collect();
    let extra: Vec<&String> = bound.iter().filter(|r| !expected.contains(r)).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "the bound row set must be exactly §6's. Missing: {missing:?}. Not in §6: {extra:?}"
    );
    assert_eq!(
        SECTION_6_ROWS.len(),
        SECTION_6_ROW_COUNT,
        "§6 has {SECTION_6_ROW_COUNT} rows (1..20 plus 9b); the ledger binds {}",
        SECTION_6_ROWS.len()
    );

    for row in SECTION_6_ROWS {
        assert!(
            !row.discharged_by.is_empty(),
            "§6 row {} names no discharging test — a row with an empty list is un-covered, \
             not covered",
            row.row
        );
        assert!(
            !row.scenario.trim().is_empty(),
            "§6 row {} states no scenario, so a reader cannot tell which row it is",
            row.row
        );
    }
}

/// Every cited file exists on this tree.
///
/// A moved or deleted file is the cheapest way to un-cover a row, and it is invisible to
/// the test inside it: that test moves with the file and stays green.
#[test]
fn every_cited_file_resolves() {
    let root = workspace_root();
    let mut missing: Vec<String> = Vec::new();

    for (row, discharge) in discharges() {
        if !root.join(discharge.file).is_file() {
            missing.push(format!("§6 row {}: {}", row.row, discharge.file));
        }
    }

    assert!(
        missing.is_empty(),
        "cited file(s) do not resolve under {}:\n  {}",
        root.display(),
        missing.join("\n  ")
    );
}

/// Every cited `#[test] fn` is still defined in the file that cites it.
///
/// This is the load-bearing half. Renaming or deleting a test in another crate is
/// otherwise silent: the suite it lived in stays green with one fewer test, and every
/// prose citation of it goes stale without any gate noticing. Here it reds.
#[test]
fn every_cited_test_fn_is_defined_in_its_file() {
    let root = workspace_root();
    let mut unresolved: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for (row, discharge) in discharges() {
        let path = root.join(discharge.file);
        match std::fs::read_to_string(&path) {
            Err(e) => unresolved.push(format!(
                "§6 row {}: cannot read {} ({e})",
                row.row, discharge.file
            )),
            Ok(source) => {
                checked += 1;
                if !defines_fn(&source, discharge.test_fn) {
                    unresolved.push(format!(
                        "§6 row {}: `fn {}(` is not defined in {} — it was renamed, moved \
                         or deleted, which un-covers this row",
                        row.row, discharge.test_fn, discharge.file
                    ));
                }
            }
        }
    }

    assert!(unresolved.is_empty(), "{}", unresolved.join("\n"));
    assert_eq!(
        checked,
        discharges().count(),
        "every cited file must have been read and searched; {checked} of {} were",
        discharges().count()
    );
    assert!(
        checked >= SECTION_6_ROWS.len(),
        "{checked} citation(s) checked for {} rows — fewer citations than rows means a row \
         was bound to nothing",
        SECTION_6_ROWS.len()
    );
}
