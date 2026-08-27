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
//!   (d) `thickness` is a solver-delegated `ValueCellKind::Auto { free: true }` cell.
//!       Pinning the COMPILED KIND rather than the source text `= auto` means a
//!       rename or a reformat cannot satisfy it, and a downgrade to a fixed
//!       `param thickness : Length = 6mm` (which would still compile and still
//!       "run") fails here. Free-ness is pinned too, and is a RUNTIME-COST
//!       contract: strict `auto` adds a perturbation-based uniqueness re-solve —
//!       a second full Nelder-Mead run, so roughly double the real FEA solves —
//!       which would blow past the measured cost recorded for the e2e test's atom
//!       in `scripts/heavy-test-filter-lib.sh`;
//!   (e) the template carries an `ObjectiveSet` with exactly one
//!       `ObjectiveSense::Minimize` term — pins the `minimize` by compiled property;
//!   (f) the template carries EXACTLY ONE compiled constraint, and that constraint's
//!       compiled expression actually reaches `solve_elastic_static`.
//!
//! Pin (f) is the structural guard against a specific, silent trap. Reify's grammar
//! accepts `minimize <expr> where <cond>`, and the parser dutifully lowers the guard
//! into `MinimizeDecl.where_clause` — but the compiler's Minimize/Maximize lowering
//! arms never read that field (every OTHER member kind routes its guard through
//! `compile_per_decl_guard`; these two do not). A `minimize mass where <stress
//! predicate>` therefore compiles to an objective with the predicate DISCARDED and
//! ZERO constraints. The optimiser would then run unopposed, drive `thickness` to
//! the ~1 micron auto-param lower bound, and "converge" — a false green that a naive
//! test would pass.
//!
//! A bare "at least one constraint" pin is too weak to catch the whole family: swap
//! the von-Mises predicate for `constraint thickness > 1mm` and the FEA leaves the
//! design loop entirely while every other pin stays green. So (f) pins the count AND
//! that the surviving constraint's compiled expression tree contains the
//! `solve_elastic_static` call — which is what makes the loop close through a real
//! analysis rather than through an arbitrary inequality.
//!
//! Finally, this module walks the PARSED AST and asserts no `minimize`/`maximize`
//! member carries a `where_clause` at all. That covers the MIXED shape — a real
//! `constraint` member PLUS a decorative objective guard — which satisfies (f) while
//! still teaching the next reader that objective guards are honoured. It is an AST
//! walk rather than a source-text scan deliberately: a text scan is line-local and
//! spelling-sensitive, so the natural wrapped formatting (`minimize mass` on one line,
//! `where <predicate>` on the next) and the identically-broken `maximize` arm both
//! escape it.

