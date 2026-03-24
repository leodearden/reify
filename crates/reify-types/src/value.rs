use std::collections::{BTreeMap, BTreeSet};

use crate::dimension::DimensionVector;
use crate::expr::CompiledExpr;
use crate::hash::ContentHash;
use crate::identity::ValueCellId;
use crate::persistent::PersistentMap;

/// The source kind of a field value at runtime.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FieldSourceKind {
    Analytical,
    Sampled,
    Composed,
    Imported,
}

/// Runtime values in Reify (M1 subset).
#[derive(Debug, Clone)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Real(f64),
    String(String),
    /// Dimensioned scalar: value in SI base units, with dimension.
    Scalar { si_value: f64, dimension: DimensionVector },
    /// Enum variant value: type_name::variant.
    Enum { type_name: String, variant: String },
    /// Ordered list of values.
    List(Vec<Value>),
    /// Ordered set of unique values.
    Set(BTreeSet<Value>),
    /// Ordered map from values to values.
    Map(BTreeMap<Value, Value>),
    /// Optional value: Some(value) or None.
    Option(Option<Box<Value>>),
    /// Field value: a typed domain->codomain mapping with stored lambda/data.
    Field {
        domain_type: crate::ty::Type,
        codomain_type: crate::ty::Type,
        source: FieldSourceKind,
        /// The callable lambda for analytical/composed fields, or Undef for sampled/imported.
        lambda: Box<Value>,
    },
    /// Lambda closure: captures environment values and body expression.
    Lambda {
        params: Vec<(String, ValueCellId)>,
        body: Box<CompiledExpr>,
        captures: ValueMap,
    },
    /// Rank-r tensor: recursive nesting of Vec<Value> (innermost elements are scalars).
    Tensor(Vec<Value>),
    /// Geometric point with n components (all sharing the same dimension).
    Point(Vec<Value>),
    /// Geometric vector with n components (all sharing the same dimension).
    Vector(Vec<Value>),
    /// Complex number: re and im share one dimension (e.g., complex impedance in ohms).
    Complex { re: f64, im: f64, dimension: DimensionVector },
    /// Orientation as a unit quaternion (w + xi + yj + zk).
    Orientation { w: f64, x: f64, y: f64, z: f64 },
    /// Range with optional inclusive/exclusive bounds.
    Range {
        lower: Option<Box<Value>>,
        upper: Option<Box<Value>>,
        lower_inclusive: bool,
        upper_inclusive: bool,
    },
    /// Undefined — not yet determined or computation failed.
    Undef,
}

impl Value {
    /// Create a scalar with LENGTH dimension from a value in meters.
    pub fn length(meters: f64) -> Self {
        Value::Scalar {
            si_value: meters,
            dimension: DimensionVector::LENGTH,
        }
    }

    /// Create a scalar with ANGLE dimension from a value in radians.
    pub fn angle(radians: f64) -> Self {
        Value::Scalar {
            si_value: radians,
            dimension: DimensionVector::ANGLE,
        }
    }

    /// Create a Range value with normalized inclusivity flags.
    ///
    /// When a bound is `None` (unbounded), the corresponding inclusive flag is forced to
    /// `false` — infinity is never "included". This ensures that two logically identical
    /// ranges compare as equal and produce the same content hash regardless of which
    /// inclusive flag the caller passed.
    pub fn range(
        lower: Option<Value>,
        upper: Option<Value>,
        lower_inclusive: bool,
        upper_inclusive: bool,
    ) -> Value {
        let lower_inclusive = lower_inclusive && lower.is_some();
        let upper_inclusive = upper_inclusive && upper.is_some();
        Value::Range {
            lower: lower.map(Box::new),
            upper: upper.map(Box::new),
            lower_inclusive,
            upper_inclusive,
        }
    }

    pub fn is_undef(&self) -> bool {
        matches!(self, Value::Undef)
    }

    /// Get the f64 value if this is a numeric type.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Real(r) => Some(*r),
            Value::Scalar { si_value, .. } => Some(*si_value),
            _ => None,
        }
    }

    /// Get the dimension of this value (DIMENSIONLESS for non-scalar types).
    pub fn dimension(&self) -> DimensionVector {
        match self {
            Value::Scalar { dimension, .. } => *dimension,
            Value::Complex { dimension, .. } => *dimension,
            // Point/Vector: dimension derives from the first component (all components share one dimension).
            Value::Point(items) | Value::Vector(items) => {
                items.first().map(|v| v.dimension()).unwrap_or(DimensionVector::DIMENSIONLESS)
            }
            _ => DimensionVector::DIMENSIONLESS,
        }
    }

    /// Compute a content hash for incremental caching.
    pub fn content_hash(&self) -> ContentHash {
        match self {
            Value::Bool(b) => ContentHash::of(&[0, *b as u8]),
            Value::Int(i) => {
                let mut buf = [0u8; 9];
                buf[0] = 1;
                buf[1..].copy_from_slice(&i.to_le_bytes());
                ContentHash::of(&buf)
            }
            Value::Real(r) => {
                let mut buf = [0u8; 9];
                buf[0] = 2;
                // Canonicalize NaN but preserve -0.0 (PartialEq uses to_bits)
                let bits = if r.is_nan() {
                    f64::NAN.to_bits() // canonical NaN
                } else {
                    r.to_bits()
                };
                buf[1..].copy_from_slice(&bits.to_le_bytes());
                ContentHash::of(&buf)
            }
            Value::String(s) => ContentHash::of(&[3]).combine(ContentHash::of_str(s)),
            Value::Scalar { si_value, dimension } => {
                // Canonicalize NaN but preserve -0.0 (PartialEq uses to_bits)
                let bits = if si_value.is_nan() {
                    f64::NAN.to_bits()
                } else {
                    si_value.to_bits()
                };
                let mut buf = [0u8; 9];
                buf[0] = 4;
                buf[1..].copy_from_slice(&bits.to_le_bytes());
                ContentHash::of(&buf).combine(dimension.content_hash())
            }
            Value::Enum { type_name, variant } => {
                ContentHash::of(&[6])
                    .combine(ContentHash::of_str(type_name))
                    .combine(ContentHash::of_str(variant))
            }
            Value::List(items) => {
                let mut h = ContentHash::of(&[7]);
                h = h.combine(ContentHash::of(&(items.len() as u64).to_le_bytes()));
                for item in items {
                    h = h.combine(item.content_hash());
                }
                h
            }
            Value::Set(items) => {
                let mut h = ContentHash::of(&[8]);
                h = h.combine(ContentHash::of(&(items.len() as u64).to_le_bytes()));
                for item in items {
                    h = h.combine(item.content_hash());
                }
                h
            }
            Value::Map(entries) => {
                let mut h = ContentHash::of(&[9]);
                h = h.combine(ContentHash::of(&(entries.len() as u64).to_le_bytes()));
                for (k, v) in entries {
                    h = h.combine(k.content_hash()).combine(v.content_hash());
                }
                h
            }
            Value::Option(inner) => match inner {
                // Tag [11] — tag [10] is exclusively reserved for Satisfaction
                None => ContentHash::of(&[11, 0]),
                Some(v) => ContentHash::of(&[11, 1]).combine(v.content_hash()),
            },
            Value::Field { domain_type, codomain_type, source, lambda } => {
                let mut h = ContentHash::of(&[13]);
                h = h.combine(ContentHash::of_str(&format!("{}", domain_type)));
                h = h.combine(ContentHash::of_str(&format!("{}", codomain_type)));
                h = h.combine(ContentHash::of_str(&format!("{:?}", source)));
                h = h.combine(lambda.content_hash());
                h
            }
            Value::Lambda { params, body, captures } => {
                let mut h = ContentHash::of(&[12]);
                h = h.combine(ContentHash::of(&(params.len() as u64).to_le_bytes()));
                for (name, id) in params {
                    h = h.combine(ContentHash::of_str(name));
                    h = h.combine(ContentHash::of_str(&format!("{}", id)));
                }
                h = h.combine(body.content_hash);
                for (id, val) in sorted_captures(captures) {
                    h = h.combine(ContentHash::of_str(&format!("{}", id)));
                    h = h.combine(val.content_hash());
                }
                h
            }
            Value::Tensor(items) => {
                let mut h = ContentHash::of(&[14]);
                h = h.combine(ContentHash::of(&(items.len() as u64).to_le_bytes()));
                for item in items {
                    h = h.combine(item.content_hash());
                }
                h
            }
            Value::Point(items) => {
                let mut h = ContentHash::of(&[18]);
                h = h.combine(ContentHash::of(&(items.len() as u64).to_le_bytes()));
                for item in items {
                    h = h.combine(item.content_hash());
                }
                h
            }
            Value::Vector(items) => {
                let mut h = ContentHash::of(&[19]);
                h = h.combine(ContentHash::of(&(items.len() as u64).to_le_bytes()));
                for item in items {
                    h = h.combine(item.content_hash());
                }
                h
            }
            Value::Complex { re, im, dimension } => {
                // tag=15; NaN canonicalization for both re and im; combine with dimension hash
                let re_bits = if re.is_nan() { f64::NAN.to_bits() } else { re.to_bits() };
                let im_bits = if im.is_nan() { f64::NAN.to_bits() } else { im.to_bits() };
                let mut buf = [0u8; 17];
                buf[0] = 15;
                buf[1..9].copy_from_slice(&re_bits.to_le_bytes());
                buf[9..17].copy_from_slice(&im_bits.to_le_bytes());
                ContentHash::of(&buf).combine(dimension.content_hash())
            }
            Value::Orientation { w, x, y, z } => {
                // tag=16; NaN canonicalization for all 4 components
                let canon = |v: &f64| -> u64 {
                    if v.is_nan() { f64::NAN.to_bits() } else { v.to_bits() }
                };
                let mut buf = [0u8; 33];
                buf[0] = 16;
                buf[1..9].copy_from_slice(&canon(w).to_le_bytes());
                buf[9..17].copy_from_slice(&canon(x).to_le_bytes());
                buf[17..25].copy_from_slice(&canon(y).to_le_bytes());
                buf[25..33].copy_from_slice(&canon(z).to_le_bytes());
                ContentHash::of(&buf)
            }
            Value::Range { lower, upper, lower_inclusive, upper_inclusive } => {
                debug_assert!(
                    lower.is_some() || !lower_inclusive,
                    "Range invariant violated: lower_inclusive must be false when lower is None"
                );
                debug_assert!(
                    upper.is_some() || !upper_inclusive,
                    "Range invariant violated: upper_inclusive must be false when upper is None"
                );
                // tag=17; flags then optional bounds
                let mut h = ContentHash::of(&[17, *lower_inclusive as u8, *upper_inclusive as u8]);
                match lower {
                    None => h = h.combine(ContentHash::of(&[0])),
                    Some(v) => h = h.combine(ContentHash::of(&[1])).combine(v.content_hash()),
                }
                match upper {
                    None => h = h.combine(ContentHash::of(&[0])),
                    Some(v) => h = h.combine(ContentHash::of(&[1])).combine(v.content_hash()),
                }
                h
            }
            Value::Undef => ContentHash::of(&[5]),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Real(a), Value::Real(b)) => a.to_bits() == b.to_bits(),
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Scalar { si_value: a, dimension: ad }, Value::Scalar { si_value: b, dimension: bd }) => {
                a.to_bits() == b.to_bits() && ad == bd
            }
            (Value::Enum { type_name: a, variant: av }, Value::Enum { type_name: b, variant: bv }) => {
                a == b && av == bv
            }
            (Value::List(a), Value::List(b)) => a == b,
            (Value::Tensor(a), Value::Tensor(b)) => a == b,
            (Value::Point(a), Value::Point(b)) => a == b,
            (Value::Vector(a), Value::Vector(b)) => a == b,
            (Value::Set(a), Value::Set(b)) => a == b,
            (Value::Map(a), Value::Map(b)) => a == b,
            (Value::Option(a), Value::Option(b)) => a == b,
            (
                Value::Field { domain_type: ad, codomain_type: ac, source: as_, lambda: al },
                Value::Field { domain_type: bd, codomain_type: bc, source: bs, lambda: bl },
            ) => ad == bd && ac == bc && as_ == bs && al == bl,
            (
                Value::Lambda { params: ap, body: ab, captures: ac },
                Value::Lambda { params: bp, body: bb, captures: bc },
            ) => {
                ap == bp && ab.content_hash == bb.content_hash && {
                    let a_caps = sorted_captures(ac);
                    let b_caps = sorted_captures(bc);
                    a_caps.len() == b_caps.len()
                        && a_caps.iter().zip(b_caps.iter()).all(|((aid, av), (bid, bv))| {
                            aid == bid && av == bv
                        })
                }
            }
            (
                Value::Complex { re: ar, im: ai, dimension: ad },
                Value::Complex { re: br, im: bi, dimension: bd },
            ) => ar.to_bits() == br.to_bits() && ai.to_bits() == bi.to_bits() && ad == bd,
            (
                Value::Orientation { w: aw, x: ax, y: ay, z: az },
                Value::Orientation { w: bw, x: bx, y: by, z: bz },
            ) => {
                aw.to_bits() == bw.to_bits()
                    && ax.to_bits() == bx.to_bits()
                    && ay.to_bits() == by.to_bits()
                    && az.to_bits() == bz.to_bits()
            }
            (
                Value::Range { lower: al, upper: au, lower_inclusive: ali, upper_inclusive: aui },
                Value::Range { lower: bl, upper: bu, lower_inclusive: bli, upper_inclusive: bui },
            ) => {
                debug_assert!(
                    al.is_some() || !ali,
                    "Range invariant violated: lower_inclusive must be false when lower is None"
                );
                debug_assert!(
                    au.is_some() || !aui,
                    "Range invariant violated: upper_inclusive must be false when upper is None"
                );
                debug_assert!(
                    bl.is_some() || !bli,
                    "Range invariant violated: lower_inclusive must be false when lower is None"
                );
                debug_assert!(
                    bu.is_some() || !bui,
                    "Range invariant violated: upper_inclusive must be false when upper is None"
                );
                al == bl && au == bu && ali == bli && aui == bui
            }
            (Value::Undef, Value::Undef) => true,
            _ => false,
        }
    }
}

