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
//! THE CANONICAL SCOPE STATEMENT FOR THIS FILE — the test docstrings point back
//! here rather than restating it, so there is exactly one place to correct.
//!
//! - **The compiler checks nothing about the five oracle names.** None has an
//!   arg-slot entry in the private `builtin_signatures` table, so neither arity
//!   nor argument dimension is rejected; and an unknown call NAME is not itself
//!   an error, because a `structure def` body types an unresolved call from its
//!   FIRST argument's `result_type` (the same permissive fallback noted at the
//!   `line_segment`/`arc`/`polygon` block below). Compile-acceptance of a fence
//!   is therefore a parse/shape result, not a signature check.
//! - **Arity is pinned anyway — by cross-check, not by the compiler.**
//!   `documented_oracle_arities_are_exercised_by_a_compiling_fence` requires
//!   every documented `name(…) -> Type` signature to be matched by a fence call
//!   at the SAME arity, so a doc-side arity edit that the fences do not mirror is
//!   RED here even though `min_clearance(s)` compiles clean. Argument DIMENSION
//!   stays unchecked in both directions. (Adopted from the sibling suite's
//!   `every_documented_geometry_op_form_is_exercised_by_the_fixture`.)
//! - **The fence guard's residual power over call NAMES is indirect and narrow.**
//!   Mutating `distance(` → `distanceZZZ(` is caught only downstream, by the
//!   fence's own `constraint gap > 1mm` (`CmpOperandKind`); the same typo on a
//!   name whose result merely feeds `not` produces no error at all.
//!   `bogus_query_name_feeding_a_comparison_is_an_error` is the negative control
//!   pinning the half that works, so it cannot rot away silently.
//!
//! The registry-membership assertions in
//! `interference_oracle_names_documented_in_geometry_chunk` are what close the
//! phantom-NAME direction; the fence compile is a parse/shape guard.
//!
//! # The one doc-FORMAT pin this file does impose
//!
//! Stated here rather than left implicit, because it cuts against the house rule
//! the coverage check in `interference_oracle_names_documented_in_geometry_chunk`
//! invokes (that comment declines to require a leading backtick precisely so the
//! call form may be rewrapped, bolded, or tabulated freely).
//!
//! `documented_oracle_arities_are_exercised_by_a_compiling_fence` requires each of
//! the five oracle names to carry a literal `-> <Type>` immediately after a
//! balanced call form somewhere in the section. **`-> <Type>` after the call form
//! is therefore a PINNED notation for those five names, and a markdown table with
//! the return type in its own column (`| min_clearance(s, id_a, id_b) | Length |`)
//! is RED even though no capability regressed.** That is a deliberate trade, not
//! an oversight: the `->` is the ONLY thing separating a documented SIGNATURE from
//! the traps subsection's prose mentions of unsupported forms
//! (`min_clearance(a, b)`, `min_clearance(s, id, id)`), which must not be held to
//! the fences. Widening the scan to accept a table cell would re-admit those. If
//! the section is ever tabulated, widen `documented_signature_arities` in the same
//! commit — and re-check that the trap prose still reads as prose.
//!
//! # Known duplication
//!
//! THREE chunk-scraping scanners now live in this one test binary — this module,
//! `stdlib_chunk_geometry_ops_smoke.rs`, and `enums_chunk_option_smoke.rs` — and
//! no two of them agree on what a "section" or a "fence" is. Extracting a shared
//! `chunk_io` needs edits to `tests/harness_doc_chunks.rs` and to both siblings,
//! none of which is in task 5389's locked file set.
//!
//! STILL THREE, not four: task 5759 added `units_chunk_smoke.rs` and pointed it
//! at THIS module's scanners (`reify_tagged_fences`, `assert_module_compiles`,
//! `strip_reify_comments`, `call_sites`, `section_body`, `cited_source_paths`,
//! `resolve_cited_path`, `source_files_by_basename`, all raised to
//! `pub(crate)`) rather than copying them. That is why those helpers now take
//! `chunk_path` / `tag` / `section_title` parameters instead of reading this
//! module's consts — a sibling's failure must name the sibling's chunk. The
//! extraction below is still owed; this is reuse inside the existing binary, not
//! the shared module.
//!
//! Task **#5924** (filed as ticket `tkt_0RS9A7843SBQ4BZX1A2ACY5TC1`) owns the
//! extraction AND the axis-by-axis reconciliation contract — which heading /
//! fence-delimiter / info-string / section-end / chunk-read behaviour the shared
//! module should adopt, and which of this file's hand-rolled scanners the
//! sibling's AST walk subsumes outright. That contract deliberately lives THERE,
//! where it is the extraction author's working document, and not here: a table
//! in this file describing two OTHER files' internals is unenforced prose that
//! goes silently wrong the moment either sibling changes — the same rot mode the
//! rest of this module spends 200 lines closing for the chunk's SYNC blocks.
//!
//! Delete this section when the extraction lands.

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
///
/// `chunk_path` NAMES THE CHUNK THE SOURCE CAME FROM and is threaded rather than
/// read off this module's `CHUNK_PATH` const, because sibling chunk modules in
/// this same harness binary call this helper for THEIR chunk (task 5759 raised
/// it to `pub(crate)` for `units_chunk_smoke.rs`). A hardcoded const would name
/// geometry.md in a units.md failure — a panic that sends the reader to the
/// wrong file.
pub(crate) fn assert_module_compiles(chunk_path: &str, label: &str, module_src: &str) {
    let compiled = compile_source_with_stdlib(module_src);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "{chunk_path} — {label}: expected this module to compile with zero Error diagnostics, \
         got: {:#?}\n--- module source ---\n{module_src}\n--- end module source ---",
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
        CHUNK_PATH,
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
// headings, or ordering. That rule has NO carve-out here: the coverage scan is
// scoped by a dedicated HTML-comment marker the chunk carries for this purpose
// (see `ORACLE_SECTION_MARKER`), never by the section's title, so retitling
// `## Interference & Clearance Queries` is free.

/// The chunk this file owns. Read (never written). If the chunk moves, this
/// const must move with it — the failure mode is a loud `expect` on the read,
/// not a silent skip. Mirrors the `CHUNK_PATH` const in
/// `stdlib_chunk_geometry_ops_smoke.rs`.
const CHUNK_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reify-mcp/src/tools/chunks/geometry.md"
);

/// Marker that OPENS the chunk section the coverage scan is scoped to. Matched
/// BYTE-EXACTLY against the trimmed line; the chunk carries it directly under
/// the section heading for exactly this purpose.
///
/// Scoping matters: without it the scan is satisfied by ANY backticked mention
/// of a name anywhere in the 280-line chunk, so deleting the whole oracle
/// section would still pass on incidental hits elsewhere. The regression this
/// guards (the in-GUI assistant not discovering the oracle at all) is about the
/// SECTION existing, so the section is what gets scanned.
///
/// A MARKER RATHER THAN THE HEADING TEXT, deliberately. Scoping by the heading —
/// what `stdlib_chunk_geometry_ops_smoke.rs`'s `CHUNK_SECTION` still does — makes
/// the scan a wording pin on shipped prose: `&` → `and`, reordering the nouns, or
/// dropping a word all go RED with a panic claiming the oracle is undocumented
/// when it plainly is. That is the one thing the house rule in this file's
/// preamble forbids. An inert HTML comment costs the chunk one line, is invisible
/// in rendered markdown, and leaves the title free to change.
const ORACLE_SECTION_MARKER: &str = "<!-- ORACLE-SECTION -->";

/// Human-readable name of the marked section. Used ONLY in panic text, so a
/// reader is told which part of the chunk to look at; nothing matches on it.
/// A retitle may update this for legibility but need not — no test reads it.
const ORACLE_SECTION_TITLE: &str = "## Interference & Clearance Queries";

/// Marker that OPENS the section cataloguing WHICH ARGUMENT of which geometry
/// constructor is length-semantic. Matched BYTE-EXACTLY on the trimmed line,
/// exactly as [`ORACLE_SECTION_MARKER`] is, and for the identical reason: the
/// scan must be anchored to something inert so the heading's wording stays free.
///
/// Scoping matters here for a second reason too. This chunk mentions geometry
/// call forms everywhere — the primitives block, the anchoring table, the oracle
/// fences — so an UNSCOPED name scan would be satisfied by any of them and would
/// say nothing about whether the length-argument catalogue still exists. The
/// catalogue is what an author consults before dimensioning an unfamiliar
/// signature, so the catalogue is what gets scanned.
const LENGTH_ARGS_SECTION_MARKER: &str = "<!-- LENGTH-ARGS-SECTION -->";

/// Human-readable name of [`LENGTH_ARGS_SECTION_MARKER`]'s section. Panic text
/// only; nothing matches on it.
const LENGTH_ARGS_SECTION_TITLE: &str = "### Dimensioned arguments";

/// Marker that OPENS the MEASUREMENT / mass-property section — the one that
/// documents [`reify_compiler::GEOMETRY_QUERY_NAMES`] as call forms. Matched
/// BYTE-EXACTLY on the trimmed line, exactly as [`ORACLE_SECTION_MARKER`] is,
/// and for the identical reason: the anchor must be inert so the heading's
/// wording stays free to change.
///
/// Scoping matters here more than anywhere else in this file, because the names
/// in this family are the ones a chunk-wide scan cannot distinguish. `volume`
/// and `area` are ordinary English words that already appeared in this repo's
/// chunks as HAND-COMPUTED parameter arithmetic (`structures.md`'s
/// `let volume = thickness * width * width`), and `contains` is also the
/// `List`/`Set`/`Range` method documented in the `collections` chunk. An
/// unscoped word scan is satisfied by every one of those while teaching a reader
/// nothing about the kernel query — which is precisely the misdirection task
/// 5581 exists to remove, so the SECTION is what gets scanned.
const MEASUREMENT_SECTION_MARKER: &str = "<!-- MEASUREMENT-SECTION -->";

/// Human-readable name of [`MEASUREMENT_SECTION_MARKER`]'s section. Panic text
/// only; nothing matches on it.
const MEASUREMENT_SECTION_TITLE: &str = "## Measurement & Mass-Property Queries";

