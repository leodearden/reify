//! Why the second output can refuse a program outright.

use reify_ast::ParseError;
use reify_core::{DefinitionName, ModulePath};

/// A program the second output cannot lower at all. It never yields a
/// partial list.
#[derive(Debug, Clone)]
pub enum SourceRefusal {
    /// A module of the program does not parse.
    NotParsed {
        module: ModulePath,
        errors: Vec<ParseError>,
    },
    /// A module imports a module the program did not compile.
    ImportNotCompiled {
        module: ModulePath,
        import: ModulePath,
    },
    /// The existing compile produced no template for a definition, so its
    /// names and types cannot be read.
    TemplateMissing { definition: DefinitionName },
}
