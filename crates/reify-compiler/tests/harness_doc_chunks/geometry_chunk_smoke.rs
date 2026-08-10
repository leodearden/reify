//! Compile-smoke test for the "geometry" language-reference chunk
//! (`crates/reify-mcp/src/tools/chunks/geometry.md`), served to the in-GUI
//! assistant via `reify_language_reference`.
//!
//! Mirrors sibling task 5347's stdlib.md audit: this test pins every
//! documented geometry-constructor call form to what the compiler actually
//! accepts, so a phantom signature in the doc (or a future compiler
//! signature change) is caught here rather than by a designer typing the
//! documented-but-wrong call into a `.ri` file.
//!
//! Lives in `reify-compiler` (not `reify-mcp`, where `geometry.md` itself
//! lives) because `reify-mcp` does not depend on `reify-compiler` and so
//! cannot invoke `compile_source_with_stdlib` — the same cross-crate
//! placement sibling task 5347 used for its stdlib-chunk smoke test.
//!
//! Fixtures are curated `.ri` snippets, not scraped from geometry.md itself:
//! the doc intermixes non-compilable schematic notation (type params, trait
//! lists, `-> Solid` return annotations) with real call forms, so a scraper
//! would need a fragile grammar to separate the two. Each fixture instead
//! mirrors one documented call form exactly, wrapped in the minimal
//! compilable module shape (`examples/bracket.ri`'s
//! `structure def X { let body = box(...) }` pattern).
//!
//! # What is NOT established
//!
//! THE CANONICAL SCOPE STATEMENT FOR THIS FILE. Everything below is stated
//! once, here; the individual test docstrings point back rather than restating,
//! so there is exactly one place to correct when the compiler gets stricter.
//!
//! The compile-acceptance assertions here (both `assert_compiles` and the
//! ```` ```reify ````-fence scrape) establish PARSE + COMPILE ACCEPTANCE and
//! nothing stronger. Specifically:
//!
//! - **Arity is not enforced** for the interference/clearance query names.
//!   Mutation-verified against the live fences: rewriting them to
//!   `min_clearance(s)`, `intersects(housing)` and `distance(housing)` leaves
//!   this whole suite GREEN. A wrong-arity documented call form would not be
//!   caught here.
//! - **Argument dimension/type is not enforced** for those names either — none
//!   of the five has a checkable slot in `builtin_signatures`'s arg-slot table,
//!   so the `(Snapshot, Int, Int)` contract the chunk documents is a convention
//!   the fences FOLLOW but nothing in this file checks.
//! - **An unknown call name is not, by itself, an error.** A `structure def`
//!   body silently accepts an unresolved function and types the call from its
//!   FIRST argument's `result_type` (the same permissive fallback noted at the
//!   `line_segment`/`arc`/`polygon` block below). What discriminating power the
//!   fence guard has is INCIDENTAL and downstream: mutating `distance(` →
//!   `distanceZZZ(` makes the call type as `Geometry`, so the fence's own
//!   `constraint gap > 1mm` then fails `CmpOperandKind`. Mutating
//!   `intersects(` → `intersectsZZZ(` produces NO error at all, because the
//!   fence only feeds that result to `not`.
//!   `bogus_query_name_feeding_a_comparison_is_an_error` is the negative
//!   control that pins the part that does work, so it cannot silently rot away.
//!
//! STALENESS CAVEAT — the first two bullets are HAND-VERIFIED mutation results,
//! not executed assertions, so they can go quietly false. Their shared cause is
//! that `builtin_signatures`'s arg-slot table has no entry for any of the five
//! names; the day one gains a checkable slot, both bullets stop being true.
//! They are not asserted for two reasons: `builtin_signatures` is a private
//! module (`mod builtin_signatures;` in `reify-compiler/src/lib.rs`), so a test
//! cannot read the table; and an assertion of the form "the mutated arity still
//! compiles clean" would go RED the day the hole is FIXED, which is exactly the
//! wrong incentive. Re-verify these two bullets by hand whenever
//! `builtin_signatures.rs` gains geometry-query entries. What IS executed:
//! `bogus_query_name_feeding_a_comparison_is_an_error` asserts on the
//! `CmpOperandKind` diagnostic identity, so the third bullet's mechanism is
//! pinned rather than merely asserted.
//!
//! The registry-membership assertions in
//! `interference_oracle_names_documented_in_geometry_chunk` are what actually
//! close the phantom-name direction; the fence compile is a parse/shape guard.
//!
//! # Known duplication (out of scope here)
//!
//! `harness_doc_chunks` compiles this module and
//! `stdlib_chunk_geometry_ops_smoke.rs` into one test binary, and the two now
//! carry near-identical scaffolding: a `geometry.md` path const (here
//! `CHUNK_PATH`, there `GEOMETRY_CHUNK_PATH`), a loud-panic-never-skip
//! `read_chunk`, and a `## `-heading section scanner. Worse than mere
//! duplication, the two scanners DISAGREE — the one here is fence-aware (a
//! `## ` line inside a fence is content, not a boundary) and matches headings by
//! normalised words, while the sibling's is neither, so which slicing semantics
//! you get depends on which module's test fires.
//!
//! The right fix is a shared `chunk_io` module declared from
//! `tests/harness_doc_chunks.rs`, holding the path consts, `read_chunk(path)`
//! and the fence-aware `section_body`. That needs edits to
//! `tests/harness_doc_chunks.rs` and `stdlib_chunk_geometry_ops_smoke.rs`,
//! neither of which is in task 5389's locked file set, so it is deferred rather
//! than done here — filed as ticket `tkt_0RS9A7843SBQ4BZX1A2ACY5TC1` (curator
//! runs asynchronously, so that is a ticket id, not yet a task id). Delete this
//! section when the extraction lands.

