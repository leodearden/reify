use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, Documentation, MarkupContent, MarkupKind, Position, Url,
};

use crate::analysis::AnalysisContext;
use crate::convert::position_to_offset;

/// The syntactic context at the cursor position, used to filter completions.
#[derive(Debug)]
pub enum CursorContext {
    /// Cursor is outside all structure/occurrence spans.
    TopLevel,
    /// Cursor is inside a structure/occurrence body on a line that doesn't
    /// indicate a more specific context (expression, dot access, type position).
    StructureBody {
        /// Name of the enclosing structure/occurrence.
        structure_name: String,
    },
    /// Cursor is in an expression position (after `=`, inside a constraint, etc).
    Expression {
        /// Name of the enclosing structure, if any.
        structure_name: Option<String>,
    },
    /// Cursor is immediately after a `.` — member access.
    DotAccess,
    /// Cursor is in a type annotation position (after `:` in a declaration).
    TypePosition,
}

/// Determine the syntactic context at the given cursor position.
pub fn determine_context(source: &str, position: Position, ctx: &AnalysisContext) -> CursorContext {
    let offset = position_to_offset(source, position);

    // Check if cursor is inside a structure/occurrence span
    let enclosing = ctx.enclosing_decl_name_at(offset);

    if enclosing.is_none() {
        return CursorContext::TopLevel;
    }

    let structure_name = enclosing.unwrap().to_string();

    // Extract the current line prefix (text from start of line to cursor)
    let line_prefix = extract_line_prefix(source, offset);

    // Check for DotAccess: scan backward through whitespace for a '.'
    {
        let trimmed = line_prefix.trim_end();
        if trimmed.ends_with('.') {
            return CursorContext::DotAccess;
        }
    }

    // Check for TypePosition: look for ':' without intervening '=' on the line prefix
    // Must check before Expression since 'param x: ' has no '=' yet
    {
        let trimmed = line_prefix.trim_start();
        if starts_with_decl_keyword(trimmed)
            && let Some(colon_pos) = line_prefix.rfind(':')
        {
            let after_colon = &line_prefix[colon_pos + 1..];
            if !after_colon.contains('=') {
                return CursorContext::TypePosition;
            }
        }
    }

    // Check for Expression: cursor after '=' on the line, or inside a constraint expression
    {
        if line_prefix.contains('=') {
            // Cursor is after an '=' sign — expression position
            // But only if the cursor is after the last '=' on the line
            if let Some(eq_pos) = line_prefix.rfind('=') {
                let cursor_in_line = line_prefix.len();
                if cursor_in_line > eq_pos {
                    return CursorContext::Expression {
                        structure_name: Some(structure_name),
                    };
                }
            }
        }

        // Constraint lines: everything after "constraint " is an expression
        let trimmed = line_prefix.trim_start();
        if trimmed.starts_with("constraint") && trimmed.len() > "constraint".len() {
            let after_kw = &trimmed["constraint".len()..];
            if after_kw.starts_with(|c: char| c.is_whitespace()) {
                return CursorContext::Expression {
                    structure_name: Some(structure_name),
                };
            }
        }
    }

    // Default: inside a structure body but no more specific context
    CursorContext::StructureBody { structure_name }
}

/// Extract the text from the start of the current line to the given byte offset.
fn extract_line_prefix(source: &str, offset: usize) -> &str {
    let start = source[..offset].rfind('\n').map(|pos| pos + 1).unwrap_or(0);
    &source[start..offset]
}

/// Check if a trimmed line starts with a declaration keyword (param, let, sub).
fn starts_with_decl_keyword(trimmed: &str) -> bool {
    for kw in &["param", "let", "sub"] {
        if trimmed.starts_with(kw) && trimmed[kw.len()..].starts_with(|c: char| c.is_whitespace()) {
            return true;
        }
    }
    false
}

/// Compute completion items for the given position.
///
/// Returns context-sensitive completions based on the cursor position:
/// - TopLevel: top-level keywords, type names, structure names, builtins
/// - StructureBody: body/expr keywords, scoped members, structures, builtins, types
/// - Expression: expr keywords, members, builtins, structures, types
/// - DotAccess: member names only
/// - TypePosition: type names and structure names only
pub fn compute_completions(source: &str, uri: &Url, position: Position) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let ctx = AnalysisContext::new(source, uri);
    let cursor_ctx = determine_context(source, position, &ctx);

    match cursor_ctx {
        CursorContext::TopLevel => {
            push_keywords(&mut items, TOP_LEVEL_KEYWORDS);
            push_builtins(&mut items);
            push_type_names(&mut items);
            push_entity_names(&mut items, &ctx);
        }
        CursorContext::StructureBody { ref structure_name } => {
            push_keywords(&mut items, BODY_KEYWORDS);
            push_keywords(&mut items, EXPR_KEYWORDS);
            push_builtins(&mut items);
            push_type_names(&mut items);
            push_scoped_members(&mut items, &ctx, structure_name);
            push_entity_names(&mut items, &ctx);
        }
        CursorContext::Expression {
            ref structure_name, ..
        } => {
            push_keywords(&mut items, EXPR_KEYWORDS);
            push_builtins(&mut items);
            push_type_names(&mut items);
            if let Some(name) = structure_name {
                push_scoped_members(&mut items, &ctx, name);
            } else {
                push_all_members(&mut items, &ctx);
            }
            push_entity_names(&mut items, &ctx);
        }
        CursorContext::DotAccess => {
            push_all_members(&mut items, &ctx);
            push_complex_methods(&mut items);
        }
        CursorContext::TypePosition => {
            push_type_names(&mut items);
            push_entity_names(&mut items, &ctx);
        }
    }

    items
}

fn push_keywords(items: &mut Vec<CompletionItem>, keywords: &[&str]) {
    for kw in keywords {
        items.push(CompletionItem {
            label: kw.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            ..Default::default()
        });
    }
}

fn push_builtins(items: &mut Vec<CompletionItem>) {
    for info in BUILTIN_FUNCTIONS {
        items.push(CompletionItem {
            label: info.name.to_string(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some(info.signature.to_string()),
            documentation: Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: info.doc.to_string(),
            })),
            sort_text: Some(format!("{}-{}", info.sort_group, info.name)),
            ..Default::default()
        });
    }
}

fn push_type_names(items: &mut Vec<CompletionItem>) {
    for ty in TYPE_NAMES {
        items.push(CompletionItem {
            label: ty.to_string(),
            kind: Some(CompletionItemKind::CLASS),
            ..Default::default()
        });
    }
}

fn push_all_members(items: &mut Vec<CompletionItem>, ctx: &AnalysisContext) {
    for (name, _kind, cell_type) in ctx.member_names() {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::VARIABLE),
            detail: Some(cell_type.to_string()),
            ..Default::default()
        });
    }
}

fn push_scoped_members(
    items: &mut Vec<CompletionItem>,
    ctx: &AnalysisContext,
    structure_name: &str,
) {
    for (name, _kind, cell_type) in ctx.member_names_for_structure(structure_name) {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::VARIABLE),
            detail: Some(cell_type.to_string()),
            ..Default::default()
        });
    }
}

fn push_entity_names(items: &mut Vec<CompletionItem>, ctx: &AnalysisContext) {
    for entity in ctx.entity_names() {
        items.push(CompletionItem {
            label: entity.name.to_string(),
            kind: Some(CompletionItemKind::STRUCT),
            ..Default::default()
        });
    }
}

/// Push METHOD-kind completions for the fixed list of complex-number methods.
///
/// This is emitted unconditionally in the DotAccess cursor context. A full
/// type-aware method completion would require running compilation-time type
/// inference inside the LSP on each keystroke — out of scope for this polish
/// pass. If the user dotted into a non-Complex value, they get a few extra
/// suggestions alongside the correct ones.
fn push_complex_methods(items: &mut Vec<CompletionItem>) {
    for name in COMPLEX_METHODS {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::METHOD),
            detail: Some("complex method".to_string()),
            ..Default::default()
        });
    }
}

