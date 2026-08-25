use reify_core::diagnostics::DiagnosticCode;
use reify_test_support::compile_source_with_stdlib;

fn dump(label: &str, src: &str) {
    let m = compile_source_with_stdlib(src);
    let n: Vec<_> = m.diagnostics.iter()
        .filter(|d| matches!(d.code, Some(DiagnosticCode::CtorUnknownField) | Some(DiagnosticCode::CtorArity)))
        .map(|d| (d.code, d.message.clone(), d.labels.first().map(|l| (l.span.start, l.span.end))))
        .collect();
    println!("### {label}: {} ctor diags", n.len());
    for x in &n { println!("    {:?}", x); }
    let errs: Vec<_> = m.diagnostics.iter().filter(|d| d.severity == reify_core::Severity::Error).map(|d| d.message.clone()).collect();
    if !errs.is_empty() { println!("    ERRORS: {:?}", errs); }
}

#[test]
fn scratch() {
    dump("inner instantiated twice via sub", r#"module t.g
structure def W { param label : String }
structure def Inner { let z = W(labl: "x") }
structure def Root {
    sub a = Inner()
    sub b = Inner()
}
"#);
    dump("fn generic-ish reused", r#"module t.h
structure def W { param label : String }
structure def Inner { param k : Int
 let z = W(labl: "x") }
structure def Root {
    sub a = Inner(k: 1)
    sub b = Inner(k: 2)
}
"#);
}
