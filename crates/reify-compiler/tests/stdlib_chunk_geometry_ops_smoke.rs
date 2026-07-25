//! Compile-smoke + name-existence guards for the "Key Geometry Operations"
//! (+ "Curves") table served by the `reify_language_reference` MCP tool
//! (topic="stdlib") — the chunk `crates/reify-mcp/src/tools/chunks/stdlib.md`.
//!
//! The chunk is the AUTHORITATIVE language reference shown verbatim to the
//! in-GUI assistant, so a documented signature that does not match the
//! compiler's real geometry-op arms silently misleads designers (task 5347,
//! same stale-reference class as task 5203).
//!
//! # What is actually enforced
//!
//! The fixture `tests/fixtures/stdlib_geometry_ops_smoke.ri` is an EXECUTABLE
//! TRANSCRIPTION of that chunk's Key Geometry Operations + Curves sections,
//! concretized over real primitives/profiles. Three complementary properties
//! are asserted, and NO ONE OF THEM ALONE IS SUFFICIENT:
//!
//! 1. **Arity.** The fixture compiles with ZERO Error-severity diagnostics —
//!    i.e. every documented form is accepted AT THE ARITY WRITTEN, *for the
//!    names the compiler resolves*. See "What is NOT established" below: this
//!    is an arity property, not a type-check.
//! 2. **Name existence, fixture → compiler.** Every call name in the fixture is
//!    a real entry in the compiler's own registries (`GEOMETRY_FUNCTION_NAMES` /
//!    `GEOMETRY_TOPOLOGY_SELECTOR_NAMES`). This is NOT implied by (1): an
//!    unknown call name in a `structure def` body is silently ignored by the
//!    compiler — NO diagnostic at ANY severity — so only names that already
//!    have a geometry-op arm are ever arity-checked, and a
//!    documented-but-nonexistent op (the phantom `offset_surface` task 5347
//!    removed by hand) passes (1) untouched.
//!    `bogus_geometry_op_names_are_reported_as_unrecognised` is the negative
//!    control that pins this guard's discriminating power, so the hole cannot
//!    silently reopen.
//! 3. **Name existence, chunk → compiler → fixture.** The names the CHUNK
//!    documents are read back out of `stdlib.md` itself and checked against the
//!    same two registries, and against the fixture's call names. That closes
//!    the direction (2) cannot see: a phantom op added to the table but never
//!    mirrored into the fixture is caught AT ITS SOURCE. This is a
//!    name-existence check against code registries, deliberately NOT a
//!    wording/content pin on the chunk's prose (house rule: no doc-content
//!    meta-tests).
//!
//! # What is NOT established
//!
//! - **Argument dimension/type.** Only arity is enforced for these ops.
//!   Mutation-tested: `extrude(prof, 5)`, `rotate(solid, 0,0,1, 90mm)` and
//!   `linear_pattern(solid, 1mm,0mm,0mm, 3mm, 5)` all leave this suite GREEN.
//!   `builtin_signatures.rs`'s checkable-arg-slot table covers topology
//!   selectors and `generate` only — no Key Geometry Operation has a checked
//!   slot. The chunk's "positions/lengths take a `Length`; angles take an
//!   `Angle`; direction/axis/normal components and counts are plain numbers"
//!   line is therefore a convention this fixture FOLLOWS but nothing enforces.
//! - **Argument order / semantics.** A permutation of two same-dimension
//!   arguments satisfies all three properties above.
//! - **Doc → fixture coverage at FORM granularity.** Property (3) pairs the two
//!   files by NAME. Whether each documented *overload* (arity/value-form) has a
//!   compiling instance in the fixture is not checked — adding a second arity
//!   for an already-documented name without mirroring it fails no test. Keeping
//!   the forms in step is a review-time responsibility.
//!
//! The parse→compile→filter-`Severity::Error` sequence mirrors
//! `examples_smoke.rs::smoke_one`; the fixture-path-const + named-test shape
//! mirrors `reify-eval/tests/topology_selector_smoke_tests.rs`; the
//! cross-crate-source read plus anti-vacuity self-check mirrors
//! `reify-eval/tests/ambient_default_material_integration_gate.rs`.

use reify_ast::{Declaration, Expr, ExprKind, MemberDecl, ParsedModule, StringPart};
use reify_compiler::{
    GEOMETRY_FUNCTION_NAMES, GEOMETRY_TOPOLOGY_SELECTOR_NAMES, compile_with_stdlib,
    parse_with_stdlib,
};
use reify_core::{ModulePath, Severity};