use reify_test_support::{compile_source_with_stdlib, errors_only};

/// Compile `module_src` AS A WHOLE MODULE and assert zero Severity::Error
/// diagnostics. The source is echoed in the panic so a failing scraped fence is
/// fixable without re-reading the chunk.
///
/// This is the single zero-Error assertion site in the file; `assert_compiles`
/// delegates here after wrapping a bare expression. A whole-module entry point
/// is needed because the clearance examples are multi-let `structure def`s (both
/// the query call and its operands must be separate let bindings), which the
/// `structure def Smoke { let g = … }` wrapper cannot express.
fn assert_module_compiles(label: &str, module_src: &str) {
    let compiled = compile_source_with_stdlib(module_src);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "{label}: expected this module to compile with zero Error diagnostics, got: {:#?}\n\
         --- module source ---\n{module_src}\n--- end module source ---",
        errors
    );
}

/// Compile `geometry_expr` wrapped in a minimal `structure def Smoke { let g
/// = <geometry_expr> }` module and assert there are zero Severity::Error
/// diagnostics. On failure, the panic message names `label` and the expression
/// under test, and dumps every Error diagnostic, so a failing signature is
/// immediately identifiable.
fn assert_compiles(label: &str, geometry_expr: &str) {
    assert_module_compiles(
        &format!("{label} (expression `{geometry_expr}`)"),
        &format!("structure def Smoke {{ let g = {} }}", geometry_expr),
    );
}

// --- Solid primitives (geometry.md "Solid Primitives" block) ---

#[test]
fn box_primitive_compiles() {
    assert_compiles("box", "box(10mm, 20mm, 5mm)");
}

#[test]
fn box_centered_compiles() {
    assert_compiles("box_centered", "box_centered(10mm, 20mm, 5mm)");
}

#[test]
fn cylinder_compiles() {
    assert_compiles("cylinder", "cylinder(5mm, 10mm)");
}

#[test]
fn cylinder_centered_compiles() {
    assert_compiles("cylinder_centered", "cylinder_centered(5mm, 10mm)");
}

#[test]
fn cone_compiles() {
    assert_compiles("cone", "cone(5mm, 3mm, 10mm)");
}

#[test]
fn sphere_compiles() {
    assert_compiles("sphere", "sphere(5mm)");
}

#[test]
fn torus_compiles() {
    assert_compiles("torus", "torus(10mm, 2mm)");
}

#[test]
fn tube_compiles() {
    assert_compiles("tube", "tube(10mm, 5mm, 20mm)");
}

#[test]
fn wedge_compiles() {
    assert_compiles("wedge", "wedge(10mm, 10mm, 10mm, 5mm)");
}

