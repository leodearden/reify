//! The member tables of the second output (docs/prds/instances-on-the-core.md
//! section 5.2), seen through its one entry, `compile_entry`.

use std::path::Path;

use reify_core::{
    DefinitionName, DiagnosticCode, DimensionVector, ModuleName, PathName, Severity, Type,
};
use reify_eval_source::{
    DeclaredType, Member, MemberKind, MemberOrigin, MemberTable, SourceOutput, compile_entry,
};

fn name(text: &str) -> PathName {
    PathName::new(text).unwrap_or_else(|e| panic!("`{text}` is not a name: {e:?}"))
}

fn entry_definition(text: &str) -> DefinitionName {
    DefinitionName::new(None, name(text))
}

fn module_definition(dotted: &str, text: &str) -> DefinitionName {
    let module = ModuleName::new(dotted.split('.').map(name).collect())
        .unwrap_or_else(|e| panic!("`{dotted}` is not a module: {e:?}"));
    DefinitionName::new(Some(module), name(text))
}

fn accepted(source: &str) -> SourceOutput {
    compile_entry(Path::new("tables.ri"), source)
        .unwrap_or_else(|refusal| panic!("the source was refused: {refusal:?}\n{source}"))
}

fn table<'o>(output: &'o SourceOutput, definition: &str) -> &'o MemberTable {
    &output
        .definitions
        .get(&entry_definition(definition))
        .unwrap_or_else(|| panic!("`{definition}` is not in the list"))
        .members
}

fn member<'t>(table: &'t MemberTable, member: &str) -> &'t Member {
    table
        .iter()
        .find(|m| m.name.as_ref().is_some_and(|n| n.as_str() == member))
        .unwrap_or_else(|| panic!("no member `{member}` in {table:#?}"))
}

/// Each member's kind and name, in order.
fn shape(table: &MemberTable) -> Vec<(MemberKind, Option<&str>)> {
    table
        .iter()
        .map(|m| (m.kind, m.name.as_ref().map(PathName::as_str)))
        .collect()
}

fn error_codes(output: &SourceOutput) -> Vec<Option<DiagnosticCode>> {
    output
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.code)
        .collect()
}

const EVERY_MEMBER_FORM: &str = r#"
trait Flow {}
constraint def Positive {
    param v : Real
    v > 0
}
structure def Plate { param t : Length = 3mm   port face : out Flow {} }
enum Mode { A, B }
structure def Forms {
    param t : Length = 3mm
    param on : Bool = true
    param mode : Mode = Mode.A
    let w = t * 2
    constraint t > 1mm
    constraint Positive(v: 1)
    sub p : Plate
    sub ps : List<Plate>
    sub ks : Keyed<Plate> {
        "a" => { t = 1mm }
    }
    minimize t
    maximize w
    where on {
        param g : Length = 1mm
    } else {
        param h : Length = 2mm
        constraint h > 0mm
    }
    match mode {
        A => sub q : Plate,
        B => sub q : Plate
    }
    port inlet : in Flow {}
    connect p.face -> inlet
    chain p -> ps
    relate { concentric(p, p) }
    sketch profile {
        let a = point(0mm, 0mm)
    }
    meta { owner = "me" }
    forall v in ps: constraint v.t > 0mm
    forall v in ps: connect v.face -> inlet
    fn twice(self) -> Length { t * 2 }
    param z : Length = 1mm where on
}
"#;