const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/stdlib_geometry_ops_smoke.ri"
);

/// The chunk this fixture transcribes. Read (never written) to check documented
/// names against the compiler's registries. If the chunk moves, this const must
/// move with it — the failure mode is a loud `expect` on the read, not a silent
/// skip.
const CHUNK_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reify-mcp/src/tools/chunks/stdlib.md"
);

/// Heading of the chunk section whose documented names are checked.
const CHUNK_SECTION: &str = "## Key Geometry Operations";

fn read_fixture() -> String {
    std::fs::read_to_string(FIXTURE_PATH)
        .expect("tests/fixtures/stdlib_geometry_ops_smoke.ri should exist")
}

fn parse_or_panic(source: &str, label: &str) -> ParsedModule {
    // Prelude-aware parsing (matches the `compile_with_stdlib` companion). A
    // parse error is a fixture/snippet bug, not the property under test, so
    // surface it distinctly.
    let parsed = parse_with_stdlib(source, ModulePath::single("stdlib_geometry_ops_smoke"));
    assert!(
        parsed.errors.is_empty(),
        "{label} must parse cleanly, got parse errors:\n{}",
        parsed
            .errors
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join("\n")
    );
    parsed
}

/// Every geometry-op / curve-constructor form documented in stdlib.md's
/// "Key Geometry Operations" (+ "Curves") table must compile with no
/// Error-severity diagnostics.
#[test]
fn stdlib_chunk_geometry_ops_compile_with_stdlib_no_errors() {
    let source = read_fixture();
    let parsed = parse_or_panic(&source, "fixture");

    // Compile phase — filter to Error severity only (warnings are allowed).
    let compiled = compile_with_stdlib(&parsed);
    let errors: Vec<String> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect();

    assert!(
        errors.is_empty(),
        "stdlib.md's documented geometry-op forms must all be accepted at the \
         arity written, with zero Error-severity diagnostics (fixture: \
         stdlib_geometry_ops_smoke.ri); got {} error(s):\n{}",
        errors.len(),
        errors.join("\n")
    );
}

// ── Name-existence guards ────────────────────────────────────────────────────
//
// The compile smoke above is only PART of the contract. The compiler silently
// ignores an unknown call name in a `structure def` body — it emits NO
// diagnostic at ANY severity — so only names that already have a geometry-op
// arm ever get arity-checked. That means a documented-but-nonexistent op (the
// phantom `offset_surface` this task removed by hand) sails through the
// zero-Error assertion untouched. The tests below close that hole by checking
// call names — extracted from the fixture's parsed AST, and from the chunk
// itself — against the compiler's OWN name registries.

/// Argument-scaffolding constructors the fixture uses to BUILD inputs for the
/// documented ops. Exact-match and deliberately minimal: these are datum/value
/// constructors, NOT entries in stdlib.md's "Key Geometry Operations"/"Curves"
/// table, and none of them is reachable from an integration test through an
/// exported name slice (the `point3`/`plane_xy`/`axis_z`/`orient_identity`
/// datum constructors are resolved by an arg-aware resolver with no exported
/// slice at all). Widening more `pub use` exports purely to satisfy a test
/// would enlarge reify-compiler's public API for names that are not under
/// review here. Because the allowlist is exact-match, a typo'd scaffold call
/// (`point33`) is still reported.
const SCAFFOLD_CTORS: &[&str] = &["point3", "plane_xy", "axis_z", "orient_identity"];

/// Is `name` a call the compiler recognises for the purposes of this guard?
///
/// `GEOMETRY_FUNCTION_NAMES` / `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` ARE the
/// compiler's recognition semantics: `is_geometry_function` /
/// `is_geometry_topology_selector` are defined as bare `.contains(&name)` over
/// exactly these slices, so checking the slices reproduces them with no
/// production-code change.
fn is_recognised_geometry_call(name: &str) -> bool {
    GEOMETRY_FUNCTION_NAMES.contains(&name) || GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(&name)
}

