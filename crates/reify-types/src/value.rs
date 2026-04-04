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
    /// A field produced by `gradient()` — its lambda slot stores the original
    /// field and the sample handler dispatches to central-difference evaluation.
    Gradient,
}

/// Runtime values in Reify (M1 subset).
#[derive(Debug, Clone)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Real(f64),
    String(String),
    /// Dimensioned scalar: value in SI base units, with dimension.
    Scalar {
        si_value: f64,
        dimension: DimensionVector,
    },
    /// Enum variant value: type_name::variant.
    Enum {
        type_name: String,
        variant: String,
    },
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
    Complex {
        re: f64,
        im: f64,
        dimension: DimensionVector,
    },
    /// Orientation as a unit quaternion (w + xi + yj + zk).
    Orientation {
        w: f64,
        x: f64,
        y: f64,
        z: f64,
    },
    /// Coordinate frame: an origin point and a basis orientation.
    Frame {
        origin: Box<Value>,
        basis: Box<Value>,
    },
    /// Rigid-body transformation: a rotation (Orientation) and a translation (Vector).
    Transform {
        rotation: Box<Value>,
        translation: Box<Value>,
    },
    /// 3D plane: an origin Point3 and a unit normal Vector3 (dimensionless).
    Plane {
        origin: Box<Value>,
        normal: Box<Value>,
    },
    /// 3D axis (ray): an origin Point3 and a unit direction Vector3 (dimensionless).
    Axis {
        origin: Box<Value>,
        direction: Box<Value>,
    },
    /// 3D axis-aligned bounding box: min and max corner Point3 values.
    BoundingBox {
        min: Box<Value>,
        max: Box<Value>,
    },
    /// Range with optional inclusive/exclusive bounds.
    Range {
        lower: Option<Box<Value>>,
        upper: Option<Box<Value>>,
        lower_inclusive: bool,
        upper_inclusive: bool,
    },
    /// User-facing matrix literal (m rows × n cols).
    ///
    /// Before arithmetic evaluation, canonicalized to nested [`Value::Tensor`] (rank-2
    /// Tensor where each element is a Tensor row) via [`Value::canonicalize_matrix()`].
    /// The evaluator in `reify-expr` operates exclusively on the nested-Tensor
    /// representation for matrix arithmetic.
    Matrix(Vec<Vec<Value>>),
    /// Undefined — not yet determined or computation failed.
    Undef,
}

