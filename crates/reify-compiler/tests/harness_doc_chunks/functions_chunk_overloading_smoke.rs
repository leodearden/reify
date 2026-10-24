//! Doc-truth gate for the example signatures served by the `functions`
//! language-reference chunk (`crates/reify-mcp/src/tools/chunks/functions.md`),
//! which `language_chunks.rs` `include_str!`s and `reify_language_reference`
//! hands to design agents verbatim.
//!
//! # The hazard
//!
//! A chunk example declares a signature and shows its call form. A reader who
//! copies the CALL FORM without the surrounding declaration gets whatever the
//! compiler resolves that bare name to — and if the name collides with a
//! builtin that rejects the documented arity, the served documentation is
//! asserting a call form the compiler refuses. That is what the `## Overloading`
//! fence did with `rotate(geometry, axis, angle)`: the builtin `rotate` accepts
//! arities 2 and 5 only, so the copied 3-argument form drew
//! `rotate() expects 2 or 5 arguments, got 3` (task #6890).
//!
//! # What is enforced
//!
//! Every `fn`-declaration signature written anywhere in the chunk, called BARE
//! at the arity written, draws no argument-count diagnostic — plus the
//! structural companion that THE CHUNK still declares one name at two distinct
//! arities, so "delete the offending line" cannot pass as a fix.
//!
//! Both scans are chunk-wide rather than scoped to the `## Overloading` section:
//! selecting that section would pin heading text, which this module otherwise
//! never does. What that costs is recorded below.
//!
//! # What is deliberately NOT established
//!
//! - **Argument types and dimensions.** The probe fills every slot with the
//!   placeholder `1mm`; a documented `Angle` parameter passed a `Length` is
//!   invisible here.
//! - **Argument order.** A permutation of two parameters satisfies everything
//!   asserted below.
//! - **Anything about prose.** The scan reads structure (a declared name and its
//!   parameter count) and checks it against compiler behaviour. No wording,
//!   heading text, docstring or fence tag is pinned, so this is not a
//!   doc-content meta-test.
//! - **That a documented name EXISTS.** An unknown call name in a `structure def`
//!   body compiles silently — no diagnostic at any severity (measured; also
//!   recorded at `stdlib_chunk_geometry_ops_smoke.rs`'s property 2). This module
//!   is the complement of that sibling's name-existence guard: it asks whether a
//!   name that DOES resolve accepts the documented arity, not whether it
//!   resolves at all.
//! - **That any scanned name is arity-gated AT ALL.** A name the compiler never
//!   arity-checks contributes nothing to the gate — and none of the chunk's
//!   current names is gated: probing `align`, `area`, `clamp` and `von_mises` at
//!   arities 0–6 produced ZERO `"…() expects"` diagnostics (measured). So the
//!   gate is green today because nothing it scans reaches an arity gate, which
//!   it cannot itself distinguish from green-because-clean. The `rotate` CONTROL
//!   test in `arity_gate_probe.rs` — never the live gate — is what proves the
//!   matcher still
//!   discriminates. (Mutation check: reverting the chunk's `align` to `rotate`
//!   turns the gate RED with `rotate/3 — rotate() expects 2 or 5 arguments,
//!   got 3`.)
//! - **Overloading by parameter TYPE.** [`DocSignature`] carries a name and an
//!   arity only, so the chunk's two `fn area(…) -> Scalar<Area>` declarations —
//!   the by-TYPE illustration the Overloading section opens by promising — dedup
//!   to one `area/1` entry and are guarded by nothing: deleting both lines leaves
//!   every test here green (measured). Only overloading by ARITY has a
//!   structural guard.
//! - **That the surviving overload pair sits in the Overloading section.** The
//!   structural companion scans the whole chunk, so an arity-overloaded pair
//!   added anywhere — the `## Syntax` fence, say — would hold it green even if
//!   the Overloading fence lost its own illustration entirely.
//! - **A declaration whose parameter list WRAPS across lines.** The scan is
//!   line-scoped, so such a signature is dropped and never gated; see
//!   [`declared_signatures`]'s "Known limitation". Every declaration in the
//!   chunk fits one line today.

