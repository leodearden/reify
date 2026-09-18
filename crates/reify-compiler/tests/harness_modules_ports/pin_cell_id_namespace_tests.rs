//! Pins the two GUI cell-id namespaces apart via a real compile of
//! `prj/printer_v01/printer.ri`, so neither a printer.ri rename nor a
//! compiler lowering change can make them silently converge again.
//!
//! - **constraint namespace** — `gui/src-tauri/src/engine.rs::build_constraints`
//!   renders each `parameter_ids` entry as `<parent>.<sub_name>.<member>`, an
//!   INSTANCE path, via the `scoped_entity` lowering in
//!   `crates/reify-compiler/src/expr.rs` (reached from two `MemberAccess` arms).
//! - **values namespace** — `gui/src-tauri/src/engine.rs::build_values` renders
//!   each `cell_id` as `<TemplateName>.<member>`, a TYPE path, straight off
//!   `template.value_cells`.
//!
//! This file MIRRORS those two projections compiler-side rather than calling
//! them (`build_constraints`/`build_values` are private to the `gui` crate);
//! it therefore guards compiler lowering, not the GUI functions themselves —
//! a change shared by both sides of a mirror would not red here. See task
//! #7450 (esc-5098-6) for that incident's narrative and for the four `PIN_*`
//! ids' provenance. `gui/test/visual/railLengtheningGate.mjs` is the GUI-side
//! gate that first caught esc-5098-6, and it IS on `main` — but a Rust test
//! cannot import a `.mjs` module, so instead of a prose citation,
//! `scoped_ids_match_the_gui_gates_pin_constants` below reads its source text
//! and asserts its four `PIN_*_CELL` literals equal this file's `PIN_*`
//! consts, closing the drift a citation alone could not.
//!
//! # COMPILE ONLY
//!
//! Every test here compiles `prj/printer_v01/printer.ri` via
//! `reify_test_support::compile_source_with_stdlib` and inspects the
//! resulting `CompiledModule` only — never `check_source*`, `eval_source`, or
//! anything that reaches `reify_eval`. printer.ri SIGSEGVs the engine on the
//! eval/CSG path (#7383); a crashed test process reds the whole merge gate,
//! not just this file.

use reify_compiler::{CompiledConstraint, CompiledModule, TopologyTemplate};

// The CONSTRAINT namespace (`build_constraints`'s `parameter_ids`) for the
// two pins the task names — `Printer.<sub_name>.<member>` instance paths
// (see module doc for provenance).

/// `self.a_frame.rail_span_m` half of the rail-span pin (vs `self.motion.y_rail_len`).
const PIN_RAIL_SPAN: &str = "Printer.a_frame.rail_span_m";

/// `self.motion.y_rail_len` half of the rail-span pin (vs `self.a_frame.rail_span_m`).
const PIN_Y_RAIL_LEN: &str = "Printer.motion.y_rail_len";

/// `self.a_frame.travel_avail` half of the tool-dock pin (vs `self.tool_dock.yh_min_today`).
const PIN_TRAVEL_AVAIL: &str = "Printer.a_frame.travel_avail";

/// `self.tool_dock.yh_min_today` half of the tool-dock pin (vs `self.a_frame.travel_avail`).
const PIN_YH_MIN_TODAY: &str = "Printer.tool_dock.yh_min_today";

/// One row of the pin table: locate a pin by the MEMBER shape of its value
/// refs (namespace-agnostic — see module doc), then require its constraints'
/// `parameter_ids` to CONTAIN the given `Printer`-scoped instance paths.
struct PinCase {
    pin_name: &'static str,
    member_shape: &'static [&'static str],
    required_scoped_ids: &'static [&'static str],
}

const PIN_CASES: &[PinCase] = &[
    PinCase {
        pin_name: "rail-span pin (self.a_frame.rail_span_m vs self.motion.y_rail_len)",
        member_shape: &["rail_span_m", "y_rail_len"],
        required_scoped_ids: &[PIN_RAIL_SPAN, PIN_Y_RAIL_LEN],
    },
    PinCase {
        pin_name: "tool-dock pin (self.tool_dock.yh_min_today vs self.a_frame.travel_avail)",
        member_shape: &["yh_min_today", "travel_avail"],
        required_scoped_ids: &[PIN_YH_MIN_TODAY, PIN_TRAVEL_AVAIL],
    },
];

