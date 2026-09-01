//! Shared input-cracking helpers for the Tensegrity-consuming compute
//! trampolines (`form_find.rs`, `tensegrity_load.rs`, `membrane_load.rs`).
//!
//! Each cracks a `Tensegrity` `Value::StructureInstance` into node coordinates +
//! member connectivity (plus, for `membrane_load.rs`, surface triples),
//! range-checking every index so the kernel never indexes out of bounds. Those
//! helpers were near-verbatim copies that differed only in their
//! `E_*Infeasible` diagnostic mnemonic (and an independently-drifted
//! `crack_index_pairs` parameter order); centralising them here — the same
//! single-definition-site discipline as the result builders in [`super`]
//! (`point3_length` / `scalar_list`) — keeps each caller's located error wording
//! while removing the copy, so the next Tensegrity-consuming trampoline reuses
//! rather than re-clones. The same treatment folds the unit-checking scalar
//! crackers in here alongside them: [`crack_dimensioned_scalar`] was a verbatim
//! pair across `tensegrity_load.rs` and `membrane_load.rs`.
//!
//! # The four scalar/list crackers
//!
//! Two PAIRS, one per acceptance set, each a scalar cracker plus its list
//! lifting:
//!
//! - [`crack_dimensioned_scalar`] / [`crack_scalar_list`] — the position wants
//!   one PARTICULAR unit (a Force, a Pressure); a `Scalar` in any other is
//!   rejected.
//! - [`crack_dimensionless_scalar`] / [`crack_dimensionless_list`] — the
//!   position is a bare RATIO; a `Scalar` in *any* unit is rejected.
//!
//! They stay four functions rather than one parameterised over an
//! `Option<DimensionVector>` because "no dimension at all" is not one more
//! choice on the same axis: it changes which `Value` variants read (`Int` is a
//! ratio spelling but not a Force spelling) and which advice the diagnostic
//! carries. What they DO share is one diagnostic vocabulary — the same "has the
//! wrong unit" phrasing and the same located `"{what}[{i}]"` entry naming — so
//! the tensegrity trampolines speak with one voice.
//!
//! Every fallible helper takes a `code: &str` diagnostic mnemonic (e.g.
//! `"E_FormFindInfeasible"` or `"E_TensegrityLoadInfeasible"`) which is prefixed
//! onto each message as `"{code}: …"`, so the located wording stays caller-owned.
//! All four scalar/list crackers extend that convention with a second
//! caller-owned string, `hint: &str` — the trailing clause saying what the caller
//! wanted instead ([`crack_dimensioned_scalar`]'s is argument-order advice;
//! [`crack_dimensionless_scalar`]'s explains why the position is bare). It is
//! threaded in rather than inferred here because each trampoline's hint names
//! that trampoline's OWN arguments (`membrane_load` has a `membrane_thickness`;
//! `tensegrity_load` does not), so choosing it inside this module would mean
//! enumerating its callers — exactly the coupling this file exists to remove.

use reify_core::DimensionVector;
use reify_ir::Value;

/// Extract an f64 from a `Scalar` (any dimension) or a bare `Real`.
///
/// `point3(1m, …)` lowers each component to `Scalar{LENGTH}`; `[1.0, …]` lowers
/// to `Real`. Returns `None` for any other `Value` — the caller turns that into
/// a located error — so this helper carries no diagnostic mnemonic itself.
pub(crate) fn scalar_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Scalar { si_value, .. } => Some(*si_value),
        Value::Real(r) => Some(*r),
        _ => None,
    }
}

