//! The corpus runner of docs/prds/instances-on-the-core.md section 5.8: it
//! loads every entry under tests/corpus/ and checks the corpus is valid.

mod bookkeeping;
mod corpus_tree;
mod entry;
mod expectation;
mod expected_value;
mod loader_refusal;
mod needs;
mod parse_check;
mod validity;
