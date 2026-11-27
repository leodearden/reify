//! Scenario C6, first half (docs/prds/instances-on-the-core.md section 10):
//! every member written in a corpus source that parses is in its
//! definition's member table, with the name its form gives and a kind its
//! form allows. The oracle is the syntax tree and the map below, not the
//! lowering it checks.

use std::path::Path;

use reify_ast::{Declaration, MemberDecl};
use reify_core::{DefinitionName, ModuleName, ModulePath, PathName};
use reify_eval_source::{MemberKind, MemberOrigin, MemberTable, SourceOutput, compile_entry};

use crate::compile_step::entry_file;
use crate::corpus_tree::{self, LoadedEntry, Problem, SourceFile};
use crate::parse_check::first_parse_fault;

/// The name a written member gives itself; `None` for a form that names
/// nothing.
fn written_name(decl: &MemberDecl) -> Option<&str> {
    match decl {
        MemberDecl::Param(param) => Some(&param.name),
        MemberDecl::Let(binding) => Some(&binding.name),
        MemberDecl::Sub(sub) => Some(&sub.name),
        MemberDecl::AssociatedType(associated) => Some(&associated.name),
        MemberDecl::Fn(function) => Some(&function.name),
        MemberDecl::Port(port) => Some(&port.name),
        MemberDecl::Sketch(sketch) => Some(&sketch.name),
        MemberDecl::Constraint(_)
        | MemberDecl::ConstraintInst(_)
        | MemberDecl::Minimize(_)
        | MemberDecl::Maximize(_)
        | MemberDecl::GuardedGroup(_)
        | MemberDecl::Connect(_)
        | MemberDecl::Chain(_)
        | MemberDecl::MetaBlock(_)
        | MemberDecl::ForallConnect(_)
        | MemberDecl::ForallConstraint(_)
        | MemberDecl::MatchArmDeclGroup(_)
        | MemberDecl::Relate(_) => None,
    }
}

/// The kinds a written member's form allows.
fn allowed_kinds(decl: &MemberDecl) -> &'static [MemberKind] {
    match decl {
        MemberDecl::Param(_) | MemberDecl::Let(_) => {
            &[MemberKind::Cell, MemberKind::GeometryMember]
        }
        MemberDecl::Sub(_) => &[MemberKind::Sub, MemberKind::Collection],
        MemberDecl::Minimize(_) | MemberDecl::Maximize(_) => &[MemberKind::Objective],
        MemberDecl::Constraint(_) => &[MemberKind::Constraint],
        MemberDecl::ConstraintInst(_) => &[MemberKind::AppliedConstraintDefinition],
        MemberDecl::GuardedGroup(_) => &[MemberKind::GuardedGroup],
        MemberDecl::AssociatedType(_) => &[MemberKind::AssociatedType],
        MemberDecl::Fn(_) => &[MemberKind::AssociatedFunction],
        MemberDecl::Port(_) => &[MemberKind::Port],
        MemberDecl::Connect(_) => &[MemberKind::Connect],
        MemberDecl::Chain(_) => &[MemberKind::Chain],
        MemberDecl::MetaBlock(_) => &[MemberKind::Meta],
        MemberDecl::ForallConnect(_) => &[MemberKind::ForallConnect],
        MemberDecl::ForallConstraint(_) => &[MemberKind::ForallConstraint],
        MemberDecl::MatchArmDeclGroup(_) => &[MemberKind::MatchGroup],
        MemberDecl::Relate(_) => &[MemberKind::Relate],
        MemberDecl::Sketch(_) => &[MemberKind::Sketch],
    }
}

/// The members a written member holds as branches of its table entry: a
/// guarded group's then- and else-members, a match group's arms.
fn written_branches(decl: &MemberDecl) -> Vec<&[MemberDecl]> {
    match decl {
        MemberDecl::GuardedGroup(group) => vec![&group.members, &group.else_members],
        MemberDecl::MatchArmDeclGroup(group) => group
            .arms
            .iter()
            .map(|arm| std::slice::from_ref(arm.member.as_ref()))
            .collect(),
        _ => Vec::new(),
    }
}

/// How the table entries `table` differ from the members `written`, which
/// they lower one to one and in order.
fn correspondence_problems(
    written: &[MemberDecl],
    table: &[&reify_eval_source::Member],
) -> Vec<String> {
    if written.len() != table.len() {
        let names: Vec<Option<&str>> = written.iter().map(written_name).collect();
        let listed: Vec<(MemberKind, Option<&str>)> = table
            .iter()
            .map(|member| (member.kind, member.name.as_ref().map(PathName::as_str)))
            .collect();
        return vec![format!(
            "writes {} members {names:?}, its table lists {} {listed:?}",
            written.len(),
            table.len()
        )];
    }
    let mut problems = Vec::new();
    for (at, (decl, member)) in written.iter().zip(table).enumerate() {
        let expected_name = written_name(decl);
        let name = member.name.as_ref().map(PathName::as_str);
        if name != expected_name {
            problems.push(format!(
                "member {at} is named {name:?}, its form names {expected_name:?}"
            ));
        }
        let allowed = allowed_kinds(decl);
        if !allowed.contains(&member.kind) {
            problems.push(format!(
                "member {at} ({expected_name:?}) is a {:?}, its form allows {allowed:?}",
                member.kind
            ));
        }
        let branches = written_branches(decl);
        if branches.len() != member.branches.len() {
            problems.push(format!(
                "member {at} ({expected_name:?}) writes {} branches, its entry holds {}",
                branches.len(),
                member.branches.len()
            ));
            continue;
        }
        for (branch, (written_branch, listed_branch)) in
            branches.into_iter().zip(&member.branches).enumerate()
        {
            let listed: Vec<&reify_eval_source::Member> = listed_branch.iter().collect();
            problems.extend(
                correspondence_problems(written_branch, &listed)
                    .into_iter()
                    .map(|what| format!("in branch {branch} of member {at}: {what}")),
            );
        }
    }
    problems
}

