//! Pins the two GUI cell-id namespaces derived from a `CompiledModule` apart,
//! from a real compile of `prj/printer_v01/printer.ri`, so a future rename in
//! that file or a lowering change in the compiler cannot make the namespaces
//! silently drift back into each other (task #7450, a follow-up to #5098).
//!
//! - **constraint namespace** — `gui/src-tauri/src/engine.rs::build_constraints`
//!   renders each `parameter_ids` entry as `<parent>.<sub_name>.<member>`, an
//!   INSTANCE path, via the `scoped_entity` lowering in
//!   `crates/reify-compiler/src/expr.rs` (reached from two `MemberAccess` arms).
//! - **values namespace** — `gui/src-tauri/src/engine.rs::build_values` renders
//!   each `cell_id` as `<TemplateName>.<member>`, a TYPE path, straight off
//!   `template.value_cells`.
//!
//! esc-5098-6 found task #5098's GUI gate selecting pins with values-namespace
//! ids against constraints that only ever carry instance-namespace ids, so
//! every selection came back empty and the gate's constraint half was inert
//! at every phase. That gate, `gui/test/visual/railLengtheningGate.mjs`, is
//! not read by this file: it is not on `main` (it lives on the unmerged
//! `task/5098` branch). The four scoped ids it needs are instead stated below
//! as documented `const`s, cited to their producers, and proven by a real
//! compile.
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
// two pins the task names — `Printer.<sub_name>.<member>` instance paths,
// cited via #5098 (not a `main` path; see module doc).

/// `self.a_frame.rail_span_m` half of the rail-span pin — printer.ri:3504-3505.
const PIN_RAIL_SPAN: &str = "Printer.a_frame.rail_span_m";

/// `self.motion.y_rail_len` half of the rail-span pin — printer.ri:3504-3505.
const PIN_Y_RAIL_LEN: &str = "Printer.motion.y_rail_len";

/// `self.a_frame.travel_avail` half of the tool-dock pin — printer.ri:3560-3561.
const PIN_TRAVEL_AVAIL: &str = "Printer.a_frame.travel_avail";

/// `self.tool_dock.yh_min_today` half of the tool-dock pin — printer.ri:3560-3561.
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
        pin_name: "rail-span pin (printer.ri:3504-3505)",
        member_shape: &["rail_span_m", "y_rail_len"],
        required_scoped_ids: &[PIN_RAIL_SPAN, PIN_Y_RAIL_LEN],
    },
    PinCase {
        pin_name: "tool-dock pin (printer.ri:3560-3561)",
        member_shape: &["yh_min_today", "travel_avail"],
        required_scoped_ids: &[PIN_YH_MIN_TODAY, PIN_TRAVEL_AVAIL],
    },
];

/// Best-effort `values`-namespace spelling of a scoped id, for failure
/// messages only (never an assertion) — see module doc for the two
/// namespaces.
fn values_namespace_spelling_hint(printer: &TopologyTemplate, scoped_id: &str) -> String {
    let mut parts = scoped_id.splitn(3, '.');
    let _printer_name = parts.next().unwrap_or_default();
    let sub_name = parts.next().unwrap_or_default();
    let member = parts.next().unwrap_or_default();
    let type_name = sub_structure_name(printer, sub_name).unwrap_or(sub_name);
    format!("{type_name}.{member}")
}

// ── Derivation machinery (S2) ────────────────────────────────────────────────

/// The real `prj/printer_v01/printer.ri`, resolved from this crate's manifest
/// dir. Mirrors `harness_constructor_typing/orientation_constructor_typing_tests.rs`'s
/// `PRINTER_RI` constant, copied verbatim so the two gates cannot disagree
/// about which file they gate.
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
/// (`select_pin_constraints`, mirroring `selectPinConstraints` — #5098, not
/// a `main` path; see module doc) selects by scoped id. This is the
/// assertion that would have caught esc-5098-6 directly: the previous
/// selector strings matched no constraint's `parameter_ids`, so
/// `foldPinStatus` reported PIN_ABSENT and the gate's constraint half was
/// inert at every phase.
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

