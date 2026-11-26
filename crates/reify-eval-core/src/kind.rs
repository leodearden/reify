//! What a node kind supplies. A kind is a type, never a value: it has no
//! state of its own.

use std::future::Future;

use crate::cx::Cx;
use crate::declaration::KindId;
use crate::outcome::Value;
use crate::produced::Produced;

pub trait Kind: 'static {
    type Out: Value;

    const ID: KindId;

    /// The body, written `async fn run` by a kind.
    fn run(cx: &Cx) -> impl Future<Output = Produced<Self::Out>> + Send;
}
