//! Pins the two GUI cell-id namespaces derived from a `CompiledModule` apart,
//! from a real compile of `prj/printer_v01/printer.ri`, so a future rename in
//! that file or a lowering change in the compiler cannot make the namespaces
//! silently drift back into each other (task #7450, a follow-up to #5098).
//!
//! # The two namespaces
//!
//! - **constraint namespace** — `gui/src-tauri/src/engine.rs::build_constraints`
//!   (L4997) fills each `ConstraintData.parameter_ids` entry from
//!   `CompiledExpr::collect_value_refs` (`crates/reify-ir/src/expr.rs:1008`),
//!   rendered through `ValueCellId`'s `Display` impl
//!   (`crates/reify-core/src/identity.rs:134`, `"{entity}.{member}"`). For a
//!   cross-sub reference (`self.<sub>.<member>`) the compiler stamps the
//!   entity half as `format!("{}.{}", scope.entity_name, sub_name)` —
//!   `crates/reify-compiler/src/expr.rs:4215` and `:5909`, the one lowering
//!   line reached from two different `MemberAccess` arms — so a constraint's
//!   ref renders as `<parent>.<sub_name>.<member>`, an INSTANCE path.
//! - **values namespace** — `build_values` (L4897) fills each
//!   `ValueData.cell_id` with `cell.id.to_string()` walked straight off
//!   `template.value_cells`, so its entity half is the DECLARING TEMPLATE's
//!   own name — a TYPE path, `<TemplateName>.<member>`.
//!
//! These two namespaces are not the same string for a cross-sub cell, and
//! esc-5098-6 is the review that found task #5098's GUI gate selecting pins
//! with values-namespace ids against constraints that only ever carry
//! instance-namespace ids, so every selection came back empty and the gate's
//! constraint half was silently inert at every phase. That gate,
//! `gui/test/visual/railLengtheningGate.mjs`, is not read by this file: it is
//! not on `main` (it lives on the unmerged `task/5098` branch). The four
//! scoped ids it needs are instead stated here as documented `const`s, cited
//! to their producers below, and proven by a real compile.
//!
//! # COMPILE ONLY
//!
//! Every test in this file compiles `prj/printer_v01/printer.ri` via
//! `reify_test_support::compile_source_with_stdlib` and inspects the
//! resulting `CompiledModule` only — never `check_source*`, `eval_source`, or
//! anything that reaches `reify_eval`. printer.ri SIGSEGVs the engine on the
//! eval/CSG path (#7383); a crashed test process reds the whole merge gate,
//! not just this file. This file and its helpers must never import
//! `reify_eval` or call a `check_*`/`eval_*` helper.

use reify_compiler::{CompiledConstraint, CompiledModule, TopologyTemplate};

// ── S1 contract constants — the CONSTRAINT namespace (`build_constraints`'s
// `parameter_ids`, gui/src-tauri/src/engine.rs:4997) for the two pins the
// task names. Each is `Printer.<sub_name>.<member>`, reached from source's
// `self.<sub>.<member>` via the `scoped_entity` lowering at
// crates/reify-compiler/src/expr.rs:4215 and :5909. These are the ids the
// GUI gate on the unmerged `task/5098` branch needs (cited via #5098 — not a
// `main` path; see this file's module doc for why).

/// `self.a_frame.rail_span_m` half of the rail-span pin —
/// `prj/printer_v01/printer.ri:3504`.
const PIN_RAIL_SPAN: &str = "Printer.a_frame.rail_span_m";

/// `self.motion.y_rail_len` half of the rail-span pin —
/// `prj/printer_v01/printer.ri:3504-3505`
/// (`constraint self.a_frame.rail_span_m <|> self.motion.y_rail_len +|- o1_pin_slack`).
const PIN_Y_RAIL_LEN: &str = "Printer.motion.y_rail_len";

/// `self.a_frame.travel_avail` half of the tool-dock pin —
/// `prj/printer_v01/printer.ri:3560-3561`.
const PIN_TRAVEL_AVAIL: &str = "Printer.a_frame.travel_avail";

/// `self.tool_dock.yh_min_today` half of the tool-dock pin —
/// `prj/printer_v01/printer.ri:3560-3561`
/// (`constraint self.tool_dock.yh_min_today <|> self.a_frame.centre_y -
/// self.a_frame.travel_avail / 2 +|- o1_pin_slack`).
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

