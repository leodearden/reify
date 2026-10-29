//! No chunk fence declares a `fn` under a name a compiler builtin already owns.
//!
//! A user fn named like a builtin REPLACES it for the whole module, so a reader
//! who copies such a documented declaration silently loses the builtin.
//!
//! # What is NOT established
//!
//! - **Prose code spans.** They are never read, because a chunk may document a
//!   builtin's OWN signature in `fn` notation (topology.md's `faces`).
//! - **Scope.** A trait-body `fn` member is read like a top-level one; a text
//!   scan cannot tell them apart.
//! - **Stdlib `.ri` functions and stdlib intrinsics.** They are outside
//!   `reify_compiler::is_known_builtin`'s world (#8897's).
//! - **Anything beyond the name.** Parameter and return types are not compared.

use reify_compiler::is_known_builtin;

use crate::chunk_io::{all_chunks, chunk_label, report};
use crate::module_compile::{ModuleCompile, compile_module};
use crate::written_calls::{CallSource, WrittenCall, chunk_written_calls};

/// Whether `call` is a `fn` declaration head written in a copyable FENCE — the
/// gate's one statement of what it reads.
fn is_fenced_declaration(call: &WrittenCall) -> bool {
    call.declaration
        && match call.source {
            CallSource::ReifyFence | CallSource::FragmentFence | CallSource::SchematicListing => {
                true
            }
            CallSource::ProseSpan => false,
        }
}

/// Every violation in the chunk `stem` holding `markdown`: one per fenced
/// declaration of a builtin's name, or one saying the chunk cannot be read.
fn chunk_violations(stem: &str, markdown: &str) -> Vec<String> {
    match chunk_written_calls(markdown) {
        Err(error) => vec![format!(
            "{}: {error} — so no fn declaration in it is checked. FIX: repair the markup.",
            chunk_label(stem)
        )],
        Ok(calls) => calls
            .iter()
            .filter(|call| is_fenced_declaration(call) && is_known_builtin(&call.name))
            .map(|call| shadowing_violation(stem, call))
            .collect(),
    }
}

/// The violation line for `call`, a fenced `fn` declaration in chunk `stem`
/// whose name a builtin owns.
fn shadowing_violation(stem: &str, call: &WrittenCall) -> String {
    format!(
        "{}:{} — `fn {}` is declared in a {:?} under a name a compiler builtin already owns \
         (reify_compiler::is_known_builtin). A user fn named like a builtin REPLACES it for the \
         whole module — a reader who copies this declaration silently loses the builtin. FIX: \
         rename the example off the builtin's name (functions.md's `align`, task #6890).",
        chunk_label(stem),
        call.line,
        call.name,
        call.source
    )
}

/// Everything wrong across `chunks` (`(stem, markdown)`), one line each, sorted
/// and deduped. Pure — no file I/O.
pub(crate) fn builtin_shadowing_violations(chunks: &[(&str, &str)]) -> Vec<String> {
    let mut violations: Vec<String> = chunks
        .iter()
        .flat_map(|(stem, markdown)| chunk_violations(stem, markdown))
        .collect();
    violations.sort();
    violations.dedup();
    violations
}

/// The violations of the one chunk `demo` holding `markdown`.
fn demo_violations(markdown: &str) -> Vec<String> {
    builtin_shadowing_violations(&[("demo", markdown)])
}

/// A fence tagged `tag` whose one body line, chunk line 2, declares `fn name`
/// with `clamp`'s parameter list.
fn clamp_shaped_fence(tag: &str, name: &str) -> String {
    format!("```{tag}\nfn {name}(x : Real, lo : Real, hi : Real) -> Real {{ ... }}\n```\n")
}

#[test]
fn a_fenced_fn_declared_under_a_builtin_name_is_reported_at_its_line() {
    let violations = demo_violations(&clamp_shaped_fence("reify-fragment", "clamp"));
    assert_eq!(
        violations.len(),
        1,
        "`fn clamp` in a ```reify-fragment fence is exactly one violation; got {violations:#?}"
    );
    for expected in [format!("{}:2 ", chunk_label("demo")), "clamp".to_string()] {
        assert!(
            violations[0].contains(&expected),
            "the violation must name `{expected}` — the declaration's location and its name; \
             got {}",
            violations[0]
        );
    }

    assert_eq!(
        demo_violations(&clamp_shaped_fence("reify-fragment", "bounded")),
        Vec::<String>::new(),
        "the same declaration under a name no builtin owns is clean"
    );
}

#[test]
fn every_copyable_fence_kind_is_read_and_a_rejection_fence_is_not() {
    for (tag, reported) in [
        ("reify", true),
        ("reify-fragment", true),
        ("reify-schematic", true),
        ("reify-invalid", false),
    ] {
        let violations = demo_violations(&clamp_shaped_fence(tag, "clamp"));
        assert_eq!(
            violations.len(),
            usize::from(reported),
            "a ```{tag} fence is {} by a reader, so its `fn clamp` is {}reported; got \
             {violations:#?}",
            if reported { "copied" } else { "never copied" },
            if reported { "" } else { "not " }
        );
    }
}

