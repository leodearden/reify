//! What a node kind supplies. A kind is a type, never a value: it has no
//! state of its own.

use std::future::Future;

use crate::cx::Cx;
use crate::declaration::{Declaration, KindId};
use crate::outcome::Value;
use crate::produced::Produced;
use crate::step::Step;

pub trait Kind: 'static {
    type Out: Value;

    const ID: KindId;

    /// The body, written `async fn run` by a kind.
    fn run(cx: &Cx) -> impl Future<Output = Produced<Self::Out>> + Send;

    /// What the child at `step` below a node whose value is `out` is
    /// (contract 7.1). A kind whose values declare no children keeps this.
    fn declares(_out: &Self::Out, _step: &Step) -> Option<Declaration> {
        None
    }
}
