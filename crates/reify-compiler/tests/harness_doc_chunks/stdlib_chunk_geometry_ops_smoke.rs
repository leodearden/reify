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

/// The primitive/profile constructor chunk. Read (never written) to justify the
/// "documented elsewhere" exclusions of the registry → doc guard below. Same
/// contract as [`CHUNK_PATH`]: if the chunk moves, this const must move with it,
/// and the failure mode is a loud `expect` on the read, not a silent skip.
const GEOMETRY_CHUNK_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reify-mcp/src/tools/chunks/geometry.md"
);

/// Heading of the chunk section whose documented names are checked.
const CHUNK_SECTION: &str = "## Key Geometry Operations";

/// Read a chunk, panicking loudly (never skipping) if it has moved.
fn read_chunk(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| {
        panic!("{path} must be readable ({e}) — update the chunk path const if the chunk moved")
    })
}

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
///
/// `transform3_identity` / `affine_scale` build the rigid-transform and
/// affine-map arguments that `apply_transform` / `affine_apply` consume. They
/// are value constructors, not geometry ops, so they are not documented in the
/// operations table and have no place in the fixture's name-existence check.
/// This pair was chosen over `transform3` + `orient_axis_angle` + `vec3`
/// precisely because it adds two names to this exact-match allowlist instead of
/// four — every entry here is a name the fixture guard stops checking, so the
/// smallest sufficient extension is the right one.
const SCAFFOLD_CTORS: &[&str] = &[
    "point3",
    "plane_xy",
    "axis_z",
    "orient_identity",
    "transform3_identity",
    "affine_scale",
];

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

// ── Registry → chunk (the reverse direction) ─────────────────────────────────
//
// Every guard above answers "is what the CHUNK says real?". None answers the
// reverse: "is what the COMPILER implements written down anywhere?". That is
// the direction an op drifts through invisibly — a name is added to
// `GEOMETRY_FUNCTION_NAMES`, gets a working arm, and is simply never
// documented, so designers and the in-GUI assistant never learn it exists.
// Task 5347 closed doc → registry; the guard below closes registry → doc, so
// the next undocumented op is RED AT ITS SOURCE rather than found by hand years
// later. Like its siblings this is a NAME-EXISTENCE check against code
// registries, deliberately NOT a wording/content pin on either chunk's prose.

/// Registry entries that are primitive/profile CONSTRUCTORS, documented in
/// `chunks/geometry.md` rather than in stdlib.md's "Key Geometry Operations"
/// table.
///
/// These build the geometry that the operations table transforms, and
/// geometry.md is where a designer looks for them — so their absence from
/// stdlib.md is a placement decision, not a coverage gap.
///
/// This is a claim about where the docs live, and the guard checks it: an entry
/// listed here that is NOT actually mentioned in geometry.md is itself
/// reported, so the list cannot become a dumping ground for silencing failures.
const CONSTRUCTORS_DOCUMENTED_IN_GEOMETRY_CHUNK: &[&str] = &[
    "box",
    "cylinder",
    "sphere",
    "tube",
    "torus",
    "box_centered",
    "cylinder_centered",
    "cone",
    "wedge",
    "rectangle",
    "circle",
    "polygon",
    "ellipse",
    "rounded_box",
    "rounded_rect",
];

/// Registry entries that are implemented but documented in NO chunk at all.
///
/// Verified against every file in `crates/reify-mcp/src/tools/chunks/`: none of
/// these seven is mentioned in any of them. This is a REAL residual
/// documentation gap, carried here explicitly rather than folded into
/// [`CONSTRUCTORS_DOCUMENTED_IN_GEOMETRY_CHUNK`] — filing them as "documented
/// elsewhere" would launder a gap into a false coverage claim, which is exactly
/// the failure mode this guard exists to catch, one level down. They are out of
/// scope here (they are constructors, not operations) and are covered by a
/// separate follow-up.
///
/// This list is expected to SHRINK and must never grow. Documenting one of
/// these means deleting its entry; the guard reports any entry that has in fact
/// been documented, so a closed gap cannot linger here and mask a later
/// regression.
const CONSTRUCTORS_DOCUMENTED_NOWHERE: &[&str] = &[
    "zone_slab",
    "zone_cylinder",
    "zone_annulus",
    "zone_profile",
    "half_space",
    "nurbs_surface",
    "isosurface",
];

/// Is `name` MENTIONED in this markdown — the identifier followed by `(`?
///
/// Deliberately loose. It exists only to justify an exclusion (does the chunk
/// talk about this name at all?), never to pin a form or a wording, so it does
/// not care about the surrounding prose, the section, or the arity written.
///
/// The looseness is a substring match, so `box` is also satisfied by
/// `rounded_box(`. For the "documented elsewhere" direction that bias is
/// toward NOT failing, which is the safe direction for an exclusion
/// justification. For the known-gap direction it biases the other way — toward
/// reporting an entry as closed — which merely asks a human to look, and is
/// checked against the real chunks by
/// `the_exclusion_lists_are_accurate_over_the_real_chunks`.
fn chunk_mentions(markdown: &str, name: &str) -> bool {
    markdown.contains(&format!("{name}("))
}

