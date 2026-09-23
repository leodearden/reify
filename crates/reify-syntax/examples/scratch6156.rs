// UNTRACKED SCRATCH (task 6156 measurement aid) — never commit.
// Usage: scratch6156 <file.ri>... — prints the CST and every rendered parse error.
fn main() {
    for path in std::env::args().skip(1) {
        let source = std::fs::read_to_string(&path).unwrap();
        let mut p = tree_sitter::Parser::new();
        p.set_language(&tree_sitter_reify::language().into()).unwrap();
        let tree = p.parse(&source, None).unwrap();
        println!("=== {path}\n--- source:\n{source}--- cst:");
        print_tree(tree.root_node(), 0);
        let m = reify_syntax::parse(&source, reify_core::ModulePath::single("scratch"));
        println!("--- errors ({}):", m.errors.len());
        for e in &m.errors {
            println!(
                "  {:?}  [{}..{}]",
                e.render(&source),
                e.span.start,
                e.span.end
            );
        }
        for d in &m.declarations {
            if let reify_ast::Declaration::Constraint(c) = d {
                println!(
                    "--- constraint {}: params={} predicates={}",
                    c.name,
                    c.params.len(),
                    c.predicates.len()
                );
                for pr in &c.predicates {
                    println!("    pred: {:?}", pr.kind);
                }
            }
        }
    }
}

fn print_tree(n: tree_sitter::Node, depth: usize) {
    let s = n.start_position();
    let e = n.end_position();
    let tag = if n.is_missing() {
        "MISSING "
    } else if n.is_error() {
        "!! "
    } else {
        ""
    };
    if n.is_named() || n.is_missing() || n.is_error() {
        println!(
            "{}{}{} [{},{}]-[{},{}] bytes {}..{}{}",
            "  ".repeat(depth),
            tag,
            n.kind(),
            s.row,
            s.column,
            e.row,
            e.column,
            n.start_byte(),
            n.end_byte(),
            if n.has_error() { " (has_error)" } else { "" }
        );
    }
    let mut c = n.walk();
    for ch in n.children(&mut c) {
        print_tree(ch, depth + 1);
    }
}
