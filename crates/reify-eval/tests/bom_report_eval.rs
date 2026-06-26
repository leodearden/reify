//! Runtime evaluation tests for the BOM / cost / waste / provenance rollup
//! (`reify report --bom`, io-lifecycle-bom-cost #4292).
//!
//! These lock the `Engine::build_bom_report` rollup end-to-end against plain
//! `engine.eval` (no geometry kernel): a `Costed`-conforming line item rolls up
//! into a BOM line with its `line_cost`, a `Discard` becomes a waste row, and an
//! `Input` (e.g. `STEPInput`) becomes a provenance row. The cost path reuses the
//! same Money-scalar substrate as `cost_aggregation_eval.rs`.
//!
//! Harness mirrors `cost_aggregation_eval.rs`: `parse_and_compile_with_stdlib`
//! + `make_simple_engine` (the io traits live in the stdlib prelude).

use reify_eval::{BomLine, BomReport};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

/// Absolute path to the canonical BOM-lifecycle example fixture (ε row).
/// Mirrors the `CARGO_MANIFEST_DIR` pattern from `cost_aggregation_eval.rs`.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/bom_lifecycle.ri"
);

/// Compile `source` with the stdlib prelude, eval it (asserting zero Error
/// diagnostics), and roll up the BOM report. Shared by every test below.
fn build_report(source: &str) -> BomReport {
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "expected zero Error diagnostics from eval, got: {:#?}",
        eval_errors
    );
    engine.build_bom_report(&compiled, &result.values)
}

/// Locate the single BOM line for sub field `sub`. Panics with the available
/// lines on miss so a structural regression reports what *was* enumerated.
fn line_for<'a>(report: &'a BomReport, sub: &str) -> &'a BomLine {
    report.lines.iter().find(|l| l.sub == sub).unwrap_or_else(|| {
        panic!(
            "no BOM line for sub {:?}; lines: {:?}",
            sub,
            report.lines.iter().map(|l| &l.sub).collect::<Vec<_>>()
        )
    })
}

// ─── step-3: two Costed line items roll up to a Money grand total ────────────

/// Two `Costed : Buy` structure-def subs of a `Widget` enumerate (in
/// declaration order) into exactly two BOM lines carrying
/// supplier/part_number/unit_cost/quantity/line_total, and their determined
/// `line_cost`s (5.00 + 6.00) sum into an 11.00-USD grand total.
///
/// USD has factor 1.0 (units.ri), so the SI magnitude IS the USD value: the
/// `< 1e-9` asserts are exact (0.5·10 = 5.0, 3.0·2 = 6.0, both binary-exact).
#[test]
fn bom_report_two_costed_lines_roll_up_to_money_total() {
    let report = build_report(
        r#"
structure def Bolt : Costed {
    param supplier          : String = "Fastenal"
    param part_number       : String = "BOLT-M6-20"
    param unit_cost         : Money  = 0.50USD
    param lead_time         : Time   = 24h
    param quantity_produced : Real   = 10.0
}

structure def Plate : Costed {
    param supplier          : String = "Ryerson"
    param part_number       : String = "PLATE-A36-6"
    param unit_cost         : Money  = 3.00USD
    param lead_time         : Time   = 72h
    param quantity_produced : Real   = 2.0
}

structure def Widget {
    sub bolts = Bolt()
    sub plate = Plate()
}
"#,
    );

    // Exactly two lines, enumerated in declaration order.
    assert_eq!(
        report.lines.len(),
        2,
        "expected exactly 2 BOM lines, got: {:?}",
        report.lines
    );
    assert_eq!(report.lines[0].sub, "bolts", "line 0 must be the first sub");
    assert_eq!(report.lines[1].sub, "plate", "line 1 must be the second sub");

    // Line item identity: owner template, sub field, resolved type.
    let bolt = line_for(&report, "bolts");
    assert_eq!(bolt.entity, "Widget");
    assert_eq!(bolt.type_name, "Bolt");
    assert_eq!(bolt.supplier, "Fastenal");
    assert_eq!(bolt.part_number, "BOLT-M6-20");
    assert!(!bolt.undetermined, "Bolt has a determined cost");
    assert!(
        (bolt.unit_cost.unwrap() - 0.50).abs() < 1e-9,
        "Bolt unit_cost = 0.50 USD, got {:?}",
        bolt.unit_cost
    );
    assert!(
        (bolt.quantity.unwrap() - 10.0).abs() < 1e-9,
        "Bolt quantity_produced = 10.0, got {:?}",
        bolt.quantity
    );
    assert!(
        (bolt.line_total.unwrap() - 5.00).abs() < 1e-9,
        "Bolt line_cost = 0.50 * 10 = 5.00 USD, got {:?}",
        bolt.line_total
    );

    let plate = line_for(&report, "plate");
    assert_eq!(plate.type_name, "Plate");
    assert_eq!(plate.part_number, "PLATE-A36-6");
    assert!(
        (plate.line_total.unwrap() - 6.00).abs() < 1e-9,
        "Plate line_cost = 3.00 * 2 = 6.00 USD, got {:?}",
        plate.line_total
    );

    // Grand total = 5.00 + 6.00 = 11.00 USD (SI magnitude, factor 1.0).
    let total = report.total.expect("two determined lines ⇒ Some(total)");
    assert!(
        (total - 11.00).abs() < 1e-9,
        "expected grand total 11.00 USD, got {}",
        total
    );
}