/// Crack a single dimensioned `Scalar` into an f64, requiring its unit to equal
/// `expected`. A bare `Real` is still accepted — the dimensionless ergonomic
/// escape hatch [`scalar_f64`] already allowed (so `[1.0, …]`-style literals keep
/// working) — but a *dimensioned* `Scalar` whose unit disagrees (e.g. an Area
/// passed where a Pressure is expected: the classic `youngs_modulus` ↔ `area`
/// argument swap, or a Length where a Force is expected) is rejected with a
/// located error rather than silently solving a physically wrong problem. This
/// tightens the v1 form-find relaxation for the positionally-adjacent section
/// scalars without losing the bare-`Real` ergonomics.
///
/// `label` is the human unit name shown in the diagnostic. `hint` is the
/// caller-owned argument-order advice appended in parentheses — the same
/// caller-owned-wording convention as `code` (see the module doc), threaded in
/// rather than inferred because it names the *caller's* own arguments.
pub(crate) fn crack_dimensioned_scalar(
    v: &Value,
    what: &str,
    expected: DimensionVector,
    label: &str,
    code: &str,
    hint: &str,
) -> Result<f64, String> {
    match v {
        Value::Real(r) => Ok(*r),
        Value::Scalar {
            si_value,
            dimension,
        } if *dimension == expected => Ok(*si_value),
        Value::Scalar { .. } => Err(format!(
            "{code}: {what} has the wrong unit — expected a {label}; \
             check the call argument order ({hint})"
        )),
        other => Err(format!("{code}: {what} must be a scalar, got {other:?}")),
    }
}

/// Crack a value at a DIMENSIONLESS position, rejecting a dimensioned `Scalar`
/// instead of silently stripping its unit.
///
/// # Why these positions are bare
///
/// Some trampoline inputs are genuinely RATIOS rather than physical quantities.
/// `form_find`'s force densities, seed ratios and surface stresses are
/// nullity-invariant — as the `form_find_free` stdlib doc puts it, "overall
/// scaling of q is nullity-invariant, so only relative ratios matter" — so their
/// absolute magnitude carries no information and there is no unit that would
/// make them more precise.
///
/// # The two-sided contract
///
/// The dimension-checked-readers PRD
/// (`docs/prds/v0_6/dimension-checked-readers.md`) puts exactly these positions
/// in its Leg B "Deliberately bare" bucket, whose contract has TWO sides:
///
/// 1. Stay ACCEPTING — every numeric ratio spelling reads (see below), so the
///    bare `[1.0, -1.0, …]` literals the whole tensegrity corpus passes keep
///    working, and a future over-tightening to "bare `Real` only" is a
///    regression, not a hardening.
/// 2. Still REJECT a dimensioned `Scalar` — "silence about a position is not
///    permission". Reading `1 N/m` as the ratio `1.0` silently reinterprets what
///    the author wrote: they asked for a physical force density and got their
///    SI magnitude repurposed as a dimensionless number, with no diagnostic and
///    a plausible-looking solve at the end of it. That is a located error here
///    instead.
///
/// # Accepted spellings
///
/// Accepts all THREE numeric spellings of a bare ratio — `Real`, `Int`, and a
/// `Scalar` carrying `DIMENSIONLESS` — and nothing else. `Int` is not redundant
/// with `Real`: `Int → Real` widening in this codebase is a TYPE-level rule only
/// (`type_compat.rs` defines it for `type_compatible`; `coerce.rs` states that
/// overload selection "does NOT apply Int→Real (or any other) widening"), so
/// there is no value-level coercion and an integer ratio literal such as
/// `form_find(net, [1, 1, 1, 1], anchors)` arrives here as `Value::Int`
/// verbatim. `elastic_static.rs::dimensionless_component` records the same
/// reasoning for the `MaterialFrame` dimensionless axis.
///
/// The dimensionless counterpart of [`crack_dimensioned_scalar`]: that one wants
/// one PARTICULAR unit, this one wants no unit at all, so the acceptance sets
/// differ and they stay distinct functions — but they deliberately share the
/// "has the wrong unit" wording, and the same caller-owned `code` / `hint`
/// convention (see the module doc), so the tensegrity trampolines present one
/// diagnostic vocabulary. There is no `expected: DimensionVector` parameter
/// because "no dimension at all" is not a choice the caller gets to make.
///
/// Forward pointer: task alpha (#5791) relocates `arg_acceptance` into
/// `reify-ir` and adds a `dimensionless_spec()` whose acceptance set is exactly
/// the `Real | Int | Scalar{DIMENSIONLESS}` above. Today's `accept_arg` rejects
/// a bare `Value::Real` outright, so it cannot yet express side 1 of the
/// contract; once alpha lands that additive redesign, this helper and
/// [`crack_dimensionless_list`] should become thin adapters over it rather than
/// a second definition site.
pub(crate) fn crack_dimensionless_scalar(
    v: &Value,
    what: &str,
    code: &str,
    hint: &str,
) -> Result<f64, String> {
    match v {
        Value::Real(r) => Ok(*r),
        Value::Int(n) => Ok(*n as f64),
        Value::Scalar {
            si_value,
            dimension,
        } if dimension.is_dimensionless() => Ok(*si_value),
        Value::Scalar { dimension, .. } => Err(format!(
            "{code}: {what} has the wrong unit — expected a dimensionless ratio \
             (a bare Real or a dimensionless Scalar), got a Scalar in {dimension}; {hint}"
        )),
        other => Err(format!("{code}: {what} must be a real, got {other:?}")),
    }
}

