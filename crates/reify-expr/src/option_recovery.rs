//! Option/Map recovery combinator evaluation — task β of PRD
//! docs/prds/v0_6/result-and-fallback.md §8 Phase 2.
//!
//! Exposes two pure functions called from `eval_expr`'s `UserFunctionCall`
//! arm:
//!
//! - `is_combinator(name, arity) -> bool` — cheap gate on name + compiled arg
//!   count; no evaluation.  Returns `true` only for the seven known combinators
//!   at their expected arities.
//!
//! - `eval_combinator(name, args: &[Value]) -> Value` — tag-driven Value→Value
//!   dispatch.  Callers must ensure `is_combinator` returned `true` and that
//!   `args` has already been evaluated.
//!
//! # Recovery semantics (PRD contract C-1, decisions D2/D4, INV-2)
//!
//! Recovery is driven by the **SUBJECT** (first arg) tag, NOT by strict
//! all-args undef propagation.  This is the critical distinction from the
//! `eval_user_function_call` any-arg-undef shortcircuit:
//!
//! | Combinator          | subject=some(x)         | subject=none   | subject=undef |
//! |---------------------|-------------------------|----------------|---------------|
//! | unwrap_or(o, dflt)  | x (unboxed inner)        | dflt           | Undef         |
//! | or_default(o, dflt) | x (unboxed inner)        | dflt           | Undef         |
//! | fallback(o, dflt)   | x (unboxed inner)        | dflt           | Undef         |
//! | or_else(o, alt)     | o (whole Option, intact) | alt            | Undef         |
//! | is_some(o)          | Bool(true)               | Bool(false)    | Undef         |
//! | is_none(o)          | Bool(false)              | Bool(true)     | Undef         |
//! | get_or(m, k, dflt)  | m[k] if present, else dflt | N/A          | Undef         |
//!
//! `get_or` operates on `Value::Map`, not `Value::Option`.  A missing key
//! recovers to `dflt`; an undef map subject propagates Undef.  It must NOT
//! reuse `eval_index_access` (which returns Undef on a miss, conflating absence
//! with undef passthrough).
//!
//! An undef *key* in `get_or` (a key expression that produced `Value::Undef`)
//! also propagates `Value::Undef` — mirroring `eval_index_access` in `lib.rs`.
//! A failed key computation must not be conflated with a legitimate key miss
//! (which recovers to `dflt`).
//!
//! `or_default` and `fallback` are aliases of `unwrap_or` (PRD fork F2-a,
//! decision D6) — they share the same extract-or-default match arm.
//!
//! `map_or` is intentionally NOT handled in this pure module: it must APPLY its
//! function argument `f`, which requires the `EvalContext` (`apply_lambda` —
//! recursion depth, scope, captures).  It is handled by a dedicated ctx-aware
//! branch in `eval_expr`'s `UserFunctionCall` arm in `lib.rs` (task 4595),
//! keeping this module pure (INV-1).  `is_combinator` therefore deliberately
//! omits `map_or`.
//!
//! # Result recovery semantics (task result-fallback Layer-B γ #4037)
//!
//! `unwrap_or` and `or_else` are OVERLOADED: the same name also dispatches
//! `Value::Enum { type_name: "Result", variant: "Ok"/"Err", .. }` subjects,
//! disambiguated purely by the evaluated subject's Value tag (compiler-side
//! overload head-match already picked the right static signature per call
//! site by the subject's constructor head — `some`/`none` → `Option`,
//! `Ok`/`Err` → `Result`; #4991). `is_ok`/`is_err` are the `Result` analogues
//! of `is_some`/`is_none`, and `ok_or` is the `Option`→`Result` bridge:
//!
//! | Combinator          | subject=Ok{value:x}     | subject=Err{error:e} | subject=undef |
//! |---------------------|--------------------------|------------------------|---------------|
//! | unwrap_or(r, dflt)  | x (unboxed inner)        | dflt                   | Undef         |
//! | or_else(r, alt)     | r (whole Result, intact) | alt                    | Undef         |
//! | is_ok(r)            | Bool(true)               | Bool(false)            | Undef         |
//! | is_err(r)           | Bool(false)              | Bool(true)             | Undef         |
//!
//! `ok_or(o, err)`: `some(x) -> Ok{value:x}`; `none -> Err{error:err}`;
//! `undef -> Undef` (subject is the `Option`, not a `Result`).
//!
//! `map_err` is ctx-aware for the same reason `map_or` is (it must APPLY its
//! lambda `f` to the `Err` payload) and is handled by a dedicated branch in
//! `lib.rs`'s `UserFunctionCall` arm, mirroring `map_or` — `is_combinator`
//! deliberately omits it too.
//!
//! # Invariants
//!
//! INV-1 (orthogonality): `eval_combinator` consumes only evaluated `Value`
//! args and never reads `Freshness` nor emits `EventKind::Failed`.
//!
//! INV-2 (Kleene three-valued): for every combinator, a `Value::Undef` subject
//! propagates `Value::Undef` regardless of other args.
//!
//! INV-3 (back-compat): purely additive — existing `Option`/`some`/`none`/
//! `undef` and all Freshness machinery are untouched.
//!
//! INV-4: no new `Value` variant is added.