#[test]
fn a_call_to_a_builtin_is_not_a_declaration() {
    assert_eq!(
        demo_violations("```reify-fragment\nlet c = clamp(x, lo, hi)\n```\n"),
        Vec::<String>::new(),
        "calling `clamp` uses the builtin; only DECLARING a fn of that name replaces it"
    );
}

#[test]
fn a_prose_code_span_is_never_read() {
    assert!(
        is_known_builtin("faces"),
        "control: `faces` must be a builtin, or the case below proves nothing"
    );
    assert_eq!(
        demo_violations("Signature: `fn faces(solid: Solid) -> List<Surface>`.\n"),
        Vec::<String>::new(),
        "a prose span may document a builtin's OWN signature in `fn` notation (topology.md's \
         `faces`), so prose is outside the gate's scope"
    );
}

#[test]
fn an_unreadable_chunk_is_reported_never_skipped() {
    let violations =
        demo_violations("```reify-fragment\nfn clamp(x : Real, lo : Real, hi : Real) -> Real\n");
    assert_eq!(
        violations.len(),
        1,
        "an unterminated fence is one violation; got {violations:#?}"
    );
    assert!(
        violations[0].starts_with(&chunk_label("demo"))
            && violations[0].contains("repair the markup"),
        "an unterminated fence names its chunk and the fix; got {}",
        violations[0]
    );
}

/// The hazard this gate exists for, made executable: a user fn named like a
/// builtin REPLACES the builtin for the whole module, so a call the builtin
/// accepts stops compiling.
#[test]
fn a_user_fn_named_like_a_builtin_replaces_the_builtin_module_wide() {
    let user_clamp = "fn clamp(x : Real, lo : Real, hi : Real) -> Real {\n\
                      \x20   if x < lo then lo else if x > hi then hi else x\n\
                      }\n";
    let calling = |args: &str| format!("structure def Clamped {{\n    let c = clamp({args})\n}}\n");
    let builtin_only_call = calling("5mm, 0mm, 3mm");
    let with_user_clamp = |args: &str| format!("{user_clamp}\n{}", calling(args));

    assert!(
        matches!(compile_module(&builtin_only_call), ModuleCompile::Clean),
        "control: the bare builtin call `clamp(5mm, 0mm, 3mm)` must compile clean, or the case \
         below proves nothing"
    );
    assert!(
        matches!(
            compile_module(&with_user_clamp("5.0, 0.0, 3.0")),
            ModuleCompile::Clean
        ),
        "control: the user `fn clamp(Real, Real, Real)` must compile clean beside a call it \
         accepts, or a failure below could be the declaration's own rejection rather than a \
         replacement"
    );
    assert!(
        matches!(
            compile_module(&with_user_clamp("5mm, 0mm, 3mm")),
            ModuleCompile::SemanticErrors(_)
        ),
        "a user `fn clamp(Real, Real, Real)` must make the builtin call `clamp(5mm, 0mm, 3mm)` \
         fail: that replacement is the hazard this gate exists for. A red here means the \
         compiler stopped letting a user fn replace a builtin, so the gate's premise must be \
         revisited (#8897's planned W_UserFnShadowsBuiltin warning is specified to keep choosing \
         the user fn, so the error would stand under it)"
    );
}

/// The exact number of `fn` declaration heads the live chunks write in
/// copyable fences — the anti-vacuity floor on the scan. Re-measured by the
/// protocol stated once beside `geometry_chunk_smoke.rs`'s `MINIMUM_FN_CITES`.
const MINIMUM_FENCED_FN_DECLARATIONS: usize = 11;

/// The number of `fn` declaration heads written in copyable fences across the
/// readable chunks of `chunks` (`(stem, markdown)`).
fn fenced_declaration_count(chunks: &[(&str, &str)]) -> usize {
    chunks
        .iter()
        .filter_map(|(_, markdown)| chunk_written_calls(markdown).ok())
        .flatten()
        .filter(is_fenced_declaration)
        .count()
}

/// THE GATE over the live corpus.
#[test]
fn no_chunk_fence_declares_a_fn_named_like_a_builtin() {
    let chunks = all_chunks("the builtin-shadowing gate");
    let chunks: Vec<(&str, &str)> = chunks
        .iter()
        .map(|(stem, markdown)| (stem.as_str(), markdown.as_str()))
        .collect();
    let declarations = fenced_declaration_count(&chunks);
    assert!(
        declarations >= MINIMUM_FENCED_FN_DECLARATIONS,
        "the chunk fences declare only {declarations} fn(s), floor \
         {MINIMUM_FENCED_FN_DECLARATIONS} — the builtin-shadowing gate would be vacuous"
    );

    report(
        "chunk fences that declare a `fn` under a builtin's name, which a reader who copies \
         them silently loses",
        &builtin_shadowing_violations(&chunks),
    );
}
