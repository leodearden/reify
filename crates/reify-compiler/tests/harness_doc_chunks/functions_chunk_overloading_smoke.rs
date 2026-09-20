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
//! structural companion that the Overloading section still shows one name at two
//! distinct arities, so "delete the offending line" cannot pass as a fix.
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

use reify_compiler::CompiledModule;
use reify_core::{Diagnostic, Severity};
use reify_test_support::compile_source_with_stdlib;

/// The centralised label every `arg_check.rs` arity rejection carries
/// (`crates/reify-compiler/src/arg_check.rs:72`). NOT universal — see
/// [`arg_count_rejections`].
const ARG_COUNT_LABEL: &str = "wrong number of arguments";

/// The arity a `"…, got {N}"` arg-count message reports, or `None` when its tail
/// is not a bare count this matcher can read.
///
/// The split is from the RIGHT and the parse is whole-tail, so `", got 1"` is
/// never read out of `", got 12"`.
fn reported_arity(message: &str) -> Option<usize> {
    message.rsplit_once(", got ")?.1.parse().ok()
}

/// Every diagnostic in `compiled` that is an argument-count rejection of `name`
/// at `arity`.
///
/// # Why the match is on the MESSAGE SHAPE, and the label is only a fallback
///
/// `arg_check.rs` does centralise the [`ARG_COUNT_LABEL`] wording, but the label
/// is NOT universal: `crates/reify-compiler/src/builtin_signatures.rs`
/// (`probe_lowering_accepted_arities`, and the doc block above it) records the
/// measurement that `geometry.rs`'s `extrude` arm pushes an arg-count error
/// carrying no label at all. A label-ONLY matcher therefore has UNSAFE polarity
/// — an unlabelled arity rejection reads as "arity accepted" and yields a false
/// GREEN, the exact silent pass this module exists to prevent. So the label
/// cannot be required.
///
/// Nor can it be a plain alternative to the arity tail: a rejection carrying the
/// label would then match at EVERY arity, and the matcher would stop
/// discriminating the one thing it is asked about. The two signals are therefore
/// layered rather than OR'd —
///
/// 1. a `"{name}() expects"` message whose tail [`reported_arity`] can read is
///    attributed to exactly that arity;
/// 2. a `"{name}() expects"` message whose tail it CANNOT read is attributed to
///    every arity if it carries the label, and to none otherwise.
///
/// Layer 2 is what keeps the failure polarity safe: an arity message this
/// matcher does not recognise can only produce a false RED that forces a human
/// look, never a false GREEN. The message shape `"{name}() expects …, got {N}"`
/// held for all 34 observable names across all three emit sites when the
/// builtin_signatures ledger was measured, so layer 2 is expected to stay
/// unreached.
fn arg_count_rejections<'a>(
    compiled: &'a CompiledModule,
    name: &str,
    arity: usize,
) -> Vec<&'a Diagnostic> {
    let prefix = format!("{name}() expects");
    compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.message.starts_with(&prefix)
                && match reported_arity(&d.message) {
                    Some(reported) => reported == arity,
                    None => d.labels.iter().any(|l| l.message == ARG_COUNT_LABEL),
                }
        })
        .collect()
}

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
///   does not truncate the run — see [`is_arrow`];
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
/// The obvious reuse would be `geometry_chunk_smoke::reify_tagged_fences`, which
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

/// Whether `c`, preceded by `prev`, is the `>` of an `->` arrow rather than a
/// closing angle bracket.
///
/// A generic bound may carry a function type — `F: Fn(A) -> B` — whose arrow
/// would otherwise close the bound early, leaving `B>(…)` where the parameter
/// list is expected, and DROPPING a well-formed declaration that balances
/// perfectly well. The same rule stops a comma nested in `Map<Fn(X) -> Y, Z>`
/// from being counted at depth 0. Stated once here so both bracket walks below
/// read the arrow identically.
fn is_arrow(prev: char, c: char) -> bool {
    c == '>' && prev == '-'
}

/// `s` with a leading balanced `open`…`close` run removed, or `s` unchanged when
/// it does not start with `open`. `None` if such a run starts but never closes.
fn skip_balanced(s: &str, open: char, close: char) -> Option<&str> {
    if !s.starts_with(open) {
        return Some(s);
    }
    let mut depth = 0usize;
    let mut prev = ' ';
    for (i, c) in s.char_indices() {
        if c == open {
            depth += 1;
        } else if c == close && !is_arrow(prev, c) {
            depth -= 1;
            if depth == 0 {
                return Some(&s[i + c.len_utf8()..]);
            }
        }
        prev = c;
    }
    None
}

