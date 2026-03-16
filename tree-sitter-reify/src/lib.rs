//! Tree-sitter grammar for the Reify CAD language.

use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_reify() -> *const ();
}

/// Returns the tree-sitter [`LanguageFn`] for Reify.
pub fn language() -> LanguageFn {
    unsafe { LanguageFn::from_raw(tree_sitter_reify) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_load_grammar() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&language().into())
            .expect("Error loading Reify grammar");
    }
}