/// Normalize range inclusivity flags: force `inclusive=false` when the
/// corresponding bound is `None` (unbounded endpoint cannot be inclusive).
fn normalize_range_flags<T>(
    lower: &Option<T>,
    upper: &Option<T>,
    lower_inclusive: bool,
    upper_inclusive: bool,
) -> (bool, bool) {
    (
        lower_inclusive && lower.is_some(),
        upper_inclusive && upper.is_some(),
    )
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

    /// Create a `Real` or `Scalar` from a raw f64 component and a dimension.
    ///
    /// Returns `Real(value)` when the dimension is dimensionless, or
    /// `Scalar { si_value: value, dimension }` otherwise.  This is the
    /// shared pattern used by complex component extraction (re, im) and
    /// magnitude computation.
    pub fn from_component(value: f64, dimension: DimensionVector) -> Self {
        if dimension.is_dimensionless() {
            Value::Real(value)
        } else {
            Value::Scalar {
                si_value: value,
                dimension,
            }
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
        let (lower_inclusive, upper_inclusive) =
            normalize_range_flags(&lower, &upper, lower_inclusive, upper_inclusive);
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

    /// Convert a `Value::Matrix` to nested `Value::Tensor` (rank-2 Tensor where each
    /// element is a Tensor row).  All other variants are returned unchanged.
    ///
    /// This is used by the evaluator in `reify-expr` to canonicalize matrix literals
    /// before dispatching to the arithmetic engine, which operates exclusively on the
    /// nested-Tensor representation.
    pub fn canonicalize_matrix(self) -> Self {
        match self {
            Value::Matrix(rows) => Value::Tensor(rows.into_iter().map(Value::Tensor).collect()),
            other => other,
        }
    }

    /// Convert a rank-2 nested `Value::Tensor` back to a `Value::Matrix`.
    ///
    /// Returns `Some(Matrix(...))` if `self` is a `Tensor` with at least one element
    /// and every element is itself a `Tensor`.  Returns `None` otherwise.
    pub fn try_into_matrix(self) -> Option<Self> {
        match self {
            Value::Tensor(rows)
                if !rows.is_empty() && rows.iter().all(|r| matches!(r, Value::Tensor(_))) =>
            {
                let matrix_rows: Vec<Vec<Value>> = rows
                    .into_iter()
                    .map(|r| match r {
                        Value::Tensor(elems) => elems,
                        _ => unreachable!("checked above"),
                    })
                    .collect();
                Some(Value::Matrix(matrix_rows))
            }
            _ => None,
        }
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

    /// Negate each component, returning Undef if any component negation fails.
    fn neg_components(components: Vec<Value>, wrap: fn(Vec<Value>) -> Value) -> Value {
        let results: Vec<Value> = components.into_iter().map(|c| -c).collect();
        if results.iter().any(|v| v.is_undef()) {
            Value::Undef
        } else {
            wrap(results)
        }
    }

    /// Get the dimension of this value (DIMENSIONLESS for non-scalar types).
    pub fn dimension(&self) -> DimensionVector {
        match self {
            Value::Scalar { dimension, .. } => *dimension,
            Value::Complex { dimension, .. } => *dimension,
            // Point/Vector: dimension derives from the first component (all components share one dimension).
            Value::Point(items) | Value::Vector(items) => items
                .first()
                .map(|v| v.dimension())
                .unwrap_or(DimensionVector::DIMENSIONLESS),
            Value::Frame { .. } => DimensionVector::DIMENSIONLESS,
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
            Value::Scalar {
                si_value,
                dimension,
            } => {
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
            Value::Enum { type_name, variant } => ContentHash::of(&[6])
                .combine(ContentHash::of_str(type_name))
                .combine(ContentHash::of_str(variant)),
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
            Value::Field {
                domain_type,
                codomain_type,
                source,
                lambda,
            } => {
                let mut h = ContentHash::of(&[13]);
                h = h.combine(ContentHash::of_str(&format!("{}", domain_type)));
                h = h.combine(ContentHash::of_str(&format!("{}", codomain_type)));
                h = h.combine(ContentHash::of_str(&format!("{:?}", source)));
                h = h.combine(lambda.content_hash());
                h
            }
            Value::Lambda {
                params,
                body,
                captures,
            } => {
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
                let re_bits = if re.is_nan() {
                    f64::NAN.to_bits()
                } else {
                    re.to_bits()
                };
                let im_bits = if im.is_nan() {
                    f64::NAN.to_bits()
                } else {
                    im.to_bits()
                };
                let mut buf = [0u8; 17];
                buf[0] = 15;
                buf[1..9].copy_from_slice(&re_bits.to_le_bytes());
                buf[9..17].copy_from_slice(&im_bits.to_le_bytes());
                ContentHash::of(&buf).combine(dimension.content_hash())
            }
            Value::Orientation { w, x, y, z } => {
                // tag=16; NaN canonicalization for all 4 components
                let canon = |v: &f64| -> u64 {
                    if v.is_nan() {
                        f64::NAN.to_bits()
                    } else {
                        v.to_bits()
                    }
                };
                let mut buf = [0u8; 33];
                buf[0] = 16;
                buf[1..9].copy_from_slice(&canon(w).to_le_bytes());
                buf[9..17].copy_from_slice(&canon(x).to_le_bytes());
                buf[17..25].copy_from_slice(&canon(y).to_le_bytes());
                buf[25..33].copy_from_slice(&canon(z).to_le_bytes());
                ContentHash::of(&buf)
            }
            Value::Frame { origin, basis } => {
                // tag=20; combine origin and basis content hashes
                ContentHash::of(&[20])
                    .combine(origin.content_hash())
                    .combine(basis.content_hash())
            }
            Value::Transform {
                rotation,
                translation,
            } => {
                // tag=21; combine rotation and translation content hashes
                ContentHash::of(&[21])
                    .combine(rotation.content_hash())
                    .combine(translation.content_hash())
            }
            Value::Plane { origin, normal } => {
                // tag=22; combine origin and normal content hashes
                ContentHash::of(&[22])
                    .combine(origin.content_hash())
                    .combine(normal.content_hash())
            }
            Value::Axis { origin, direction } => {
                // tag=23; combine origin and direction content hashes
                ContentHash::of(&[23])
                    .combine(origin.content_hash())
                    .combine(direction.content_hash())
            }
            Value::BoundingBox { min, max } => {
                // tag=24; combine min and max content hashes
                ContentHash::of(&[24])
                    .combine(min.content_hash())
                    .combine(max.content_hash())
            }
            Value::Range {
                lower,
                upper,
                lower_inclusive,
                upper_inclusive,
            } => {
                // Defensive re-normalization: None bounds → inclusive=false
                let (lower_inclusive, upper_inclusive) =
                    normalize_range_flags(lower, upper, *lower_inclusive, *upper_inclusive);
                // tag=17; flags then optional bounds
                let mut h = ContentHash::of(&[17, lower_inclusive as u8, upper_inclusive as u8]);
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
            Value::Matrix(rows) => {
                // tag=18; hash row count, then per-row col count + element hashes
                let mut h = ContentHash::of(&[18]);
                h = h.combine(ContentHash::of(&(rows.len() as u64).to_le_bytes()));
                for row in rows {
                    h = h.combine(ContentHash::of(&(row.len() as u64).to_le_bytes()));
                    for elem in row {
                        h = h.combine(elem.content_hash());
                    }
                }
                h
            }
            Value::Undef => ContentHash::of(&[5]),
        }
    }

    // --- Type-spine consolidated methods ---
    //
    // These methods centralise logic that previously lived as match-on-Value
    // blocks in downstream crates (builders, classifier, LSP analysis).
    // Adding a new Value variant now only requires updating value.rs (and
    // ty.rs for the corresponding Type variant), instead of editing 4+
    // files across 4 crates.

    /// Infer the [`Type`] of a runtime [`Value`].
    ///
    /// Used by test builders to derive a type from a literal value.
    /// For variants whose type cannot be fully inferred (Tensor, Matrix),
    /// this method panics — use `CompiledExpr::literal(value, type)` directly.
    pub fn infer_type(&self) -> crate::ty::Type {
        use crate::ty::Type;
        match self {
            Value::Bool(_) => Type::Bool,
            Value::Int(_) => Type::Int,
            Value::Real(_) => Type::Real,
            Value::String(_) => Type::String,
            Value::Scalar { dimension, .. } => Type::Scalar {
                dimension: *dimension,
            },
            Value::Enum { type_name, .. } => Type::Enum(type_name.clone()),
            Value::List(items) => {
                let elem_ty = items.first().map(Value::infer_type).unwrap_or(Type::Int);
                Type::List(Box::new(elem_ty))
            }
            Value::Set(items) => {
                let elem_ty = items
                    .iter()
                    .next()
                    .map(Value::infer_type)
                    .unwrap_or(Type::Int);
                Type::Set(Box::new(elem_ty))
            }
            Value::Map(m) => {
                let (k_ty, v_ty) = m
                    .iter()
                    .next()
                    .map(|(k, v)| (k.infer_type(), v.infer_type()))
                    .unwrap_or((Type::String, Type::Int));
                Type::Map(Box::new(k_ty), Box::new(v_ty))
            }
            Value::Option(Some(inner)) => Type::Option(Box::new(inner.infer_type())),
            Value::Option(None) => Type::Option(Box::new(Type::Bool)),
            Value::Lambda { params, body, .. } => {
                let param_types = params.iter().map(|_| Type::Real).collect();
                Type::Function {
                    params: param_types,
                    return_type: Box::new(body.result_type.clone()),
                }
            }
            Value::Field {
                domain_type,
                codomain_type,
                ..
            } => Type::Field {
                domain: Box::new(domain_type.clone()),
                codomain: Box::new(codomain_type.clone()),
            },
            Value::Tensor(_) => {
                panic!(
                    "infer_type() cannot infer Tensor type (rank/n/quantity). \
                     Use CompiledExpr::literal(value, type) directly."
                )
            }
            Value::Complex { dimension, .. } => Type::complex(Type::Scalar {
                dimension: *dimension,
            }),
            Value::Matrix(_) => {
                panic!(
                    "infer_type() cannot infer Matrix type. \
                     Use CompiledExpr::literal(value, type) directly."
                )
            }
            Value::Point(components) => {
                let q = components
                    .first()
                    .map(Value::infer_type)
                    .unwrap_or(Type::Real);
                Type::Point {
                    n: components.len(),
                    quantity: Box::new(q),
                }
            }
            Value::Vector(components) => {
                let q = components
                    .first()
                    .map(Value::infer_type)
                    .unwrap_or(Type::Real);
                Type::Vector {
                    n: components.len(),
                    quantity: Box::new(q),
                }
            }
            Value::Orientation { .. } => Type::Orientation(3),
            Value::Frame { .. } => {
                panic!(
                    "infer_type() cannot infer Frame dimensionality. \
                     Use CompiledExpr::literal(value, type) directly."
                )
            }
            Value::Transform { .. } => {
                panic!(
                    "infer_type() cannot infer Transform dimensionality. \
                     Use CompiledExpr::literal(value, type) directly."
                )
            }
            Value::Plane { .. } => Type::Plane,
            Value::Axis { .. } => Type::Axis,
            Value::BoundingBox { .. } => Type::BoundingBox,
            Value::Range { lower, upper, .. } => {
                let elem_ty = lower
                    .as_ref()
                    .or(upper.as_ref())
                    .map(|v| v.infer_type())
                    .unwrap_or(Type::Real);
                Type::Range(Box::new(elem_ty))
            }
            Value::Undef => Type::Bool,
        }
    }

    /// Returns `true` if this value is a numeric leaf for constraint
    /// domain classification (Int, Real, or Scalar).
    pub fn is_domain_numeric_leaf(&self) -> bool {
        matches!(self, Value::Int(_) | Value::Real(_) | Value::Scalar { .. })
    }

    /// Returns `true` if this value is a logical leaf for constraint
    /// domain classification (Bool).
    pub fn is_domain_logical_leaf(&self) -> bool {
        matches!(self, Value::Bool(_))
    }

    /// Format this value for user-friendly display (e.g., hover tooltips).
    ///
    /// Unlike the [`Display`](std::fmt::Display) impl which shows raw
    /// dimension vectors, this method uses human-readable SI unit labels.
    pub fn format_hover(&self) -> String {
        match self {
            Value::Bool(b) => format!("{b}"),
            Value::Int(i) => format!("{i}"),
            Value::Real(r) => format!("{r}"),
            Value::String(s) => format!("\"{s}\""),
            Value::Scalar {
                si_value,
                dimension,
            } => {
                let unit = dimension_unit_label(dimension);
                if unit.is_empty() {
                    format!("{si_value}")
                } else {
                    format!("{si_value} {unit}")
                }
            }
            Value::Enum { type_name, variant } => format!("{type_name}::{variant}"),
            Value::List(items) => {
                let inner: Vec<String> = items.iter().map(Value::format_hover).collect();
                format!("[{}]", inner.join(", "))
            }
            Value::Set(items) => {
                let inner: Vec<String> = items.iter().map(Value::format_hover).collect();
                format!("{{{}}}", inner.join(", "))
            }
            Value::Map(entries) => {
                let inner: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k.format_hover(), v.format_hover()))
                    .collect();
                format!("{{{}}}", inner.join(", "))
            }
            Value::Option(inner) => match inner {
                None => "none".to_string(),
                Some(v) => format!("some({})", v.format_hover()),
            },
            Value::Tensor(items) => {
                let inner: Vec<String> = items.iter().map(Value::format_hover).collect();
                format!("[{}]", inner.join(", "))
            }
            Value::Lambda { .. } => "<lambda>".to_string(),
            Value::Field {
                domain_type,
                codomain_type,
                source,
                ..
            } => {
                format!("Field<{}, {}>({:?})", domain_type, codomain_type, source)
            }
            Value::Complex { re, im, dimension } => {
                let unit = dimension_unit_label(dimension);
                let (sign, im_abs) = if *im < 0.0 {
                    ("-", im.abs())
                } else {
                    ("+", *im)
                };
                if unit.is_empty() {
                    format!("{re} {sign} {im_abs}i")
                } else {
                    format!("{re} {sign} {im_abs}i {unit}")
                }
            }
            Value::Matrix(rows) => {
                let inner: Vec<String> = rows
                    .iter()
                    .map(|row| {
                        let cols: Vec<String> = row.iter().map(Value::format_hover).collect();
                        format!("[{}]", cols.join(", "))
                    })
                    .collect();
                format!("[{}]", inner.join(", "))
            }
            Value::Point(components) => {
                let inner: Vec<String> = components.iter().map(Value::format_hover).collect();
                format!("Point({})", inner.join(", "))
            }
            Value::Vector(components) => {
                let inner: Vec<String> = components.iter().map(Value::format_hover).collect();
                format!("Vector({})", inner.join(", "))
            }
            Value::Orientation { w, x, y, z } => {
                format!("Orientation(w={w}, x={x}, y={y}, z={z})")
            }
            Value::Frame { origin, basis } => {
                format!(
                    "Frame(origin={}, basis={})",
                    origin.format_hover(),
                    basis.format_hover()
                )
            }
            Value::Transform {
                rotation,
                translation,
            } => {
                format!(
                    "Transform(rotation={}, translation={})",
                    rotation.format_hover(),
                    translation.format_hover()
                )
            }
            Value::Plane { origin, normal } => {
                format!(
                    "Plane(origin={}, normal={})",
                    origin.format_hover(),
                    normal.format_hover()
                )
            }
            Value::Axis { origin, direction } => {
                format!(
                    "Axis(origin={}, direction={})",
                    origin.format_hover(),
                    direction.format_hover()
                )
            }
            Value::BoundingBox { min, max } => {
                format!(
                    "BoundingBox(min={}, max={})",
                    min.format_hover(),
                    max.format_hover()
                )
            }
            Value::Range { lower, upper, .. } => {
                let lo = lower
                    .as_ref()
                    .map(|v| v.format_hover())
                    .unwrap_or_else(|| "..".to_string());
                let hi = upper
                    .as_ref()
                    .map(|v| v.format_hover())
                    .unwrap_or_else(|| "..".to_string());
                format!("{lo}..{hi}")
            }
            Value::Undef => "(undefined)".to_string(),
        }
    }

    /// Format this value for GUI display, returning only the display string.
    ///
    /// For Scalar and Complex, the unit is discarded. Use [`format_display_pair`](Value::format_display_pair)
    /// directly when the unit must be preserved.
    ///
    /// This avoids the unnecessary `String::new()` allocations that `format_display_pair().0`
    /// would create on every recursive call inside composite types.
    pub fn format_display(&self) -> String {
        match self {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                let (display_value, _unit) = dimension.to_display_units(*si_value);
                format_display_number(display_value)
            }
            Value::Int(i) => i.to_string(),
            Value::Real(r) => format_display_number(*r),
            Value::Bool(b) => b.to_string(),
            Value::String(s) => s.clone(),
            Value::Enum { variant, .. } => variant.clone(),
            Value::List(items) => {
                let strs: Vec<String> = items.iter().map(|v| v.format_display()).collect();
                format!("[{}]", strs.join(", "))
            }
            Value::Set(items) => {
                let strs: Vec<String> = items.iter().map(|v| v.format_display()).collect();
                format!("set{{{}}}", strs.join(", "))
            }
            Value::Map(entries) => {
                let strs: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| format!("{} => {}", k.format_display(), v.format_display()))
                    .collect();
                format!("map{{{}}}", strs.join(", "))
            }
            Value::Option(opt) => match opt {
                Some(inner) => inner.format_display(),
                None => "none".to_string(),
            },
            Value::Lambda { .. } => "<lambda>".to_string(),
            Value::Field {
                domain_type,
                codomain_type,
                source,
                ..
            } => format!("Field<{}, {}>({:?})", domain_type, codomain_type, source),
            Value::Tensor(items) => {
                let strs: Vec<String> = items.iter().map(|v| v.format_display()).collect();
                format!("[{}]", strs.join(", "))
            }
            Value::Point(items) => {
                let strs: Vec<String> = items.iter().map(|v| v.format_display()).collect();
                format!("point({})", strs.join(", "))
            }
            Value::Vector(items) => {
                let strs: Vec<String> = items.iter().map(|v| v.format_display()).collect();
                format!("vec({})", strs.join(", "))
            }
            Value::Matrix(rows) => {
                let row_strs: Vec<String> = rows
                    .iter()
                    .map(|row| {
                        let inner: Vec<String> =
                            row.iter().map(|v| v.format_display()).collect();
                        format!("[{}]", inner.join(", "))
                    })
                    .collect();
                format!("[{}]", row_strs.join(", "))
            }
            Value::Complex { re, im, dimension } => {
                let (display_re, _) = dimension.to_display_units(*re);
                let (display_im, _) = dimension.to_display_units(*im);
                format!(
                    "{} + {}i",
                    format_display_number(display_re),
                    format_display_number(display_im)
                )
            }
            Value::Orientation { w, x, y, z } => {
                format!("[{}, {}, {}, {}]q", w, x, y, z)
            }
            Value::Frame { origin, basis } => {
                format!("frame({}, {})", origin.format_display(), basis.format_display())
            }
            Value::Transform {
                rotation,
                translation,
            } => format!(
                "transform({}, {})",
                rotation.format_display(),
                translation.format_display()
            ),
            Value::Plane { origin, normal } => {
                format!("plane({}, {})", origin.format_display(), normal.format_display())
            }
            Value::Axis { origin, direction } => {
                format!("axis({}, {})", origin.format_display(), direction.format_display())
            }
            Value::BoundingBox { min, max } => {
                format!("bbox({}, {})", min.format_display(), max.format_display())
            }
            Value::Range {
                lower,
                upper,
                lower_inclusive,
                upper_inclusive,
            } => {
                let (lower_inclusive, upper_inclusive) =
                    normalize_range_flags(lower, upper, *lower_inclusive, *upper_inclusive);
                let lower_bracket = if lower_inclusive { "[" } else { "(" };
                let upper_bracket = if upper_inclusive { "]" } else { ")" };
                let lower_str = lower
                    .as_ref()
                    .map(|v| v.format_display())
                    .unwrap_or_else(|| "-\u{221E}".to_string());
                let upper_str = upper
                    .as_ref()
                    .map(|v| v.format_display())
                    .unwrap_or_else(|| "+\u{221E}".to_string());
                format!(
                    "{}{}..{}{}",
                    lower_bracket, lower_str, upper_str, upper_bracket
                )
            }
            Value::Undef => "undefined".to_string(),
        }
    }

    /// Format this value for GUI display, returning `(formatted_value, unit_string)`.
    ///
    /// Unlike [`format_hover`](Value::format_hover) which shows raw SI values,
    /// this method converts to standard engineering display units (mm, deg, mm², mm³)
    /// via [`DimensionVector::to_display_units`].
    pub fn format_display_pair(&self) -> (String, String) {
        match self {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                let (display_value, unit) = dimension.to_display_units(*si_value);
                (format_display_number(display_value), unit.to_string())
            }
            Value::Complex { re, im, dimension } => {
                let (display_re, unit) = dimension.to_display_units(*re);
                let (display_im, _) = dimension.to_display_units(*im);
                let formatted = format!(
                    "{} + {}i",
                    format_display_number(display_re),
                    format_display_number(display_im)
                );
                (formatted, unit.to_string())
            }
            Value::Option(Some(inner)) => inner.format_display_pair(),
            _ => (self.format_display(), String::new()),
        }
    }
}

