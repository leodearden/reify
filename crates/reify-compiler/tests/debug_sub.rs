#[test]
fn debug_sub_deprecation() {
    let source = r#"
        @deprecated("Use NewBolt")
        structure OldBolt { param d : Real = 1.0 }

        structure Assembly {
            sub b : OldBolt
        }
    "#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let module = reify_compiler::compile(&parsed);
    println!("Templates:");
    for t in &module.templates {
        println!("  {} annotations: {:?}", t.name, t.annotations);
    }
    println!("All diagnostics:");
    for d in &module.diagnostics {
        println!("  [{:?}] {}", d.severity, d.message);
    }
    // Just see what we get
}