/// How the table of a definition whose body is `written` departs from it:
/// its own members lower the body one to one, and every inherited member
/// precedes them.
fn table_problems(written: &[MemberDecl], table: &MemberTable) -> Vec<String> {
    let first_own = table
        .iter()
        .position(|member| member.origin == MemberOrigin::Own)
        .unwrap_or(table.len());
    let mut problems = Vec::new();
    if table
        .iter()
        .skip(first_own)
        .any(|member| member.origin != MemberOrigin::Own)
    {
        problems.push("an inherited member follows an own member".to_string());
    }
    let own: Vec<&reify_eval_source::Member> = table
        .iter()
        .filter(|member| member.origin == MemberOrigin::Own)
        .collect();
    problems.extend(correspondence_problems(written, &own));
    problems
}

/// The module a source file's definitions are listed under: none for the
/// entry file, else its `module` line, else its file name.
fn listed_module(
    file: &SourceFile,
    is_entry: bool,
    declared: Option<&ModulePath>,
) -> Option<ModuleName> {
    if is_entry {
        return None;
    }
    let segments: Vec<String> = match declared {
        Some(path) => path.0.clone(),
        None => vec![
            Path::new(&file.name)
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ],
    };
    let names = segments.iter().map(|segment| identifier(segment)).collect();
    Some(ModuleName::new(names).unwrap_or_else(|e| panic!("`{}` is a module: {e:?}", file.name)))
}

fn identifier(text: &str) -> PathName {
    PathName::new(text).unwrap_or_else(|e| panic!("`{text}` is an identifier: {e:?}"))
}

/// The problems of one covered file of `entry`, lowered as `output`.
fn file_problems(
    entry: &LoadedEntry,
    file: &SourceFile,
    is_entry: bool,
    output: &SourceOutput,
) -> Vec<Problem> {
    let parsed = reify_syntax::parse(&file.text, ModulePath::single("corpus_entry"));
    let module = listed_module(file, is_entry, parsed.declared_module_path.as_ref());
    let mut problems = Vec::new();
    for declaration in &parsed.declarations {
        let (written_definition, members) = match declaration {
            Declaration::Structure(def) => (&def.name, &def.members),
            Declaration::Occurrence(def) => (&def.name, &def.members),
            _ => continue,
        };
        let name = DefinitionName::new(module.clone(), identifier(written_definition));
        let subject = format!("{} `{written_definition}` of {}", entry.id, file.name);
        match output.definitions.get(&name) {
            None => problems.push(Problem::new(
                &subject,
                format!("{name:?} is not in the list"),
            )),
            Some(definition) => problems.extend(
                table_problems(members, &definition.members)
                    .into_iter()
                    .map(|what| Problem::new(&subject, what)),
            ),
        }
    }
    problems
}

#[test]
fn scenario_c6_member_in_table() {
    let root = corpus_tree::corpus_root();
    let (entries, mut problems) = corpus_tree::load_corpus(&root);
    let mut parsing = 0usize;
    let mut covered = 0usize;
    for entry in &entries {
        let parses: Vec<&SourceFile> = entry
            .sources
            .files
            .iter()
            .filter(|file| first_parse_fault(&file.text).is_none())
            .collect();
        parsing += parses.len();
        if parses.is_empty() {
            continue;
        }
        let Some(compiled) = entry_file(entry) else {
            problems.push(Problem::new(&entry.id, "has no one entry file to compile"));
            continue;
        };
        let output = match compile_entry(
            &entry.sources.import_root.join(&compiled.name),
            &compiled.text,
        ) {
            Ok(output) => output,
            Err(refusal) => {
                problems.push(Problem::new(
                    &entry.id,
                    format!(
                        "has a source that parses, but the second output refused it: {refusal:?}"
                    ),
                ));
                continue;
            }
        };
        for file in parses {
            problems.extend(file_problems(
                entry,
                file,
                file.name == compiled.name,
                &output,
            ));
            covered += 1;
        }
    }
    println!("C6: {covered} corpus files covered of {parsing} that parse");

    let listed: Vec<String> = problems.iter().map(ToString::to_string).collect();
    assert!(
        problems.is_empty(),
        "{} member-table problems in the corpus at {}:\n{}",
        problems.len(),
        root.display(),
        listed.join("\n")
    );
    assert!(parsing > 0, "some corpus file parses");
    assert_eq!(covered, parsing, "every corpus file that parses is covered");
}