/// Every `registry` name that no chunk documents.
///
/// A name counts as documented when the stdlib.md scan
/// ([`documented_geometry_op_names`], reused verbatim so this guard inherits
/// its exact semantics) finds it, or when one of the two exclusion slices
/// carries it.
///
/// Pure and fully parameterized over its inputs — rather than reading the
/// consts and the real chunks directly — so the controls below can drive it
/// with synthetic data and pin its discriminating power.
fn undocumented_geometry_ops(
    registry: &[&str],
    stdlib_md: &str,
    _geometry_md: &str,
    documented_elsewhere: &[&str],
    known_gap: &[&str],
) -> Vec<String> {
    let documented = documented_geometry_op_names(stdlib_md);

    let mut out: Vec<String> = registry
        .iter()
        .copied()
        .filter(|name| {
            !documented.iter().any(|d| d.as_str() == *name)
                && !documented_elsewhere.contains(name)
                && !known_gap.contains(name)
        })
        .map(str::to_string)
        .collect();

    out.sort();
    out.dedup();
    out
}

/// Every geometry op the compiler implements must be documented in some chunk.
///
/// The chunks are the AUTHORITATIVE language reference served verbatim to the
/// in-GUI assistant, so an implemented-but-undocumented op is invisible to
/// every designer using it.
#[test]
fn every_implemented_geometry_op_is_documented_in_a_chunk() {
    let stdlib_md = read_chunk(CHUNK_PATH);
    let geometry_md = read_chunk(GEOMETRY_CHUNK_PATH);

    // Anti-vacuity, both inputs. An empty registry would make this guard pass
    // trivially, and an emptied scan (heading renamed, rows no longer `**`/
    // backticked) would make it report all 64 names — pin both so neither
    // failure mode is mistaken for a real signal.
    assert!(
        GEOMETRY_FUNCTION_NAMES.len() >= 50,
        "anti-vacuity: GEOMETRY_FUNCTION_NAMES holds only {} name(s) — the registry this \
         guard walks has been gutted or moved, and the guard gives NO protection",
        GEOMETRY_FUNCTION_NAMES.len()
    );
    let documented = documented_geometry_op_names(&stdlib_md);
    assert!(
        documented.len() >= 20,
        "anti-vacuity: the '{CHUNK_SECTION}' scan found only {} name(s) in {CHUNK_PATH}",
        documented.len()
    );
    assert!(
        !geometry_md.trim().is_empty(),
        "anti-vacuity: {GEOMETRY_CHUNK_PATH} is empty, so the 'documented elsewhere' \
         exclusions rest on nothing"
    );

    let undocumented = undocumented_geometry_ops(
        GEOMETRY_FUNCTION_NAMES,
        &stdlib_md,
        &geometry_md,
        CONSTRUCTORS_DOCUMENTED_IN_GEOMETRY_CHUNK,
        CONSTRUCTORS_DOCUMENTED_NOWHERE,
    );

    assert!(
        undocumented.is_empty(),
        "the compiler implements geometry op(s) that NO chunk documents, so designers and \
         the in-GUI assistant cannot discover them — document each in stdlib.md's \
         '{CHUNK_SECTION}' table (and mirror it into \
         tests/fixtures/stdlib_geometry_ops_smoke.ri). Undocumented name(s): {}",
        undocumented.join(", ")
    );
}

// ── Controls for the registry → chunk guard ──────────────────────────────────
//
// Same posture as `bogus_geometry_op_names_are_reported_as_unrecognised` /
// `known_geometry_op_names_are_not_reported` do for the doc → registry guard:
// a guard is only worth its green if it has been SHOWN to both fire and stay
// silent. Extended here with two allowlist anti-rot cases, because this guard
// has something its siblings do not — two exclusion lists. An unaudited
// exclusion list is a silencer: anyone can make the guard pass by appending a
// name to it, which is precisely how this class of drift recurs.

/// Minimum shape `documented_geometry_op_names` recognises — the section
/// heading plus one `**`-prefixed backticked row. Documents `documented_op`
/// and nothing else.
const SYNTHETIC_STDLIB_MD: &str = "\
## Key Geometry Operations

**Modify:** `documented_op(solid, radius)`
";

/// Stand-in for geometry.md. Mentions `mentioned_ctor` and nothing else.
const SYNTHETIC_GEOMETRY_MD: &str = "`mentioned_ctor(x, y, z)` builds a thing.";