/// Push the callee name of every `FunctionCall` in `expr`'s subtree onto `out`.
///
/// The match is intentionally exhaustive with **no `_` wildcard**, so adding an
/// `ExprKind` variant breaks this file at compile time rather than silently
/// dropping a whole class of call site from the guard (same posture as
/// `find_node` in `tests/harness_langcore/type_error_propagation_tests.rs`).
/// Walking the parsed AST — rather than lexing the source — means no comment or
/// string-literal blind spots and no keyword/heuristic allowlists.
///
/// Non-`FunctionCall` callee names (a trait method, an ad-hoc port selector)
/// are deliberately NOT collected: they are dispatched through a different
/// resolver and are not geometry ops.
fn collect_call_names(expr: &Expr, out: &mut Vec<String>) {
    match &expr.kind {
        // Leaves — no subexpressions, no callee name.
        ExprKind::NumberLiteral { .. }
        | ExprKind::QuantityLiteral { .. }
        | ExprKind::StringLiteral(_)
        | ExprKind::BoolLiteral(_)
        | ExprKind::Ident(_)
        | ExprKind::EnumAccess { .. }
        | ExprKind::Undef => {}

        // The variant under test.
        ExprKind::FunctionCall { name, args, .. } => {
            out.push(name.clone());
            for arg in args {
                collect_call_names(arg, out);
            }
        }

        // Compound variants — recurse into every child subexpression.
        ExprKind::BinOp { left, right, .. } => {
            collect_call_names(left, out);
            collect_call_names(right, out);
        }
        ExprKind::UnOp { operand, .. } => collect_call_names(operand, out),
        ExprKind::MemberAccess { object, .. } => collect_call_names(object, out),
        ExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_call_names(condition, out);
            collect_call_names(then_branch, out);
            collect_call_names(else_branch, out);
        }
        ExprKind::ListLiteral(items) | ExprKind::SetLiteral(items) => {
            for item in items {
                collect_call_names(item, out);
            }
        }
        ExprKind::MapLiteral(entries) => {
            for (key, value) in entries {
                collect_call_names(key, out);
                collect_call_names(value, out);
            }
        }
        ExprKind::IndexAccess { object, index } => {
            collect_call_names(object, out);
            collect_call_names(index, out);
        }
        ExprKind::Match { discriminant, arms } => {
            collect_call_names(discriminant, out);
            for arm in arms {
                collect_call_names(&arm.body, out);
            }
        }
        ExprKind::Auto { params, .. } => {
            for (_, value) in params {
                collect_call_names(value, out);
            }
        }
        ExprKind::Lambda { body, .. } => collect_call_names(body, out),
        ExprKind::Quantifier {
            collection,
            predicate,
            ..
        } => {
            collect_call_names(collection, out);
            collect_call_names(predicate, out);
        }
        ExprKind::AdHocSelector { base, args, .. } => {
            collect_call_names(base, out);
            for arg in args {
                collect_call_names(arg, out);
            }
        }
        ExprKind::QualifiedAccess { qualifier, .. } => collect_call_names(qualifier, out),
        ExprKind::InstanceQualifiedAccess { object, qualified } => {
            collect_call_names(object, out);
            collect_call_names(qualified, out);
        }
        ExprKind::Range { lower, upper, .. } => {
            if let Some(lower) = lower {
                collect_call_names(lower, out);
            }
            if let Some(upper) = upper {
                collect_call_names(upper, out);
            }
        }
        ExprKind::TraitMethodCall { object, args, .. } => {
            collect_call_names(object, out);
            for arg in args {
                collect_call_names(arg, out);
            }
        }
        ExprKind::TraitStaticCall { args, .. } => {
            for arg in args {
                collect_call_names(arg, out);
            }
        }
        ExprKind::VariantConstruct { fields, .. } => {
            for (_, value) in fields {
                collect_call_names(value, out);
            }
        }
        ExprKind::InterpolatedString(parts) => {
            for part in parts {
                match part {
                    StringPart::Literal(_) => {}
                    StringPart::Hole(inner) => collect_call_names(inner, out),
                }
            }
        }
    }
}

/// Every call name in `source`, deduped and sorted for deterministic output.
///
/// `source` must be `structure def`s whose members are all `let` bindings —
/// the shape of the fixture and of the inline snippets below. Anything else
/// PANICS rather than being skipped, so growing the fixture a new declaration
/// or member kind is a loud "extend the walker", never a silent coverage hole.
fn geometry_call_names(source: &str, label: &str) -> Vec<String> {
    let parsed = parse_or_panic(source, label);

    let mut names = Vec::new();
    for decl in &parsed.declarations {
        let Declaration::Structure(structure) = decl else {
            panic!(
                "{label}: the name-existence guard only walks `structure def` declarations, \
                 but this source has another declaration kind — extend `geometry_call_names` \
                 rather than leaving those call sites unchecked"
            );
        };
        for member in &structure.members {
            let MemberDecl::Let(binding) = member else {
                panic!(
                    "{label}: the name-existence guard only walks `let` members of `{}`, \
                     but it has another member kind — extend `geometry_call_names` rather \
                     than leaving those call sites unchecked",
                    structure.name
                );
            };
            collect_call_names(&binding.value, &mut names);
        }
    }

    names.sort();
    names.dedup();
    names
}