/// Faithful mirror of the GUI's `selectPinConstraints` (#5098; not a `main`
/// path — see module doc): keeps a constraint when every `cells` entry is in
/// its `parameter_ids` — superset, not equality, since a pin legitimately
/// also names `Printer.o1_pin_slack`. Do not "improve" this into equality or
/// prefix matching: the guard's value is running the SAME predicate the GUI
/// runs, so a divergence the GUI cannot see is one this guard must not see
/// either.
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

/// The four `(sub_name, member)` cells this file pins, reused to derive both
/// namespace spellings from the same compile rather than by hand.
const PINNED_CELLS: &[(&str, &str)] = &[
    ("a_frame", "rail_span_m"),
    ("motion", "y_rail_len"),
    ("a_frame", "travel_avail"),
    ("tool_dock", "yh_min_today"),
];

/// The two namespaces are distinct for every pinned cell, and their
/// correspondence is DERIVED from `sub_components`/`value_cells`, never
/// hand-kept anywhere in this file.
#[test]
fn values_namespace_is_distinct_from_and_corresponds_to_the_constraint_namespace() {
    let module = compiled_printer();
    let printer = printer_template(module);

    for &(sub_name, member) in PINNED_CELLS {
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

        // constraint namespace: Printer.<sub_name>.<member>. Must match one
        // of the PIN_* constants, and must appear in the parameter_ids of
        // the pin this cell belongs to (located by member shape, S2).
        let constraint_spelling = format!("Printer.{sub_name}.{member}");
        let all_pin_constants: Vec<&str> =
            PIN_CASES.iter().flat_map(|c| c.required_scoped_ids.iter().copied()).collect();
        assert!(
            all_pin_constants.contains(&constraint_spelling.as_str()),
            "constraint-namespace spelling '{constraint_spelling}' (derived from sub \
             '{sub_name}' + member '{member}') does not match any PIN_* constant; PIN_* \
             constants: {all_pin_constants:?}",
        );
        // Exactly one PinCase may claim this member: a `find` would silently
        // pick the first on an ambiguous (future, reused) member name and
        // validate the wrong pin's constraints instead of reddening.
        let matching_cases: Vec<&PinCase> =
            PIN_CASES.iter().filter(|c| c.member_shape.contains(&member)).collect();
        assert_eq!(
            matching_cases.len(),
            1,
            "member {member:?} must appear in exactly one PinCase's member_shape, found \
             {} ({:?}); disambiguate PIN_CASES before reusing a member name across pins",
            matching_cases.len(),
            matching_cases.iter().map(|c| c.pin_name).collect::<Vec<_>>(),
        );
        let case = matching_cases[0];
        for c in &pin_constraints_by_members(printer, case.member_shape) {
            let ids = constraint_parameter_ids(c);
            assert!(
                ids.iter().any(|id| id.as_str() == constraint_spelling),
                "constraint {} parameter_ids does not contain '{constraint_spelling}'; \
                 parameter_ids: {:?}",
                c.id,
                ids,
            );
        }

        // (b) the declaring template (type_name) really declares `member` as
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

        // (c) the two spellings differ for every pinned cell.
        assert_ne!(
            constraint_spelling, values_spelling,
            "constraint-namespace spelling '{constraint_spelling}' and values-namespace \
             spelling '{values_spelling}' must DIFFER for a cross-sub cell — if they now \
             match, the constraints panel and the values panel would silently agree on an \
             id that means two different things",
        );

        // (d) the regression assertion: no values-namespace spelling may
        // appear in the parameter_ids of ANY constraint of Printer. This is
        // exactly the state the old (pre-#7450) selector strings assumed,
        // and it is false.
        for c in &printer.constraints {
            let ids = constraint_parameter_ids(c);
            assert!(
                !ids.iter().any(|id| id.as_str() == values_spelling),
                "constraint {} parameter_ids CONTAINS the values-namespace spelling \
                 '{values_spelling}' (constraint-namespace spelling would be \
                 '{constraint_spelling}') — a Printer constraint must only ever carry \
                 constraint-namespace (instance-path) ids. parameter_ids: {:?}",
                c.id,
                ids,
            );
        }
    }
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