/// Format a floating-point number for display: whole numbers render without
/// decimal points (e.g. `80.0` → `"80"`).
pub fn format_display_number(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{}", v)
    }
}

/// Map a DimensionVector to a human-readable SI unit label.
///
/// Used by [`Value::format_hover`] for user-facing display.
fn dimension_unit_label(dim: &DimensionVector) -> &'static str {
    if *dim == DimensionVector::LENGTH {
        "m"
    } else if *dim == DimensionVector::AREA {
        "m\u{00B2}"
    } else if *dim == DimensionVector::VOLUME {
        "m\u{00B3}"
    } else if *dim == DimensionVector::MASS {
        "kg"
    } else if *dim == DimensionVector::ANGLE {
        "rad"
    } else if dim.is_dimensionless() {
        ""
    } else {
        "SI"
    }
}

/// Bit-identity equality for `Value`.
///
/// Float-bearing variants (`Real`, `Scalar`, `Complex`, `Orientation`) compare via
/// `to_bits()`, giving bit-pattern identity: `-0.0 != +0.0` and `NaN == NaN`
/// (for the same canonical NaN bit pattern). This is deliberate for
/// content-addressable storage — values with different bit representations must
/// hash and compare differently, so two `Value`s that differ only in float sign
/// or NaN payload are distinct keys.
///
/// **Eq/Ord contract:** this impl and `impl Ord for Value` both define equality
/// as bit-pattern identity, preserving the invariant: `a == b` iff
/// `a.cmp(&b) == Ordering::Equal`. Any change to either impl must preserve this
/// contract — update both impls together.
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Real(a), Value::Real(b)) => a.to_bits() == b.to_bits(),
            (Value::String(a), Value::String(b)) => a == b,
            (
                Value::Scalar {
                    si_value: a,
                    dimension: ad,
                },
                Value::Scalar {
                    si_value: b,
                    dimension: bd,
                },
            ) => a.to_bits() == b.to_bits() && ad == bd,
            (
                Value::Enum {
                    type_name: a,
                    variant: av,
                },
                Value::Enum {
                    type_name: b,
                    variant: bv,
                },
            ) => a == b && av == bv,
            (Value::List(a), Value::List(b)) => a == b,
            (Value::Tensor(a), Value::Tensor(b)) => a == b,
            (Value::Point(a), Value::Point(b)) => a == b,
            (Value::Vector(a), Value::Vector(b)) => a == b,
            (Value::Set(a), Value::Set(b)) => a == b,
            (Value::Map(a), Value::Map(b)) => a == b,
            (Value::Option(a), Value::Option(b)) => a == b,
            (
                Value::Field {
                    domain_type: ad,
                    codomain_type: ac,
                    source: as_,
                    lambda: al,
                },
                Value::Field {
                    domain_type: bd,
                    codomain_type: bc,
                    source: bs,
                    lambda: bl,
                },
            ) => ad == bd && ac == bc && as_ == bs && al == bl,
            (
                Value::Lambda {
                    params: ap,
                    body: ab,
                    captures: ac,
                },
                Value::Lambda {
                    params: bp,
                    body: bb,
                    captures: bc,
                },
            ) => {
                ap == bp && ab.content_hash == bb.content_hash && {
                    let a_caps = sorted_captures(ac);
                    let b_caps = sorted_captures(bc);
                    a_caps.len() == b_caps.len()
                        && a_caps
                            .iter()
                            .zip(b_caps.iter())
                            .all(|((aid, av), (bid, bv))| aid == bid && av == bv)
                }
            }
            (
                Value::Complex {
                    re: ar,
                    im: ai,
                    dimension: ad,
                },
                Value::Complex {
                    re: br,
                    im: bi,
                    dimension: bd,
                },
            ) => ar.to_bits() == br.to_bits() && ai.to_bits() == bi.to_bits() && ad == bd,
            (
                Value::Orientation {
                    w: aw,
                    x: ax,
                    y: ay,
                    z: az,
                },
                Value::Orientation {
                    w: bw,
                    x: bx,
                    y: by,
                    z: bz,
                },
            ) => {
                aw.to_bits() == bw.to_bits()
                    && ax.to_bits() == bx.to_bits()
                    && ay.to_bits() == by.to_bits()
                    && az.to_bits() == bz.to_bits()
            }
            (
                Value::Frame {
                    origin: ao,
                    basis: ab,
                },
                Value::Frame {
                    origin: bo,
                    basis: bb,
                },
            ) => ao == bo && ab == bb,
            (
                Value::Transform {
                    rotation: ar,
                    translation: at,
                },
                Value::Transform {
                    rotation: br,
                    translation: bt,
                },
            ) => ar == br && at == bt,
            (
                Value::Plane {
                    origin: ao,
                    normal: an,
                },
                Value::Plane {
                    origin: bo,
                    normal: bn,
                },
            ) => ao == bo && an == bn,
            (
                Value::Axis {
                    origin: ao,
                    direction: ad,
                },
                Value::Axis {
                    origin: bo,
                    direction: bd,
                },
            ) => ao == bo && ad == bd,
            (
                Value::BoundingBox {
                    min: amin,
                    max: amax,
                },
                Value::BoundingBox {
                    min: bmin,
                    max: bmax,
                },
            ) => amin == bmin && amax == bmax,
            (
                Value::Range {
                    lower: al,
                    upper: au,
                    lower_inclusive: ali,
                    upper_inclusive: aui,
                },
                Value::Range {
                    lower: bl,
                    upper: bu,
                    lower_inclusive: bli,
                    upper_inclusive: bui,
                },
            ) => {
                // Defensive re-normalization: None bounds → inclusive=false
                let (ali, aui) = normalize_range_flags(al, au, *ali, *aui);
                let (bli, bui) = normalize_range_flags(bl, bu, *bli, *bui);
                al == bl && au == bu && ali == bli && aui == bui
            }
            (Value::Matrix(a), Value::Matrix(b)) => a == b,
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

/// Total order for `Value`, consistent with `impl PartialEq for Value`.
///
/// Float-bearing variants use `to_bits()` unsigned comparison, giving a
/// deterministic total order that agrees with bit-identity equality:
/// `-0.0` and `+0.0` sort differently (negative zero has the sign bit set,
/// so it compares greater than positive zero under unsigned `u64` comparison),
/// and `NaN` occupies a fixed position in the order.
///
/// **Eq/Ord contract:** Both `PartialEq` and `Ord` define equality as
/// bit-pattern identity, so the contract `a == b` iff `a.cmp(&b) == Ordering::Equal`
/// is preserved.
///
/// **WARNING:** Any change to the comparison strategy (e.g. migrating to
/// `total_cmp()`) must preserve this invariant and must update **both** impls
/// together — if equality semantics change in one, they must change in the other.
impl Ord for Value {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;

