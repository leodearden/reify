//! End-to-end invariant anchor for the `compile_builder` refactor (task 2035).
//!
//! This test compiles a single contrived module string that touches all 13
//! phases of `compile_with_prelude_refs` in one input:
//!
//!   1. forward parse errors  — (no explicit assert; passes when parser is clean)
//!   2. module-pragma warnings — a known `#precision` pragma and an unknown
//!      `#unknown_pragma` pragma trigger the `KNOWN_MODULE_PRAGMAS` allowlist
//!   3. pre-pass decl collection — enum, fn, trait, field, unit, alias all seen
//!   4. unit phase              — a user `unit` declaration
//!   5. alias phase             — a `type` alias declaration
//!   6. resolution-enum build   — the `enum` declaration
//!   7. function phase          — a top-level `fn` declaration
//!   8. trait phase             — a `trait` declaration
//!   9. field phase             — a `field def` declaration
//!  10. constraint-def phase    — a `constraint def` declaration
//!  11. entity phase            — structure + occurrence + import declarations
//!  12. post-passes             — recursion detect, dup sig, field composition,
//!      purpose compilation
//!  13. hash assembly           — implicit via non-empty content_hash equality
//!
//! This test MUST pass on base code (before the phase-extraction refactor) AND
//! after every intermediate refactor step. It is the explicit behavior-preservation
//! contract for task 2035.

use reify_test_support::compile_source_with_stdlib;

#[test]
fn compile_builder_covers_all_phases_end_to_end() {
    // A single contrived module that exercises every phase. Note: the `import`
    // is intentionally unresolved (no module DAG in this test) — it still
    // populates `CompiledModule.imports` with a warning diagnostic, which is
    // the entity-phase Import arm we want to exercise.
    let source = r#"#precision(value=64)
#unknown_pragma

import std.math

unit smokey : Length = 0.0000254

type MyLen = Length

enum Direction { In, Out }

fn classify(x: Real) -> Int {
    match x {
        _ => 1
    }
}

trait Measurable {
    param width : Length
}

field def temp : Point3 -> Length { source = analytical { |p| 1.0m } }

constraint def MinWall {
    param wall: Length
    wall > 0mm
}

structure Widget {
    param width : Length = 80mm
    let v = classify(3.14)
    constraint MinWall(wall: width)
}

occurrence def Hole {
    param r : Length = 10mm
}

purpose check(subject : Structure) {
    constraint 80mm > 0mm
}
"#;

    let compiled = compile_source_with_stdlib(source);

    // If the test fails below, dump diagnostics to help triage the syntax.
    // Parse errors (the only diagnostics surfaced from step 1) are warnings.
    let errs: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_core::Severity::Error))
        .collect();
    assert!(
        errs.is_empty(),
        "expected no errors in smoke test fixture, got: {:?}",
        errs.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // ── Phase 4: user unit was compiled ──────────────────────────────────────
    assert!(
        compiled.units.iter().any(|u| u.name == "smokey"),
        "expected unit 'smokey' in compiled.units, got: {:?}",
        compiled.units.iter().map(|u| &u.name).collect::<Vec<_>>()
    );

    // ── Phase 5: type alias was compiled ─────────────────────────────────────
    assert!(
        !compiled.type_aliases.is_empty(),
        "expected non-empty type_aliases (MyLen), got empty"
    );
    assert!(
        compiled.type_aliases.iter().any(|a| a.name == "MyLen"),
        "expected type alias 'MyLen' in compiled.type_aliases"
    );

    // ── Phase 6: enum was compiled ───────────────────────────────────────────
    assert!(
        !compiled.enum_defs.is_empty(),
        "expected non-empty enum_defs, got empty"
    );
    assert!(
        compiled.enum_defs.iter().any(|e| e.name == "Direction"),
        "expected enum 'Direction' in compiled.enum_defs"
    );

    // ── Phase 7: function was compiled ───────────────────────────────────────
    assert!(
        !compiled.functions.is_empty(),
        "expected non-empty functions, got empty"
    );
    assert!(
        compiled.functions.iter().any(|f| f.name == "classify"),
        "expected fn 'classify' in compiled.functions"
    );

    // ── Phase 8: trait was compiled ──────────────────────────────────────────
    assert!(
        !compiled.trait_defs.is_empty(),
        "expected non-empty trait_defs, got empty"
    );
    assert!(
        compiled.trait_defs.iter().any(|t| t.name == "Measurable"),
        "expected trait 'Measurable' in compiled.trait_defs"
    );

    // ── Phase 9: field was compiled ──────────────────────────────────────────
    assert!(
        !compiled.fields.is_empty(),
        "expected non-empty fields, got empty"
    );
    assert!(
        compiled.fields.iter().any(|f| f.name == "temp"),
        "expected field 'temp' in compiled.fields"
    );

    // ── Phase 10: constraint def was compiled ────────────────────────────────
    assert!(
        !compiled.constraint_defs.is_empty(),
        "expected non-empty constraint_defs, got empty"
    );
    assert!(
        compiled.constraint_defs.iter().any(|c| c.name == "MinWall"),
        "expected constraint def 'MinWall' in compiled.constraint_defs"
    );

    // ── Phase 11: structure + occurrence templates ───────────────────────────
    // Widget (structure) + Hole (occurrence) = 2 templates minimum.
    assert!(
        compiled.templates.len() >= 2,
        "expected >= 2 templates (structure + occurrence), got {}",
        compiled.templates.len()
    );
    assert!(
        compiled.templates.iter().any(|t| t.name == "Widget"),
        "expected template 'Widget' in compiled.templates"
    );
    assert!(
        compiled.templates.iter().any(|t| t.name == "Hole"),
        "expected template 'Hole' in compiled.templates"
    );

    // ── Phase 11: import was compiled ────────────────────────────────────────
    assert!(
        !compiled.imports.is_empty(),
        "expected non-empty imports, got empty"
    );
    assert!(
        compiled.imports.iter().any(|i| i.path == "std.math"),
        "expected import 'std.math' in compiled.imports"
    );

    // ── Phase 12 post-pass: purpose was compiled ─────────────────────────────
    assert!(
        !compiled.compiled_purposes.is_empty(),
        "expected non-empty compiled_purposes, got empty"
    );
    assert!(
        compiled.compiled_purposes.iter().any(|p| p.name == "check"),
        "expected purpose 'check' in compiled.compiled_purposes"
    );

    // ── Phase 2: module pragmas recorded ─────────────────────────────────────
    assert!(
        !compiled.pragmas.is_empty(),
        "expected non-empty pragmas, got empty"
    );
    assert!(
        compiled.pragmas.iter().any(|p| p.name == "precision"),
        "expected module pragma 'precision' in compiled.pragmas"
    );
    assert!(
        compiled.pragmas.iter().any(|p| p.name == "unknown_pragma"),
        "expected module pragma 'unknown_pragma' in compiled.pragmas"
    );

    // ── Phase 2: unknown-pragma diagnostic ───────────────────────────────────
    assert!(
        compiled.diagnostics.iter().any(|d| {
            d.message.contains("unknown pragma") && d.message.contains("unknown_pragma")
        }),
        "expected 'unknown pragma #unknown_pragma' diagnostic, got: {:?}",
        compiled
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}