impl Eq for Value {}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Value {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;

        // Type-tag discriminant for cross-type ordering:
        // Undef=0, Bool=1, Int=2, Real=3, Scalar=4, String=5, Enum=6, List=7, Set=8, Map=9, Option=10, Field=11, Lambda=12, Tensor=13, Complex=14, Orientation=15, Range=16, Point=17, Vector=18
        fn type_tag(v: &Value) -> u8 {
            match v {
                Value::Undef => 0,
                Value::Bool(_) => 1,
                Value::Int(_) => 2,
                Value::Real(_) => 3,
                Value::Scalar { .. } => 4,
                Value::String(_) => 5,
                Value::Enum { .. } => 6,
                Value::List(_) => 7,
                Value::Set(_) => 8,
                Value::Map(_) => 9,
                Value::Option(_) => 10,
                Value::Field { .. } => 11,
                Value::Lambda { .. } => 12,
                Value::Tensor(_) => 13,
                Value::Complex { .. } => 14,
                Value::Orientation { .. } => 15,
                Value::Range { .. } => 16,
                Value::Point(_) => 17,
                Value::Vector(_) => 18,
            }
        }

        let tag_a = type_tag(self);
        let tag_b = type_tag(other);

        if tag_a != tag_b {
            return tag_a.cmp(&tag_b);
        }

        // Same type — compare within type
        match (self, other) {
            (Value::Undef, Value::Undef) => Ordering::Equal,
            (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
            (Value::Int(a), Value::Int(b)) => a.cmp(b),
            (Value::Real(a), Value::Real(b)) => {
                // Total order via to_bits(), consistent with PartialEq
                a.to_bits().cmp(&b.to_bits())
            }
            (Value::Scalar { si_value: a, dimension: ad }, Value::Scalar { si_value: b, dimension: bd }) => {
                // Compare by dimension first, then by value bits
                ad.cmp(bd).then_with(|| a.to_bits().cmp(&b.to_bits()))
            }
            (Value::String(a), Value::String(b)) => a.cmp(b),
            (Value::Enum { type_name: a, variant: av }, Value::Enum { type_name: b, variant: bv }) => {
                a.cmp(b).then_with(|| av.cmp(bv))
            }
            (Value::List(a), Value::List(b)) => a.cmp(b),
            (Value::Tensor(a), Value::Tensor(b)) => a.cmp(b),
            (Value::Point(a), Value::Point(b)) => a.cmp(b),
            (Value::Vector(a), Value::Vector(b)) => a.cmp(b),
            (Value::Set(a), Value::Set(b)) => a.cmp(b),
            (Value::Map(a), Value::Map(b)) => {
                // Lexicographic on (key, value) pairs in sorted key order
                a.iter().cmp(b.iter())
            }
            (Value::Option(a), Value::Option(b)) => a.cmp(b),
            (
                Value::Field { domain_type: ad, codomain_type: ac, source: as_, lambda: al },
                Value::Field { domain_type: bd, codomain_type: bc, source: bs, lambda: bl },
            ) => {
                format!("{}", ad).cmp(&format!("{}", bd))
                    .then_with(|| format!("{}", ac).cmp(&format!("{}", bc)))
                    .then_with(|| format!("{:?}", as_).cmp(&format!("{:?}", bs)))
                    .then_with(|| al.cmp(bl))
            }
            (
                Value::Lambda { params: ap, body: ab, captures: ac },
                Value::Lambda { params: bp, body: bb, captures: bc },
            ) => {
                ap.cmp(bp)
                    .then_with(|| ab.content_hash.0.cmp(&bb.content_hash.0))
                    .then_with(|| {
                        sorted_captures(ac).cmp(&sorted_captures(bc))
                    })
            }
            (
                Value::Complex { re: ar, im: ai, dimension: ad },
                Value::Complex { re: br, im: bi, dimension: bd },
            ) => {
                ad.cmp(bd)
                    .then_with(|| ar.to_bits().cmp(&br.to_bits()))
                    .then_with(|| ai.to_bits().cmp(&bi.to_bits()))
            }
            (
                Value::Orientation { w: aw, x: ax, y: ay, z: az },
                Value::Orientation { w: bw, x: bx, y: by, z: bz },
            ) => {
                aw.to_bits().cmp(&bw.to_bits())
                    .then_with(|| ax.to_bits().cmp(&bx.to_bits()))
                    .then_with(|| ay.to_bits().cmp(&by.to_bits()))
                    .then_with(|| az.to_bits().cmp(&bz.to_bits()))
            }
            (
                Value::Range { lower: al, upper: au, lower_inclusive: ali, upper_inclusive: aui },
                Value::Range { lower: bl, upper: bu, lower_inclusive: bli, upper_inclusive: bui },
            ) => {
                debug_assert!(
                    al.is_some() || !ali,
                    "Range invariant violated: lower_inclusive must be false when lower is None"
                );
                debug_assert!(
                    au.is_some() || !aui,
                    "Range invariant violated: upper_inclusive must be false when upper is None"
                );
                debug_assert!(
                    bl.is_some() || !bli,
                    "Range invariant violated: lower_inclusive must be false when lower is None"
                );
                debug_assert!(
                    bu.is_some() || !bui,
                    "Range invariant violated: upper_inclusive must be false when upper is None"
                );
                ali.cmp(bli)
                    .then_with(|| al.cmp(bl))
                    .then_with(|| aui.cmp(bui))
                    .then_with(|| au.cmp(bu))
            }
            _ => unreachable!("same type tag but different variants"),
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Bool(b) => write!(f, "{}", b),
            Value::Int(i) => write!(f, "{}", i),
            Value::Real(r) => {
                // Format cleanly: no trailing ".0" for whole numbers.
                // Use {:.0} instead of `as i64` to avoid silent saturation
                // for f64 values beyond i64 range (e.g., 1e20).
                if *r == r.trunc() && r.is_finite() {
                    write!(f, "{:.0}", r)
                } else {
                    write!(f, "{}", r)
                }
            }
            Value::String(s) => write!(f, "\"{}\"", s),
            Value::Scalar { si_value, dimension } => {
                write!(f, "{} {}", si_value, dimension)
            }
            Value::Enum { type_name, variant } => write!(f, "{}::{}", type_name, variant),
            Value::List(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "]")
            }
            Value::Tensor(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "]")
            }
            Value::Point(items) => {
                write!(f, "point(")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, ")")
            }
            Value::Vector(items) => {
                write!(f, "vec(")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, ")")
            }
            Value::Set(items) => {
                write!(f, "{{")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "}}")
            }
            Value::Map(entries) => {
                write!(f, "{{")?;
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, "}}")
            }
            Value::Option(None) => write!(f, "None"),
            Value::Option(Some(v)) => write!(f, "Some({})", v),
            Value::Field { domain_type, codomain_type, source, .. } => {
                write!(f, "Field<{}, {}>({:?})", domain_type, codomain_type, source)
            }
            Value::Lambda { params, .. } => {
                write!(f, "|")?;
                for (i, (name, _)) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", name)?;
                }
                write!(f, "| <lambda>")
            }
            Value::Complex { re, im, dimension } => {
                // Format re and im using Real's whole-number convention (no trailing .0)
                let fmt_f64 = |v: f64| -> String {
                    if v == v.trunc() && v.is_finite() {
                        format!("{:.0}", v)
                    } else {
                        format!("{}", v)
                    }
                };
                let re_str = fmt_f64(*re);
                let im_abs_str = fmt_f64(im.abs());
                let sign = if im.is_sign_negative() { "-" } else { "+" };
                if dimension.is_dimensionless() {
                    write!(f, "{}{}{}", re_str, sign, im_abs_str)?;
                    write!(f, "i")
                } else {
                    write!(f, "({}{}{}i) {}", re_str, sign, im_abs_str, dimension)
                }
            }
            Value::Orientation { w, x, y, z } => {
                // Format quaternion components using same whole-number convention as Real
                let fmt_f64 = |v: f64| -> String {
                    if v == v.trunc() && v.is_finite() {
                        format!("{:.0}", v)
                    } else {
                        format!("{}", v)
                    }
                };
                write!(f, "[{}, {}, {}, {}]q", fmt_f64(*w), fmt_f64(*x), fmt_f64(*y), fmt_f64(*z))
            }
            Value::Range { lower, upper, lower_inclusive, upper_inclusive } => {
                debug_assert!(
                    lower.is_some() || !lower_inclusive,
                    "Range invariant violated: lower_inclusive must be false when lower is None"
                );
                debug_assert!(
                    upper.is_some() || !upper_inclusive,
                    "Range invariant violated: upper_inclusive must be false when upper is None"
                );
                let lb = if *lower_inclusive { '[' } else { '(' };
                let ub = if *upper_inclusive { ']' } else { ')' };
                let lower_str = match lower {
                    None => "-inf".to_string(),
                    Some(v) => format!("{}", v),
                };
                let upper_str = match upper {
                    None => "inf".to_string(),
                    Some(v) => format!("{}", v),
                };
                write!(f, "{}{}..{}{}", lb, lower_str, upper_str, ub)
            }
            Value::Undef => write!(f, "undef"),
        }
    }
}