/// NEGATIVE CONTROL — the guard reports a genuinely undocumented registry name,
/// and stays silent for one that stdlib.md documents and one that a justified
/// exclusion covers. Proves it is neither blind nor "reports everything".
#[test]
fn undocumented_registry_names_are_reported_and_documented_ones_are_not() {
    let reported = undocumented_geometry_ops(
        &["documented_op", "mentioned_ctor", "undocumented_op"],
        SYNTHETIC_STDLIB_MD,
        SYNTHETIC_GEOMETRY_MD,
        &["mentioned_ctor"],
        &[],
    );

    assert_eq!(
        reported.len(),
        1,
        "exactly one of the three synthetic names is undocumented — \
         `documented_op` is in the stdlib.md scan and `mentioned_ctor` is covered by a \
         justified exclusion. Got: {reported:?}"
    );
    assert!(
        reported[0].contains("undocumented_op"),
        "the guard must report `undocumented_op`, which no markdown input documents and \
         no exclusion slice carries. Got: {reported:?}"
    );
}

/// ANTI-ROT — an exclusion claiming geometry.md coverage that does not exist
/// must be reported.
///
/// Without this, [`CONSTRUCTORS_DOCUMENTED_IN_GEOMETRY_CHUNK`] is an unaudited
/// dumping ground: appending a name silences the guard while documenting
/// nothing, recreating the very gap this guard exists to close.
#[test]
fn an_exclusion_claiming_coverage_that_does_not_exist_is_reported() {
    let reported = undocumented_geometry_ops(
        &["ghost_ctor"],
        SYNTHETIC_STDLIB_MD,
        SYNTHETIC_GEOMETRY_MD,
        &["ghost_ctor"],
        &[],
    );

    assert!(
        reported.iter().any(|v| v.contains("ghost_ctor")),
        "`ghost_ctor` is excluded as 'documented in geometry.md' but geometry.md does not \
         mention it — the exclusion is a false coverage claim and must be reported. \
         Got: {reported:?}"
    );
}

/// ANTI-ROT — a known-gap entry that has since been documented must be
/// reported, so the list is forced to SHRINK as the gap closes.
///
/// A stale entry in [`CONSTRUCTORS_DOCUMENTED_NOWHERE`] would permanently
/// exempt a name from the guard, so a later regression (the row being deleted
/// again) would go unnoticed.
#[test]
fn a_known_gap_entry_that_has_since_been_documented_is_reported() {
    let via_stdlib = undocumented_geometry_ops(
        &["documented_op"],
        SYNTHETIC_STDLIB_MD,
        SYNTHETIC_GEOMETRY_MD,
        &[],
        &["documented_op"],
    );
    assert!(
        via_stdlib.iter().any(|v| v.contains("documented_op")),
        "`documented_op` is carried as a known gap but stdlib.md now documents it — the \
         stale entry must be reported so it gets deleted. Got: {via_stdlib:?}"
    );

    let via_geometry = undocumented_geometry_ops(
        &["mentioned_ctor"],
        SYNTHETIC_STDLIB_MD,
        SYNTHETIC_GEOMETRY_MD,
        &[],
        &["mentioned_ctor"],
    );
    assert!(
        via_geometry.iter().any(|v| v.contains("mentioned_ctor")),
        "`mentioned_ctor` is carried as a known gap but geometry.md now mentions it — the \
         stale entry must be reported so it gets deleted. Got: {via_geometry:?}"
    );
}

/// Both exclusion lists must be accurate over the REAL chunks.
///
/// This is the same property the two controls above pin synthetically, asserted
/// against the files that actually ship — so the lists cannot drift out of
/// truth even if the helper is later refactored.
#[test]
fn the_exclusion_lists_are_accurate_over_the_real_chunks() {
    let stdlib_md = read_chunk(CHUNK_PATH);
    let geometry_md = read_chunk(GEOMETRY_CHUNK_PATH);

    let unbacked: Vec<&str> = CONSTRUCTORS_DOCUMENTED_IN_GEOMETRY_CHUNK
        .iter()
        .copied()
        .filter(|name| !chunk_mentions(&geometry_md, name))
        .collect();
    assert!(
        unbacked.is_empty(),
        "CONSTRUCTORS_DOCUMENTED_IN_GEOMETRY_CHUNK excludes name(s) from the registry → doc \
         guard on the grounds that {GEOMETRY_CHUNK_PATH} documents them, but it does not \
         mention them at all — either document them there or drop the exclusion: {}",
        unbacked.join(", ")
    );

    let closed: Vec<&str> = CONSTRUCTORS_DOCUMENTED_NOWHERE
        .iter()
        .copied()
        .filter(|name| chunk_mentions(&stdlib_md, name) || chunk_mentions(&geometry_md, name))
        .collect();
    assert!(
        closed.is_empty(),
        "CONSTRUCTORS_DOCUMENTED_NOWHERE carries name(s) that ARE now documented — the list \
         must shrink as the gap closes, so delete these entries and let the guard cover \
         them normally: {}",
        closed.join(", ")
    );
}
