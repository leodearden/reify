//! Kernel-free compile witness for the committed `.ri` sites that call
//! `rotate`, `rotate_around` or `mirror` and that no other gate compiles
//! (direction-typing ι-a, task 9646).
//!
//! `prj/` and `designs/` sit outside the corpus roots `examples_smoke` and
//! `unresolved_function_corpus_sweep` walk, and `reify check` segfaults on
//! `prj/printer_v01/printer.ri`, so without this witness a site left in a
//! retired scalar form would surface only when someone next builds the design.
//! Each file is compiled the way `reify check` compiles it, but with a mock
//! constraint checker and no kernel.

use std::path::Path;

use reify_compiler::cfg::CfgSet;
use reify_compiler::module_dag::ModuleResolver;
use reify_compiler::parse_with_stdlib;
use reify_compiler::program::{CompiledProgram, compile_program_with_parsed_entry};
use reify_compiler::{CompiledGeometryOp, PatternKind, TransformKind};
use reify_core::{ModulePath, Severity};
use reify_ir::Satisfaction;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::test_package;
use reify_test_support::workspace_root::workspace_root;

/// How many ops of each Direction form a file lowers to.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct DirectionForms {
    /// `Transform{Rotate}` carrying `axis`: `rotate(geo, dir(..), angle)`.
    rotate: usize,
    /// `Transform{RotateAround}` carrying `pivot`.
    rotate_around: usize,
    /// `Pattern{Mirror}` carrying `normal`: `mirror(geo, point3(..), dir(..))`.
    mirror: usize,
}

impl DirectionForms {
    fn covers(self, floor: DirectionForms) -> bool {
        self.rotate >= floor.rotate
            && self.rotate_around >= floor.rotate_around
            && self.mirror >= floor.mirror
    }
}

const fn floor(rotate: usize, rotate_around: usize, mirror: usize) -> DirectionForms {
    DirectionForms {
        rotate,
        rotate_around,
        mirror,
    }
}

/// Each witnessed file, workspace-relative, with the call sites counted in its
/// source as a floor. Every site lowers to at least one op, and more when a
/// later `let` builds on the one holding it, since each realization lowers its
/// dependencies' ops again. A floor drops only with a new measurement saying why.
const CORPUS: &[(&str, DirectionForms)] = &[
    ("designs/litter_tray/bottom_deck.ri", floor(4, 0, 0)),
    ("designs/litter_tray/bottom_deck_split.ri", floor(6, 0, 0)),
    ("designs/litter_tray/top_deck.ri", floor(4, 0, 0)),
    ("designs/litter_tray/top_deck_split.ri", floor(5, 0, 0)),
    ("prj/printer_v01/printer.ri", floor(3, 0, 1)),
    ("prj/printer_v01/dev_capstan.ri", floor(1, 0, 0)),
    (
        "examples/geometric_relations/tangent_roller.ri",
        floor(1, 0, 0),
    ),
    ("examples/best_practices/symmetry_mirror.ri", floor(0, 0, 1)),
    (
        "docs/prds/v0_6/fixtures/helix_sweep_dep_5342.ri",
        floor(1, 0, 0),
    ),
    (
        "tests/prd-gate/fixtures/pnrg_envelope_spline.ri",
        floor(2, 0, 0),
    ),
];

/// What one file's lowered rotate/rotate_around/mirror ops look like.
#[derive(Debug, Default)]
struct Tally {
    direction_forms: DirectionForms,
    /// One entry per op that carries none of its accepted forms' arg names,
    /// i.e. a site still lowered through a retired scalar form.
    legacy: Vec<String>,
}

/// Counts `op` if it is a Direction form, records it if it is in no accepted
/// form, and ignores every other op.
fn tally_op(op: &CompiledGeometryOp, site: &str, tally: &mut Tally) {
    let forms = &mut tally.direction_forms;
    let (call, args, direction_name, other_form_name, count) = match op {
        CompiledGeometryOp::Transform {
            kind: TransformKind::Rotate,
            args,
            ..
        } => (
            "rotate",
            args,
            "axis",
            Some("orientation"),
            &mut forms.rotate,
        ),
        CompiledGeometryOp::Transform {
            kind: TransformKind::RotateAround,
            args,
            ..
        } => (
            "rotate_around",
            args,
            "pivot",
            None,
            &mut forms.rotate_around,
        ),
        CompiledGeometryOp::Pattern {
            kind: PatternKind::Mirror,
            args,
            ..
        } => ("mirror", args, "normal", Some("plane"), &mut forms.mirror),
        _ => return,
    };
    let names: Vec<&str> = args.iter().map(|(name, _)| name.as_str()).collect();
    if names.contains(&direction_name) {
        *count += 1;
    } else if !other_form_name.is_some_and(|other| names.contains(&other)) {
        tally
            .legacy
            .push(format!("{site}: {call} lowered with {names:?}"));
    }
}

fn tally(program: &CompiledProgram) -> Tally {
    let mut tally = Tally::default();
    for template in &program.entry().templates {
        for realization in &template.realizations {
            let site = format!(
                "{}.{}",
                template.name,
                realization.name.as_deref().unwrap_or("<unnamed>")
            );
            for op in &realization.operations {
                tally_op(op, &site, &mut tally);
            }
        }
    }
    tally
}

fn compile_file(path: &Path) -> Result<CompiledProgram, String> {
    let source = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path:?}: {e}"))?;
    let stem = path
        .file_stem()
        .ok_or_else(|| format!("{path:?} has no file stem"))?
        .to_string_lossy()
        .into_owned();
    let parsed = parse_with_stdlib(&source, ModulePath::single(&stem));
    if !parsed.errors.is_empty() {
        return Err(format!("{path:?} does not parse: {:?}", parsed.errors));
    }
    let dir = path
        .parent()
        .ok_or_else(|| format!("{path:?} has no parent directory"))?;
    let resolver = ModuleResolver::new(dir, test_package::manifest_dir().join("stdlib"));
    Ok(compile_program_with_parsed_entry(
        path,
        &source,
        &parsed,
        &resolver,
        &CfgSet::default(),
        &MockConstraintChecker::new().with_default(Satisfaction::Indeterminate),
    ))
}

fn problems_in(rel: &str, floor: DirectionForms) -> Vec<String> {
    let program = match compile_file(&workspace_root().join(rel)) {
        Ok(program) => program,
        Err(why) => return vec![format!("{rel}: {why}")],
    };
    let mut problems: Vec<String> = program
        .entry_own_diagnostics()
        .filter(|d| d.severity == Severity::Error)
        .filter(|d| d.message.starts_with("rotate") || d.message.starts_with("mirror"))
        .map(|d| format!("{rel}: error: {}", d.message))
        .collect();
    let tally = tally(&program);
    problems.extend(tally.legacy.iter().map(|site| format!("{rel}: {site}")));
    if !tally.direction_forms.covers(floor) {
        problems.push(format!(
            "{rel}: lowered {:?}, below the measured floor {floor:?}",
            tally.direction_forms
        ));
    }
    problems
}

#[test]
fn every_witnessed_corpus_site_lowers_through_a_direction_form() {
    let problems: Vec<String> = CORPUS
        .iter()
        .flat_map(|(rel, floor)| problems_in(rel, *floor))
        .collect();
    assert!(
        problems.is_empty(),
        "{} problem(s) across the rotate/rotate_around/mirror corpus:\n{}",
        problems.len(),
        problems.join("\n")
    );
}