#[test]
fn rounded_box_compiles() {
    // corner_r=3mm > 0 and 2*3mm=6mm < min(20mm,20mm) — satisfies the
    // rounded-corner constraint documented at geometry.md line 79.
    assert_compiles("rounded_box", "rounded_box(20mm, 20mm, 10mm, 3mm)");
}

// --- 2D profiles (geometry.md "2D profiles" block) ---

#[test]
fn rectangle_compiles() {
    assert_compiles("rectangle", "rectangle(20mm, 10mm)");
}

#[test]
fn circle_profile_compiles() {
    assert_compiles("circle_profile", "circle(5mm)");
}

#[test]
fn ellipse_compiles() {
    assert_compiles("ellipse", "ellipse(10mm, 5mm)");
}

#[test]
fn rounded_rect_compiles() {
    // corner_r=2mm > 0 and 2*2mm=4mm < min(20mm,10mm) — satisfies the
    // rounded-corner constraint documented at geometry.md line 94.
    assert_compiles("rounded_rect", "rounded_rect(20mm, 10mm, 2mm)");
}

// --- Sweep (geometry.md anchoring table) ---

#[test]
fn extrude_compiles() {
    assert_compiles("extrude", "extrude(circle(5mm), 10mm)");
}

// --- Point/vector constructors (geometry.md "Geometry Constructors (Prelude)" block) ---

#[test]
fn point2_compiles() {
    assert_compiles("point2", "point2(0mm, 0mm)");
}

#[test]
fn point3_compiles() {
    assert_compiles("point3", "point3(0mm, 0mm, 0mm)");
}

#[test]
fn vec2_compiles() {
    assert_compiles("vec2", "vec2(1.0, 0.0)");
}

#[test]
fn vec3_compiles() {
    assert_compiles("vec3", "vec3(0.0, 0.0, 1.0)");
}

// --- Anchoring table: revolve(profile, ox, oy, oz, ax, ay, az, angle) ---

#[test]
fn revolve_compiles() {
    // geometry.md line 121 documents the 8-arg
    // `revolve(profile, ox, oy, oz, ax, ay, az, angle)` form (geometry.rs:1969):
    // profile + origin (0,0,0) + axis direction (0,0,1) + angle. The origin
    // triple (ox,oy,oz) is a Length position, so it takes `mm` literals;
    // the axis triple (ax,ay,az) is a direction only — its magnitude is
    // irrelevant and the compiler does not unit-check it — so it takes
    // dimensionless literals to avoid implying a direction vector has a
    // length unit.
    assert_compiles(
        "revolve",
        "revolve(circle(5mm), 0mm, 0mm, 0mm, 0.0, 0.0, 1.0, 90deg)",
    );
}

// --- Prelude/2D-profile constructors: line_segment, arc, polygon ---
//
// geometry.md's "Geometry Constructors (Prelude)" block (lines 56-61) and
// "2D profiles" block (line 87) documented four phantom call forms with no
// matching compiler arm: `line(start, end)`, `arc(center, radius,
// start_angle, end_angle)` [4-arg], `circle(center, radius)` [2-arg], and
// `polygon(points)`/`polygon(vertices)` [1-arg collection]. All four are now
// corrected to their authoritative flat-coordinate forms below.
//
// (`line` was never a registered builtin at all — absent from
// GEOMETRY_FUNCTION_NAMES and reify_ir::geometry::GEOMETRY_OP_DESCRIPTORS —
// so pre-fix it silently "compiled" via expr.rs's permissive
// unresolved-function fallback (types an >= 1-arg call from its first
// argument's result_type, no diagnostic) rather than erroring; arc/circle/
// polygon are registered builtins and pre-fix failed their own
// arg-count-exact / coordinate-pair checks. See this task's escalation
// resolution for the verification detail. The phantom `circle(center,
// radius)` prelude form is simply removed, not replaced — there is only
// ever the one 1-arg `circle(radius)`, already pinned by
// `circle_profile_compiles` above.)

#[test]
fn line_segment_compiles() {
    // geometry.md line 59 documents the 6-arg
    // `line_segment(x1, y1, z1, x2, y2, z2)` form (geometry_curve.rs:22).
    assert_compiles(
        "line_segment",
        "line_segment(0mm, 0mm, 0mm, 10mm, 0mm, 0mm)",
    );
}