        // Type-tag discriminant for cross-type ordering:
        // Undef=0, Bool=1, Int=2, Real=3, Scalar=4, String=5, Enum=6, List=7, Set=8, Map=9, Option=10, Field=11, Lambda=12, Tensor=13, Complex=14, Orientation=15, Range=16, Point=17, Vector=18, Matrix=19, Frame=20, Transform=21, Plane=22, Axis=23, BoundingBox=24
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
                Value::Matrix(_) => 19,
                Value::Frame { .. } => 20,
                Value::Transform { .. } => 21,
                Value::Plane { .. } => 22,
                Value::Axis { .. } => 23,
                Value::BoundingBox { .. } => 24,
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
            (
                Value::Scalar {
                    si_value: a,
                    dimension: ad,
                },
                Value::Scalar {
                    si_value: b,
                    dimension: bd,
                },
            ) => {
                // Compare by dimension first, then by value bits
                ad.cmp(bd).then_with(|| a.to_bits().cmp(&b.to_bits()))
            }
            (Value::String(a), Value::String(b)) => a.cmp(b),
            (
                Value::Enum {
                    type_name: a,
                    variant: av,
                },
                Value::Enum {
                    type_name: b,
                    variant: bv,
                },
            ) => a.cmp(b).then_with(|| av.cmp(bv)),
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
                Value::Field {
                    domain_type: ad,
                    codomain_type: ac,
                    source: as_,
                    lambda: al,
                },
                Value::Field {
                    domain_type: bd,
                    codomain_type: bc,
                    source: bs,
                    lambda: bl,
                },
            ) => format!("{}", ad)
                .cmp(&format!("{}", bd))
                .then_with(|| format!("{}", ac).cmp(&format!("{}", bc)))
                .then_with(|| format!("{:?}", as_).cmp(&format!("{:?}", bs)))
                .then_with(|| al.cmp(bl)),
            (
                Value::Lambda {
                    params: ap,
                    body: ab,
                    captures: ac,
                },
                Value::Lambda {
                    params: bp,
                    body: bb,
                    captures: bc,
                },
            ) => ap
                .cmp(bp)
                .then_with(|| ab.content_hash.0.cmp(&bb.content_hash.0))
                .then_with(|| sorted_captures(ac).cmp(&sorted_captures(bc))),
            (
                Value::Complex {
                    re: ar,
                    im: ai,
                    dimension: ad,
                },
                Value::Complex {
                    re: br,
                    im: bi,
                    dimension: bd,
                },
            ) => ad
                .cmp(bd)
                .then_with(|| ar.to_bits().cmp(&br.to_bits()))
                .then_with(|| ai.to_bits().cmp(&bi.to_bits())),
            (
                Value::Orientation {
                    w: aw,
                    x: ax,
                    y: ay,
                    z: az,
                },
                Value::Orientation {
                    w: bw,
                    x: bx,
                    y: by,
                    z: bz,
                },
            ) => aw
                .to_bits()
                .cmp(&bw.to_bits())
                .then_with(|| ax.to_bits().cmp(&bx.to_bits()))
                .then_with(|| ay.to_bits().cmp(&by.to_bits()))
                .then_with(|| az.to_bits().cmp(&bz.to_bits())),
            (
                Value::Range {
                    lower: al,
                    upper: au,
                    lower_inclusive: ali,
                    upper_inclusive: aui,
                },
                Value::Range {
                    lower: bl,
                    upper: bu,
                    lower_inclusive: bli,
                    upper_inclusive: bui,
                },
            ) => {
                // Defensive re-normalization: None bounds → inclusive=false
                let (ali, aui) = normalize_range_flags(al, au, *ali, *aui);
                let (bli, bui) = normalize_range_flags(bl, bu, *bli, *bui);
                ali.cmp(&bli)
                    .then_with(|| al.cmp(bl))
                    .then_with(|| aui.cmp(&bui))
                    .then_with(|| au.cmp(bu))
            }
            (Value::Matrix(a), Value::Matrix(b)) => a.cmp(b),
            (
                Value::Frame {
                    origin: ao,
                    basis: ab,
                },
                Value::Frame {
                    origin: bo,
                    basis: bb,
                },
            ) => ao.cmp(bo).then_with(|| ab.cmp(bb)),
            (
                Value::Transform {
                    rotation: ar,
                    translation: at,
                },
                Value::Transform {
                    rotation: br,
                    translation: bt,
                },
            ) => ar.cmp(br).then_with(|| at.cmp(bt)),
            (
                Value::Plane {
                    origin: ao,
                    normal: an,
                },
                Value::Plane {
                    origin: bo,
                    normal: bn,
                },
            ) => ao.cmp(bo).then_with(|| an.cmp(bn)),
            (
                Value::Axis {
                    origin: ao,
                    direction: ad,
                },
                Value::Axis {
                    origin: bo,
                    direction: bd,
                },
            ) => ao.cmp(bo).then_with(|| ad.cmp(bd)),
            (
                Value::BoundingBox {
                    min: amin,
                    max: amax,
                },
                Value::BoundingBox {
                    min: bmin,
                    max: bmax,
                },
            ) => amin.cmp(bmin).then_with(|| amax.cmp(bmax)),
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
            Value::Scalar {
                si_value,
                dimension,
            } => {
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
            Value::Field {
                domain_type,
                codomain_type,
                source,
                ..
            } => {
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
                write!(
                    f,
                    "[{}, {}, {}, {}]q",
                    fmt_f64(*w),
                    fmt_f64(*x),
                    fmt_f64(*y),
                    fmt_f64(*z)
                )
            }
            Value::Frame { origin, basis } => {
                write!(f, "frame({}, {})", origin, basis)
            }
            Value::Transform {
                rotation,
                translation,
            } => {
                write!(f, "transform({}, {})", rotation, translation)
            }
            Value::Plane { origin, normal } => {
                write!(f, "plane({}, {})", origin, normal)
            }
            Value::Axis { origin, direction } => {
                write!(f, "axis({}, {})", origin, direction)
            }
            Value::BoundingBox { min, max } => {
                write!(f, "bbox({}, {})", min, max)
            }
            Value::Range {
                lower,
                upper,
                lower_inclusive,
                upper_inclusive,
            } => {
                // Defensive re-normalization: if someone bypassed Value::range(),
                // ensure None bounds never appear as inclusive.
                let (lower_inclusive, upper_inclusive) =
                    normalize_range_flags(lower, upper, *lower_inclusive, *upper_inclusive);
                let lb = if lower_inclusive { '[' } else { '(' };
                let ub = if upper_inclusive { ']' } else { ')' };
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
            Value::Matrix(rows) => {
                write!(f, "[")?;
                for (i, row) in rows.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "[")?;
                    for (j, elem) in row.iter().enumerate() {
                        if j > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", elem)?;
                    }
                    write!(f, "]")?;
                }
                write!(f, "]")
            }
            Value::Undef => write!(f, "undef"),
        }
    }
}

impl std::ops::Neg for Value {
    type Output = Value;