/// Crack a `List<Real>` at a DIMENSIONLESS position, requiring every entry to be
/// a bare ratio — the list lifting of [`crack_dimensionless_scalar`], whose
/// `code` / `hint` it threads through unchanged.
///
/// Each entry's located name is `"{what}[{i}]"`, so a wrong-unit diagnostic tells
/// the author WHICH entry carries a unit rather than merely naming the list —
/// the same located-naming contract [`crack_scalar_list`] gives the dimensioned
/// positions. Shared by all four of `form_find.rs`'s bare positions (anchored
/// `force_densities` / `surface_stresses`, free `seed_ratios` /
/// `surface_stresses`) through its `crack_reals` wrapper.
pub(crate) fn crack_dimensionless_list(
    v: &Value,
    what: &str,
    code: &str,
    hint: &str,
) -> Result<Vec<f64>, String> {
    let list = match v {
        Value::List(items) => items,
        other => {
            return Err(format!(
                "{code}: {what} must be a list of dimensionless ratios, got {other:?}"
            ));
        }
    };
    let mut out = Vec::with_capacity(list.len());
    for (i, item) in list.iter().enumerate() {
        out.push(crack_dimensionless_scalar(
            item,
            &format!("{what}[{i}]"),
            code,
            hint,
        )?);
    }
    Ok(out)
}

/// Crack a `List<Scalar>` requiring each entry to carry `expected` units (a bare
/// `Real` is still accepted per entry for ergonomics) — the list lifting of
/// [`crack_dimensioned_scalar`], whose `code` / `label` / `hint` it threads
/// through unchanged.
///
/// Each entry's located name is `"{what}[{i}]"`, so a wrong-unit diagnostic tells
/// the author WHICH entry is wrong rather than merely naming the list. Shared by
/// `tensegrity_load.rs`'s `crack_forces` and `membrane_load.rs`'s `crack_forces`
/// / `crack_pressures`, which supply the FORCE / PRESSURE choice.
pub(crate) fn crack_scalar_list(
    v: &Value,
    what: &str,
    expected: DimensionVector,
    label: &str,
    code: &str,
    hint: &str,
) -> Result<Vec<f64>, String> {
    let list = match v {
        Value::List(items) => items,
        other => {
            return Err(format!(
                "{code}: {what} must be a list of {label} scalars, got {other:?}"
            ));
        }
    };
    let mut out = Vec::with_capacity(list.len());
    for (i, item) in list.iter().enumerate() {
        out.push(crack_dimensioned_scalar(
            item,
            &format!("{what}[{i}]"),
            expected,
            label,
            code,
            hint,
        )?);
    }
    Ok(out)
}

/// Range-check a signed node index against `0..n`, returning a located
/// `"{code}: {ctx} index N is out of range 0..n"` error. A negative index — or
/// one at/after the node count — is rejected here rather than wrapping to a huge
/// `usize` and indexing out of bounds in the kernel.
pub(crate) fn check_index(idx: i64, n: usize, ctx: &str, code: &str) -> Result<usize, String> {
    if idx < 0 || idx as usize >= n {
        return Err(format!("{code}: {ctx} index {idx} is out of range 0..{n}"));
    }
    Ok(idx as usize)
}