// ─── step-5: a Discard sub becomes a waste row ──────────────────────────────

/// A `Discard : Sink` structure-def sub enumerates into exactly one
/// `WasteEntry` carrying its `reason` / `disposal_method` enum variants — and
/// does NOT appear as a BOM line (a Discard is not a Buy).
#[test]
fn bom_report_discard_sub_becomes_waste_entry() {
    let report = build_report(
        r#"
structure def ScrapOffcut : Discard {
    param reason          : DiscardReason  = DiscardReason.Offcut
    param disposal_method : DisposalMethod = DisposalMethod.Recycle
}

structure def Widget {
    sub scrap = ScrapOffcut()
}
"#,
    );

    assert_eq!(
        report.waste.len(),
        1,
        "expected exactly 1 waste entry, got: {:?}",
        report.waste
    );
    let w = &report.waste[0];
    assert_eq!(w.entity, "Widget");
    assert_eq!(w.sub, "scrap");
    assert_eq!(w.type_name, "ScrapOffcut");
    assert_eq!(w.reason, "Offcut", "Discard.reason variant");
    assert_eq!(w.disposal_method, "Recycle", "Discard.disposal_method variant");

    assert!(
        report.lines.is_empty(),
        "a Discard must not be counted as a BOM line, got: {:?}",
        report.lines
    );
}

/// A design with no `Discard` sub produces an empty waste section.
#[test]
fn bom_report_no_discard_means_empty_waste() {
    let report = build_report(
        r#"
structure def Bolt : Costed {
    param supplier          : String = "Fastenal"
    param part_number       : String = "BOLT-M6-20"
    param unit_cost         : Money  = 0.50USD
    param lead_time         : Time   = 24h
    param quantity_produced : Real   = 10.0
}

structure def Widget {
    sub bolts = Bolt()
}
"#,
    );

    assert!(
        report.waste.is_empty(),
        "no Discard sub ⇒ empty waste, got: {:?}",
        report.waste
    );
}

// ─── step-7: an Input sub becomes a provenance row ──────────────────────────

/// An `Input : Source` sub (here the `STEPInput` occurrence) enumerates into
/// exactly one `ProvenanceEntry` carrying its `source` and the nested
/// `provenance.source_tool` (the STEPInput default `"step-import"`) — and is
/// neither a BOM line nor a waste row.
#[test]
fn bom_report_input_sub_becomes_provenance_entry() {
    let report = build_report(
        r#"
structure def Widget {
    sub imported = STEPInput(source: "incoming.step")
}
"#,
    );

    assert_eq!(
        report.provenance.len(),
        1,
        "expected exactly 1 provenance entry, got: {:?}",
        report.provenance
    );
    let p = &report.provenance[0];
    assert_eq!(p.entity, "Widget");
    assert_eq!(p.sub, "imported");
    assert_eq!(p.type_name, "STEPInput");
    assert_eq!(p.source, "incoming.step", "Input.source");
    assert_eq!(
        p.source_tool, "step-import",
        "STEPInput default provenance.source_tool"
    );

    assert!(
        report.lines.is_empty(),
        "an Input must not be a BOM line, got: {:?}",
        report.lines
    );
    assert!(
        report.waste.is_empty(),
        "an Input must not be a waste row, got: {:?}",
        report.waste
    );
}