/// Marker that OPENS the TOPOLOGY-SELECTOR catalogue section — the one that
/// documents [`reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES`] as a table.
/// Matched BYTE-EXACTLY on the trimmed line, as every other marker here is.
///
/// Scoping is what makes the scan mean anything for THIS family in particular:
/// `edges` and `faces` already appear all over this chunk as the fillet / chamfer
/// / shell ARGUMENT name (`fillet(solid, edges, radius)`), where they say nothing
/// about the selector that produces such a list. The catalogue is the one place
/// they are documented as callable selectors, so the catalogue is what is
/// scanned.
const TOPOLOGY_SECTION_MARKER: &str = "<!-- TOPOLOGY-SECTION -->";

/// Human-readable name of [`TOPOLOGY_SECTION_MARKER`]'s section. Panic text
/// only; nothing matches on it.
const TOPOLOGY_SECTION_TITLE: &str = "## Topology Selectors";

fn read_chunk() -> String {
    std::fs::read_to_string(CHUNK_PATH).unwrap_or_else(|e| {
        panic!("{CHUNK_PATH} must be readable ({e}) — update CHUNK_PATH if the chunk moved")
    })
}

/// Length of the leading run of backticks on `line`, counted from COLUMN 0.
///
/// Zero for an indented fence, deliberately: `reify_tagged_fences` below matches
/// its delimiters at column 0 too, so an indented fence must be invisible to
/// both scanners rather than to only one of them.
fn leading_backtick_run(line: &str) -> usize {
    line.chars().take_while(|&c| c == '`').count()
}