#[test]
fn every_member_form_is_in_the_table_with_its_kind() {
    let output = accepted(EVERY_MEMBER_FORM);
    let forms = table(&output, "Forms");
    assert_eq!(
        shape(forms),
        vec![
            (MemberKind::Cell, Some("t")),
            (MemberKind::Cell, Some("on")),
            (MemberKind::Cell, Some("mode")),
            (MemberKind::Cell, Some("w")),
            (MemberKind::Constraint, None),
            (MemberKind::AppliedConstraintDefinition, None),
            (MemberKind::Sub, Some("p")),
            (MemberKind::Collection, Some("ps")),
            (MemberKind::Collection, Some("ks")),
            (MemberKind::Objective, None),
            (MemberKind::Objective, None),
            (MemberKind::GuardedGroup, None),
            (MemberKind::MatchGroup, None),
            (MemberKind::Port, Some("inlet")),
            (MemberKind::Connect, None),
            (MemberKind::Chain, None),
            (MemberKind::Relate, None),
            (MemberKind::Sketch, Some("profile")),
            (MemberKind::Meta, None),
            (MemberKind::ForallConstraint, None),
            (MemberKind::ForallConnect, None),
            (MemberKind::AssociatedFunction, Some("twice")),
            (MemberKind::Cell, Some("z")),
        ],
        "every member, once, in source order, with its kind: {forms:#?}"
    );

    let guarded: Vec<&str> = forms
        .iter()
        .filter(|m| m.guarded)
        .filter_map(|m| m.name.as_ref().map(PathName::as_str))
        .collect();
    assert_eq!(
        guarded,
        vec!["z"],
        "only `z` is written with a trailing `where`"
    );
    assert!(
        forms.iter().all(|m| m.origin == MemberOrigin::Own),
        "Forms conforms to no trait, so every member is its own"
    );

    let group = forms
        .iter()
        .find(|m| m.kind == MemberKind::GuardedGroup)
        .expect("the guarded group is listed");
    let branches: Vec<_> = group.branches.iter().map(shape).collect();
    assert_eq!(
        branches,
        vec![
            vec![(MemberKind::Cell, Some("g"))],
            vec![
                (MemberKind::Cell, Some("h")),
                (MemberKind::Constraint, None)
            ],
        ],
        "a guarded group holds its then- and else-members as two branches"
    );

    let matched = forms
        .iter()
        .find(|m| m.kind == MemberKind::MatchGroup)
        .expect("the match group is listed");
    let arms: Vec<_> = matched.branches.iter().map(shape).collect();
    assert_eq!(
        arms,
        vec![
            vec![(MemberKind::Sub, Some("q"))],
            vec![(MemberKind::Sub, Some("q"))],
        ],
        "a match group holds one branch per arm"
    );

    for leaf in ["inlet", "profile", "p", "ks"] {
        assert!(
            member(forms, leaf).branches.is_empty(),
            "what `{leaf}` holds is lowered by its own leaf, not here"
        );
    }
    let no_branches = forms
        .iter()
        .filter(|m| !matches!(m.kind, MemberKind::GuardedGroup | MemberKind::MatchGroup))
        .all(|m| m.branches.is_empty());
    assert!(no_branches, "only a guarded or match group has branches");

    let plate = table(&output, "Plate");
    assert_eq!(
        shape(plate),
        vec![
            (MemberKind::Cell, Some("t")),
            (MemberKind::Port, Some("face"))
        ]
    );
}

#[test]
fn declared_types_come_from_the_written_type() {
    let output = accepted(
        "
structure def Plate { param t : Length = 3mm }
structure def Typed {
    param t : Length = 3mm
    let w : Area = t * t
    sub p : Plate
    sub ps : List<Plate>
    sub ks : Keyed<Plate> {
        \"a\" => { t = 1mm }
    }
    sub d : DimensionalTolerance
}
",
    );
    let typed = table(&output, "Typed");
    assert_eq!(
        member(typed, "t").declared_type,
        DeclaredType::Value(Type::length())
    );
    assert_eq!(
        member(typed, "w").declared_type,
        DeclaredType::Value(Type::Scalar {
            dimension: DimensionVector::AREA
        })
    );
    assert_eq!(member(typed, "w").kind, MemberKind::Cell);
    let plate = DeclaredType::Definition(entry_definition("Plate"));
    assert_eq!(member(typed, "p").declared_type, plate);
    assert_eq!(member(typed, "p").kind, MemberKind::Sub);
    for collection in ["ps", "ks"] {
        assert_eq!(member(typed, collection).kind, MemberKind::Collection);
        assert_eq!(
            member(typed, collection).declared_type,
            plate,
            "a collection names its element definition"
        );
    }
    assert_eq!(
        member(typed, "d").declared_type,
        DeclaredType::Definition(module_definition("std.tolerancing", "DimensionalTolerance")),
        "a sub naming a stdlib definition names it under its std module"
    );
}

