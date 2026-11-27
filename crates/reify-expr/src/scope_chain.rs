//! The chain of local scopes an evaluation reads cells through.
//!
//! A local binder (a function's params and lets, a lambda's captures and
//! params, a structure ctor's members, a match arm's binders, a quantifier's
//! element) lives in a [`Frame`]. A [`ScopeChain`] reads a cell from its
//! innermost frame outward and, past the last frame, from its [`CellReader`].
//! A frame is one of two kinds:
//! - OPEN: a miss goes on to the chain the frame was opened over. A match arm
//!   and a quantifier predicate extend the scope they appear in.
//! - SEALED: a miss is `Undef` and never reaches the reader. A function body,
//!   a lambda body and a ctor member see their own bindings only.
//!
//! A frame takes its reader and its outer link from the chain that opened it,
//! and [`Frame::chain`] is the only way in, so a frame can never be entered
//! from a chain other than the one it was opened over.

use reify_core::ValueCellId;
use reify_ir::{Value, ValueMap};

use crate::reader::CellReader;

/// The scope an expression is evaluated in: local frames, innermost first,
/// over the store's reader.
#[derive(Clone, Copy)]
pub(crate) struct ScopeChain<'a> {
    reader: &'a dyn CellReader,
    innermost: Option<&'a Frame<'a>>,
}

/// One local scope's bindings, and where a read goes when they miss.
pub(crate) struct Frame<'a> {
    bindings: &'a ValueMap,
    reader: &'a dyn CellReader,
    beyond: Beyond<'a>,
}

/// Where a read that misses a frame's bindings goes next.
enum Beyond<'a> {
    /// Nowhere: the frame is sealed, and the read is `Undef`.
    Nothing,
    /// The frame this one was opened over, or the reader when there is none.
    Outer(Option<&'a Frame<'a>>),
}

impl<'a> ScopeChain<'a> {
    /// A chain with no local frame, reading every cell from `reader`.
    pub(crate) fn over(reader: &'a dyn CellReader) -> Self {
        Self {
            reader,
            innermost: None,
        }
    }

    /// The value of `id` in this scope: the innermost frame binding it, else
    /// `Undef` at a sealed frame, else the reader's value.
    pub(crate) fn read(&self, id: &ValueCellId) -> Value {
        let mut next = self.innermost;
        while let Some(frame) = next {
            if let Some(value) = frame.bindings.get(id) {
                return value.clone();
            }
            match frame.beyond {
                Beyond::Nothing => return Value::Undef,
                Beyond::Outer(outer) => next = outer,
            }
        }
        self.reader.read(id)
    }

    /// A frame over `bindings` whose misses go on to this chain.
    pub(crate) fn open_frame<'b>(&self, bindings: &'b ValueMap) -> Frame<'b>
    where
        'a: 'b,
    {
        Frame {
            bindings,
            reader: self.reader,
            beyond: Beyond::Outer(self.innermost),
        }
    }

    /// A frame over `bindings` whose misses are `Undef`.
    pub(crate) fn sealed_frame<'b>(&self, bindings: &'b ValueMap) -> Frame<'b>
    where
        'a: 'b,
    {
        Frame {
            bindings,
            reader: self.reader,
            beyond: Beyond::Nothing,
        }
    }

    /// Every cell this scope reads, as one map: the innermost sealed frame's
    /// bindings (or, with no sealed frame, the store) with each open frame
    /// above it laid over, inner shadowing outer.
    pub(crate) fn visible_values(&self) -> ValueMap {
        let mut open_bindings = Vec::new();
        let mut next = self.innermost;
        let mut visible = loop {
            let Some(frame) = next else {
                break self.reader.as_value_map().clone();
            };
            match frame.beyond {
                Beyond::Nothing => break frame.bindings.clone(),
                Beyond::Outer(outer) => {
                    open_bindings.push(frame.bindings);
                    next = outer;
                }
            }
        };
        for bindings in open_bindings.iter().rev() {
            for (id, value) in bindings.iter() {
                visible.insert(id.clone(), value.clone());
            }
        }
        visible
    }
}

impl Frame<'_> {
    /// The chain this frame was opened over, with this frame innermost.
    pub(crate) fn chain(&self) -> ScopeChain<'_> {
        ScopeChain {
            reader: self.reader,
            innermost: Some(self),
        }
    }
}

#[cfg(test)]
mod tests {
    use reify_core::ValueCellId;
    use reify_ir::{Value, ValueMap};

    use super::ScopeChain;

    fn cell(member: &str) -> ValueCellId {
        ValueCellId::new("Probe", member)
    }

    fn map(entries: &[(&str, i64)]) -> ValueMap {
        let mut map = ValueMap::new();
        for (member, n) in entries {
            map.insert(cell(member), Value::Int(*n));
        }
        map
    }

    fn entries(map: &ValueMap) -> Vec<(ValueCellId, Value)> {
        let mut entries: Vec<_> = map.iter().map(|(id, v)| (id.clone(), v.clone())).collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries
    }