/// Keywords that are only valid at the top level (outside structure bodies).
pub(crate) const TOP_LEVEL_KEYWORDS: &[&str] =
    &["structure", "occurrence", "import", "fn", "trait", "enum"];

/// Keywords that start declaration lines inside a structure body.
pub(crate) const BODY_KEYWORDS: &[&str] = &[
    "param",
    "let",
    "constraint",
    "sub",
    "auto",
    "purpose",
    "minimize",
    "maximize",
    "port",
    "connect",
    "where",
];

/// Keywords valid inside expressions (conditions, values, operators).
pub(crate) const EXPR_KEYWORDS: &[&str] =
    &["if", "then", "else", "and", "or", "not", "true", "false"];

/// Metadata for a single built-in function exposed in LSP completions.
struct BuiltinFunctionInfo {
    name: &'static str,
    signature: &'static str,
    doc: &'static str,
    /// Category prefix used for sort_text grouping (e.g. "03-trig").
    sort_group: &'static str,
}

/// All built-in functions, organized by category.
///
/// Each entry provides: name, signature (shown in detail), brief doc
/// (shown in documentation popup), and sort_group (for grouping in
/// the completion list).
const BUILTIN_FUNCTIONS: &[BuiltinFunctionInfo] = &[
    // --- 01-geometry: solid geometry primitives ---
    BuiltinFunctionInfo {
        name: "box",
        signature: "box(width: Real, height: Real, depth: Real) -> Solid",
        doc: "Creates a rectangular box solid centred at the origin.",
        sort_group: "01-geometry",
    },
    BuiltinFunctionInfo {
        name: "cylinder",
        signature: "cylinder(radius: Real, height: Real) -> Solid",
        doc: "Creates a cylinder solid along the Z axis.",
        sort_group: "01-geometry",
    },
    BuiltinFunctionInfo {
        name: "sphere",
        signature: "sphere(radius: Real) -> Solid",
        doc: "Creates a sphere solid centred at the origin.",
        sort_group: "01-geometry",
    },
    // --- 02-numeric: numeric / scalar math ---
    BuiltinFunctionInfo {
        name: "abs",
        signature: "abs(x) -> Real",
        doc: "Absolute value of `x`.",
        sort_group: "02-numeric",
    },
    BuiltinFunctionInfo {
        name: "sqrt",
        signature: "sqrt(x: Real) -> Real",
        doc: "Square root of `x`.",
        sort_group: "02-numeric",
    },
    BuiltinFunctionInfo {
        name: "floor",
        signature: "floor(x: Real) -> Int",
        doc: "Largest integer ≤ `x`.",
        sort_group: "02-numeric",
    },
    BuiltinFunctionInfo {
        name: "ceil",
        signature: "ceil(x: Real) -> Int",
        doc: "Smallest integer ≥ `x`.",
        sort_group: "02-numeric",
    },
    BuiltinFunctionInfo {
        name: "round",
        signature: "round(x: Real) -> Int",
        doc: "Round `x` to the nearest integer.",
        sort_group: "02-numeric",
    },
    BuiltinFunctionInfo {
        name: "sign",
        signature: "sign(x: Real) -> Real",
        doc: "Sign of `x`: +1, −1, or 0.",
        sort_group: "02-numeric",
    },
    BuiltinFunctionInfo {
        name: "log",
        signature: "log(x: Real) -> Real",
        doc: "Natural logarithm of `x`.",
        sort_group: "02-numeric",
    },
    BuiltinFunctionInfo {
        name: "log10",
        signature: "log10(x: Real) -> Real",
        doc: "Base-10 logarithm of `x`.",
        sort_group: "02-numeric",
    },
    BuiltinFunctionInfo {
        name: "exp",
        signature: "exp(x: Real) -> Real",
        doc: "Euler's number raised to the power `x` (eˣ).",
        sort_group: "02-numeric",
    },
    BuiltinFunctionInfo {
        name: "min",
        signature: "min(a, b) -> Real",
        doc: "Returns the smaller of `a` and `b`.",
        sort_group: "02-numeric",
    },
    BuiltinFunctionInfo {
        name: "max",
        signature: "max(a, b) -> Real",
        doc: "Returns the larger of `a` and `b`.",
        sort_group: "02-numeric",
    },
    BuiltinFunctionInfo {
        name: "pow",
        signature: "pow(base: Real, exp: Real) -> Real",
        doc: "Raises `base` to the power `exp`.",
        sort_group: "02-numeric",
    },
    BuiltinFunctionInfo {
        name: "mod",
        signature: "mod(a, b) -> Real",
        doc: "Remainder of `a` divided by `b`.",
        sort_group: "02-numeric",
    },
    BuiltinFunctionInfo {
        name: "clamp",
        signature: "clamp(x, lo, hi) -> Real",
        doc: "Clamps `x` to the range [`lo`, `hi`].",
        sort_group: "02-numeric",
    },
    BuiltinFunctionInfo {
        name: "lerp",
        signature: "lerp(a, b, t: Real) -> Real",
        doc: "Linearly interpolates from `a` to `b` by factor `t`.",
        sort_group: "02-numeric",
    },
    BuiltinFunctionInfo {
        name: "remap",
        signature: "remap(x, in_lo, in_hi, out_lo, out_hi) -> Real",
        doc: "Maps `x` from the input range to the output range.",
        sort_group: "02-numeric",
    },
    // --- 03-trig: trigonometric functions ---
    BuiltinFunctionInfo {
        name: "sin",
        signature: "sin(angle: Angle) -> Real",
        doc: "Sine of `angle`.",
        sort_group: "03-trig",
    },
    BuiltinFunctionInfo {
        name: "cos",
        signature: "cos(angle: Angle) -> Real",
        doc: "Cosine of `angle`.",
        sort_group: "03-trig",
    },
    BuiltinFunctionInfo {
        name: "tan",
        signature: "tan(angle: Angle) -> Real",
        doc: "Tangent of `angle`.",
        sort_group: "03-trig",
    },
    BuiltinFunctionInfo {
        name: "asin",
        signature: "asin(x: Real) -> Angle",
        doc: "Arc-sine of `x`; returns an angle.",
        sort_group: "03-trig",
    },
    BuiltinFunctionInfo {
        name: "acos",
        signature: "acos(x: Real) -> Angle",
        doc: "Arc-cosine of `x`; returns an angle.",
        sort_group: "03-trig",
    },
    BuiltinFunctionInfo {
        name: "atan",
        signature: "atan(x: Real) -> Angle",
        doc: "Arc-tangent of `x`; returns an angle.",
        sort_group: "03-trig",
    },
    BuiltinFunctionInfo {
        name: "atan2",
        signature: "atan2(y: Real, x: Real) -> Angle",
        doc: "Two-argument arc-tangent; returns the angle of the vector (`x`, `y`).",
        sort_group: "03-trig",
    },
    BuiltinFunctionInfo {
        name: "sinh",
        signature: "sinh(x: Real) -> Real",
        doc: "Hyperbolic sine of `x`.",
        sort_group: "03-trig",
    },
    BuiltinFunctionInfo {
        name: "cosh",
        signature: "cosh(x: Real) -> Real",
        doc: "Hyperbolic cosine of `x`.",
        sort_group: "03-trig",
    },
    BuiltinFunctionInfo {
        name: "tanh",
        signature: "tanh(x: Real) -> Real",
        doc: "Hyperbolic tangent of `x`.",
        sort_group: "03-trig",
    },
    // --- 04-linalg: linear algebra ---
    BuiltinFunctionInfo {
        name: "dot",
        signature: "dot(a: Vector, b: Vector) -> Real",
        doc: "Dot product of vectors `a` and `b`.",
        sort_group: "04-linalg",
    },
    BuiltinFunctionInfo {
        name: "cross",
        signature: "cross(a: Vector, b: Vector) -> Vector",
        doc: "Cross product of 3D vectors `a` and `b`.",
        sort_group: "04-linalg",
    },
    BuiltinFunctionInfo {
        name: "normalize",
        signature: "normalize(v: Vector) -> Vector",
        doc: "Returns a unit vector in the direction of `v`.",
        sort_group: "04-linalg",
    },
    BuiltinFunctionInfo {
        name: "magnitude",
        signature: "magnitude(v: Vector) -> Real",
        doc: "Euclidean length of vector `v`.",
        sort_group: "04-linalg",
    },
    BuiltinFunctionInfo {
        name: "determinant",
        signature: "determinant(m: Matrix) -> Real",
        doc: "Determinant of square matrix `m`.",
        sort_group: "04-linalg",
    },
    BuiltinFunctionInfo {
        name: "inverse",
        signature: "inverse(m: Matrix) -> Matrix",
        doc: "Inverse of square matrix `m`.",
        sort_group: "04-linalg",
    },
    BuiltinFunctionInfo {
        name: "transpose",
        signature: "transpose(m: Matrix) -> Matrix",
        doc: "Transpose of matrix `m`.",
        sort_group: "04-linalg",
    },
    BuiltinFunctionInfo {
        name: "outer",
        signature: "outer(a: Vector, b: Vector) -> Matrix",
        doc: "Outer (tensor) product of vectors `a` and `b`.",
        sort_group: "04-linalg",
    },
    BuiltinFunctionInfo {
        name: "trace",
        signature: "trace(m: Matrix) -> Real",
        doc: "Trace (sum of diagonal elements) of matrix `m`.",
        sort_group: "04-linalg",
    },
    BuiltinFunctionInfo {
        name: "eigenvalues",
        signature: "eigenvalues(m: Matrix) -> Vector",
        doc: "Eigenvalues of symmetric matrix `m`.",
        sort_group: "04-linalg",
    },
    // --- 05-complex: complex number operations ---
    BuiltinFunctionInfo {
        name: "complex",
        signature: "complex(re: Real, im: Real) -> Complex",
        doc: "Constructs a complex number with real part `re` and imaginary part `im`.",
        sort_group: "05-complex",
    },
    BuiltinFunctionInfo {
        name: "conjugate",
        signature: "conjugate(z: Complex) -> Complex",
        doc: "Complex conjugate of `z`.",
        sort_group: "05-complex",
    },
    BuiltinFunctionInfo {
        name: "phase",
        signature: "phase(z: Complex) -> Angle",
        doc: "Phase angle of complex number `z`.",
        sort_group: "05-complex",
    },
    BuiltinFunctionInfo {
        name: "complex_magnitude",
        signature: "complex_magnitude(z: Complex) -> Real",
        doc: "Magnitude (absolute value) of complex number `z`.",
        sort_group: "05-complex",
    },
    BuiltinFunctionInfo {
        name: "complex_add",
        signature: "complex_add(a: Complex, b: Complex) -> Complex",
        doc: "Sum of complex numbers `a` and `b`.",
        sort_group: "05-complex",
    },
    BuiltinFunctionInfo {
        name: "complex_mul",
        signature: "complex_mul(a: Complex, b: Complex) -> Complex",
        doc: "Product of complex numbers `a` and `b`.",
        sort_group: "05-complex",
    },
    BuiltinFunctionInfo {
        name: "complex_div",
        signature: "complex_div(a: Complex, b: Complex) -> Complex",
        doc: "Quotient of complex numbers `a` and `b`.",
        sort_group: "05-complex",
    },
    // --- 06-constructors: geometry value constructors ---
    BuiltinFunctionInfo {
        name: "point2",
        signature: "point2(x: Real, y: Real) -> Point",
        doc: "Constructs a 2D point.",
        sort_group: "06-constructors",
    },
    BuiltinFunctionInfo {
        name: "point3",
        signature: "point3(x: Real, y: Real, z: Real) -> Point",
        doc: "Constructs a 3D point.",
        sort_group: "06-constructors",
    },
    BuiltinFunctionInfo {
        name: "vec2",
        signature: "vec2(x: Real, y: Real) -> Vector",
        doc: "Constructs a 2D vector.",
        sort_group: "06-constructors",
    },
    BuiltinFunctionInfo {
        name: "vec3",
        signature: "vec3(x: Real, y: Real, z: Real) -> Vector",
        doc: "Constructs a 3D vector.",
        sort_group: "06-constructors",
    },
    BuiltinFunctionInfo {
        name: "frame3",
        signature: "frame3(origin: Point, basis: Orientation) -> Frame",
        doc: "Constructs a 3D coordinate frame from an origin and orientation.",
        sort_group: "06-constructors",
    },
    BuiltinFunctionInfo {
        name: "frame3_identity",
        signature: "frame3_identity() -> Frame",
        doc: "Returns the identity 3D frame (origin at zero, standard orientation).",
        sort_group: "06-constructors",
    },
    BuiltinFunctionInfo {
        name: "transform3",
        signature: "transform3(rotation: Orientation, translation: Vector) -> Transform",
        doc: "Constructs a 3D rigid-body transform from a rotation and translation.",
        sort_group: "06-constructors",
    },
    BuiltinFunctionInfo {
        name: "transform3_identity",
        signature: "transform3_identity() -> Transform",
        doc: "Returns the identity 3D transform (no rotation, no translation).",
        sort_group: "06-constructors",
    },
    // --- 07-orientation: orientation constructors ---
    BuiltinFunctionInfo {
        name: "orient_identity",
        signature: "orient_identity() -> Orientation",
        doc: "Returns the identity orientation (no rotation).",
        sort_group: "07-orientation",
    },
    BuiltinFunctionInfo {
        name: "orient_quaternion",
        signature: "orient_quaternion(w: Real, x: Real, y: Real, z: Real) -> Orientation",
        doc: "Constructs an orientation from a unit quaternion (w, x, y, z).",
        sort_group: "07-orientation",
    },
    BuiltinFunctionInfo {
        name: "orient_euler",
        signature: "orient_euler(a1: Angle, a2: Angle, a3: Angle, order: String) -> Orientation",
        doc: "Constructs an orientation from Euler angles and a rotation order string (e.g. `\"xyz\"`).",
        sort_group: "07-orientation",
    },
    BuiltinFunctionInfo {
        name: "orient_basis",
        signature: "orient_basis(x_axis: Vector, y_axis: Vector, z_axis: Vector) -> Orientation",
        doc: "Constructs an orientation from three orthonormal basis vectors.",
        sort_group: "07-orientation",
    },
    BuiltinFunctionInfo {
        name: "orient_axis_angle",
        signature: "orient_axis_angle(axis: Vector, angle: Angle) -> Orientation",
        doc: "Constructs an orientation from an axis vector and a rotation angle.",
        sort_group: "07-orientation",
    },
    // --- 08-coordinate: coordinate frames, planes, axes ---
    BuiltinFunctionInfo {
        name: "frame_to_frame",
        signature: "frame_to_frame(from: Frame, to: Frame) -> Transform",
        doc: "Computes the transform that maps `from` frame to `to` frame.",
        sort_group: "08-coordinate",
    },
    BuiltinFunctionInfo {
        name: "plane_xy",
        signature: "plane_xy() -> Frame",
        doc: "Returns the XY plane as a coordinate frame.",
        sort_group: "08-coordinate",
    },
    BuiltinFunctionInfo {
        name: "plane_xz",
        signature: "plane_xz() -> Frame",
        doc: "Returns the XZ plane as a coordinate frame.",
        sort_group: "08-coordinate",
    },
    BuiltinFunctionInfo {
        name: "plane_yz",
        signature: "plane_yz() -> Frame",
        doc: "Returns the YZ plane as a coordinate frame.",
        sort_group: "08-coordinate",
    },
    BuiltinFunctionInfo {
        name: "axis_x",
        signature: "axis_x() -> Vector",
        doc: "Returns the unit X axis vector.",
        sort_group: "08-coordinate",
    },
    BuiltinFunctionInfo {
        name: "axis_y",
        signature: "axis_y() -> Vector",
        doc: "Returns the unit Y axis vector.",
        sort_group: "08-coordinate",
    },
    BuiltinFunctionInfo {
        name: "axis_z",
        signature: "axis_z() -> Vector",
        doc: "Returns the unit Z axis vector.",
        sort_group: "08-coordinate",
    },
    // --- 09-bbox: bounding box ---
    BuiltinFunctionInfo {
        name: "bbox",
        signature: "bbox(solid) -> BoundingBox",
        doc: "Returns the axis-aligned bounding box of a solid.",
        sort_group: "09-bbox",
    },
    BuiltinFunctionInfo {
        name: "bbox_size",
        signature: "bbox_size(bb: BoundingBox) -> Vector",
        doc: "Returns the size (width × height × depth) of a bounding box.",
        sort_group: "09-bbox",
    },
    BuiltinFunctionInfo {
        name: "bbox_center",
        signature: "bbox_center(bb: BoundingBox) -> Point",
        doc: "Returns the centre point of a bounding box.",
        sort_group: "09-bbox",
    },
    // --- 10-field: field operations (not yet fully implemented) ---
    BuiltinFunctionInfo {
        name: "sample",
        signature: "sample(field, point: Point) -> Real",
        doc: "Evaluates a scalar field at the given point. *(Not yet implemented.)*",
        sort_group: "10-field",
    },
    BuiltinFunctionInfo {
        name: "gradient",
        signature: "gradient(field, point: Point) -> Vector",
        doc: "Gradient of a scalar field at the given point. *(Not yet implemented.)*",
        sort_group: "10-field",
    },
    BuiltinFunctionInfo {
        name: "divergence",
        signature: "divergence(field, point: Point) -> Real",
        doc: "Divergence of a vector field at the given point. *(Not yet implemented.)*",
        sort_group: "10-field",
    },
    BuiltinFunctionInfo {
        name: "curl",
        signature: "curl(field, point: Point) -> Vector",
        doc: "Curl of a vector field at the given point. *(Not yet implemented.)*",
        sort_group: "10-field",
    },
    // --- 11-determinacy: constraint-system query functions ---
    BuiltinFunctionInfo {
        name: "determined",
        signature: "determined(x) -> Bool",
        doc: "Returns `true` if the value `x` is fully determined by the constraint system.",
        sort_group: "11-determinacy",
    },
    BuiltinFunctionInfo {
        name: "undetermined",
        signature: "undetermined(x) -> Bool",
        doc: "Returns `true` if the value `x` is not yet determined.",
        sort_group: "11-determinacy",
    },
    BuiltinFunctionInfo {
        name: "constrained",
        signature: "constrained(x) -> Bool",
        doc: "Returns `true` if the value `x` has at least one active constraint.",
        sort_group: "11-determinacy",
    },
    BuiltinFunctionInfo {
        name: "partially_determined",
        signature: "partially_determined(x) -> Bool",
        doc: "Returns `true` if the value `x` is partially — but not fully — determined.",
        sort_group: "11-determinacy",
    },
];