/// Splits a `Printer.<sub_name>.<member>` scoped id into `(sub_name,
/// member)`. Returns `None` unless `id` has exactly three non-empty
/// dot-separated segments, rather than degrading a malformed or
/// already-values-namespace (two-segment) id into nonsense empty parts.
fn split_scoped(id: &str) -> Option<(&str, &str)> {
    let parts: Vec<&str> = id.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    Some((parts[1], parts[2]))
}

/// Best-effort `values`-namespace spelling of a scoped id, for failure
/// messages only (never an assertion) — see module doc for the two
/// namespaces.
fn values_namespace_spelling_hint(printer: &TopologyTemplate, scoped_id: &str) -> String {
    match split_scoped(scoped_id) {
        Some((sub_name, member)) => {
            let type_name = sub_structure_name(printer, sub_name).unwrap_or(sub_name);
            format!("{type_name}.{member}")
        }
        None => format!(
            "{scoped_id} is not a 3-segment <Structure>.<sub>.<member> instance path — \
             it may already be a values-namespace id"
        ),
    }
}

/// Path to the GUI-side gate that first caught esc-5098-6 (see module doc).
const RAIL_LENGTHENING_GATE_MJS: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../gui/test/visual/railLengtheningGate.mjs");

/// The string literal assigned to `export const {name} = "...";` in `source`
/// — a minimal parse of exactly the shape `railLengtheningGate.mjs` uses for
/// its `PIN_*_CELL` constants, not a general JS parser.
fn extract_js_string_const(source: &str, name: &str) -> Option<String> {
    let needle = format!("const {name} = \"");
    let start = source.find(needle.as_str())? + needle.len();
    let end = source[start..].find('"')?;
    Some(source[start..start + end].to_string())
}

/// This Rust test cannot import a `.mjs` module, so instead it reads
/// `railLengtheningGate.mjs`'s source text and asserts its four
/// `PIN_*_CELL` literals equal this file's `PIN_*` consts — the executable
/// link that keeps the two from silently diverging (see module doc).
#[test]
fn scoped_ids_match_the_gui_gates_pin_constants() {
    let source = std::fs::read_to_string(RAIL_LENGTHENING_GATE_MJS)
        .unwrap_or_else(|e| panic!("cannot read {RAIL_LENGTHENING_GATE_MJS}: {e}"));

    for (rust_name, rust_value, js_name) in [
        ("PIN_RAIL_SPAN", PIN_RAIL_SPAN, "PIN_RAIL_SPAN_CELL"),
        ("PIN_Y_RAIL_LEN", PIN_Y_RAIL_LEN, "PIN_Y_RAIL_LEN_CELL"),
        ("PIN_TRAVEL_AVAIL", PIN_TRAVEL_AVAIL, "PIN_TRAVEL_AVAIL_CELL"),
        ("PIN_YH_MIN_TODAY", PIN_YH_MIN_TODAY, "PIN_YH_MIN_TODAY_CELL"),
    ] {
        let js_value = extract_js_string_const(&source, js_name).unwrap_or_else(|| {
            panic!(
                "{RAIL_LENGTHENING_GATE_MJS} has no `export const {js_name} = \"...\";` \
                 — has the GUI gate's constant been renamed?"
            )
        });
        assert_eq!(
            rust_value, js_value,
            "{rust_name} (this file) and {js_name} ({RAIL_LENGTHENING_GATE_MJS}) have diverged"
        );
    }
}

// ── Derivation machinery (S2) ────────────────────────────────────────────────

/// The real `prj/printer_v01/printer.ri`, resolved from this crate's manifest
/// dir. Hand-copied from `harness_constructor_typing/orientation_constructor_typing_tests.rs`'s
/// `PRINTER_RI` constant rather than shared — neither file has a common home
/// for it yet (`reify-test-support`, which both dev-depend on, would be the
/// natural one; see task #7450's follow-up). Being two copies, these CAN
/// disagree about which file they gate until hoisted; keep both in sync by
/// hand in the meantime.
const PRINTER_RI: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../prj/printer_v01/printer.ri");

/// Compiles `PRINTER_RI` with the stdlib prelude once per test binary
/// (`OnceLock`). COMPILE ONLY — see module doc, #7383. Asserts nothing about
/// `.diagnostics` (that belongs to the orientation gate); every positive
/// assertion in this file is non-vacuous by construction, so a degraded
/// compile still reds via a missing pin rather than passing silently.
fn compiled_printer() -> &'static CompiledModule {
    static MODULE: std::sync::OnceLock<CompiledModule> = std::sync::OnceLock::new();
    MODULE.get_or_init(|| {
        let source = std::fs::read_to_string(PRINTER_RI)
            .unwrap_or_else(|e| panic!("cannot read {PRINTER_RI}: {e}"));
        reify_test_support::compile_source_with_stdlib(&source)
    })
}

