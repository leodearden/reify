//! A value's content is tagged with its type (contract 5.2) even where two
//! types share a name: `std::any::type_name` is not unique, and two
//! closures written in one function are given the same one.

use reify_eval_core::{Builder, Hash128, Input, InputKey};

#[test]
fn values_of_two_types_that_share_a_type_name_never_share_a_content() {
    let first = || 1_i64;
    let second = || 2_i64;
    assert_eq!(type_name_of(&first), type_name_of(&second));
    let leaf = Hash128::of_bytes(b"leaf");
    let mut core = Builder::new().build();

    let with_first = core.snapshot([(InputKey::named("x"), Input::of(first, leaf))]);
    let with_second = core.snapshot([(InputKey::named("x"), Input::of(second, leaf))]);

    assert_ne!(with_first.id(), with_second.id());
}

fn type_name_of<T>(_: &T) -> &'static str {
    std::any::type_name::<T>()
}
