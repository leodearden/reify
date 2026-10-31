//! One module per `reify` subcommand; `main` dispatches into them.

pub(crate) mod build;
pub(crate) mod check;
pub(crate) mod doc;
pub(crate) mod eval;
pub(crate) mod explain;
pub(crate) mod gui;
pub(crate) mod lsp;
pub(crate) mod report;
pub(crate) mod test;