/// Built-in type names.
const TYPE_NAMES: &[&str] = &["Scalar", "Bool", "Int", "Real", "String"];

/// Method names offered in `DotAccess` completions as METHOD-kind items.
///
/// These are the methods defined on `Value::Complex` (see
/// `reify-expr/src/complex.rs::eval_complex_method` and
/// `reify-stdlib/src/complex.rs`). Emission is unconditional in DotAccess
/// context — a future type-aware filter could restrict to actual Complex
/// receivers.
const COMPLEX_METHODS: &[&str] = &["re", "im", "magnitude", "phase", "conjugate"];

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::{CompletionItemKind, Url};

    fn test_uri() -> Url {
        Url::parse("file:///test.ri").unwrap()
    }

    const GUARDED_GROUP_SOURCE: &str = "structure S {\n    param cond : Bool = true\n    where cond {\n        param guarded_x : Scalar = 5mm\n    }\n}";

    // --- step-9: completion tests ---

    #[test]
    fn completions_include_keywords() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let keywords: Vec<_> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
            .collect();
        // Should include at least: structure, param, let, constraint, sub, import,
        // if, then, else, and, or, not, true, false, auto
        assert!(
            keywords.len() >= 12,
            "expected at least 12 keywords, got {}",
            keywords.len()
        );
        let keyword_labels: Vec<&str> = keywords.iter().map(|k| k.label.as_str()).collect();
        assert!(keyword_labels.contains(&"param"), "should include 'param'");
        assert!(keyword_labels.contains(&"let"), "should include 'let'");
        assert!(
            keyword_labels.contains(&"constraint"),
            "should include 'constraint'"
        );
        // Note: Position(1,0) is inside the structure body, so 'structure'
        // (a top-level keyword) is not expected here after position-aware narrowing.
    }

    #[test]
    fn completions_include_scope_identifiers() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(7, 17));
        let variables: Vec<_> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
            .collect();
        let var_labels: Vec<&str> = variables.iter().map(|v| v.label.as_str()).collect();
        // Should include all value cells: width, height, thickness,
        // fillet_radius, hole_diameter, volume (and possibly body)
        assert!(
            variables.len() >= 6,
            "expected at least 6 scope variables, got {}",
            variables.len()
        );
        assert!(var_labels.contains(&"width"), "should include 'width'");
        assert!(var_labels.contains(&"height"), "should include 'height'");
        assert!(
            var_labels.contains(&"thickness"),
            "should include 'thickness'"
        );
        assert!(var_labels.contains(&"volume"), "should include 'volume'");
        // Variables should have type detail
        let width_item = variables.iter().find(|v| v.label == "width").unwrap();
        assert!(width_item.detail.is_some(), "width should have type detail");
        assert!(
            width_item.detail.as_ref().unwrap().contains("Scalar"),
            "width detail should mention Scalar"
        );
    }

    #[test]
    fn completions_include_structure_names() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let structs: Vec<_> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::STRUCT))
            .collect();
        assert!(
            structs.iter().any(|s| s.label == "Bracket"),
            "should include 'Bracket' struct"
        );
    }

    #[test]
    fn completions_include_builtin_functions() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let functions: Vec<_> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .collect();
        let func_labels: Vec<&str> = functions.iter().map(|f| f.label.as_str()).collect();
        // Should include built-in geometry/math functions
        assert!(func_labels.contains(&"box"), "should include 'box'");
        assert!(func_labels.contains(&"sin"), "should include 'sin'");
        assert!(func_labels.contains(&"cos"), "should include 'cos'");
        assert!(func_labels.contains(&"sqrt"), "should include 'sqrt'");
        assert!(func_labels.contains(&"abs"), "should include 'abs'");
        assert!(func_labels.contains(&"min"), "should include 'min'");
        assert!(func_labels.contains(&"max"), "should include 'max'");
    }

    #[test]
    fn re_im_not_in_builtin_completions() {
        // re, im, real, imag are method-only accessors, not standalone builtins.
        // They should NOT appear in function completions.
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let func_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|f| f.label.as_str())
            .collect();
        assert!(
            !func_labels.contains(&"re"),
            "'re' should not be in builtin function completions"
        );
        assert!(
            !func_labels.contains(&"im"),
            "'im' should not be in builtin function completions"
        );
        assert!(
            !func_labels.contains(&"real"),
            "'real' should not be in builtin function completions"
        );
        assert!(
            !func_labels.contains(&"imag"),
            "'imag' should not be in builtin function completions"
        );
    }

    #[test]
    fn complex_methods_appear_as_method_kind_on_dot_access() {
        // When the cursor is in a DotAccess context (immediately after `.`),
        // the complex-number methods re, im, magnitude, phase, conjugate must
        // be offered as METHOD-kind completions. This is the positive counterpart
        // to `re_im_not_in_builtin_completions` (which asserts they are NOT
        // offered as FUNCTION completions at non-dot positions).
        let source = "structure S {\n    let z = complex(1.0, 2.0)\n    constraint z.\n}";
        // Line 2, col 17: just after the "." in "    constraint z."
        let items = compute_completions(source, &test_uri(), Position::new(2, 17));

        let method_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
            .map(|m| m.label.as_str())
            .collect();

        for expected in &["re", "im", "magnitude", "phase", "conjugate"] {
            assert!(
                method_labels.contains(expected),
                "expected '{}' as METHOD-kind completion in DotAccess context, got {:?}",
                expected,
                method_labels
            );
        }
    }

    #[test]
    fn completions_include_type_names() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let types: Vec<_> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::CLASS))
            .collect();
        let type_labels: Vec<&str> = types.iter().map(|t| t.label.as_str()).collect();
        assert!(type_labels.contains(&"Scalar"), "should include 'Scalar'");
        assert!(type_labels.contains(&"Bool"), "should include 'Bool'");
        assert!(type_labels.contains(&"Int"), "should include 'Int'");
        assert!(type_labels.contains(&"Real"), "should include 'Real'");
        assert!(type_labels.contains(&"String"), "should include 'String'");
    }

    #[test]
    fn completions_on_empty_source_still_include_keywords_and_builtins() {
        let items = compute_completions("", &test_uri(), Position::new(0, 0));
        let keywords: Vec<_> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
            .collect();
        let functions: Vec<_> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .collect();
        assert!(
            !keywords.is_empty(),
            "empty source should still have keywords"
        );
        assert!(
            !functions.is_empty(),
            "empty source should still have built-in functions"
        );
    }

    #[test]
    fn completions_include_occurrence_names() {
        let source = "occurrence def Joint {\n    param diameter: Scalar = 10mm\n}";
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let structs: Vec<_> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::STRUCT))
            .collect();
        assert!(
            structs.iter().any(|s| s.label == "Joint"),
            "should include 'Joint' occurrence in completions"
        );
    }

    // --- position-sensitive completion tests ---

    #[test]
    fn completion_top_level_excludes_body_keywords() {
        // Source: one structure, then a blank line. Cursor is outside any structure.
        let source = "structure Foo {\n    param x: Scalar = 1mm\n}\n";
        let items = compute_completions(source, &test_uri(), Position::new(3, 0));

        let keyword_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
            .map(|k| k.label.as_str())
            .collect();

        // At top level, structure-defining and import keywords should be present
        assert!(
            keyword_labels.contains(&"structure"),
            "top-level should include 'structure'"
        );
        assert!(
            keyword_labels.contains(&"import"),
            "top-level should include 'import'"
        );

        // Body-only keywords should NOT be present at top level
        // (Future keywords like fn, trait, enum would also be asserted here once added to KEYWORDS)
        assert!(
            !keyword_labels.contains(&"param"),
            "top-level should NOT include 'param'"
        );
        assert!(
            !keyword_labels.contains(&"let"),
            "top-level should NOT include 'let'"
        );
        assert!(
            !keyword_labels.contains(&"constraint"),
            "top-level should NOT include 'constraint'"
        );
        assert!(
            !keyword_labels.contains(&"sub"),
            "top-level should NOT include 'sub'"
        );
    }

    #[test]
    fn completion_inside_body_excludes_top_level_keywords() {
        let source = reify_test_support::bracket_source();
        // Line 6 is the empty line between params and lets, inside body (col 0 since line is empty)
        let items = compute_completions(source, &test_uri(), Position::new(6, 0));

        let keyword_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
            .map(|k| k.label.as_str())
            .collect();

        // Inside a structure body, declaration keywords should be present
        assert!(
            keyword_labels.contains(&"param"),
            "body should include 'param'"
        );
        assert!(keyword_labels.contains(&"let"), "body should include 'let'");
        assert!(
            keyword_labels.contains(&"constraint"),
            "body should include 'constraint'"
        );
        assert!(keyword_labels.contains(&"sub"), "body should include 'sub'");

        // Top-level-only keywords should NOT appear inside a body
        assert!(
            !keyword_labels.contains(&"structure"),
            "body should NOT include 'structure'"
        );
        assert!(
            !keyword_labels.contains(&"import"),
            "body should NOT include 'import'"
        );
    }

    #[test]
    fn completion_expression_excludes_declaration_keywords() {
        // Cursor is in an expression position (after `= `)
        let source = "structure Foo {\n    let x = \n}";
        // Line 1, col 12 is after "    let x = " — inside the expression
        let items = compute_completions(source, &test_uri(), Position::new(1, 12));

        let func_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|f| f.label.as_str())
            .collect();

        let keyword_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
            .map(|k| k.label.as_str())
            .collect();

        // In an expression, builtin functions should be available
        assert!(
            func_labels.contains(&"sin"),
            "expression should include 'sin'"
        );
        assert!(
            func_labels.contains(&"cos"),
            "expression should include 'cos'"
        );

        // Declaration keywords should NOT appear in expression context
        assert!(
            !keyword_labels.contains(&"param"),
            "expression should NOT include 'param'"
        );
        assert!(
            !keyword_labels.contains(&"let"),
            "expression should NOT include 'let'"
        );
        assert!(
            !keyword_labels.contains(&"constraint"),
            "expression should NOT include 'constraint'"
        );
        assert!(
            !keyword_labels.contains(&"structure"),
            "expression should NOT include 'structure'"
        );
    }

    #[test]
    fn completion_after_dot_returns_only_members() {
        // Cursor is after a dot — should only return member completions
        // Note: Bar is undefined, but the exclusion assertions are what matter
        let source = "structure Foo {\n    param a: Scalar = 1mm\n    param b: Scalar = 2mm\n    sub part: Bar\n    let x = part.\n}";
        // Line 4, col 17 is after the dot on "    let x = part."
        let items = compute_completions(source, &test_uri(), Position::new(4, 17));

        let keyword_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
            .map(|k| k.label.as_str())
            .collect();

        let func_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|f| f.label.as_str())
            .collect();

        let type_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::CLASS))
            .map(|t| t.label.as_str())
            .collect();

        // After a dot, no keywords should appear
        assert!(
            keyword_labels.is_empty(),
            "after dot should have no keywords, got: {:?}",
            keyword_labels
        );
        // After a dot, no builtin functions should appear
        assert!(
            func_labels.is_empty(),
            "after dot should have no builtin functions, got: {:?}",
            func_labels
        );
        // After a dot, no type names should appear
        assert!(
            type_labels.is_empty(),
            "after dot should have no type names, got: {:?}",
            type_labels
        );
        // Ideally this would also assert that Bar's members are returned,
        // but Bar is undefined so we can only check exclusions here.
    }

    #[test]
    fn completion_after_dot_defined_struct_returns_only_members() {
        // Foo is defined with params a and b.
        // Bar references Foo via `sub part: Foo`, then `let x = part.` triggers dot-access.
        // This test has both positive (a, b present) AND negative (no keywords/functions/types)
        // assertions, addressing the vacuous_test finding in the exclusion-only test above.
        let source = "structure Foo {\n    param a: Scalar = 1mm\n    param b: Scalar = 2mm\n}\nstructure Bar {\n    sub part: Foo\n    let x = part.\n}";
        // Line 6, col 17 is after the dot on "    let x = part."
        let items = compute_completions(source, &test_uri(), Position::new(6, 17));

        let var_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
            .map(|v| v.label.as_str())
            .collect();

        let keyword_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
            .map(|k| k.label.as_str())
            .collect();

        let func_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|f| f.label.as_str())
            .collect();

        let type_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::CLASS))
            .map(|t| t.label.as_str())
            .collect();

        // Positive: Foo's members should appear
        assert!(
            !var_labels.is_empty(),
            "after dot with defined struct should have member completions"
        );
        assert!(
            var_labels.contains(&"a"),
            "should include Foo's 'a', got: {:?}",
            var_labels
        );
        assert!(
            var_labels.contains(&"b"),
            "should include Foo's 'b', got: {:?}",
            var_labels
        );

        // Negative: no keywords, functions, or types after dot
        assert!(
            keyword_labels.is_empty(),
            "after dot should have no keywords, got: {:?}",
            keyword_labels
        );
        assert!(
            func_labels.is_empty(),
            "after dot should have no builtin functions, got: {:?}",
            func_labels
        );
        assert!(
            type_labels.is_empty(),
            "after dot should have no type names, got: {:?}",
            type_labels
        );
    }

    #[test]
    fn completion_after_dot_includes_known_members() {
        // Two defined structures so push_all_members returns real members.
        let source = "structure Bracket {\n    param width: Scalar = 80mm\n    param height: Scalar = 100mm\n}\nstructure Assembly {\n    sub part: Bracket\n    let x = part.\n}";
        // Line 6, col 17 is after the dot on "    let x = part."
        let items = compute_completions(source, &test_uri(), Position::new(6, 17));

        let var_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
            .map(|v| v.label.as_str())
            .collect();

        // DotAccess context calls push_all_members, which returns members from
        // all defined structures. Bracket's params should appear.
        assert!(
            !var_labels.is_empty(),
            "after dot with defined structures should have member completions"
        );
        assert!(
            var_labels.contains(&"width"),
            "should include Bracket's 'width', got: {:?}",
            var_labels
        );
        assert!(
            var_labels.contains(&"height"),
            "should include Bracket's 'height', got: {:?}",
            var_labels
        );
    }

    #[test]
    fn completion_type_position_returns_types_and_structs() {
        // Cursor is in a type annotation position (after `x: `)
        let source = "structure Foo {\n    param x: \n}";
        // Line 1, col 13 is after "    param x: " — in type position
        let items = compute_completions(source, &test_uri(), Position::new(1, 13));

        let type_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::CLASS))
            .map(|t| t.label.as_str())
            .collect();

        let struct_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::STRUCT))
            .map(|s| s.label.as_str())
            .collect();

        let keyword_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
            .map(|k| k.label.as_str())
            .collect();

        let func_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|f| f.label.as_str())
            .collect();

        // In type position, type names should be present
        assert!(
            type_labels.contains(&"Scalar"),
            "type position should include 'Scalar'"
        );
        assert!(
            type_labels.contains(&"Bool"),
            "type position should include 'Bool'"
        );
        assert!(
            type_labels.contains(&"Int"),
            "type position should include 'Int'"
        );
        assert!(
            type_labels.contains(&"Real"),
            "type position should include 'Real'"
        );
        assert!(
            type_labels.contains(&"String"),
            "type position should include 'String'"
        );

        // Structure names should be available as types
        assert!(
            struct_labels.contains(&"Foo"),
            "type position should include struct 'Foo'"
        );

        // Keywords should NOT appear in type position
        assert!(
            !keyword_labels.contains(&"param"),
            "type position should NOT include 'param'"
        );
        assert!(
            !keyword_labels.contains(&"let"),
            "type position should NOT include 'let'"
        );
        assert!(
            !keyword_labels.contains(&"structure"),
            "type position should NOT include 'structure'"
        );

        // Builtin functions should NOT appear in type position
        assert!(
            !func_labels.contains(&"sin"),
            "type position should NOT include 'sin'"
        );
        assert!(
            !func_labels.contains(&"box"),
            "type position should NOT include 'box'"
        );
    }

    #[test]
    fn completion_constraint_expr_excludes_declaration_keywords() {
        let source = reify_test_support::bracket_source();
        // Line 9: "    constraint thickness > 2mm" — col 27 is inside the expression
        let items = compute_completions(source, &test_uri(), Position::new(9, 27));

        let var_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
            .map(|v| v.label.as_str())
            .collect();

        let func_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|f| f.label.as_str())
            .collect();

        let keyword_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
            .map(|k| k.label.as_str())
            .collect();

        // In a constraint expression, member variables should be available
        assert!(
            var_labels.contains(&"width"),
            "constraint expr should include 'width'"
        );
        assert!(
            var_labels.contains(&"height"),
            "constraint expr should include 'height'"
        );

        // Builtin functions should be available in expressions
        assert!(
            func_labels.contains(&"sin"),
            "constraint expr should include 'sin'"
        );
        assert!(
            func_labels.contains(&"abs"),
            "constraint expr should include 'abs'"
        );

        // Declaration keywords should NOT appear inside a constraint expression
        assert!(
            !keyword_labels.contains(&"param"),
            "constraint expr should NOT include 'param'"
        );
        assert!(
            !keyword_labels.contains(&"let"),
            "constraint expr should NOT include 'let'"
        );
        assert!(
            !keyword_labels.contains(&"constraint"),
            "constraint expr should NOT include 'constraint'"
        );
        assert!(
            !keyword_labels.contains(&"structure"),
            "constraint expr should NOT include 'structure'"
        );
    }

    // --- determine_context unit tests ---

    #[test]
    fn determine_context_top_level_outside_structure() {
        // Cursor on line 3 (after the closing brace) is outside any structure.
        let source = "structure Foo {\n    param x: Scalar = 1mm\n}\n";
        let ctx = AnalysisContext::new(source, &test_uri());
        let result = determine_context(source, Position::new(3, 0), &ctx);
        assert!(
            matches!(result, CursorContext::TopLevel),
            "expected TopLevel, got {:?}",
            result
        );
    }

    #[test]
    fn determine_context_structure_body_blank_line() {
        // Cursor inside bracket source on the empty line 6 (col 0 since line is empty).
        let source = reify_test_support::bracket_source();
        let ctx = AnalysisContext::new(source, &test_uri());
        let result = determine_context(source, Position::new(6, 0), &ctx);
        assert!(
            matches!(result, CursorContext::StructureBody { .. }),
            "expected StructureBody, got {:?}",
            result
        );
    }

    #[test]
    fn determine_context_expression_after_equals() {
        // "let x = " — cursor after '=' on a let line
        let source = "structure Foo {\n    let x = \n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let result = determine_context(source, Position::new(1, 12), &ctx);
        assert!(
            matches!(result, CursorContext::Expression { .. }),
            "expected Expression after '=', got {:?}",
            result
        );
    }

    #[test]
    fn determine_context_expression_in_constraint() {
        // "constraint thickness > 2mm" — cursor inside the expression
        let source = reify_test_support::bracket_source();
        let ctx = AnalysisContext::new(source, &test_uri());
        let result = determine_context(source, Position::new(9, 27), &ctx);
        assert!(
            matches!(result, CursorContext::Expression { .. }),
            "expected Expression in constraint, got {:?}",
            result
        );
    }

    #[test]
    fn determine_context_expression_param_default() {
        // "param x: Scalar = " — cursor after '=' in a param default
        let source = "structure Foo {\n    param x: Scalar = \n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let result = determine_context(source, Position::new(1, 23), &ctx);
        assert!(
            matches!(result, CursorContext::Expression { .. }),
            "expected Expression after param default '=', got {:?}",
            result
        );
    }

    #[test]
    fn determine_context_dot_access_after_dot() {
        // "let x = part." — cursor immediately after the dot
        let source =
            "structure Foo {\n    param a: Scalar = 1mm\n    sub part: Bar\n    let x = part.\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let result = determine_context(source, Position::new(3, 18), &ctx);
        assert!(
            matches!(result, CursorContext::DotAccess),
            "expected DotAccess after '.', got {:?}",
            result
        );
    }

    #[test]
    fn determine_context_dot_access_with_trailing_space() {
        // "let x = part. " — cursor after dot + space
        let source =
            "structure Foo {\n    param a: Scalar = 1mm\n    sub part: Bar\n    let x = part. \n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let result = determine_context(source, Position::new(3, 19), &ctx);
        assert!(
            matches!(result, CursorContext::DotAccess),
            "expected DotAccess after '. ', got {:?}",
            result
        );
    }

    #[test]
    fn determine_context_type_position_after_colon_in_param() {
        // "param x: " — cursor after ': ' in a param declaration
        let source = "structure Foo {\n    param x: \n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let result = determine_context(source, Position::new(1, 13), &ctx);
        assert!(
            matches!(result, CursorContext::TypePosition),
            "expected TypePosition after ':' in param, got {:?}",
            result
        );
    }

    #[test]
    fn determine_context_type_position_after_colon_in_let() {
        // "let x: " — cursor after ': ' in a let with type annotation
        let source = "structure Foo {\n    let x: Int = 5\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        // Position right after "let x: " — col 11 = after "    let x: "
        let result = determine_context(source, Position::new(1, 11), &ctx);
        assert!(
            matches!(result, CursorContext::TypePosition),
            "expected TypePosition after ':' in let, got {:?}",
            result
        );
    }

    #[test]
    fn determine_context_empty_source_is_top_level() {
        let source = "";
        let ctx = AnalysisContext::new(source, &test_uri());
        let result = determine_context(source, Position::new(0, 0), &ctx);
        assert!(
            matches!(result, CursorContext::TopLevel),
            "expected TopLevel for empty source, got {:?}",
            result
        );
    }

    /// Spot-check that one representative (source, position) pair per `CursorContext`
    /// variant resolves to the expected context. Intentionally covers every variant of
    /// the enum — including `DotAccess`, `TypePosition`, and `TopLevel` which also have
    /// dedicated per-variant `determine_context_*` tests — so that this table acts as a
    /// single compact regression guard for all branches simultaneously. The overlap with
    /// per-variant tests is deliberate: it ensures no future refactor silently breaks one
    /// branch without this table catching it.
    #[test]
    fn determine_context_at_sampled_positions() {
        let check =
            |source: &str, pos: Position, label: &str, matcher: fn(&CursorContext) -> bool| {
                let ctx = AnalysisContext::new(source, &test_uri());
                let result = determine_context(source, pos, &ctx);
                assert!(
                    matcher(&result),
                    "expected {} @ {:?}, got {:?}",
                    label,
                    pos,
                    result
                );
            };
        // (a) bracket_source() @ Position(1,0) → StructureBody
        check(
            reify_test_support::bracket_source(),
            Position::new(1, 0),
            "bracket_source/StructureBody",
            |r| matches!(r, CursorContext::StructureBody { .. }),
        );
        // (b) bracket_source() @ Position(7,17) → Expression
        check(
            reify_test_support::bracket_source(),
            Position::new(7, 17),
            "bracket_source/Expression",
            |r| matches!(r, CursorContext::Expression { .. }),
        );
        // (c) occurrence def Joint source @ Position(1,0) → StructureBody
        check(
            "occurrence def Joint {\n    param diameter: Scalar = 10mm\n}",
            Position::new(1, 0),
            "Joint occurrence/StructureBody",
            |r| matches!(r, CursorContext::StructureBody { .. }),
        );
        // (d) guarded-group source @ Position(1,0) → StructureBody
        check(
            GUARDED_GROUP_SOURCE,
            Position::new(1, 0),
            "guarded-group/StructureBody",
            |r| matches!(r, CursorContext::StructureBody { .. }),
        );
        // (e) dot-access source @ Position(3,18) → DotAccess
        check(
            "structure Foo {\n    param a: Scalar = 1mm\n    sub part: Bar\n    let x = part.\n}",
            Position::new(3, 18),
            "dot-access/DotAccess",
            |r| matches!(r, CursorContext::DotAccess),
        );
        // (f) type-position source @ Position(1,13) → TypePosition
        check(
            "structure Foo {\n    param x: \n}",
            Position::new(1, 13),
            "type-position/TypePosition",
            |r| matches!(r, CursorContext::TypePosition),
        );
        // (g) top-level source @ Position(3,0) → TopLevel
        check(
            "structure Foo {\n    param x: Scalar = 1mm\n}\n",
            Position::new(3, 0),
            "top-level/TopLevel",
            |r| matches!(r, CursorContext::TopLevel),
        );
    }

    // --- guarded-group completion tests ---

    #[test]
    fn completions_include_guarded_group_members() {
        let source = GUARDED_GROUP_SOURCE;
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let variables: Vec<_> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
            .collect();
        let var_labels: Vec<&str> = variables.iter().map(|v| v.label.as_str()).collect();
        assert!(
            var_labels.contains(&"cond"),
            "should include top-level param 'cond', got: {var_labels:?}"
        );
        assert!(
            var_labels.contains(&"guarded_x"),
            "should include guarded-group param 'guarded_x', got: {var_labels:?}"
        );
    }

    // --- linalg builtin completions (step-11) ---

    #[test]
    fn completions_include_linalg_functions() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let func_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|f| f.label.as_str())
            .collect();
        assert!(func_labels.contains(&"dot"), "should include 'dot'");
        assert!(func_labels.contains(&"cross"), "should include 'cross'");
        assert!(
            func_labels.contains(&"normalize"),
            "should include 'normalize'"
        );
        assert!(
            func_labels.contains(&"magnitude"),
            "should include 'magnitude'"
        );
    }

    // --- stdlib completions: trig functions (step-1) ---
    #[test]
    fn completions_include_all_trig_functions() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let func_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|f| f.label.as_str())
            .collect();
        assert!(func_labels.contains(&"asin"), "should include 'asin'");
        assert!(func_labels.contains(&"acos"), "should include 'acos'");
        assert!(func_labels.contains(&"atan"), "should include 'atan'");
        assert!(func_labels.contains(&"atan2"), "should include 'atan2'");
        assert!(func_labels.contains(&"sinh"), "should include 'sinh'");
        assert!(func_labels.contains(&"cosh"), "should include 'cosh'");
        assert!(func_labels.contains(&"tanh"), "should include 'tanh'");
    }

    // --- stdlib completions: numeric functions (step-2) ---
    #[test]
    fn completions_include_all_numeric_functions() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let func_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|f| f.label.as_str())
            .collect();
        assert!(func_labels.contains(&"floor"), "should include 'floor'");
        assert!(func_labels.contains(&"ceil"), "should include 'ceil'");
        assert!(func_labels.contains(&"round"), "should include 'round'");
        assert!(func_labels.contains(&"sign"), "should include 'sign'");
        assert!(func_labels.contains(&"log"), "should include 'log'");
        assert!(func_labels.contains(&"log10"), "should include 'log10'");
        assert!(func_labels.contains(&"exp"), "should include 'exp'");
        assert!(func_labels.contains(&"pow"), "should include 'pow'");
        assert!(func_labels.contains(&"mod"), "should include 'mod'");
        assert!(func_labels.contains(&"clamp"), "should include 'clamp'");
        assert!(func_labels.contains(&"lerp"), "should include 'lerp'");
        assert!(func_labels.contains(&"remap"), "should include 'remap'");
    }

    // --- stdlib completions: geometry constructors (step-3) ---
    #[test]
    fn completions_include_geometry_constructors() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let func_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|f| f.label.as_str())
            .collect();
        assert!(func_labels.contains(&"point2"), "should include 'point2'");
        assert!(func_labels.contains(&"point3"), "should include 'point3'");
        assert!(func_labels.contains(&"vec2"), "should include 'vec2'");
        assert!(func_labels.contains(&"vec3"), "should include 'vec3'");
        assert!(func_labels.contains(&"frame3"), "should include 'frame3'");
        assert!(
            func_labels.contains(&"frame3_identity"),
            "should include 'frame3_identity'"
        );
        assert!(
            func_labels.contains(&"transform3"),
            "should include 'transform3'"
        );
        assert!(
            func_labels.contains(&"transform3_identity"),
            "should include 'transform3_identity'"
        );
        assert!(func_labels.contains(&"bbox"), "should include 'bbox'");
        assert!(
            func_labels.contains(&"bbox_size"),
            "should include 'bbox_size'"
        );
        assert!(
            func_labels.contains(&"bbox_center"),
            "should include 'bbox_center'"
        );
    }

    // --- stdlib completions: orientation functions (step-4) ---
    #[test]
    fn completions_include_orientation_functions() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let func_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|f| f.label.as_str())
            .collect();
        assert!(
            func_labels.contains(&"orient_identity"),
            "should include 'orient_identity'"
        );
        assert!(
            func_labels.contains(&"orient_quaternion"),
            "should include 'orient_quaternion'"
        );
        assert!(
            func_labels.contains(&"orient_euler"),
            "should include 'orient_euler'"
        );
        assert!(
            func_labels.contains(&"orient_basis"),
            "should include 'orient_basis'"
        );
        assert!(
            func_labels.contains(&"orient_axis_angle"),
            "should include 'orient_axis_angle'"
        );
    }

    // --- stdlib completions: complex functions (step-5) ---
    #[test]
    fn completions_include_complex_functions() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let func_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|f| f.label.as_str())
            .collect();
        assert!(func_labels.contains(&"complex"), "should include 'complex'");
        assert!(
            func_labels.contains(&"conjugate"),
            "should include 'conjugate'"
        );
        assert!(func_labels.contains(&"phase"), "should include 'phase'");
        assert!(
            func_labels.contains(&"complex_magnitude"),
            "should include 'complex_magnitude'"
        );
        assert!(
            func_labels.contains(&"complex_add"),
            "should include 'complex_add'"
        );
        assert!(
            func_labels.contains(&"complex_mul"),
            "should include 'complex_mul'"
        );
        assert!(
            func_labels.contains(&"complex_div"),
            "should include 'complex_div'"
        );
    }

    // --- stdlib completions: plane/axis functions (step-6) ---
    #[test]
    fn completions_include_plane_axis_functions() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let func_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|f| f.label.as_str())
            .collect();
        assert!(
            func_labels.contains(&"plane_xy"),
            "should include 'plane_xy'"
        );
        assert!(
            func_labels.contains(&"plane_xz"),
            "should include 'plane_xz'"
        );
        assert!(
            func_labels.contains(&"plane_yz"),
            "should include 'plane_yz'"
        );
        assert!(func_labels.contains(&"axis_x"), "should include 'axis_x'");
        assert!(func_labels.contains(&"axis_y"), "should include 'axis_y'");
        assert!(func_labels.contains(&"axis_z"), "should include 'axis_z'");
        assert!(
            func_labels.contains(&"frame_to_frame"),
            "should include 'frame_to_frame'"
        );
    }

    // --- stdlib completions: linalg extended (step-7) ---
    #[test]
    fn completions_include_linalg_extended() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let func_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|f| f.label.as_str())
            .collect();
        assert!(
            func_labels.contains(&"determinant"),
            "should include 'determinant'"
        );
        assert!(func_labels.contains(&"inverse"), "should include 'inverse'");
        assert!(
            func_labels.contains(&"transpose"),
            "should include 'transpose'"
        );
        assert!(func_labels.contains(&"outer"), "should include 'outer'");
        assert!(func_labels.contains(&"trace"), "should include 'trace'");
        assert!(
            func_labels.contains(&"eigenvalues"),
            "should include 'eigenvalues'"
        );
    }

    // --- stdlib completions: field ops and determinacy (step-8) ---
    #[test]
    fn completions_include_field_and_determinacy() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let func_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|f| f.label.as_str())
            .collect();
        assert!(func_labels.contains(&"sample"), "should include 'sample'");
        assert!(
            func_labels.contains(&"gradient"),
            "should include 'gradient'"
        );
        assert!(
            func_labels.contains(&"divergence"),
            "should include 'divergence'"
        );
        assert!(func_labels.contains(&"curl"), "should include 'curl'");
        assert!(
            func_labels.contains(&"determined"),
            "should include 'determined'"
        );
        assert!(
            func_labels.contains(&"undetermined"),
            "should include 'undetermined'"
        );
        assert!(
            func_labels.contains(&"constrained"),
            "should include 'constrained'"
        );
        assert!(
            func_labels.contains(&"partially_determined"),
            "should include 'partially_determined'"
        );
    }

    // --- stdlib completions: signatures in detail field (step-9) ---
    #[test]
    fn builtin_completions_have_signatures() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let functions: Vec<_> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .collect();
        assert!(
            !functions.is_empty(),
            "expected at least one FUNCTION completion"
        );
        for f in &functions {
            assert!(
                f.detail.as_ref().map(|d| !d.is_empty()).unwrap_or(false),
                "FUNCTION completion '{}' should have a non-empty detail (signature), got: {:?}",
                f.label,
                f.detail
            );
        }
    }

    // --- stdlib completions: markdown documentation (step-10) ---
    #[test]
    fn builtin_completions_have_documentation() {
        use tower_lsp::lsp_types::{Documentation, MarkupKind};
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let functions: Vec<_> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .collect();
        assert!(
            !functions.is_empty(),
            "expected at least one FUNCTION completion"
        );
        for f in &functions {
            match &f.documentation {
                Some(Documentation::MarkupContent(mc)) => {
                    assert_eq!(
                        mc.kind,
                        MarkupKind::Markdown,
                        "FUNCTION '{}' documentation should be Markdown",
                        f.label
                    );
                    assert!(
                        !mc.value.is_empty(),
                        "FUNCTION '{}' documentation should be non-empty",
                        f.label
                    );
                }
                other => panic!(
                    "FUNCTION '{}' should have MarkupContent documentation, got: {:?}",
                    f.label, other
                ),
            }
        }
    }

    // --- stdlib completions: sort_text by category (step-11) ---
    #[test]
    fn builtin_completions_have_sort_text_by_category() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let functions: Vec<_> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .collect();
        assert!(
            !functions.is_empty(),
            "expected at least one FUNCTION completion"
        );
        // Every FUNCTION completion must have sort_text set
        for f in &functions {
            assert!(
                f.sort_text.is_some(),
                "FUNCTION '{}' should have sort_text set",
                f.label
            );
        }
        // Trig functions should share the same sort_text prefix
        let trig_names = ["sin", "cos", "tan", "asin", "acos", "atan"];
        let trig_prefixes: Vec<_> = functions
            .iter()
            .filter(|f| trig_names.contains(&f.label.as_str()))
            .filter_map(|f| {
                f.sort_text.as_ref().and_then(|s| {
                    // prefix = everything before the last '-'
                    s.rfind('-').map(|pos| s[..pos].to_string())
                })
            })
            .collect();
        assert!(
            !trig_prefixes.is_empty(),
            "expected trig functions to have sort_text"
        );
        let first_prefix = &trig_prefixes[0];
        assert!(
            trig_prefixes.iter().all(|p| p == first_prefix),
            "all trig functions should share the same sort_text prefix, got: {:?}",
            trig_prefixes
        );
    }
}
