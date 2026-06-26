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
