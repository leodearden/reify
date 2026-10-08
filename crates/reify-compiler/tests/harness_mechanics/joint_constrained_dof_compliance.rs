//! Constrained-DOF compliance on the six `std.kinematic` joint structures — task
//! 7163, PRD `docs/prds/v0_6/assembly-modal-connection-graph.md` §7 C2.
//!
//! Each joint kind splits its six relative DOFs into FREE DOFs (the joint's
//! motion) and CONSTRAINED DOFs. The constrained ones are grouped by symmetry,
//! and each group is one named `Option<…Stiffness> = none` field, where `none`
//! means ideal-rigid. Because only constrained groups are named, compliance on a
//! free DOF cannot be written at all.
//!
//! `PARTITION` below is the one test-side statement of that ruling. Every test
//! drives it through USER SOURCE compiled against the real stdlib, so the tests
//! pin what a user can and cannot write, and which diagnostic stops them.

use std::collections::BTreeMap;

use reify_compiler::{CompiledModule, ValueCellDecl, ValueCellKind, stdlib_loader};
use reify_core::{Diagnostic, DiagnosticCode, DimensionVector, Severity, Type};
use reify_ir::CompiledExprKind;
use reify_test_support::{collect_errors, compile_source_with_stdlib};

// ─── the partition ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Family {
    Translational,
    Rotational,
}

impl Family {
    fn literal(self) -> &'static str {
        match self {
            Family::Translational => "some(200N/um)",
            Family::Rotational => "some(5000N*m/rad^2)",
        }
    }

    fn other(self) -> Family {
        match self {
            Family::Translational => Family::Rotational,
            Family::Rotational => Family::Translational,
        }
    }

    fn dimension(self) -> DimensionVector {
        match self {
            Family::Translational => DimensionVector::TRANSLATIONAL_STIFFNESS,
            Family::Rotational => DimensionVector::ROTATIONAL_STIFFNESS,
        }
    }
}

/// One constrained-DOF compliance group: an isotropic spring over `dofs`
/// directions of one family.
struct Group {
    field: &'static str,
    family: Family,
    dofs: u32,
}

struct JointKind {
    name: &'static str,
    required_args: &'static str,
    free_rot: u32,
    free_trans: u32,
    groups: &'static [Group],
}

const fn translational(field: &'static str, dofs: u32) -> Group {
    Group {
        field,
        family: Family::Translational,
        dofs,
    }
}

const fn rotational(field: &'static str, dofs: u32) -> Group {
    Group {
        field,
        family: Family::Rotational,
        dofs,
    }
}

const PARTITION: &[JointKind] = &[
    JointKind {
        name: "Prismatic",
        required_args: "axis: vec3(1.0, 0.0, 0.0)",
        free_rot: 0,
        free_trans: 1,
        groups: &[
            translational("radial_stiffness", 2),
            rotational("tilt_stiffness", 2),
            rotational("torsional_stiffness", 1),
        ],
    },
    JointKind {
        name: "Revolute",
        required_args: "axis: vec3(0.0, 0.0, 1.0)",
        free_rot: 1,
        free_trans: 0,
        groups: &[
            translational("axial_stiffness", 1),
            translational("radial_stiffness", 2),
            rotational("tilt_stiffness", 2),
        ],
    },
    JointKind {
        name: "Cylindrical",
        required_args: "axis: vec3(0.0, 0.0, 1.0)",
        free_rot: 1,
        free_trans: 1,
        groups: &[
            translational("radial_stiffness", 2),
            rotational("tilt_stiffness", 2),
        ],
    },
    JointKind {
        name: "Planar",
        required_args: "axis_x: vec3(1.0, 0.0, 0.0), axis_y: vec3(0.0, 1.0, 0.0)",
        free_rot: 1,
        free_trans: 2,
        groups: &[
            translational("normal_stiffness", 1),
            rotational("tilt_stiffness", 2),
        ],
    },
    JointKind {
        name: "Spherical",
        required_args: "",
        free_rot: 3,
        free_trans: 0,
        groups: &[translational("translational_stiffness", 3)],
    },
    JointKind {
        name: "Fixed",
        required_args: "",
        free_rot: 0,
        free_trans: 0,
        groups: &[
            translational("translational_stiffness", 3),
            rotational("rotational_stiffness", 3),
        ],
    },
];

