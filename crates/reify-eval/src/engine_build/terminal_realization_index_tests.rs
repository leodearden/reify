//! The edge cases of `terminal_realization_index` that no module reaches
//! through a build surface: a template with no realizations, a realization
//! cycle, and a realization that names itself.

use reify_compiler::{CompiledGeometryOp, GeomRef, TopologyTemplate};
use reify_test_support::parse_and_compile;

use super::terminal_realization_index;

fn template_named(src: &str, name: &str) -> TopologyTemplate {
    parse_and_compile(src)
        .templates
        .into_iter()
        .find(|template| template.name == name)
        .unwrap_or_else(|| panic!("fixture should compile a template named {name}"))
}

/// `S`, with `a` (S#0) a box and `b` (S#1) a pattern of `a`.
fn pattern_after_its_box() -> TopologyTemplate {
    template_named(
        "structure S {\n    \
             let a = box(2mm, 2mm, 2mm)\n    \
             let b = circular_pattern(a, axis_z(point3(0mm, 0mm, 0mm)), 4, 90deg)\n\
         }",
        "S",
    )
}

fn retarget_pattern(template: &mut TopologyTemplate, r_idx: usize, target_name: &str) {
    let retargeted = template.realizations[r_idx]
        .operations
        .iter_mut()
        .find_map(|op| match op {
            CompiledGeometryOp::Pattern { target, .. } => Some(target),
            _ => None,
        })
        .expect("premise: the realization holds a pattern op");
    *retargeted = GeomRef::Sub(target_name.to_string());
}

#[test]
fn a_template_with_no_realizations_has_no_terminal() {
    let template = template_named("structure S { param width: Length = 100mm }", "S");
    assert!(
        template.realizations.is_empty(),
        "premise: S declares no geometry"
    );

    assert_eq!(terminal_realization_index(&template), None);
}

#[test]
fn a_producer_before_its_consumer_keeps_the_declaration_last_terminal() {
    assert_eq!(
        terminal_realization_index(&pattern_after_its_box()),
        Some(1)
    );
}

#[test]
fn a_realization_naming_itself_is_not_consumed_by_that_reference() {
    let mut template = pattern_after_its_box();
    retarget_pattern(&mut template, 1, "b");

    assert_eq!(
        terminal_realization_index(&template),
        Some(1),
        "b's reference to its own name must not make b a consumed producer and hand \
         the terminal to a"
    );
}

#[test]
fn a_realization_cycle_falls_back_to_the_highest_declared_realization() {
    let mut template = pattern_after_its_box();
    let b_pattern = template.realizations[1].operations.clone();
    template.realizations[0].operations = b_pattern;
    retarget_pattern(&mut template, 0, "b");

    assert_eq!(
        terminal_realization_index(&template),
        Some(1),
        "a and b consume each other, so no realization is a sink"
    );
}
