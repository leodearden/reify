//! In the unified source-order pass, an entity body and a fn body declared at
//! the same point construct `P()` against one ctor view: the prelude ctor view
//! overlaid by the module-local templates of every kind declared above
//! (`name_env::overlay_local_templates`, NS-P1/P3).

use reify_ir::{CompiledExpr, CompiledExprKind};

use crate::types::CompiledModule;

fn parse_at(module: &str, source: &str) -> reify_ast::ParsedModule {
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single(module));
    assert!(
        parsed.errors.is_empty(),
        "fixture `{module}` must parse: {:?}",
        parsed.errors
    );
    parsed
}

/// `source` compiled as module `test` with module `p`, compiled from
/// `prelude_source`, as its prelude.
fn compile_against_prelude(prelude_source: &str, source: &str) -> CompiledModule {
    let prelude = crate::compile(&parse_at("p", prelude_source));
    crate::compile_with_prelude(&parse_at("test", source), &[prelude])
}

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

fn fn_result<'m>(module: &'m CompiledModule, fn_name: &str) -> &'m CompiledExpr {
    let found = module.functions.iter().find(|f| f.name == fn_name);
    &found
        .unwrap_or_else(|| {
            panic!(
                "expected fn `{fn_name}`; found functions: {:?}",
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        })
        .body
        .result_expr
}

/// The param names a structure ctor fills with defaults, or `None` when `expr`
/// constructs no structure. `CompiledExprKind` has no `PartialEq`, so the two
/// sites are compared by what they construct.
fn constructed_params(expr: &CompiledExpr) -> Option<Vec<String>> {
    match &expr.kind {
        CompiledExprKind::StructureInstanceCtor { defaults, .. } => {
            Some(defaults.iter().map(|(name, _)| name.clone()).collect())
        }
        _ => None,
    }
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
            constructed_params(fn_result(&module, "mk")),
            expected,
            "{label}: fn body `mk() = P()`"
        );
    }
}
