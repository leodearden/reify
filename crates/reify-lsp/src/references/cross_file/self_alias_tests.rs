//! Cross-file rename across a self-aliased import, `import parts.Hole as Hole`.
//!
//! The alias is a local binding distinct from the entity even when it is spelt
//! the same, so renaming the home entity rewrites the import's entity token and
//! leaves the alias and its uses alone: the importer still reaches the renamed
//! entity through `Hole`.

use super::*;
use crate::convert::offset_to_position;
use crate::decl_oracle::test_fixtures::occurrences;
use crate::references::test_support::apply_edits;
use reify_core::ModulePath;

const PARTS_SRC: &str = "structure Hole {\n    param diameter: Length = 10mm\n}";
const MAIN_SRC: &str = "import parts.Hole as Hole\nstructure Assembly {\n    sub hole = Hole()\n}";

#[test]
fn renaming_a_self_aliased_entity_rewrites_only_the_import_entity_token() {
    let parts_uri = Url::parse("file:///proj/parts.ri").unwrap();
    let main_uri = Url::parse("file:///proj/main.ri").unwrap();
    let docs = vec![
        (parts_uri.clone(), PARTS_SRC.to_string()),
        (main_uri.clone(), MAIN_SRC.to_string()),
    ];
    let resolver =
        |path: &str| (path == "parts").then(|| (parts_uri.clone(), PARTS_SRC.to_string()));
    let parsed_parts = reify_syntax::parse(PARTS_SRC, ModulePath::single("parts"));
    let parts_decl = occurrences(PARTS_SRC, "Hole")[0];

    let edit = compute_rename_cross_file(
        PARTS_SRC,
        &parsed_parts,
        &parts_uri,
        offset_to_position(PARTS_SRC, parts_decl as u32),
        "Bore",
        &docs,
        &resolver,
    )
    .expect("renaming the home declaration yields a WorkspaceEdit");
    let changes = edit.changes.expect("changes present");
    let main_edits = changes.get(&main_uri).expect("main.ri edits present");

    let main_after = apply_edits(MAIN_SRC, main_edits);
    assert_eq!(
        main_after,
        "import parts.Bore as Hole\nstructure Assembly {\n    sub hole = Hole()\n}"
    );
    let reparsed = reify_syntax::parse(&main_after, ModulePath::single("main"));
    assert!(
        reparsed.errors.is_empty(),
        "main.ri re-parses clean after rename: {:?}\n{main_after}",
        reparsed.errors
    );
}
