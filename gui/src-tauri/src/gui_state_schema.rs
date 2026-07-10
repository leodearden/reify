//! Schema-derived per-field GUI state sync (PRD `docs/prds/v0_6/gui-state-sync.md`
//! §8 L5, INV-GUI-1 real fix).
//!
//! Houses the `gui_state!` macro that generates `GuiState`, `StateDelta`,
//! `StateDelta::full`, a diff function, and an events function from a single
//! schema definition — a field written without a leading sync
//! classification token matches no muncher arm and is therefore a compile
//! error, making sync-drift unrepresentable.
//!
//! # Field DSL
//!
//! Each field in a `gui_state!` invocation is prefixed with exactly one
//! classification:
//!
//! - `diffed keyed(key=.., item=.., update=.., remove=.., changed=.., removed=..)`
//!   — a `Vec<T>` field keyed by a `&str`-valued member (e.g. `entity_path`).
//!   Diffed by key: an item present in `new` but absent (or changed) vs.
//!   `old` is "changed"; a key present in `old` but absent from `new` is
//!   "removed".
//! - `diffed whole(event=.., item_type=.., item_id=.., changed=..)` — a
//!   `Vec<T>` field diffed as a single unit: the whole list is "changed"
//!   when it differs from the previous snapshot (no per-item granularity).
//! - `full_reload_only("reason")` — the field is not carried on the diff
//!   channel at all; it is synced some other way (a bespoke Tauri emitter,
//!   or is observability-only), named by `reason`. The macro generates
//!   nothing for these beyond the `state` struct field itself, and asserts
//!   `reason` is non-empty at compile time.
//!
//! A field with none of these three leading tokens matches no arm in the
//! muncher below and fails to compile — this is the INV-GUI-1 enforcement
//! mechanism.
//!
//! ## Why `changed=`/`removed=` are explicit, not derived
//!
//! It would be more compact for the macro to derive the delta's "changed"
//! field name from the state field name by string concatenation (e.g.
//! `meshes` -> `changed_meshes`). Plain `macro_rules!` cannot do this on
//! stable Rust: `changed_$name` in a macro expansion lexes as two separate
//! tokens (`changed_` and whatever `$name` expands to), not one pasted
//! identifier — that requires an external crate (e.g. `paste`), which this
//! task's substrate decision rules out ("no new crate": see design
//! decision 1 in the task plan). Instead, callers name the already-composed
//! identifier explicitly, exactly as the `removed=` param and the
//! event-name/item-type params already do for irregular naming (design
//! decision 3). See esc-5034-1 for the full writeup.
//!
//! Event ordering: `events_fn` emits all keyed-update events, then all
//! keyed-removed events, then all whole-value events — three accumulation
//! passes in field-declaration order — matching the pre-L5 hand-written
//! `delta_to_events`.

use std::collections::HashMap;

/// Diff two slices of keyed items into "changed or added" and "removed key"
/// lists.
///
/// `key_fn` extracts the `&str` key used to match items between `old` and
/// `new`. An item present in `new` whose key is absent from `old`, or whose
/// value differs from the `old` item sharing its key, is "changed" (this
/// also covers added items, which have no `old` counterpart at all). A key
/// present in `old` but absent from `new` is "removed".
///
/// Mirrors the HashMap-by-key diff logic each keyed `GuiState` field
/// (meshes/values/constraints) used before L5 — lifted here so the
/// `gui_state!` macro's generated `diffed keyed` arm can call one shared,
/// independently testable implementation instead of re-emitting the loop
/// bodies per field.
pub fn diff_keyed<T, K>(old: &[T], new: &[T], key_fn: K) -> (Vec<T>, Vec<String>)
where
    T: Clone + PartialEq,
    K: Fn(&T) -> &str,
{
    let old_by_key: HashMap<&str, &T> = old.iter().map(|item| (key_fn(item), item)).collect();
    let new_by_key: HashMap<&str, &T> = new.iter().map(|item| (key_fn(item), item)).collect();

    let changed: Vec<T> = new
        .iter()
        .filter(|item| {
            old_by_key
                .get(key_fn(*item))
                .is_none_or(|old_item| *old_item != *item)
        })
        .cloned()
        .collect();

    let removed: Vec<String> = old
        .iter()
        .filter(|item| !new_by_key.contains_key(key_fn(*item)))
        .map(|item| key_fn(item).to_string())
        .collect();

    (changed, removed)
}