use reify_test_support::compile_source_with_stdlib;

use crate::arity_gate_probe::arg_count_rejections;
use crate::chunk_io::{FUNCTIONS_CHUNK_PATH as CHUNK_PATH, read_chunk};
use crate::written_calls::{balanced_run, count_parameters, skip_balanced};

/// One example signature declared in the chunk: a name and the number of
/// parameters written for it. A `fn` declaration is never variadic, so a plain
/// `usize` suffices where the sibling `stdlib_chunk_geometry_ops_smoke.rs`
/// needs an `Arity` enum for its `…` table rows.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DocSignature {
    name: String,
    arity: usize,
}

/// Every `fn`-declaration signature written in `markdown`.
///
/// # Scan shape — deliberately narrow, so this can never drift into a wording pin
///
/// - only lines whose TRIMMED form starts with `"fn "`, optionally preceded by
///   `"pub "`, are considered — the chunk's own Properties list documents
///   `pub` as a legal declaration form, so a `pub fn` example must be scanned
///   like any other. A signature quoted inside prose contributes nothing and no
///   heading, bullet or sentence is ever read;
/// - the name is the run of `[A-Za-z0-9_]` immediately after `fn `;
/// - an optional generic list on the NAME is skipped by walking a balanced
///   `<…>` run from the end of the name, within which `->` is read as an arrow
///   and not as a closing bracket, so a function-typed bound (`F: Fn(A) -> B`)
///   does not truncate the run — see `written_calls::is_arrow`;
/// - the parameter list is the balanced `(…)` run that follows; its interior is
///   split on DEPTH-0 commas only, tracking `<>`, `()` and `[]`, so
///   `Tensor<2, 3, Pressure>` counts as ONE parameter;
/// - an empty or whitespace-only interior is arity 0;
/// - a line whose brackets do not balance contributes nothing rather than
///   panicking.
///
/// # Known limitation: the scan is LINE-scoped
///
/// A declaration whose parameter list WRAPS across lines is dropped, silently
/// and by construction: the opening `(` is found but its `)` sits on a later
/// line. The caller's `!is_empty()` anti-vacuity check cannot see such a PARTIAL
/// drop, so the shape is pinned as a negative row in
/// `declared_signatures_extracts_name_and_arity` and disclosed in this module's
/// header — deliberate and visible rather than incidental. Every declaration in
/// the chunk fits one line today; widening the scan is worth it only once one
/// does not.
///
/// # Why fence-agnostic and tag-agnostic
///
/// The obvious reuse would be `chunk_markdown::tagged_fence_bodies`, which
/// is already parameterised by info string. But it matches that string BYTE-
/// EXACTLY, and task #5479 will retag this very fence `reify-schematic` (its
/// `{ ... }` bodies are literal elisions that can never compile). A tag-keyed
/// scrape would go silently vacuous the moment that lands — the worst possible
/// failure for a guard whose whole job is to not pass trivially. A `fn `-prefix
/// line scan survives any retagging, needs no fence state machine, and reads
/// precisely the artefact the hazard is about: a declared example signature.
/// It is also a different extraction from anything that module offers (it scans
/// call sites and tagged fences, never declaration signatures), so this is not a
/// duplicate scraper.
///
/// Deduped and sorted, so a caller's `assert_eq!` names the exact signature.
/// Callers must anti-vacuity-check the result: a chunk restructured so its
/// examples no longer start a line with `fn ` would otherwise silently empty the
/// scan.
fn declared_signatures(markdown: &str) -> Vec<DocSignature> {
    let mut signatures: Vec<DocSignature> = markdown
        .lines()
        .filter_map(|line| {
            let rest = line.trim_start();
            let rest = rest
                .strip_prefix("pub ")
                .unwrap_or(rest)
                .strip_prefix("fn ")?;
            let name_len = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            let (name, after_name) = rest.split_at(name_len);
            if name.is_empty() {
                return None;
            }
            let after_generics = skip_balanced(after_name, '<', '>')?;
            let params = balanced_run(after_generics, '(', ')')?;
            Some(DocSignature {
                name: name.to_string(),
                arity: count_parameters(params),
            })
        })
        .collect();

    signatures.sort();
    signatures.dedup();
    signatures
}

