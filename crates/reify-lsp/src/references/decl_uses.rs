//! Binding-free collection of every use site of a DECLARATION name within one
//! parsed document — `sub _ = Name` construction sites, `constraint Name(…)`
//! instantiations and every type position; `collect_decl_name_spans` is the one
//! place that category list is written down.

#[cfg(test)]
mod tests;