/// Each pin, located by member shape alone, carries `Printer`-scoped
/// INSTANCE PATH ids in its `parameter_ids` — the half of
/// `build_constraints`'s output that drifted in esc-5098-6.
#[test]
fn pin_parameter_ids_are_printer_scoped_instance_paths() {
    let module = compiled_printer();
    let printer = module
        .templates
        .iter()
        .find(|t| t.name == "Printer")
        .unwrap_or_else(|| {
            panic!(
                "no template named 'Printer'; templates present: {:?}",
                module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
            )
        });

    for case in PIN_CASES {
        let located = pin_constraints_by_members(printer, case.member_shape);
        assert_eq!(
            located.len(),
            2,
            "{}: expected 2 constraints matching member shape {:?} (the `<`/`>` halves \
             of a two-sided pin — a pin removed from printer.ri must red here rather \
             than pass silently), found {}",
            case.pin_name,
            case.member_shape,
            located.len(),
        );

        let located_with_ids: Vec<(String, Vec<String>)> = located
            .into_iter()
            .map(|c| (c.id.to_string(), constraint_parameter_ids(c)))
            .collect();

        for required in case.required_scoped_ids.iter().copied() {
            let mut missing: Vec<&str> = Vec::new();
            for (cid, ids) in &located_with_ids {
                if !ids.iter().any(|id| id.as_str() == required) {
                    missing.push(cid.as_str());
                }
            }
            assert!(
                missing.is_empty(),
                "{}: required scoped id '{}' (values-namespace spelling: '{}') is MISSING \
                 from parameter_ids of constraint(s) {:?}. All constraints located by member \
                 shape {:?} and their actual parameter_ids: {:#?}",
                case.pin_name,
                required,
                values_namespace_spelling_hint(printer, required),
                missing,
                case.member_shape,
                located_with_ids,
            );
        }
    }
}

/// Best-effort `values`-namespace spelling of a `Printer.<sub>.<member>`
/// instance-path id, for FAILURE MESSAGES ONLY ("both spellings printed when
/// they diverge") — never an assertion. Resolves `<sub>` to its declared
/// structure name via `printer.sub_components`
/// (`SubComponentDecl { name, structure_name, .. }`,
/// crates/reify-compiler/src/types.rs:1080), the same field `build_values`
/// would need to render its own namespace (gui/src-tauri/src/engine.rs:4897).
/// Falls back to the raw sub name when unresolvable — e.g. against S1's
/// placeholder `compiled_printer` stub below, which declares no subs; S5/S6
/// add the real, asserted namespace correspondence.
fn values_namespace_spelling_hint(printer: &TopologyTemplate, scoped_id: &str) -> String {
    let mut parts = scoped_id.splitn(3, '.');
    let _printer_name = parts.next().unwrap_or_default();
    let sub_name = parts.next().unwrap_or_default();
    let member = parts.next().unwrap_or_default();
    let type_name = printer
        .sub_components
        .iter()
        .find(|s| s.name == sub_name)
        .map(|s| s.structure_name.as_str())
        .unwrap_or(sub_name);
    format!("{type_name}.{member}")
}

// ── S1 stubs — replaced with real bodies in S2 ──────────────────────────────

/// STUB (S1): compiles a trivial placeholder `Printer` structure so the test
/// above has a template to locate constraints against. Replaced in S2 with
/// the real `prj/printer_v01/printer.ri` compile, cached in the same
/// `OnceLock` so the (then expensive) compile still happens once per binary.
fn compiled_printer() -> &'static CompiledModule {
    static MODULE: std::sync::OnceLock<CompiledModule> = std::sync::OnceLock::new();
    MODULE.get_or_init(|| reify_test_support::compile_source_with_stdlib("structure Printer {}"))
}

/// STUB (S1): always empty regardless of `template`/`members` — this is what
/// makes `pin_parameter_ids_are_printer_scoped_instance_paths` RED. Real body
/// (the namespace-agnostic locator) lands in S2.
fn pin_constraints_by_members<'a>(
    _template: &'a TopologyTemplate,
    _members: &[&str],
) -> Vec<&'a CompiledConstraint> {
    Vec::new()
}

/// STUB (S1): always empty. Real body lands in S2.
fn constraint_parameter_ids(_c: &CompiledConstraint) -> Vec<String> {
    Vec::new()
}