/// The names `markdown` DECLARES with `fn` at two or more distinct arities,
/// sorted and deduped.
fn arity_overloaded_declarations(_markdown: &str) -> Vec<String> {
    Vec::new()
}

#[test]
fn arity_overloaded_declarations_counts_declarations_never_calls() {
    let declared_once_called_twice = "\
```reify-schematic
fn mover(a: A) -> G { ... }
```

```reify-fragment
let m = mover(x, y)
```
";
    assert_eq!(
        arity_overloaded_declarations(declared_once_called_twice),
        Vec::<String>::new(),
        "a CALL at a second arity is not an overload — only `fn` declarations count"
    );

    let declared_at_two_arities = "\
```reify-schematic
fn mover<G>(g: G, o: O) -> G { ... }
fn mover<G>(g: G, a: A, b: B) -> G { ... }
```
";
    assert_eq!(
        arity_overloaded_declarations(declared_at_two_arities),
        vec!["mover".to_string()],
        "one name declared at two distinct arities is overloading by arity"
    );

    let declared_by_type = "\
```reify-schematic
fn area(c: Circle) -> Scalar<Area> { ... }
fn area(r: Rect) -> Scalar<Area> { ... }
```
";
    assert_eq!(
        arity_overloaded_declarations(declared_by_type),
        Vec::<String>::new(),
        "two declarations at ONE arity overload by TYPE, not by arity"
    );
}

/// Pins the extraction rule directly, on inline markdown with no chunk
/// involved, before anything consumes it.
///
/// The genuine hazards are the bracket cases: a comma nested inside a generic
/// argument list must not split a parameter, a generic list on the NAME must not
/// be mistaken for the parameter list, and an `->` inside a function-typed bound
/// must not close that list early. All are pinned here, as are the two negative
/// cases — the prose signature that keeps this a declaration scan rather than a
/// wording pin, and the line-WRAPPED signature the scan drops by construction,
/// pinned so the limitation is deliberate rather than incidental.
#[test]
fn declared_signatures_extracts_name_and_arity() {
    let markdown = r#"
# Function Declarations

```
fn clamp(x : Real, lo : Real, hi : Real) -> Real {
    if x < lo then lo else if x > hi then hi else x
}

fn von_mises(t : Tensor<2, 3, Pressure>) -> Scalar<Pressure> {
    sqrt(0.5)
}
```

- **Type parameters supported:** `fn distance<Q: Dimension>(a: Point3<Q>, b: Point3<Q>) -> Scalar<Q>`

```
fn mover<G: Transformable>(geometry: G, axis: Vector3<Dimensionless>, angle: Angle) -> G { ... }
fn mover<G: Transformable>(geometry: G, orientation: Orientation<3>) -> G { ... }
fn clamp(x : Real, lo : Real, hi : Real) -> Real { ... }
pub fn extrude_to(profile : Surface, height : Length) -> Solid { ... }
fn apply<F: Fn(A) -> B>(f : F, a : A) -> B { ... }
fn wrapped(
    a : Length,
    b : Length,
) -> Length { ... }
```
"#;

    assert_eq!(
        declared_signatures(markdown),
        vec![
            DocSignature {
                name: "apply".to_string(),
                arity: 2
            },
            DocSignature {
                name: "clamp".to_string(),
                arity: 3
            },
            DocSignature {
                name: "extrude_to".to_string(),
                arity: 2
            },
            DocSignature {
                name: "mover".to_string(),
                arity: 2
            },
            DocSignature {
                name: "mover".to_string(),
                arity: 3
            },
            DocSignature {
                name: "von_mises".to_string(),
                arity: 1
            },
        ],
        "the scan must: count a plain parameter list (clamp/3); NOT split on commas nested in \
         generic brackets (von_mises/1, not /3); skip a generic list on the NAME and tolerate a \
         `{{ ... }}` elision body (mover/3 and mover/2 — two DISTINCT arities under one name, \
         which is the overloading this module exists to guard); scan a `pub fn` declaration like \
         any other (extrude_to/2), since the chunk itself documents `pub` as a legal form; read \
         an `->` inside a function-typed bound as an arrow rather than the close of the NAME's \
         generic list (apply/2, not dropped); ignore a signature that merely appears inside \
         PROSE rather than starting its line (`distance` must be absent, so this stays a \
         declaration scan and never a wording pin); DROP a line-WRAPPED declaration whose \
         parameter list never closes on its own line (`wrapped` must be absent — a known, \
         disclosed limitation of the line-scoped scan, pinned here so it cannot drift into an \
         accident); and collapse a duplicate declaration to one sorted entry"
    );
}

// ── The live chunk ───────────────────────────────────────────────────────────

/// THE GATE. Every example signature the chunk declares, called BARE at the
/// arity written, must draw no argument-count diagnostic.
///
/// This is the copy-the-call-form path: a reader who lifts a documented call out
/// of an example without its declaration resolves the bare name against the
/// builtins, and a documented arity a same-named builtin rejects makes the
/// served documentation assert something the compiler refuses.
///
/// One compile covers the whole scanned set — `builtin_signatures.rs`'s
/// `probe_lowering_accepted_arities` measured a 660-call probe producing 459
/// arity diagnostics with no truncation and no diagnostic cap. Placeholder `1mm`
/// arguments suffice because the arity dispatch is reached without argument
/// resolution having to succeed.
#[test]
fn functions_chunk_example_signatures_are_never_rejected_on_arity_by_a_builtin() {
    let signatures = declared_signatures(&read_chunk(CHUNK_PATH));
    assert!(
        !signatures.is_empty(),
        "no `fn` declaration was scanned out of {CHUNK_PATH} — a restructured chunk must be RED \
         here, not trivially green; fix `declared_signatures` to read the chunk's new shape"
    );

    let mut src = String::from("module functions_chunk_probe\n\nstructure def SignatureProbe {\n");
    for (i, signature) in signatures.iter().enumerate() {
        let args = vec!["1mm"; signature.arity].join(", ");
        src.push_str(&format!("    let v{i} = {}({args})\n", signature.name));
    }
    src.push_str("}\n");
    let compiled = compile_source_with_stdlib(&src);

    let offenders: Vec<String> = signatures
        .iter()
        .flat_map(|signature| {
            arg_count_rejections(&compiled.diagnostics, &signature.name, signature.arity)
                .into_iter()
                .map(move |d| format!("  {}/{} — {}", signature.name, signature.arity, d.message))
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "{CHUNK_PATH} documents (name, arity) form(s) that a real builtin of the same name \
         REJECTS on arity:\n{}\n\nA reader who copies the CALL FORM out of the example without \
         its declaration resolves that bare name to the builtin, so the chunk is asserting a \
         call the compiler refuses. Remedy: rename the illustrative example to a name that does \
         not collide with a builtin. Do NOT add a caveat telling the reader to notice the \
         collision — noticing before copying is the exact failure mode.",
        offenders.join("\n")
    );
}

/// The structural companion to the gate above: the chunk must still declare one
/// name at two DISTINCT arities.
///
/// Without this, deleting the offending line would also turn the gate green
/// while destroying the `## Overloading` section's whole illustration.
///
/// The scan is chunk-wide, not section-scoped — see the module header's "NOT
/// established" list for what that leaves unguarded (an overload pair added to
/// any other section would hold this green) and for the separate by-TYPE gap.
#[test]
fn functions_chunk_still_illustrates_overloading_by_arity() {
    let signatures = declared_signatures(&read_chunk(CHUNK_PATH));

    let overloaded: Vec<&str> = signatures
        .windows(2)
        .filter(|pair| pair[0].name == pair[1].name && pair[0].arity != pair[1].arity)
        .map(|pair| pair[0].name.as_str())
        .collect();

    assert!(
        !overloaded.is_empty(),
        "{CHUNK_PATH} no longer declares any example name at two distinct arities, so it no \
         longer illustrates overloading by arity. Scanned: {signatures:#?}"
    );
}
