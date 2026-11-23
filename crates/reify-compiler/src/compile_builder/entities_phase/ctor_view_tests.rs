//! In the unified source-order pass, an entity body and a fn body declared at
//! the same point construct `P()` against one ctor view: the prelude ctor view
//! overlaid by the module-local templates of every kind declared above
//! (`name_env::overlay_local_templates`, NS-P1/P3). The entity scope's member
//! checks read that same view, so a local occurrence above an entity is
//! visible to them as well.

use reify_core::{DiagnosticCode, Severity};
use reify_ir::CompiledExpr;

use crate::compile_builder::test_support::{
    compile_against_prelude, compile_src, constructed_params, fn_result_expr,
};
use crate::types::CompiledModule;

fn let_default<'m>(module: &'m CompiledModule, entity: &str, cell: &str) -> &'m CompiledExpr {
    let template = module
        .templates
        .iter()
        .find(|t| t.name == entity)
        .unwrap_or_else(|| {
            panic!(
                "expected entity `{entity}`; found templates: {:?}",
                module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
            )
        });
    let value_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == cell)
        .unwrap_or_else(|| {
            panic!(
                "expected cell `{entity}.{cell}`; found cells: {:?}",
                template
                    .value_cells
                    .iter()
                    .map(|c| &c.id.member)
                    .collect::<Vec<_>>()
            )
        });
    value_cell
        .default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("`{entity}.{cell}` has no default expression"))
}

#[test]
fn entity_and_fn_bodies_construct_p_against_one_ctor_view() {
    const PRELUDE: &str = "structure def P { param from_prelude : Real = 1.0 }\n";
    const USERS: &str = "structure def User { let x = P() }\nfn mk() -> P { P() }\n";
    let local_structure_above =
        format!("structure def P {{ param from_local : Real = 2.0 }}\n{USERS}");
    let rows: [(&str, &str, Option<&[&str]>); 3] = [
        ("prelude only", USERS, Some(&["from_prelude"])),
        (
            "local structure P above",
            &local_structure_above,
            Some(&["from_local"]),
        ),
        (
            "local occurrence P above",
            "occurrence P { let d = 1.0 }\n\
             structure def User { let x = P() }\n\
             fn mk() -> Real { P() }\n",
            None,
        ),
    ];

    for (label, source, expected) in rows {
        let module = compile_against_prelude(PRELUDE, source);
        let expected: Option<Vec<String>> =
            expected.map(|names| names.iter().map(|n| n.to_string()).collect());
        assert_eq!(
            constructed_params(let_default(&module, "User", "x")),
            expected,
            "{label}: entity body `User.x = P()`"
        );
        assert_eq!(
            constructed_params(fn_result_expr(&module, "mk")),
            expected,
            "{label}: fn body `mk() = P()`"
        );
    }
}

#[test]
fn a_sub_of_a_local_occurrence_above_gates_priv_and_unknown_members() {
    const SOURCE: &str = "occurrence Out {\n    priv param p : Real = 0\n    param q : Real = 0\n}\n\
                          structure def User {\n    sub o = Out()\n    let hidden = o.p\n    \
                          let missing = o.nope\n    let open = o.q\n}\n";
    let module = compile_src(SOURCE);
    let errors: Vec<(Option<DiagnosticCode>, Vec<Option<&str>>)> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| {
            // `get`, not indexing: a label outside SOURCE reads `None` in the diff, not a panic.
            let labelled_text = d
                .labels
                .iter()
                .map(|l| SOURCE.get(l.span.start as usize..l.span.end as usize))
                .collect();
            (d.code, labelled_text)
        })
        .collect();
    assert_eq!(
        errors,
        [
            (Some(DiagnosticCode::PrivMemberAccess), vec![Some("o.p")]),
            (
                Some(DiagnosticCode::StructureMemberNotFound),
                vec![Some("o.nope")]
            ),
        ],
        "`o.p` is gated, `o.nope` is rejected and `o.q` resolves"
    );
}