/// Diff two whole-value list fields as a single unit: `Some(new.to_vec())`
/// when they differ, `None` when equal.
///
/// Mirrors the whole-list diff logic each non-keyed `GuiState` list field
/// (tessellation_diagnostics, compile_diagnostics, tensegrity_wires,
/// tensegrity_surfaces, display_panes, display_appearance) used before L5 —
/// lifted here so the `gui_state!` macro's generated `diffed whole` arm can
/// call one shared, independently testable implementation.
pub fn diff_whole<T: Clone + PartialEq>(old: &[T], new: &[T]) -> Option<Vec<T>> {
    if old != new { Some(new.to_vec()) } else { None }
}

/// Generates a GUI state struct, its diff-delta struct, and the pure diff /
/// events functions between them from a single schema definition.
///
/// See the module docs for the field DSL. Invocation shape:
///
/// ```text
/// gui_state! {
///     state=StateTypeName, delta=DeltaTypeName, diff_fn=diff_fn_name, events_fn=events_fn_name;
///     <classified field>*
/// }
/// ```
///
/// # Examples
///
/// A minimal, fully-classified field compiles:
///
/// ```
/// reify_gui::gui_state! {
///     state=DocState, delta=DocDelta, diff_fn=diff_doc, events_fn=doc_events;
///     full_reload_only("documented example — synced by some other mechanism")
///     note: String,
/// }
///
/// let state = DocState { note: "hello".to_string() };
/// assert_eq!(state.note, "hello");
/// ```
///
/// A field with no leading classification token (`diffed keyed(..)` /
/// `diffed whole(..)` / `full_reload_only(..)`) matches no arm of the
/// muncher below, so it fails to compile. This is the INV-GUI-1 enforcement
/// mechanism: an unclassified `GuiState` field is unrepresentable.
///
/// ```compile_fail
/// reify_gui::gui_state! {
///     state=DocState, delta=DocDelta, diff_fn=diff_doc, events_fn=doc_events;
///     note: String,
/// }
/// ```
///
/// A `full_reload_only` field's reason must be non-empty; an empty reason
/// fails to compile via a `const` assertion:
///
/// ```compile_fail
/// reify_gui::gui_state! {
///     state=DocState, delta=DocDelta, diff_fn=diff_doc, events_fn=doc_events;
///     full_reload_only("")
///     note: String,
/// }
/// ```
// Hygiene note: `old`/`new`/`cur`/`dst`/`evs` name the diff_fn/full/events_fn
// parameters and the events accumulator local. They are threaded through
// every `@munch` step as `ident` metavariables (minted ONCE, in the entry
// arm below) rather than re-spelled as literal tokens in each arm. A
// metavariable keeps the hygiene ("syntax context") of wherever it was
// first captured as it is threaded through further macro expansions; a bare
// literal token re-typed in a *different* recursive expansion (e.g. writing
// `old` again in the field arm) would NOT resolve to the same binding as
// the `old` written in the terminal arm's function signature, even though
// both are spelled identically — each recursive call to `gui_state!` is a
// distinct expansion with its own hygiene context. Threading avoids that
// trap.
//
// `#[macro_export]`: places `gui_state!` at the crate root (`$crate::gui_state!`
// from inside the crate, `reify_gui::gui_state!` from an external crate) so the
// producer-face doctests below — compiled as a separate crate that only sees
// `reify_gui`'s public API — can invoke it. This is also required for the
// INV-GUI-1 compile-fail guarantee to be checkable from outside the crate: a
// macro that only external code can reach demonstrates the classification
// requirement isn't an internal convention callers could route around.
//
// Production call site (step-8, task #5034): `crate::types::GuiState` is
// defined via this macro, so the `#[allow(unused_macros)]` this comment used
// to note (needed while the macro was only invoked from `#[cfg(test)]`
// fixtures) is no longer necessary — a non-test library build now has a real
// call site.
#[macro_export]
macro_rules! gui_state {
    // --- Entry point: parse the header, mint the shared idents, kick off the muncher. ---
    (
        state=$state:ident, delta=$delta:ident, diff_fn=$diff_fn:ident, events_fn=$events_fn:ident;
        $($fields:tt)*
    ) => {
        $crate::gui_state! {
            @munch
            state=$state, delta=$delta, diff_fn=$diff_fn, events_fn=$events_fn,
            old=old, new=new, cur=cur, dst=dst, evs=evs,
            sfields=[], dfields=[], full=[], diffpre=[], diffbody=[],
            evtupd=[], evtrem=[], evtwhole=[], asserts=[],
            fields=[$($fields)*]
        }
    };

    // --- Terminal arm: no fields left — emit the generated items. ---
    (
        @munch
        state=$state:ident, delta=$delta:ident, diff_fn=$diff_fn:ident, events_fn=$events_fn:ident,
        old=$old:ident, new=$new:ident, cur=$cur:ident, dst=$dst:ident, evs=$evs:ident,
        sfields=[$($sfields:tt)*], dfields=[$($dfields:tt)*], full=[$($full:tt)*],
        diffpre=[$($diffpre:tt)*], diffbody=[$($diffbody:tt)*],
        evtupd=[$($evtupd:tt)*], evtrem=[$($evtrem:tt)*], evtwhole=[$($evtwhole:tt)*],
        asserts=[$($asserts:tt)*],
        fields=[]
    ) => {
        #[derive(Debug, Clone, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
        pub struct $state {
            $($sfields)*
        }

        #[derive(Debug, Clone, ::serde::Serialize, ::serde::Deserialize)]
        pub struct $delta {
            $($dfields)*
        }

        impl $delta {
            /// Create a delta containing all items from a state snapshot as
            /// changed.
            ///
            /// Used for the initial state emission where there is no
            /// previous snapshot.
            pub fn full($cur: &$state) -> Self {
                $delta {
                    $($full)*
                }
            }
        }

        /// Compare two state snapshots and return a minimal delta.
        pub fn $diff_fn($old: &$state, $new: &$state) -> $delta {
            $($diffpre)*
            $delta {
                $($diffbody)*
            }
        }

        /// Convert a delta into a list of `(event_name, payload)` tuples.
        pub fn $events_fn($dst: &$delta) -> ::std::vec::Vec<(::std::string::String, ::serde_json::Value)> {
            let mut $evs = ::std::vec::Vec::new();
            $($evtupd)*
            $($evtrem)*
            $($evtwhole)*
            $evs
        }

        $($asserts)*
    };

    // --- `diffed keyed(..)` field arm. ---
    (
        @munch
        state=$state:ident, delta=$delta:ident, diff_fn=$diff_fn:ident, events_fn=$events_fn:ident,
        old=$old:ident, new=$new:ident, cur=$cur:ident, dst=$dst:ident, evs=$evs:ident,
        sfields=[$($sfields:tt)*], dfields=[$($dfields:tt)*], full=[$($full:tt)*],
        diffpre=[$($diffpre:tt)*], diffbody=[$($diffbody:tt)*],
        evtupd=[$($evtupd:tt)*], evtrem=[$($evtrem:tt)*], evtwhole=[$($evtwhole:tt)*],
        asserts=[$($asserts:tt)*],
        fields=[
            diffed keyed(key=$k:ident, item=$it:literal, update=$u:literal, remove=$rm:literal, changed=$cf:ident, removed=$rv:ident)
            $(#[$a:meta])*
            $vis:vis $name:ident : Vec<$ty:ty> ,
            $($rest:tt)*
        ]
    ) => {
        $crate::gui_state! {
            @munch
            state=$state, delta=$delta, diff_fn=$diff_fn, events_fn=$events_fn,
            old=$old, new=$new, cur=$cur, dst=$dst, evs=$evs,
            sfields=[$($sfields)* $(#[$a])* $vis $name: Vec<$ty>,],
            dfields=[$($dfields)* pub $cf: Vec<$ty>, pub $rv: ::std::vec::Vec<::std::string::String>,],
            full=[$($full)* $cf: $cur.$name.clone(), $rv: ::std::vec::Vec::new(),],
            diffpre=[$($diffpre)* let ($cf, $rv) = $crate::gui_state_schema::diff_keyed(&$old.$name, &$new.$name, |x: &$ty| x.$k.as_str());],
            diffbody=[$($diffbody)* $cf, $rv,],
            evtupd=[$($evtupd)* for __item in &$dst.$cf {
                $crate::diff::push_serialized_event(&mut $evs, $u, $it, __item.$k.as_str(), ::serde_json::to_value(__item));
            }],
            evtrem=[$($evtrem)* for __id in &$dst.$rv {
                $evs.push(($rm.to_string(), ::serde_json::Value::String(__id.clone())));
            }],
            evtwhole=[$($evtwhole)*],
            asserts=[$($asserts)*],
            fields=[$($rest)*]
        }
    };

    // --- `diffed whole(..)` field arm. ---
    (
        @munch
        state=$state:ident, delta=$delta:ident, diff_fn=$diff_fn:ident, events_fn=$events_fn:ident,
        old=$old:ident, new=$new:ident, cur=$cur:ident, dst=$dst:ident, evs=$evs:ident,
        sfields=[$($sfields:tt)*], dfields=[$($dfields:tt)*], full=[$($full:tt)*],
        diffpre=[$($diffpre:tt)*], diffbody=[$($diffbody:tt)*],
        evtupd=[$($evtupd:tt)*], evtrem=[$($evtrem:tt)*], evtwhole=[$($evtwhole:tt)*],
        asserts=[$($asserts:tt)*],
        fields=[
            diffed whole(event=$e:literal, item_type=$it:literal, item_id=$id:literal, changed=$cf:ident)
            $(#[$a:meta])*
            $vis:vis $name:ident : Vec<$ty:ty> ,
            $($rest:tt)*
        ]
    ) => {
        $crate::gui_state! {
            @munch
            state=$state, delta=$delta, diff_fn=$diff_fn, events_fn=$events_fn,
            old=$old, new=$new, cur=$cur, dst=$dst, evs=$evs,
            sfields=[$($sfields)* $(#[$a])* $vis $name: Vec<$ty>,],
            dfields=[$($dfields)* pub $cf: ::std::option::Option<::std::vec::Vec<$ty>>,],
            full=[$($full)* $cf: if $cur.$name.is_empty() { ::std::option::Option::None } else { ::std::option::Option::Some($cur.$name.clone()) },],
            diffpre=[$($diffpre)* let $cf = $crate::gui_state_schema::diff_whole(&$old.$name, &$new.$name);],
            diffbody=[$($diffbody)* $cf,],
            evtupd=[$($evtupd)*],
            evtrem=[$($evtrem)*],
            evtwhole=[$($evtwhole)* if let ::std::option::Option::Some(v) = &$dst.$cf {
                $crate::diff::push_serialized_event(&mut $evs, $e, $it, $id, ::serde_json::to_value(v));
            }],
            asserts=[$($asserts)*],
            fields=[$($rest)*]
        }
    };

    // --- `full_reload_only(..)` field arm. ---
    (
        @munch
        state=$state:ident, delta=$delta:ident, diff_fn=$diff_fn:ident, events_fn=$events_fn:ident,
        old=$old:ident, new=$new:ident, cur=$cur:ident, dst=$dst:ident, evs=$evs:ident,
        sfields=[$($sfields:tt)*], dfields=[$($dfields:tt)*], full=[$($full:tt)*],
        diffpre=[$($diffpre:tt)*], diffbody=[$($diffbody:tt)*],
        evtupd=[$($evtupd:tt)*], evtrem=[$($evtrem:tt)*], evtwhole=[$($evtwhole:tt)*],
        asserts=[$($asserts:tt)*],
        fields=[
            full_reload_only($reason:literal)
            $(#[$a:meta])*
            $vis:vis $name:ident : $ty:ty ,
            $($rest:tt)*
        ]
    ) => {
        $crate::gui_state! {
            @munch
            state=$state, delta=$delta, diff_fn=$diff_fn, events_fn=$events_fn,
            old=$old, new=$new, cur=$cur, dst=$dst, evs=$evs,
            sfields=[$($sfields)* $(#[$a])* $vis $name: $ty,],
            dfields=[$($dfields)*],
            full=[$($full)*],
            diffpre=[$($diffpre)*],
            diffbody=[$($diffbody)*],
            evtupd=[$($evtupd)*],
            evtrem=[$($evtrem)*],
            evtwhole=[$($evtwhole)*],
            asserts=[$($asserts)* const _: () = { ::std::assert!(!$reason.is_empty(), "full_reload_only reason must be non-empty"); };],
            fields=[$($rest)*]
        }
    };
}

pub(crate) use gui_state;