#[test]
fn arc_compiles() {
    // geometry.md line 60 documents the 9-arg
    // `arc(cx, cy, cz, radius, start_angle, end_angle, ax, ay, az)` form
    // (geometry_curve.rs:47): center + radius + angle range + axis direction.
    assert_compiles(
        "arc",
        "arc(0mm, 0mm, 0mm, 5mm, 0deg, 90deg, 0mm, 0mm, 1mm)",
    );
}

#[test]
fn polygon_compiles() {
    // geometry.md line 61 (prelude block) and line 87 (2D-profiles block)
    // document the variadic flat coordinate-pairs form
    // `polygon(x1, y1, x2, y2, ...)` (>= 6 args, even count; geometry.rs:1570).
    assert_compiles("polygon", "polygon(0mm, 0mm, 10mm, 0mm, 5mm, 10mm)");
}

// --- Interference & clearance oracle: chunk <-> compiler-registry guard ---
//
// Task 5389. The five static interference/clearance query names were entirely
// absent from every `reify_language_reference` chunk, so the in-GUI assistant
// (which retrieves only those chunks) concluded "there is no interference
// oracle" and hand-rolled bbox arithmetic instead. The knowledge existed in the
// examples corpus (`examples/best_practices/clearance_oracle.ri`,
// `examples/tolerancing/vc_bolt_pattern_clearance.ri`) but was unreachable from
// the chunks.
//
// This guard is BIDIRECTIONAL, and neither half is sufficient alone:
//
//   (a) COVERAGE — geometry.md still documents all five names as call forms.
//       Catches a doc regression that silently reopens the discoverability hole.
//   (b) REGISTRY TRUTH — each documented name is a live member of the compiler
//       registry that gives it its meaning, so a rename in `units.rs` fails HERE
//       rather than leaving the chunk pointing at a phantom builtin.
//
// Per the house rule stated in `stdlib_chunk_geometry_ops_smoke.rs`'s module
// doc ("a name-existence check against code registries, deliberately NOT a
// wording/content pin on the chunk's prose") nothing below asserts on prose,
// headings, or ordering — the one exception being the section HEADING itself,
// which is load-bearing: it is how the coverage scan is scoped (see
// `ORACLE_SECTION`).

/// The chunk this file owns. Read (never written). If the chunk moves, this
/// const must move with it — the failure mode is a loud `expect` on the read,
/// not a silent skip. Mirrors the `CHUNK_PATH` const in
/// `stdlib_chunk_geometry_ops_smoke.rs`.
const CHUNK_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reify-mcp/src/tools/chunks/geometry.md"
);

/// Heading of the chunk section the coverage scan is scoped to.
///
/// Scoping matters: without it the scan is satisfied by ANY backticked mention
/// of a name anywhere in the 200-line chunk, so deleting the whole oracle
/// section would still pass on incidental hits elsewhere. The regression this
/// guards (the in-GUI assistant not discovering the oracle at all) is about the
/// SECTION existing, so the section is what gets scanned. Mirrors
/// `stdlib_chunk_geometry_ops_smoke.rs`'s `CHUNK_SECTION`.
///
/// Matched by NORMALISED WORDS, not byte equality — see [`heading_matches`]. This
/// is a discoverability guard, so retitling the section (reordering the two nouns,
/// changing `&` to `and`, changing case) must not fail it.
const ORACLE_SECTION: &str = "## Interference & Clearance Queries";

fn read_chunk() -> String {
    std::fs::read_to_string(CHUNK_PATH).unwrap_or_else(|e| {
        panic!("{CHUNK_PATH} must be readable ({e}) — update CHUNK_PATH if the chunk moved")
    })
}

