//! The `functions` chunk (`crates/reify-mcp/src/tools/chunks/functions.md`)
//! still illustrates overloading by arity: it declares one name with `fn` at two
//! distinct arities. Pinned so that deleting a line cannot pass as the fix for
//! an arity violation in its `## Overloading` listing (task #6890).
//!
//! The arity truth of every form the chunk writes is `call_arity_gate.rs`'s,
//! which reads functions.md like every other chunk.
//!
//! # What is NOT established
//!
//! - **Overloading by parameter TYPE.** Declarations are compared by name and
//!   arity only, so the chunk's two `fn area(…) -> Scalar<Area>` declarations —
//!   the by-TYPE illustration the Overloading section opens by promising — are
//!   guarded by nothing.
//! - **That the overload pair sits in the Overloading section.** The scan is
//!   chunk-wide, since selecting a section would pin heading text, so an
//!   arity-overloaded pair declared anywhere holds this green.

use crate::chunk_io::{FUNCTIONS_CHUNK_PATH as CHUNK_PATH, read_chunk};
use crate::written_calls::chunk_written_calls;

/// Every `fn` declaration head `markdown` writes as copyable code, as
/// `(name, arity)`, sorted and deduped. Panics if the markup cannot be read,
/// which would otherwise hide declarations.
fn declared_forms(markdown: &str) -> Vec<(String, usize)> {
    let mut forms: Vec<(String, usize)> = chunk_written_calls(markdown)
        .unwrap_or_else(|error| panic!("{CHUNK_PATH}: {error} — repair the markup"))
        .into_iter()
        .filter(|call| call.declaration)
        .map(|call| (call.name, call.arity))
        .collect();
    forms.sort();
    forms.dedup();
    forms
}

/// The names `markdown` DECLARES with `fn` at two or more distinct arities,
/// sorted and deduped.
fn arity_overloaded_declarations(markdown: &str) -> Vec<String> {
    let mut names: Vec<String> = declared_forms(markdown)
        .windows(2)
        .filter(|pair| pair[0].0 == pair[1].0)
        .map(|pair| pair[0].0.clone())
        .collect();
    names.dedup();
    names
}

#[test]
fn arity_overloaded_declarations_counts_declarations_never_calls() {
    let declared_once_called_twice = "\
```reify-schematic
fn mover(a: A) -> G { ... }
```

```reify-fragment
let m = mover(x, y)
```
";
    assert_eq!(
        arity_overloaded_declarations(declared_once_called_twice),
        Vec::<String>::new(),
        "a CALL at a second arity is not an overload — only `fn` declarations count"
    );

    let declared_at_two_arities = "\
```reify-schematic
fn mover<G>(g: G, o: O) -> G { ... }
fn mover<G>(g: G, a: A, b: B) -> G { ... }
```
";
    assert_eq!(
        arity_overloaded_declarations(declared_at_two_arities),
        vec!["mover".to_string()],
        "one name declared at two distinct arities is overloading by arity"
    );

    let declared_by_type = "\
```reify-schematic
fn area(c: Circle) -> Scalar<Area> { ... }
fn area(r: Rect) -> Scalar<Area> { ... }
```
";
    assert_eq!(
        arity_overloaded_declarations(declared_by_type),
        Vec::<String>::new(),
        "two declarations at ONE arity overload by TYPE, not by arity"
    );
}

#[test]
fn functions_chunk_still_illustrates_overloading_by_arity() {
    let markdown = read_chunk(CHUNK_PATH);
    assert!(
        !arity_overloaded_declarations(&markdown).is_empty(),
        "{CHUNK_PATH} no longer declares any example name at two distinct arities, so it no \
         longer illustrates overloading by arity. Declarations read: {:#?}",
        declared_forms(&markdown)
    );
}
