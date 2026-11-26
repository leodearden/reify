//! A snapshot: the inputs one evaluation sees (contract 3). Two snapshots
//! with the same inputs are the same snapshot (contract 3.1).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use imbl::OrdMap;

use crate::content::Content;
use crate::core_id::CoreId;
use crate::hash::{Encoder, Hash128};
use crate::input::{Input, InputKey};

/// Which snapshot: the core that made it and the content of its inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SnapshotId {
    pub(crate) core: CoreId,
    inputs: InputsContent,
}

/// The content of a map of inputs: the wrapping sum of one hash per input,
/// of its key and its content, so an edit updates it change by change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct InputsContent(Hash128);

impl InputsContent {
    fn of(inputs: &OrdMap<InputKey, Input>) -> Self {
        let none = Self(Hash128::from_u128(0));
        inputs
            .iter()
            .fold(none, |content, (key, input)| content.with(key, input))
    }

    fn with(self, key: &InputKey, input: &Input) -> Self {
        let sum = self.0.as_u128().wrapping_add(Self::entry(key, input));
        Self(Hash128::from_u128(sum))
    }

    fn without(self, key: &InputKey, input: &Input) -> Self {
        let sum = self.0.as_u128().wrapping_sub(Self::entry(key, input));
        Self(Hash128::from_u128(sum))
    }

    fn entry(key: &InputKey, input: &Input) -> u128 {
        let mut encoder = Encoder::new("snapshot.input");
        key.encode(&mut encoder);
        encoder
            .hash(Content::of_input(input).hash())
            .finish()
            .as_u128()
    }
}

/// Which snapshots of one core the host holds. Every value of a snapshot,
/// whether the core or an edit made it, shares the one token its id has
/// while any value is held, so the core sees a snapshot held in any of
/// them (contract 4.4).
#[derive(Default)]
pub(crate) struct SnapshotTokens(Mutex<BTreeMap<SnapshotId, Weak<()>>>);

impl SnapshotTokens {
    /// The token every held value of snapshot `id` shares.
    fn token(&self, id: SnapshotId) -> Arc<()> {
        let mut tokens = self.lock();
        if let Some(token) = tokens.get(&id).and_then(Weak::upgrade) {
            return token;
        }
        let token = Arc::new(());
        tokens.insert(id, Arc::downgrade(&token));
        token
    }

    /// Whether the host holds snapshot `id` in any value.
    pub fn is_held(&self, id: SnapshotId) -> bool {
        self.lock()
            .get(&id)
            .is_some_and(|token| token.strong_count() > 0)
    }

    /// Forgets every snapshot the host no longer holds.
    pub fn forget_unheld(&self) {
        self.lock().retain(|_, token| token.strong_count() > 0);
    }

    /// The map; one that a panic poisoned is whole, since every change to
    /// it is a single insert or removal.
    fn lock(&self) -> MutexGuard<'_, BTreeMap<SnapshotId, Weak<()>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[derive(Clone)]
pub struct Snapshot {
    id: SnapshotId,
    inputs: OrdMap<InputKey, Input>,
    /// Shared by every value of this snapshot, so that holding one keeps
    /// the snapshot held.
    _token: Arc<()>,
    tokens: Arc<SnapshotTokens>,
}

impl Snapshot {
    pub(crate) fn new(
        core: CoreId,
        tokens: &Arc<SnapshotTokens>,
        inputs: impl IntoIterator<Item = (InputKey, Input)>,
    ) -> Self {
        let inputs: OrdMap<_, _> = inputs.into_iter().collect();
        let id = SnapshotId {
            core,
            inputs: InputsContent::of(&inputs),
        };
        Self::with_id(id, inputs, tokens)
    }

    /// The snapshot of the same core with these inputs, with each `Some` in
    /// `changed` set and each `None` removed (contract 2.4): a new snapshot
    /// unless the inputs come out equal. This snapshot is unchanged.
    pub fn edit(&self, changed: impl IntoIterator<Item = (InputKey, Option<Input>)>) -> Snapshot {
        let (mut inputs, mut content) = (self.inputs.clone(), self.id.inputs);
        for (key, input) in changed {
            if let Some(replaced) = inputs.get(&key) {
                content = content.without(&key, replaced);
            }
            match input {
                Some(input) => {
                    content = content.with(&key, &input);
                    inputs.insert(key, input);
                }
                None => {
                    inputs.remove(&key);
                }
            }
        }
        let id = SnapshotId {
            core: self.id.core,
            inputs: content,
        };
        Self::with_id(id, inputs, &self.tokens)
    }

    pub fn id(&self) -> SnapshotId {
        self.id
    }

    pub(crate) fn inputs(&self) -> &OrdMap<InputKey, Input> {
        &self.inputs
    }

    fn with_id(
        id: SnapshotId,
        inputs: OrdMap<InputKey, Input>,
        tokens: &Arc<SnapshotTokens>,
    ) -> Self {
        Self {
            id,
            inputs,
            _token: tokens.token(id),
            tokens: Arc::clone(tokens),
        }
    }
}