use reify_ast::{Declaration, MemberDecl};
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
    // Free-ness is a RUNTIME-COST contract, not a taste preference. Strict `auto`
    // triggers a perturbation-based uniqueness re-solve (`verify_uniqueness` — a
    // second full Nelder-Mead run), roughly DOUBLING the number of real FEA solves
    // in the e2e convergence test. That test's heavy-filter atom in
    // `scripts/heavy-test-filter-lib.sh` records a MEASURED cost premised on the
    // free form; silently promoting this param to strict `auto` would falsify it.
    // The example's own comment states the same rationale — this pins it.
    assert!(
        thickness.kind.is_auto_free(),
        "{TEMPLATE_NAME}.thickness must be `auto(free)`, not strict `auto`; got {:?}. \
         Strict `auto` adds a uniqueness re-solve that roughly doubles the real FEA \
         solves in fea_bracket_minimize_mass_e2e, falsifying the measured cost recorded \
         for its atom in scripts/heavy-test-filter-lib.sh. The design loop here is \
         single-variable and monotone, so the stress-limited optimum is unique by \
         construction and the re-solve buys no signal.",
        thickness.kind
    );
    assert!(
        thickness.default_expr.is_none(),
        "an auto param carries no default expression (the solver supplies the value); \
         got default_expr {:?}",
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

    // ── (f) Exactly one constraint, and it reaches the FEA call ──────────────
    //
    // THE structural guard. See the module doc: a `minimize … where <predicate>`
    // form parses, lowers its guard into `MinimizeDecl.where_clause`, and is then
    // silently DISCARDED by the Minimize lowering arm — yielding an objective with
    // ZERO constraints. That shape compiles clean, passes (a)-(e), and produces an
    // unopposed optimiser that parks `thickness` at its lower bound.
    //
    // The COUNT is pinned exactly rather than as a lower bound: the example declares
    // one design predicate, and a second one appearing is a change to the demo's
    // meaning that should be read, not absorbed silently.
    assert_eq!(
        bracket.constraints.len(),
        1,
        "leaf signal 'binding stress constraint': {TEMPLATE_NAME} must carry EXACTLY ONE \
         COMPILED constraint — the von-Mises-vs-yield predicate, as a separate \
         `constraint` member. Zero means it was written as a `minimize … where …` guard, \
         which the compiler's Minimize lowering arm parses and then silently drops, \
         leaving the objective unopposed. More than one means the demo grew a second \
         predicate. Got: {:?}",
        bracket
            .constraints
            .iter()
            .map(|c| &c.label)
            .collect::<Vec<_>>()
    );

    // …and the surviving constraint must actually close the loop through the FEA.
    // Without this, `constraint thickness > 1mm` satisfies every pin above while
    // removing `solve_elastic_static` from the design loop entirely — precisely the
    // silent degradation this module exists to catch, and one the ~88s e2e test can
    // no longer be relied on to catch alone now that it is in the heavy set and off
    // the merge gate.
    //
    // Matched on the compiled expression's Debug rendering rather than a hand-rolled
    // `CompiledExpr` walk: the call is nested under a `BinOp`/member access today,
    // and a walk enumerating only today's spine would go quietly vacuous if the
    // predicate were later restructured (say, wrapped in a `min(...)` reduction).
    // `UserFunctionCall.function_name` is a `String` field, so the name is present
    // in the Debug output of any tree shape that contains the call.
    const FEA_CALL: &str = "solve_elastic_static";
    let predicate = format!("{:?}", bracket.constraints[0].expr);
    assert!(
        predicate.contains(FEA_CALL),
        "leaf signal 'the loop closes through a real analysis': \
         {TEMPLATE_NAME}'s single constraint must call `{FEA_CALL}`. A predicate that \
         compiles, binds, and converges — `constraint thickness > 1mm`, say — keeps every \
         other pin in this module green while dropping the FEA out of the design loop, \
         which leaves the example demonstrating nothing it claims to. Compiled predicate \
         was: {predicate}"
    );

    // ── AST guard against reintroducing the no-op `where` form ───────────────
    //
    // (f) catches the pure `minimize … where …` shape (zero constraints). This
    // additionally catches the MIXED shape — a real `constraint` member PLUS a
    // decorative `where` guard on the objective — which would satisfy (f) while
    // still misleading the next reader into believing objective guards are honoured.
    //
    // Walked over the parsed AST rather than scanned over `src`: a per-line text
    // scan is spelling- and formatting-sensitive, so the wrapped form (`minimize
    // mass` on one line, `where <predicate>` on the next — the natural formatting
    // once the predicate is long) slips past it, as does `maximize … where …`,
    // which carries the identical dropped-guard bug.
    for decl in &parsed.declarations {
        let members = match decl {
            Declaration::Structure(s) => &s.members,
            Declaration::Occurrence(o) => &o.members,
            _ => continue,
        };
        assert_no_objective_guard(members);
    }
}

/// Assert no `minimize`/`maximize` member in `members` (or in any nested member
/// block) carries a `where` guard.
///
/// Recursive because a guard buried in a `where … { }` group or a `sub … { }` body
/// is dropped just as silently as a top-level one — the compiler's Minimize and
/// Maximize lowering arms never read `where_clause` at any depth. Every `MemberDecl`
/// variant that can nest further members is descended into; the rest cannot hold an
/// objective and are skipped.
fn assert_no_objective_guard(members: &[MemberDecl]) {
    const WHY: &str = "`minimize … where …` / `maximize … where …` is a SILENT NO-OP — \
                       the compiler's Minimize and Maximize lowering arms never read \
                       `where_clause`, so the guard is parsed and then discarded, leaving \
                       the objective unopposed. Express the predicate as a separate \
                       `constraint` member instead, which is the only form the compiler \
                       honours.";
    for member in members {
        match member {
            MemberDecl::Minimize(m) => assert!(
                m.where_clause.is_none(),
                "{WHY} Offending `minimize` at {:?}",
                m.span
            ),
            MemberDecl::Maximize(m) => assert!(
                m.where_clause.is_none(),
                "{WHY} Offending `maximize` at {:?}",
                m.span
            ),
            MemberDecl::GuardedGroup(g) => {
                assert_no_objective_guard(&g.members);
                assert_no_objective_guard(&g.else_members);
            }
            MemberDecl::Sub(sub) => {
                if let Some(body) = &sub.body {
                    assert_no_objective_guard(body);
                }
            }
            MemberDecl::Port(port) => assert_no_objective_guard(&port.members),
            MemberDecl::MatchArmDeclGroup(group) => {
                for arm in &group.arms {
                    assert_no_objective_guard(std::slice::from_ref(&arm.member));
                }
            }
            _ => {}
        }
    }
}