/// The INTERIOR of the balanced `open`…`close` run that `s` starts with, or
/// `None` when `s` does not start with `open` or the run never closes.
fn balanced_run(s: &str, open: char, close: char) -> Option<&str> {
    let rest = s.trim_start();
    if !rest.starts_with(open) {
        return None;
    }
    let after = skip_balanced(rest, open, close)?;
    let end = rest.len() - after.len() - close.len_utf8();
    Some(&rest[open.len_utf8()..end])
}

/// The number of DEPTH-0 comma-separated parameters in a parameter-list
/// interior, tracking `<>`, `()` and `[]` so a nested argument list contributes
/// no separators, and reading `->` as an arrow rather than a bracket (see
/// [`is_arrow`]). A whitespace-only interior is 0 parameters.
fn count_parameters(inner: &str) -> usize {
    if inner.trim().is_empty() {
        return 0;
    }
    let mut depth = 0usize;
    let mut parameters = 1usize;
    let mut prev = ' ';
    for c in inner.chars() {
        match c {
            '<' | '(' | '[' => depth += 1,
            _ if is_arrow(prev, c) => {}
            '>' | ')' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => parameters += 1,
            _ => {}
        }
        prev = c;
    }
    parameters
}

/// Anti-vacuity control for every later assertion in this module: it pins that
/// [`arg_count_rejections`] really does see a builtin's arity rejection, on a
/// HARDCODED probe (never read from any chunk) whose rejected and accepted
/// arities are both known.
///
/// Without this, a matcher that silently stopped recognising arity diagnostics
/// — or a compiler that stopped arity-gating `rotate` at all — would make the
/// live-chunk gate below pass trivially, which is the one failure a guard of
/// this kind must never have.
#[test]
fn arg_count_rejection_is_detected_for_the_builtin_rotate_arity_gate() {
    let compiled = compile_source_with_stdlib(
        "module functions_chunk_probe\n\
         \n\
         structure def ArgCountControl {\n\
         \x20   let bad = rotate(1mm, 1mm, 1mm)\n\
         \x20   let good = rotate(1mm, 1mm)\n\
         }\n",
    );

    let rejected = arg_count_rejections(&compiled, "rotate", 3);
    assert_eq!(
        rejected.len(),
        1,
        "the builtin `rotate` rejects arity 3 (geometry_transform.rs's `n =>` arm), so exactly \
         one arg-count diagnostic must be matched; got {rejected:#?}"
    );
    assert_eq!(
        rejected[0].message, "rotate() expects 2 or 5 arguments, got 3",
        "the arity rejection's wording is the matcher's anchor — see arg_count_rejections"
    );
    assert_eq!(
        rejected[0].severity,
        Severity::Error,
        "an arity rejection is emitted through Diagnostic::error (arg_check.rs:71)"
    );
    assert!(
        rejected[0]
            .labels
            .iter()
            .any(|label| label.message == ARG_COUNT_LABEL),
        "this arm routes through push_labeled_arg_count_error, so it must carry the centralised \
         label (crates/reify-compiler/src/arg_check.rs:72); got {:#?}",
        rejected[0].labels
    );

    assert!(
        arg_count_rejections(&compiled, "rotate", 2).is_empty(),
        "arity 2 is one of `rotate`'s accepted arities, so the matcher must report nothing for \
         it — a matcher that fires here would make every gate below unconditionally red"
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

/// The chunk under test. Read, never written. If it moves, this const must move
/// with it — the failure mode is a loud panic on the read, never a silent skip.
const CHUNK_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reify-mcp/src/tools/chunks/functions.md"
);

/// Read [`CHUNK_PATH`], panicking loudly (never skipping) if it has moved.
fn read_chunk() -> String {
    std::fs::read_to_string(CHUNK_PATH).unwrap_or_else(|e| {
        panic!("{CHUNK_PATH} must be readable ({e}) — update CHUNK_PATH if the chunk moved")
    })
}

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
    let signatures = declared_signatures(&read_chunk());
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
            arg_count_rejections(&compiled, &signature.name, signature.arity)
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

/// The structural companion to the gate above: the chunk must still show one
/// name at two DISTINCT arities.
///
/// Without this, deleting the offending line would also turn the gate green
/// while destroying the `## Overloading` section's whole illustration.
#[test]
fn functions_chunk_still_illustrates_overloading_by_arity() {
    let signatures = declared_signatures(&read_chunk());

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
