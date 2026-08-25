//! Compile-surface contract for `examples/fea_bracket_minimize_mass.ri` (task 2930).
//!
//! The example is the task deliverable: a bracket whose `thickness` is an `auto`
//! design variable resolved by an FEA-driven optimisation loop — `minimize mass`
//! opposed by a von-Mises stress constraint built on a real `solve_elastic_static`
//! call. The end-to-end *convergence* signal (that the loop actually resolves a
//! finite, interior thickness) lives in the eval layer at
//! `crates/reify-eval/tests/harness_fea_solver_e2e/fea_bracket_minimize_mass_e2e.rs`,
//! because it costs a real Nelder-Mead-over-FEA run. THIS module is the cheap
//! compile-surface half: it pins the example's *structure* so a future edit cannot
//! silently degrade the demo into something that still compiles but no longer
//! demonstrates anything.
//!
//! # What is pinned, and why each pin is load-bearing
//!
//!   (a) the file parses with zero errors;
//!   (b) it compiles under the stdlib prelude with ZERO Error-severity diagnostics.
//!       This is also the gate that catches a dimensional-typing mismatch in the
//!       stress predicate: the compile-time analysis signatures have no `Type::Field`
//!       arm, so a field-valued `von_mises(...)` can type as dimensionless `Real`
//!       and make `< yield_limit` (a `Pressure`) a dimensional-mismatch Error;
//!   (c) the compiled module exposes the bracket structure template;
//!   (d) `thickness` is a solver-delegated `ValueCellKind::Auto` cell. Pinning the
//!       COMPILED KIND rather than the source text `= auto` means a rename or a
//!       reformat cannot satisfy it, and a downgrade to a fixed `param thickness :
//!       Length = 6mm` (which would still compile and still "run") fails here;
//!   (e) the template carries an `ObjectiveSet` with exactly one
//!       `ObjectiveSense::Minimize` term — pins the `minimize` by compiled property;
//!   (f) the template carries AT LEAST ONE compiled constraint.
//!
//! Pin (f) is the structural guard against a specific, silent trap. Reify's grammar
//! accepts `minimize <expr> where <cond>`, and the parser dutifully lowers the guard
//! into `MinimizeDecl.where_clause` — but the compiler's Minimize/Maximize lowering
//! arms never read that field (every OTHER member kind routes its guard through
//! `compile_per_decl_guard`; these two do not). A `minimize mass where <stress
//! predicate>` therefore compiles to an objective with the predicate DISCARDED and
//! ZERO constraints. The optimiser would then run unopposed, drive `thickness` to
//! the ~1 micron auto-param lower bound, and "converge" — a false green that a naive
//! test would pass. So this module additionally asserts, on the raw source text, that
//! no `minimize` statement carries a `where` guard: the stress predicate must be a
//! separate `constraint` MEMBER, which is the only form the compiler actually honours.

use reify_compiler::ValueCellKind;
use reify_core::{ModulePath, Severity};
use reify_ir::ObjectiveSense;

/// Path to the example, resolved from `CARGO_MANIFEST_DIR` so it works in any
/// worktree (mirrors `multi_load_bracket_example_tests.rs`).
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/fea_bracket_minimize_mass.ri"
);

/// The structure declared by the example.
const TEMPLATE_NAME: &str = "FeaBracketMinimizeMass";