/// Every call name in `source` that the compiler does not recognise as a
/// geometry op, topology selector, or known input-scaffold constructor.
fn unrecognised_geometry_call_names(source: &str, label: &str) -> Vec<String> {
    geometry_call_names(source, label)
        .into_iter()
        .filter(|name| {
            !is_recognised_geometry_call(name) && !SCAFFOLD_CTORS.contains(&name.as_str())
        })
        .collect()
}

/// NEGATIVE CONTROL — the reviewer's exact repro, inline (no fixture edit).
///
/// `scal`/`thikken` are typo'd renames of real ops and `offset_surface` is the
/// phantom signature this task deleted from stdlib.md BY HAND. None of the
/// three produces a diagnostic at any severity, so
/// `stdlib_chunk_geometry_ops_compile_with_stdlib_no_errors` cannot see them.
/// This test pins the guard's discriminating power so that hole cannot silently
/// reopen.
#[test]
fn bogus_geometry_op_names_are_reported_as_unrecognised() {
    let source = r#"
structure def BogusOps {
    let solid   = box(10mm, 10mm, 10mm)
    let surface = rectangle(10mm, 10mm)
    let sc = scal(solid, 2.0)
    let th = thikken(solid, 1mm)
    let of = offset_surface(surface, 1mm)
}
"#;

    assert_eq!(
        unrecognised_geometry_call_names(source, "bogus-ops snippet"),
        vec![
            "offset_surface".to_string(),
            "scal".to_string(),
            "thikken".to_string(),
        ],
        "the guard must report exactly the three names that have no arm in the \
         compiler's geometry-op / topology-selector registries"
    );
}

/// The control's control — with the CORRECT names the guard must report
/// nothing, so it is not merely "reports everything".
#[test]
fn known_geometry_op_names_are_not_reported() {
    let source = r#"
structure def KnownOps {
    let solid   = box(10mm, 10mm, 10mm)
    let surface = rectangle(10mm, 10mm)
    let sc = scale(solid, 2.0)
    let th = thicken(solid, 1mm)
    let of = offset_solid(solid, 1mm)
}
"#;

    assert_eq!(
        unrecognised_geometry_call_names(source, "known-ops snippet"),
        Vec::<String>::new(),
        "every name in this snippet has a real compiler arm, so the guard must \
         report none of them"
    );
}

/// POSITIVE EXISTENCE, over the real fixture: every geometry-op / curve
/// constructor form transcribed from stdlib.md names a function the compiler
/// actually has.
///
/// The names are EXTRACTED FROM the fixture rather than hardcoded, so this
/// auto-tracks fixture edits: renaming a call to something with no compiler arm
/// goes RED even though the compile smoke stays green.
#[test]
fn fixture_geometry_call_names_all_exist_in_the_compiler() {
    let source = read_fixture();
    let unrecognised = unrecognised_geometry_call_names(&source, "fixture");

    assert!(
        unrecognised.is_empty(),
        "stdlib.md documents a geometry op the compiler does not have — the \
         chunk crates/reify-mcp/src/tools/chunks/stdlib.md must be corrected \
         (an unknown call name in a `structure def` body compiles silently, so \
         the zero-Error compile smoke cannot catch this). Unrecognised name(s): {}",
        unrecognised.join(", ")
    );
}

// ── Chunk → compiler / chunk → fixture ───────────────────────────────────────