    /// Negate this value. Returns `Value::Undef` for unsupported types or on
    /// overflow (e.g. `Int(i64::MIN)`).
    fn neg(self) -> Value {
        match self {
            Value::Int(i) => i.checked_neg().map(Value::Int).unwrap_or(Value::Undef),
            Value::Real(r) => Value::Real(-r),
            Value::Scalar {
                si_value,
                dimension,
            } => Value::Scalar {
                si_value: -si_value,
                dimension,
            },
            Value::Complex { re, im, dimension } => Value::Complex {
                re: -re,
                im: -im,
                dimension,
            },
            Value::Tensor(components) => Self::neg_components(components, Value::Tensor),
            Value::Vector(components) => Self::neg_components(components, Value::Vector),
            // Affine geometry: point negation is undefined (spec 3.3.1)
            Value::Point(_) => Value::Undef,
            _ => Value::Undef,
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
    Pending {
        last_substantive: Option<ContentHash>,
    },
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

    // ── normalize_range_flags unit tests ─────────────────────────────────────

    #[test]
    fn test_normalize_range_flags() {
        // Both bounds present → flags pass through unchanged
        assert_eq!(
            normalize_range_flags(&Some(1), &Some(2), true, true),
            (true, true)
        );

        // Lower is None → lower_inclusive forced false
        assert_eq!(
            normalize_range_flags::<i32>(&None, &Some(2), true, true),
            (false, true)
        );

        // Upper is None → upper_inclusive forced false
        assert_eq!(
            normalize_range_flags(&Some(1), &None::<i32>, true, true),
            (true, false)
        );

        // Both None → both forced false
        assert_eq!(
            normalize_range_flags::<i32>(&None, &None, true, true),
            (false, false)
        );

        // Both present but flags already false → stays false
        assert_eq!(
            normalize_range_flags(&Some(1), &Some(2), false, false),
            (false, false)
        );
    }

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
        let pos = Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        };
        let neg = Value::Scalar {
            si_value: -0.0,
            dimension: DimensionVector::LENGTH,
        };
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
        let f = Freshness::Pending {
            last_substantive: None,
        };
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
        let f = Freshness::Pending {
            last_substantive: Some(hash),
        };
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
        assert!(
            map.get_or_undef(&ValueCellId::new("Bracket", "missing"))
                .is_undef()
        );
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

        #[allow(clippy::clone_on_copy)]
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
    fn value_ord_real_nan_and_neg_zero_still_consistent() {
        use std::cmp::Ordering;

        let pos = Value::Real(0.0);
        let neg = Value::Real(-0.0);
        let nan = Value::Real(f64::NAN);
        let inf = Value::Real(f64::INFINITY);

        // (a) PartialEq: -0.0 != +0.0 (different bit patterns via to_bits())
        assert_ne!(neg, pos, "neg-zero and pos-zero must differ under bit-identity PartialEq");

        // (b) Ord: -0.0 has a strict ordering vs +0.0.
        // With to_bits() unsigned comparison:
        //   (-0.0f64).to_bits() == 0x8000000000000000 > 0x0000000000000000 == (0.0f64).to_bits()
        // so neg > pos.
        assert_ne!(neg.cmp(&pos), Ordering::Equal, "neg-zero and pos-zero must not compare Equal");
        assert_eq!(neg.cmp(&pos), Ordering::Greater, "neg-zero must sort after pos-zero under to_bits() unsigned order");
        assert_eq!(pos.cmp(&neg), Ordering::Less, "pos-zero must sort before neg-zero under to_bits() unsigned order");

        // (c) PartialEq: NaN == NaN (same canonical bit pattern)
        assert_eq!(nan, nan, "NaN must equal itself under bit-identity PartialEq");

        // (d) Ord: NaN has a defined non-Equal ordering vs finite and infinite values
        assert_ne!(nan.cmp(&inf), Ordering::Equal, "NaN must not compare Equal to infinity");
        assert_ne!(nan.cmp(&pos), Ordering::Equal, "NaN must not compare Equal to pos-zero");
        // NaN.to_bits() == 0x7FF8000000000000, which is less than INFINITY.to_bits() == 0x7FF0000000000000
        // Actually let's assert consistent direction using cmp symmetry:
        assert_eq!(nan.cmp(&inf), inf.cmp(&nan).reverse(), "NaN/inf ordering must be antisymmetric");

        // (e) PartialEq/Ord consistency invariant: a == b iff a.cmp(&b) == Equal
        // Check eq implies cmp == Equal:
        assert_eq!(nan.cmp(&nan), Ordering::Equal, "NaN == NaN so cmp must be Equal");
        assert_eq!(pos.cmp(&pos), Ordering::Equal, "pos == pos so cmp must be Equal");
        assert_eq!(neg.cmp(&neg), Ordering::Equal, "neg == neg so cmp must be Equal");
        // Check !eq implies cmp != Equal:
        assert_ne!(nan.cmp(&inf), Ordering::Equal, "nan != inf so cmp must not be Equal");
        assert_ne!(nan.cmp(&pos), Ordering::Equal, "nan != pos so cmp must not be Equal");
        assert_ne!(neg.cmp(&pos), Ordering::Equal, "neg != pos so cmp must not be Equal");
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
        assert_eq!(
            Value::Map(m1).content_hash(),
            Value::Map(m2.clone()).content_hash()
        );
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
        let enum_val = Value::Enum {
            type_name: "Z".into(),
            variant: "Z".into(),
        };
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
        let a = Value::Enum {
            type_name: "Color".into(),
            variant: "Red".into(),
        };
        let b = Value::Enum {
            type_name: "Color".into(),
            variant: "Red".into(),
        };
        let c = Value::Enum {
            type_name: "Color".into(),
            variant: "Blue".into(),
        };
        let d = Value::Enum {
            type_name: "Shape".into(),
            variant: "Red".into(),
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    #[test]
    fn value_enum_ordering() {
        let enum_val = Value::Enum {
            type_name: "Color".into(),
            variant: "Red".into(),
        };
        let string_val = Value::String("zzz".into());
        // Enum sorts after String
        assert!(enum_val > string_val);

        // Within Enum: sort by type_name then variant
        let a = Value::Enum {
            type_name: "Color".into(),
            variant: "Blue".into(),
        };
        let b = Value::Enum {
            type_name: "Color".into(),
            variant: "Red".into(),
        };
        let c = Value::Enum {
            type_name: "Shape".into(),
            variant: "A".into(),
        };
        assert!(a < b); // same type_name, Blue < Red
        assert!(b < c); // Color < Shape
    }

    #[test]
    fn value_enum_content_hash() {
        let a = Value::Enum {
            type_name: "Color".into(),
            variant: "Red".into(),
        };
        let b = Value::Enum {
            type_name: "Color".into(),
            variant: "Red".into(),
        };
        let c = Value::Enum {
            type_name: "Color".into(),
            variant: "Blue".into(),
        };
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
        assert_eq!(format!("{}", Value::Real(1e20)), "100000000000000000000");
    }

    #[test]
    fn value_display_real_large_negative() {
        assert_eq!(format!("{}", Value::Real(-1e20)), "-100000000000000000000");
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
        let value_hash = Value::Option(Some(Box::new(Value::Bool(true)))).content_hash();
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
        let pos = Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        };
        let neg = Value::Scalar {
            si_value: -0.0,
            dimension: DimensionVector::LENGTH,
        };
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
        assert!(
            display.contains("Field"),
            "expected display to contain 'Field', got: {}",
            display
        );
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
        assert_eq!(
            map.get(&id_a),
            Some(&Value::Int(1)),
            "other entries should remain"
        );
        assert_eq!(
            map.get(&id_c),
            Some(&Value::Int(3)),
            "other entries should remain"
        );
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
        assert!(
            tensor > list,
            "Tensor (tag 13) should order after List (tag 7)"
        );

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
        let v = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(format!("{}", v), "3+4i");
    }

    #[test]
    fn value_complex_display_negative_imaginary() {
        let v = Value::Complex {
            re: 3.0,
            im: -4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(format!("{}", v), "3-4i");
    }

    #[test]
    fn value_complex_display_fractional() {
        let v = Value::Complex {
            re: 3.5,
            im: 4.2,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(format!("{}", v), "3.5+4.2i");
    }

    #[test]
    fn value_complex_display_dimensioned() {
        let v = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_eq!(format!("{}", v), "(3+4i) m");
    }

    #[test]
    fn value_complex_display_zero_imaginary() {
        let v = Value::Complex {
            re: 3.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(format!("{}", v), "3+0i");
    }

    #[test]
    fn value_complex_display_negative_real() {
        let v = Value::Complex {
            re: -3.0,
            im: -4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(format!("{}", v), "-3-4i");
    }

    // ── Value::Complex PartialEq tests (step-4) ───────────────────────────────

    #[test]
    fn value_complex_eq_same() {
        let a = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(a, b);
    }

    #[test]
    fn value_complex_neq_different_re() {
        let a = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 5.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_ne!(a, b);
    }

    #[test]
    fn value_complex_neq_different_im() {
        let a = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 3.0,
            im: 5.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_ne!(a, b);
    }

    #[test]
    fn value_complex_neq_different_dimension() {
        let a = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_ne!(a, b);
    }

    #[test]
    fn value_complex_neg_zero_distinguished() {
        // -0.0 vs 0.0 distinguished via to_bits()
        let pos = Value::Complex {
            re: 0.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let neg_re = Value::Complex {
            re: -0.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let neg_im = Value::Complex {
            re: 0.0,
            im: -0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_ne!(pos, neg_re);
        assert_ne!(pos, neg_im);
    }

    #[test]
    fn value_complex_neq_real() {
        let c = Value::Complex {
            re: 3.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_ne!(c, Value::Real(3.0));
    }

    #[test]
    fn value_complex_neq_scalar() {
        let c = Value::Complex {
            re: 3.0,
            im: 0.0,
            dimension: DimensionVector::LENGTH,
        };
        let s = Value::Scalar {
            si_value: 3.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_ne!(c, s);
    }

    // ── Value::Complex Ord tests (step-5) ─────────────────────────────────────

    #[test]
    fn value_complex_sorts_after_tensor() {
        // Complex type_tag=14 > Tensor type_tag=13
        let complex = Value::Complex {
            re: 0.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let tensor = Value::Tensor(vec![Value::Int(99)]);
        assert!(
            complex > tensor,
            "Complex (tag 14) should order after Tensor (tag 13)"
        );
    }

    #[test]
    fn value_complex_sorts_before_undef() {
        // Undef tag=0, Complex tag=14 — Complex > Undef
        // (lower tag sorts first, so Undef=0 < Complex=14)
        // But also test vs something with tag > 14 doesn't exist yet,
        // so just verify cross-type ordering is consistent
        let complex = Value::Complex {
            re: 0.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let undef = Value::Undef;
        assert!(
            complex > undef,
            "Complex (tag 14) should order after Undef (tag 0)"
        );
    }

    #[test]
    fn value_complex_ord_dimension_first() {
        // Same re/im, different dimension — dimension compared first
        // LENGTH > DIMENSIONLESS in DimensionVector ordering
        let a = Value::Complex {
            re: 1.0,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 1.0,
            im: 1.0,
            dimension: DimensionVector::LENGTH,
        };
        // They should not be equal; whichever dimension ordering, they differ
        assert_ne!(a.cmp(&b), std::cmp::Ordering::Equal);
    }

    #[test]
    fn value_complex_ord_re_second() {
        // Same dimension, different re — re bits compared second
        let a = Value::Complex {
            re: 1.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 2.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(a < b);
    }

    #[test]
    fn value_complex_ord_im_third() {
        // Same dimension+re, different im — im bits compared third
        let a = Value::Complex {
            re: 1.0,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(a < b);
    }

    #[test]
    fn value_complex_partial_ord_consistent() {
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 1.0,
            im: 3.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(a.partial_cmp(&b), Some(std::cmp::Ordering::Less));
        assert_eq!(b.partial_cmp(&a), Some(std::cmp::Ordering::Greater));
    }

    // ── Value::Complex content_hash tests (step-6) ────────────────────────────

    #[test]
    fn value_complex_hash_determinism() {
        let a = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_complex_nan_re_canonicalized() {
        let a = Value::Complex {
            re: f64::NAN,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: f64::NAN,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_complex_hash_eq_implies_same_hash() {
        // Equal values produce equal hashes
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::LENGTH,
        };
        let b = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_eq!(a, b);
        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_complex_different_re_different_hash() {
        let a = Value::Complex {
            re: 1.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 2.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_complex_different_dimension_different_hash() {
        let a = Value::Complex {
            re: 3.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 3.0,
            im: 0.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_complex_hash_differs_from_scalar() {
        // Complex tag=15 vs Scalar tag=4 — hashes must differ even with same numeric value
        let c = Value::Complex {
            re: 3.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let s = Value::Scalar {
            si_value: 3.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_ne!(c.content_hash(), s.content_hash());
    }

    // ── Value::Complex dimension() test (step-7) ──────────────────────────────

    #[test]
    fn value_complex_dimension_returns_stored() {
        let v = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_eq!(v.dimension(), DimensionVector::LENGTH);
    }

    #[test]
    fn value_complex_dimensionless_returns_dimensionless() {
        let v = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(v.dimension(), DimensionVector::DIMENSIONLESS);
    }

    // ── Value::Orientation tests (step-3) ────────────────────────────────────

    #[test]
    fn value_orientation_construction() {
        let o = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        // Should not be undef
        assert!(!o.is_undef());
    }

    #[test]
    fn value_orientation_eq_same() {
        let a = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let b = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert_eq!(a, b);
    }

    #[test]
    fn value_orientation_eq_different() {
        let a = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let b = Value::Orientation {
            w: 0.0,
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        assert_ne!(a, b);
    }

    #[test]
    fn value_orientation_eq_nan_bitwise() {
        // NaN == NaN via to_bits (bitwise equality)
        let a = Value::Orientation {
            w: f64::NAN,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let b = Value::Orientation {
            w: f64::NAN,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert_eq!(a, b);
    }

    #[test]
    fn value_orientation_eq_neg_zero() {
        // -0.0 != 0.0 via to_bits
        let a = Value::Orientation {
            w: -0.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let b = Value::Orientation {
            w: 0.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert_ne!(a, b);
    }

    #[test]
    fn value_orientation_ord_cross_type() {
        // Orientation should sort after Complex (tag 14), so Orientation tag = 15
        let complex = Value::Complex {
            re: 0.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let orient = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(complex < orient);
    }

    #[test]
    fn value_orientation_ord_within_type() {
        // Lexicographic on w, x, y, z via to_bits
        let a = Value::Orientation {
            w: 0.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let b = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(a < b);

        // Same w, different x
        let c = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let d = Value::Orientation {
            w: 1.0,
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(c < d);
    }

    #[test]
    fn value_orientation_ord_equal_w_different_x() {
        // Equal w, different x with non-zero y — catches field-order swap regressions.
        // Correct Ord (w→x→y→z): e > f because x=1.0 > x=0.5 when w is tied.
        // A wrong impl comparing y before x would say e < f (y=0.5 < y=1.0).
        let e = Value::Orientation {
            w: 0.5,
            x: 1.0,
            y: 0.5,
            z: 0.0,
        };
        let f = Value::Orientation {
            w: 0.5,
            x: 0.5,
            y: 1.0,
            z: 0.0,
        };
        assert!(e > f);
    }

    #[test]
    fn value_orientation_ord_equal_wx_different_y() {
        // Equal w and x, different y with non-zero z — catches y↔z field-order swap regressions.
        // Correct Ord (w→x→y→z): g > h because y=1.0 > y=0.5 when w and x are tied.
        // A wrong impl comparing z before y would say g < h (z=0.5 < z=1.0).
        let g = Value::Orientation {
            w: 0.5,
            x: 0.5,
            y: 1.0,
            z: 0.5,
        };
        let h = Value::Orientation {
            w: 0.5,
            x: 0.5,
            y: 0.5,
            z: 1.0,
        };
        assert!(g > h);
    }

    #[test]
    fn value_orientation_ord_equal_wxy_different_z() {
        // Equal w, x, and y — different z only. Catches regression where z comparison is dropped.
        // Correct Ord (w→x→y→z): i > j because z=1.0 > z=0.5 when w, x, y are tied.
        let i = Value::Orientation {
            w: 0.5,
            x: 0.5,
            y: 0.5,
            z: 1.0,
        };
        let j = Value::Orientation {
            w: 0.5,
            x: 0.5,
            y: 0.5,
            z: 0.5,
        };
        assert!(i > j);
    }

    #[test]
    fn value_orientation_display() {
        let o = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert_eq!(format!("{}", o), "[1, 0, 0, 0]q");
    }

    #[test]
    fn value_orientation_display_fractional() {
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let o = Value::Orientation {
            w: s,
            x: 0.0,
            y: 0.0,
            z: s,
        };
        let display = format!("{}", o);
        assert!(display.starts_with('['));
        assert!(display.ends_with("]q"));
    }

    #[test]
    fn value_orientation_content_hash_deterministic() {
        let a = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let b = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_orientation_content_hash_nan_canonical() {
        let a = Value::Orientation {
            w: f64::NAN,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let b = Value::Orientation {
            w: f64::NAN,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_orientation_content_hash_distinct_from_complex() {
        // Tag 16 for Orientation vs tag 15 for Complex
        let o = Value::Orientation {
            w: 0.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let c = Value::Complex {
            re: 0.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_ne!(o.content_hash(), c.content_hash());
    }

    #[test]
    fn value_orientation_content_hash_neg_zero() {
        let a = Value::Orientation {
            w: -0.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let b = Value::Orientation {
            w: 0.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn value_orientation_as_f64_none() {
        let o = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert_eq!(o.as_f64(), None);
    }

    #[test]
    fn value_orientation_dimension_dimensionless() {
        let o = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
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
        let orient = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
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
        let orient = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
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
            Value::Range {
                lower_inclusive, ..
            } => assert!(!lower_inclusive),
            _ => panic!("expected Range"),
        }
    }

    #[test]
    fn value_range_normalize_upper_inclusive_when_none() {
        let r = Value::range(Some(Value::Int(0)), None, true, true);
        match r {
            Value::Range {
                upper_inclusive, ..
            } => assert!(!upper_inclusive),
            _ => panic!("expected Range"),
        }
    }

    #[test]
    fn value_range_normalize_both_when_none() {
        let r = Value::range(None, None, true, true);
        match r {
            Value::Range {
                lower_inclusive,
                upper_inclusive,
                ..
            } => {
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
            Value::Range {
                lower_inclusive,
                upper_inclusive,
                ..
            } => {
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

    // ── Range gap tests: both-None hash, both-bounds-present eq/hash (task-364 pre) ─

    #[test]
    fn value_range_hash_both_none_ignores_inclusive() {
        let r1 = Value::range(None, None, true, true);
        let r2 = Value::range(None, None, false, false);
        assert_eq!(r1.content_hash(), r2.content_hash());
    }

    #[test]
    fn value_range_eq_both_bounds_present() {
        let r1 = Value::range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        let r2 = Value::range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        assert_eq!(r1, r2);
        // Different upper bound → not equal
        let r3 = Value::range(Some(Value::Int(0)), Some(Value::Int(20)), true, false);
        assert_ne!(r1, r3);
    }

    #[test]
    fn value_range_hash_both_bounds_present() {
        let r1 = Value::range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        let r2 = Value::range(Some(Value::Int(0)), Some(Value::Int(10)), true, false);
        assert_eq!(r1.content_hash(), r2.content_hash());
        // Different bounds → different hash
        let r3 = Value::range(Some(Value::Int(0)), Some(Value::Int(20)), true, false);
        assert_ne!(r1.content_hash(), r3.content_hash());
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

    // ── Range invariant re-normalization tests (step-9) ───────────────────────
    // These tests bypass Value::range() factory and directly construct Value::Range
    // with an invariant violation (lower/upper_inclusive=true when bound is None).
    // Each impl (content_hash, PartialEq, Ord, Display) silently re-normalizes
    // via normalize_range_flags.

    #[test]
    fn value_range_bypass_hash_renormalizes() {
        // Bypassed Range with lower=None+lower_inclusive=true should hash
        // identically to the correctly-constructed version.
        let bypassed = Value::Range {
            lower: None,
            lower_inclusive: true,
            upper: Some(Box::new(Value::Int(10))),
            upper_inclusive: false,
        };
        let correct = Value::range(None, Some(Value::Int(10)), false, false);
        assert_eq!(bypassed.content_hash(), correct.content_hash());
    }

    #[test]
    fn value_range_bypass_eq_renormalizes() {
        // Two Range values: one with lower=None+lower_inclusive=true (bypassed),
        // one with lower=None+lower_inclusive=false. They are logically identical.
        let bypassed = Value::Range {
            lower: None,
            lower_inclusive: true,
            upper: Some(Box::new(Value::Int(10))),
            upper_inclusive: false,
        };
        let correct = Value::range(None, Some(Value::Int(10)), false, false);
        assert_eq!(bypassed, correct);
    }

    #[test]
    fn value_range_bypass_cmp_renormalizes() {
        // Two Range values with lower=None and different lower_inclusive flags:
        // after normalization both should have lower_inclusive=false → Equal.
        let bypassed = Value::Range {
            lower: None,
            lower_inclusive: true,
            upper: Some(Box::new(Value::Int(10))),
            upper_inclusive: false,
        };
        let correct = Value::range(None, Some(Value::Int(10)), false, false);
        assert_eq!(bypassed.cmp(&correct), std::cmp::Ordering::Equal);
    }

    // ── Bypass normalization-verifying tests (task-364) ─────────────────────
    // These construct Value::Range directly (bypassing Value::range()), setting
    // invariant-violating flags. Each impl must silently re-normalize so the
    // output is correct.

    #[test]
    fn value_range_bypass_display_renormalizes_lower() {
        // lower=None + lower_inclusive=true → Display must output '(' not '['
        let r = Value::Range {
            lower: None,
            lower_inclusive: true,
            upper: Some(Box::new(Value::Int(10))),
            upper_inclusive: false,
        };
        let s = format!("{}", r);
        assert!(s.starts_with('('), "expected '(' but got: {}", s);
    }

    #[test]
    fn value_range_bypass_display_renormalizes_upper() {
        // upper=None + upper_inclusive=true → Display must output ')' not ']'
        let r = Value::Range {
            lower: Some(Box::new(Value::Int(0))),
            lower_inclusive: true,
            upper: None,
            upper_inclusive: true,
        };
        let s = format!("{}", r);
        assert!(s.ends_with(')'), "expected ')' but got: {}", s);
    }

    // ── Value::Matrix Ord tests (step-7) ─────────────────────────────────────

    #[test]
    fn value_matrix_ord_cross_type_after_range() {
        // (a) Matrix (tag 17) > Range (tag 16)
        let matrix = Value::Matrix(vec![vec![Value::Int(1)]]);
        let range = Value::range(Some(Value::Int(0)), Some(Value::Int(10)), true, true);
        assert!(matrix > range);
    }

    #[test]
    fn value_matrix_ord_within_type_lexicographic() {
        // (b) lexicographic ordering on rows: [[1,2],[3,4]] < [[1,2],[3,5]]
        let m1 = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2)],
            vec![Value::Int(3), Value::Int(4)],
        ]);
        let m2 = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2)],
            vec![Value::Int(3), Value::Int(5)],
        ]);
        assert!(m1 < m2);
        assert!(m2 > m1);
        // Equal matrices compare equal
        let m3 = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2)],
            vec![Value::Int(3), Value::Int(4)],
        ]);
        assert_eq!(m1.cmp(&m3), std::cmp::Ordering::Equal);
    }

    // ── Value::Matrix content_hash tests (step-5) ────────────────────────────

    #[test]
    fn value_matrix_content_hash_determinism() {
        // (a) same matrix produces same hash
        let m1 = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2)],
            vec![Value::Int(3), Value::Int(4)],
        ]);
        let m2 = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2)],
            vec![Value::Int(3), Value::Int(4)],
        ]);
        assert_eq!(m1.content_hash(), m2.content_hash());
    }

    #[test]
    fn value_matrix_content_hash_transposed_differs() {
        // (b) transposed matrix has different hash
        let m_normal = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2)],
            vec![Value::Int(3), Value::Int(4)],
        ]);
        let m_transposed = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(3)],
            vec![Value::Int(2), Value::Int(4)],
        ]);
        assert_ne!(m_normal.content_hash(), m_transposed.content_hash());
    }

    #[test]
    fn value_matrix_content_hash_distinct_from_tensor() {
        // (c) same elements as Tensor produce different hash (different tag)
        let matrix = Value::Matrix(vec![vec![Value::Int(1), Value::Int(2)]]);
        let tensor = Value::Tensor(vec![Value::Int(1), Value::Int(2)]);
        assert_ne!(matrix.content_hash(), tensor.content_hash());
    }

    // ── Value::Matrix tests (step-3) ─────────────────────────────────────────

    #[test]
    fn value_matrix_construction_and_partial_eq() {
        // (a) same rows equal
        let m1 = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2), Value::Int(3)],
            vec![Value::Int(4), Value::Int(5), Value::Int(6)],
        ]);
        let m2 = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2), Value::Int(3)],
            vec![Value::Int(4), Value::Int(5), Value::Int(6)],
        ]);
        assert_eq!(m1, m2);

        // different element — not equal
        let m3 = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2), Value::Int(3)],
            vec![Value::Int(4), Value::Int(5), Value::Int(7)],
        ]);
        assert_ne!(m1, m3);

        // different shape — not equal
        let m4 = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2)],
            vec![Value::Int(3), Value::Int(4)],
        ]);
        assert_ne!(m1, m4);

        // cross-variant: Matrix != List
        let list = Value::List(vec![Value::Int(1), Value::Int(2)]);
        assert_ne!(
            Value::Matrix(vec![vec![Value::Int(1), Value::Int(2)]]),
            list
        );

        // cross-variant: Matrix != Tensor
        let tensor = Value::Tensor(vec![Value::Int(1), Value::Int(2)]);
        assert_ne!(
            Value::Matrix(vec![vec![Value::Int(1), Value::Int(2)]]),
            tensor
        );
    }

    #[test]
    fn value_matrix_display_2x3() {
        // (b) 2x3 matrix: [[1, 2, 3], [4, 5, 6]]
        let m = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2), Value::Int(3)],
            vec![Value::Int(4), Value::Int(5), Value::Int(6)],
        ]);
        assert_eq!(format!("{}", m), "[[1, 2, 3], [4, 5, 6]]");
    }

    #[test]
    fn value_matrix_display_1x1() {
        // (b) 1x1 matrix: [[1]]
        let m = Value::Matrix(vec![vec![Value::Int(1)]]);
        assert_eq!(format!("{}", m), "[[1]]");
    }

    // ── Value::Matrix canonicalize_matrix / try_into_matrix tests (step-11) ─

    #[test]
    fn canonicalize_matrix_converts_to_nested_tensor() {
        // (a) Matrix([[1,2],[3,4]]) → Tensor([Tensor([1,2]), Tensor([3,4])])
        let matrix = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2)],
            vec![Value::Int(3), Value::Int(4)],
        ]);
        let expected = Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(1), Value::Int(2)]),
            Value::Tensor(vec![Value::Int(3), Value::Int(4)]),
        ]);
        assert_eq!(matrix.canonicalize_matrix(), expected);
    }

    #[test]
    fn canonicalize_matrix_is_identity_for_non_matrix() {
        // (b) non-Matrix values pass through unchanged
        assert_eq!(Value::Int(42).canonicalize_matrix(), Value::Int(42));
        assert_eq!(
            Value::Tensor(vec![Value::Int(1), Value::Int(2)]).canonicalize_matrix(),
            Value::Tensor(vec![Value::Int(1), Value::Int(2)])
        );
        assert_eq!(Value::Undef.canonicalize_matrix(), Value::Undef);
    }

    #[test]
    fn canonicalize_matrix_empty_rows() {
        // (c) Matrix([[],[]])  → Tensor([Tensor([]), Tensor([])])
        let matrix = Value::Matrix(vec![vec![], vec![]]);
        let expected = Value::Tensor(vec![Value::Tensor(vec![]), Value::Tensor(vec![])]);
        assert_eq!(matrix.canonicalize_matrix(), expected);
    }

    #[test]
    fn try_into_matrix_rank2_tensor_converts() {
        // (d) rank-2 Tensor([Tensor([1,2]), Tensor([3,4])]) → Some(Matrix([[1,2],[3,4]]))
        let tensor = Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(1), Value::Int(2)]),
            Value::Tensor(vec![Value::Int(3), Value::Int(4)]),
        ]);
        let expected = Some(Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2)],
            vec![Value::Int(3), Value::Int(4)],
        ]));
        assert_eq!(tensor.try_into_matrix(), expected);
    }

    #[test]
    fn try_into_matrix_rank1_tensor_returns_none() {
        // (e) rank-1 Tensor([1,2,3]) → None (not all-Tensor elements)
        let tensor = Value::Tensor(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        assert_eq!(tensor.try_into_matrix(), None);
    }

    #[test]
    fn try_into_matrix_non_tensor_returns_none() {
        // (f) non-Tensor values return None
        assert_eq!(Value::Int(42).try_into_matrix(), None);
        assert_eq!(
            Value::Matrix(vec![vec![Value::Int(1)]]).try_into_matrix(),
            None
        );
    }

    #[test]
    fn canonicalize_matrix_round_trip() {
        // (g) round-trip: matrix.clone().canonicalize_matrix().try_into_matrix() == Some(matrix)
        let matrix = Value::Matrix(vec![
            vec![Value::Int(1), Value::Int(2)],
            vec![Value::Int(3), Value::Int(4)],
        ]);
        let round_tripped = matrix.clone().canonicalize_matrix().try_into_matrix();
        assert_eq!(round_tripped, Some(matrix));
    }

    // ── Value::Frame tests (step-3) ──────────────────────────────────────────

    fn make_point3_length() -> Value {
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ])
    }

    fn make_orientation_identity() -> Value {
        Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    fn make_frame(origin: Value, basis: Value) -> Value {
        Value::Frame {
            origin: Box::new(origin),
            basis: Box::new(basis),
        }
    }

    #[test]
    fn value_frame_construction() {
        let origin = make_point3_length();
        let basis = make_orientation_identity();
        let frame = make_frame(origin.clone(), basis.clone());
        match frame {
            Value::Frame {
                origin: o,
                basis: b,
            } => {
                assert_eq!(*o, origin);
                assert_eq!(*b, basis);
            }
            other => panic!("expected Value::Frame, got {:?}", other),
        }
    }

    #[test]
    fn value_frame_partial_eq_equal() {
        let f1 = make_frame(make_point3_length(), make_orientation_identity());
        let f2 = make_frame(make_point3_length(), make_orientation_identity());
        assert_eq!(f1, f2);
    }

    #[test]
    fn value_frame_partial_eq_different_origin() {
        let origin_a = Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        let origin_b = Value::Point(vec![
            Value::length(9.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        let basis = make_orientation_identity();
        let f1 = make_frame(origin_a, basis.clone());
        let f2 = make_frame(origin_b, basis);
        assert_ne!(f1, f2);
    }

    #[test]
    fn value_frame_partial_eq_different_basis() {
        let origin = make_point3_length();
        let basis_a = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let basis_b = Value::Orientation {
            w: 0.0,
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        let f1 = make_frame(origin.clone(), basis_a);
        let f2 = make_frame(origin, basis_b);
        assert_ne!(f1, f2);
    }

    #[test]
    fn value_frame_display() {
        let origin = Value::Point(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let basis = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let frame = make_frame(origin, basis);
        let s = format!("{}", frame);
        assert_eq!(s, "frame(point(0 m, 0 m, 0 m), [1, 0, 0, 0]q)");
    }

    #[test]
    fn value_frame_dimension_is_dimensionless() {
        let frame = make_frame(make_point3_length(), make_orientation_identity());
        assert_eq!(frame.dimension(), DimensionVector::DIMENSIONLESS);
    }

    #[test]
    fn value_frame_content_hash_determinism() {
        let f1 = make_frame(make_point3_length(), make_orientation_identity());
        let f2 = make_frame(make_point3_length(), make_orientation_identity());
        assert_eq!(f1.content_hash(), f2.content_hash());
    }

    #[test]
    fn value_frame_content_hash_distinct_from_orientation() {
        let frame = make_frame(make_point3_length(), make_orientation_identity());
        let orientation = make_orientation_identity();
        assert_ne!(frame.content_hash(), orientation.content_hash());
    }

    #[test]
    fn value_frame_content_hash_distinct_from_point() {
        let frame = make_frame(make_point3_length(), make_orientation_identity());
        let point = make_point3_length();
        assert_ne!(frame.content_hash(), point.content_hash());
    }

    #[test]
    fn value_frame_ord_type_tag_gt_matrix() {
        // Frame type_tag=20 > Matrix type_tag=19
        let frame = make_frame(make_point3_length(), make_orientation_identity());
        let matrix = Value::Matrix(vec![vec![Value::Int(1)]]);
        assert!(frame > matrix);
    }

    #[test]
    fn value_frame_ord_same_type_compare_origin_first() {
        // Two frames with same basis but different origin should order by origin
        let origin_a = Value::Point(vec![
            Value::length(1.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let origin_b = Value::Point(vec![
            Value::length(2.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let basis = make_orientation_identity();
        let f1 = make_frame(origin_a, basis.clone());
        let f2 = make_frame(origin_b, basis);
        assert!(f1 < f2);
    }

    #[test]
    fn value_frame_ord_same_origin_compare_basis() {
        // Same origin, different basis: order by basis quaternion
        let origin = make_point3_length();
        // Valid 180° rotation around X-axis (unit quaternion: |q|=1).
        // w=0.0 < w=1.0 by to_bits ordering, so basis_a < basis_b.
        let basis_a = Value::Orientation {
            w: 0.0,
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        let basis_b = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let f1 = make_frame(origin.clone(), basis_a);
        let f2 = make_frame(origin, basis_b);
        assert!(f1 < f2);
    }

    #[test]
    fn value_frame_content_hash_neg_zero_origin_differs() {
        // neg-zero and pos-zero in origin produce different hashes
        let origin_pos = Value::Point(vec![
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let origin_neg = Value::Point(vec![
            Value::Scalar {
                si_value: -0.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let basis = make_orientation_identity();
        let f1 = make_frame(origin_pos, basis.clone());
        let f2 = make_frame(origin_neg, basis);
        assert_ne!(f1.content_hash(), f2.content_hash());
    }

    #[test]
    fn value_frame_dimension_explicit_arm() {
        // Ensures dimension() has an explicit Frame arm (not just the wildcard).
        let frame = make_frame(make_point3_length(), make_orientation_identity());
        assert_eq!(frame.dimension(), DimensionVector::DIMENSIONLESS);
    }

    #[test]
    #[should_panic(expected = "infer_type() cannot infer Frame")]
    fn value_frame_infer_type_panics() {
        let frame = make_frame(make_point3_length(), make_orientation_identity());
        let _ = frame.infer_type();
    }

    #[test]
    fn value_frame_ne_orientation() {
        let frame = make_frame(make_point3_length(), make_orientation_identity());
        let orientation = make_orientation_identity();
        assert_ne!(frame, orientation);
    }

    #[test]
    fn value_frame_ne_point() {
        let frame = make_frame(make_point3_length(), make_orientation_identity());
        let point = make_point3_length();
        assert_ne!(frame, point);
    }

    #[test]
    #[should_panic(expected = "infer_type() cannot infer Transform")]
    fn value_transform_infer_type_panics() {
        let transform = make_transform(make_orientation_identity(), make_vector3_length());
        let _ = transform.infer_type();
    }

    // ── Value::Transform tests (step-3) ──────────────────────────────────────

    fn make_vector3_length() -> Value {
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ])
    }

    fn make_transform(rotation: Value, translation: Value) -> Value {
        Value::Transform {
            rotation: Box::new(rotation),
            translation: Box::new(translation),
        }
    }

    #[test]
    fn value_transform_construction() {
        let rotation = make_orientation_identity();
        let translation = make_vector3_length();
        let transform = make_transform(rotation.clone(), translation.clone());
        match transform {
            Value::Transform {
                rotation: r,
                translation: t,
            } => {
                assert_eq!(*r, rotation);
                assert_eq!(*t, translation);
            }
            other => panic!("expected Value::Transform, got {:?}", other),
        }
    }

    #[test]
    fn value_transform_partial_eq_equal() {
        let t1 = make_transform(make_orientation_identity(), make_vector3_length());
        let t2 = make_transform(make_orientation_identity(), make_vector3_length());
        assert_eq!(t1, t2);
    }

    #[test]
    fn value_transform_partial_eq_different_rotation() {
        let rot_a = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let rot_b = Value::Orientation {
            w: 0.0,
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        let translation = make_vector3_length();
        let t1 = make_transform(rot_a, translation.clone());
        let t2 = make_transform(rot_b, translation);
        assert_ne!(t1, t2);
    }

    #[test]
    fn value_transform_partial_eq_different_translation() {
        let rotation = make_orientation_identity();
        let trans_a = Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        let trans_b = Value::Vector(vec![
            Value::length(9.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        let t1 = make_transform(rotation.clone(), trans_a);
        let t2 = make_transform(rotation, trans_b);
        assert_ne!(t1, t2);
    }

    #[test]
    fn value_transform_display() {
        let rotation = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let translation = Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let transform = make_transform(rotation, translation);
        let s = format!("{}", transform);
        // Expected: transform([1, 0, 0, 0]q, vec(0 m, 0 m, 0 m))
        assert!(
            s.starts_with("transform("),
            "display should start with 'transform(', got: {}",
            s
        );
        assert!(
            s.contains("[1, 0, 0, 0]q"),
            "display should contain rotation, got: {}",
            s
        );
    }

    #[test]
    fn value_transform_dimension_is_dimensionless() {
        let transform = make_transform(make_orientation_identity(), make_vector3_length());
        assert_eq!(transform.dimension(), DimensionVector::DIMENSIONLESS);
    }

    #[test]
    fn value_transform_content_hash_determinism() {
        let t1 = make_transform(make_orientation_identity(), make_vector3_length());
        let t2 = make_transform(make_orientation_identity(), make_vector3_length());
        assert_eq!(t1.content_hash(), t2.content_hash());
    }

    #[test]
    fn value_transform_content_hash_distinct_from_frame() {
        let transform = make_transform(make_orientation_identity(), make_vector3_length());
        let frame = make_frame(make_point3_length(), make_orientation_identity());
        assert_ne!(transform.content_hash(), frame.content_hash());
    }

    #[test]
    fn value_transform_content_hash_distinct_from_orientation() {
        let transform = make_transform(make_orientation_identity(), make_vector3_length());
        let orientation = make_orientation_identity();
        assert_ne!(transform.content_hash(), orientation.content_hash());
    }

    #[test]
    fn value_transform_content_hash_distinct_from_vector() {
        let transform = make_transform(make_orientation_identity(), make_vector3_length());
        let vector = make_vector3_length();
        assert_ne!(transform.content_hash(), vector.content_hash());
    }

    #[test]
    fn value_transform_ord_type_tag_gt_frame() {
        // Transform type_tag=21 > Frame type_tag=20
        let transform = make_transform(make_orientation_identity(), make_vector3_length());
        let frame = make_frame(make_point3_length(), make_orientation_identity());
        assert!(transform > frame);
    }

    #[test]
    fn value_transform_ord_same_type_compare_rotation_first() {
        // Two transforms with same translation but different rotation: order by rotation
        let rot_a = Value::Orientation {
            w: 0.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let rot_b = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let translation = make_vector3_length();
        let t1 = make_transform(rot_a, translation.clone());
        let t2 = make_transform(rot_b, translation);
        assert!(t1 < t2);
    }

    #[test]
    fn value_transform_ord_same_rotation_compare_translation() {
        // Same rotation, different translation: order by translation
        let rotation = make_orientation_identity();
        let trans_a = Value::Vector(vec![
            Value::length(1.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let trans_b = Value::Vector(vec![
            Value::length(2.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let t1 = make_transform(rotation.clone(), trans_a);
        let t2 = make_transform(rotation, trans_b);
        assert!(t1 < t2);
    }

    #[test]
    fn value_transform_content_hash_neg_zero_translation_differs() {
        // neg-zero and pos-zero in translation produce different hashes
        let trans_pos = Value::Vector(vec![
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let trans_neg = Value::Vector(vec![
            Value::Scalar {
                si_value: -0.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let rotation = make_orientation_identity();
        let t1 = make_transform(rotation.clone(), trans_pos);
        let t2 = make_transform(rotation, trans_neg);
        assert_ne!(t1.content_hash(), t2.content_hash());
    }

    // ── Value::Plane tests (pre-2) ────────────────────────────────────────────

    fn make_plane(origin: Value, normal: Value) -> Value {
        Value::Plane {
            origin: Box::new(origin),
            normal: Box::new(normal),
        }
    }

    fn make_point3_origin() -> Value {
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ])
    }

    fn make_normal_z() -> Value {
        Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)])
    }

    #[test]
    fn value_plane_construction() {
        let origin = make_point3_origin();
        let normal = make_normal_z();
        let plane = make_plane(origin.clone(), normal.clone());
        match plane {
            Value::Plane {
                origin: o,
                normal: n,
            } => {
                assert_eq!(*o, origin);
                assert_eq!(*n, normal);
            }
            other => panic!("expected Value::Plane, got {:?}", other),
        }
    }

    #[test]
    fn value_plane_partial_eq_same() {
        let p1 = make_plane(make_point3_origin(), make_normal_z());
        let p2 = make_plane(make_point3_origin(), make_normal_z());
        assert_eq!(p1, p2);
    }

    #[test]
    fn value_plane_partial_eq_different() {
        let p1 = make_plane(make_point3_origin(), make_normal_z());
        let normal_x = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let p2 = make_plane(make_point3_origin(), normal_x);
        assert_ne!(p1, p2);
    }

    #[test]
    fn value_plane_display() {
        let origin = Value::Point(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let normal = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let plane = make_plane(origin, normal);
        let s = format!("{}", plane);
        assert!(
            s.starts_with("plane("),
            "display should start with 'plane(', got: {}",
            s
        );
    }

    #[test]
    fn value_plane_content_hash_deterministic() {
        let p1 = make_plane(make_point3_origin(), make_normal_z());
        let p2 = make_plane(make_point3_origin(), make_normal_z());
        assert_eq!(p1.content_hash(), p2.content_hash());
    }

    #[test]
    fn value_plane_content_hash_no_collision_with_transform() {
        let plane = make_plane(make_point3_origin(), make_normal_z());
        let transform = make_transform(make_orientation_identity(), make_vector3_length());
        assert_ne!(plane.content_hash(), transform.content_hash());
    }

    #[test]
    fn value_plane_ord_cross_type() {
        // Plane type_tag=22 > Transform type_tag=21
        let plane = make_plane(make_point3_origin(), make_normal_z());
        let transform = make_transform(make_orientation_identity(), make_vector3_length());
        assert!(plane > transform);
    }

    #[test]
    fn value_plane_dimension_dimensionless() {
        let plane = make_plane(make_point3_origin(), make_normal_z());
        assert_eq!(plane.dimension(), DimensionVector::DIMENSIONLESS);
    }

    // ── Value::Axis tests (pre-3) ─────────────────────────────────────────────

    fn make_axis(origin: Value, direction: Value) -> Value {
        Value::Axis {
            origin: Box::new(origin),
            direction: Box::new(direction),
        }
    }

    fn make_direction_z() -> Value {
        Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)])
    }

    #[test]
    fn value_axis_construction() {
        let origin = make_point3_origin();
        let direction = make_direction_z();
        let axis = make_axis(origin.clone(), direction.clone());
        match axis {
            Value::Axis {
                origin: o,
                direction: d,
            } => {
                assert_eq!(*o, origin);
                assert_eq!(*d, direction);
            }
            other => panic!("expected Value::Axis, got {:?}", other),
        }
    }

    #[test]
    fn value_axis_partial_eq_same() {
        let a1 = make_axis(make_point3_origin(), make_direction_z());
        let a2 = make_axis(make_point3_origin(), make_direction_z());
        assert_eq!(a1, a2);
    }

    #[test]
    fn value_axis_partial_eq_different() {
        let a1 = make_axis(make_point3_origin(), make_direction_z());
        let dir_x = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let a2 = make_axis(make_point3_origin(), dir_x);
        assert_ne!(a1, a2);
    }

    #[test]
    fn value_axis_display() {
        let origin = Value::Point(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let direction = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let axis = make_axis(origin, direction);
        let s = format!("{}", axis);
        assert!(
            s.starts_with("axis("),
            "display should start with 'axis(', got: {}",
            s
        );
    }

    #[test]
    fn value_axis_content_hash_deterministic() {
        let a1 = make_axis(make_point3_origin(), make_direction_z());
        let a2 = make_axis(make_point3_origin(), make_direction_z());
        assert_eq!(a1.content_hash(), a2.content_hash());
    }

    #[test]
    fn value_axis_content_hash_no_collision_with_plane() {
        let axis = make_axis(make_point3_origin(), make_direction_z());
        let plane = make_plane(make_point3_origin(), make_normal_z());
        // Plane tag=22, Axis tag=23 — distinct even if fields match
        assert_ne!(axis.content_hash(), plane.content_hash());
    }

    #[test]
    fn value_axis_ord_cross_type() {
        // Axis type_tag=23 > Plane type_tag=22
        let axis = make_axis(make_point3_origin(), make_direction_z());
        let plane = make_plane(make_point3_origin(), make_normal_z());
        assert!(axis > plane);
    }

    #[test]
    fn value_axis_dimension_dimensionless() {
        let axis = make_axis(make_point3_origin(), make_direction_z());
        assert_eq!(axis.dimension(), DimensionVector::DIMENSIONLESS);
    }

    // ── Value::BoundingBox tests (pre-4) ──────────────────────────────────────

    fn make_bbox(min: Value, max: Value) -> Value {
        Value::BoundingBox {
            min: Box::new(min),
            max: Box::new(max),
        }
    }

    fn make_point3_min() -> Value {
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ])
    }

    fn make_point3_max() -> Value {
        Value::Point(vec![
            Value::length(4.0),
            Value::length(6.0),
            Value::length(9.0),
        ])
    }

    #[test]
    fn value_bbox_construction() {
        let min = make_point3_min();
        let max = make_point3_max();
        let bbox = make_bbox(min.clone(), max.clone());
        match bbox {
            Value::BoundingBox { min: mn, max: mx } => {
                assert_eq!(*mn, min);
                assert_eq!(*mx, max);
            }
            other => panic!("expected Value::BoundingBox, got {:?}", other),
        }
    }

    #[test]
    fn value_bbox_partial_eq_same() {
        let b1 = make_bbox(make_point3_min(), make_point3_max());
        let b2 = make_bbox(make_point3_min(), make_point3_max());
        assert_eq!(b1, b2);
    }

    #[test]
    fn value_bbox_partial_eq_different() {
        let b1 = make_bbox(make_point3_min(), make_point3_max());
        let max2 = Value::Point(vec![
            Value::length(5.0),
            Value::length(6.0),
            Value::length(9.0),
        ]);
        let b2 = make_bbox(make_point3_min(), max2);
        assert_ne!(b1, b2);
    }

    #[test]
    fn value_bbox_display() {
        let bbox = make_bbox(make_point3_min(), make_point3_max());
        let s = format!("{}", bbox);
        assert!(
            s.starts_with("bbox("),
            "display should start with 'bbox(', got: {}",
            s
        );
    }

    #[test]
    fn value_bbox_content_hash_deterministic() {
        let b1 = make_bbox(make_point3_min(), make_point3_max());
        let b2 = make_bbox(make_point3_min(), make_point3_max());
        assert_eq!(b1.content_hash(), b2.content_hash());
    }

    #[test]
    fn value_bbox_content_hash_no_collision_with_axis() {
        let bbox = make_bbox(make_point3_min(), make_point3_max());
        let axis = make_axis(make_point3_origin(), make_direction_z());
        // BoundingBox tag=24, Axis tag=23 — distinct
        assert_ne!(bbox.content_hash(), axis.content_hash());
    }

    #[test]
    fn value_bbox_ord_cross_type() {
        // BoundingBox type_tag=24 > Axis type_tag=23
        let bbox = make_bbox(make_point3_min(), make_point3_max());
        let axis = make_axis(make_point3_origin(), make_direction_z());
        assert!(bbox > axis);
    }

    #[test]
    fn value_bbox_dimension_dimensionless() {
        let bbox = make_bbox(make_point3_min(), make_point3_max());
        assert_eq!(bbox.dimension(), DimensionVector::DIMENSIONLESS);
    }

    // ── Value::neg() scalar tests ───────────────────────────────────────────

    #[test]
    fn neg_int_positive() {
        assert_eq!(-Value::Int(5), Value::Int(-5));
    }

    #[test]
    fn neg_real() {
        assert_eq!(-Value::Real(2.5), Value::Real(-2.5));
    }

    #[test]
    fn neg_scalar_length() {
        assert_eq!(
            -Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: -1.0,
                dimension: DimensionVector::LENGTH,
            }
        );
    }

    #[test]
    fn neg_complex() {
        assert_eq!(
            -Value::Complex {
                re: 1.0,
                im: 2.0,
                dimension: DimensionVector::DIMENSIONLESS,
            },
            Value::Complex {
                re: -1.0,
                im: -2.0,
                dimension: DimensionVector::DIMENSIONLESS,
            }
        );
    }

    #[test]
    fn neg_int_min_overflow_returns_undef() {
        assert_eq!(-Value::Int(i64::MIN), Value::Undef);
    }

    #[test]
    fn neg_bool_returns_undef() {
        assert_eq!(-Value::Bool(true), Value::Undef);
    }

    #[test]
    fn neg_undef_returns_undef() {
        assert_eq!(-Value::Undef, Value::Undef);
    }

    // ── Value::neg() composite tests ────────────────────────────────────────

    #[test]
    fn neg_tensor_int_elements() {
        assert_eq!(
            -Value::Tensor(vec![Value::Int(1), Value::Int(2)]),
            Value::Tensor(vec![Value::Int(-1), Value::Int(-2)])
        );
    }

    #[test]
    fn neg_tensor_with_overflow_returns_undef() {
        // One element overflows → entire result is Undef
        assert_eq!(
            -Value::Tensor(vec![Value::Int(i64::MIN), Value::Int(1)]),
            Value::Undef
        );
    }

    #[test]
    fn neg_vector_length_components() {
        assert_eq!(
            -Value::Vector(vec![Value::length(1.0), Value::length(2.0)]),
            Value::Vector(vec![Value::length(-1.0), Value::length(-2.0)])
        );
    }

    #[test]
    fn neg_point_returns_undef() {
        // Affine geometry: point negation is undefined (spec 3.3.1)
        assert_eq!(
            -Value::Point(vec![Value::length(1.0), Value::length(2.0)]),
            Value::Undef
        );
    }
}