/// Short diagnostics summary for the "not found" panics/assertions below, so
/// a degraded compile is distinguishable at a glance from an actual
/// printer.ri design change. Never asserted on.
fn diagnostics_summary(module: &CompiledModule) -> String {
    let errors: Vec<&str> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .map(|d| d.message.as_str())
        .collect();
    format!(
        "printer.ri compiled with {} error diagnostic(s){}",
        errors.len(),
        if errors.is_empty() {
            String::new()
        } else {
            format!(": {:?}", errors.iter().take(3).copied().collect::<Vec<_>>())
        }
    )
}

/// The top-level `Printer` template of `module`; panics naming every
/// template name present (plus a diagnostics summary) if missing, so a
/// rename of the top-level structure reds legibly here rather than via an
/// `unwrap` backtrace.
fn printer_template(module: &CompiledModule) -> &TopologyTemplate {
    module.templates.iter().find(|t| t.name == "Printer").unwrap_or_else(|| {
        panic!(
            "no template named 'Printer'; templates present: {:?} ({})",
            module.templates.iter().map(|t| &t.name).collect::<Vec<_>>(),
            diagnostics_summary(module),
        )
    })
}

/// Locates constraints by MEMBER shape alone (never `.entity` or the
/// rendered id) — the seam that keeps this guard non-circular, since member
/// names are identical in both namespaces and so cannot smuggle in the scope
/// half that actually drifted in esc-5098-6. Superset match, not equality: a
/// pin's `<`/`>` halves also reference unrelated cells like `o1_pin_slack`.
fn pin_constraints_by_members<'a>(
    template: &'a TopologyTemplate,
    members: &[&str],
) -> Vec<&'a CompiledConstraint> {
    template
        .constraints
        .iter()
        .filter(|c| {
            let ref_members: std::collections::HashSet<String> = c
                .expr
                .collect_value_refs()
                .into_iter()
                .map(|id| id.member)
                .collect();
            members.iter().all(|m| ref_members.contains(*m))
        })
        .collect()
}

/// The projection `build_constraints` publishes as `parameter_ids`
/// (reimplemented here since `gui/src-tauri/src/engine.rs`'s private
/// `collect_value_refs` wrapper can't be called from this crate). Sort+dedup
/// are cosmetic — assertions here compare membership, not order.
fn constraint_parameter_ids(c: &CompiledConstraint) -> Vec<String> {
    let mut ids: Vec<String> = c
        .expr
        .collect_value_refs()
        .into_iter()
        .map(|id| id.to_string())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// Each pin, located by member shape alone, is exactly the `<`/`>` halves of
/// a two-sided pin (non-vacuity: a pin removed from printer.ri must red
/// here), AND is exactly what the GUI's superset selector
/// (`select_pin_constraints`, mirroring the GUI's own predicate — see module
/// doc) selects by scoped id. This is the assertion that would have caught
/// esc-5098-6 directly: the previous selector strings matched no
/// constraint's `parameter_ids`, so the selection came back empty at every
/// phase.
#[test]
fn gui_pin_selectors_select_exactly_the_shape_located_pins() {
    let module = compiled_printer();
    let printer = printer_template(module);

    for case in PIN_CASES {
        let shape_located = pin_constraints_by_members(printer, case.member_shape);
        assert_eq!(
            shape_located.len(),
            2,
            "{}: expected 2 constraints matching member shape {:?} (the `<`/`>` halves \
             of a two-sided pin — a pin removed from printer.ri must red here rather \
             than pass silently), found {} ({})",
            case.pin_name,
            case.member_shape,
            shape_located.len(),
            diagnostics_summary(module),
        );

        let id_selected = select_pin_constraints(&printer.constraints, case.required_scoped_ids);

        // Non-emptiness is asserted separately from the set-equality check
        // below: an empty selection is the PIN_ABSENT condition itself
        // (esc-5098-6) and must red loudly on its own.
        assert!(
            !id_selected.is_empty(),
            "{}: select_pin_constraints(&constraints, {:?}) selected ZERO constraints \
             — the PIN_ABSENT condition (esc-5098-6): the selector ids named a \
             namespace no constraint's parameter_ids carries ({}). values-namespace \
             spelling of each selector id: {:?}.",
            case.pin_name,
            case.required_scoped_ids,
            diagnostics_summary(module),
            case.required_scoped_ids
                .iter()
                .map(|id| values_namespace_spelling_hint(printer, id))
                .collect::<Vec<_>>(),
        );

        let shape_ids: std::collections::BTreeSet<String> =
            shape_located.iter().map(|c| c.id.to_string()).collect();
        let selected_ids: std::collections::BTreeSet<String> =
            id_selected.iter().map(|c| c.id.to_string()).collect();

        assert_eq!(
            selected_ids,
            shape_ids,
            "{}: select_pin_constraints(&constraints, {:?}) must select EXACTLY the \
             constraints located by member shape {:?} (by ConstraintNodeId). \
             shape-located: {:#?}; id-selected: {:#?}",
            case.pin_name,
            case.required_scoped_ids,
            case.member_shape,
            shape_located
                .iter()
                .map(|c| (c.id.to_string(), constraint_parameter_ids(c)))
                .collect::<Vec<_>>(),
            id_selected
                .iter()
                .map(|c| (c.id.to_string(), constraint_parameter_ids(c)))
                .collect::<Vec<_>>(),
        );
    }
}

/// Faithful mirror of the GUI's selector predicate (see module doc): keeps a
/// constraint when every `cells` entry is in its `parameter_ids` — superset,
/// not equality, since a pin legitimately also names `Printer.o1_pin_slack`.
/// Do not "improve" this into equality or prefix matching: the guard's value
/// is running the SAME predicate the GUI runs, so a divergence the GUI
/// cannot see is one this guard must not see either.
fn select_pin_constraints<'a>(
    constraints: &'a [CompiledConstraint],
    cells: &[&str],
) -> Vec<&'a CompiledConstraint> {
    constraints
        .iter()
        .filter(|c| {
            let ids = constraint_parameter_ids(c);
            cells.iter().all(|cell| ids.iter().any(|id| id.as_str() == *cell))
        })
        .collect()
}

