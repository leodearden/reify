//! What a node kind supplies. A kind is a type, never a value: it has no
//! state of its own.

use std::future::Future;

use crate::cx::Cx;
use crate::declaration::Declaration;
use crate::hash::Hash128;
use crate::kind_id::KindId;
use crate::outcome::Value;
use crate::produced::Produced;
use crate::step::Step;

pub trait Kind: 'static {
    type Out: Value;

    const ID: KindId;

    /// Bumped whenever the body's meaning changes: a result is found again
    /// only by the version that produced it (contract 2.1, 2.2).
    const VERSION: u32;

    /// The body, written `async fn run` by a kind.
    fn run(cx: &Cx) -> impl Future<Output = Produced<Self::Out>> + Send;

    /// The content of a result where hashing it is cheap; `None`: not
    /// hashed by value (contract 5.2). It must cover everything a reader
    /// can see of the result (O7); the core tags it with the type.
    fn content(_out: &Self::Out) -> Option<Hash128> {
        None
    }

    /// What the child at `step` below a node whose value is `out` is
    /// (contract 7.1). A kind whose values declare no children keeps this.
    fn declares(_out: &Self::Out, _step: &Step) -> Option<Declaration> {
        None
    }
}
