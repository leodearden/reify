//! A kind as the core holds it: every kind behind one interface.

use std::any::Any;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;

use crate::cx::Cx;
use crate::declaration::Declaration;
use crate::kind::Kind;
use crate::outcome::Erased;
use crate::produced::Produced;
use crate::step::Step;

pub(crate) type Body = Pin<Box<dyn Future<Output = Produced<Erased>> + Send>>;

pub(crate) trait ErasedKind: Send + Sync {
    fn start(&self, cx: Cx) -> Body;

    /// What `out`, a value this kind produced, declares at `step`.
    fn declares(&self, out: &Erased, step: &Step) -> Option<Declaration>;
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

    fn declares(&self, out: &Erased, step: &Step) -> Option<Declaration> {
        let out: &dyn Any = out;
        K::declares(out.downcast_ref::<K::Out>()?, step)
    }
}
