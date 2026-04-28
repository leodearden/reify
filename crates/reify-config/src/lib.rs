//! Project manifest (`reify.toml`) schema, parser, and validator.
//!
//! This crate owns the schema for the project pin described in the v0.2
//! multi-kernel PRD ("Resolved design decisions (2026-04-28)" → "Project pin").
//! It is intentionally self-contained: no other workspace crate consumes it
//! yet, but the binary entry points (CLI, GUI launcher, MCP server) and the
//! future kernel registry will read the parsed pin from here.
//!
//! See the doc comment on [`Manifest`] (added in later steps) for the on-disk
//! schema and worked examples.
