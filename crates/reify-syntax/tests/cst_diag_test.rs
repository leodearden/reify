mod common;
use common::{find_cst_node, make_ts_parser};

fn deep_scan_node(node: tree_sitter::Node, source: &[u8], depth: usize) {
    let prefix = "  ".repeat(depth);
    eprintln!("{prefix}node: kind={:?} bytes=({},{}) is_named={} is_missing={} is_error={} has_error={} child_count={} named_child_count={}",
        node.kind(), node.start_byte(), node.end_byte(),
        node.is_named(), node.is_missing(), node.is_error(), node.has_error(),
        node.child_count(), node.named_child_count()
    );
    let mut c = node.walk();
    if c.goto_first_child() {
        loop {
            deep_scan_node(c.node(), source, depth + 1);
            if !c.goto_next_sibling() { break; }
        }
    }
}

#[test]
fn diagnose_type_arg_list_cst() {
    let source = "fn f() -> Map<Vec<,>, String> { 0 }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    let root = tree.root_node();
    eprintln!("CST: {}", root.to_sexp());
    
    // Find and deeply scan number_literal at (18,18)
    if let Some(nl) = find_cst_node(root, "number_literal") {
        if nl.start_byte() == 18 {
            eprintln!("Deep scan of number_literal@(18,18):");
            deep_scan_node(nl, source.as_bytes(), 0);
        }
    }
    
    // Deep scan the outer type_arg_list
    if let Some(outer_tal) = find_cst_node(root, "type_arg_list") {
        if outer_tal.start_byte() == 14 {
            eprintln!("Deep scan of outer type_arg_list @(14,28):");
            deep_scan_node(outer_tal, source.as_bytes(), 0);
        }
    }
    
    panic!("diagnostic end");
}