/// Significant words of a markdown heading: lowercased, `#` markers stripped,
/// split on every non-alphanumeric run. `"## Interference & Clearance Queries"` →
/// `["interference", "clearance", "queries"]`.
fn heading_words(line: &str) -> Vec<String> {
    line.trim()
        .trim_start_matches('#')
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

/// Does `line` name the section `heading` refers to?
///
/// NORMALISED-WORD CONTAINMENT, deliberately not byte equality: every word of
/// `heading` must appear somewhere in `line`, in any order, ignoring case and
/// punctuation. So `"## Clearance & Interference Queries"` and
/// `"## Interference and Clearance Queries"` both still match.
///
/// The reason is that this file's subject is DISCOVERABILITY — whether the chunk
/// documents the oracle at all — never the chunk's wording. The sibling
/// `stdlib_chunk_geometry_ops_smoke.rs` states the house rule: these are name-
/// existence checks against code registries, "deliberately NOT a wording/content
/// pin on the chunk's prose". Byte equality would break that: a purely cosmetic
/// retitle would go RED and the panic would then tell the author the oracle is
/// undocumented when it plainly is.
///
/// Residual sensitivity, stated so it is not mistaken for robustness: DROPPING a
/// word (retitling to `"## Interference & Clearance"`) still fails, because
/// containment cannot distinguish that from the section being gone. The panic
/// message names `ORACLE_SECTION` as the knob for exactly that case.
fn heading_matches(line: &str, heading: &str) -> bool {
    let wanted = heading_words(heading);
    let present = heading_words(line);
    !wanted.is_empty() && wanted.iter().all(|word| present.contains(word))
}

/// The body of the `## `-level section headed by `heading`, from that heading
/// to the next `## ` heading (exclusive). `### ` subsections stay inside.
///
/// FENCE-AWARE: a `## ` line inside a ``` ``` ``` code fence is content, not a
/// section boundary. Without this the scan would truncate the section at the
/// first fenced comment line that happens to start with `## ` — the oracle
/// section's own ```` ```reify ```` fences are exactly where such a line would
/// appear, so the fragility is live, not theoretical.
///
/// PANICS if the heading is absent — that is the anti-vacuity guard, and it is
/// the failure a reader of a renamed/removed section should see, rather than an
/// empty slice that makes every downstream assertion pass trivially.
fn section_body(markdown: &str, heading: &str) -> String {
    let mut body: Vec<&str> = Vec::new();
    let mut in_section = false;
    let mut in_fence = false;

    for line in markdown.lines() {
        // Any ```-prefixed line toggles fence state (opening tags carry a
        // language, closing tags do not — both are just toggles here).
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            if in_section {
                body.push(line);
            }
            continue;
        }
        if !in_fence && line.starts_with("## ") {
            if in_section {
                break;
            }
            in_section = heading_matches(line, heading);
            continue;
        }
        if in_section {
            body.push(line);
        }
    }

    assert!(
        in_section,
        "{CHUNK_PATH} has no `{heading}` section — it was removed or renamed. That section is \
         what the in-GUI assistant retrieves when a designer asks about interference or \
         clearance; without it the assistant reads the oracle as a MISSING CAPABILITY and \
         hand-rolls bbox arithmetic instead (task 5389). Restore the section, or update \
         ORACLE_SECTION if it was deliberately retitled."
    );
    body.join("\n")
}

/// FORM A oracle names — dispatched through the kinematic-query post-process
/// and taking `(Snapshot, ...)` arguments. `is_geometry_kinematic_query` is a
/// bare `.contains()` over `GEOMETRY_KINEMATIC_QUERY_NAMES`, so membership in
/// that slice IS the compiler's recognition semantics.
///
/// EXHAUSTIVE, not a subset: this list must equal
/// `GEOMETRY_KINEMATIC_QUERY_NAMES` entry-for-entry, and
/// `documented_oracle_list_covers_the_whole_kinematic_registry` enforces that.
/// The whole kinematic family is interference/clearance, so any member of it
/// that the chunk does not document reopens exactly the discoverability hole
/// task 5389 closed.
const KINEMATIC_ORACLE_NAMES: &[&str] = &["interferes", "interferes_with", "min_clearance"];

/// FORM B oracle names — plain geometry queries over let-bound geometry
/// operands. `is_geometry_query` is likewise a bare `.contains()` over
/// `GEOMETRY_QUERY_NAMES`.
///
/// A DELIBERATE SUBSET, unlike [`KINEMATIC_ORACLE_NAMES`], and therefore NOT
/// set-equality-checked. `GEOMETRY_QUERY_NAMES` is a much larger and
/// heterogeneous family (`volume`, `area`, `centroid`, `bounding_box`,
/// `normal`, `feature`, …); only these two answer an interference/clearance
/// question, so only these two belong in the oracle section. Names outside this
/// pair are documented elsewhere in the chunk corpus and are the sibling
/// `stdlib_chunk_geometry_ops_smoke.rs`'s coverage concern, not this file's.
const GEOMETRY_ORACLE_NAMES: &[&str] = &["intersects", "distance"];

