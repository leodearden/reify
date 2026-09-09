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
//! concretized over real primitives/profiles. Four complementary properties
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
//!    removed by hand — since IMPLEMENTED for real by task #4192, so it is no
//!    longer a phantom) passes (1) untouched.
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
//! 4. **Doc → fixture pairing at FORM granularity.** Every documented
//!    (name, arity) form — with `…` read as variadic — has a fixture call at
//!    that arity, so a documented signature the compiler rejects is RED
//!    either here (form unmirrored) or in the compile smoke (mirrored and
//!    rejected); task #5583.
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
//!   arguments satisfies all four properties above.
//!
//! The parse→compile→filter-`Severity::Error` sequence mirrors
//! `examples_smoke.rs::smoke_one`; the fixture-path-const + named-test shape
//! mirrors `reify-eval/tests/harness_topology_selector/topology_selector_smoke_tests.rs`; the
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

/// The whole chunk corpus served by `reify_language_reference`. The known-gap
/// exclusion below claims a name is documented in NO chunk, so that claim has to
/// be checked against every chunk, not just the two named above.
const CHUNKS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../reify-mcp/src/tools/chunks");

/// Heading of the chunk section whose documented names are checked.
const CHUNK_SECTION: &str = "## Key Geometry Operations";

/// Read a chunk, panicking loudly (never skipping) if it has moved.
fn read_chunk(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| {
        panic!("{path} must be readable ({e}) — update the chunk path const if the chunk moved")
    })
}

/// Every `*.md` under [`CHUNKS_DIR`], concatenated in a stable (sorted) order.
///
/// Same loud-panic-never-skip posture as [`read_chunk`]: an unreadable directory
/// or entry aborts rather than silently shrinking the corpus, because a shrunken
/// corpus would make the "documented in NO chunk" claim pass vacuously.
fn read_all_chunks() -> String {
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(CHUNKS_DIR)
        .unwrap_or_else(|e| {
            panic!("{CHUNKS_DIR} must be readable ({e}) — update CHUNKS_DIR if the chunks moved")
        })
        .map(|entry| {
            entry
                .unwrap_or_else(|e| panic!("reading an entry of {CHUNKS_DIR} failed ({e})"))
                .path()
        })
        .filter(|p| p.extension().is_some_and(|ext| ext == "md"))
        .collect();
    paths.sort();

    // Anti-vacuity: the corpus is 17 chunks today. An empty or near-empty read
    // would silently turn the known-gap audit into a no-op.
    assert!(
        paths.len() >= 10,
        "anti-vacuity: {CHUNKS_DIR} yielded only {} markdown chunk(s) — the corpus the \
         known-gap audit reads has moved or been gutted, and the audit gives NO protection",
        paths.len()
    );

    paths
        .iter()
        .map(|p| read_chunk(&p.to_string_lossy()))
        .collect::<Vec<_>>()
        .join("\n")
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
// phantom `offset_surface` this task removed by hand — task #4192 has since
// implemented it for real) sails through the
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

/// Push `(callee name, arg count)` for every `FunctionCall` in `expr`'s
/// subtree onto `out`.
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
fn collect_call_forms(expr: &Expr, out: &mut Vec<(String, usize)>) {
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
            out.push((name.clone(), args.len()));
            for arg in args {
                collect_call_forms(arg, out);
            }
        }

        // Compound variants — recurse into every child subexpression.
        ExprKind::BinOp { left, right, .. } => {
            collect_call_forms(left, out);
            collect_call_forms(right, out);
        }
        ExprKind::UnOp { operand, .. } => collect_call_forms(operand, out),
        ExprKind::MemberAccess { object, .. } => collect_call_forms(object, out),
        ExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_call_forms(condition, out);
            collect_call_forms(then_branch, out);
            collect_call_forms(else_branch, out);
        }
        ExprKind::ListLiteral(items) | ExprKind::SetLiteral(items) => {
            for item in items {
                collect_call_forms(item, out);
            }
        }
        ExprKind::MapLiteral(entries) => {
            for (key, value) in entries {
                collect_call_forms(key, out);
                collect_call_forms(value, out);
            }
        }
        ExprKind::IndexAccess { object, index } => {
            collect_call_forms(object, out);
            collect_call_forms(index, out);
        }
        ExprKind::Match { discriminant, arms } => {
            collect_call_forms(discriminant, out);
            for arm in arms {
                collect_call_forms(&arm.body, out);
            }
        }
        ExprKind::Auto { params, .. } => {
            for (_, value) in params {
                collect_call_forms(value, out);
            }
        }
        ExprKind::Lambda { body, .. } => collect_call_forms(body, out),
        ExprKind::Quantifier {
            collection,
            predicate,
            ..
        } => {
            collect_call_forms(collection, out);
            collect_call_forms(predicate, out);
        }
        ExprKind::AdHocSelector { base, args, .. } => {
            collect_call_forms(base, out);
            for arg in args {
                collect_call_forms(arg, out);
            }
        }
        ExprKind::QualifiedAccess { qualifier, .. } => collect_call_forms(qualifier, out),
        ExprKind::InstanceQualifiedAccess { object, qualified } => {
            collect_call_forms(object, out);
            collect_call_forms(qualified, out);
        }
        ExprKind::Range { lower, upper, .. } => {
            if let Some(lower) = lower {
                collect_call_forms(lower, out);
            }
            if let Some(upper) = upper {
                collect_call_forms(upper, out);
            }
        }
        ExprKind::TraitMethodCall { object, args, .. } => {
            collect_call_forms(object, out);
            for arg in args {
                collect_call_forms(arg, out);
            }
        }
        ExprKind::TraitStaticCall { args, .. } => {
            for arg in args {
                collect_call_forms(arg, out);
            }
        }
        ExprKind::VariantConstruct { fields, .. } => {
            for (_, value) in fields {
                collect_call_forms(value, out);
            }
        }
        ExprKind::InterpolatedString(parts) => {
            for part in parts {
                match part {
                    StringPart::Literal(_) => {}
                    StringPart::Hole(inner) => collect_call_forms(inner, out),
                }
            }
        }
    }
}

