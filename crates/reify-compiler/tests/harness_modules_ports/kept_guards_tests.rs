//! Scenario C4 (instances-on-the-core γ₂, task #10638): the guards the parser
//! now keeps change nothing the old compile produces.
//!
//! The parser keeps the `where` guard of a braces assignment, of a keyed
//! entry's assignment and of a port, and the old compile reads none of them.
//! Clearing every kept guard gives back the tree the parser produced before it
//! kept them: same source, so the same spans and content hashes, minus only
//! what it used to drop. Compiling both trees and comparing the outputs is
//! therefore the claim itself.

use std::collections::BTreeMap;

use reify_ast::{Declaration, MemberDecl, ParsedModule, WhereClause};
use reify_compiler::CompiledModule;
use reify_core::{ModulePath, Severity};

/// `thick` is false, so a compile that honoured any of the four guards would
/// produce something different from one that ignores them.
const FIXTURE: &str = r#"
trait Mount {}
structure def Plate {
    param t : Length = 1mm
    param w : Length = 1mm
}
structure def Vent {
    param w : Length = 1mm
}
structure def Asm {
    param thick : Bool = false
    sub p : Plate {
        t = 6mm where thick
        w = 3mm
    }
    sub vents : Keyed<Vent> {
        "a" => { w = 2mm where thick }
    }
    port m : in Mount where thick
    port s : Mount { direction = out } where thick
}
"#;

/// The guards `FIXTURE` writes in the three kept positions.
const FIXTURE_KEPT_GUARDS: usize = 4;

/// Empties every kept guard slot of the module's structures and returns how
/// many held a guard. The slots themselves stay, so each override still has
/// exactly one.
fn clear_kept_guards(module: &mut ParsedModule) -> usize {
    let mut cleared = 0;
    let mut clear = |slot: &mut Option<WhereClause>| {
        cleared += usize::from(slot.take().is_some());
    };
    for declaration in &mut module.declarations {
        let Declaration::Structure(structure) = declaration else {
            continue;
        };
        for member in &mut structure.members {
            match member {
                MemberDecl::Sub(sub) => {
                    sub.spec_param_override_guards
                        .iter_mut()
                        .for_each(&mut clear);
                    for entry in &mut sub.keyed_members {
                        entry.param_override_guards.iter_mut().for_each(&mut clear);
                    }
                }
                MemberDecl::Port(port) => clear(&mut port.where_clause),
                _ => {}
            }
        }
    }
    cleared
}

/// The module's full `Debug` render, with each template's two unordered
/// collections moved into ordered ones so two compiles render alike.
fn render(module: &CompiledModule) -> String {
    let mut module = module.clone();
    let ordered: Vec<_> = module
        .templates
        .iter_mut()
        .map(|template| {
            let mut controlling: Vec<_> = std::mem::take(&mut template.structure_controlling)
                .into_iter()
                .collect();
            controlling.sort();
            let meta: BTreeMap<_, _> = std::mem::take(&mut template.meta).into_iter().collect();
            (template.name.clone(), controlling, meta)
        })
        .collect();
    format!("{module:#?}\n{ordered:#?}")
}

#[test]
fn scenario_c4_kept_guards() {
    let kept = reify_syntax::parse(FIXTURE, ModulePath::single("kept_guards"));
    assert!(kept.errors.is_empty(), "parse errors: {:?}", kept.errors);

    let mut cleared = kept.clone();
    assert_eq!(
        clear_kept_guards(&mut cleared),
        FIXTURE_KEPT_GUARDS,
        "the parser must keep every guard the fixture writes"
    );

    let kept_output = reify_compiler::compile_with_stdlib(&kept);
    let cleared_output = reify_compiler::compile_with_stdlib(&cleared);
    let errors: Vec<_> = cleared_output
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "the comparison must be over a clean compile; errors: {errors:#?}"
    );

    let kept_render = render(&kept_output);
    let cleared_render = render(&cleared_output);
    assert!(
        kept_render == cleared_render,
        "keeping the guards changed the old compile's output; first differing line: {:?}",
        kept_render
            .lines()
            .zip(cleared_render.lines())
            .find(|(kept, cleared)| kept != cleared)
    );
}
