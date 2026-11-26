//! A kind as the core holds it: every kind behind one interface.

use std::any::Any;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;

use crate::cx::Cx;
use crate::declaration::Declaration;
use crate::hash::Hash128;
use crate::kind::Kind;
use crate::outcome::Erased;
use crate::produced::Produced;
use crate::recipe::Recipe;
use crate::step::Step;

pub(crate) type Body = Pin<Box<dyn Future<Output = Produced<Erased>> + Send>>;

pub(crate) trait ErasedKind: Send + Sync {
    fn start(&self, cx: Cx) -> Body;

    /// The kind and the version of its body.
    fn recipe(&self) -> Recipe;

    /// The content of `out`, a value this kind produced, if it is hashed
    /// by value.
    fn content(&self, out: &Erased) -> Option<Hash128>;

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

    fn recipe(&self) -> Recipe {
        Recipe {
            kind: K::ID,
            version: K::VERSION,
        }
    }

    fn content(&self, out: &Erased) -> Option<Hash128> {
        K::content(typed::<K>(out))
    }

    fn declares(&self, out: &Erased, step: &Step) -> Option<Declaration> {
        K::declares(typed::<K>(out), step)
    }
}

fn typed<K: Kind>(out: &Erased) -> &K::Out {
    let out: &dyn Any = out;
    out.downcast_ref::<K::Out>()
        .expect("a node's value is its kind's Out")
}