#[test]
fn a_sub_naming_no_definition_is_reported() {
    let output = accepted("structure def A { sub p : Nope }\n");
    let a = table(&output, "A");
    assert_eq!(
        shape(a),
        vec![(MemberKind::Sub, Some("p"))],
        "the member is kept"
    );
    assert_eq!(member(a, "p").declared_type, DeclaredType::None);
    assert!(
        error_codes(&output).contains(&Some(DiagnosticCode::UnknownSubStructure)),
        "a sub naming no definition is an UnknownSubStructure error: {:#?}",
        output.diagnostics
    );
}

#[test]
fn an_unresolved_written_type_is_reported() {
    let output = accepted("structure def A { param x : Nonsuch }\n");
    let a = table(&output, "A");
    assert_eq!(
        shape(a),
        vec![(MemberKind::Cell, Some("x"))],
        "the member is kept"
    );
    assert_eq!(
        member(a, "x").declared_type,
        DeclaredType::Value(Type::Error),
        "never a defaulted type"
    );
    assert!(
        error_codes(&output).contains(&Some(DiagnosticCode::UnresolvedType)),
        "an unresolved written type is an UnresolvedType error: {:#?}",
        output.diagnostics
    );
}

#[test]
fn an_untyped_let_has_the_type_of_its_expression() {
    let output = accepted(
        "
structure def Plate { param t : Length = 3mm }
structure def Lets {
    param t : Length = 1mm
    let w = t * 2
    let c = Plate()
    let kids = self.children
}
",
    );
    let lets = table(&output, "Lets");
    assert_eq!(member(lets, "w").kind, MemberKind::Cell);
    assert_eq!(
        member(lets, "w").declared_type,
        DeclaredType::Value(Type::length())
    );
    assert_eq!(
        member(lets, "c").declared_type,
        DeclaredType::Value(Type::StructureRef("Plate".into()))
    );
    assert_eq!(
        member(lets, "kids").declared_type,
        DeclaredType::Value(Type::List(Box::new(Type::StructureRef("Structure".into())))),
        "a structural query is typed as the new mode types it: a list of instances"
    );
}

#[test]
fn a_let_whose_expression_does_not_compile_is_reported() {
    let output = accepted("structure def A { let x = nonesuch + 1 }\n");
    let a = table(&output, "A");
    assert_eq!(
        shape(a),
        vec![(MemberKind::Cell, Some("x"))],
        "the member is kept"
    );
    assert_eq!(
        member(a, "x").declared_type,
        DeclaredType::Value(Type::Error)
    );
    assert!(
        error_codes(&output).contains(&Some(DiagnosticCode::UnresolvedName)),
        "the expression compiler's coded error is carried: {:#?}",
        output.diagnostics
    );
}

#[test]
fn a_geometry_typed_member_is_a_geometry_member_never_a_cell() {
    let output = accepted(
        "
structure def Held {
    param w : Length = 1mm
    pub let body = box(w, w, w)
}
structure def Shapes {
    param w : Length = 10mm
    param feature : Geometry = box(1mm, 1mm, 1mm)
    let body = box(w, w, w)
    pub let measured = translate(body, 0.5mm, 0mm, 0mm)
    sub held : Held
    let b = self.held.body
    let v = volume(body)
}
",
    );
    let shapes = table(&output, "Shapes");
    for geometry in ["feature", "body", "measured", "b"] {
        assert_eq!(
            member(shapes, geometry).kind,
            MemberKind::GeometryMember,
            "`{geometry}` holds geometry: {:#?}",
            member(shapes, geometry)
        );
    }
    assert_eq!(member(shapes, "v").kind, MemberKind::Cell);
    assert_eq!(member(shapes, "w").kind, MemberKind::Cell);
    assert_eq!(
        member(table(&output, "Held"), "body").kind,
        MemberKind::GeometryMember
    );
}