#[test]
fn fea_bracket_minimize_mass_example_compiles_and_pins_the_design_loop() {
    let src = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "failed to read examples/fea_bracket_minimize_mass.ri — it is task 2930's \
         deliverable; check CARGO_MANIFEST_DIR resolution if the file does exist",
    );

    // ── (a) Parse ────────────────────────────────────────────────────────────
    //
    // Via `parse_with_stdlib`, NOT bare `reify_syntax::parse`: the example
    // references `ShellForce.Off`, an enum declared in the stdlib's
    // `solver_elastic.ri`. Without prelude-enum seeding the parser lowers that to a
    // `MemberAccess` instead of an `EnumAccess`, which then fails compilation with
    // "unresolved name: ShellForce". The production path (`compile_with_stdlib`)
    // always parses with stdlib enum awareness, so the test must mirror it.
    let parsed =
        reify_compiler::parse_with_stdlib(&src, ModulePath::single("fea_bracket_minimize_mass"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in fea_bracket_minimize_mass.ri: {:?}",
        parsed.errors
    );

    // ── (b) Compile with zero Error diagnostics ──────────────────────────────

    let module = reify_compiler::compile_with_stdlib(&parsed);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics compiling fea_bracket_minimize_mass.ri under \
         stdlib, got:\n{:#?}",
        errors
    );

    // ── (c) Template presence ────────────────────────────────────────────────

    let bracket = module
        .templates
        .iter()
        .find(|t| t.name == TEMPLATE_NAME)
        .unwrap_or_else(|| {
            panic!(
                "{TEMPLATE_NAME} template should be present in compiled \
                 fea_bracket_minimize_mass.ri; found templates: {:?}",
                module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
            )
        });

    // ── (d) `thickness` is a solver-delegated auto param ─────────────────────
    //
    // Compiled-KIND assertion, not a source-text one: `= auto` in a comment cannot
    // satisfy it, and a downgrade to a fixed numeric `param thickness : Length = 6mm`
    // — which would still compile and still produce a working FEA solve — fails here.
    // That downgrade is exactly the degradation this example exists to prevent: with
    // a fixed thickness there is no design loop left to demonstrate.
    let thickness = bracket
        .value_cells
        .iter()
        .find(|c| c.id.member == "thickness")
        .unwrap_or_else(|| {
            panic!(
                "{TEMPLATE_NAME} should carry a 'thickness' value cell; found cells: {:?}",
                bracket
                    .value_cells
                    .iter()
                    .map(|c| &c.id.member)
                    .collect::<Vec<_>>()
            )
        });
    assert!(
        thickness.kind.is_auto(),
        "leaf signal 'auto thickness': {TEMPLATE_NAME}.thickness must compile to a \
         solver-delegated ValueCellKind::Auto cell (strict `auto` or `auto(free)`), got \
         {:?} — a fixed numeric thickness still compiles but leaves no design loop to \
         demonstrate",
        thickness.kind
    );
    assert!(
        matches!(thickness.kind, ValueCellKind::Auto { .. }) && thickness.default_expr.is_none(),
        "an auto param carries no default expression (the solver supplies the value); \
         got kind {:?} with default_expr {:?}",
        thickness.kind,
        thickness.default_expr
    );

    // ── (e) A single Minimize objective ──────────────────────────────────────

    let objective = bracket.objective.as_ref().unwrap_or_else(|| {
        panic!(
            "leaf signal 'minimize mass': {TEMPLATE_NAME} should carry a compiled \
             ObjectiveSet; got None — the `minimize` member did not lower"
        )
    });
    assert_eq!(
        objective.terms.len(),
        1,
        "expected exactly one objective term (minimize mass), got {:?}",
        objective
            .terms
            .iter()
            .map(|t| t.sense)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        objective.terms[0].sense,
        ObjectiveSense::Minimize,
        "the objective must MINIMIZE (mass); maximizing would invert the whole demo"
    );

    // ── (f) At least one compiled constraint ─────────────────────────────────
    //
    // THE structural guard. See the module doc: a `minimize … where <predicate>`
    // form parses, lowers its guard into `MinimizeDecl.where_clause`, and is then
    // silently DISCARDED by the Minimize lowering arm — yielding an objective with
    // ZERO constraints. That shape compiles clean, passes (a)-(e), and produces an
    // unopposed optimiser that parks `thickness` at its lower bound. Requiring a real
    // compiled constraint is what makes that failure visible at compile-surface cost
    // rather than only after a ~490s eval run.
    assert!(
        !bracket.constraints.is_empty(),
        "leaf signal 'binding stress constraint': {TEMPLATE_NAME} must carry at least one \
         COMPILED constraint — the von-Mises-vs-yield predicate has to be a separate \
         `constraint` member. If it were written as a `minimize … where …` guard instead, \
         the guard would be parsed and then silently dropped by the compiler's Minimize \
         lowering arm, leaving the objective unopposed."
    );

    // ── Source-text guard against reintroducing the no-op `where` form ───────
    //
    // (f) catches the pure `minimize … where …` shape (zero constraints). This
    // additionally catches the MIXED shape — a real `constraint` member PLUS a
    // decorative `where` guard on the objective — which would satisfy (f) while
    // still misleading the next reader into believing objective guards are honoured.
    // Scanning per-statement (rather than the whole file) keeps an unrelated `where`
    // elsewhere in the file from tripping this.
    for (lineno, line) in src.lines().enumerate() {
        let code = line.split("//").next().unwrap_or("");
        let is_minimize_stmt = code
            .trim_start()
            .strip_prefix("minimize")
            .is_some_and(|rest| rest.starts_with(char::is_whitespace));
        assert!(
            !(is_minimize_stmt && code.contains(" where ")),
            "line {}: `minimize … where …` is a SILENT NO-OP — the compiler's Minimize \
             lowering arm never reads `MinimizeDecl.where_clause`, so the guard is parsed \
             and then discarded, leaving the objective unopposed. Express the stress \
             predicate as a separate `constraint` member instead. Offending line: {:?}",
            lineno + 1,
            line
        );
    }
}
