//! TEMPORARY probe 2 for task 6862 — arity-acceptance observability.
use reify_test_support::compile_source_with_stdlib;

const SLOTTED: &[&str] = &[
    "center_of_mass", "moment_of_inertia", "faces_by_normal", "edges_parallel_to",
    "faces_perpendicular_to", "edges_perpendicular_to", "edges_at_height",
    "extremal_by_bbox", "extremal_by_centroid", "generate",
    "linear_pattern", "linear_pattern_2d",
];

/// Build a call at `arity`: arg0 = `b` (a box), the rest `1mm`.
fn call_text(name: &str, arity: usize) -> String {
    let mut a: Vec<String> = Vec::new();
    for i in 0..arity {
        a.push(if i == 0 { "b".into() } else { "1mm".into() });
    }
    format!("{name}({})", a.join(", "))
}

#[test]
fn probe_arity_acceptance() {
    for &name in SLOTTED {
        let mut accepted = Vec::new();
        for arity in 0usize..=12 {
            let src = format!(
                "structure def T {{\n    let b = box(50mm, 30mm, 10mm)\n    let x = {}\n}}",
                call_text(name, arity)
            );
            let c = compile_source_with_stdlib(&src);
            let needle_exact = format!("{name}() expects");
            let saw_arity_err = c.diagnostics.iter().any(|d| d.message.contains(&needle_exact));
            if !saw_arity_err {
                accepted.push(arity);
            }
        }
        println!("{name}: accepted-arity(no arity diag) = {accepted:?}");
    }
}

/// Sample the raw diagnostics for a couple of names so we can see what
/// non-arity errors look like at a rejected arity.
#[test]
fn probe_raw() {
    for (name, arity) in [("linear_pattern", 6usize), ("linear_pattern", 4), ("center_of_mass", 2), ("center_of_mass", 5), ("generate", 2), ("edges_at_height", 3), ("edges_at_height", 9)] {
        let src = format!(
            "structure def T {{\n    let b = box(50mm, 30mm, 10mm)\n    let x = {}\n}}",
            call_text(name, arity)
        );
        let c = compile_source_with_stdlib(&src);
        let msgs: Vec<String> = c.diagnostics.iter().map(|d| format!("  [{:?}] {}", d.severity, d.message)).collect();
        println!("### {name}/{arity}\n{}", msgs.join("\n"));
    }
}