/// The body of the section opened by `marker`, from the marker line to the next
/// `## ` heading (exclusive). `### ` subsections stay inside.
///
/// FENCE-AWARE: a `## ` line inside a ``` ``` ``` code fence is content, not a
/// section boundary. Without this the scan would truncate the section at the
/// first fenced comment line that happens to start with `## ` — the oracle
/// section's own ```` ```reify ```` fences are exactly where such a line would
/// appear, so the fragility is live, not theoretical.
///
/// FENCES ARE PAIRED BY DELIMITER RUN LENGTH, not toggled. A toggle flips on
/// every column-0 ```-prefixed line, so a 4-backtick fence that DISPLAYS a
/// 3-backtick one — a routine shape in docs about markdown — inverts the state
/// for the rest of the document and makes the marker line unreachable. The
/// visible symptom would be the anti-vacuity panic below blaming a marker that
/// is present and untouched, which sends the reader to the wrong line entirely.
/// CommonMark's rule is used: a fence opened by a run of N closes on a line that
/// is a bare run of at least N and nothing else.
///
/// PANICS in two cases, each naming ITS OWN cause: an unterminated fence (which
/// swallows the rest of the chunk, so it is reported before the marker is
/// blamed), and an absent marker — the anti-vacuity guard, and the failure a
/// reader of a gutted section should see, rather than an empty slice that makes
/// every downstream assertion pass trivially.
/// `chunk_path` and `section_title` are THREADED rather than read off this
/// module's consts, for the same reason [`assert_module_compiles`] threads its
/// path (task 5759): this helper now scopes more than one marked section — the
/// oracle section AND the length-arguments section — and a sibling chunk module
/// may scope its own. A hardcoded `ORACLE_SECTION_TITLE` would make a
/// length-section failure panic about interference queries.
pub(crate) fn section_body(
    markdown: &str,
    marker: &str,
    chunk_path: &str,
    section_title: &str,
) -> String {
    let mut body: Vec<&str> = Vec::new();
    let mut in_section = false;
    // `Some(n)` while inside a fence opened by a column-0 run of `n` backticks.
    let mut fence: Option<usize> = None;

    for line in markdown.lines() {
        let run = leading_backtick_run(line);

        if let Some(open_run) = fence {
            // Bare run of >= the opening length closes; anything else (a shorter
            // run, or a run carrying an info string) is fence CONTENT.
            if run >= open_run && line.trim_end().len() == run {
                fence = None;
            }
            if in_section {
                body.push(line);
            }
            continue;
        }
        if run >= 3 {
            fence = Some(run);
            if in_section {
                body.push(line);
            }
            continue;
        }
        if !in_section && line.trim() == marker {
            in_section = true;
            continue;
        }
        if in_section && line.starts_with("## ") {
            break;
        }
        if in_section {
            body.push(line);
        }
    }

    // Checked FIRST, and named for what it is. An unterminated fence makes every
    // later line read as fence content, so the marker assertion below would fire
    // with a cause that is not the real one.
    assert!(
        fence.is_none(),
        "{chunk_path} has an unterminated code fence: a column-0 run of {} backtick(s) is never \
         closed by a bare run of at least that many. Everything after it is being read as fence \
         content, so the `{marker}` scan cannot reach the section even when the marker line is \
         present and intact. Close the fence — do not touch the marker.",
        fence.unwrap_or(0)
    );

    assert!(
        in_section,
        "{chunk_path} carries no `{marker}` marker — the line that opens the \
         `{section_title}` section was removed along with (or independently of) the \
         section itself. That section is what the in-GUI assistant retrieves when a designer \
         asks about the topic it covers; without it the assistant reads the capability as \
         MISSING and hand-rolls a substitute instead (task 5389, for the oracle section). \
         Restore the section WITH its marker line directly under the heading. Retitling the \
         heading is free and needs no change here — only the marker is matched."
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
         {CHUNK_PATH}'s `{ORACLE_SECTION_TITLE}` section is not required to document it, and the in-GUI \
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
    let section = section_body(
        &markdown,
        ORACLE_SECTION_MARKER,
        CHUNK_PATH,
        ORACLE_SECTION_TITLE,
    );

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
    // rule this file inherits, prose formatting is not the subject. (The ONE
    // exception, `-> <Type>`, is stated in the module doc's "The one doc-FORMAT
    // pin this file does impose" — read it before tabulating this section.) The real
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
            "{CHUNK_PATH}'s `{ORACLE_SECTION_TITLE}` section does not document the \
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

/// Every member of the geometry-QUERY registry must be documented as a call form
/// in the chunk's measurement section.
///
/// THE REGISTRY IS ITERATED DIRECTLY, not mirrored into a local list plus a
/// set-equality guard the way [`KINEMATIC_ORACLE_NAMES`] is. That two-test shape
/// exists so a failure can distinguish "the registry grew" from "the doc
/// shrank", and at three names it is cheap. At fifteen — and at the thirty-one of
/// `topology_selector_family_documented_in_geometry_chunk` — the mirror becomes
/// its own drift surface: a hand-copied list that must be edited in lockstep with
/// the registry it claims to reproduce. Iterating the registry collapses both
/// halves into one check with strictly less to maintain, and keeps the
/// distinguishing power where it is actually read: the panic message below names
/// the offending registry member and both remedies.
///
/// A CALL FORM (`name(`) rather than a bare word, and that needle is doing real
/// work here rather than mirroring a convention. `volume`, `area`, `centroid` and
/// `contains` all already appear as bare words elsewhere in the chunk corpus —
/// `volume` as `structures.md`'s hand-computed `thickness * width * width`,
/// `contains` as the `List`/`Set`/`Range` method in the `collections` chunk — so
/// a word-boundary scan (which is exactly what the out-of-band PDOCCOVER detector
/// in `crates/reify-audit/src/pdoccover.rs` performs) reports them DOCUMENTED
/// while a reader learns nothing about the kernel query. The open paren is what
/// separates a query from a noun.
///
/// No leading backtick is required, for the reason
/// `interference_oracle_names_documented_in_geometry_chunk` states at length: the
/// house rule this file inherits forbids pinning doc TYPOGRAPHY. The one
/// exception — the `-> <Type>` notation — is imposed only on the whole-handle
/// four, by `documented_measurement_arities_are_exercised_by_a_compiling_fence`.
///
/// Anti-vacuity comes free from [`section_body`], which panics when its marker is
/// absent, so deleting the section is RED rather than silently green.
#[test]
fn measurement_query_family_documented_in_geometry_chunk() {
    let markdown = read_chunk();
    let section = section_body(
        &markdown,
        MEASUREMENT_SECTION_MARKER,
        CHUNK_PATH,
        MEASUREMENT_SECTION_TITLE,
    );

    for name in reify_compiler::GEOMETRY_QUERY_NAMES {
        let call_form = format!("{name}(");
        assert!(
            section.contains(&call_form),
            "{CHUNK_PATH}'s `{MEASUREMENT_SECTION_TITLE}` section does not document the geometry \
             query `{name}` as a call form ({call_form}...). The chunk is what the in-GUI \
             assistant retrieves, so an undocumented query reads to it as a MISSING CAPABILITY: \
             asked for a part's mass or its centre of area it will hand-compute the figure from \
             the parameters instead of asking the kernel, and silently return a number that no \
             longer describes the realized geometry once a feature is added (task 5581). Either \
             document the call form in that section, or — if the builtin itself is gone — remove \
             `{name}` from reify_compiler::GEOMETRY_QUERY_NAMES, which is iterated directly here \
             and is the sole source of this list."
        );
    }
}

/// Minimum CATALOGUE ROWS the TOPOLOGY-SELECTOR table must carry.
///
/// The EXACT live length of `reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES`
/// (31 today), not a round number under it. The coverage half of
/// [`topology_selector_family_documented_in_geometry_chunk`] cannot catch a
/// gutted table on its own — reformatting the table into prose bullets empties
/// [`catalogue_table_rows`] and the coverage loop then compares against an empty
/// set. The floor is what makes that RED.
///
/// A DERIVED value would be better and is deliberately not used: asserting
/// `rows.len() >= GEOMETRY_TOPOLOGY_SELECTOR_NAMES.len()` reads as tighter but is
/// strictly weaker as a floor, because a row documenting one selector under two
/// names (the shared-cell shape `catalogue_table_rows` supports) legitimately
/// makes rows FEWER than names. Keeping the number literal keeps the two claims
/// independent, which is the point of having both.
///
/// RE-MEASUREMENT PROTOCOL: see [`MINIMUM_FN_CITES`], which states it once for
/// every `MINIMUM_*` floor in this file. Raise this WITH the table; never lower
/// it to go green.
const MINIMUM_TOPOLOGY_CATALOGUE_ROWS: usize = 31;

/// Every member of the topology-selector registry must have a CATALOGUE ROW, and
/// every catalogue row must name a real selector.
///
/// BOTH DIRECTIONS, because each catches a different rot. Coverage (registry →
/// table) catches a selector landing in `units.rs` that the chunk never learns
/// about — the in-GUI assistant then cannot reach it and falls back to indexing
/// `faces(...)` by hand. Registry truth (table → registry) catches the failure
/// that has actually happened in this repo: the 2026-07-24 language review found
/// `rotate(geo, axis, angle)` and `translate(geo, vector)` documented at
/// signatures the compiler had never been shown (tasks #5347 / #5364). A phantom
/// selector is worse than a missing one, because the reader has no reason to
/// doubt it.
///
/// A TABLE rather than the measurement section's call-form prose, and that is a
/// scanner decision rather than a style one: [`catalogue_table_rows`] already
/// exists to read exactly this shape (it backs the length-argument catalogue),
/// and a 31-entry family rendered as prose is unreadable for the human as well as
/// unscannable for the test. The registry is iterated DIRECTLY here for the same
/// reason `measurement_query_family_documented_in_geometry_chunk` iterates its
/// own: at 31 names a hand-copied mirror is a bigger drift surface than the thing
/// it guards.
#[test]
fn topology_selector_family_documented_in_geometry_chunk() {
    let markdown = read_chunk();
    // Panics if the marker is gone, so deleting the section is RED rather than
    // vacuously green — the same anti-vacuity guarantee the oracle scan relies on.
    let section = section_body(
        &markdown,
        TOPOLOGY_SECTION_MARKER,
        CHUNK_PATH,
        TOPOLOGY_SECTION_TITLE,
    );

    let rows = catalogue_table_rows(&section);
    assert!(
        rows.len() >= MINIMUM_TOPOLOGY_CATALOGUE_ROWS,
        "only {} catalogue row(s) found in {CHUNK_PATH}'s `{TOPOLOGY_SECTION_TITLE}` table — \
         expected at least {MINIMUM_TOPOLOGY_CATALOGUE_ROWS}. Either rows were deleted, or the \
         table was reformatted into a shape this scan cannot read: a catalogue row is a \
         `|`-leading line whose FIRST cell backticks the selector it is about. Without the rows \
         the coverage check below compares against an empty set and protects nothing. Rows seen: \
         {rows:?}",
        rows.len()
    );

    let table_names: Vec<String> = {
        let mut out: Vec<String> = Vec::new();
        for row in &rows {
            for name in row {
                if !out.contains(name) {
                    out.push(name.clone());
                }
            }
        }
        out
    };

    // (a) COVERAGE — registry → table.
    for name in reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES {
        assert!(
            table_names.iter().any(|n| n == name),
            "{CHUNK_PATH}'s `{TOPOLOGY_SECTION_TITLE}` catalogue has no row for the topology \
             selector `{name}`. The chunk is what the in-GUI assistant retrieves, so a selector \
             missing from this table reads to it as a MISSING CAPABILITY: it will index \
             `faces(...)` positionally, or hand-roll a filter, instead of calling the selector \
             that exists (task 5581). Add a row whose FIRST cell backticks `{name}`, or — if the \
             builtin itself is gone — remove it from \
             reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES, which is iterated directly here \
             and is the sole source of this list. Names the table does carry: {table_names:?}"
        );
    }

    // (b) REGISTRY TRUTH — table → registry. A row naming something that is not
    // a selector sends an author to write a call the compiler will not accept,
    // and — because a `structure def` body types an unresolved call from its
    // first argument — often will not even say so.
    for name in &table_names {
        if reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(&name.as_str()) {
            continue;
        }
        // Split by WHY it is not a selector, so the panic names the actual
        // remedy. `phantom_name_panic` is the shared voice for "no registry has
        // this name at all"; a name that IS real but lives in a sibling family
        // is a different (and more confusing) mistake, and gets told so.
        match registry_family(name) {
            None => panic!(
                "{}",
                phantom_name_panic(
                    CHUNK_PATH,
                    "the topology-selector catalogue table's first column",
                    name
                )
            ),
            Some(family) => panic!(
                "{CHUNK_PATH}'s `{TOPOLOGY_SECTION_TITLE}` catalogue has a row for `{name}`, \
                 which is a real builtin but belongs to {family}, NOT \
                 GEOMETRY_TOPOLOGY_SELECTOR_NAMES. The families are disjoint by construction and \
                 dispatch differently, so documenting one here teaches a reader the wrong \
                 result type and the wrong resolution stage. Move the row to the section that \
                 owns {family}, or drop it."
            ),
        }
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
///
/// `tag` IS A PARAMETER, not the hardcoded `reify` this scanner started with
/// (task 5759). units.md carries a deliberately-INVALID rejected-forms block
/// tagged ```` ```reify-rejected ````, which a rejection-truth negative control
/// must scrape and a zero-Error compile gate must never sweep in. Parameterising
/// the tag lets both gates share this one scanner instead of the harness growing
/// its FIFTH near-identical scraper (see "Known duplication" above). Matching
/// stays BYTE-EXACT on the whole info string, so `reify` still excludes
/// `reify-rejected` in both directions.
///
/// `chunk_path` is threaded for the same reason [`assert_module_compiles`]
/// threads its own: a sibling chunk module's unterminated fence must be blamed
/// on ITS chunk, not on geometry.md.
pub(crate) fn reify_tagged_fences(markdown: &str, tag: &str, chunk_path: &str) -> Vec<String> {
    let opener = format!("```{tag}");
    let mut fences: Vec<String> = Vec::new();
    let mut body: Vec<&str> = Vec::new();
    let mut open = false;

    for line in markdown.lines() {
        if !open {
            // Exact tag match: `reify-something` is a different language and
            // must not be swept in.
            if line.trim_end() == opener {
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
        "{chunk_path} has an unterminated ```{tag} fence — the scrape cannot be trusted"
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
    let fences = reify_tagged_fences(&markdown, "reify", CHUNK_PATH);

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
    // ALL FIVE oracle names, so coverage is symmetric. Before task 5389's
    // amendment pass, `interferes(`/`interferes_with(` appeared only in the FORM A
    // bullet list, so their sole guard was a `section.contains(...)` string match —
    // which the module doc characterises as establishing essentially nothing. They
    // are now bound in the FORM A fence and sentinelled here like the rest.
    //
    // `distance(` is included even though the module doc singles it out as the
    // only name with (indirect) discriminating power — precisely BECAUSE of
    // that: without a sentinel the FORM B fence could lose its `distance(` call
    // and the scrape would still pass, quietly retiring the one claim
    // `bogus_query_name_feeding_a_comparison_is_an_error` is the control for.
    //
    // Scanned COMMENT-FREE. The FORM A fence's own `// MUST be let-bound.
    // Writing `constraint min_clearance(s, id_a, id_b) > 2mm`` annotation
    // otherwise satisfies the `min_clearance(` sentinel by itself, so deleting
    // the fence's real call would leave this green while the panic text below
    // still promised the form was compile-verified. See `strip_reify_comments`.
    let code: Vec<String> = fences.iter().map(|f| strip_reify_comments(f)).collect();
    for sentinel in [
        "min_clearance(",
        "interferes(",
        "interferes_with(",
        "intersects(",
        "distance(",
    ] {
        assert!(
            code.iter().any(|fence| fence.contains(sentinel)),
            "anti-vacuity: no ```reify fence in {CHUNK_PATH} contains `{sentinel}` OUTSIDE A \
             COMMENT — the worked clearance examples are no longer compile-verified, so a \
             documented call form the compiler outright rejects would ship unnoticed. (A call \
             form mentioned only in a fence's `//` annotation does not count; it is never \
             compiled.)"
        );
    }

    for (index, fence) in fences.iter().enumerate() {
        assert_module_compiles(CHUNK_PATH, &format!("```reify fence #{}", index + 1), fence);
    }
}

/// `src` with Reify comments removed. Every other byte — newlines included — is
/// left exactly where it was, so a stripped fence still reads like the original
/// in a panic message.
///
/// WHY THIS EXISTS. [`call_sites`] is a text scan, so without it a call form
/// written only in a `//` comment counts as a real call. That was live, not
/// hypothetical: geometry.md's FORM A fence carries the line
/// `// MUST be let-bound. Writing `constraint min_clearance(s, id_a, id_b) > 2mm``,
/// whose 3-arg `min_clearance(` is exactly the documented arity — so deleting the
/// fence's REAL `let clr = min_clearance(s, id_a, id_b)` left both
/// `reify_tagged_fences_in_geometry_chunk_compile` and
/// `documented_oracle_arities_are_exercised_by_a_compiling_fence` green while
/// their panic text claimed the form was "compile-verified" / "exercised by a
/// compiling fence". A commented-out call is not a call.
///
/// NOT AN AST WALK, and that is a scope decision rather than a preference. The
/// sibling `stdlib_chunk_geometry_ops_smoke.rs` already extracts `(name, arity)`
/// from the real parser via its `collect_call_forms` walk, which would close this
/// hole for free AND handle nesting exactly — but that helper is a private `fn` in
/// a sibling module, so reaching it needs a visibility edit to a file outside task
/// 5389's locked set, and copying its ~120-line exhaustive `ExprKind` match here
/// would make this binary's FOURTH near-identical scanner (see "Known
/// duplication" above), which is the opposite of what ticket
/// `tkt_0RS9A7843SBQ4BZX1A2ACY5TC1` exists to fix. The reconciled `chunk_io`
/// extraction should take the AST route for the fence side; until then this
/// stripper plus the unit tests at the bottom of this file are the guard.
///
/// Handles both comment forms the grammar defines (`tree-sitter-reify/grammar.js`
/// `line_comment` / `block_comment`) and does not strip inside a double-quoted
/// string. `://` is deliberately NOT a comment start, so the same helper is safe
/// on the chunk's markdown prose, where a URL would otherwise truncate its line.
/// A mis-tracked string can only cause a comment to survive, never content to be
/// dropped — i.e. it degrades to the un-stripped behaviour, never past it.
pub(crate) fn strip_reify_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_string = false;

    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            match c {
                '\\' => {
                    if let Some(escaped) = chars.next() {
                        out.push(escaped);
                    }
                }
                '"' => in_string = false,
                // An unterminated literal ends at the line break rather than
                // swallowing the rest of the input.
                '\n' => in_string = false,
                _ => {}
            }
            continue;
        }

        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            // `//` to end of line — but not the `//` in a `scheme://` URL.
            '/' if chars.peek() == Some(&'/') && !out.ends_with(':') => {
                chars.next();
                while chars.peek().is_some_and(|&n| n != '\n') {
                    chars.next();
                }
            }
            // `/* … */`, newlines preserved so line structure survives.
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev_star = false;
                for n in chars.by_ref() {
                    if n == '\n' {
                        out.push('\n');
                    }
                    if prev_star && n == '/' {
                        break;
                    }
                    prev_star = n == '*';
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// Every call to `name` in `text`, as `(arity, byte offset just past the closing
/// paren)`, in document order.
///
/// `text` MUST already be comment-free — pass it through
/// [`strip_reify_comments`] first. This function cannot tell a call from a
/// mention of one.
///
/// Arity is TOP-LEVEL commas + 1 over the balanced argument list, so a nested
/// call (`translate(box(a, b, c), …)`) contributes ONE argument and an empty list
/// is arity 0. Two things are skipped rather than guessed at: a `name(` whose
/// parens never balance (a call form wrapped across a markdown line), and a match
/// preceded by an identifier character, so `min_clearance(` is not harvested out
/// of a hypothetical `xmin_clearance(`.
pub(crate) fn call_sites(text: &str, name: &str) -> Vec<(usize, usize)> {
    let needle = format!("{name}(");
    let mut out = Vec::new();
    let mut cursor = 0usize;

    while let Some(rel) = text[cursor..].find(&needle) {
        let ident_start = cursor + rel;
        let open = ident_start + needle.len() - 1; // byte index of the `(`
        cursor = open + 1;

        if text[..ident_start]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
        {
            continue;
        }

        let mut depth = 0usize;
        let mut close = None;
        for (i, c) in text[open..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    // Cannot underflow: this scan starts AT the `(`, so `depth`
                    // is already >= 1 by the time any `)` is reached. Saturating
                    // here would be WRONG, not safer — it would make the
                    // `== 0` test fire on a stray `)` and report a short arity.
                    depth -= 1;
                    if depth == 0 {
                        close = Some(open + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close) = close else { continue };

        let inner = &text[open + 1..close];
        if inner.trim().is_empty() {
            out.push((0, close + 1));
            continue;
        }
        // Only `(`/`[` nest here. `<`/`>` are deliberately NOT treated as
        // brackets: they appear in this chunk as comparisons far more often than
        // as type parameters, and an unbalanced `>` would silently swallow the
        // commas after it.
        let mut depth = 0usize;
        let mut arity = 1usize;
        for c in inner.chars() {
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => arity += 1,
                _ => {}
            }
        }
        out.push((arity, close + 1));
    }
    out
}

/// Every DISTINCT identifier CALLED as `name(` in `text`, in document order.
///
/// `text` MUST already be comment-free — pass it through
/// [`strip_reify_comments`] first.
///
/// Complements [`call_sites`], which answers "at what arities is THIS name
/// called". This answers "which names are called AT ALL" — the direction a
/// phantom-signature check needs, because a phantom name is by definition one
/// nobody thought to ask about. `min_clearance(a, b)` was found by asking the
/// first question; `rotate(geo, axis, angle)` and `translate(geo, vector)`
/// (tasks #5347 / #5364) could only have been found by asking this one.
///
/// Each candidate is CONFIRMED through [`call_sites`] rather than trusted, so
/// the two scanners cannot disagree about what a call is: a `name(` whose parens
/// never balance (a call form wrapped across a markdown line) is skipped here by
/// exactly the rule that skips it there. A run starting with a digit is a
/// numeric literal juxtaposed with a paren, never a call.
pub(crate) fn called_names(text: &str) -> Vec<String> {
    fn is_ident(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<String> = Vec::new();

    for (i, &c) in chars.iter().enumerate() {
        if c != '(' {
            continue;
        }
        let mut start = i;
        while start > 0 && is_ident(chars[start - 1]) {
            start -= 1;
        }
        if start == i {
            continue;
        }
        let name: String = chars[start..i].iter().collect();
        if name.starts_with(|c: char| c.is_ascii_digit()) || out.contains(&name) {
            continue;
        }
        if call_sites(text, &name).is_empty() {
            continue;
        }
        out.push(name);
    }
    out
}

/// The compiler name registries a documented call form may legitimately belong
/// to, each paired with the const name to quote in a panic so the reader is told
/// WHERE to look rather than just that the lookup failed.
///
/// INCOMPLETE, AND KNOWABLY SO. `units::AFFINE_MAP_CONSTRUCTOR_NAMES` and
/// `math_signatures::MATH_CONSTRUCTION_NAMES` (which carries the `point2` /
/// `point3` / `vec2` / `vec3` prelude constructors geometry.md documents) are
/// real registries but are NOT re-exported from `reify_compiler`'s crate root —
/// `mod units` and `mod math_signatures` are both private, and lib.rs's
/// `pub use units::{…}` omits the affine one. Reaching them needs an edit to
/// `crates/reify-compiler/src/lib.rs`, outside task 5759's file scope. Callers
/// therefore carry an explicit, justified allowlist for names in those two
/// families; when the re-export lands, move those entries here and delete the
/// allowlist rather than growing it.
pub(crate) const CALLABLE_NAME_REGISTRIES: &[(&str, &[&str])] = &[
    (
        "GEOMETRY_FUNCTION_NAMES",
        reify_compiler::GEOMETRY_FUNCTION_NAMES,
    ),
    (
        "GEOMETRY_QUERY_HELPER_NAMES",
        reify_compiler::GEOMETRY_QUERY_HELPER_NAMES,
    ),
    (
        "GEOMETRY_KINEMATIC_QUERY_NAMES",
        reify_compiler::GEOMETRY_KINEMATIC_QUERY_NAMES,
    ),
    (
        "GEOMETRY_TOPOLOGY_SELECTOR_NAMES",
        reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES,
    ),
    ("GEOMETRY_QUERY_NAMES", reify_compiler::GEOMETRY_QUERY_NAMES),
];

/// The [`CALLABLE_NAME_REGISTRIES`] family `name` belongs to, or `None`.
pub(crate) fn registry_family(name: &str) -> Option<&'static str> {
    CALLABLE_NAME_REGISTRIES
        .iter()
        .find(|(_, names)| names.contains(&name))
        .map(|(family, _)| *family)
}

/// Panic text shared by both chunks' registry checks, so the two cannot drift
/// apart on what a reader is told to do about a phantom name.
pub(crate) fn phantom_name_panic(chunk_path: &str, where_: &str, name: &str) -> String {
    format!(
        "{chunk_path} documents a call to `{name}(…)` in {where_}, but `{name}` is not a member \
         of ANY compiler name registry ({:?}). Either the chunk teaches a PHANTOM signature the \
         compiler was never shown — the failure mode that cost live probe cycles in the \
         2026-07-24 language review (`rotate(geo, axis, angle)`, `translate(geo, vector)`; tasks \
         #5347 / #5364) — or the name is a prelude/math constructor from one of the registries \
         `CALLABLE_NAME_REGISTRIES` documents as unreachable, in which case add it to THIS \
         call site's allowlist with a justification. Do not widen the allowlist to silence a \
         name you have not looked up.",
        CALLABLE_NAME_REGISTRIES
            .iter()
            .map(|(family, _)| *family)
            .collect::<Vec<_>>()
    )
}

/// The arities `name` is DOCUMENTED at in `section`, read off its
/// `name(<args>) -> <Type>` signature forms.
///
/// The `->` is what separates a SIGNATURE from a mere mention, and the
/// distinction is load-bearing: the traps subsection deliberately writes
/// `min_clearance(a, b)` (the unsupported 2-arg overload) and
/// `min_clearance(s, id, id)` (the self-pair rider) as prose. Neither is a
/// contract the fences should be held to.
fn documented_signature_arities(section: &str, name: &str) -> Vec<usize> {
    // Comment-free first: the section body includes the fence lines, so an
    // annotated call form inside a fence comment must not read as a documented
    // signature either.
    let section = strip_reify_comments(section);
    call_sites(&section, name)
        .into_iter()
        .filter(|(_, after)| section[*after..].trim_start().starts_with("->"))
        .map(|(arity, _)| arity)
        .collect()
}

/// Every arity the oracle section DOCUMENTS must be exercised by a fence that
/// compiles.
///
/// This is the doc↔fence half of the arity story; the compiler half does not
/// exist (see the module doc). Adopted from the sibling suite's
/// `every_documented_geometry_op_form_is_exercised_by_the_fixture`, which pairs
/// documented (name, arity) forms against fixture calls for exactly this reason:
/// a documented signature and the worked example that is supposed to demonstrate
/// it must not be able to drift apart, since a designer copies whichever one they
/// read first.
#[test]
fn documented_oracle_arities_are_exercised_by_a_compiling_fence() {
    let markdown = read_chunk();
    let section = section_body(
        &markdown,
        ORACLE_SECTION_MARKER,
        CHUNK_PATH,
        ORACLE_SECTION_TITLE,
    );
    let fences = reify_tagged_fences(&markdown, "reify", CHUNK_PATH);

    for name in KINEMATIC_ORACLE_NAMES.iter().chain(GEOMETRY_ORACLE_NAMES) {
        let documented = documented_signature_arities(&section, name);
        // Anti-vacuity. A signature form that loses its `-> <Type>` annotation
        // would otherwise drop out of this check silently instead of failing it.
        assert!(
            !documented.is_empty(),
            "{CHUNK_PATH}'s `{ORACLE_SECTION_TITLE}` section documents no `{name}(…) -> <Type>` \
             signature form, so nothing pins that name's arity and this check would pass \
             vacuously for it. Restore the `-> <Type>` return annotation on the call form."
        );

        // Comment-free fence bodies: a call form that appears only in a `//`
        // annotation is not an exercised form (see `strip_reify_comments`).
        let in_fences: Vec<usize> = fences
            .iter()
            .map(|fence| strip_reify_comments(fence))
            .flat_map(|fence| {
                call_sites(&fence, name)
                    .into_iter()
                    .map(|(arity, _)| arity)
                    .collect::<Vec<_>>()
            })
            .collect();

        for arity in &documented {
            assert!(
                in_fences.contains(arity),
                "{CHUNK_PATH} documents `{name}` at {arity} argument(s), but no ```reify fence \
                 calls it at that arity (fence call arities for `{name}`: {in_fences:?}). Either \
                 the documented signature is a phantom the compiler was never shown, or a fence \
                 drifted off the form it demonstrates. The fences are what actually compile, so \
                 fix whichever of the two is wrong — a designer copies whichever they read first."
            );
        }
    }
}

/// Minimum CATALOGUE ROWS the LENGTH-ARGUMENTS table must carry.
///
/// The EXACT live count (`translate`, `rotate_around`, `revolve`,
/// `line_segment`, `arc`, `helix`, `interp`/`bezier`, `nurbs`, `polygon`), not a
/// round number under it — at a lower floor a whole row could be deleted while
/// this stayed green, which is precisely the regression the floor claims to
/// catch. Raise it WITH the table; never lower it to go green.
const MINIMUM_CATALOGUE_ROWS: usize = 9;

/// The constructor names the LENGTH-ARGUMENTS catalogue TABLE claims a
/// dimensioned argument for — one entry per markdown row, in document order.
///
/// FIRST COLUMN ONLY, and that is the whole point of the scan rather than a
/// simplification of it. The catalogue's claim is made by the row's subject: the
/// later columns are prose that legitimately backticks argument NAMES
/// (`degree`, `n_points`), which are not constructors and must not be fed to a
/// registry lookup. Taking column one keeps "every name this asserts is real" a
/// true statement instead of one needing an allowlist to stay green.
///
/// A cell may name more than one constructor (the live table's
/// ``| `interp` / `bezier` |`` row), so a row yields a Vec. Rows that yield
/// nothing — the header, the `|---|---|---|` delimiter, any `|`-leading line
/// without a backticked identifier — are dropped, so `.len()` counts CATALOGUE
/// rows and nothing else.
///
/// A backticked span is accepted only if it is a bare identifier, optionally
/// followed by a call form: ``` `helix` ``` and ``` `helix(radius, pitch,
/// height)` ``` both yield `helix`. Anything else (a prose span, a unit
/// literal) is skipped rather than guessed at.
pub(crate) fn catalogue_table_rows(section: &str) -> Vec<Vec<String>> {
    fn is_ident(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    let mut rows: Vec<Vec<String>> = Vec::new();

    for line in section.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix('|') else {
            continue;
        };
        // The FIRST cell: everything up to the next `|`, or the whole remainder
        // for a (malformed) single-column row.
        let first_cell = rest.split('|').next().unwrap_or_default();

        let mut names: Vec<String> = Vec::new();
        for span in first_cell.split('`').skip(1).step_by(2) {
            // Trim a trailing call form, so `helix` and
            // `helix(radius, pitch, height)` are the same claim.
            let head = span.split('(').next().unwrap_or_default().trim();
            if !head.is_empty()
                && head.chars().all(is_ident)
                && !head.starts_with(|c: char| c.is_ascii_digit())
                && !names.contains(&head.to_string())
            {
                names.push(head.to_string());
            }
        }
        if !names.is_empty() {
            rows.push(names);
        }
    }
    rows
}

/// Every geometry constructor the LENGTH-ARGUMENTS section names as taking a
/// dimensioned argument must be a REAL registry entry.
///
/// Twin of `units_chunk_smoke.rs`'s
/// `documented_call_names_in_units_chunk_are_real_registry_entries`: units.md
/// owns the RULE and the migration idiom, this chunk owns the per-position
/// CATALOGUE, and each is registry-verified on its own side rather than one
/// restating the other.
///
/// This is the phantom-NAME direction, and it is the one that has actually
/// failed in this repo: the 2026-07-24 language review found `rotate(geo, axis,
/// angle)` and `translate(geo, vector)` documented at signatures the compiler
/// had never been shown (tasks #5347 / #5364). A catalogue that names a
/// constructor which does not exist sends an author to write a call that cannot
/// compile, and — because a `structure def` body types an unresolved call from
/// its first argument — often will not even say so.
///
/// # What is actually scanned
///
/// TWO INDEPENDENT SETS, because [`called_names`] alone did not reach the
/// catalogue this test is named for. That scanner harvests an identifier only
/// where it is immediately followed by `(`, and the table writes eight of its
/// nine constructors as a bare backticked name — so DELETING EVERY TABLE ROW
/// left this test green, all three sentinels still satisfied by the ```reify
/// fence below the table (measured; only the `helix(radius, pitch, height)` row
/// was ever visible). The advertised reach was the reach a future editor would
/// rely on, so the scan was widened rather than the docstring narrowed:
///
///   - [`catalogue_table_rows`] harvests the TABLE's first column, floored at
///     [`MINIMUM_CATALOGUE_ROWS`] so deleting rows is RED;
///   - [`called_names`] harvests the section's CALL FORMS, floored at
///     [`MINIMUM_SECTION_CALL_NAMES`] so rewriting the fence into prose is RED.
///
/// Every name from either set faces the same registry assertion, and the three
/// sentinels are asserted against BOTH — the table must NAME them and the fence
/// must CALL them. Union-sentinels would let one half lose a constructor while
/// the other half covered for it, which is the failure this amendment exists to
/// close.
///
/// SCOPE — this checks NAMES, not arities and not argument dimensions. Arity for
/// the oracle names is cross-checked separately by
/// `documented_oracle_arities_are_exercised_by_a_compiling_fence`; argument
/// DIMENSION is pinned on the eval side by the tests the section's SYNC block
/// cites, not here.
#[test]
fn documented_call_names_in_the_length_section_are_real_registry_entries() {
    let markdown = read_chunk();
    // `section_body` panics if the marker is gone, so gutting the catalogue is
    // RED rather than vacuously green.
    let section = section_body(
        &markdown,
        LENGTH_ARGS_SECTION_MARKER,
        CHUNK_PATH,
        LENGTH_ARGS_SECTION_TITLE,
    );

    let table_rows = catalogue_table_rows(&section);
    let table_names: Vec<String> = {
        let mut out: Vec<String> = Vec::new();
        for row in &table_rows {
            for name in row {
                if !out.contains(name) {
                    out.push(name.clone());
                }
            }
        }
        out
    };
    let called = called_names(&strip_reify_comments(&section));

    // Anti-vacuity, one floor per set. The ROW floor catches a deleted or
    // gutted catalogue table — the case that used to pass silently. The CALL
    // floor catches a section rewritten into prose without call forms, so the
    // worked fence stops demonstrating anything.
    assert!(
        table_rows.len() >= MINIMUM_CATALOGUE_ROWS,
        "only {} catalogue row(s) found in {CHUNK_PATH}'s `{LENGTH_ARGS_SECTION_TITLE}` table — \
         expected at least {MINIMUM_CATALOGUE_ROWS}. Either rows were deleted, or the table was \
         reformatted into a shape this scan cannot read: a catalogue row is a `|`-leading line \
         whose FIRST cell backticks the constructor it is about. Rows seen: {table_rows:?}",
        table_rows.len()
    );
    assert!(
        called.len() >= MINIMUM_SECTION_CALL_NAMES,
        "only {} distinct call name(s) found in {CHUNK_PATH}'s `{LENGTH_ARGS_SECTION_TITLE}` \
         section — expected at least {MINIMUM_SECTION_CALL_NAMES}. The worked forms were \
         rewritten into prose without call forms, so the section demonstrates nothing a compiler \
         has seen. Names seen: {called:?}",
        called.len()
    );

    // Sentinels, against BOTH sets. `translate` is the headline case (every
    // component length-semantic, bare `0` included); `polygon` is the one with
    // NO dimensionless position at all; `nurbs` is the one that mixes both in a
    // single argument list, so it is where a catalogue is load-bearing.
    for sentinel in ["translate", "polygon", "nurbs"] {
        assert!(
            table_names.iter().any(|n| n == sentinel),
            "{CHUNK_PATH}'s `{LENGTH_ARGS_SECTION_TITLE}` TABLE no longer has a row for \
             `{sentinel}`. That position is one of the three the catalogue exists to \
             distinguish — `translate` (every component length-semantic, bare `0` included), \
             `polygon` (no dimensionless position at all) and `nurbs` (lengths and counts in one \
             argument list). A worked fence below the table is NOT a substitute: the table is \
             what an author scans to find their constructor. Table names seen: {table_names:?}"
        );
        assert!(
            called.iter().any(|n| n == sentinel),
            "{CHUNK_PATH}'s `{LENGTH_ARGS_SECTION_TITLE}` section no longer CALLS `{sentinel}` \
             outside a comment, so the row that names it is no longer demonstrated by anything \
             the compiler has accepted. Call names seen: {called:?}"
        );
    }

    for name in table_names.iter().chain(called.iter()) {
        assert!(
            registry_family(name).is_some()
                || LENGTH_SECTION_NAME_ALLOWLIST.contains(&name.as_str()),
            "{}",
            phantom_name_panic(
                CHUNK_PATH,
                &format!("its `{LENGTH_ARGS_SECTION_TITLE}` section"),
                name
            )
        );
    }
}

/// Minimum distinct CALL names the LENGTH-ARGUMENTS section's worked forms must
/// carry, as the anti-vacuity floor on the [`called_names`] half.
///
/// SEPARATE from [`MINIMUM_CATALOGUE_ROWS`], and deliberately so: this floors
/// the FENCE, which is a different claim from the table's coverage — a section
/// can tabulate a constructor it never demonstrates, and demonstrate one it
/// never tabulates — so it keeps its own number instead of tracking the table's.
///
/// That number is the EXACT live count of 8 (`helix`, `translate`, `cylinder`,
/// `rotate_around`, `polygon`, `interp`, `nurbs`, `scale`), under the same
/// contract as [`MINIMUM_FN_CITES`]: at a floor under live, calls can be deleted
/// from the worked forms while this stays green. See that constant for the
/// re-measurement protocol.
const MINIMUM_SECTION_CALL_NAMES: usize = 8;

/// Names the length-arguments section may call that are in none of
/// [`CALLABLE_NAME_REGISTRIES`], each with its justification.
///
/// EMPTY, and kept empty on purpose — every constructor the catalogue names is a
/// `GEOMETRY_FUNCTION_NAMES` entry. The hook exists because the prelude
/// constructors this chunk documents elsewhere (`point3`, `vec3`, …) live in
/// `math_signatures::MATH_CONSTRUCTION_NAMES`, which is not reachable from the
/// crate root; see [`CALLABLE_NAME_REGISTRIES`]. Adding an entry here is a claim
/// that a name is real-but-unreachable, so write the justification next to it.
const LENGTH_SECTION_NAME_ALLOWLIST: &[&str] = &[];

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

// --- Cited-test resolution ---------------------------------------------------
//
// The chunk's two `<!-- SYNC ... -->` blocks hand-inventory the eval/CLI tests
// that pin each clearance-query trap, and that inventory is explicitly written to
// be read as the AUTHORITY on which traps are safe to rely on. Nothing else in
// this file looks at it: the guards above cover names, arities and fence
// compilation only. So a renamed or deleted test silently turns a PINNED row into
// a false claim — a rot mode strictly worse than plain prose, because the row
// still LOOKS load-bearing. The check below closes that.

/// Repo root, derived from this crate's manifest dir
/// (`<repo>/crates/reify-compiler`).
fn repo_root() -> std::path::PathBuf {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap_or_else(|| {
            panic!("CARGO_MANIFEST_DIR ({manifest:?}) must sit two levels under the repo root")
        })
        .to_path_buf()
}

/// Index of every tracked-ish `.rs`/`.ri` file under `crates/` and `examples/`,
/// keyed by BASENAME, so the chunk may cite a test by bare file name (as its
/// prose already does) without this check hard-coding a directory.
///
/// Build artifacts are skipped by directory name rather than by path prefix, so a
/// nested `target/` cannot smuggle a stale duplicate into the index and make an
/// otherwise-unique basename ambiguous.
pub(crate) fn source_files_by_basename()
-> std::collections::BTreeMap<String, Vec<std::path::PathBuf>> {
    fn walk(
        dir: &std::path::Path,
        out: &mut std::collections::BTreeMap<String, Vec<std::path::PathBuf>>,
    ) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if matches!(name.as_str(), "target" | ".git" | "node_modules") {
                    continue;
                }
                walk(&path, out);
            } else if name.ends_with(".rs") || name.ends_with(".ri") {
                out.entry(name).or_default().push(path);
            }
        }
    }

    let root = repo_root();
    let mut out = std::collections::BTreeMap::new();
    walk(&root.join("crates"), &mut out);
    walk(&root.join("examples"), &mut out);
    out
}

/// Every `<path>` / `<path>::<fn_name>` cite naming a `.rs` or `.ri` file in
/// `markdown`, deduped, in document order.
///
/// Scans maximal runs of path-ish characters, so markdown decoration (backticks,
/// parens, commas, the possessive `'s`) bounds a run rather than being swallowed.
/// A run is only a cite if the part before `::` ends in `.rs`/`.ri` — that is what
/// keeps the chunk's C++ cites (`BRepExtrema_DistShapeShape::InnerSolution()`) out
/// of the resolution attempt without an exclusion list.
///
/// SCOPED TO THE CITE FORMS THE SYNC BLOCKS ACTUALLY USE — a token counts only if
/// it carries a `::<fn>` half or a `/`-bearing repo-relative path. A BARE basename
/// in prose is NOT a cite. geometry.md is a designer-facing tutorial whose whole
/// subject is writing `.ri` files, so it will keep acquiring illustrative
/// filenames ("save the model as `my_bracket.ri`"); resolving those would make an
/// ordinary doc edit RED with a panic about SYNC blocks and false PINNED claims,
/// i.e. a message that names neither the edit nor its cause. Every real cite in
/// the two SYNC blocks is written in one of the two accepted forms, and
/// `cited_test_paths_in_the_chunk_resolve`'s floors keep it that way, so nothing
/// the check exists for is lost by ignoring bare basenames.
pub(crate) fn cited_source_paths(markdown: &str) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = Vec::new();

    for run in markdown
        .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | ':' | '-')))
    {
        // Sentence punctuation that the run charset happens to include.
        let run = run.trim_end_matches(['.', '/', ':', '-']);
        let (path, fn_name) = match run.split_once("::") {
            Some((path, rest)) => (path, Some(rest)),
            None => (run, None),
        };
        if !(path.ends_with(".rs") || path.ends_with(".ri")) {
            continue;
        }
        // Tested on the RAW `::` split, before the identifier filter below: a
        // malformed fn half still marks the token as an intended cite, so its
        // path half stays subject to resolution.
        if fn_name.is_none() && !path.contains('/') {
            continue;
        }
        // A cite whose fn half is not a bare identifier is a malformed cite, not
        // a licence to skip the path half — keep the path, drop the fn.
        let fn_name = fn_name
            .filter(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        let cite = (path.to_string(), fn_name.map(str::to_string));
        if !out.contains(&cite) {
            out.push(cite);
        }
    }
    out
}

/// Resolve one cited path token to a real file, or explain why it did not.
pub(crate) fn resolve_cited_path(
    token: &str,
    index: &std::collections::BTreeMap<String, Vec<std::path::PathBuf>>,
) -> Result<std::path::PathBuf, String> {
    let root = repo_root();
    if token.contains('/') {
        // Repo-relative, or crate-relative (the chunk writes both forms).
        for candidate in [root.join(token), root.join("crates").join(token)] {
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        return Err(format!(
            "no such file — tried {:?} and {:?}",
            root.join(token),
            root.join("crates").join(token)
        ));
    }
    match index.get(token).map(Vec::as_slice) {
        None | Some([]) => Err(format!(
            "no file named `{token}` exists under crates/ or examples/"
        )),
        Some([only]) => Ok(only.clone()),
        Some(many) => Err(format!(
            "`{token}` is ambiguous — {} files share that basename ({many:?}); cite it by its \
             full repo-relative path instead",
            many.len()
        )),
    }
}

/// Assert every `<path>::<fn>` cite in `markdown` resolves, and that the chunk
/// still carries at least `min_fn` / `min_rs` / `min_ri` of them.
///
/// SHARED BY BOTH CHUNK MODULES — this file's traps SYNC block and
/// `units_chunk_smoke.rs`'s PINNED/UNPINNED inventory. It exists because the
/// second copy of this loop was a ~55-line near-verbatim duplicate of the first,
/// differing only in the chunk path, three numeric floors and the panic wording;
/// a fix to the resolution or existence rule (a `#[cfg]`-gated fn, a `fn foo<T>(`
/// with a generic parameter) then had to be applied twice or drift. Growing that
/// kind of copy is exactly the tracked defect
/// (`tkt_0RS9A7843SBQ4BZX1A2ACY5TC1` / task #5924) this harness binary is trying
/// to shrink.
///
/// The CHUNK-SPECIFIC "why this matters" prose lives in each caller's docstring,
/// not in the panic text here, so the shared message stays true for both. What
/// the panics do carry is the chunk path, the floor that was missed and the full
/// cite list, which is what a reader needs to act.
///
/// Floors are `>=`, so ADDING a cite is always safe; raise them WITH the chunk
/// when one is added, and never lower one to go green — a lowered floor is a
/// SYNC row that has quietly stopped claiming anything.
///
/// SCOPE — an EXISTENCE check, not a semantic one. It cannot tell that a
/// still-named test stopped asserting what the row claims, it says nothing about
/// rows marked UNPINNED, and it does not verify the fn is a `#[test]`.
pub(crate) fn assert_cited_paths_resolve(
    chunk_path: &str,
    markdown: &str,
    min_fn: usize,
    min_rs: usize,
    min_ri: usize,
) {
    let index = source_files_by_basename();
    let cites = cited_source_paths(markdown);

    // Keyed by the RESOLVED path, not the cite token: both chunks cite some
    // files two ways (bare basename with a `::fn` half, and again by full
    // repo-relative path), so token-counting would let one real reference
    // disappear while the floor stayed satisfied by its own duplicate — exactly
    // the regression these floors claim to catch.
    let mut rs_paths: std::collections::BTreeSet<std::path::PathBuf> =
        std::collections::BTreeSet::new();
    let mut ri_paths: std::collections::BTreeSet<std::path::PathBuf> =
        std::collections::BTreeSet::new();
    let mut fn_cites = 0usize;

    for (path_token, fn_name) in &cites {
        let resolved = resolve_cited_path(path_token, &index).unwrap_or_else(|why| {
            panic!(
                "{chunk_path} cites `{path_token}`, which does not resolve: {why}. The chunk is \
                 served verbatim to the in-GUI assistant and its SYNC rows are written to be read \
                 as the authority on which claims a real test pins — a dangling cite is a false \
                 claim. Update the cite, or mark the row UNPINNED."
            )
        });

        if path_token.ends_with(".ri") {
            ri_paths.insert(resolved.clone());
        } else {
            rs_paths.insert(resolved.clone());
        }

        let Some(fn_name) = fn_name else { continue };
        fn_cites += 1;
        let body = std::fs::read_to_string(&resolved)
            .unwrap_or_else(|e| panic!("{resolved:?} must be readable ({e})"));
        assert!(
            body.contains(&format!("fn {fn_name}(")),
            "{chunk_path} cites `{path_token}::{fn_name}` as pinning one of its claims, but \
             {resolved:?} declares no `fn {fn_name}(`. The test was renamed or deleted, so that \
             row now claims a pin that does not exist. Re-point the cite, or downgrade the row \
             to UNPINNED."
        );
    }

    // Anti-vacuity. Reformatting a SYNC block into a shape this scan cannot read
    // — a path wrapped across two lines, or tabulated into two columns — would
    // otherwise empty the loop above and pass.
    assert!(
        fn_cites >= min_fn,
        "only {fn_cites} `<path>::<fn>` cite(s) found in {chunk_path} — expected at least \
         {min_fn}. CITES MUST BE WRITTEN WHOLE ON ONE LINE, never wrapped and never tabulated; a \
         wrapped path is invisible to this scan. Either the chunk was reformatted into a shape it \
         cannot read, or a row lost its cite while still claiming to pin something. Cites seen: \
         {cites:?}"
    );
    assert!(
        rs_paths.len() >= min_rs,
        "only {} distinct `.rs` FILE(s) cited in {chunk_path} (distinct after resolution — the \
         same file cited two ways counts once), expected at least {min_rs}. Losing one turns a \
         PINNED row into prose. Cites seen: {cites:?}",
        rs_paths.len()
    );
    assert!(
        ri_paths.len() >= min_ri,
        "only {} distinct `.ri` example FILE(s) cited in {chunk_path} (distinct after resolution \
         — the same example cited both bare and by full path counts once), expected at least \
         {min_ri}. The worked examples are what a designer is sent to next, so losing a cite is a \
         discoverability regression. Cites seen: {cites:?}",
        ri_paths.len()
    );
}

/// Cite floors for [`cited_test_paths_in_the_chunk_resolve`].
///
/// The EXACT live counts, not round numbers under them: 13 `<path>::<fn>` cites,
/// resolving to 6 distinct `.rs` files and 4 distinct `.ri` files. The 13 are
/// three self-cites in this file, two into `units_chunk_smoke.rs`, two into
/// `cli_vc_clearance.rs`, one into `kernel_queries_intersects_smoke.rs` and five
/// into `mechanism_interference_smoke.rs`.
///
/// WHY EXACT — a measured incident, not a principle. With the cite floor one
/// below live, dropping trap 5's `single_body_self_pair_excluded` row left
/// fn_cites=8, rs_paths=5 and ri_paths=4 — every floor still green while that
/// row went on reading "PINNED by", a SYNC row silently claiming a pin it had
/// lost. Any gap between floor and live re-opens exactly that hole, which is why
/// these track the tree rather than sitting at a round number under it.
///
/// The `.ri` floor covers the four worked references (clearance_oracle,
/// vc_bolt_pattern_clearance, dock_pickup, intersects_smoke) a designer is sent
/// to next; losing one is the same discoverability regression task 5389 closed.
///
/// RE-MEASUREMENT PROTOCOL, for every `MINIMUM_*` floor in this file and in
/// `units_chunk_smoke.rs` — stated once, here, and cross-referenced rather than
/// copied. These floors are hand-maintained and NOTHING detects their drift:
/// adding a SYNC row, a cite, a catalogue row or a fence call name silently
/// widens the gap between floor and live, and the check stays green while its
/// own docstring still claims to be exact. To re-measure one, set it to 999, run
/// `env cargo test -p reify-compiler --test harness_doc_chunks`, read the live
/// count out of the panic text, and restore it. Use `env cargo`, never bare
/// `cargo`: the skim PreToolUse wrapper condenses the run to PASS/FAIL counts
/// and swallows the panic the measurement depends on. MEASURE ONE FLOOR AT A
/// TIME — floors inside a single test assert sequentially, so a failing early
/// floor MASKS every later one in that test, and a batched sweep under-reports.
/// That masking is not hypothetical: it is why a first pass over these ten
/// floors found two of the four that had gone stale, and a one-at-a-time sweep
/// found all four.
const MINIMUM_FN_CITES: usize = 13;
const MINIMUM_RS_FILES: usize = 6;
const MINIMUM_RI_FILES: usize = 4;

/// Every test the chunk cites as PINNING a runtime claim must still exist.
///
/// WHY THIS CHUNK NEEDS IT. geometry.md's two `<!-- SYNC ... -->` blocks
/// hand-inventory the eval/CLI tests that pin each clearance-query trap, and
/// that inventory is explicitly written to be read as the AUTHORITY on which
/// traps are safe to rely on. Nothing else in this file looks at it — the guards
/// above cover names, arities and fence compilation only — so a renamed or
/// deleted test would silently turn a PINNED row into a false claim, a rot mode
/// strictly worse than plain prose because the row still LOOKS load-bearing.
///
/// The mechanism is shared with `units_chunk_smoke.rs`'s twin; see
/// [`assert_cited_paths_resolve`] for what it checks and, importantly, what it
/// does NOT (an existence check, never a semantic one).
#[test]
fn cited_test_paths_in_the_chunk_resolve() {
    assert_cited_paths_resolve(
        CHUNK_PATH,
        &read_chunk(),
        MINIMUM_FN_CITES,
        MINIMUM_RS_FILES,
        MINIMUM_RI_FILES,
    );
}

// --- Scanner unit tests ------------------------------------------------------
//
// `call_sites`, `strip_reify_comments`, `section_body`, `cited_source_paths`,
// `called_names` and `catalogue_table_rows` are the hand-rolled text scanners in
// this file, and every doc↔fence assertion above is downstream of one of them,
// so they are pinned directly here rather than only through the chunk. Mirrors
// the posture of `stdlib_chunk_geometry_ops_smoke.rs`, whose
// `documented_geometry_op_forms` scanner carries its own `_extracts_exact_arity`
// / `_zero_arg_span_is_exact_zero` / `_skips_unbalanced_parens_without_panicking`
// unit tests.
//
// THE FAILURE THESE CLOSE IS SELF-CONCEALING. `called_names` is the sole
// extraction path for BOTH chunks' phantom-name gates, and `catalogue_table_rows`
// for geometry.md's catalogue floor — but every anti-vacuity floor and sentinel
// above is drawn from those same scanners' OWN output. A regression that made one
// of them over-skip would weaken the gate while leaving every floor and sentinel
// satisfied, because both sides of the comparison would shrink together. Only a
// direct test over a known input can see that.

#[test]
fn call_sites_counts_a_nested_call_as_one_argument() {
    let arities: Vec<usize> = call_sites(
        "let b = translate(box(20mm, 20mm, 20mm), 30mm, 0mm, 0mm)",
        "translate",
    )
    .into_iter()
    .map(|(arity, _)| arity)
    .collect();
    assert_eq!(
        arities,
        vec![4],
        "the nested `box(...)` must contribute ONE argument, not its own three"
    );
}

#[test]
fn call_sites_reads_an_empty_argument_list_as_arity_zero() {
    let arities: Vec<usize> = call_sites("let m0 = mechanism()", "mechanism")
        .into_iter()
        .map(|(arity, _)| arity)
        .collect();
    assert_eq!(arities, vec![0]);
}

#[test]
fn call_sites_skips_an_identifier_prefixed_match() {
    assert!(
        call_sites("let x = xmin_clearance(s, id_a, id_b)", "min_clearance").is_empty(),
        "`min_clearance(` must not be harvested out of a longer identifier"
    );
}

#[test]
fn call_sites_skips_a_call_form_whose_parens_never_balance() {
    assert!(
        call_sites("min_clearance(s, id_a,", "min_clearance").is_empty(),
        "an unbalanced call form is skipped rather than guessed at"
    );
}

#[test]
fn call_sites_offset_lands_just_past_the_closing_paren() {
    // This is the offset `documented_signature_arities` uses to find the `->`.
    let src = "`distance(a, b) -> Length` (2-arg)";
    let sites = call_sites(src, "distance");
    assert_eq!(sites.len(), 1);
    let (arity, after) = sites[0];
    assert_eq!(arity, 2);
    assert!(
        src[after..].trim_start().starts_with("-> Length"),
        "expected the return annotation just past the closing paren, got {:?}",
        &src[after..]
    );
}

#[test]
fn call_sites_does_not_see_a_call_that_only_appears_in_a_comment() {
    // geometry.md's FORM A fence, reduced to the two lines that matter: the
    // annotation names the 3-arg form, the code calls the 2-arg one.
    let fence = "// Writing `constraint min_clearance(s, id_a, id_b) > 2mm` inline is wrong.\n\
                 let clr = min_clearance(s, id_a)";

    let raw: Vec<usize> = call_sites(fence, "min_clearance")
        .into_iter()
        .map(|(arity, _)| arity)
        .collect();
    assert_eq!(
        raw,
        vec![3, 2],
        "a RAW scan sees the commented form too — this is the hole `strip_reify_comments` closes"
    );

    let code: Vec<usize> = call_sites(&strip_reify_comments(fence), "min_clearance")
        .into_iter()
        .map(|(arity, _)| arity)
        .collect();
    assert_eq!(
        code,
        vec![2],
        "a comment-free scan must see only the call the compiler actually gets"
    );
}

#[test]
fn strip_reify_comments_leaves_ordinary_source_untouched() {
    let src = "structure def S {\n    let g = box(1mm, 2mm, 3mm)\n}";
    assert_eq!(strip_reify_comments(src), src);
}

#[test]
fn strip_reify_comments_keeps_a_url_intact() {
    // The same helper runs over the chunk's markdown prose, where `//` after a
    // scheme is not a comment.
    let src = "see https://example.test/clearance for more";
    assert_eq!(strip_reify_comments(src), src);
}

#[test]
fn strip_reify_comments_leaves_a_double_slash_inside_a_string_literal() {
    let src = r#"let m1 = body(m0, "a//b", fixed()) // drop me"#;
    assert_eq!(
        strip_reify_comments(src),
        r#"let m1 = body(m0, "a//b", fixed()) "#
    );
}

#[test]
fn strip_reify_comments_removes_a_block_comment_and_preserves_line_count() {
    let src = "let a = box(1mm, 1mm, 1mm)\n/* two\n   lines */\nlet b = sphere(1mm)";
    let out = strip_reify_comments(src);
    assert_eq!(
        out.lines().count(),
        src.lines().count(),
        "line structure must survive so panic messages still line up with the chunk"
    );
    assert!(
        !out.contains("two"),
        "block-comment body must be gone: {out:?}"
    );
    assert!(
        out.contains("sphere(1mm)"),
        "code after the comment must survive"
    );
}

#[test]
fn section_body_reads_a_shorter_fence_run_as_content_of_a_longer_one() {
    // A 4-backtick fence displaying a 3-backtick one — the routine shape in a doc
    // that shows markdown, and the shape a toggle-based scanner desyncs on: the
    // inner ``` lines would flip fence state twice more, leaving the marker line
    // "inside a fence" and unreachable.
    let md = "# Chunk\n\
              ````markdown\n\
              ```reify\n\
              let g = box(1mm, 1mm, 1mm)\n\
              ```\n\
              ````\n\
              <!-- ORACLE-SECTION -->\n\
              body line\n\
              ## Next section\n\
              not in the body\n";

    assert_eq!(
        section_body(md, ORACLE_SECTION_MARKER, CHUNK_PATH, ORACLE_SECTION_TITLE),
        "body line",
        "the inner ``` run is shorter than the ```` that opened the fence, so it is CONTENT — \
         only a bare run of >= 4 closes"
    );
}

#[test]
fn section_body_keeps_a_fenced_heading_out_of_the_section_boundary() {
    // The property the old toggle already had, re-pinned against the rewrite: a
    // `## ` line inside a fence is content, not the end of the section.
    let md = "<!-- ORACLE-SECTION -->\n\
              ```reify\n\
              // ## not a heading\n\
              ```\n\
              tail\n\
              ## Real heading\n\
              gone\n";

    let body = section_body(md, ORACLE_SECTION_MARKER, CHUNK_PATH, ORACLE_SECTION_TITLE);
    assert!(body.contains("// ## not a heading"), "got {body:?}");
    assert!(body.contains("tail"), "got {body:?}");
    assert!(!body.contains("gone"), "got {body:?}");
}

#[test]
#[should_panic(expected = "unterminated code fence")]
fn section_body_blames_an_unterminated_fence_rather_than_the_marker() {
    // The marker is PRESENT here. Blaming it (which a toggle-based scanner's
    // anti-vacuity panic does, because the swallowed tail leaves `in_section`
    // false) sends the reader to a line that is not the defect.
    let md = "```reify\n\
              let g = box(1mm, 1mm, 1mm)\n\
              <!-- ORACLE-SECTION -->\n\
              body\n";
    let _ = section_body(md, ORACLE_SECTION_MARKER, CHUNK_PATH, ORACLE_SECTION_TITLE);
}

#[test]
fn cited_source_paths_ignores_a_bare_illustrative_basename() {
    // geometry.md is a designer-facing tutorial about authoring `.ri` files, so
    // prose like this is ordinary content — not a claim that a repo file exists.
    // Resolving it would make an ordinary doc edit RED with a panic about SYNC
    // blocks and false PINNED claims.
    let md = "Save the model as `my_bracket.ri` and run `reify build my_bracket.ri`.\n\
              trap 5 — PINNED by\n\
              crates/reify-eval/tests/harness_mechanism/mechanism_interference_smoke.rs::single_body_self_pair_excluded\n\
              See `examples/kinematic/dock_pickup.ri`, and `geometry_chunk_smoke.rs`, whose\n\
              `geometry_chunk_smoke.rs::cited_test_paths_in_the_chunk_resolve` resolves them.\n";

    assert_eq!(
        cited_source_paths(md),
        vec![
            (
                "crates/reify-eval/tests/harness_mechanism/mechanism_interference_smoke.rs"
                    .to_string(),
                Some("single_body_self_pair_excluded".to_string()),
            ),
            ("examples/kinematic/dock_pickup.ri".to_string(), None),
            (
                "geometry_chunk_smoke.rs".to_string(),
                Some("cited_test_paths_in_the_chunk_resolve".to_string()),
            ),
        ],
        "only `/`-bearing paths and `::<fn>`-carrying tokens are cites; `my_bracket.ri` and the \
         bare `geometry_chunk_smoke.rs` mention are prose"
    );
}

#[test]
fn cited_source_paths_keeps_a_malformed_fn_half_as_a_path_cite() {
    // `::` marks the token as an INTENDED cite even when the fn half is not a bare
    // identifier, so the path half stays subject to resolution rather than being
    // dropped as if it were a prose basename.
    assert_eq!(
        cited_source_paths("geometry_chunk_smoke.rs::not-an-ident"),
        vec![("geometry_chunk_smoke.rs".to_string(), None)]
    );
}

#[test]
fn cited_source_paths_leaves_a_cxx_cite_alone() {
    // The chunk cites OCCT's C++ API for the containment behaviour; the path half
    // does not end in `.rs`/`.ri`, so no resolution is attempted.
    assert!(
        cited_source_paths("BRepExtrema_DistShapeShape::InnerSolution()").is_empty(),
        "a C++ `Type::method()` cite is not a source-file cite"
    );
}

/// A digit-prefixed run juxtaposed with `(` is a numeric literal, not a call.
///
/// The discriminator `called_names` documents, asserted directly. Without the
/// skip, `2(x + 1)` would feed `2` to `registry_family` and every registry gate
/// downstream would fail on a name no author ever wrote.
#[test]
fn called_names_skips_a_numeric_literal_juxtaposed_with_a_paren() {
    assert_eq!(
        called_names("let scaled = 2(x) + box(1mm, 1mm, 1mm)"),
        vec!["box".to_string()],
        "`2(` is a literal juxtaposed with a paren; only `box` is a call"
    );
}

/// A `name(` whose parens never balance is skipped, matching `call_sites`.
///
/// The two scanners CONFIRM each other by construction — `called_names` runs
/// every candidate back through `call_sites` — and this pins that the
/// confirmation actually discriminates. A call form wrapped across a markdown
/// line is the live shape this protects against: half a call is not a call, and
/// counting it would let a fence "demonstrate" a form it never compiled.
///
/// The skip is PER NAME, not per line, which is the behaviour the assertion
/// below fixes in place: in `translate(box(1mm, 1mm, 1mm),` the OUTER
/// `translate(` never closes and is dropped, while the INNER `box(…)` closes on
/// its own and is kept. That is the right call — `box` really is demonstrated
/// here — and it is worth pinning precisely because the coarser "drop the whole
/// unbalanced line" reading is the one a reader assumes.
#[test]
fn called_names_skips_a_call_whose_parens_never_balance() {
    assert_eq!(
        called_names("let wrapped = translate(box(1mm, 1mm, 1mm),"),
        vec!["box".to_string()],
        "`translate(` never closes so it is not a call; the nested `box(…)` does, so it is"
    );
    // Control: the same text, closed, yields BOTH in document order — so the
    // assertion above is about `translate` being unbalanced, not about the
    // scanner being unable to see an outer call at all.
    assert_eq!(
        called_names("let ok = translate(box(1mm, 1mm, 1mm), 0mm, 0mm, 0mm)"),
        vec!["translate".to_string(), "box".to_string()]
    );
}

/// Repeated calls collapse to ONE entry, and the order is the order of first
/// appearance.
///
/// Both halves matter to the callers: the registry loops assert per DISTINCT
/// name, and every anti-vacuity floor above counts `names.len()`, so a scanner
/// that stopped deduping would inflate a floor into passing on one repeated
/// constructor.
#[test]
fn called_names_dedups_and_preserves_document_order() {
    assert_eq!(
        called_names("polygon(0mm, 0mm) ; nurbs(1, 2) ; polygon(1mm, 1mm) ; nurbs(3, 4)"),
        vec!["polygon".to_string(), "nurbs".to_string()]
    );
}

/// A call appearing ONLY inside a `//` comment is absent once the input is
/// comment-stripped.
///
/// `called_names` documents "`text` MUST already be comment-free" as a
/// PRECONDITION, not a behaviour — it does no stripping of its own. This pins
/// the contract from the caller's side, which is how every call site above uses
/// it: `called_names(&strip_reify_comments(&section))`. The hazard is live, not
/// hypothetical — geometry.md's FORM A fence carries a commented call at exactly
/// the documented arity (see `strip_reify_comments`), and a commented-out call
/// is not a call.
#[test]
fn called_names_does_not_see_a_call_that_only_appears_in_a_comment() {
    let src = "// let old = helix(10mm, 2mm, 50mm)\nlet spine = interp(0mm, 0mm, 0mm)";
    assert_eq!(
        called_names(&strip_reify_comments(src)),
        vec!["interp".to_string()],
        "the commented `helix(` must not count once the input is stripped"
    );
    // Control: UNSTRIPPED, the scanner does see it — so the assertion above is
    // about the precondition being honoured, not about the input being inert.
    assert!(called_names(src).contains(&"helix".to_string()));
}

/// The catalogue scan reads the FIRST cell only, and takes both spellings of a
/// constructor name.
///
/// The first-column rule is what keeps "every name this asserts is real" true:
/// `n_points` here is a backticked ARGUMENT name in a later column, and feeding
/// it to a registry lookup would force an allowlist entry for a name that is not
/// a constructor at all.
#[test]
fn catalogue_table_rows_reads_the_first_cell_and_strips_a_call_form() {
    let table = "\
| Constructor | Length-semantic arguments | Stays dimensionless |
|---|---|---|
| `helix` | **all three** — `helix(radius, pitch, height)` | — |
| `nurbs` | the control points | leading `n_points` counts |
";
    assert_eq!(
        catalogue_table_rows(table),
        vec![vec!["helix".to_string()], vec!["nurbs".to_string()]],
        "the header, the delimiter row, the trailing call form and the later-column \
         `n_points` must all be absent"
    );
}

/// A cell naming two constructors yields both, and a `|`-leading line with no
/// backticked identifier yields no ROW at all.
///
/// The row count is a floor in `documented_call_names_in_the_length_section_are_
/// real_registry_entries`, so what counts as a row is load-bearing: a header or
/// delimiter line that slipped into the count would let a real catalogue row be
/// deleted while the floor stayed satisfied.
#[test]
fn catalogue_table_rows_splits_a_shared_cell_and_drops_a_rowless_line() {
    let table = "\
|---|---|
| `interp` / `bezier` | every argument |
| no backticks here | so this is not a catalogue row |
prose, not a table row at all
";
    assert_eq!(
        catalogue_table_rows(table),
        vec![vec!["interp".to_string(), "bezier".to_string()]]
    );
}
