//! The adoption-ratchet engine: counts the inline copies of a primitive in a
//! crate's `src/` and `tests/` and holds each count at a committed baseline.

mod health;
mod items;
mod lexer;
mod source;