/// Every `(call name, arg count)` form in `source`, deduped and sorted for
/// deterministic output.
///
/// `source` must be `structure def`s whose members are all `let` bindings —
/// the shape of the fixture and of the inline snippets above. Anything else
/// PANICS rather than being skipped, so growing the fixture a new declaration
/// or member kind is a loud "extend the walker", never a silent coverage hole.
fn geometry_call_forms(source: &str, label: &str) -> Vec<(String, usize)> {
    let parsed = parse_or_panic(source, label);

    let mut forms = Vec::new();
    for decl in &parsed.declarations {
        let Declaration::Structure(structure) = decl else {
            panic!(
                "{label}: the name-existence guard only walks `structure def` declarations, \
                 but this source has another declaration kind — extend `geometry_call_forms` \
                 rather than leaving those call sites unchecked"
            );
        };
        for member in &structure.members {
            let MemberDecl::Let(binding) = member else {
                panic!(
                    "{label}: the name-existence guard only walks `let` members of `{}`, \
                     but it has another member kind — extend `geometry_call_forms` rather \
                     than leaving those call sites unchecked",
                    structure.name
                );
            };
            collect_call_forms(&binding.value, &mut forms);
        }
    }

    forms.sort();
    forms.dedup();
    forms
}

/// Every call name in `source`, projected from [`geometry_call_forms`] so
/// exactly one AST walker exists (overloads of the same name collapse to one
/// entry here — see `geometry_call_forms` for the arity-preserving form).
fn geometry_call_names(source: &str, label: &str) -> Vec<String> {
    let mut names: Vec<String> = geometry_call_forms(source, label)
        .into_iter()
        .map(|(name, _count)| name)
        .collect();
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
/// All three names are typo'd renames of real ops. None produces a diagnostic
/// at any severity, so `stdlib_chunk_geometry_ops_compile_with_stdlib_no_errors`
/// cannot see them. This test pins the guard's discriminating power so that
/// hole cannot silently reopen.
///
/// The third slot originally held `offset_surface` itself — the phantom
/// signature task 5347 deleted from stdlib.md by hand, and the reviewer's
/// actual repro. Task #4192 (PRD `geometry-modify-sweep-completion.md` task θ)
/// IMPLEMENTED `offset_surface`, registering it in `GEOMETRY_FUNCTION_NAMES`,
/// so the name is now recognised and can no longer serve as a control. It is
/// replaced by a typo of itself, which keeps the slot in the same class as
/// `scal`/`thikken` and cannot be invalidated by a future implementation task.
/// The guard's mechanism is name-class-blind — it reports any call name with
/// no arm in the registries — so the substitution costs no coverage.
#[test]
fn bogus_geometry_op_names_are_reported_as_unrecognised() {
    let source = r#"
structure def BogusOps {
    let solid   = box(10mm, 10mm, 10mm)
    let surface = rectangle(10mm, 10mm)
    let sc = scal(solid, 2.0)
    let th = thikken(solid, 1mm)
    let of = offset_surfase(surface, 1mm)
}
"#;

    assert_eq!(
        unrecognised_geometry_call_names(source, "bogus-ops snippet"),
        vec![
            "offset_surfase".to_string(),
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

/// A documented form's declared argument count: either an exact arity, or a
/// variadic form carrying the given MINIMUM arity.
///
/// An argument equal to `…` (U+2026) or ENDING IN `…` (e.g. `weights…`) marks
/// the form variadic and contributes 0 to the minimum — see
/// `documented_geometry_op_forms`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Arity {
    Exact(usize),
    AtLeast(usize),
}

/// One documented (name, arity) overload. A single row commonly documents
/// several of these for the same name (e.g. `mirror(geo, plane)` and
/// `mirror(geo, ox, oy, oz, nx, ny, nz)`) — they are deliberately NOT
/// collapsed, which is the whole point of the FORM-granularity upgrade over
/// `documented_geometry_op_names`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DocForm {
    name: String,
    arity: Arity,
}

/// Every geometry-op / curve-constructor (name, arity) FORM documented in the
/// chunk's [`CHUNK_SECTION`] section.
///
/// Scan shape (deliberately narrow, so this stays a NAME+ARITY check and
/// never becomes a wording pin) — identical section/line/span selection to
/// the name-only scan this supersedes: inside that section — from its
/// heading to the next `## ` heading — every line that starts with `**` is a
/// bolded signature row; within such a row only the backtick-delimited spans
/// are inspected. From each span:
///   - no `(`, or no `)`, or `)` before `(` → the span contributes no form
///     (spans like `List<Geometry>`, `Length` have no `(` at all; a broken
///     span with an unbalanced `(` is skipped rather than panicking);
///   - the identifier immediately preceding the `(` is the name (as before);
///   - the text between the first `(` and the last `)`, split on `,` and
///     trimmed per piece, is the argument list: empty → `Exact(0)`; otherwise
///     an argument equal to `…` or ENDING IN `…` contributes 0 to the count
///     and marks the form variadic → `AtLeast(remaining count)`; with no such
///     argument → `Exact(args.len())`.
///
/// Deduped and sorted (by name, then arity), so a caller's `assert_eq!` names
/// the exact form. Callers must anti-vacuity-check the result: a heading
/// rename or a row that stops using `**`/backticks would otherwise silently
/// empty the scan.
fn documented_geometry_op_forms(markdown: &str) -> Vec<DocForm> {
    let mut forms = Vec::new();
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
            let Some(close) = span.rfind(')') else {
                continue;
            };
            if close < open {
                continue;
            }
            let name = span[..open]
                .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .next()
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }

            let inner = span[open + 1..close].trim();
            let arity = if inner.is_empty() {
                Arity::Exact(0)
            } else {
                let mut variadic = false;
                let mut count = 0usize;
                for arg in inner.split(',') {
                    let arg = arg.trim();
                    if arg == "…" || arg.ends_with('…') {
                        variadic = true;
                    } else {
                        count += 1;
                    }
                }
                if variadic {
                    Arity::AtLeast(count)
                } else {
                    Arity::Exact(count)
                }
            };

            forms.push(DocForm {
                name: name.to_string(),
                arity,
            });
        }
    }

    forms.sort();
    forms.dedup();
    forms
}

