//! Temporary diagnostic test for task 295.
//! Delete after debugging.

use reify_compiler::CompiledModule;
use reify_test_support::{make_simple_engine, mm, parse_and_compile_with_stdlib};
use reify_types::{ValueCellId};

const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/integration_full_v01.ri"
);

fn source() -> &'static str {
    static S: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    S.get_or_init(|| std::fs::read_to_string(EXAMPLE_PATH).expect("file exists"))
      .as_str()
}

fn compiled() -> &'static CompiledModule {
    static C: std::sync::OnceLock<CompiledModule> = std::sync::OnceLock::new();
    C.get_or_init(|| parse_and_compile_with_stdlib(source()))
}

#[test]
fn debug_where_guard_constraints() {
    let e = "Assembly";
    
    // Collect guarded constraint IDs from compiled module
    let compiled_module = compiled();
    let assembly_template = compiled_module.templates.iter()
        .find(|t| t.name == e)
        .expect("Assembly template should exist");
    
    eprintln!("Guarded groups count: {}", assembly_template.guarded_groups.len());
    for (i, group) in assembly_template.guarded_groups.iter().enumerate() {
        eprintln!("  Group {i}: guard_cell = {}", group.guard_value_cell);
        for (j, c) in group.constraints.iter().enumerate() {
            eprintln!("    Guarded constraint {j}: {}", c.id);
        }
    }
    
    // Build the engine and eval
    let mut engine = make_simple_engine();
    engine.eval(compiled_module);
    
    // Check the snapshot graph
    let snap = engine.snapshot().expect("snapshot should exist");
    eprintln!("\nGraph guarded_groups count: {}", snap.graph.guarded_groups.len());
    for (i, group) in snap.graph.guarded_groups.iter().enumerate() {
        eprintln!("  Graph group {i}: guard_cell = {}", group.guard_cell);
        eprintln!("    guard_cell value in snapshot: {:?}", snap.values.get(&group.guard_cell));
        for (j, c) in group.constraints.iter().enumerate() {
            eprintln!("    Graph guarded constraint {j}: {}", c);
        }
    }
    
    eprintln!("\nAll constraints in graph:");
    let mut constraint_ids: Vec<_> = snap.graph.constraints.iter().map(|(id, _)| id.clone()).collect();
    constraint_ids.sort_by_key(|id| id.index);
    for id in &constraint_ids {
        eprintln!("  {}", id);
    }
    
    // Now do edit_check
    let px_id = ValueCellId::new(e, "position_x");
    let check_result = engine.edit_check(px_id, mm(200.0)).expect("edit_check should succeed");
    
    eprintln!("\ncheck_result constraint count: {}", check_result.constraint_results.len());
    let mut result_ids: Vec<_> = check_result.constraint_results.iter().collect();
    result_ids.sort_by_key(|e| e.id.index);
    for entry in &result_ids {
        eprintln!("  {} → {:?}", entry.id, entry.satisfaction);
    }
}
