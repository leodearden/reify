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

use std::path::{Path, PathBuf};

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

/// The document §6 lives in. Cited in doc comments throughout; READ by
/// [`section_6_row_labels`], which is what keeps those citations from going stale.
const SECTION_6_PRD: &str = "docs/prds/v0_6/units-length-gate-completion.md";

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
        // Both are `#[cfg(not(debug_assertions))]`, so neither EXECUTES in a debug run —
        // only under the merge gate's `--profile both`. The binding below is a
        // source-text check and so holds in either profile, which is what keeps this row
        // bound in the run where its tests cannot appear.
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

/// What a cited discharge turned out to be, when the ledger went and looked.
///
/// The ledger's claim is that a row is still DISCHARGED, which is a stronger property
/// than "an identifier by that name still exists". Three ways to un-cover a row cost
/// nothing and leave the identifier in place: delete it, drop its `#[test]` so it becomes
/// a helper, or add `#[ignore]`. All three are reported distinctly, because the fix
/// differs for each.
#[derive(PartialEq, Eq, Debug)]
enum BindingState {
    Discharged,
    /// No `fn <name>(` definition at all — renamed, moved or deleted.
    Undefined,
    /// Defined, but carries no `#[test]`. A test demoted to a helper discharges nothing.
    NotATest,
    /// Defined and attributed, but ignored. `#[ignore = "blocked on #NNNN"]` is an honest
    /// marker on a test that is not running, and a row bound to one is not covered.
    Ignored,
}

/// How `source` DEFINES `fn <test_fn>(`, if at all.
///
/// The definition is anchored to the start of a line (after indentation), so a mention of
/// the name in a doc comment, a citation or a call site does not satisfy the binding. The
/// attributes are the contiguous run of attribute and comment lines directly above it —
/// the shape every cited site uses, `#[cfg(debug_assertions)] / #[test] /
/// #[should_panic(expected = "Box")]` included.
fn binding_state(source: &str, test_fn: &str) -> BindingState {
    let opener = format!("fn {test_fn}(");
    let lines: Vec<&str> = source.lines().collect();
    let Some(at) = lines
        .iter()
        .position(|line| line.trim_start().starts_with(&opener))
    else {
        return BindingState::Undefined;
    };

    let attributes: Vec<&str> = lines[..at]
        .iter()
        .rev()
        .map(|line| line.trim())
        .take_while(|line| line.starts_with("#[") || line.starts_with("//"))
        .filter(|line| line.starts_with("#["))
        .collect();

    if attributes.iter().any(|a| a.starts_with("#[ignore")) {
        BindingState::Ignored
    } else if attributes.iter().any(|a| a.starts_with("#[test]")) {
        BindingState::Discharged
    } else {
        BindingState::NotATest
    }
}

/// §6's own row labels, read out of the PRD's table rather than transcribed from it.
///
/// [`SECTION_6_ROWS`] is a binding between two things, and until this existed the ledger
/// only ever checked one of them: adding a row 21 to §6, renaming a label, or moving the
/// PRD left every test here green while the binding went stale or dangled. The table's
/// first column is a single token per row, so the label set is readable without an ad-hoc
/// parser for the rest of it (heuristic 12) — the header row and the `|---|` separator are
/// the only non-label rows, and both are recognisable by their own first cell.
fn section_6_row_labels(root: &Path) -> Vec<String> {
    let prd = std::fs::read_to_string(root.join(SECTION_6_PRD)).unwrap_or_else(|e| {
        panic!("this ledger binds §6 of {SECTION_6_PRD}, which must be readable ({e})")
    });

    let labels: Vec<String> = prd
        .lines()
        .skip_while(|line| !line.starts_with("## 6."))
        .skip(1)
        .take_while(|line| !line.starts_with("## "))
        .filter_map(|line| line.trim().strip_prefix('|'))
        .filter_map(|row| row.split('|').next())
        .map(|cell| cell.trim().to_string())
        .filter(|label| label != "#" && !label.starts_with('-'))
        .collect();

    assert!(
        !labels.is_empty(),
        "no table rows found under the `## 6.` heading of {SECTION_6_PRD} — the section \
         was renumbered, renamed or restructured, so this ledger is binding a row set \
         that no longer describes anything"
    );
    labels
}

/// Every (row, discharging test) pair in the ledger.
fn discharges() -> impl Iterator<Item = (&'static BoundaryRow, &'static Discharge)> {
    SECTION_6_ROWS
        .iter()
        .flat_map(|row| row.discharged_by.iter().map(move |d| (row, d)))
}

/// The bound row set is exactly §6's — read from the PRD, not transcribed from it.
///
/// Taking the expected set from [`section_6_row_labels`] rather than from a hand-written
/// `1..=20` plus `9b` is what makes this a BINDING check rather than a second copy of the
/// same list: a row added to, removed from or renamed in §6 reds here, naming the label,
/// instead of leaving the ledger silently describing a table that has moved on.
#[test]
fn every_section_6_row_is_bound_exactly_once() {
    let mut expected = section_6_row_labels(&workspace_root());
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
        expected.len(),
        "{SECTION_6_PRD} §6 has {} rows; the ledger binds {}",
        expected.len(),
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

/// Every cited file exists on this tree — the PRD among them.
///
/// A moved or deleted file is the cheapest way to un-cover a row, and it is invisible to
/// the test inside it: that test moves with the file and stays green. [`SECTION_6_PRD`]
/// is checked alongside the discharges because it is the other end of the binding, and a
/// binding with one dangling end is not a binding.
#[test]
fn every_cited_file_resolves() {
    let root = workspace_root();
    let mut missing: Vec<String> = Vec::new();

    if !root.join(SECTION_6_PRD).is_file() {
        missing.push(format!("the PRD this ledger binds: {SECTION_6_PRD}"));
    }
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

/// Every cited `#[test] fn` is still defined in the file that cites it, still a `#[test]`,
/// and still running.
///
/// This is the load-bearing half. Renaming or deleting a test in another crate is
/// otherwise silent: the suite it lived in stays green with one fewer test, and every
/// prose citation of it goes stale without any gate noticing. Here it reds.
///
/// Definition alone is not enough, and the gap is not hypothetical: four of these rows
/// (13 and 14) cite `fn`s living in the `src/lib.rs` and `src/kernel.rs` of two kernel
/// crates, where a non-test `fn` is the norm. Dropping a `#[test]` there, or adding an
/// `#[ignore = "blocked on #NNNN"]`, un-covers the row at zero cost while leaving the
/// identifier exactly where the ledger expects it. Both are rejected, and reported
/// distinctly, because the fix differs.
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
                let complaint = match binding_state(&source, discharge.test_fn) {
                    BindingState::Discharged => None,
                    BindingState::Undefined => Some(
                        "is not defined there — it was renamed, moved or deleted, which \
                         un-covers this row",
                    ),
                    BindingState::NotATest => Some(
                        "is defined there but carries no `#[test]` — a test demoted to a \
                         helper discharges nothing",
                    ),
                    BindingState::Ignored => Some(
                        "is defined and attributed there but is `#[ignore]`d — an ignored \
                         test discharges nothing",
                    ),
                };
                if let Some(complaint) = complaint {
                    unresolved.push(format!(
                        "§6 row {}: `fn {}(` in {} {complaint}",
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