/// The function names documented in the chunk's [`CHUNK_SECTION`] section, a
/// projection over [`documented_geometry_op_forms`] so exactly one markdown
/// scan exists (overloads of the same name collapse to one entry here — see
/// `documented_geometry_op_forms` for the arity-preserving form).
///
/// Deduped and sorted. Callers must anti-vacuity-check the result: a heading
/// rename or a row that stops using `**`/backticks would otherwise silently
/// empty the scan.
fn documented_geometry_op_names(markdown: &str) -> Vec<String> {
    let mut names: Vec<String> = documented_geometry_op_forms(markdown)
        .into_iter()
        .map(|form| form.name)
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Anti-vacuity guard every [`documented_geometry_op_names`] caller owes its
/// assertions: a heading rename, or rows that stop using `**`/backticks, would
/// empty the scan and make the checks built on it pass (or fire) for reasons
/// that have nothing to do with the property under test. The section carries
/// ~43 distinct names today.
fn assert_scan_not_vacuous(documented: &[String]) {
    assert!(
        documented.len() >= 20,
        "the '{CHUNK_SECTION}' scan found only {} name(s) in {CHUNK_PATH} — the scan is \
         vacuous (heading renamed, or the signature rows no longer start with `**` and \
         use backticks) and gives NO protection",
        documented.len()
    );
}

/// The chunk itself must not document an op the compiler does not have.
///
/// This is the direction the fixture-side guard cannot see: the phantom
/// `offset_surface` was only caught by hand-reading the table. Checking the
/// documented names against the compiler's registries catches the next one AT
/// ITS SOURCE, before it is ever mirrored into the fixture.
#[test]
fn documented_geometry_op_names_all_exist_in_the_compiler() {
    let markdown = read_chunk(CHUNK_PATH);
    let documented = documented_geometry_op_names(&markdown);

    // Anti-vacuity: a heading rename or a reformatted table would empty the
    // scan and make every assertion below pass trivially. The sentinels
    // additionally prove the scan reaches the Sweep / Pattern / Curves rows,
    // not just the first one.
    assert_scan_not_vacuous(&documented);
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
/// second overload of an already-documented name — is covered by its sibling
/// `every_documented_geometry_op_form_is_exercised_by_the_fixture`; see the
/// module doc.)
#[test]
fn every_documented_geometry_op_name_is_exercised_by_the_fixture() {
    let markdown = read_chunk(CHUNK_PATH);
    let documented = documented_geometry_op_names(&markdown);
    assert_scan_not_vacuous(&documented);

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
/// None of these seven is mentioned in any file under [`CHUNKS_DIR`] — a claim
/// the guard enforces against the WHOLE corpus (via [`read_all_chunks`]), not
/// just stdlib.md and geometry.md, so documenting one in any chunk reports it.
/// This is a REAL residual documentation gap, carried here explicitly rather
/// than folded into [`CONSTRUCTORS_DOCUMENTED_IN_GEOMETRY_CHUNK`] — filing them
/// as "documented elsewhere" would launder a gap into a false coverage claim,
/// which is exactly the failure mode this guard exists to catch, one level down.
/// They are out of scope here (they are constructors, not operations) and are
/// covered by a separate follow-up, task #5700.
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
/// Deliberately loose about CONTEXT: it exists only to justify an exclusion
/// (does the chunk talk about this name at all?), never to pin a form or a
/// wording, so it does not care about the surrounding prose, the section, or the
/// arity written.
///
/// It is NOT loose about the identifier itself. The match is anchored on a
/// non-identifier boundary, so `box` is satisfied only by a standalone `box(` —
/// never by `rounded_box(`. Both names are live entries of
/// [`CONSTRUCTORS_DOCUMENTED_IN_GEOMETRY_CHUNK`], so an unanchored substring
/// match would let the `box` exclusion ride on the `rounded_box` row: deleting
/// geometry.md's `box(` row would keep the exclusion green while the doc gap it
/// claims to cover was real. Laundering a gap into a false coverage claim is the
/// exact failure this guard exists to catch, so the strict direction is the safe
/// one here.
fn chunk_mentions(markdown: &str, name: &str) -> bool {
    let needle = format!("{name}(");
    markdown.match_indices(&needle).any(|(at, _)| {
        markdown[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_')
    })
}

/// Everything wrong with registry → chunk documentation coverage, as
/// human-actionable violation lines.
///
/// THREE distinct classes, because a guard whose exclusions are unaudited is
/// not a guard — appending a name to either slice would silence it while
/// documenting nothing:
///
/// 1. A `registry` name that no chunk documents. The reason this guard exists.
///    A name counts as documented when the stdlib.md scan
///    ([`documented_geometry_op_names`], reused verbatim so this inherits its
///    exact semantics) finds it, or when an exclusion slice carries it.
/// 2. A `documented_elsewhere` entry that `geometry_md` does NOT mention — an
///    exclusion claiming coverage that does not exist.
/// 3. A `known_gap` entry that `all_chunks_md` mentions — a gap that has since
///    been closed, so the entry is stale and would otherwise exempt the name
///    from class 1 forever, masking a later regression. This class reads the
///    WHOLE chunk corpus, not just the two chunks above, because the known-gap
///    claim is "documented in NO chunk": checking two of seventeen would let a
///    name documented in, say, `types.md` stay permanently exempt.
///
/// Each line names its own corrective action, so a failure tells a maintainer
/// what to do rather than only what is wrong. Classes 2 and 3 are checked
/// against the exclusion slices themselves, independent of `registry`
/// membership, so a stale entry is caught even after its name leaves the
/// registry.
///
/// Pure and fully parameterized over its inputs — rather than reading the
/// consts and the real chunks directly — so the controls below can drive it
/// with synthetic data and pin its discriminating power.
fn geometry_op_doc_coverage_violations(
    registry: &[&str],
    stdlib_md: &str,
    geometry_md: &str,
    all_chunks_md: &str,
    documented_elsewhere: &[&str],
    known_gap: &[&str],
) -> Vec<String> {
    let documented = documented_geometry_op_names(stdlib_md);
    let mut out = Vec::new();

    // Class 1 — implemented, but written down nowhere.
    for name in registry.iter().copied() {
        let covered = documented.iter().any(|d| d.as_str() == name)
            || documented_elsewhere.contains(&name)
            || known_gap.contains(&name);
        if !covered {
            out.push(format!(
                "{name}: implemented but documented in NO chunk — FIX: add a row to \
                 stdlib.md's '{CHUNK_SECTION}' table and mirror the call into \
                 tests/fixtures/stdlib_geometry_ops_smoke.ri"
            ));
        }
    }

    // Class 2 — an exclusion claiming coverage that does not exist.
    for name in documented_elsewhere.iter().copied() {
        if !chunk_mentions(geometry_md, name) {
            out.push(format!(
                "{name}: excluded as 'documented in geometry.md', but geometry.md never \
                 mentions it — FIX: correct the exclusion list (document it there, or drop \
                 the entry so this guard covers the name normally)"
            ));
        }
    }

    // Class 3 — a known gap that has since been closed, anywhere in the corpus.
    for name in known_gap.iter().copied() {
        if chunk_mentions(all_chunks_md, name) {
            out.push(format!(
                "{name}: carried as a known documentation gap, but some chunk DOES now \
                 mention it — FIX: delete the stale known-gap entry so this guard covers \
                 the name normally"
            ));
        }
    }

    out.sort();
    out.dedup();
    out
}

/// Every geometry op the compiler implements must be documented in some chunk,
/// and both exclusion lists must be telling the truth about why a name is
/// exempt.
///
/// The chunks are the AUTHORITATIVE language reference served verbatim to the
/// in-GUI assistant, so an implemented-but-undocumented op is invisible to
/// every designer using it.
#[test]
fn every_implemented_geometry_op_is_documented_in_a_chunk() {
    let stdlib_md = read_chunk(CHUNK_PATH);
    let geometry_md = read_chunk(GEOMETRY_CHUNK_PATH);
    let all_chunks_md = read_all_chunks();

    // Anti-vacuity, every input. An empty registry would make this guard pass
    // trivially, and an emptied scan (heading renamed, rows no longer `**`/
    // backticked) would make it report all 64 names — pin both so neither
    // failure mode is mistaken for a real signal. (`read_all_chunks` pins its
    // own corpus size.)
    assert!(
        GEOMETRY_FUNCTION_NAMES.len() >= 50,
        "anti-vacuity: GEOMETRY_FUNCTION_NAMES holds only {} name(s) — the registry this \
         guard walks has been gutted or moved, and the guard gives NO protection",
        GEOMETRY_FUNCTION_NAMES.len()
    );
    assert_scan_not_vacuous(&documented_geometry_op_names(&stdlib_md));
    assert!(
        !geometry_md.trim().is_empty(),
        "anti-vacuity: {GEOMETRY_CHUNK_PATH} is empty, so the 'documented elsewhere' \
         exclusions rest on nothing"
    );

    let violations = geometry_op_doc_coverage_violations(
        GEOMETRY_FUNCTION_NAMES,
        &stdlib_md,
        &geometry_md,
        &all_chunks_md,
        CONSTRUCTORS_DOCUMENTED_IN_GEOMETRY_CHUNK,
        CONSTRUCTORS_DOCUMENTED_NOWHERE,
    );

    assert!(
        violations.is_empty(),
        "registry → doc coverage is broken. The chunks are served verbatim to the in-GUI \
         assistant, so an op documented nowhere is one no designer can discover, and an \
         untrue exclusion hides exactly that. Each line below names its own fix:\n{}",
        violations.join("\n")
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

/// Stand-in for a THIRD chunk — one the class-1 and class-2 checks never read.
/// Mentions `elsewhere_ctor` and nothing else.
const SYNTHETIC_OTHER_CHUNK_MD: &str = "See also `elsewhere_ctor(v)` for the implicit form.";

/// Stand-in for the whole chunk corpus, mirroring `read_all_chunks`'s
/// concatenation: everything the three synthetic chunks say, and nothing else.
fn synthetic_all_chunks() -> String {
    format!("{SYNTHETIC_STDLIB_MD}\n{SYNTHETIC_GEOMETRY_MD}\n{SYNTHETIC_OTHER_CHUNK_MD}")
}

/// NEGATIVE CONTROL — the guard reports a genuinely undocumented registry name,
/// and stays silent for one that stdlib.md documents and one that a justified
/// exclusion covers. Proves it is neither blind nor "reports everything".
#[test]
fn undocumented_registry_names_are_reported_and_documented_ones_are_not() {
    let reported = geometry_op_doc_coverage_violations(
        &["documented_op", "mentioned_ctor", "undocumented_op"],
        SYNTHETIC_STDLIB_MD,
        SYNTHETIC_GEOMETRY_MD,
        &synthetic_all_chunks(),
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
    let reported = geometry_op_doc_coverage_violations(
        &["ghost_ctor"],
        SYNTHETIC_STDLIB_MD,
        SYNTHETIC_GEOMETRY_MD,
        &synthetic_all_chunks(),
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
///
/// The third case is the one that pins the corpus WIDTH: `elsewhere_ctor` lives
/// only in the synthetic third chunk, which neither the stdlib.md scan nor the
/// geometry.md mention-check ever reads. It fires only because class 3 walks the
/// whole corpus, so narrowing that read back to two chunks goes RED here.
#[test]
fn a_known_gap_entry_that_has_since_been_documented_is_reported() {
    let via_stdlib = geometry_op_doc_coverage_violations(
        &["documented_op"],
        SYNTHETIC_STDLIB_MD,
        SYNTHETIC_GEOMETRY_MD,
        &synthetic_all_chunks(),
        &[],
        &["documented_op"],
    );
    assert!(
        via_stdlib.iter().any(|v| v.contains("documented_op")),
        "`documented_op` is carried as a known gap but stdlib.md now documents it — the \
         stale entry must be reported so it gets deleted. Got: {via_stdlib:?}"
    );

    let via_geometry = geometry_op_doc_coverage_violations(
        &["mentioned_ctor"],
        SYNTHETIC_STDLIB_MD,
        SYNTHETIC_GEOMETRY_MD,
        &synthetic_all_chunks(),
        &[],
        &["mentioned_ctor"],
    );
    assert!(
        via_geometry.iter().any(|v| v.contains("mentioned_ctor")),
        "`mentioned_ctor` is carried as a known gap but geometry.md now mentions it — the \
         stale entry must be reported so it gets deleted. Got: {via_geometry:?}"
    );

    let via_other_chunk = geometry_op_doc_coverage_violations(
        &["elsewhere_ctor"],
        SYNTHETIC_STDLIB_MD,
        SYNTHETIC_GEOMETRY_MD,
        &synthetic_all_chunks(),
        &[],
        &["elsewhere_ctor"],
    );
    assert!(
        via_other_chunk.iter().any(|v| v.contains("elsewhere_ctor")),
        "`elsewhere_ctor` is carried as documented in NO chunk, but a chunk OTHER than \
         stdlib.md/geometry.md mentions it — the known-gap audit must read the whole \
         corpus, or a name documented in any other chunk stays exempt forever. \
         Got: {via_other_chunk:?}"
    );
}

/// ANTI-ROT — the mention check is anchored on an identifier boundary, so a name
/// that is a SUFFIX of a documented one is not counted as documented.
///
/// [`CONSTRUCTORS_DOCUMENTED_IN_GEOMETRY_CHUNK`] carries both `box` and
/// `rounded_box` today. Under an unanchored substring match the `box` exclusion
/// would ride on `rounded_box(` — a false coverage claim surviving on a
/// coincidence of spelling, which is the exact laundering class 2 exists to
/// catch.
#[test]
fn a_name_that_is_only_a_suffix_of_a_documented_one_is_not_counted_as_mentioned() {
    assert!(
        !chunk_mentions("`rounded_box(w, h, d, r)` builds a thing.", "box"),
        "`box` must NOT count as mentioned by a chunk that only documents `rounded_box(`"
    );
    assert!(
        chunk_mentions("`rounded_box(w, h, d, r)` and `box(w, h, d)`.", "box"),
        "`box` MUST count as mentioned once the chunk documents it standalone"
    );

    let reported = geometry_op_doc_coverage_violations(
        &["box"],
        SYNTHETIC_STDLIB_MD,
        "`rounded_box(w, h, d, r)` builds a thing.",
        &synthetic_all_chunks(),
        &["box"],
        &[],
    );
    assert!(
        reported.iter().any(|v| v.contains("box")),
        "`box` is excluded as documented in geometry.md, but geometry.md only documents \
         `rounded_box(` — the exclusion must not ride on a suffix match. Got: {reported:?}"
    );
}

// ── Doc form scan: (name, arity) ─────────────────────────────────────────────
//
// `documented_geometry_op_names` (above) collapses every overload of a name
// into one entry, so it alone cannot see a second arity for an
// already-documented name. `documented_geometry_op_forms` is the same span
// scan widened to also record each span's argument count as an `Arity`, so
// overloads are tracked as distinct forms rather than collapsed to a name —
// feeding `unmirrored_documented_forms`, so an unmirrored overload is RED via
// `every_documented_geometry_op_form_is_exercised_by_the_fixture` (task #5583).
//
// The tests below pin the extraction rule directly (inline markdown, no
// fixture/chunk involved) before anything consumes it. The ellipsis rule is
// the one genuine hazard: an arg equal to `…` or ENDING IN `…` (e.g.
// `weights…`) contributes 0 to the minimum and marks the form variadic —
// `documented_geometry_op_forms_suffixed_ellipsis_is_at_least` pins exactly
// that case — the one real chunk rows like `shell(..., faces…)` rely on.

#[test]
fn documented_geometry_op_forms_extracts_exact_arity() {
    let markdown = r#"
## Key Geometry Operations

**Transform:** `rotate(geo, ax, ay, az, angle)`
"#;

    assert_eq!(
        documented_geometry_op_forms(markdown),
        vec![DocForm {
            name: "rotate".to_string(),
            arity: Arity::Exact(5),
        }],
        "a fixed-arity span must extract as Exact(arg count)"
    );
}

#[test]
fn documented_geometry_op_forms_keeps_overloads_on_one_row_distinct() {
    let markdown = r#"
## Key Geometry Operations

**Transform:** `mirror(geo, plane)`, `mirror(geo, ox, oy, oz, nx, ny, nz)`
"#;

    assert_eq!(
        documented_geometry_op_forms(markdown),
        vec![
            DocForm {
                name: "mirror".to_string(),
                arity: Arity::Exact(2),
            },
            DocForm {
                name: "mirror".to_string(),
                arity: Arity::Exact(7),
            },
        ],
        "two overloads of one name on one row must NOT collapse into a single form"
    );
}

#[test]
fn documented_geometry_op_forms_bare_ellipsis_is_at_least() {
    let markdown = r#"
## Key Geometry Operations

**Booleans:** `union_all(a, b, …)`
"#;

    assert_eq!(
        documented_geometry_op_forms(markdown),
        vec![DocForm {
            name: "union_all".to_string(),
            arity: Arity::AtLeast(2),
        }],
        "a bare `…` arg must contribute 0 to the minimum and mark the form variadic"
    );
}

#[test]
fn documented_geometry_op_forms_suffixed_ellipsis_is_at_least() {
    let markdown = r#"
## Key Geometry Operations

**Modify:** `shell(solid, thickness, faces…)`
**Curves:** `nurbs(degree, n_points, coords…, weights…)`
"#;

    assert_eq!(
        documented_geometry_op_forms(markdown),
        vec![
            DocForm {
                name: "nurbs".to_string(),
                arity: Arity::AtLeast(2),
            },
            DocForm {
                name: "shell".to_string(),
                arity: Arity::AtLeast(2),
            },
        ],
        "an arg ENDING IN `…` (not just an arg equal to it) must also contribute 0 to the \
         minimum — this is the case that decides whether the fixture's nurbs/10 call can ever \
         match its documented form"
    );
}

#[test]
fn documented_geometry_op_forms_spans_without_parens_contribute_nothing() {
    let markdown = r#"
## Key Geometry Operations

**NoParens:** `List<Geometry>`, `Length`
"#;

    assert_eq!(
        documented_geometry_op_forms(markdown),
        Vec::<DocForm>::new(),
        "a backtick span with no `(` (a type name, not a call) must contribute no form"
    );
}

#[test]
fn documented_geometry_op_forms_zero_arg_span_is_exact_zero() {
    let markdown = r#"
## Key Geometry Operations

**Zero:** `foo()`
"#;

    assert_eq!(
        documented_geometry_op_forms(markdown),
        vec![DocForm {
            name: "foo".to_string(),
            arity: Arity::Exact(0),
        }],
        "an empty arg list must extract as Exact(0), not Exact(1) from a phantom blank arg"
    );
}

#[test]
fn documented_geometry_op_forms_skips_unbalanced_parens_without_panicking() {
    let markdown = r#"
## Key Geometry Operations

**Bad:** `broken(a, b`
**Good:** `fine(a, b)`
"#;

    assert_eq!(
        documented_geometry_op_forms(markdown),
        vec![DocForm {
            name: "fine".to_string(),
            arity: Arity::Exact(2),
        }],
        "a span with `(` but no matching `)` must be skipped, not panic, and must not stop \
         later valid spans from being scanned"
    );
}

#[test]
fn documented_geometry_op_forms_respects_section_and_bold_prefix_narrowing() {
    let markdown = r#"
## Other Section

**Ignored:** `should_not_appear(a, b)`

## Key Geometry Operations

Prose with `also_ignored(a, b)` inline is not a bolded row.

**Real:** `included(a, b)`

## Constants

**Ignored2:** `also_not_appear(a, b)`
"#;

    assert_eq!(
        documented_geometry_op_forms(markdown),
        vec![DocForm {
            name: "included".to_string(),
            arity: Arity::Exact(2),
        }],
        "only `**`-prefixed lines inside the Key Geometry Operations section may contribute a \
         form — reuses documented_geometry_op_names's existing narrowing"
    );
}

// ── Call form scan: (name, arity) ────────────────────────────────────────────
//
// `geometry_call_names` collapses every call to a name to a single entry, so
// two arities of the same call name in the fixture are indistinguishable from
// one. `geometry_call_forms` is the fixture-side counterpart to
// `documented_geometry_op_forms`: the same AST walk, additionally recording
// each `FunctionCall`'s argument count.
//
// Driven by inline `structure def` snippets — the same shape
// `geometry_call_names`'s existing tests use — so the existing `let`-member
// walker contract (panic, not skip, on any other declaration/member kind)
// stays exercised.

#[test]
fn geometry_call_forms_records_distinct_arities_of_the_same_name() {
    let source = r#"
structure def TwoArities {
    let solid = box(10mm, 10mm, 10mm)
    let r5 = rotate(solid, 0, 0, 1, 90deg)
    let r2 = rotate(solid, orient_identity())
}
"#;

    assert_eq!(
        geometry_call_forms(source, "two-arities snippet"),
        vec![
            ("box".to_string(), 3),
            ("orient_identity".to_string(), 0),
            ("rotate".to_string(), 2),
            ("rotate".to_string(), 5),
        ],
        "two calls to the same name at different arities must both survive as distinct forms, \
         not collapse to one"
    );
    assert_eq!(
        geometry_call_names(source, "two-arities snippet"),
        vec![
            "box".to_string(),
            "orient_identity".to_string(),
            "rotate".to_string(),
        ],
        "the existing name-only view must still collapse the two rotate arities to one entry — \
         pins the step-4 re-derivation as behaviour-preserving"
    );
}

#[test]
fn geometry_call_forms_records_nested_calls_without_inflating_the_outer_count() {
    let source = r#"
structure def NestedCall {
    let solid = box(10mm, 10mm, 10mm)
    let f = fillet(solid, edges(solid), 1mm)
}
"#;

    assert_eq!(
        geometry_call_forms(source, "nested-call snippet"),
        vec![
            ("box".to_string(), 3),
            ("edges".to_string(), 1),
            ("fillet".to_string(), 3),
        ],
        "a nested call inside an argument must record its OWN form (edges/1) and must not \
         inflate the outer call's own arg count (fillet/3, not fillet/1 or fillet/5)"
    );
    assert_eq!(
        geometry_call_names(source, "nested-call snippet"),
        vec!["box".to_string(), "edges".to_string(), "fillet".to_string()],
        "the existing name-only view must still return the same deduped name set"
    );
}

#[test]
fn geometry_call_forms_zero_arg_call() {
    let source = r#"
structure def ZeroArgCall {
    let o = orient_identity()
}
"#;

    assert_eq!(
        geometry_call_forms(source, "zero-arg snippet"),
        vec![("orient_identity".to_string(), 0)],
        "a zero-arg call must record arity 0, not be dropped or miscounted"
    );
    assert_eq!(
        geometry_call_names(source, "zero-arg snippet"),
        vec!["orient_identity".to_string()],
        "the existing name-only view must still return the same deduped name set"
    );
}

#[test]
fn geometry_call_forms_output_is_sorted_and_deduped() {
    let source = r#"
structure def SortedAndDeduped {
    let z = scale(box(10mm, 10mm, 10mm), 2.0)
    let a = scale(box(5mm, 5mm, 5mm), 2.0)
}
"#;

    assert_eq!(
        geometry_call_forms(source, "sorted-and-deduped snippet"),
        vec![("box".to_string(), 3), ("scale".to_string(), 2)],
        "two calls sharing the same (name, arity) — here box/3 and scale/2, each appearing \
         twice — must collapse to one entry each in a deterministic sorted order"
    );
    assert_eq!(
        geometry_call_names(source, "sorted-and-deduped snippet"),
        vec!["box".to_string(), "scale".to_string()],
        "the existing name-only view must still return the same deduped name set"
    );
}

// ── Doc → fixture pairing at FORM granularity ────────────────────────────────
//
// `unmirrored_documented_forms` is the FORM-granularity counterpart to
// `every_documented_geometry_op_name_is_exercised_by_the_fixture`: it pairs
// `documented_geometry_op_forms` against `geometry_call_forms` so a documented
// OVERLOAD with no compiling instance at that exact arity is reported, not
// just an absent NAME.
//
// The tests below replay the printer_v01 regression this task exists to shut
// a door on: `rotate(geo, axis, angle)` / `translate(geo, vector)` were once
// documented in stdlib.md at an arity the compiler rejects (task 5347). The
// name-only guard cannot see this — `rotate` and `translate` are both real
// names, exercised by the fixture, just not at THAT arity. Each test feeds a
// SYNTHETIC markdown snippet — never editing stdlib.md or the fixture — but
// checks it against the REAL fixture source via `read_fixture()`.

/// Every documented form in `markdown` with no matching call in
/// `fixture_source` — the doc → fixture direction at FORM granularity.
///
/// A `DocForm` is considered mirrored by any fixture `(name, count)` call
/// where `count == n` for `Arity::Exact(n)`, or `count >= n` for
/// `Arity::AtLeast(n)`. Pure over two `&str`s — no file I/O — so callers
/// (including the inline printer_v01-replay controls below) can drive it with
/// synthetic markdown checked against the real fixture. Sorted and deduped,
/// so a caller's failure message names the exact unmirrored form(s).
fn unmirrored_documented_forms(markdown: &str, fixture_source: &str, label: &str) -> Vec<DocForm> {
    let documented = documented_geometry_op_forms(markdown);
    let fixture_forms = geometry_call_forms(fixture_source, label);

    let mut unmirrored: Vec<DocForm> = documented
        .into_iter()
        .filter(|form| {
            !fixture_forms.iter().any(|(name, count)| {
                *name == form.name
                    && match form.arity {
                        Arity::Exact(n) => *count == n,
                        Arity::AtLeast(n) => *count >= n,
                    }
            })
        })
        .collect();

    unmirrored.sort();
    unmirrored.dedup();
    unmirrored
}

#[test]
fn unmirrored_documented_forms_replays_the_printer_v01_regression() {
    let markdown = r#"
## Key Geometry Operations

**Transform:** `translate(geo, vector)`, `rotate(geo, axis, angle)`, `scale(geo, factor)`
"#;

    assert_eq!(
        unmirrored_documented_forms(markdown, &read_fixture(), "fixture"),
        vec![
            DocForm {
                name: "rotate".to_string(),
                arity: Arity::Exact(3),
            },
            DocForm {
                name: "translate".to_string(),
                arity: Arity::Exact(2),
            },
        ],
        "this is the reviewer's exact repro of the printer_v01 regression (task 5347): \
         rotate/3 and translate/2 must be reported unmirrored even though `rotate` and \
         `translate` are both real, fixture-exercised names — scale/2 IS mirrored and must \
         NOT be reported, proving this is not merely the name-only guard in disguise"
    );
}

#[test]
fn unmirrored_documented_forms_reports_nothing_for_the_corrected_row() {
    let markdown = r#"
## Key Geometry Operations

**Transform:** `translate(geo, dx, dy, dz)`, `rotate(geo, ax, ay, az, angle)` or `rotate(geo, orientation)`, `scale(geo, factor)`
"#;

    assert_eq!(
        unmirrored_documented_forms(markdown, &read_fixture(), "fixture"),
        Vec::<DocForm>::new(),
        "the control's control: with the compiler-true forms stdlib.md documents today, \
         nothing must be reported — the guard is not merely \"reports everything\""
    );
}

#[test]
fn unmirrored_documented_forms_at_least_matches_greater_or_equal_but_exact_does_not() {
    let markdown = r#"
## Key Geometry Operations

**Curves:** `nurbs(degree, n_points, coords…, weights…)`, `nurbs(a, b, c, d, e, f, g, h, i, j, k, l)`
"#;

    assert_eq!(
        unmirrored_documented_forms(markdown, &read_fixture(), "fixture"),
        vec![DocForm {
            name: "nurbs".to_string(),
            arity: Arity::Exact(12),
        }],
        "the fixture's nurbs call has 10 args: AtLeast(2) must match (10 >= 2, so the variadic \
         form is NOT reported) but Exact(12) must not (10 != 12, so it IS reported) — pins that \
         AtLeast compares `>=` and Exact compares `==`"
    );
}

#[test]
fn unmirrored_documented_forms_reports_a_name_with_no_fixture_call_at_all() {
    let markdown = r#"
## Key Geometry Operations

**Bogus:** `nonexistent_op(a, b)`
"#;

    assert_eq!(
        unmirrored_documented_forms(markdown, &read_fixture(), "fixture"),
        vec![DocForm {
            name: "nonexistent_op".to_string(),
            arity: Arity::Exact(2),
        }],
        "a documented name absent from the fixture entirely must be reported — the form gate \
         subsumes the name gate's case too"
    );
}

/// Anti-vacuity guard every [`documented_geometry_op_forms`] caller owes its
/// assertions — the overload-aware sibling of [`assert_scan_not_vacuous`].
///
/// Two failure modes, not one. The COUNT floor catches the same vacuity that
/// helper does (a heading rename, or rows that stop using `**`/backticks would
/// empty the scan). The SENTINELS additionally catch a mode the name-level
/// helper cannot have: a scan that still finds every name but silently
/// collapses each name's overloads back to one entry, degrading this gate into
/// the name-only gate it is supposed to complement. Both members of each
/// multi-overload pair are therefore required.
///
/// The floor is deliberately well under the live count so ordinary chunk
/// editing does not trip it, but well over half of it so a genuinely gutted
/// scan cannot slip through: the section carries 50 forms across ~43 distinct
/// names today (measured, task #5583 — it was 33 before task 5675 documented 15
/// further ops).
fn assert_form_scan_not_vacuous(documented: &[DocForm]) {
    assert!(
        documented.len() >= 40,
        "the '{CHUNK_SECTION}' form scan found only {} form(s) in {CHUNK_PATH} — the scan is \
         vacuous (heading renamed, or the signature rows no longer start with `**` and use \
         backticks) and gives NO protection",
        documented.len()
    );
    for sentinel in [
        DocForm {
            name: "rotate".to_string(),
            arity: Arity::Exact(5),
        },
        DocForm {
            name: "rotate".to_string(),
            arity: Arity::Exact(2),
        },
        DocForm {
            name: "mirror".to_string(),
            arity: Arity::Exact(2),
        },
        DocForm {
            name: "mirror".to_string(),
            arity: Arity::Exact(7),
        },
        DocForm {
            name: "circular_pattern".to_string(),
            arity: Arity::Exact(4),
        },
        DocForm {
            name: "circular_pattern".to_string(),
            arity: Arity::Exact(9),
        },
        DocForm {
            name: "union_all".to_string(),
            arity: Arity::AtLeast(2),
        },
    ] {
        assert!(
            documented.contains(&sentinel),
            "anti-vacuity: {sentinel:?} is documented in '{CHUNK_SECTION}' but the form scan \
             did not find it — both members of each multi-overload pair (rotate, mirror, \
             circular_pattern) are required sentinels, proving the scan splits overloads \
             instead of collapsing them back to the name-only scan this gate complements"
        );
    }
}

/// Every documented FORM in the real chunk must be exercised by the real
/// fixture at that exact arity — the doc → fixture direction at FORM
/// granularity (task #5583), closing the gap
/// `every_documented_geometry_op_name_is_exercised_by_the_fixture` leaves
/// open (see the module doc).
///
/// This is GREEN on arrival: all 50 documented forms already have a compiling
/// fixture instance at their documented arity (measured — task 5675's
/// name-level guard had already forced each of the 15 ops it documented into
/// the fixture, and wrote each at the documented arity). Its discriminating
/// power is therefore pinned by the inline printer_v01-replay controls above
/// (`unmirrored_documented_forms_replays_the_printer_v01_regression` and its
/// control's control), not by an artificial gap planted here. What it buys is
/// forward protection: the NEXT op documented with an overload nobody mirrors
/// goes RED at its source.
#[test]
fn every_documented_geometry_op_form_is_exercised_by_the_fixture() {
    let markdown = std::fs::read_to_string(CHUNK_PATH).unwrap_or_else(|e| {
        panic!("{CHUNK_PATH} must be readable ({e}) — update CHUNK_PATH if the chunk moved")
    });
    let documented = documented_geometry_op_forms(&markdown);
    assert_form_scan_not_vacuous(&documented);

    let unmirrored = unmirrored_documented_forms(&markdown, &read_fixture(), "fixture");
    assert!(
        unmirrored.is_empty(),
        "stdlib.md documents geometry-op FORM(s) with no compiling instance at that exact \
         arity in tests/fixtures/stdlib_geometry_ops_smoke.ri — a documented overload can go \
         unmirrored even when its NAME is exercised, just at a different arity (task #5583). \
         Remediation: add a call at that arity to \
         crates/reify-compiler/tests/fixtures/stdlib_geometry_ops_smoke.ri, or correct the \
         signature in crates/reify-mcp/src/tools/chunks/stdlib.md if the compiler does not \
         accept it. Unmirrored form(s): {}",
        unmirrored
            .iter()
            .map(|f| format!("{}/{:?}", f.name, f.arity))
            .collect::<Vec<_>>()
            .join(", ")
    );
}