use reify_ir::Value;

/// Return `true` if `(name, arity)` identifies a known recovery combinator.
///
/// This is a cheap gate on the *compiled* arg count — no evaluation.  The
/// `UserFunctionCall` arm in `eval_expr` calls this before evaluating any
/// args so that non-combinator calls fall straight through to
/// `eval_user_function_call` without paying evaluation cost here.
///
/// # Sync note
///
/// The names + arities here must stay in sync with:
/// - `crates/reify-compiler/stdlib/option_recovery.ri` — canonical `pub fn`
///   declarations; the source of truth for arities.
/// - `crates/reify-compiler/stdlib/result.ri` — same role for the `Result`
///   overloads of `unwrap_or`/`or_else` plus `is_ok`/`is_err`/`ok_or`
///   (`map_err` is ctx-aware and excluded, see the module doc above).
/// - `crates/reify-compiler/src/expr.rs` `FALLBACK_COMBINATORS` — the
///   type-checker's overlapping subset: `["unwrap_or", "or_default",
///   "fallback", "get_or"]` (arity-2/3 extract-or-default names only;
///   `or_else`, `is_some`, and `is_none` are absent because they carry no
///   default-vs-element-type contract).
///
/// Adding a combinator to `option_recovery.ri` / `result.ri` without a
/// matching entry here means the placeholder `.ri` body runs instead of the
/// real intercept.  The `sync_drift_check_all_combinators_recognized` test in
/// `crates/reify-expr/tests/option_recovery_eval_tests.rs` and the
/// `sync_drift_check_result_combinators_recognized` test in
/// `crates/reify-expr/tests/result_combinator_eval_tests.rs` catch this at
/// test time.
pub fn is_combinator(name: &str, arity: usize) -> bool {
    matches!(
        (name, arity),
        ("unwrap_or", 2)
            | ("or_default", 2)
            | ("fallback", 2)
            | ("or_else", 2)
            | ("is_some", 1)
            | ("is_none", 1)
            | ("get_or", 3)
            | ("is_ok", 1)
            | ("is_err", 1)
            | ("ok_or", 2)
    )
}

