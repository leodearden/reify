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
    let printer = printer_template(module);

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
/// Falls back to the raw sub name when unresolvable (e.g. a malformed
/// `scoped_id`). Reuses `sub_structure_name` for the sub→type lookup rather
/// than re-deriving it a second time; this helper's only remaining job is
/// the id-string split and the display fallback, never an assertion.
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
/// dir. Mirrors the `PRINTER_RI` constant in
/// `harness_constructor_typing/orientation_constructor_typing_tests.rs:220`
/// (the existing precedent for gating on a REAL design file), copied verbatim
/// so the two gates cannot disagree about which file they gate.
const PRINTER_RI: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../prj/printer_v01/printer.ri");

/// Compiles `PRINTER_RI` with the stdlib prelude exactly once per test
/// binary — a `OnceLock`, not a per-`#[test]` compile.
///
/// COMPILE ONLY: `compile_source_with_stdlib`, never `check_source*`,
/// `eval_source`, or anything reaching `reify_eval` — printer.ri SIGSEGVs the
/// engine on the eval/CSG path (#7383), and a crashed test process reds the
/// whole merge gate.
///
/// Deliberately asserts NOTHING about `.diagnostics` here, not even "zero
/// errors": that criterion belongs to the orientation gate
/// (`real_printer_ri_emits_zero_infer_warnings`), and inheriting it here
/// would make this namespace guard red on unrelated compiler warnings. Every
/// positive assertion in this file is non-vacuous by construction — if the
/// compile degraded, the pins would not be found and the tests would red
/// anyway.
fn compiled_printer() -> &'static CompiledModule {
    static MODULE: std::sync::OnceLock<CompiledModule> = std::sync::OnceLock::new();
    MODULE.get_or_init(|| {
        let source = std::fs::read_to_string(PRINTER_RI)
            .unwrap_or_else(|e| panic!("cannot read {PRINTER_RI}: {e}"));
        reify_test_support::compile_source_with_stdlib(&source)
    })
}

/// The top-level `Printer` template of `module`. Panics with the full list of
/// template names present when absent, so a rename of the top-level
/// structure reds legibly here rather than via an `unwrap` backtrace.
fn printer_template(module: &CompiledModule) -> &TopologyTemplate {
    module.templates.iter().find(|t| t.name == "Printer").unwrap_or_else(|| {
        panic!(
            "no template named 'Printer'; templates present: {:?}",
            module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
        )
    })
}

/// The NAMESPACE-AGNOSTIC pin locator — the seam that keeps this guard
/// non-circular (see module doc). Reads ONLY `ValueCellId::member` off each
/// constraint's value refs, never `.entity` or the rendered id, so it cannot
/// encode the assumption under test: member names are identical in both
/// namespaces, so selecting on them cannot smuggle in the scope half that
/// actually drifted in esc-5098-6.
///
/// A constraint matches when its ref-member set is a SUPERSET of `members`
/// (a pin's `<`/`>` halves each also reference unrelated cells, e.g.
/// `o1_pin_slack`) — a shape match, not equality.
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

/// The projection `build_constraints` publishes as `ConstraintData.parameter_ids`
/// (gui/src-tauri/src/engine.rs:8312, its local `collect_value_refs` wrapper)
/// — reimplemented here because that wrapper is a private `fn` in the `gui`
/// crate and cannot be called from `reify-compiler`. The shared primitive
/// that actually matters, `ValueCellId`'s `Display` impl
/// (`crates/reify-core/src/identity.rs:134`), IS reused rather than
/// re-spelled. Sort+dedup are cosmetic for this file's assertions, which
/// compare membership, not order.
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

