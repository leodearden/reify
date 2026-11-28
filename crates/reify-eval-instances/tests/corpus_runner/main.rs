//! The corpus runner of docs/prds/instances-on-the-core.md section 5.8: it
//! loads every entry under tests/corpus/ and checks the corpus is valid.

mod bookkeeping;
mod compile_step;
mod corpus_tree;
mod entry;
mod evaluate_step;
mod expectation;
mod expected_value;
mod host_scenarios;
mod loader_refusal;
mod member_table_coverage;
mod needs;
mod parse_check;
mod path_text;
mod seeded;
mod validity;