/// Crack `Tensegrity.nodes` (a `List<Point>`) into `[f64; 3]` SI coordinates.
///
/// Both `Value::Point` and `Value::Vector` 3-tuples are accepted — a node is a
/// coordinate triple either way.
pub(crate) fn crack_nodes(v: Option<&Value>, code: &str) -> Result<Vec<[f64; 3]>, String> {
    let list = match v {
        Some(Value::List(ns)) => ns,
        other => {
            return Err(format!(
                "{code}: Tensegrity.nodes must be a list of points, got {other:?}"
            ));
        }
    };
    let mut out = Vec::with_capacity(list.len());
    for (i, node) in list.iter().enumerate() {
        match node {
            Value::Point(c) | Value::Vector(c) if c.len() == 3 => {
                let bad = || format!("{code}: Tensegrity.nodes[{i}] has a non-numeric coordinate");
                out.push([
                    scalar_f64(&c[0]).ok_or_else(bad)?,
                    scalar_f64(&c[1]).ok_or_else(bad)?,
                    scalar_f64(&c[2]).ok_or_else(bad)?,
                ]);
            }
            other => {
                return Err(format!(
                    "{code}: Tensegrity.nodes[{i}] must be a 3-component point, got {other:?}"
                ));
            }
        }
    }
    Ok(out)
}

/// Crack a `List<List<Int>>` connectivity field (`field` is the field name, e.g.
/// `"struts"` / `"cables"`) into index pairs, range-checking each endpoint
/// against the node count `n` so an out-of-range member index is a located
/// trampoline-level error rather than a generic kernel `DimensionMismatch`.
pub(crate) fn crack_index_pairs(
    v: Option<&Value>,
    field: &str,
    n: usize,
    code: &str,
) -> Result<Vec<(usize, usize)>, String> {
    let list = match v {
        Some(Value::List(pairs)) => pairs,
        other => {
            return Err(format!(
                "{code}: Tensegrity.{field} must be a list of index pairs, got {other:?}"
            ));
        }
    };
    let mut out = Vec::with_capacity(list.len());
    for (i, pair) in list.iter().enumerate() {
        let (from, to) = match pair {
            Value::List(idx) if idx.len() == 2 => match (&idx[0], &idx[1]) {
                (Value::Int(a), Value::Int(b)) => (*a, *b),
                _ => {
                    return Err(format!(
                        "{code}: Tensegrity.{field}[{i}] must be two integer indices"
                    ));
                }
            },
            _ => {
                return Err(format!(
                    "{code}: Tensegrity.{field}[{i}] must be a 2-element index list"
                ));
            }
        };
        out.push((
            check_index(from, n, &format!("Tensegrity.{field}[{i}] start"), code)?,
            check_index(to, n, &format!("Tensegrity.{field}[{i}] end"), code)?,
        ));
    }
    Ok(out)
}