/// Evaluate a recovery combinator over already-evaluated `Value` args.
///
/// # Panics
///
/// Does not panic — any unrecognised combinator name or unexpected arity
/// returns `Value::Undef` (graceful type-error degradation), matching the
/// behaviour of `eval_user_function_call` on an unresolved call.
///
/// Callers are expected to have called `is_combinator` first; this function is
/// only called from the hot path when the gate matched.
pub fn eval_combinator(name: &str, args: &[Value]) -> Value {
    match name {
        // ── extract-or-default family (arity 2) ──────────────────────────────
        //
        // unwrap_or / or_default / fallback: identical semantics.
        // subject=some(x) -> x (unboxed); subject=none -> dflt; subject=undef -> Undef.
        //
        // CRITICAL: check the SUBJECT tag first.  Do NOT propagate Undef when
        // the subject is some(x) even if dflt is Undef.
        "unwrap_or" | "or_default" | "fallback" if args.len() == 2 => {
            eval_extract_or_default(&args[0], &args[1])
        }

        // ── or_else (arity 2) ─────────────────────────────────────────────────
        //
        // subject=some(x) -> return the whole Option unchanged;
        // subject=none    -> return alt (the alternative Option);
        // subject=undef   -> Undef.
        "or_else" if args.len() == 2 => eval_or_else(&args[0], &args[1]),

        // ── presence predicates (arity 1) ─────────────────────────────────────
        //
        // Kleene three-valued: some->true/false, none->false/true, undef->Undef.
        "is_some" if args.len() == 1 => eval_is_some(&args[0]),
        "is_none" if args.len() == 1 => eval_is_none(&args[0]),

        // ── get_or (arity 3) ──────────────────────────────────────────────────
        //
        // subject=Map(entries): return entries[key] if present else dflt.
        // subject=Undef: Undef passthrough.
        // key absent -> dflt (the §9.2.6 map-miss recovery).
        "get_or" if args.len() == 3 => eval_get_or(&args[0], &args[1], &args[2]),

        // ── Result presence predicates (arity 1) ────────────────────────────
        //
        // Kleene three-valued: Ok->true/false, Err->false/true, undef->Undef.
        "is_ok" if args.len() == 1 => eval_is_ok(&args[0]),
        "is_err" if args.len() == 1 => eval_is_err(&args[0]),

        // ── ok_or: Option→Result bridge (arity 2) ───────────────────────────
        //
        // subject=some(x) -> Ok{value:x}; subject=none -> Err{error:err};
        // subject=undef -> Undef.
        "ok_or" if args.len() == 2 => eval_ok_or(&args[0], &args[1]),

        // Unrecognised or wrong-arity — graceful degradation.
        _ => Value::Undef,
    }
}

// ── private dispatch helpers ──────────────────────────────────────────────────

/// Extract-or-default: unwrap_or / or_default / fallback.
///
/// Overloaded over `Result<T,E>` subjects too (task #4037): `Ok{value:x} ->
/// x`, `Err{..} -> dflt`, disambiguated by the subject's `Value::Enum`
/// `type_name` tag.
#[inline]
fn eval_extract_or_default(subject: &Value, dflt: &Value) -> Value {
    match subject {
        // some(x) -> x, regardless of dflt (even undef dflt).
        Value::Option(Some(inner)) => *inner.clone(),
        // none -> dflt.
        Value::Option(None) => dflt.clone(),
        // Result::Ok{value:x} -> x (unboxed), regardless of dflt.
        Value::Enum {
            type_name,
            variant,
            payload,
        } if type_name == "Result" && variant == "Ok" => payload
            .iter()
            .find(|(field, _)| field == "value")
            .map(|(_, v)| v.clone())
            .unwrap_or(Value::Undef),
        // Result::Err{..} -> dflt.
        Value::Enum {
            type_name, variant, ..
        } if type_name == "Result" && variant == "Err" => dflt.clone(),
        // undef subject -> Undef (INV-2).
        Value::Undef => Value::Undef,
        // Type error: subject is not an Option/Result — graceful degradation.
        _ => Value::Undef,
    }
}

/// or_else: return subject if some/Ok, else alt.
///
/// Overloaded over `Result<T,E>` subjects too (task #4037): `Ok{..} ->`
/// subject unchanged, `Err{..} -> alt`, disambiguated by the subject's
/// `Value::Enum` `type_name` tag.
#[inline]
fn eval_or_else(subject: &Value, alt: &Value) -> Value {
    match subject {
        // some(x) -> return the whole Value::Option(Some(_)) unchanged.
        Value::Option(Some(_)) => subject.clone(),
        // none -> return alt (the alternative Option).
        Value::Option(None) => alt.clone(),
        // Result::Ok{..} -> return the whole subject unchanged.
        Value::Enum {
            type_name, variant, ..
        } if type_name == "Result" && variant == "Ok" => subject.clone(),
        // Result::Err{..} -> return alt (the alternative Result).
        Value::Enum {
            type_name, variant, ..
        } if type_name == "Result" && variant == "Err" => alt.clone(),
        // undef subject -> Undef (INV-2).
        Value::Undef => Value::Undef,
        // Type error: graceful degradation.
        _ => Value::Undef,
    }
}