/// The determinacy state of a value cell in the evaluation graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeterminacyState {
    /// Value is fully determined.
    Determined,
    /// Value cannot be determined (missing input, cycle, error).
    Undetermined,
    /// Value is provisionally determined (may change during solving).
    Provisional,
    /// Value is marked auto — to be resolved by the constraint solver.
    Auto,
}

/// The satisfaction state of a constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Satisfaction {
    /// All constraint conditions are met.
    Satisfied,
    /// At least one constraint condition is violated.
    Violated,
    /// Satisfaction cannot be determined (undef inputs).
    Indeterminate,
}

impl Satisfaction {
    /// Compute a content hash for incremental caching.
    /// Domain-separated with tag byte [10], exclusively reserved for Satisfaction
    /// (Value tags use 0-9, 11+).
    pub fn content_hash(&self) -> ContentHash {
        match self {
            Satisfaction::Satisfied => ContentHash::of(&[10, 0]),
            Satisfaction::Violated => ContentHash::of(&[10, 1]),
            Satisfaction::Indeterminate => ContentHash::of(&[10, 2]),
        }
    }
}

/// An error produced during value evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalError(pub String);

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for EvalError {}

/// Freshness of a cached value (for incremental evaluation).
///
/// M2 model: tracks evaluation lifecycle with richer state than M1's
/// simple Fresh/Stale/Uncomputed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freshness {
    /// Value is fully computed and up-to-date.
    Final,
    /// Value is a provisional result from an in-progress evaluation pass.
    Intermediate { generation: u64 },
    /// Value has been requested but not yet computed.
    /// `last_substantive` holds the content hash of the last known-good value, if any.
    Pending { last_substantive: Option<ContentHash> },
    /// Evaluation failed with an error.
    Failed { error: EvalError },
}

/// Sort captures by ValueCellId for deterministic comparison/hashing.
fn sorted_captures(captures: &ValueMap) -> Vec<(&ValueCellId, &Value)> {
    let mut caps: Vec<_> = captures.iter().collect();
    caps.sort_by_key(|(id, _)| *id);
    caps
}

/// Map from ValueCellId to Value. Uses PersistentMap (im::HashMap) for
/// O(1) structural-sharing clones.
#[derive(Debug, Clone, Default)]
pub struct ValueMap {
    inner: PersistentMap<ValueCellId, Value>,
}