/// A kinematic query added to the compiler but never documented must be RED.
///
/// The bidirectional guard below covers doc → registry (no phantom names) and
/// documented-name → chunk (no silent doc deletion), but neither direction sees
/// a FOURTH kinematic query landing in `units.rs` and never reaching the chunk —
/// which is the discoverability regression task 5389 exists to prevent, arriving
/// from the compiler side instead of the doc side. Set equality closes it: a new
/// registry entry fails here until it is added to `KINEMATIC_ORACLE_NAMES`, and
/// adding it there immediately fails the coverage half until the chunk documents
/// it. Mirrors the sibling suite's
/// `every_implemented_geometry_op_is_documented_in_a_chunk`.
///
/// SET equality, not sequence equality — both sides are sorted before comparing.
/// Order in `GEOMETRY_KINEMATIC_QUERY_NAMES` carries no meaning
/// (`is_geometry_kinematic_query` is a bare `.contains()` over it), so a no-op
/// reordering there — alphabetising it, say — must not fail a test whose whole
/// subject is which names EXIST. Order-sensitivity here would be a false
/// positive whose message actively misdirects, telling the reader to add or
/// document a name when nothing was added or removed.
#[test]
fn documented_oracle_list_covers_the_whole_kinematic_registry() {
    let mut documented = KINEMATIC_ORACLE_NAMES.to_vec();
    let mut registry = reify_compiler::GEOMETRY_KINEMATIC_QUERY_NAMES.to_vec();
    documented.sort_unstable();
    registry.sort_unstable();
    assert_eq!(
        documented, registry,
        "KINEMATIC_ORACLE_NAMES must list the kinematic-query registry exactly (as a SET — both \
         sides are sorted here, so a pure reordering is fine). A query was added to (or removed \
         from) reify_compiler::GEOMETRY_KINEMATIC_QUERY_NAMES without updating this list — so \
         {CHUNK_PATH}'s `{ORACLE_SECTION}` section is not required to document it, and the in-GUI \
         assistant will keep reading it as a MISSING CAPABILITY (task 5389). Add the name here \
         AND document its call form in the chunk."
    );
}

#[test]
fn interference_oracle_names_documented_in_geometry_chunk() {
    let markdown = read_chunk();
    // Scoped to the oracle section, NOT the whole file: a backticked `distance(`
    // elsewhere in the chunk (or an incidental mention that survives the
    // section's deletion) must not satisfy this. `section_body` panics if the
    // section is gone, so gutting it is RED rather than vacuously green.
    let section = section_body(&markdown, ORACLE_SECTION);

    // (a) COVERAGE. Each name must appear as a CALL form (`name(`) rather than a
    // bare word — geometry.md already contained the word "distance" before task
    // 5389, but only as the unrelated `extrude(profile, distance)` parameter
    // name, which taught a reader nothing about the query. The open paren is what
    // excludes that false positive (`extrude(profile, distance)` has no
    // `distance(` in it), and it is all this needle asks for.
    //
    // NO leading backtick is required, deliberately. Demanding one would pin doc
    // TYPOGRAPHY: writing the same call form as `**`min_clearance`**(s, …)`, or
    // moving it into a markdown table cell, would go RED with zero capability
    // regression and a panic claiming the oracle is undocumented. Per the house
    // rule this file inherits, prose formatting is not the subject. The real
    // weight is carried by (b) below, by `section_body`'s anti-vacuity panic, and
    // by `reify_tagged_fences_in_geometry_chunk_compile`'s per-name sentinels,
    // which require each call form inside a COMPILING fence — a strictly stronger
    // property than any string match here.
    //
    // (Placement WITHIN the section is not further constrained: a name mentioned
    // only in the traps subsection would still satisfy this. Section presence is
    // the property under test.)
    for name in KINEMATIC_ORACLE_NAMES.iter().chain(GEOMETRY_ORACLE_NAMES) {
        let call_form = format!("{name}(");
        assert!(
            section.contains(&call_form),
            "{CHUNK_PATH}'s `{ORACLE_SECTION}` section does not document the \
             interference/clearance query `{name}` as a call form ({call_form}...). The chunk \
             is what the in-GUI assistant retrieves, so an undocumented oracle reads to it as a \
             MISSING CAPABILITY and it will hand-roll bbox arithmetic instead (task 5389). \
             Re-add the call form, or delete this name from \
             KINEMATIC_ORACLE_NAMES/GEOMETRY_ORACLE_NAMES if the builtin itself is gone."
        );
    }

    // (b) REGISTRY TRUTH, kinematic trio.
    for name in KINEMATIC_ORACLE_NAMES {
        assert!(
            reify_compiler::GEOMETRY_KINEMATIC_QUERY_NAMES.contains(name),
            "`{name}` is documented in {CHUNK_PATH} as a kinematic interference/clearance \
             query but is NOT in reify_compiler::GEOMETRY_KINEMATIC_QUERY_NAMES ({:?}) — the \
             chunk is served verbatim to the assistant, so it now documents a phantom \
             builtin. Rename the doc's call form to match the registry.",
            reify_compiler::GEOMETRY_KINEMATIC_QUERY_NAMES
        );
    }

    // (b) REGISTRY TRUTH, plain-geometry pair.
    for name in GEOMETRY_ORACLE_NAMES {
        assert!(
            reify_compiler::GEOMETRY_QUERY_NAMES.contains(name),
            "`{name}` is documented in {CHUNK_PATH} as a geometry query but is NOT in \
             reify_compiler::GEOMETRY_QUERY_NAMES ({:?}) — the chunk now documents a phantom \
             builtin. Rename the doc's call form to match the registry.",
            reify_compiler::GEOMETRY_QUERY_NAMES
        );
    }
}