    fn reads(chain: ScopeChain<'_>, member: &str) -> Value {
        chain.read(&cell(member))
    }

    #[test]
    fn a_chain_with_no_frame_reads_through_to_the_reader() {
        let store = map(&[("x", 5)]);
        let chain = ScopeChain::over(&store);
        assert_eq!(reads(chain, "x"), Value::Int(5));
        assert_eq!(reads(chain, "absent"), Value::Undef);
    }

    #[test]
    fn an_open_frame_binding_is_read_before_the_reader() {
        let store = map(&[("x", 5)]);
        let bindings = map(&[("x", 7)]);
        let frame = ScopeChain::over(&store).open_frame(&bindings);
        assert_eq!(reads(frame.chain(), "x"), Value::Int(7));
    }

    #[test]
    fn a_miss_in_an_open_frame_goes_on_to_the_outer_frame_then_the_reader() {
        let store = map(&[("x", 5)]);
        let outer_bindings = map(&[("y", 2)]);
        let inner_bindings = map(&[("z", 3)]);
        let outer = ScopeChain::over(&store).open_frame(&outer_bindings);
        let inner = outer.chain().open_frame(&inner_bindings);
        let chain = inner.chain();
        assert_eq!(reads(chain, "z"), Value::Int(3));
        assert_eq!(reads(chain, "y"), Value::Int(2));
        assert_eq!(reads(chain, "x"), Value::Int(5));
        assert_eq!(reads(chain, "absent"), Value::Undef);
    }

    #[test]
    fn a_miss_in_a_sealed_frame_is_undef_although_the_reader_holds_the_cell() {
        let store = map(&[("x", 5)]);
        let bindings = map(&[("p", 1)]);
        let frame = ScopeChain::over(&store).sealed_frame(&bindings);
        assert_eq!(reads(frame.chain(), "p"), Value::Int(1));
        assert_eq!(reads(frame.chain(), "x"), Value::Undef);
    }

    #[test]
    fn an_open_frame_over_a_sealed_frame_sees_both_frames_and_not_the_reader() {
        let store = map(&[("x", 5)]);
        let sealed_bindings = map(&[("p", 1)]);
        let open_bindings = map(&[("v", 7)]);
        let sealed = ScopeChain::over(&store).sealed_frame(&sealed_bindings);
        let open = sealed.chain().open_frame(&open_bindings);
        let chain = open.chain();
        assert_eq!(reads(chain, "v"), Value::Int(7));
        assert_eq!(reads(chain, "p"), Value::Int(1));
        assert_eq!(reads(chain, "x"), Value::Undef);
    }

    #[test]
    fn a_frame_enters_the_chain_it_was_opened_over_and_leaves_that_chain_unchanged() {
        let store = map(&[("x", 5)]);
        let outer_bindings = map(&[("y", 2)]);
        let inner_bindings = map(&[("z", 3)]);
        let outer = ScopeChain::over(&store).open_frame(&outer_bindings);
        let opened_over = outer.chain();
        let inner = opened_over.open_frame(&inner_bindings);
        assert_eq!(reads(inner.chain(), "z"), Value::Int(3));
        assert_eq!(
            reads(inner.chain(), "y"),
            Value::Int(2),
            "the opening chain's frame"
        );
        assert_eq!(
            reads(inner.chain(), "x"),
            Value::Int(5),
            "the opening chain's reader"
        );
        assert_eq!(
            reads(opened_over, "z"),
            Value::Undef,
            "a binder stays in its frame"
        );
    }

    #[test]
    fn visible_values_with_no_sealed_frame_is_the_store_plus_the_open_frames_inner_first() {
        let store = map(&[("x", 5), ("y", 1)]);
        let outer_bindings = map(&[("y", 2), ("w", 4)]);
        let inner_bindings = map(&[("y", 3), ("z", 6)]);
        let outer = ScopeChain::over(&store).open_frame(&outer_bindings);
        let inner = outer.chain().open_frame(&inner_bindings);
        assert_eq!(
            entries(&inner.chain().visible_values()),
            entries(&map(&[("x", 5), ("y", 3), ("w", 4), ("z", 6)])),
        );
        assert_eq!(
            entries(&ScopeChain::over(&store).visible_values()),
            entries(&store)
        );
    }

    #[test]
    fn visible_values_at_or_above_a_sealed_frame_leaves_the_store_out() {
        let store = map(&[("x", 5)]);
        let sealed_bindings = map(&[("p", 1), ("q", 2)]);
        let open_bindings = map(&[("q", 9), ("v", 7)]);
        let sealed = ScopeChain::over(&store).sealed_frame(&sealed_bindings);
        let open = sealed.chain().open_frame(&open_bindings);
        assert_eq!(
            entries(&sealed.chain().visible_values()),
            entries(&sealed_bindings),
        );
        assert_eq!(
            entries(&open.chain().visible_values()),
            entries(&map(&[("p", 1), ("q", 9), ("v", 7)])),
        );
    }
}