/// Every DOF of `kind` is free XOR in exactly one constrained group, and no
/// group is named twice.
fn assert_partition_is_complete(kind: &JointKind) {
    let constrained = |family: Family| -> u32 {
        kind.groups
            .iter()
            .filter(|g| g.family == family)
            .map(|g| g.dofs)
            .sum()
    };
    assert_eq!(
        (
            kind.free_rot + constrained(Family::Rotational),
            kind.free_trans + constrained(Family::Translational),
        ),
        (3, 3),
        "{}: free + constrained DOFs must cover exactly (3 rot, 3 trans)",
        kind.name
    );
    for (i, g) in kind.groups.iter().enumerate() {
        assert!(
            kind.groups[i + 1..].iter().all(|h| h.field != g.field),
            "{}: compliance group `{}` is named twice",
            kind.name,
            g.field
        );
    }
}

/// The compliance vocabulary — every group name across all kinds — with each
/// name's one stiffness family. A name never changes family between kinds, and
/// no name contains another, so a diagnostic message that contains a name
/// identifies the field unambiguously.
fn vocabulary() -> BTreeMap<&'static str, Family> {
    let mut vocab = BTreeMap::new();
    for g in PARTITION.iter().flat_map(|k| k.groups) {
        let family = *vocab.entry(g.field).or_insert(g.family);
        assert_eq!(
            family, g.family,
            "`{}` must name the same stiffness family on every kind",
            g.field
        );
    }
    for a in vocab.keys() {
        for b in vocab.keys() {
            assert!(
                a == b || !a.contains(b),
                "vocabulary name `{a}` contains `{b}`"
            );
        }
    }
    vocab
}

// ─── probe sources ────────────────────────────────────────────────────────────

/// `Kind(<required args>, field: literal, …)`.
fn ctor_call(kind: &JointKind, compliance: &[(&str, &str)]) -> String {
    let args: Vec<String> = std::iter::once(kind.required_args.to_owned())
        .filter(|a| !a.is_empty())
        .chain(compliance.iter().map(|(f, lit)| format!("{f}: {lit}")))
        .collect();
    format!("{}({})", kind.name, args.join(", "))
}

/// Compile a `Probe` structure holding one `let` per call.
fn compile_probe(calls: &[String]) -> CompiledModule {
    let lets: String = calls
        .iter()
        .enumerate()
        .map(|(i, call)| format!("    let j{i} = {call}\n"))
        .collect();
    compile_source_with_stdlib(&format!("structure def Probe {{\n{lets}}}\n"))
}

fn errors_with_code(module: &CompiledModule, code: DiagnosticCode) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.code == Some(code))
        .collect()
}

fn names_field(errors: &[&Diagnostic], field: &str) -> bool {
    errors.iter().any(|d| d.message.contains(field))
}

// ─── (b) every constrained group is writable ─────────────────────────────────

#[test]
fn each_joint_kind_accepts_compliance_on_every_constrained_group() {
    for kind in PARTITION {
        assert_partition_is_complete(kind);
        let every_group: Vec<(&str, &str)> = kind
            .groups
            .iter()
            .map(|g| (g.field, g.family.literal()))
            .collect();
        let module = compile_probe(&[ctor_call(kind, &every_group)]);
        let errors = collect_errors(&module.diagnostics);
        assert!(
            errors.is_empty(),
            "{}: compliance on every constrained group must compile clean; got {errors:#?}",
            kind.name
        );
    }
}

// ─── (c) each group's stiffness family is enforced ───────────────────────────

