//! Kernel-free compile witness for the committed `.ri` sites that call a
//! Direction-taking builtin and that no other gate compiles: ι-a's `rotate`,
//! `rotate_around` and `mirror` (task 9646), and ι-c's `linear_pattern` and
//! `linear_pattern_2d` (task 9648) (direction-typing).
//!
//! `prj/` and `designs/` sit outside the corpus roots `examples_smoke` and
//! `unresolved_function_corpus_sweep` walk, and `reify check` segfaults on
//! `prj/printer_v01/printer.ri`, so without this witness a site left in a
//! retired scalar form would surface only when someone next builds the design.
//! Each file is compiled the way `reify check` compiles it, but with a mock
//! constraint checker and no kernel. It reads only paths whose edits run the
//! Rust gate; the `docs/` and prd-gate fixtures' call shapes are mirrored inline.

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
    /// `Pattern{Linear}` carrying `direction`:
    /// `linear_pattern(geo, dir(..), count, spacing)`.
    linear_pattern: usize,
}

impl DirectionForms {
    fn covers(self, floor: DirectionForms) -> bool {
        self.rotate >= floor.rotate
            && self.rotate_around >= floor.rotate_around
            && self.mirror >= floor.mirror
            && self.linear_pattern >= floor.linear_pattern
    }
}

const fn floor(
    rotate: usize,
    rotate_around: usize,
    mirror: usize,
    linear_pattern: usize,
) -> DirectionForms {
    DirectionForms {
        rotate,
        rotate_around,
        mirror,
        linear_pattern,
    }
}

/// Each witnessed file, workspace-relative, with the call sites counted in its
/// source as a floor. Every site lowers to at least one op, and more when a
/// later `let` builds on the one holding it, since each realization lowers its
/// dependencies' ops again. A floor drops only with a new measurement saying why.
const CORPUS: &[(&str, DirectionForms)] = &[
    ("designs/litter_tray/bottom_deck.ri", floor(4, 0, 0, 0)),
    ("designs/litter_tray/bottom_deck_split.ri", floor(6, 0, 0, 0)),
    ("designs/litter_tray/top_deck.ri", floor(4, 0, 0, 0)),
    ("designs/litter_tray/top_deck_split.ri", floor(5, 0, 0, 0)),
    ("prj/printer_v01/printer.ri", floor(3, 0, 1, 3)),
    ("prj/printer_v01/dev_capstan.ri", floor(1, 0, 0, 0)),
    (
        "examples/geometric_relations/tangent_roller.ri",
        floor(1, 0, 0, 0),
    ),
    ("examples/best_practices/symmetry_mirror.ri", floor(0, 0, 1, 0)),
];

/// Mirrors docs/prds/v0_6/fixtures/helix_sweep_dep_5342.ri's `Test` structure.
const HELIX_SWEEP_DEP_5342: &str = "structure def Test {
    let path = helix(2.15mm, 0.8mm, 10mm)
    aux let profile = translate(rotate(circle(0.4mm), dir(1.0, 0.0, 0.0), -90deg), 2.15mm, 0mm, 0mm)
    let body = sweep(profile, path)
}
";

/// Mirrors tests/prd-gate/fixtures/pnrg_envelope_spline.ri — pg-drift:allow — prose only, the source is inlined here.
const PNRG_ENVELOPE_SPLINE: &str = "module pnrg_envelope_spline

#precision(20mm)

structure PnrgSpline {
    let profile = rotate(rotate(circle(100mm), dir(1.0, 0.0, 0.0), -90deg), dir(0.0, 0.0, 1.0), -32.0053832deg)
    let g = sweep(profile, bezier(0mm, 0mm, 0mm, 500mm, 800mm, 0mm, 1000mm, 0mm, 0mm))
}
";

/// Each mirrored fixture's stem, used as its module path, with its call sites
/// inlined and counted as a floor the way `CORPUS` counts them.
const MIRRORED: &[(&str, &str, DirectionForms)] = &[
    ("helix_sweep_dep_5342", HELIX_SWEEP_DEP_5342, floor(1, 0, 0, 0)),
    ("pnrg_envelope_spline", PNRG_ENVELOPE_SPLINE, floor(2, 0, 0, 0)),
];

/// What one file's lowered Direction-taking ops look like.
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
        CompiledGeometryOp::Pattern {
            kind: PatternKind::Linear,
            args,
            ..
        } => (
            "linear_pattern",
            args,
            "direction",
            None,
            &mut forms.linear_pattern,
        ),
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
    let dir = path
        .parent()
        .ok_or_else(|| format!("{path:?} has no parent directory"))?;
    compile_source(path, &source, &stem, dir)
}

/// `entry_path` only labels the entry; the compile never opens it.
fn compile_source(
    entry_path: &Path,
    source: &str,
    stem: &str,
    resolver_dir: &Path,
) -> Result<CompiledProgram, String> {
    let parsed = parse_with_stdlib(source, ModulePath::single(stem));
    if !parsed.errors.is_empty() {
        return Err(format!(
            "{entry_path:?} does not parse: {:?}",
            parsed.errors
        ));
    }
    let resolver = ModuleResolver::new(resolver_dir, test_package::manifest_dir().join("stdlib"));
    Ok(compile_program_with_parsed_entry(
        entry_path,
        source,
        &parsed,
        &resolver,
        &CfgSet::default(),
        &MockConstraintChecker::new().with_default(Satisfaction::Indeterminate),
    ))
}

fn problems_in(
    rel: &str,
    compiled: Result<CompiledProgram, String>,
    floor: DirectionForms,
) -> Vec<String> {
    let program = match compiled {
        Ok(program) => program,
        Err(why) => return vec![format!("{rel}: {why}")],
    };
    let mut problems: Vec<String> = program
        .entry_own_diagnostics()
        .filter(|d| d.severity == Severity::Error)
        .filter(|d| {
            ["rotate", "mirror", "linear_pattern"]
                .iter()
                .any(|call| d.message.starts_with(call))
        })
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

fn assert_no_problems(corpus: &str, problems: Vec<String>) {
    assert!(
        problems.is_empty(),
        "{} problem(s) across the {corpus}:\n{}",
        problems.len(),
        problems.join("\n")
    );
}

#[test]
fn every_witnessed_corpus_site_lowers_through_a_direction_form() {
    let problems = CORPUS
        .iter()
        .flat_map(|(rel, floor)| {
            problems_in(rel, compile_file(&workspace_root().join(rel)), *floor)
        })
        .collect();
    assert_no_problems("Direction-form corpus", problems);
}

#[test]
fn every_mirrored_fixture_call_shape_lowers_through_a_direction_form() {
    let problems = MIRRORED
        .iter()
        .flat_map(|(stem, source, floor)| {
            let compiled =
                compile_source(Path::new(stem), source, stem, test_package::manifest_dir());
            problems_in(stem, compiled, *floor)
        })
        .collect();
    assert_no_problems("mirrored fixture call shapes", problems);
}