/// is_some: true if some, false if none, Undef if undef.
#[inline]
fn eval_is_some(subject: &Value) -> Value {
    match subject {
        Value::Option(Some(_)) => Value::Bool(true),
        Value::Option(None) => Value::Bool(false),
        Value::Undef => Value::Undef,
        _ => Value::Undef,
    }
}

/// is_none: false if some, true if none, Undef if undef.
#[inline]
fn eval_is_none(subject: &Value) -> Value {
    match subject {
        Value::Option(Some(_)) => Value::Bool(false),
        Value::Option(None) => Value::Bool(true),
        Value::Undef => Value::Undef,
        _ => Value::Undef,
    }
}

/// is_ok: true if `Result::Ok`, false if `Result::Err`, Undef if undef.
#[inline]
fn eval_is_ok(subject: &Value) -> Value {
    match subject {
        Value::Enum {
            type_name, variant, ..
        } if type_name == "Result" && variant == "Ok" => Value::Bool(true),
        Value::Enum {
            type_name, variant, ..
        } if type_name == "Result" && variant == "Err" => Value::Bool(false),
        Value::Undef => Value::Undef,
        _ => Value::Undef,
    }
}

/// is_err: false if `Result::Ok`, true if `Result::Err`, Undef if undef.
#[inline]
fn eval_is_err(subject: &Value) -> Value {
    match subject {
        Value::Enum {
            type_name, variant, ..
        } if type_name == "Result" && variant == "Ok" => Value::Bool(false),
        Value::Enum {
            type_name, variant, ..
        } if type_name == "Result" && variant == "Err" => Value::Bool(true),
        Value::Undef => Value::Undef,
        _ => Value::Undef,
    }
}

/// get_or: map[key] if present, else dflt.  Undef map or undef key -> Undef.
///
/// Performs its own `BTreeMap::get` lookup — must NOT delegate to
/// `eval_index_access`, which returns `Value::Undef` on a miss and would
/// conflate an absent key (should recover to dflt) with undef passthrough.
///
/// An undef *key* propagates `Value::Undef` — mirroring `eval_index_access`
/// in `lib.rs`, which returns `Undef` whenever the index is undef.  A failed
/// key computation must not be silently conflated with a legitimate key miss
/// (which recovers to `dflt`).
#[inline]
fn eval_get_or(subject: &Value, key: &Value, dflt: &Value) -> Value {
    // Undef key propagates Undef regardless of the subject.
    if key.is_undef() {
        return Value::Undef;
    }
    match subject {
        Value::Map(entries) => entries.get(key).cloned().unwrap_or_else(|| dflt.clone()),
        Value::Undef => Value::Undef,
        _ => Value::Undef,
    }
}

/// ok_or: the `Option`→`Result` bridge.  `some(x) -> Ok{value:x}`; `none ->
/// Err{error:err}`; undef subject -> Undef (INV-2).
///
/// The subject here is an `Option`, not a `Result` — this is the one
/// combinator in this module whose subject tag is `Value::Option` but whose
/// return value is the OTHER enum's `Value::Enum` shape.
#[inline]
fn eval_ok_or(subject: &Value, err: &Value) -> Value {
    match subject {
        Value::Option(Some(inner)) => Value::Enum {
            type_name: "Result".to_string(),
            variant: "Ok".to_string(),
            payload: vec![("value".to_string(), *inner.clone())],
        },
        Value::Option(None) => Value::Enum {
            type_name: "Result".to_string(),
            variant: "Err".to_string(),
            payload: vec![("error".to_string(), err.clone())],
        },
        Value::Undef => Value::Undef,
        _ => Value::Undef,
    }
}