/// The function names documented in the chunk's [`CHUNK_SECTION`] section.
///
/// Scan shape (deliberately narrow, so this stays a NAME check and never
/// becomes a wording pin): inside that section — from its heading to the next
/// `## ` heading — every line that starts with `**` is a bolded signature row;
/// within such a row only the backtick-delimited spans are inspected, and from
/// each span the identifier immediately preceding a `(` is taken. Spans without
/// a `(` (`List<Geometry>`, `Length`) contribute nothing, as does any prose or
/// HTML comment outside a `**`-prefixed line.
///
/// Deduped and sorted. Callers must anti-vacuity-check the result: a heading
/// rename or a row that stops using `**`/backticks would otherwise silently
/// empty the scan.
fn documented_geometry_op_names(markdown: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_section = false;

    for line in markdown.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            in_section = heading.trim() == CHUNK_SECTION.trim_start_matches("## ");
            continue;
        }
        if !in_section || !line.starts_with("**") {
            continue;
        }

        // Odd-indexed pieces of a backtick split are the spans *inside* the
        // backticks (`a `x` b `y` c` → ["a ", "x", " b ", "y", " c"]).
        for span in line.split('`').skip(1).step_by(2) {
            let Some(open) = span.find('(') else {
                continue;
            };
            let name = span[..open]
                .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .next()
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            names.push(name.to_string());
        }
    }

    names.sort();
    names.dedup();
    names
}

/// The chunk itself must not document an op the compiler does not have.
///
/// This is the direction the fixture-side guard cannot see: the phantom
/// `offset_surface` was only caught by hand-reading the table. Checking the
/// documented names against the compiler's registries catches the next one AT
/// ITS SOURCE, before it is ever mirrored into the fixture.
#[test]
fn documented_geometry_op_names_all_exist_in_the_compiler() {
    let markdown = std::fs::read_to_string(CHUNK_PATH).unwrap_or_else(|e| {
        panic!("{CHUNK_PATH} must be readable ({e}) — update CHUNK_PATH if the chunk moved")
    });
    let documented = documented_geometry_op_names(&markdown);

    // Anti-vacuity: a heading rename or a reformatted table would empty the
    // scan and make every assertion below pass trivially. The section carries
    // ~29 distinct names today; the sentinels prove the scan reaches the
    // Sweep / Pattern / Curves rows, not just the first one.
    assert!(
        documented.len() >= 20,
        "the '{CHUNK_SECTION}' scan found only {} name(s) in {CHUNK_PATH} — the scan is \
         vacuous (heading renamed, or the signature rows no longer start with `**` and \
         use backticks) and gives NO protection",
        documented.len()
    );
    for sentinel in ["extrude", "circular_pattern", "nurbs"] {
        assert!(
            documented.iter().any(|n| n == sentinel),
            "anti-vacuity: `{sentinel}` is documented in '{CHUNK_SECTION}' but the scan did \
             not find it — the scan is not reaching every signature row"
        );
    }

    let phantom: Vec<&String> = documented
        .iter()
        .filter(|name| !is_recognised_geometry_call(name))
        .collect();
    assert!(
        phantom.is_empty(),
        "{CHUNK_PATH} documents geometry op(s) with no arm in the compiler's \
         GEOMETRY_FUNCTION_NAMES / GEOMETRY_TOPOLOGY_SELECTOR_NAMES registries — the chunk \
         is served verbatim to the in-GUI assistant, so this misleads designers. \
         Phantom name(s): {}",
        phantom
            .iter()
            .map(|n| n.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
}

/// Every documented name must be exercised by the fixture.
///
/// This is the doc → fixture direction at NAME granularity: a row added to the
/// chunk and never mirrored into the fixture is RED. (Form granularity — a
/// second overload of an already-documented name — remains a review-time
/// responsibility; see the module doc.)
#[test]
fn every_documented_geometry_op_name_is_exercised_by_the_fixture() {
    let markdown = std::fs::read_to_string(CHUNK_PATH).unwrap_or_else(|e| {
        panic!("{CHUNK_PATH} must be readable ({e}) — update CHUNK_PATH if the chunk moved")
    });
    let documented = documented_geometry_op_names(&markdown);
    assert!(
        documented.len() >= 20,
        "anti-vacuity: the '{CHUNK_SECTION}' scan found only {} name(s) in {CHUNK_PATH}",
        documented.len()
    );

    let exercised = geometry_call_names(&read_fixture(), "fixture");
    let missing: Vec<&String> = documented
        .iter()
        .filter(|name| !exercised.contains(name))
        .collect();

    assert!(
        missing.is_empty(),
        "stdlib.md documents geometry op(s) with no compiling instance in \
         tests/fixtures/stdlib_geometry_ops_smoke.ri, so nothing checks that the documented \
         form is accepted at the arity written — add a call for each. Unmirrored name(s): {}",
        missing
            .iter()
            .map(|n| n.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
}
