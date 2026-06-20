//! Shared input-cracking helpers for the Tensegrity-consuming compute
//! trampolines (`form_find.rs`, `tensegrity_load.rs`).
//!
//! Both trampolines crack a `Tensegrity` `Value::StructureInstance` into node
//! coordinates + member connectivity, range-checking every index so the kernel
//! never indexes out of bounds. Those helpers were near-verbatim copies that
//! differed only in their `E_*Infeasible` diagnostic mnemonic (and an
//! independently-drifted `crack_index_pairs` parameter order); centralising them
//! here — the same single-definition-site discipline as the result builders in
//! [`super`] (`point3_length` / `scalar_list`) — keeps each caller's located
//! error wording while removing the copy, so the next Tensegrity-consuming
//! trampoline reuses rather than re-clones.
//!
//! Every fallible helper takes a `code: &str` diagnostic mnemonic (e.g.
//! `"E_FormFindInfeasible"` or `"E_TensegrityLoadInfeasible"`) which is prefixed
//! onto each message as `"{code}: …"`, so the located wording stays caller-owned.

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