/// Every ```` ```reify ````-tagged fence in the chunk, in document order, with
/// the fence delimiters stripped.
///
/// Only EXPLICITLY TAGGED fences are collected. The module doc above explains
/// why geometry.md cannot be scraped wholesale: it intermixes non-compilable
/// schematic notation (type params, trait lists, `-> Solid` return annotations)
/// with real call forms, and separating the two would need a fragile grammar.
/// An opt-in tag sidesteps that — the doc author marks exactly what is meant to
/// compile, and everything else stays free-form. ```` ```reify ```` is already
/// the in-repo convention (every fence in `chunks/traits.md` is tagged that
/// way).
///
/// Callers must anti-vacuity-check the result: a dropped tag or a renamed
/// section would otherwise empty the scan and pass trivially.
fn reify_tagged_fences(markdown: &str) -> Vec<String> {
    let mut fences: Vec<String> = Vec::new();
    let mut body: Vec<&str> = Vec::new();
    let mut open = false;

    for line in markdown.lines() {
        if !open {
            // Exact tag match: `reify-something` is a different language and
            // must not be swept in.
            if line.trim_end() == "```reify" {
                open = true;
                body.clear();
            }
            continue;
        }
        if line.trim_end() == "```" {
            fences.push(body.join("\n"));
            open = false;
            continue;
        }
        body.push(line);
    }

    assert!(
        !open,
        "{CHUNK_PATH} has an unterminated ```reify fence — the scrape cannot be trusted"
    );
    fences
}