impl ValueMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: &ValueCellId) -> Option<&Value> {
        self.inner.get(id)
    }

    pub fn insert(&mut self, id: ValueCellId, value: Value) {
        self.inner.insert(id, value);
    }

    pub fn remove(&mut self, id: &ValueCellId) {
        self.inner.remove(id);
    }

    pub fn contains(&self, id: &ValueCellId) -> bool {
        self.inner.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ValueCellId, &Value)> {
        self.inner.iter()
    }

    /// Get a value, returning Undef if not present.
    pub fn get_or_undef(&self, id: &ValueCellId) -> Value {
        self.inner.get(id).cloned().unwrap_or(Value::Undef)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_content_hash_determinism() {
        let v1 = Value::Scalar {
            si_value: 0.08,
            dimension: DimensionVector::LENGTH,
        };
        let v2 = Value::Scalar {
            si_value: 0.08,
            dimension: DimensionVector::LENGTH,
        };
        assert_eq!(v1.content_hash(), v2.content_hash());
    }

    #[test]
    fn real_neg_zero_not_normalized_in_hash() {
        // -0.0 and 0.0 are different via PartialEq (to_bits), so content_hash must differ
        let pos = Value::Real(0.0);
        let neg = Value::Real(-0.0);
        assert_ne!(pos.content_hash(), neg.content_hash());
    }

    #[test]
    fn real_neg_zero_hash_differs_from_pos_zero() {
        let pos = Value::Real(0.0);
        let neg = Value::Real(-0.0);
        // PartialEq uses to_bits(), so -0.0 != 0.0
        assert_ne!(pos, neg);
        // Therefore content_hash must also differ (cache invariant)
        assert_ne!(pos.content_hash(), neg.content_hash());
    }

    #[test]
    fn scalar_neg_zero_hash_differs_from_pos_zero() {
        let pos = Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH };
        let neg = Value::Scalar { si_value: -0.0, dimension: DimensionVector::LENGTH };
        // PartialEq uses to_bits(), so -0.0 != 0.0
        assert_ne!(pos, neg);
        // Therefore content_hash must also differ (cache invariant)
        assert_ne!(pos.content_hash(), neg.content_hash());
    }

    #[test]
    fn hash_equality_invariant_real() {
        // Spot-check: for -0.0 and 0.0, if a != b then a.content_hash() != b.content_hash()
        let a = Value::Real(-0.0);
        let b = Value::Real(0.0);
        if a != b {
            assert_ne!(
                a.content_hash(),
                b.content_hash(),
                "hash-equality invariant violated: unequal values must have different hashes"
            );
        }
    }

    #[test]
    fn nan_normalized() {
        let nan1 = Value::Real(f64::NAN);
        let nan2 = Value::Real(f64::NAN);
        assert_eq!(nan1.content_hash(), nan2.content_hash());
    }

    #[test]
    fn different_values_different_hashes() {
        let a = Value::length(0.08);
        let b = Value::length(0.10);
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn different_dimensions_different_hashes() {
        let len = Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::LENGTH,
        };
        let mass = Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::MASS,
        };
        assert_ne!(len.content_hash(), mass.content_hash());
    }

    #[test]
    fn test_freshness_final() {
        let f = Freshness::Final;
        let f2 = f.clone();
        assert_eq!(f, f2);
        assert_eq!(format!("{:?}", f), "Final");
    }

    #[test]
    fn test_freshness_intermediate() {
        let f = Freshness::Intermediate { generation: 42 };
        let f2 = f.clone();
        assert_eq!(f, f2);
        match &f {
            Freshness::Intermediate { generation } => assert_eq!(*generation, 42),
            _ => panic!("expected Intermediate"),
        }
    }

    #[test]
    fn test_freshness_pending_none() {
        let f = Freshness::Pending { last_substantive: None };
        let f2 = f.clone();
        assert_eq!(f, f2);
        match &f {
            Freshness::Pending { last_substantive } => assert!(last_substantive.is_none()),
            _ => panic!("expected Pending"),
        }
    }

    #[test]
    fn test_freshness_pending_some() {
        let hash = ContentHash::of(b"test");
        let f = Freshness::Pending { last_substantive: Some(hash) };
        let f2 = f.clone();
        assert_eq!(f, f2);
        match &f {
            Freshness::Pending { last_substantive } => assert_eq!(*last_substantive, Some(hash)),
            _ => panic!("expected Pending"),
        }
    }

    #[test]
    fn test_freshness_failed() {
        let err = EvalError("type mismatch".to_string());
        let f = Freshness::Failed { error: err.clone() };
        let f2 = f.clone();
        assert_eq!(f, f2);
        match &f {
            Freshness::Failed { error } => assert_eq!(error.0, "type mismatch"),
            _ => panic!("expected Failed"),
        }
    }

    #[test]
    fn test_eval_error_display() {
        let err = EvalError("division by zero".to_string());
        assert_eq!(format!("{}", err), "division by zero");
        assert_eq!(err.0, "division by zero");

        // Verify Clone and PartialEq
        let err2 = err.clone();
        assert_eq!(err, err2);
    }

    #[test]
    fn value_map_get_or_undef() {
        let mut map = ValueMap::new();
        let id = ValueCellId::new("Bracket", "width");
        map.insert(id.clone(), Value::length(0.08));
        assert!(!map.get_or_undef(&id).is_undef());
        assert!(map.get_or_undef(&ValueCellId::new("Bracket", "missing")).is_undef());
    }

    #[test]
    fn value_map_clone_structural_sharing() {
        let mut original = ValueMap::new();
        let id_width = ValueCellId::new("Bracket", "width");
        let id_height = ValueCellId::new("Bracket", "height");
        let id_depth = ValueCellId::new("Bracket", "depth");

        original.insert(id_width.clone(), Value::length(0.08));
        original.insert(id_height.clone(), Value::length(0.10));

        // Clone the map (O(1) structural sharing via im::HashMap)
        let mut cloned = original.clone();

        // Insert into the clone
        cloned.insert(id_depth.clone(), Value::length(0.05));

        // Original is unmodified
        assert_eq!(original.len(), 2);
        assert!(!original.contains(&id_depth));
        assert!(original.contains(&id_width));

        // Clone has all three entries
        assert_eq!(cloned.len(), 3);
        assert!(cloned.contains(&id_depth));
        assert!(cloned.contains(&id_width));

        // Original values are still correct
        match original.get(&id_width) {
            Some(Value::Scalar { si_value, .. }) => assert!((si_value - 0.08).abs() < 1e-10),
            other => panic!("Expected Scalar, got {:?}", other),
        }
    }

    #[test]
    fn satisfaction_content_hash_deterministic() {
        // Same variant produces same hash on repeated calls
        let h1 = Satisfaction::Satisfied.content_hash();
        let h2 = Satisfaction::Satisfied.content_hash();
        assert_eq!(h1, h2);

        let h3 = Satisfaction::Violated.content_hash();
        let h4 = Satisfaction::Violated.content_hash();
        assert_eq!(h3, h4);

        let h5 = Satisfaction::Indeterminate.content_hash();
        let h6 = Satisfaction::Indeterminate.content_hash();
        assert_eq!(h5, h6);
    }

    #[test]
    fn determinacy_state_auto_exists_and_is_distinct() {
        // Auto variant should exist and be distinct from other variants
        let auto = DeterminacyState::Auto;
        let determined = DeterminacyState::Determined;
        let undetermined = DeterminacyState::Undetermined;
        let provisional = DeterminacyState::Provisional;

        assert_ne!(auto, determined);
        assert_ne!(auto, undetermined);
        assert_ne!(auto, provisional);
    }

    #[test]
    fn determinacy_state_auto_is_copy_clone_eq_hash() {
        let auto = DeterminacyState::Auto;
        let auto2 = auto; // Copy
        assert_eq!(auto, auto2); // PartialEq + Eq

        let auto3 = auto.clone(); // Clone
        assert_eq!(auto, auto3);

        // Hash: usable as HashMap key
        use std::collections::HashMap;
        let mut map = HashMap::new();
        map.insert(auto, "auto");
        assert_eq!(map.get(&DeterminacyState::Auto), Some(&"auto"));
    }

    #[test]
    fn determinacy_state_auto_discriminant_is_3() {
        // Determined=0, Undetermined=1, Provisional=2, Auto=3
        assert_eq!(DeterminacyState::Determined as u8, 0);
        assert_eq!(DeterminacyState::Undetermined as u8, 1);
        assert_eq!(DeterminacyState::Provisional as u8, 2);
        assert_eq!(DeterminacyState::Auto as u8, 3);
    }

    // --- Ord tests (step-1) ---

    #[test]
    fn value_ord_cross_type_ordering() {
        // Undef < Bool < Int < Real < Scalar < String
        let undef = Value::Undef;
        let bool_f = Value::Bool(false);
        let bool_t = Value::Bool(true);
        let int0 = Value::Int(0);
        let real0 = Value::Real(0.0);
        let scalar = Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::LENGTH,
        };
        let string = Value::String("z".into());

        assert!(undef < bool_f);
        assert!(bool_f < bool_t);
        assert!(bool_t < int0);
        assert!(int0 < real0);
        assert!(real0 < scalar);
        assert!(scalar < string);
    }

    #[test]
    fn value_ord_within_int() {
        assert!(Value::Int(1) < Value::Int(2));
        assert!(Value::Int(-10) < Value::Int(0));
        assert_eq!(Value::Int(5).cmp(&Value::Int(5)), std::cmp::Ordering::Equal);
    }

    #[test]
    fn value_ord_within_string() {
        assert!(Value::String("a".into()) < Value::String("b".into()));
        assert!(Value::String("abc".into()) < Value::String("abd".into()));
    }

    #[test]
    fn value_ord_within_real_and_nan() {
        // Normal ordering
        assert!(Value::Real(1.0) < Value::Real(2.0));
        // NaN consistency: NaN should have a defined position (via to_bits)
        let nan = Value::Real(f64::NAN);
        let inf = Value::Real(f64::INFINITY);
        // Just verify it doesn't panic and gives consistent results
        let _ = nan.cmp(&inf);
        assert_eq!(nan.cmp(&nan), std::cmp::Ordering::Equal);
    }

    #[test]
    fn value_ord_neg_zero() {
        // -0.0 and +0.0 have different bits, so they may have different ordering
        // (consistent with PartialEq which uses to_bits)
        let pos = Value::Real(0.0);
        let neg = Value::Real(-0.0);
        // They should have a defined comparison (not panic)
        let _ = pos.cmp(&neg);
    }

    // --- Option tests (step-11) ---

    #[test]
    fn value_option_some_and_none() {
        let some = Value::Option(Some(Box::new(Value::Int(42))));
        let none = Value::Option(None);
        assert_ne!(some, none);
    }

    #[test]
    fn value_option_equality() {
        let a = Value::Option(Some(Box::new(Value::Int(42))));
        let b = Value::Option(Some(Box::new(Value::Int(42))));
        let c = Value::Option(Some(Box::new(Value::Int(99))));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(Value::Option(None), Value::Option(None));
    }

    #[test]
    fn value_option_ordering() {
        // None < Some(anything)
        assert!(Value::Option(None) < Value::Option(Some(Box::new(Value::Int(0)))));
        // Some orders by inner value
        assert!(
            Value::Option(Some(Box::new(Value::Int(1))))
                < Value::Option(Some(Box::new(Value::Int(2))))
        );
        // Option sorts after Map
        let m = Value::Map(std::collections::BTreeMap::new());
        assert!(m < Value::Option(None));
    }

    #[test]
    fn value_option_content_hash() {
        let some1 = Value::Option(Some(Box::new(Value::Int(1))));
        let some2 = Value::Option(Some(Box::new(Value::Int(1))));
        let some3 = Value::Option(Some(Box::new(Value::Int(2))));
        let none = Value::Option(None);
        assert_eq!(some1.content_hash(), some2.content_hash());
        assert_ne!(some1.content_hash(), some3.content_hash());
        assert_ne!(some1.content_hash(), none.content_hash());
    }

    // --- Map tests (step-9) ---

    #[test]
    fn value_map_basic() {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("a".into()), Value::Int(1));
        m.insert(Value::String("b".into()), Value::Int(2));
        let v = Value::Map(m);
        if let Value::Map(ref inner) = v {
            assert_eq!(inner.len(), 2);
            assert_eq!(inner.get(&Value::String("a".into())), Some(&Value::Int(1)));
        } else {
            panic!("expected Map");
        }
    }

    #[test]
    fn value_map_equality() {
        use std::collections::BTreeMap;
        let mut m1 = BTreeMap::new();
        m1.insert(Value::String("a".into()), Value::Int(1));
        let mut m2 = BTreeMap::new();
        m2.insert(Value::String("a".into()), Value::Int(1));
        let mut m3 = BTreeMap::new();
        m3.insert(Value::String("a".into()), Value::Int(2));
        assert_eq!(Value::Map(m1), Value::Map(m2.clone()));
        assert_ne!(Value::Map(m2), Value::Map(m3));
    }

    #[test]
    fn value_map_ordering() {
        use std::collections::BTreeMap;
        // Map sorts after Set
        let s = Value::Set(std::collections::BTreeSet::new());
        let m = Value::Map(BTreeMap::new());
        assert!(s < m);
    }

    #[test]
    fn value_map_content_hash() {
        use std::collections::BTreeMap;
        let mut m1 = BTreeMap::new();
        m1.insert(Value::String("a".into()), Value::Int(1));
        let mut m2 = BTreeMap::new();
        m2.insert(Value::String("a".into()), Value::Int(1));
        let mut m3 = BTreeMap::new();
        m3.insert(Value::String("a".into()), Value::Int(2));
        assert_eq!(Value::Map(m1).content_hash(), Value::Map(m2.clone()).content_hash());
        assert_ne!(Value::Map(m2).content_hash(), Value::Map(m3).content_hash());
    }

    // --- Set tests (step-7) ---

    #[test]
    fn value_set_basic() {
        use std::collections::BTreeSet;
        let mut s = BTreeSet::new();
        s.insert(Value::Int(3));
        s.insert(Value::Int(1));
        s.insert(Value::Int(2));
        let v = Value::Set(s);
        // Verify it contains all elements
        if let Value::Set(ref inner) = v {
            assert_eq!(inner.len(), 3);
            assert!(inner.contains(&Value::Int(1)));
            assert!(inner.contains(&Value::Int(2)));
            assert!(inner.contains(&Value::Int(3)));
        } else {
            panic!("expected Set");
        }
    }

    #[test]
    fn value_set_equality() {
        use std::collections::BTreeSet;
        let mut s1 = BTreeSet::new();
        s1.insert(Value::Int(1));
        s1.insert(Value::Int(2));
        let mut s2 = BTreeSet::new();
        s2.insert(Value::Int(2));
        s2.insert(Value::Int(1)); // same elements, different insertion order
        assert_eq!(Value::Set(s1), Value::Set(s2));
    }

    #[test]
    fn value_set_ordering() {
        use std::collections::BTreeSet;
        let mut s1 = BTreeSet::new();
        s1.insert(Value::Int(1));
        let mut s2 = BTreeSet::new();
        s2.insert(Value::Int(2));
        // Set sorts after List
        assert!(Value::List(vec![]) < Value::Set(s1.clone()));
        // Between sets: lexicographic on sorted elements
        assert!(Value::Set(s1) < Value::Set(s2));
    }

    #[test]
    fn value_set_content_hash() {
        use std::collections::BTreeSet;
        let mut s1 = BTreeSet::new();
        s1.insert(Value::Int(1));
        s1.insert(Value::Int(2));
        let mut s2 = BTreeSet::new();
        s2.insert(Value::Int(2));
        s2.insert(Value::Int(1));
        assert_eq!(Value::Set(s1).content_hash(), Value::Set(s2).content_hash());
    }

    // --- List tests (step-5) ---

    #[test]
    fn value_list_equality() {
        let a = Value::List(vec![Value::Int(1), Value::Int(2)]);
        let b = Value::List(vec![Value::Int(1), Value::Int(2)]);
        let c = Value::List(vec![Value::Int(1), Value::Int(3)]);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn value_list_empty() {
        let empty = Value::List(vec![]);
        let non_empty = Value::List(vec![Value::Int(1)]);
        assert_ne!(empty, non_empty);
        assert_eq!(empty, Value::List(vec![]));
    }

    #[test]
    fn value_list_nested() {
        let nested = Value::List(vec![
            Value::List(vec![Value::Int(1)]),
            Value::List(vec![Value::Int(2)]),
        ]);
        let nested2 = Value::List(vec![
            Value::List(vec![Value::Int(1)]),
            Value::List(vec![Value::Int(2)]),
        ]);
        assert_eq!(nested, nested2);
    }

    #[test]
    fn value_list_ordering() {
        // Lexicographic ordering
        let a = Value::List(vec![Value::Int(1), Value::Int(2)]);
        let b = Value::List(vec![Value::Int(1), Value::Int(3)]);
        assert!(a < b);

        // Shorter list < longer list with same prefix
        let short = Value::List(vec![Value::Int(1)]);
        let long = Value::List(vec![Value::Int(1), Value::Int(2)]);
        assert!(short < long);

        // List sorts after Enum
        let enum_val = Value::Enum { type_name: "Z".into(), variant: "Z".into() };
        assert!(enum_val < Value::List(vec![]));
    }

    #[test]
    fn value_list_content_hash() {
        let a = Value::List(vec![Value::Int(1), Value::Int(2)]);
        let b = Value::List(vec![Value::Int(1), Value::Int(2)]);
        let c = Value::List(vec![Value::Int(2), Value::Int(1)]);
        assert_eq!(a.content_hash(), b.content_hash());
        assert_ne!(a.content_hash(), c.content_hash());
    }

    // --- Enum tests (step-3) ---

    #[test]
    fn value_enum_debug() {
        let v = Value::Enum {
            type_name: "Color".into(),
            variant: "Red".into(),
        };
        let dbg = format!("{:?}", v);
        assert!(dbg.contains("Color"));
        assert!(dbg.contains("Red"));
    }

    #[test]
    fn value_enum_equality() {
        let a = Value::Enum { type_name: "Color".into(), variant: "Red".into() };
        let b = Value::Enum { type_name: "Color".into(), variant: "Red".into() };
        let c = Value::Enum { type_name: "Color".into(), variant: "Blue".into() };
        let d = Value::Enum { type_name: "Shape".into(), variant: "Red".into() };
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    #[test]
    fn value_enum_ordering() {
        let enum_val = Value::Enum { type_name: "Color".into(), variant: "Red".into() };
        let string_val = Value::String("zzz".into());
        // Enum sorts after String
        assert!(enum_val > string_val);

        // Within Enum: sort by type_name then variant
        let a = Value::Enum { type_name: "Color".into(), variant: "Blue".into() };
        let b = Value::Enum { type_name: "Color".into(), variant: "Red".into() };
        let c = Value::Enum { type_name: "Shape".into(), variant: "A".into() };
        assert!(a < b); // same type_name, Blue < Red
        assert!(b < c); // Color < Shape
    }

    #[test]
    fn value_enum_content_hash() {
        let a = Value::Enum { type_name: "Color".into(), variant: "Red".into() };
        let b = Value::Enum { type_name: "Color".into(), variant: "Red".into() };
        let c = Value::Enum { type_name: "Color".into(), variant: "Blue".into() };
        assert_eq!(a.content_hash(), b.content_hash()); // deterministic
        assert_ne!(a.content_hash(), c.content_hash()); // distinct
    }

    #[test]
    fn satisfaction_content_hash_distinct_variants() {
        let satisfied = Satisfaction::Satisfied.content_hash();
        let violated = Satisfaction::Violated.content_hash();
        let indeterminate = Satisfaction::Indeterminate.content_hash();

        assert_ne!(satisfied, violated);
        assert_ne!(satisfied, indeterminate);
        assert_ne!(violated, indeterminate);
    }

    // --- Display tests ---

    #[test]
    fn value_display_bool() {
        assert_eq!(format!("{}", Value::Bool(true)), "true");
        assert_eq!(format!("{}", Value::Bool(false)), "false");
    }

    #[test]
    fn value_display_int() {
        assert_eq!(format!("{}", Value::Int(42)), "42");
        assert_eq!(format!("{}", Value::Int(-7)), "-7");
        assert_eq!(format!("{}", Value::Int(0)), "0");
    }

    #[test]
    fn value_display_real() {
        assert_eq!(format!("{}", Value::Real(3.15)), "3.15");
        assert_eq!(format!("{}", Value::Real(0.0)), "0");
        assert_eq!(format!("{}", Value::Real(-2.5)), "-2.5");
    }

    #[test]
    fn value_display_string() {
        assert_eq!(format!("{}", Value::String("hello".into())), "\"hello\"");
        assert_eq!(format!("{}", Value::String("".into())), "\"\"");
    }

    #[test]
    fn value_display_scalar() {
        let v = Value::length(0.08);
        assert_eq!(format!("{}", v), "0.08 m");
    }

    #[test]
    fn value_display_undef() {
        assert_eq!(format!("{}", Value::Undef), "undef");
    }

    #[test]
    fn value_display_enum() {
        let v = Value::Enum {
            type_name: "Color".into(),
            variant: "Red".into(),
        };
        assert_eq!(format!("{}", v), "Color::Red");
    }

    #[test]
    fn value_display_list() {
        let v = Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        assert_eq!(format!("{}", v), "[1, 2, 3]");
        assert_eq!(format!("{}", Value::List(vec![])), "[]");
    }

    #[test]
    fn value_display_set() {
        use std::collections::BTreeSet;
        let mut s = BTreeSet::new();
        s.insert(Value::Int(1));
        s.insert(Value::Int(2));
        assert_eq!(format!("{}", Value::Set(s)), "{1, 2}");
        assert_eq!(format!("{}", Value::Set(BTreeSet::new())), "{}");
    }

    #[test]
    fn value_display_map() {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("a".into()), Value::Int(1));
        assert_eq!(format!("{}", Value::Map(m)), "{\"a\": 1}");
        assert_eq!(format!("{}", Value::Map(BTreeMap::new())), "{}");
    }

    #[test]
    fn value_display_option() {
        assert_eq!(format!("{}", Value::Option(None)), "None");
        assert_eq!(
            format!("{}", Value::Option(Some(Box::new(Value::Int(42))))),
            "Some(42)"
        );
    }

    // --- Value::Real Display large float regression tests (step-14) ---

    #[test]
    fn value_display_real_large_positive() {
        // 1e20 is beyond i64::MAX (~9.2e18), so `*r as i64` saturates to i64::MAX.
        // Expected: the full float representation, not the saturated i64 value.
        assert_eq!(
            format!("{}", Value::Real(1e20)),
            "100000000000000000000"
        );
    }

    #[test]
    fn value_display_real_large_negative() {
        assert_eq!(
            format!("{}", Value::Real(-1e20)),
            "-100000000000000000000"
        );
    }

    #[test]
    fn value_display_real_max_safe_integer() {
        // 2^53 = 9007199254740992, the max integer exactly representable as f64
        assert_eq!(
            format!("{}", Value::Real(9.007199254740992e15)),
            "9007199254740992"
        );
    }

    // --- Cross-domain hash collision regression tests (step-12) ---

    #[test]
    fn value_option_none_hash_not_equal_satisfaction_satisfied() {
        // Value::Option(None) and Satisfaction::Satisfied must not collide.
        // Both use tag [10] currently, which produces identical hashes.
        let value_hash = Value::Option(None).content_hash();
        let satisfaction_hash = Satisfaction::Satisfied.content_hash();
        assert_ne!(
            value_hash, satisfaction_hash,
            "Value::Option(None) hash collides with Satisfaction::Satisfied hash"
        );
    }

    #[test]
    fn value_option_some_hash_not_equal_satisfaction_violated() {
        // Value::Option(Some(Bool(true))) and Satisfaction::Violated must not collide.
        let value_hash =
            Value::Option(Some(Box::new(Value::Bool(true)))).content_hash();
        let satisfaction_hash = Satisfaction::Violated.content_hash();
        assert_ne!(
            value_hash, satisfaction_hash,
            "Value::Option(Some(Bool(true))) hash collides with Satisfaction::Violated hash"
        );
    }

    // --- Comprehensive tag uniqueness regression test (step-16) ---

    #[test]
    fn value_and_satisfaction_content_hash_tags_no_cross_domain_collisions() {
        // Build representative Value for each variant
        use std::collections::{BTreeMap, BTreeSet};
        let values: Vec<(&str, Value)> = vec![
            ("Bool(false)", Value::Bool(false)),
            ("Bool(true)", Value::Bool(true)),
            ("Int(0)", Value::Int(0)),
            ("Int(1)", Value::Int(1)),
            ("Real(0.0)", Value::Real(0.0)),
            ("Real(1.0)", Value::Real(1.0)),
            ("String(empty)", Value::String(String::new())),
            ("String(a)", Value::String("a".into())),
            (
                "Scalar(0,LENGTH)",
                Value::Scalar {
                    si_value: 0.0,
                    dimension: DimensionVector::LENGTH,
                },
            ),
            ("Undef", Value::Undef),
            (
                "Enum",
                Value::Enum {
                    type_name: "T".into(),
                    variant: "V".into(),
                },
            ),
            ("List(empty)", Value::List(vec![])),
            ("List([0])", Value::List(vec![Value::Int(0)])),
            ("Set(empty)", Value::Set(BTreeSet::new())),
            ("Map(empty)", Value::Map(BTreeMap::new())),
            ("Option(None)", Value::Option(None)),
            (
                "Option(Some(Bool(false)))",
                Value::Option(Some(Box::new(Value::Bool(false)))),
            ),
            (
                "Option(Some(Bool(true)))",
                Value::Option(Some(Box::new(Value::Bool(true)))),
            ),
        ];

        let satisfactions: Vec<(&str, ContentHash)> = vec![
            ("Satisfied", Satisfaction::Satisfied.content_hash()),
            ("Violated", Satisfaction::Violated.content_hash()),
            ("Indeterminate", Satisfaction::Indeterminate.content_hash()),
        ];

        // Every Value hash must differ from every Satisfaction hash
        for (vname, val) in &values {
            let vh = val.content_hash();
            for (sname, sh) in &satisfactions {
                assert_ne!(
                    vh, *sh,
                    "Value::{} content_hash collides with Satisfaction::{}",
                    vname, sname
                );
            }
        }
    }

    #[test]
    fn scalar_neg_zero_hash_consistency() {
        // si_value -0.0 and 0.0 are different via PartialEq (to_bits), so content_hash must differ
        let pos = Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH };
        let neg = Value::Scalar { si_value: -0.0, dimension: DimensionVector::LENGTH };
        assert_ne!(pos, neg);
        assert_ne!(pos.content_hash(), neg.content_hash());
    }

    // --- Field tests (step-11) ---

    #[test]
    fn value_field_variant() {
        use crate::ty::Type;
        let field_val = Value::Field {
            domain_type: Type::Real,
            codomain_type: Type::Real,
            source: FieldSourceKind::Analytical,
            lambda: Box::new(Value::Undef),
        };
        // Display
        let display = format!("{}", field_val);
        assert!(display.contains("Field"), "expected display to contain 'Field', got: {}", display);
        // Content hash determinism
        let field_val2 = Value::Field {
            domain_type: Type::Real,
            codomain_type: Type::Real,
            source: FieldSourceKind::Analytical,
            lambda: Box::new(Value::Undef),
        };
        assert_eq!(field_val.content_hash(), field_val2.content_hash());
        // Not equal to Undef
        assert_ne!(field_val, Value::Undef);
    }

    #[test]
    fn value_display_nested() {
        // List containing Option and Enum values
        let v = Value::List(vec![
            Value::Option(Some(Box::new(Value::Int(1)))),
            Value::Enum {
                type_name: "Color".into(),
                variant: "Red".into(),
            },
            Value::Option(None),
        ]);
        assert_eq!(format!("{}", v), "[Some(1), Color::Red, None]");
    }

    #[test]
    fn value_map_remove() {
        use crate::identity::ValueCellId;

        let id_a = ValueCellId::new("E", "a");
        let id_b = ValueCellId::new("E", "b");
        let id_c = ValueCellId::new("E", "c");

        let mut map = ValueMap::new();
        map.insert(id_a.clone(), Value::Int(1));
        map.insert(id_b.clone(), Value::Int(2));
        map.insert(id_c.clone(), Value::Int(3));
        assert_eq!(map.len(), 3);

        // Remove the middle entry
        map.remove(&id_b);

        assert_eq!(map.len(), 2);
        assert!(map.get(&id_b).is_none(), "removed entry should be gone");
        assert_eq!(map.get(&id_a), Some(&Value::Int(1)), "other entries should remain");
        assert_eq!(map.get(&id_c), Some(&Value::Int(3)), "other entries should remain");
    }

    // --- Value::Tensor tests ---

    #[test]
    fn value_tensor_construction_and_partial_eq() {
        // (a) rank-1 tensor with 3 length scalars equals itself rebuilt
        let t1 = Value::Tensor(vec![
            Value::length(0.08),
            Value::length(0.10),
            Value::length(0.12),
        ]);
        let t1b = Value::Tensor(vec![
            Value::length(0.08),
            Value::length(0.10),
            Value::length(0.12),
        ]);
        assert_eq!(t1, t1b);

        // (b) tensors with different elements are unequal
        let t1c = Value::Tensor(vec![
            Value::length(0.08),
            Value::length(0.10),
            Value::length(0.99),
        ]);
        assert_ne!(t1, t1c);

        // (c) rank-2 nested tensor (Tensor of Tensors) equals itself
        let inner_a = Value::Tensor(vec![Value::Int(1), Value::Int(2)]);
        let inner_b = Value::Tensor(vec![Value::Int(3), Value::Int(4)]);
        let t2 = Value::Tensor(vec![inner_a.clone(), inner_b.clone()]);
        let t2_copy = Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(1), Value::Int(2)]),
            Value::Tensor(vec![Value::Int(3), Value::Int(4)]),
        ]);
        assert_eq!(t2, t2_copy);

        // (d) Tensor([Int(1), Int(2)]) != List([Int(1), Int(2)]) — distinct variants
        let tensor_ints = Value::Tensor(vec![Value::Int(1), Value::Int(2)]);
        let list_ints = Value::List(vec![Value::Int(1), Value::Int(2)]);
        assert_ne!(tensor_ints, list_ints);
    }

    #[test]
    fn value_tensor_display() {
        // rank-1 tensor of 3 length scalars
        let t1 = Value::Tensor(vec![
            Value::length(0.08),
            Value::length(0.10),
            Value::length(0.12),
        ]);
        assert_eq!(format!("{}", t1), "[0.08 m, 0.1 m, 0.12 m]");

        // rank-2 nested tensor of Ints
        let t2 = Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(1), Value::Int(2)]),
            Value::Tensor(vec![Value::Int(3), Value::Int(4)]),
        ]);
        assert_eq!(format!("{}", t2), "[[1, 2], [3, 4]]");
    }

    #[test]
    fn value_tensor_content_hash_determinism() {
        // (a) identical rank-1 tensors produce identical hashes
        let t1 = Value::Tensor(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        let t1b = Value::Tensor(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        assert_eq!(t1.content_hash(), t1b.content_hash());

        // (b) different elements produce different hashes
        let t1c = Value::Tensor(vec![Value::Int(1), Value::Int(2), Value::Int(99)]);
        assert_ne!(t1.content_hash(), t1c.content_hash());

        // (c) Tensor hash differs from List hash with identical elements (tag [14] vs [7])
        let list = Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        assert_ne!(t1.content_hash(), list.content_hash());

        // (d) nested rank-2 tensor hash is deterministic
        let t2 = Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(1), Value::Int(2)]),
            Value::Tensor(vec![Value::Int(3), Value::Int(4)]),
        ]);
        let t2b = Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(1), Value::Int(2)]),
            Value::Tensor(vec![Value::Int(3), Value::Int(4)]),
        ]);
        assert_eq!(t2.content_hash(), t2b.content_hash());
    }

    #[test]
    fn value_tensor_ord() {
        // (a) Tensor type_tag (13) > Lambda type_tag (12) — cross-type ordering
        // We can't easily construct a Lambda here, but we can compare with Field (tag 11)
        // and verify Tensor sorts after Lambda by inspecting the Ord contract.
        // Instead, use List (tag=7) as a reference: Tensor (13) > List (7).
        let tensor = Value::Tensor(vec![Value::Int(1)]);
        let list = Value::List(vec![Value::Int(99)]);
        assert!(tensor > list, "Tensor (tag 13) should order after List (tag 7)");

        // (b) within-type lexicographic comparison of elements
        let ta = Value::Tensor(vec![Value::Int(1), Value::Int(2)]);
        let tb = Value::Tensor(vec![Value::Int(1), Value::Int(3)]);
        assert!(ta < tb);

        // (c) shorter tensor < longer tensor with same prefix elements
        let short = Value::Tensor(vec![Value::Int(1)]);
        let long = Value::Tensor(vec![Value::Int(1), Value::Int(2)]);
        assert!(short < long);
    }

    // ── Value::Complex Display tests (step-3) ─────────────────────────────────

    #[test]
    fn value_complex_display_positive_imaginary() {
        let v = Value::Complex { re: 3.0, im: 4.0, dimension: DimensionVector::DIMENSIONLESS };
        assert_eq!(format!("{}", v), "3+4i");
    }

    #[test]
    fn value_complex_display_negative_imaginary() {
        let v = Value::Complex { re: 3.0, im: -4.0, dimension: DimensionVector::DIMENSIONLESS };
        assert_eq!(format!("{}", v), "3-4i");
    }

    #[test]
    fn value_complex_display_fractional() {
        let v = Value::Complex { re: 3.5, im: 4.2, dimension: DimensionVector::DIMENSIONLESS };
        assert_eq!(format!("{}", v), "3.5+4.2i");
    }

    #[test]
    fn value_complex_display_dimensioned() {
        let v = Value::Complex { re: 3.0, im: 4.0, dimension: DimensionVector::LENGTH };
        assert_eq!(format!("{}", v), "(3+4i) m");
    }

    #[test]
    fn value_complex_display_zero_imaginary() {
        let v = Value::Complex { re: 3.0, im: 0.0, dimension: DimensionVector::DIMENSIONLESS };
        assert_eq!(format!("{}", v), "3+0i");
    }

    #[test]
    fn value_complex_display_negative_real() {
        let v = Value::Complex { re: -3.0, im: -4.0, dimension: DimensionVector::DIMENSIONLESS };
        assert_eq!(format!("{}", v), "-3-4i");
    }

    // ── Value::Complex PartialEq tests (step-4) ───────────────────────────────

    #[test]
    fn value_complex_eq_same() {
        let a = Value::Complex { re: 3.0, im: 4.0, dimension: DimensionVector::DIMENSIONLESS };
        let b = Value::Complex { re: 3.0, im: 4.0, dimension: DimensionVector::DIMENSIONLESS };
        assert_eq!(a, b);
    }

    #[test]
    fn value_complex_neq_different_re() {
        let a = Value::Complex { re: 3.0, im: 4.0, dimension: DimensionVector::DIMENSIONLESS };
        let b = Value::Complex { re: 5.0, im: 4.0, dimension: DimensionVector::DIMENSIONLESS };
        assert_ne!(a, b);
    }

    #[test]
    fn value_complex_neq_different_im() {
        let a = Value::Complex { re: 3.0, im: 4.0, dimension: DimensionVector::DIMENSIONLESS };
        let b = Value::Complex { re: 3.0, im: 5.0, dimension: DimensionVector::DIMENSIONLESS };
        assert_ne!(a, b);
    }

    #[test]
    fn value_complex_neq_different_dimension() {
        let a = Value::Complex { re: 3.0, im: 4.0, dimension: DimensionVector::DIMENSIONLESS };
        let b = Value::Complex { re: 3.0, im: 4.0, dimension: DimensionVector::LENGTH };
        assert_ne!(a, b);
    }

    #[test]
    fn value_complex_neg_zero_distinguished() {
        // -0.0 vs 0.0 distinguished via to_bits()
        let pos = Value::Complex { re: 0.0, im: 0.0, dimension: DimensionVector::DIMENSIONLESS };
        let neg_re = Value::Complex { re: -0.0, im: 0.0, dimension: DimensionVector::DIMENSIONLESS };
        let neg_im = Value::Complex { re: 0.0, im: -0.0, dimension: DimensionVector::DIMENSIONLESS };
        assert_ne!(pos, neg_re);
        assert_ne!(pos, neg_im);
    }

    #[test]
    fn value_complex_neq_real() {
        let c = Value::Complex { re: 3.0, im: 0.0, dimension: DimensionVector::DIMENSIONLESS };
        assert_ne!(c, Value::Real(3.0));
    }

    #[test]
    fn value_complex_neq_scalar() {
        let c = Value::Complex { re: 3.0, im: 0.0, dimension: DimensionVector::LENGTH };
        let s = Value::Scalar { si_value: 3.0, dimension: DimensionVector::LENGTH };
        assert_ne!(c, s);
    }

    // ── Value::Complex Ord tests (step-5) ─────────────────────────────────────

    #[test]
    fn value_complex_sorts_after_tensor() {
        // Complex type_tag=14 > Tensor type_tag=13
        let complex = Value::Complex { re: 0.0, im: 0.0, dimension: DimensionVector::DIMENSIONLESS };
        let tensor = Value::Tensor(vec![Value::Int(99)]);
        assert!(complex > tensor, "Complex (tag 14) should order after Tensor (tag 13)");
    }

    #[test]
    fn value_complex_sorts_before_undef() {
        // Undef tag=0, Complex tag=14 — Complex > Undef
        // (lower tag sorts first, so Undef=0 < Complex=14)
        // But also test vs something with tag > 14 doesn't exist yet,
        // so just verify cross-type ordering is consistent
        let complex = Value::Complex { re: 0.0, im: 0.0, dimension: DimensionVector::DIMENSIONLESS };
        let undef = Value::Undef;
        assert!(complex > undef, "Complex (tag 14) should order after Undef (tag 0)");
    }

    #[test]
    fn value_complex_ord_dimension_first() {
        // Same re/im, different dimension — dimension compared first
        // LENGTH > DIMENSIONLESS in DimensionVector ordering
        let a = Value::Complex { re: 1.0, im: 1.0, dimension: DimensionVector::DIMENSIONLESS };
        let b = Value::Complex { re: 1.0, im: 1.0, dimension: DimensionVector::LENGTH };
        // They should not be equal; whichever dimension ordering, they differ
        assert_ne!(a.cmp(&b), std::cmp::Ordering::Equal);
    }

    #[test]
    fn value_complex_ord_re_second() {
        // Same dimension, different re — re bits compared second
        let a = Value::Complex { re: 1.0, im: 0.0, dimension: DimensionVector::DIMENSIONLESS };
        let b = Value::Complex { re: 2.0, im: 0.0, dimension: DimensionVector::DIMENSIONLESS };
        assert!(a < b);
    }

    #[test]
    fn value_complex_ord_im_third() {
        // Same dimension+re, different im — im bits compared third
        let a = Value::Complex { re: 1.0, im: 1.0, dimension: DimensionVector::DIMENSIONLESS };
        let b = Value::Complex { re: 1.0, im: 2.0, dimension: DimensionVector::DIMENSIONLESS };
        assert!(a < b);
    }

    #[test]
    fn value_complex_partial_ord_consistent() {
        let a = Value::Complex { re: 1.0, im: 2.0, dimension: DimensionVector::DIMENSIONLESS };
        let b = Value::Complex { re: 1.0, im: 3.0, dimension: DimensionVector::DIMENSIONLESS };
        assert_eq!(a.partial_cmp(&b), Some(std::cmp::Ordering::Less));
        assert_eq!(b.partial_cmp(&a), Some(std::cmp::Ordering::Greater));
    }

    // ── Value::Complex content_hash tests (step-6) ────────────────────────────

    #[test]
    fn value_complex_hash_determinism() {
        let a = Value::Complex { re: 3.0, im: 4.0, dimension: DimensionVector::DIMENSIONLESS };
        let b = Value::Complex { re: 3.0, im: 4.0, dimension: DimensionVector::DIMENSIONLESS };
        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_complex_nan_re_canonicalized() {
        let a = Value::Complex { re: f64::NAN, im: 0.0, dimension: DimensionVector::DIMENSIONLESS };
        let b = Value::Complex { re: f64::NAN, im: 0.0, dimension: DimensionVector::DIMENSIONLESS };
        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_complex_hash_eq_implies_same_hash() {
        // Equal values produce equal hashes
        let a = Value::Complex { re: 1.0, im: 2.0, dimension: DimensionVector::LENGTH };
        let b = Value::Complex { re: 1.0, im: 2.0, dimension: DimensionVector::LENGTH };
        assert_eq!(a, b);
        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_complex_different_re_different_hash() {
        let a = Value::Complex { re: 1.0, im: 0.0, dimension: DimensionVector::DIMENSIONLESS };
        let b = Value::Complex { re: 2.0, im: 0.0, dimension: DimensionVector::DIMENSIONLESS };
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_complex_different_dimension_different_hash() {
        let a = Value::Complex { re: 3.0, im: 0.0, dimension: DimensionVector::DIMENSIONLESS };
        let b = Value::Complex { re: 3.0, im: 0.0, dimension: DimensionVector::LENGTH };
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_complex_hash_differs_from_scalar() {
        // Complex tag=15 vs Scalar tag=4 — hashes must differ even with same numeric value
        let c = Value::Complex { re: 3.0, im: 0.0, dimension: DimensionVector::DIMENSIONLESS };
        let s = Value::Scalar { si_value: 3.0, dimension: DimensionVector::DIMENSIONLESS };
        assert_ne!(c.content_hash(), s.content_hash());
    }

    // ── Value::Complex dimension() test (step-7) ──────────────────────────────

    #[test]
    fn value_complex_dimension_returns_stored() {
        let v = Value::Complex { re: 1.0, im: 2.0, dimension: DimensionVector::LENGTH };
        assert_eq!(v.dimension(), DimensionVector::LENGTH);
    }

    #[test]
    fn value_complex_dimensionless_returns_dimensionless() {
        let v = Value::Complex { re: 1.0, im: 2.0, dimension: DimensionVector::DIMENSIONLESS };
        assert_eq!(v.dimension(), DimensionVector::DIMENSIONLESS);
    }

    // ── Value::Orientation tests (step-3) ────────────────────────────────────

    #[test]
    fn value_orientation_construction() {
        let o = Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };
        // Should not be undef
        assert!(!o.is_undef());
    }

    #[test]
    fn value_orientation_eq_same() {
        let a = Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };
        let b = Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };
        assert_eq!(a, b);
    }

    #[test]
    fn value_orientation_eq_different() {
        let a = Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };
        let b = Value::Orientation { w: 0.0, x: 1.0, y: 0.0, z: 0.0 };
        assert_ne!(a, b);
    }

    #[test]
    fn value_orientation_eq_nan_bitwise() {
        // NaN == NaN via to_bits (bitwise equality)
        let a = Value::Orientation { w: f64::NAN, x: 0.0, y: 0.0, z: 0.0 };
        let b = Value::Orientation { w: f64::NAN, x: 0.0, y: 0.0, z: 0.0 };
        assert_eq!(a, b);
    }

    #[test]
    fn value_orientation_eq_neg_zero() {
        // -0.0 != 0.0 via to_bits
        let a = Value::Orientation { w: -0.0, x: 0.0, y: 0.0, z: 0.0 };
        let b = Value::Orientation { w: 0.0, x: 0.0, y: 0.0, z: 0.0 };
        assert_ne!(a, b);
    }

    #[test]
    fn value_orientation_ord_cross_type() {
        // Orientation should sort after Complex (tag 14), so Orientation tag = 15
        let complex = Value::Complex { re: 0.0, im: 0.0, dimension: DimensionVector::DIMENSIONLESS };
        let orient = Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };
        assert!(complex < orient);
    }

    #[test]
    fn value_orientation_ord_within_type() {
        // Lexicographic on w, x, y, z via to_bits
        let a = Value::Orientation { w: 0.0, x: 0.0, y: 0.0, z: 0.0 };
        let b = Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };
        assert!(a < b);

        // Same w, different x
        let c = Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };
        let d = Value::Orientation { w: 1.0, x: 1.0, y: 0.0, z: 0.0 };
        assert!(c < d);
    }

    #[test]
    fn value_orientation_display() {
        let o = Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };
        assert_eq!(format!("{}", o), "[1, 0, 0, 0]q");
    }

    #[test]
    fn value_orientation_display_fractional() {
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let o = Value::Orientation { w: s, x: 0.0, y: 0.0, z: s };
        let display = format!("{}", o);
        assert!(display.starts_with('['));
        assert!(display.ends_with("]q"));
    }

    #[test]
    fn value_orientation_content_hash_deterministic() {
        let a = Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };
        let b = Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };
        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_orientation_content_hash_nan_canonical() {
        let a = Value::Orientation { w: f64::NAN, x: 0.0, y: 0.0, z: 0.0 };
        let b = Value::Orientation { w: f64::NAN, x: 0.0, y: 0.0, z: 0.0 };
        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_orientation_content_hash_distinct_from_complex() {
        // Tag 16 for Orientation vs tag 15 for Complex
        let o = Value::Orientation { w: 0.0, x: 0.0, y: 0.0, z: 0.0 };
        let c = Value::Complex { re: 0.0, im: 0.0, dimension: DimensionVector::DIMENSIONLESS };
        assert_ne!(o.content_hash(), c.content_hash());
    }

    #[test]
    fn value_orientation_content_hash_neg_zero() {
        let a = Value::Orientation { w: -0.0, x: 0.0, y: 0.0, z: 0.0 };
        let b = Value::Orientation { w: 0.0, x: 0.0, y: 0.0, z: 0.0 };
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_orientation_as_f64_none() {
        let o = Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };
        assert_eq!(o.as_f64(), None);
    }

    #[test]
    fn value_orientation_dimension_dimensionless() {
        let o = Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };
        assert_eq!(o.dimension(), DimensionVector::DIMENSIONLESS);
    }

    // ── Range Display tests (step-9) ─────────────────────────────────────────

    #[test]
    fn value_range_display_closed_inclusive() {
        let r = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, true);
        assert_eq!(format!("{}", r), "[0..10]");
    }

    #[test]
    fn value_range_display_open_exclusive() {
        let r = make_range(Some(Value::Int(0)), Some(Value::Int(10)), false, false);
        assert_eq!(format!("{}", r), "(0..10)");
    }

    #[test]
    fn value_range_display_half_open_lower_inclusive() {
        let r = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        assert_eq!(format!("{}", r), "[0..10)");
    }

    #[test]
    fn value_range_display_half_open_upper_inclusive() {
        let r = make_range(Some(Value::Int(0)), Some(Value::Int(10)), false, true);
        assert_eq!(format!("{}", r), "(0..10]");
    }

    #[test]
    fn value_range_display_unbounded_lower() {
        let r = make_range(None, Some(Value::Int(10)), false, true);
        assert_eq!(format!("{}", r), "(-inf..10]");
    }

    #[test]
    fn value_range_display_unbounded_upper() {
        let r = make_range(Some(Value::Int(0)), None, true, false);
        assert_eq!(format!("{}", r), "[0..inf)");
    }

    #[test]
    fn value_range_display_fully_unbounded() {
        let r = make_range(None, None, false, false);
        assert_eq!(format!("{}", r), "(-inf..inf)");
    }

    #[test]
    fn value_range_display_real_bounds() {
        let r = make_range(Some(Value::Real(1.5)), Some(Value::Real(3.5)), true, false);
        assert_eq!(format!("{}", r), "[1.5..3.5)");
    }

    // ── Range content_hash tests (step-7) ───────────────────────────────────

    #[test]
    fn value_range_content_hash_deterministic() {
        let r1 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        let r2 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        assert_eq!(r1.content_hash(), r2.content_hash());
    }

    #[test]
    fn value_range_content_hash_different_bounds_differ() {
        let r1 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        let r2 = make_range(Some(Value::Int(1)), Some(Value::Int(10)), true, false);
        assert_ne!(r1.content_hash(), r2.content_hash());
    }

    #[test]
    fn value_range_content_hash_none_vs_some_differ() {
        let r_none = make_range(None, Some(Value::Int(10)), false, true);
        let r_some = make_range(Some(Value::Int(0)), Some(Value::Int(10)), false, true);
        assert_ne!(r_none.content_hash(), r_some.content_hash());
    }

    #[test]
    fn value_range_content_hash_inclusivity_differs() {
        let r_open = make_range(Some(Value::Int(0)), Some(Value::Int(10)), false, false);
        let r_half = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        let r_closed = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, true);
        assert_ne!(r_open.content_hash(), r_half.content_hash());
        assert_ne!(r_half.content_hash(), r_closed.content_hash());
        assert_ne!(r_open.content_hash(), r_closed.content_hash());
    }

    #[test]
    fn value_range_content_hash_no_collision_with_orientation() {
        // Range tag=17 should not collide with Orientation tag=16
        let range = make_range(None, None, false, false);
        let orient = Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };
        assert_ne!(range.content_hash(), orient.content_hash());
    }

    #[test]
    fn value_range_content_hash_both_none_deterministic() {
        let r1 = make_range(None, None, false, false);
        let r2 = make_range(None, None, false, false);
        assert_eq!(r1.content_hash(), r2.content_hash());
    }

    // ── Range Ord tests (step-5) ─────────────────────────────────────────────

    #[test]
    fn value_range_ord_cross_type_after_orientation() {
        // Range has type_tag=16, Orientation=15 → Range > Orientation
        let range = make_range(None, None, false, false);
        let orient = Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };
        assert!(range > orient);
        assert!(orient < range);
    }

    #[test]
    fn value_range_ord_cross_type_before_undef() {
        // Range has type_tag=16, Undef=0 → Range > Undef
        let range = make_range(None, None, false, false);
        assert!(range > Value::Undef);
    }

    #[test]
    fn value_range_ord_within_type_lower_inclusive_first() {
        // lower_inclusive=false < lower_inclusive=true (false=0 < true=1)
        let r_open = make_range(Some(Value::Int(0)), Some(Value::Int(10)), false, true);
        let r_closed = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, true);
        assert!(r_open < r_closed);
    }

    #[test]
    fn value_range_ord_within_type_lower_bound_none_before_some() {
        // None lower < Some lower (Option ordering: None < Some)
        let r_unbounded = make_range(None, Some(Value::Int(10)), false, true);
        let r_bounded = make_range(Some(Value::Int(0)), Some(Value::Int(10)), false, true);
        assert!(r_unbounded < r_bounded);
    }

    #[test]
    fn value_range_ord_within_type_upper_inclusive_after_lower() {
        // When lower_inclusive and lower are equal, compare upper_inclusive
        let r_open_upper = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        let r_closed_upper = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, true);
        assert!(r_open_upper < r_closed_upper);
    }

    #[test]
    fn value_range_ord_within_type_upper_bound_none_before_some() {
        // None upper < Some upper
        let r_unbounded = make_range(Some(Value::Int(0)), None, true, false);
        let r_bounded = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        assert!(r_unbounded < r_bounded);
    }

    #[test]
    fn value_range_ord_equal_ranges() {
        use std::cmp::Ordering;
        let r1 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        let r2 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        assert_eq!(r1.cmp(&r2), Ordering::Equal);
    }

    // ── Range PartialEq tests (step-3) ───────────────────────────────────────

    fn make_range(
        lower: Option<Value>,
        upper: Option<Value>,
        lower_inclusive: bool,
        upper_inclusive: bool,
    ) -> Value {
        Value::range(lower, upper, lower_inclusive, upper_inclusive)
    }

    #[test]
    fn value_range_equal_ranges_are_equal() {
        let r1 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        let r2 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        assert_eq!(r1, r2);
    }

    #[test]
    fn value_range_different_lower_not_equal() {
        let r1 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, true);
        let r2 = make_range(Some(Value::Int(1)), Some(Value::Int(10)), true, true);
        assert_ne!(r1, r2);
    }

    #[test]
    fn value_range_different_upper_not_equal() {
        let r1 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, true);
        let r2 = make_range(Some(Value::Int(0)), Some(Value::Int(20)), true, true);
        assert_ne!(r1, r2);
    }

    #[test]
    fn value_range_different_lower_inclusive_not_equal() {
        let r1 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, true);
        let r2 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), false, true);
        assert_ne!(r1, r2);
    }

    #[test]
    fn value_range_different_upper_inclusive_not_equal() {
        let r1 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, true);
        let r2 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        assert_ne!(r1, r2);
    }

    #[test]
    fn value_range_none_vs_some_lower_not_equal() {
        let r1 = make_range(None, Some(Value::Int(10)), false, true);
        let r2 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), false, true);
        assert_ne!(r1, r2);
    }

    #[test]
    fn value_range_none_vs_some_upper_not_equal() {
        let r1 = make_range(Some(Value::Int(0)), None, true, false);
        let r2 = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        assert_ne!(r1, r2);
    }

    #[test]
    fn value_range_both_none_equal() {
        let r1 = make_range(None, None, false, false);
        let r2 = make_range(None, None, false, false);
        assert_eq!(r1, r2);
    }

    #[test]
    fn value_range_not_equal_to_other_variants() {
        let r = make_range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        assert_ne!(r, Value::Int(0));
        assert_ne!(r, Value::Undef);
        assert_ne!(r, Value::Bool(true));
    }

    // ── Range inclusivity normalization tests (task-364) ─────────────────────

    #[test]
    fn value_range_normalize_lower_inclusive_when_none() {
        let r = Value::range(None, Some(Value::Int(10)), true, true);
        match r {
            Value::Range { lower_inclusive, .. } => assert!(!lower_inclusive),
            _ => panic!("expected Range"),
        }
    }

    #[test]
    fn value_range_normalize_upper_inclusive_when_none() {
        let r = Value::range(Some(Value::Int(0)), None, true, true);
        match r {
            Value::Range { upper_inclusive, .. } => assert!(!upper_inclusive),
            _ => panic!("expected Range"),
        }
    }

    #[test]
    fn value_range_normalize_both_when_none() {
        let r = Value::range(None, None, true, true);
        match r {
            Value::Range { lower_inclusive, upper_inclusive, .. } => {
                assert!(!lower_inclusive);
                assert!(!upper_inclusive);
            }
            _ => panic!("expected Range"),
        }
    }

    #[test]
    fn value_range_no_normalize_when_some() {
        let r = Value::range(Some(Value::Int(0)), Some(Value::Int(10)), true, true);
        match r {
            Value::Range { lower_inclusive, upper_inclusive, .. } => {
                assert!(lower_inclusive);
                assert!(upper_inclusive);
            }
            _ => panic!("expected Range"),
        }
    }

    // ── Range equality/hash equivalence with differing flags (task-364 step-3) ─

    #[test]
    fn value_range_eq_none_lower_ignores_inclusive() {
        let r1 = Value::range(None, Some(Value::Int(10)), true, true);
        let r2 = Value::range(None, Some(Value::Int(10)), false, true);
        assert_eq!(r1, r2);
    }

    #[test]
    fn value_range_eq_none_upper_ignores_inclusive() {
        let r1 = Value::range(Some(Value::Int(0)), None, true, true);
        let r2 = Value::range(Some(Value::Int(0)), None, true, false);
        assert_eq!(r1, r2);
    }

    #[test]
    fn value_range_eq_both_none_ignores_inclusive() {
        let r1 = Value::range(None, None, true, true);
        let r2 = Value::range(None, None, false, false);
        assert_eq!(r1, r2);
    }

    #[test]
    fn value_range_hash_none_lower_ignores_inclusive() {
        let r1 = Value::range(None, Some(Value::Int(10)), true, true);
        let r2 = Value::range(None, Some(Value::Int(10)), false, true);
        assert_eq!(r1.content_hash(), r2.content_hash());
    }

    #[test]
    fn value_range_hash_none_upper_ignores_inclusive() {
        let r1 = Value::range(Some(Value::Int(0)), None, true, true);
        let r2 = Value::range(Some(Value::Int(0)), None, true, false);
        assert_eq!(r1.content_hash(), r2.content_hash());
    }

    // ── Range Display with inclusive+None edge cases (task-364 step-4) ─────────

    #[test]
    fn value_range_display_none_lower_with_inclusive_true() {
        let r = Value::range(None, Some(Value::Int(10)), true, true);
        assert_eq!(format!("{}", r), "(-inf..10]");
    }

    #[test]
    fn value_range_display_none_upper_with_inclusive_true() {
        let r = Value::range(Some(Value::Int(0)), None, true, true);
        assert_eq!(format!("{}", r), "[0..inf)");
    }

    #[test]
    fn value_range_display_both_none_with_inclusive_true() {
        let r = Value::range(None, None, true, true);
        assert_eq!(format!("{}", r), "(-inf..inf)");
    }

    // ── Range invariant enforcement tests (step-9) ───────────────────────────
    // These tests bypass Value::range() factory and directly construct Value::Range
    // with an invariant violation (lower/upper_inclusive=true when bound is None).
    // Each impl (content_hash, PartialEq, Ord, Display) must panic via debug_assert!.

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "Range invariant violated")]
    fn value_range_invariant_bypass_lower_none_inclusive_content_hash() {
        let r = Value::Range {
            lower: None,
            lower_inclusive: true,
            upper: Some(Box::new(Value::Int(10))),
            upper_inclusive: false,
        };
        let _ = r.content_hash();
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "Range invariant violated")]
    fn value_range_invariant_bypass_lower_none_inclusive_eq() {
        let bad = Value::Range {
            lower: None,
            lower_inclusive: true,
            upper: Some(Box::new(Value::Int(10))),
            upper_inclusive: false,
        };
        let good = Value::range(None, Some(Value::Int(10)), false, false);
        let _ = bad == good;
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "Range invariant violated")]
    fn value_range_invariant_bypass_lower_none_inclusive_cmp() {
        let bad = Value::Range {
            lower: None,
            lower_inclusive: true,
            upper: Some(Box::new(Value::Int(10))),
            upper_inclusive: false,
        };
        let good = Value::range(None, Some(Value::Int(10)), false, false);
        let _ = bad.cmp(&good);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "Range invariant violated")]
    fn value_range_invariant_bypass_lower_none_inclusive_display() {
        let r = Value::Range {
            lower: None,
            lower_inclusive: true,
            upper: Some(Box::new(Value::Int(10))),
            upper_inclusive: false,
        };
        let _ = format!("{}", r);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "Range invariant violated")]
    fn value_range_invariant_bypass_upper_none_inclusive_content_hash() {
        let r = Value::Range {
            lower: Some(Box::new(Value::Int(0))),
            lower_inclusive: true,
            upper: None,
            upper_inclusive: true,
        };
        let _ = r.content_hash();
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "Range invariant violated")]
    fn value_range_invariant_bypass_upper_none_inclusive_display() {
        let r = Value::Range {
            lower: Some(Box::new(Value::Int(0))),
            lower_inclusive: true,
            upper: None,
            upper_inclusive: true,
        };
        let _ = format!("{}", r);
    }
}
