//! No chunk fence declares a `fn` under a name a compiler builtin already owns.

use crate::chunk_io::chunk_label;
use crate::module_compile::{ModuleCompile, compile_module};

/// Everything wrong across `chunks` (`(stem, markdown)`), one line each, sorted
/// and deduped. Pure — no file I/O.
pub(crate) fn builtin_shadowing_violations(chunks: &[(&str, &str)]) -> Vec<String> {
    let _ = chunks;
    Vec::new()
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
    let call = "structure def Clamped {\n    let c = clamp(5mm, 0mm, 3mm)\n}\n";
    let shadowing = format!(
        "fn clamp(x : Real, lo : Real, hi : Real) -> Real {{\n\
         \x20   if x < lo then lo else if x > hi then hi else x\n\
         }}\n\
         \n\
         {call}"
    );

    assert!(
        matches!(compile_module(call), ModuleCompile::Clean),
        "control: the bare builtin call `clamp(5mm, 0mm, 3mm)` must compile clean, or the case \
         below proves nothing"
    );
    assert!(
        matches!(compile_module(&shadowing), ModuleCompile::SemanticErrors(_)),
        "a user `fn clamp(Real, Real, Real)` must make the builtin call `clamp(5mm, 0mm, 3mm)` \
         fail: that replacement is the hazard this gate exists for. A red here means the \
         compiler stopped letting a user fn replace a builtin, so the gate's premise must be \
         revisited (#8897's W_UserFnShadowsBuiltin warning still chooses the user fn, so the \
         error stands under it)"
    );
}