/// The two namespaces are distinct for every pinned cell, and their
/// correspondence is DERIVED from `sub_components`/`value_cells`, never
/// hand-kept anywhere in this file. Iterates `PIN_CASES` directly (each
/// `required_scoped_ids` entry split via `split_scoped`) rather than a second
/// hand-kept `(sub_name, member)` table, so there is exactly one place that
/// states which cells this file pins and no member→case search is needed.
#[test]
fn values_namespace_is_distinct_from_and_corresponds_to_the_constraint_namespace() {
    let module = compiled_printer();
    let printer = printer_template(module);

    for case in PIN_CASES {
        // Hoisted out of the per-id loop below (it doesn't depend on
        // `constraint_spelling`) and asserted non-vacuous HERE: without this,
        // the containment check in (b) would pass vacuously — silently
        // checking nothing — for a pin deleted from printer.ri.
        let pin_constraints = pin_constraints_by_members(printer, case.member_shape);
        assert_eq!(
            pin_constraints.len(),
            2,
            "{}: expected 2 constraints matching member shape {:?} (the `<`/`>` halves of a \
             two-sided pin), found {} ({})",
            case.pin_name,
            case.member_shape,
            pin_constraints.len(),
            diagnostics_summary(module),
        );

        for &constraint_spelling in case.required_scoped_ids {
            let (sub_name, member) = split_scoped(constraint_spelling).unwrap_or_else(|| {
                panic!(
                    "{}: required_scoped_ids entry {constraint_spelling:?} is not a 3-segment \
                     <Structure>.<sub>.<member> instance path",
                    case.pin_name,
                )
            });

            // (a) the sub resolves to its declared structure — the entity half
            // of a constraint ref is the SUB NAME, not the TYPE NAME, which is
            // precisely the confusion behind esc-5098-6.
            let type_name = sub_structure_name(printer, sub_name).unwrap_or_else(|| {
                panic!(
                    "sub_structure_name(printer, {sub_name:?}) returned None; \
                     Printer.sub_components: {:?} ({})",
                    printer
                        .sub_components
                        .iter()
                        .map(|s| (s.name.as_str(), s.structure_name.as_str()))
                        .collect::<Vec<_>>(),
                    diagnostics_summary(module),
                )
            });

            // (b) constraint namespace: constraint_spelling must appear in the
            // parameter_ids of every constraint of this pin (non-vacuity of
            // `pin_constraints` is asserted once per case, above).
            for c in &pin_constraints {
                let ids = constraint_parameter_ids(c);
                assert!(
                    ids.iter().any(|id| id.as_str() == constraint_spelling),
                    "constraint {} parameter_ids does not contain '{constraint_spelling}'; \
                     parameter_ids: {:?}",
                    c.id,
                    ids,
                );
            }

            // (c) the declaring template (type_name) really declares `member` as
            // a value cell, and its values-namespace id renders as
            // `<TemplateName>.<member>` — mirrors `build_values`.
            let values_spelling =
                values_namespace_id(module, type_name, member).unwrap_or_else(|| {
                    panic!(
                        "values_namespace_id(module, {type_name:?}, {member:?}) returned \
                         None ({})",
                        diagnostics_summary(module),
                    )
                });
            assert_eq!(
                values_spelling,
                format!("{type_name}.{member}"),
                "values-namespace id for sub '{sub_name}' member '{member}' must render as \
                 '<TemplateName>.<member>'",
            );

            // (d) the regression assertion: no CROSS-SUB cell's values-namespace
            // spelling may appear in the parameter_ids of ANY constraint of
            // Printer. This is exactly the state the old (pre-#7450) selector
            // strings assumed, and it is false. Deliberately scoped to
            // cross-sub cells — type_name != "Printer" is guaranteed here
            // because every PIN_CASES member is declared on a SUB, never on
            // Printer itself — because a same-template self-ref legitimately
            // renders identically in both namespaces (build_values's
            // `<TemplateName>.<member>` and a self-ref's own instance path
            // coincide when TemplateName is "Printer"); a broader claim
            // covering self-refs would be false for a correct compiler. (This
            // also entails the two spellings differ for every pinned cell:
            // constraint_spelling is already proven present in (b) above, so
            // were it equal to values_spelling this assertion would fail.)
            for c in &printer.constraints {
                let ids = constraint_parameter_ids(c);
                assert!(
                    !ids.iter().any(|id| id.as_str() == values_spelling),
                    "constraint {} parameter_ids CONTAINS the values-namespace spelling \
                     '{values_spelling}' (constraint-namespace spelling would be \
                     '{constraint_spelling}') — no cross-sub cell may appear in a Printer \
                     constraint under its declaring TYPE's name. parameter_ids: {:?}",
                    c.id,
                    ids,
                );
            }
        }
    }
}

