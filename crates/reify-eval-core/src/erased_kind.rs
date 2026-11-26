//! A kind as the core holds it: every kind behind one interface.

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;

use crate::cx::Cx;
use crate::kind::Kind;
use crate::outcome::Erased;
use crate::produced::Produced;

pub(crate) type Body = Pin<Box<dyn Future<Output = Produced<Erased>> + Send>>;

pub(crate) trait ErasedKind: Send + Sync {
    fn start(&self, cx: Cx) -> Body;
}

pub(crate) struct Of<K>(PhantomData<fn() -> K>);

impl<K> Of<K> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

impl<K: Kind> ErasedKind for Of<K> {
    fn start(&self, cx: Cx) -> Body {
        Box::pin(async move { K::run(&cx).await.erase() })
    }
}