#[test]
fn constrained_dof_compliance_is_dimension_checked() {
    for kind in PARTITION {
        let calls: Vec<String> = kind
            .groups
            .iter()
            .map(|g| ctor_call(kind, &[(g.field, g.family.other().literal())]))
            .collect();
        let module = compile_probe(&calls);
        let unknown = errors_with_code(&module, DiagnosticCode::CtorUnknownField);
        assert!(
            unknown.is_empty(),
            "{}: every probed field is declared, so none may draw CtorUnknownField; got \
             {unknown:#?}",
            kind.name
        );
        let mismatches = errors_with_code(&module, DiagnosticCode::ArgTypeMismatch);
        for g in kind.groups {
            assert!(
                names_field(&mismatches, g.field),
                "{}.{}: the other stiffness family must draw ArgTypeMismatch naming the \
                 field; diagnostics: {:#?}",
                kind.name,
                g.field,
                module.diagnostics
            );
        }
    }
}

// ─── (d) nothing outside a kind's constrained set is writable ────────────────

#[test]
fn compliance_outside_a_kinds_constrained_set_is_unrepresentable() {
    let vocab = vocabulary();
    for kind in PARTITION {
        let foreign: Vec<(&str, Family)> = vocab
            .iter()
            .filter(|(name, _)| kind.groups.iter().all(|g| g.field != **name))
            .map(|(name, family)| (*name, *family))
            .collect();
        let calls: Vec<String> = foreign
            .iter()
            .map(|(name, family)| ctor_call(kind, &[(name, family.literal())]))
            .collect();
        let module = compile_probe(&calls);
        let unknown = errors_with_code(&module, DiagnosticCode::CtorUnknownField);
        for (name, _) in &foreign {
            assert!(
                names_field(&unknown, name),
                "{}.{name} is not one of its constrained groups, so it must draw \
                 CtorUnknownField; diagnostics: {:#?}",
                kind.name,
                module.diagnostics
            );
        }
    }
}

/// Coupling<P> is derived motion, not a physical joint, so it has no
/// constrained groups of its own: every vocabulary name is unknown to it. The
/// probe uses the bare `Coupling(…)` call because a type-applied call does not
/// parse in expression position.
#[test]
fn coupling_carries_no_constrained_dof_compliance() {
    let vocab = vocabulary();
    let calls: Vec<String> = vocab
        .iter()
        .map(|(name, family)| format!("Coupling({name}: {})", family.literal()))
        .collect();
    let module = compile_probe(&calls);
    let unknown = errors_with_code(&module, DiagnosticCode::CtorUnknownField);
    for name in vocab.keys() {
        assert!(
            names_field(&unknown, name),
            "Coupling.{name} must draw CtorUnknownField; diagnostics: {:#?}",
            module.diagnostics
        );
    }
}

// ─── (e) absent compliance is ideal-rigid `none` ─────────────────────────────

fn kinematic_param(structure: &str, field: &str) -> &'static ValueCellDecl {
    let module = stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/kinematic")
        .expect("stdlib should contain the std/kinematic module");
    let template = module
        .templates
        .iter()
        .find(|t| t.name == structure)
        .unwrap_or_else(|| panic!("std/kinematic should declare `structure def {structure}`"));
    template
        .value_cells
        .iter()
        .find(|vc| matches!(vc.kind, ValueCellKind::Param) && vc.id.member == field)
        .unwrap_or_else(|| panic!("{structure} should declare param `{field}`"))
}

#[test]
fn constrained_dof_compliance_defaults_to_ideal_rigid_none() {
    for kind in PARTITION {
        for g in kind.groups {
            let param = kinematic_param(kind.name, g.field);
            assert_eq!(
                param.cell_type,
                Type::Option(Box::new(Type::Scalar {
                    dimension: g.family.dimension()
                })),
                "{}.{} should be Option<{:?} stiffness>",
                kind.name,
                g.field,
                g.family
            );
            let default = param
                .default_expr
                .as_ref()
                .unwrap_or_else(|| panic!("{}.{} needs a default", kind.name, g.field));
            assert!(
                matches!(default.kind, CompiledExprKind::OptionNone),
                "{}.{} should default to `none` (ideal-rigid), got {:?}",
                kind.name,
                g.field,
                default.kind
            );
        }
    }

    let no_compliance: Vec<String> = PARTITION.iter().map(|k| ctor_call(k, &[])).collect();
    let module = compile_probe(&no_compliance);
    let errors = collect_errors(&module.diagnostics);
    assert!(
        errors.is_empty(),
        "every kind must still construct from its required args alone; got {errors:#?}"
    );
}