/// Crack a `List<List<Int>>` surface-connectivity field (`field` is the field
/// name, e.g. `"surfaces"`) into triangle corner index-triples, range-checking
/// each of the three corners against the node count `n` so an out-of-range
/// surface index is a located trampoline-level error rather than an out-of-bounds
/// kernel panic. Each inner list must hold exactly three integer indices.
///
/// Unlike [`crack_index_pairs`], a MISSING field — `None`, or a present-but-Undef
/// value — yields an EMPTY `Vec` rather than an error. This honours the task-α
/// `tensegrity_surfaces` accessor contract: `surfaces` is legitimately absent for
/// a line-only (cable/strut) tensegrity, so the line-only form-find path must see
/// an empty triangle list, not an infeasibility diagnostic.
pub(crate) fn crack_index_triples(
    v: Option<&Value>,
    field: &str,
    n: usize,
    code: &str,
) -> Result<Vec<(usize, usize, usize)>, String> {
    let list = match v {
        // Missing or Undef ⇒ no surfaces (line-only path): empty, not an error.
        None | Some(Value::Undef) => return Ok(Vec::new()),
        Some(Value::List(tris)) => tris,
        other => {
            return Err(format!(
                "{code}: Tensegrity.{field} must be a list of index triples, got {other:?}"
            ));
        }
    };
    let mut out = Vec::with_capacity(list.len());
    for (i, tri) in list.iter().enumerate() {
        let (a, b, c) = match tri {
            Value::List(idx) if idx.len() == 3 => match (&idx[0], &idx[1], &idx[2]) {
                (Value::Int(a), Value::Int(b), Value::Int(c)) => (*a, *b, *c),
                _ => {
                    return Err(format!(
                        "{code}: Tensegrity.{field}[{i}] must be three integer indices"
                    ));
                }
            },
            _ => {
                return Err(format!(
                    "{code}: Tensegrity.{field}[{i}] must be a 3-element index list"
                ));
            }
        };
        out.push((
            check_index(a, n, &format!("Tensegrity.{field}[{i}].0"), code)?,
            check_index(b, n, &format!("Tensegrity.{field}[{i}].1"), code)?,
            check_index(c, n, &format!("Tensegrity.{field}[{i}].2"), code)?,
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::DimensionVector;

    // The two REAL caller (code, hint) pairs, transcribed verbatim from the
    // trampolines that own them — `tensegrity_load.rs` and `membrane_load.rs`.
    // Pinning the production wording (rather than a synthetic stand-in) is what
    // makes the full-string assertions below evidence that the hoist preserved
    // both diagnostics byte-for-byte.
    const TL_CODE: &str = "E_TensegrityLoadInfeasible";
    const TL_HINT: &str =
        "youngs_modulus is a Pressure, area is an Area, and prestress / loads are Forces";
    const ML_CODE: &str = "E_MembraneLoadInfeasible";
    const ML_HINT: &str = "youngs_modulus / membrane_youngs are Pressures, area is an Area, \
                           membrane_thickness is a Length, and prestress / loads are Forces";

    /// A bare `Real` is accepted whatever the expected dimension — the
    /// dimensionless ergonomic escape hatch (`[1.0, …]`-style literals keep
    /// working) that both trampolines rely on and that [`scalar_f64`] already
    /// allowed. Deliberately NOT pinned: which dimension was expected, because a
    /// `Real` carries none.
    #[test]
    fn crack_dimensioned_scalar_accepts_bare_real() {
        let got = crack_dimensioned_scalar(
            &Value::Real(2.5),
            "prestress[0]",
            DimensionVector::FORCE,
            "Force",
            TL_CODE,
            TL_HINT,
        );
        assert_eq!(got, Ok(2.5));
    }

    /// A `Scalar` whose dimension equals `expected` yields its SI value.
    #[test]
    fn crack_dimensioned_scalar_accepts_matching_dimension() {
        let got = crack_dimensioned_scalar(
            &Value::Scalar {
                si_value: 5000.0,
                dimension: DimensionVector::FORCE,
            },
            "prestress[0]",
            DimensionVector::FORCE,
            "Force",
            TL_CODE,
            TL_HINT,
        );
        assert_eq!(got, Ok(5000.0));
    }

    /// The load-bearing assertion of the hoist: a *dimensioned* `Scalar` whose
    /// unit disagrees is rejected with a message assembled from the shared
    /// template `"{code}: {what} has the wrong unit — expected a {label}; check
    /// the call argument order ({hint})"`, where BOTH the mnemonic and the
    /// argument-order advice are caller-owned. Asserted by full string equality
    /// against each of the two real caller pairs, so neither can be baked into
    /// the shared helper; the final negative assertion catches the specific
    /// regression of hardcoding membrane_load's hint, which a `contains`-style
    /// check on the tensegrity_load message would otherwise let through.
    #[test]
    fn crack_dimensioned_scalar_rejects_wrong_dimension_with_caller_owned_code_and_hint() {
        let an_area = Value::Scalar {
            si_value: 1.0e-4,
            dimension: DimensionVector::AREA,
        };

        let tl = crack_dimensioned_scalar(
            &an_area,
            "youngs_modulus",
            DimensionVector::PRESSURE,
            "Pressure",
            TL_CODE,
            TL_HINT,
        );
        assert_eq!(
            tl,
            Err(
                "E_TensegrityLoadInfeasible: youngs_modulus has the wrong unit — expected a \
                 Pressure; check the call argument order (youngs_modulus is a Pressure, area \
                 is an Area, and prestress / loads are Forces)"
                    .to_string()
            )
        );

        let ml = crack_dimensioned_scalar(
            &an_area,
            "membrane_youngs",
            DimensionVector::PRESSURE,
            "Pressure",
            ML_CODE,
            ML_HINT,
        );
        assert_eq!(
            ml,
            Err(
                "E_MembraneLoadInfeasible: membrane_youngs has the wrong unit — expected a \
                 Pressure; check the call argument order (youngs_modulus / membrane_youngs are \
                 Pressures, area is an Area, membrane_thickness is a Length, and prestress / \
                 loads are Forces)"
                    .to_string()
            )
        );

        // A helper that hardcoded membrane_load's hint would still satisfy the
        // tensegrity_load `{code}` prefix; it must not name an argument that
        // trampoline does not have.
        assert!(
            !tl.unwrap_err().contains("membrane_thickness"),
            "the tensegrity_load diagnostic must not advise about membrane_thickness"
        );
    }

    /// A non-scalar `Value` takes the shape arm, which carries the mnemonic but
    /// NOT the unit hint (the argument order is not the problem). The `{other:?}`
    /// Debug tail is deliberately left unpinned — only the located prefix is a
    /// contract.
    #[test]
    fn crack_dimensioned_scalar_rejects_non_scalar() {
        let err = crack_dimensioned_scalar(
            &Value::Undef,
            "area",
            DimensionVector::AREA,
            "Area",
            TL_CODE,
            TL_HINT,
        )
        .unwrap_err();
        assert!(
            err.starts_with("E_TensegrityLoadInfeasible: area must be a scalar, got "),
            "unexpected shape-arm message: {err}"
        );
    }

    // ---- crack_scalar_list (task #6412, second hoist) -----------------------
    //
    // Earns the same treatment as `crack_dimensioned_scalar`: `membrane_load.rs`
    // has this loop as a named helper and `tensegrity_load.rs::crack_forces`
    // inlines the identical loop with FORCE/"Force" hardcoded — two copies, so
    // the repo's single-definition-site norm applies.

    /// Every entry is cracked in order, and the per-entry bare-`Real` escape
    /// hatch survives the lift: a list may legitimately mix `3kN`-style
    /// dimensioned scalars with bare `[1.0, …]` literals.
    #[test]
    fn crack_scalar_list_accepts_matching_dimension_entries() {
        let all_dimensioned = Value::List(vec![
            Value::Scalar {
                si_value: -1000.0,
                dimension: DimensionVector::FORCE,
            },
            Value::Scalar {
                si_value: 3000.0,
                dimension: DimensionVector::FORCE,
            },
        ]);
        assert_eq!(
            crack_scalar_list(
                &all_dimensioned,
                "prestress",
                DimensionVector::FORCE,
                "Force",
                TL_CODE,
                TL_HINT,
            ),
            Ok(vec![-1000.0, 3000.0]),
        );

        let mixed = Value::List(vec![
            Value::Real(-1000.0),
            Value::Scalar {
                si_value: 3000.0,
                dimension: DimensionVector::FORCE,
            },
        ]);
        assert_eq!(
            crack_scalar_list(
                &mixed,
                "prestress",
                DimensionVector::FORCE,
                "Force",
                TL_CODE,
                TL_HINT,
            ),
            Ok(vec![-1000.0, 3000.0]),
        );
    }

    /// The list-SHAPE arm. This is the one message the hoist deliberately
    /// changes: `tensegrity_load.rs::crack_forces` used to hardcode "must be a
    /// list of forces", and now reports the parameterized (strictly more
    /// informative) "must be a list of Force scalars" that `membrane_load.rs`
    /// already produced. Nothing in the repo pinned the old phrase — so it is
    /// pinned here, at the single definition site, for the first time. The
    /// `{other:?}` Debug tail is deliberately left unpinned.
    #[test]
    fn crack_scalar_list_rejects_non_list() {
        let err = crack_scalar_list(
            &Value::Real(1.0),
            "prestress",
            DimensionVector::FORCE,
            "Force",
            TL_CODE,
            TL_HINT,
        )
        .unwrap_err();
        assert!(
            err.starts_with(
                "E_TensegrityLoadInfeasible: prestress must be a list of Force scalars, got "
            ),
            "unexpected list-shape message: {err}"
        );
    }

    /// The located `{what}[{i}]` entry index: an author whose third prestress
    /// entry carries the wrong unit must be told WHICH entry, not merely that
    /// "prestress" is wrong. This is the contract `crack_forces` /
    /// `crack_pressures` inherit and that the membrane e2e tests (f3/f4) pin
    /// end-to-end; pinning it here catches a regression at the unit level first.
    #[test]
    fn crack_scalar_list_labels_the_offending_entry_index() {
        let second_entry_wrong = Value::List(vec![
            Value::Scalar {
                si_value: 3000.0,
                dimension: DimensionVector::FORCE,
            },
            Value::Scalar {
                si_value: 1.0e5,
                dimension: DimensionVector::PRESSURE,
            },
        ]);
        let err = crack_scalar_list(
            &second_entry_wrong,
            "prestress",
            DimensionVector::FORCE,
            "Force",
            TL_CODE,
            TL_HINT,
        )
        .unwrap_err();
        assert_eq!(
            err,
            "E_TensegrityLoadInfeasible: prestress[1] has the wrong unit — expected a Force; \
             check the call argument order (youngs_modulus is a Pressure, area is an Area, and \
             prestress / loads are Forces)"
        );
        // Guards against both degenerate labellings: a constant index and a
        // bare `what` with no index at all.
        assert!(
            !err.contains("prestress[0]"),
            "must name entry 1, got: {err}"
        );
        assert!(
            err.contains("prestress[1]"),
            "must carry a located index: {err}"
        );
    }

    // ---- crack_dimensionless_scalar (task #6120) ----------------------------
    //
    // The DIMENSIONLESS half of the pair: `form_find.rs`'s `force_densities`,
    // `seed_ratios` and `surface_stresses` are Leg B "Deliberately bare"
    // positions (`docs/prds/v0_6/dimension-checked-readers.md`), so the contract
    // is two-sided — stay accepting of every bare numeric spelling, but reject a
    // *dimensioned* Scalar rather than stripping its unit.

    // The real caller (code, hint) pair, transcribed verbatim from the
    // trampoline that owns them — `form_find.rs`'s `CODE` and
    // `DIMENSIONLESS_HINT`. Same rationale as `TL_*` / `ML_*` above: pinning the
    // production wording is what makes the full-string assertions evidence.
    const FF_CODE: &str = "E_FormFindInfeasible";
    const FF_HINT: &str = "force densities, seed ratios and surface stresses are \
         nullity-invariant RELATIVE ratios, not physical quantities — only their relative \
         magnitudes and signs matter, so drop the unit (write `1.0`, not `1N/1m`)";

    /// A bare `Real` is the canonical spelling of a ratio — `[1.0, -1.0, …]`
    /// literals are what every tensegrity example in the corpus passes.
    #[test]
    fn crack_dimensionless_scalar_accepts_bare_real() {
        assert_eq!(
            crack_dimensionless_scalar(&Value::Real(2.5), "force_densities[0]", FF_CODE, FF_HINT),
            Ok(2.5)
        );
    }

    /// A `Scalar` that carries an explicitly DIMENSIONLESS unit is still a valid
    /// ratio spelling and must read. This is the gate's upper bound: over-
    /// tightening it to "bare Real only" would reject a legitimate input.
    #[test]
    fn crack_dimensionless_scalar_accepts_dimensionless_scalar() {
        assert_eq!(
            crack_dimensionless_scalar(
                &Value::Scalar {
                    si_value: 2.5,
                    dimension: DimensionVector::DIMENSIONLESS,
                },
                "force_densities[0]",
                FF_CODE,
                FF_HINT,
            ),
            Ok(2.5)
        );
    }

    /// The load-bearing assertion: a *dimensioned* `Scalar` is rejected rather
    /// than silently stripped to its SI magnitude and reinterpreted as the bare
    /// ratio. Asserted by full string equality so all three caller-visible parts
    /// are pinned at the single definition site — the caller-owned `{code}`
    /// prefix, the caller-owned trailing `{hint}`, and the `DimensionVector`
    /// `Display` rendering of the offending unit, which is what tells the author
    /// WHICH unit they attached.
    #[test]
    fn crack_dimensionless_scalar_rejects_dimensioned_scalar_with_caller_owned_code_and_hint() {
        let got = crack_dimensionless_scalar(
            &Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::FORCE_DENSITY,
            },
            "force_densities[3]",
            FF_CODE,
            FF_HINT,
        );
        assert_eq!(
            got,
            Err(
                "E_FormFindInfeasible: force_densities[3] has the wrong unit — expected a \
                 dimensionless ratio (a bare Real or a dimensionless Scalar), got a Scalar in \
                 kg·m^-2·s^-2; force densities, seed ratios and surface stresses are \
                 nullity-invariant RELATIVE ratios, not physical quantities — only their \
                 relative magnitudes and signs matter, so drop the unit (write `1.0`, not \
                 `1N/1m`)"
                    .to_string()
            )
        );
    }

    /// An INTEGER ratio literal must read too. `Int → Real` widening in this
    /// codebase is a TYPE-level rule only (`type_compat.rs`; `coerce.rs` states
    /// overload selection "does NOT apply Int→Real … widening"), so there is no
    /// value-level coercion and `form_find(net, [1, 1, 1, 1], anchors)` reaches
    /// this reader as `Value::Int` verbatim. Same reasoning
    /// `elastic_static.rs::dimensionless_component` records for the
    /// `MaterialFrame` dimensionless axis, and the same acceptance set the PRD
    /// states for `dimensionless_spec`: `Real | Int | Scalar{DIMENSIONLESS}`.
    #[test]
    fn crack_dimensionless_scalar_accepts_int() {
        assert_eq!(
            crack_dimensionless_scalar(&Value::Int(3), "force_densities[0]", FF_CODE, FF_HINT),
            Ok(3.0)
        );
    }

    // ---- crack_dimensionless_list (task #6120) ------------------------------
    //
    // The dimensionless twin of `crack_scalar_list`, and due for the same
    // reason: once #6412 centralised the per-entry `"{what}[{i}]"` loop in this
    // module, `form_find.rs::crack_reals` was left hand-rolling a second copy of
    // it — a >=2-duplicate hit against the single-definition-site discipline
    // that hoist just landed here.

    /// Every entry is cracked in order, and the per-entry acceptance set
    /// survives the lift: one list may legitimately mix all three numeric ratio
    /// spellings, because `[1, 1.0, …]`-style literals lower to `Value::Int` and
    /// `Value::Real` element-wise with no value-level widening between them.
    #[test]
    fn crack_dimensionless_list_accepts_mixed_numeric_entries() {
        let mixed = Value::List(vec![
            Value::Real(1.0),
            Value::Int(2),
            Value::Scalar {
                si_value: 3.0,
                dimension: DimensionVector::DIMENSIONLESS,
            },
        ]);
        assert_eq!(
            crack_dimensionless_list(&mixed, "force_densities", FF_CODE, FF_HINT),
            Ok(vec![1.0, 2.0, 3.0]),
        );
    }

    /// The list-SHAPE arm, which carries the mnemonic but NOT the ratio hint
    /// (the unit is not the problem — the value is not a list at all). This is
    /// the one message the lifting deliberately changes: `form_find.rs` used to
    /// hardcode "must be a list of reals", and now reports the parameterized
    /// "must be a list of dimensionless ratios" at the single definition site —
    /// the same consolidation #6412 made when `tensegrity_load`'s "must be a
    /// list of forces" became "must be a list of Force scalars".
    #[test]
    fn crack_dimensionless_list_rejects_non_list() {
        assert_eq!(
            crack_dimensionless_list(&Value::Real(1.0), "force_densities", FF_CODE, FF_HINT),
            Err(
                "E_FormFindInfeasible: force_densities must be a list of dimensionless ratios, \
                 got Real(1.0)"
                    .to_string()
            )
        );
    }

    /// The located `{what}[{i}]` entry index: an author whose THIRD seed ratio
    /// carries a unit must be told WHICH entry is wrong, not merely that
    /// "seed_ratios" is wrong — the property the t1a / t1b e2e tests pin
    /// end-to-end, caught here at the unit level first.
    #[test]
    fn crack_dimensionless_list_labels_the_offending_entry_index() {
        let third_entry_dimensioned = Value::List(vec![
            Value::Real(-1.0),
            Value::Real(1.0),
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::FORCE_DENSITY,
            },
        ]);
        let err =
            crack_dimensionless_list(&third_entry_dimensioned, "seed_ratios", FF_CODE, FF_HINT)
                .unwrap_err();
        assert_eq!(
            err,
            "E_FormFindInfeasible: seed_ratios[2] has the wrong unit — expected a dimensionless \
             ratio (a bare Real or a dimensionless Scalar), got a Scalar in kg·m^-2·s^-2; force \
             densities, seed ratios and surface stresses are nullity-invariant RELATIVE ratios, \
             not physical quantities — only their relative magnitudes and signs matter, so drop \
             the unit (write `1.0`, not `1N/1m`)"
        );
        // Guards both degenerate labellings: a constant index, and a bare
        // `what` naming only the list.
        assert!(
            !err.contains("seed_ratios[0]"),
            "must name entry 2, got: {err}"
        );
        assert!(
            !err.contains("seed_ratios has the wrong unit"),
            "must locate the entry, not merely the list: {err}"
        );
    }
}