/// The exemption (d) above carves out, made a checked fact rather than only
/// an unexercised claim in a comment: `o1_pin_slack` is a value cell declared
/// directly on `Printer` (a same-template self-ref, shared slack term of both
/// pins), so its values-namespace spelling legitimately COINCIDES with the
/// spelling a constraint carries for the same cell — both render as
/// `Printer.o1_pin_slack`, since `build_values`'s `<TemplateName>.<member>`
/// and a self-ref's own instance path agree when TemplateName is "Printer".
#[test]
fn self_ref_spellings_legitimately_coincide() {
    let module = compiled_printer();
    let printer = printer_template(module);

    let values_spelling =
        values_namespace_id(module, "Printer", "o1_pin_slack").unwrap_or_else(|| {
            panic!(
                "values_namespace_id(module, \"Printer\", \"o1_pin_slack\") returned None ({})",
                diagnostics_summary(module),
            )
        });
    assert_eq!(values_spelling, "Printer.o1_pin_slack");

    let carries_it = printer
        .constraints
        .iter()
        .any(|c| constraint_parameter_ids(c).iter().any(|id| id.as_str() == values_spelling));
    assert!(
        carries_it,
        "expected some Printer constraint's parameter_ids to contain '{values_spelling}' \
         (the shared o1_pin_slack self-ref), proving the coincidence against a real \
         constraint rather than only against values_namespace_id ({})",
        diagnostics_summary(module),
    );
}

/// `sub_name`'s declared structure in `template` (e.g. `a_frame` → `AFrame`)
/// — derived, never hard-coded, so the two namespaces' correspondence can't
/// silently drift.
fn sub_structure_name<'a>(template: &'a TopologyTemplate, sub_name: &str) -> Option<&'a str> {
    template.sub_components.iter().find(|s| s.name == sub_name).map(|s| s.structure_name.as_str())
}

/// The values-namespace id of value cell `member` on template
/// `structure_name`, mirroring `gui/src-tauri/src/engine.rs::build_values`.
/// Scoped to one template (unlike `get_value_cell_in`, which panics on
/// cross-module ambiguity), so a miss here is a clean `None`.
fn values_namespace_id(
    module: &CompiledModule,
    structure_name: &str,
    member: &str,
) -> Option<String> {
    module
        .templates
        .iter()
        .find(|t| t.name == structure_name)?
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .map(|vc| vc.id.to_string())
}
