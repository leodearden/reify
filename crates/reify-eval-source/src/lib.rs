//! The second output of the new evaluation core: a program's definition list
//! and member tables, lowered from source. docs/prds/instances-on-the-core.md
//! section 5.2.
//!
//! The member forms are lowered from the syntax tree. Names and declared types
//! are read from the existing compile's output, which runs unchanged as the
//! resolver; its diagnostics are not forwarded, since the two lowerings
//! disagree by design on what compiles.

mod definition_list;
mod definition_scope;
mod inherited_members;
mod member_kind;
mod member_table;
mod module_context;
mod program_modules;
mod refusal;
mod stdlib_modules;

use std::path::Path;

use reify_core::Diagnostic;

pub use definition_list::{Definition, DefinitionList};
pub use member_kind::MemberKind;
pub use member_table::{DeclaredType, Member, MemberOrigin, MemberTable};
pub use refusal::SourceRefusal;

/// What the second output makes of a program.
#[derive(Debug, Clone)]
pub struct SourceOutput {
    pub definitions: DefinitionList,
    /// Every defect found in a member, each coded; the member stays in its
    /// table.
    pub diagnostics: Vec<Diagnostic>,
}

/// Lower the program rooted at `entry_path`, whose text is `entry_source`
/// (`entry_path` need not exist; imports resolve from its directory).
// G-allow: the instance kind (ε₁, #10644) and the session layer (#10551) call this; the corpus runner exercises it until then
pub fn compile_entry(entry_path: &Path, entry_source: &str) -> Result<SourceOutput, SourceRefusal> {
    let program = program_modules::ProgramModules::compile(entry_path, entry_source)?;
    let mut diagnostics = Vec::new();
    let mut definitions = program.lower(
        stdlib_modules::compiled(),
        stdlib_modules::contexts()?,
        &mut diagnostics,
    )?;
    if !program.entry_has_no_prelude() {
        let stdlib = stdlib_modules::lowered().as_ref().map_err(Clone::clone)?;
        definitions.extend(stdlib.definitions.iter().cloned());
        diagnostics.extend(stdlib.errors.iter().cloned());
    }
    Ok(SourceOutput {
        definitions: DefinitionList::new(definitions),
        diagnostics,
    })
}