/// A design with no `Input` sub produces an empty provenance section.
#[test]
fn bom_report_no_input_means_empty_provenance() {
    let report = build_report(
        r#"
structure def ScrapOffcut : Discard {
    param reason          : DiscardReason  = DiscardReason.Offcut
    param disposal_method : DisposalMethod = DisposalMethod.Recycle
}

structure def Widget {
    sub scrap = ScrapOffcut()
}
"#,
    );

    assert!(
        report.provenance.is_empty(),
        "no Input sub ⇒ empty provenance, got: {:?}",
        report.provenance
    );
}

// ─── step-9: an undetermined-cost line is flagged and excluded ──────────────

/// A `Costed` line whose `unit_cost` is `undef` must build without panic, be
/// flagged `undetermined` with `None` cost fields, be EXCLUDED from the grand
/// total, and surface a warning naming it — while a sibling determined line
/// still sums. `undef * quantity_produced` propagates to `Undef` (no Error
/// diagnostic), so the rollup must read the cost fields tolerantly.
#[test]
fn bom_report_undetermined_cost_line_is_flagged_and_excluded() {
    let report = build_report(
        r#"
structure def MysteryPart : Costed {
    param supplier          : String = "Unknown"
    param part_number       : String = "MYSTERY-1"
    param unit_cost         : Money  = undef
    param lead_time         : Time   = 0h
    param quantity_produced : Real   = 3.0
}

structure def KnownPart : Costed {
    param supplier          : String = "Fastenal"
    param part_number       : String = "BOLT-M6-20"
    param unit_cost         : Money  = 0.50USD
    param lead_time         : Time   = 24h
    param quantity_produced : Real   = 10.0
}

structure def Widget {
    sub mystery = MysteryPart()
    sub known   = KnownPart()
}
"#,
    );

    assert_eq!(
        report.lines.len(),
        2,
        "both lines enumerate: {:?}",
        report.lines
    );

    let mystery = line_for(&report, "mystery");
    assert!(mystery.undetermined, "undef unit_cost ⇒ undetermined line");
    assert!(mystery.unit_cost.is_none(), "undetermined ⇒ unit_cost None");
    assert!(mystery.line_total.is_none(), "undetermined ⇒ line_total None");

    let known = line_for(&report, "known");
    assert!(!known.undetermined);
    assert!((known.line_total.unwrap() - 5.00).abs() < 1e-9);

    // The grand total EXCLUDES the undetermined line: 5.00, not 5.00 + undef.
    let total = report
        .total
        .expect("one determined line ⇒ Some(total) even with an undetermined sibling");
    assert!(
        (total - 5.00).abs() < 1e-9,
        "total must exclude the undetermined line, got {}",
        total
    );

    // A warning names the undetermined line (by part_number or sub).
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("MYSTERY-1") || w.contains("mystery")),
        "expected a warning naming the undetermined line, got: {:?}",
        report.warnings
    );
}

// ─── step-11: examples/bom_lifecycle.ri renders all three sections (ε) ───────

/// The canonical example file rolls up + renders the full BOM/cost/waste/
/// provenance report. Locks `BomReport::render()` at the eval layer (the CLI
/// `cli_report.rs` later locks the same strings through the binary): a "Bill of
/// Materials" header, the Bolt/Plate part numbers, a `Total: 11.00 USD` line, a
/// waste line naming Offcut/Recycle, and a provenance line naming incoming.step.
#[test]
fn bom_lifecycle_example_renders_all_three_sections() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .unwrap_or_else(|e| panic!("failed to read examples/bom_lifecycle.ri: {e}"));
    let report = build_report(&source);
    let text = report.render();

    // Bill of Materials section + both line items by part number.
    assert!(
        text.contains("Bill of Materials"),
        "render must carry a BOM header, got:\n{text}"
    );
    assert!(
        text.contains("BOLT-M6-20"),
        "render must list the Bolt part number, got:\n{text}"
    );
    assert!(
        text.contains("PLATE-A36-6"),
        "render must list the Plate part number, got:\n{text}"
    );

    // Grand total, formatted to 2 decimals with the USD suffix.
    assert!(
        text.contains("Total: 11.00 USD"),
        "render must carry the 11.00 USD grand total, got:\n{text}"
    );

    // Waste / Discard section naming the discard's reason + disposal method.
    assert!(
        text.contains("Offcut"),
        "render must name the discard reason Offcut, got:\n{text}"
    );
    assert!(
        text.contains("Recycle"),
        "render must name the disposal method Recycle, got:\n{text}"
    );

    // Provenance section naming the imported source.
    assert!(
        text.contains("incoming.step"),
        "render must name the imported provenance source, got:\n{text}"
    );
}
