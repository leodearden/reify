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
        let other_store = map(&[("x", 99)]);
        let outer_bindings = map(&[("y", 2)]);
        let inner_bindings = map(&[("z", 3)]);
        let outer = ScopeChain::over(&store).open_frame(&outer_bindings);
        let opened_over = outer.chain();
        let _unrelated = ScopeChain::over(&other_store);
        let inner = opened_over.open_frame(&inner_bindings);
        assert_eq!(reads(inner.chain(), "z"), Value::Int(3));
        assert_eq!(reads(inner.chain(), "y"), Value::Int(2), "the opening chain's frame");
        assert_eq!(reads(inner.chain(), "x"), Value::Int(5), "the opening chain's reader");
        assert_eq!(reads(opened_over, "z"), Value::Undef, "a binder stays in its frame");
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
        assert_eq!(entries(&ScopeChain::over(&store).visible_values()), entries(&store));
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