/// Every ```` ```reify ````-tagged fence in geometry.md must actually compile.
///
/// This upgrades the interference/clearance worked examples from unchecked prose
/// into artifacts that at least parse and lower: a designer copying a fence out
/// of the chunk gets something the compiler accepts, or this test is RED.
///
/// SCOPE — this is a parse/compile-acceptance guard, NOT a signature pin. The
/// module doc's "What is NOT established" is the canonical statement of what
/// that does and does not buy (arity, argument dimension, unknown call names);
/// read it before relying on this test. It is deliberately not restated here,
/// so the claims cannot drift apart.
#[test]
fn reify_tagged_fences_in_geometry_chunk_compile() {
    let markdown = read_chunk();
    let fences = reify_tagged_fences(&markdown);

    // Anti-vacuity. Without these, dropping the ```reify tags (or
    // rewriting the fences as plain prose) would leave the scan empty and the
    // loop below would iterate zero times — GREEN, protecting nothing. The
    // sentinels additionally prove the scan reaches BOTH documented forms, not
    // just whichever fence happens to come first.
    assert!(
        fences.len() >= 2,
        "the ```reify fence scan found only {} fence(s) in {CHUNK_PATH} — expected at least 2 \
         (one FORM B raw-geometry example, one FORM A mechanism-snapshot example). The scan is \
         vacuous (fence tags dropped, or the examples rewritten as untagged prose) and gives NO \
         protection.",
        fences.len()
    );
    // `distance(` is included even though the module doc singles it out as the
    // only name with (indirect) discriminating power — precisely BECAUSE of
    // that: without a sentinel the FORM B fence could lose its `distance(` call
    // and the scrape would still pass, quietly retiring the one claim
    // `bogus_query_name_feeding_a_comparison_is_an_error` is the control for.
    for sentinel in ["min_clearance(", "intersects(", "distance("] {
        assert!(
            fences.iter().any(|fence| fence.contains(sentinel)),
            "anti-vacuity: no ```reify fence in {CHUNK_PATH} contains `{sentinel}` — the worked \
             clearance examples are no longer compile-verified, so a documented call form the \
             compiler outright rejects would ship unnoticed"
        );
    }

    for (index, fence) in fences.iter().enumerate() {
        assert_module_compiles(
            &format!("{CHUNK_PATH} ```reify fence #{}", index + 1),
            fence,
        );
    }
}

/// NEGATIVE CONTROL for the fence guard — pins the part of it that discriminates.
///
/// The guard's power over call NAMES is real but narrow and entirely indirect,
/// and this test exists so that narrow part cannot rot away silently. A
/// `structure def` body accepts an unresolved function with no diagnostic at any
/// severity, typing the call from its first argument's `result_type`. So a
/// bogus `distanceZZZ(housing, bracket)` types as `Geometry`, and it is the
/// DOWNSTREAM `constraint … > 1mm` that fails with `CmpOperandKind` — not name
/// resolution.
///
/// The corollary is the hole documented in the module doc: the same typo on a
/// name whose result only feeds `not` (`intersectsZZZ`) produces zero errors.
/// Only the positive direction is asserted here; asserting the hole would go RED
/// the day someone fixes it, which is the wrong incentive.
#[test]
fn bogus_query_name_feeding_a_comparison_is_an_error() {
    // Same shape as the chunk's FORM B fence, inline so it is stable against
    // ordinary doc edits (mirrors the inline-repro posture of
    // `bogus_geometry_op_names_are_reported_as_unrecognised` in
    // `stdlib_chunk_geometry_ops_smoke.rs`).
    let source = r#"structure def BogusClearanceGate {
    let housing = cylinder_centered(10mm, 40mm)
    let bracket = translate(box(20mm, 20mm, 20mm), 30mm, 0mm, 0mm)

    let gap = distanceZZZ(housing, bracket)

    constraint gap > 1mm
}"#;

    let compiled = compile_source_with_stdlib(source);
    let errors = errors_only(&compiled);

    // Assert the diagnostic IDENTITY, not mere presence. A bare
    // `!errors.is_empty()` is satisfied by any unrelated Error — a future
    // `cylinder_centered` signature change, a `translate` arity change, a typo
    // introduced while editing this inline source — so the control would keep
    // reading as live while no longer pinning the mechanism it names. The
    // mechanism is specifically: the misspelled call is NOT itself resolved as
    // an error; it types as `Geometry` from its first argument, and the
    // DOWNSTREAM `constraint gap > 1mm` is what rejects it.
    assert!(
        errors
            .iter()
            .any(|d| d.code == Some(reify_core::DiagnosticCode::CmpOperandKind)),
        "a fence-shaped source whose clearance query is misspelled must be rejected by the \
         downstream comparison with DiagnosticCode::CmpOperandKind. Got these Error \
         diagnostics instead: {:#?}\n\
         If the list is EMPTY, `reify_tagged_fences_in_geometry_chunk_compile` has lost even \
         its indirect discriminating power over query names and is a pure parse check. If it \
         is non-empty but carries a different code, the rejection mechanism moved (e.g. \
         unresolved names became an error in their own right) — either way, re-verify what the \
         fence guard still establishes and update the module doc's \"What is NOT established\" \
         section.",
        errors
    );
}
