//! Which kind: the id a kind is registered under and every declaration of
//! it names.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KindId(pub &'static str);