/// The GUI's superset selector (`selectPinConstraints`,
/// `gui/test/visual/railLengtheningGate.mjs:409` on the unmerged `task/5098`
/// branch — cited via #5098, not a `main` path; see module doc), replayed in
/// Rust over the SAME compiled `Printer` template as the shape locator. This
/// is the assertion that would have caught esc-5098-6 directly: the previous
/// selector strings (values-namespace ids) matched no constraint's
/// `parameter_ids`, `foldPinStatus` reported PIN_ABSENT, and the gate's
/// constraint half was inert at every phase.
#[test]
fn gui_pin_selectors_select_exactly_the_shape_located_pins() {
    let module = compiled_printer();
    let printer = printer_template(module);

    for case in PIN_CASES {
        let shape_located = pin_constraints_by_members(printer, case.member_shape);
        let id_selected = select_pin_constraints(&printer.constraints, case.required_scoped_ids);

        // (a) Non-emptiness is asserted SEPARATELY from (b): an empty
        // selection is the PIN_ABSENT condition itself, and esc-5098-6 is
        // exactly a case where it must red loudly on its own rather than
        // fold into a same-message set-mismatch below.
        assert!(
            !id_selected.is_empty(),
            "{}: select_pin_constraints(&constraints, {:?}) selected ZERO constraints \
             — this is the PIN_ABSENT condition (esc-5098-6): the selector ids named a \
             namespace no constraint's parameter_ids carries. values-namespace spelling \
             of each selector id: {:?}. Constraints located by member shape {:?} (ground \
             truth) and their actual parameter_ids: {:#?}",
            case.pin_name,
            case.required_scoped_ids,
            case.required_scoped_ids
                .iter()
                .map(|id| values_namespace_spelling_hint(printer, id))
                .collect::<Vec<_>>(),
            case.member_shape,
            shape_located
                .iter()
                .map(|c| (c.id.to_string(), constraint_parameter_ids(c)))
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

/// The faithful Rust mirror of the GUI's `selectPinConstraints` (cited via
/// #5098 — not a `main` path; see module doc): keeps a constraint when EVERY
/// entry of `cells` is present in its `constraint_parameter_ids`, matching
/// `cells.every((cell) => ids.includes(cell))` — superset, not equality,
/// because a pin expression legitimately also names `Printer.o1_pin_slack`
/// (and, for the tool-dock pin, `Printer.a_frame.centre_y`). Do not
/// "improve" this into equality or prefix matching: the value of this guard
/// is that it runs the SAME predicate the GUI runs, so a divergence the GUI
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

/// The four `(sub_name, member)` cells this file pins — the same ones the
/// `PIN_CASES` member shapes name — reused here to derive BOTH namespace
/// spellings from the SAME compile rather than asserting the correspondence
/// by hand.
const PINNED_CELLS: &[(&str, &str)] = &[
    ("a_frame", "rail_span_m"),
    ("motion", "y_rail_len"),
    ("a_frame", "travel_avail"),
    ("tool_dock", "yh_min_today"),
];

/// The two namespaces are distinct for every pinned cell, and the
/// correspondence between them — which `Printer.<sub>.<member>` instance
/// path names the same underlying cell as which `<TemplateName>.<member>`
/// values-namespace id — is DERIVED from `sub_components`/`value_cells`,
/// never hand-kept anywhere in this file. S1-S4 alone pin one namespace;
/// this test is what makes it impossible to confuse the two.
#[test]
fn values_namespace_is_distinct_from_and_corresponds_to_the_constraint_namespace() {
    let module = compiled_printer();
    let printer = printer_template(module);

    for &(sub_name, member) in PINNED_CELLS {
        // (a) the sub resolves to its declared structure — the entity half
        // of a constraint ref is the SUB NAME, not the TYPE NAME, which is
        // precisely the confusion behind esc-5098-6. The expected type name
        // is never hard-coded here: whichever `type_name` this derives is
        // exactly what (b) below must find `member` declared on.
        let type_name = sub_structure_name(printer, sub_name).unwrap_or_else(|| {
            panic!(
                "sub_structure_name(printer, {sub_name:?}) returned None; \
                 Printer.sub_components: {:?}",
                printer
                    .sub_components
                    .iter()
                    .map(|s| (s.name.as_str(), s.structure_name.as_str()))
                    .collect::<Vec<_>>()
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
        let case = PIN_CASES.iter().find(|c| c.member_shape.contains(&member)).unwrap_or_else(
            || panic!("no PinCase has member {member:?} in its member_shape"),
        );
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
        // `<TemplateName>.<member>` — mirrors `build_values`
        // (gui/src-tauri/src/engine.rs:4897).
        let values_spelling =
            values_namespace_id(module, type_name, member).unwrap_or_else(|| {
                panic!("values_namespace_id(module, {type_name:?}, {member:?}) returned None")
            });
        eprintln!(
            "EVIDENCE {sub_name} -> {type_name}: constraint={constraint_spelling} values={values_spelling}"
        );
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

/// The derived sub→type map: `sub_name`'s declared structure in `template`,
/// e.g. `a_frame` → `AFrame`. This is the single fact that makes the
/// correspondence between the two namespaces DERIVED rather than hand-kept —
/// nothing in this file hard-codes `a_frame` → `AFrame` anywhere; every use
/// of a type name upstream of this function's return value traces back to
/// `SubComponentDecl::structure_name` (crates/reify-compiler/src/types.rs:1080).
fn sub_structure_name<'a>(template: &'a TopologyTemplate, sub_name: &str) -> Option<&'a str> {
    template.sub_components.iter().find(|s| s.name == sub_name).map(|s| s.structure_name.as_str())
}

/// The values-namespace id of the value cell named `member` on the template
/// named `structure_name`, mirroring `build_values`
/// (gui/src-tauri/src/engine.rs:4897): walks that template's OWN
/// `value_cells`, so the entity half of the returned id is the DECLARING
/// TEMPLATE's name. Deliberately not `reify_test_support::get_value_cell_in`
/// / `get_let_expr_in`: those resolve by `id.member` alone across the WHOLE
/// module and `#[track_caller]`-panic on ambiguity, whereas this lookup must
/// stay scoped to one named template (several templates in printer.ri
/// declare a `travel_avail`-shaped member family) and must return `Option`
/// so callers can report a clean miss instead of panicking inside a helper.
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
