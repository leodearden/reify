use super::*;

/// The complete set of stdlib geometry constructor names recognised by the
/// compiler. This is the **source of truth** for both [`is_geometry_function`]
/// (derived via `.contains(&name)`) and the dispatch coverage test in
/// `crates/reify-compiler/tests/geometry_traits_inference_tests.rs`.
///
/// # Maintenance contract
///
/// When adding a new geometry function name here, you **must** also add an
/// explicit arm for it in `infer_traits_for_function_call` (or its `try_*`
/// companion) in `crates/reify-compiler/src/geometry_traits_inference.rs`.
/// The test `every_geometry_function_name_has_explicit_dispatch_arm` will
/// fail loudly if a name is added here without a matching dispatch arm —
/// turning the previously-silent `_ => InferredTraits::all()` fallback into
/// a compile-time-traceable assertion failure.
///
/// Order matches the original `matches!` in the pre-refactor `is_geometry_function`
/// for diff readability. Case-sensitive: Reify function names are snake_case.
pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[
    "box",
    "cylinder",
    "sphere",
    "linear_pattern",
    "linear_pattern_2d",
    "circular_pattern",
    "mirror",
    "arbitrary_pattern",
    "loft",
    "loft_guided",
    "extrude",
    "revolve",
    "revolve_full",
    "shell",
    "thicken",
    "draft",
    "chamfer",
    "fillet",
    "union",
    "intersection",
    "difference",
    "union_all",
    "intersection_all",
    "sweep",
    "sweep_guided",
    "extrude_symmetric",
    "translate",
    "rotate",
    "scale",
    "rotate_around",
    "line_segment",
    "arc",
    "helix",
    "interp",
    "bezier",
    "nurbs",
    "tube",
    "pipe",
];

pub(crate) fn is_geometry_function(name: &str) -> bool {
    GEOMETRY_FUNCTION_NAMES.contains(&name)
}

// --- Unit conversion ---

/// Convert a unit string and value to an SI-based `Value::Scalar`.
/// Returns `None` if the unit is unrecognized.
pub(crate) fn unit_to_scalar(value: f64, unit: &str) -> Option<(Value, DimensionVector)> {
    match unit {
        "mm" => Some((
            Value::Scalar {
                si_value: value * 0.001,
                dimension: DimensionVector::LENGTH,
            },
            DimensionVector::LENGTH,
        )),
        "cm" => Some((
            Value::Scalar {
                si_value: value * 0.01,
                dimension: DimensionVector::LENGTH,
            },
            DimensionVector::LENGTH,
        )),
        "m" => Some((
            Value::Scalar {
                si_value: value,
                dimension: DimensionVector::LENGTH,
            },
            DimensionVector::LENGTH,
        )),
        "in" => Some((
            Value::Scalar {
                si_value: value * 0.0254,
                dimension: DimensionVector::LENGTH,
            },
            DimensionVector::LENGTH,
        )),
        "deg" => Some((
            Value::Scalar {
                si_value: value * std::f64::consts::PI / 180.0,
                dimension: DimensionVector::ANGLE,
            },
            DimensionVector::ANGLE,
        )),
        "rad" => Some((
            Value::Scalar {
                si_value: value,
                dimension: DimensionVector::ANGLE,
            },
            DimensionVector::ANGLE,
        )),
        "kg" => Some((
            Value::Scalar {
                si_value: value,
                dimension: DimensionVector::MASS,
            },
            DimensionVector::MASS,
        )),
        "g" => Some((
            Value::Scalar {
                si_value: value * 0.001,
                dimension: DimensionVector::MASS,
            },
            DimensionVector::MASS,
        )),
        "s" => Some((
            Value::Scalar {
                si_value: value,
                dimension: DimensionVector::TIME,
            },
            DimensionVector::TIME,
        )),
        _ => None,
    }
}

// --- Unit registry ---

/// Internal unit entry — stored in the registry during compilation.
#[derive(Debug, Clone)]
pub struct UnitEntry {
    pub name: String,
    pub dimension: DimensionVector,
    /// SI conversion factor: si_value = value * factor.
    pub factor: f64,
    /// Additive offset for affine units (e.g., °C→K): si_value = value * factor + offset.
    pub offset: Option<f64>,
    pub is_pub: bool,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
    /// Display path of the module that introduced this unit via prelude seeding,
    /// e.g. "std/units" or "dep". `None` for units declared in the current module.
    pub source_module: Option<String>,
}

impl UnitEntry {
    /// Construct a `UnitEntry` for prelude-seeded units.
    ///
    /// Bakes in `SourceSpan::prelude()` (so `is_prelude()` checks and
    /// diagnostic labels behave correctly) and the originating module's
    /// display path. The six shared fields are copied from `cu`.
    pub fn from_compiled_for_prelude(cu: &CompiledUnit, source_module: String) -> UnitEntry {
        UnitEntry {
            name: cu.name.clone(),
            dimension: cu.dimension,
            factor: cu.factor,
            offset: cu.offset,
            is_pub: cu.is_pub,
            span: SourceSpan::prelude(),
            content_hash: cu.content_hash,
            source_module: Some(source_module),
        }
    }
}

/// Registry mapping unit names to compiled unit entries.
/// Built incrementally during the unit pre-pass so later units can reference earlier ones.
pub struct UnitRegistry {
    entries: HashMap<String, UnitEntry>,
}

impl UnitRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        UnitRegistry {
            entries: HashMap::new(),
        }
    }

    /// Register a unit entry. Returns `Err(entry)` if the name is already registered.
    pub fn register(&mut self, entry: UnitEntry) -> Result<(), Box<UnitEntry>> {
        if self.entries.contains_key(&entry.name) {
            Err(Box::new(entry))
        } else {
            self.entries.insert(entry.name.clone(), entry);
            Ok(())
        }
    }

    /// Seed a prelude unit entry into the registry (overwrite semantics).
    ///
    /// Used to pre-populate the registry with units from prelude modules
    /// before processing module-local declarations. Duplicate prelude entries
    /// resolve by load order (last wins).
    pub fn seed_prelude_unit(&mut self, entry: UnitEntry) {
        self.entries.insert(entry.name.clone(), entry);
    }

    /// Look up a unit by name.
    pub fn lookup(&self, name: &str) -> Option<&UnitEntry> {
        self.entries.get(name)
    }
}

impl Default for UnitRegistry {
    fn default() -> Self {
        UnitRegistry::new()
    }
}

// --- Type alias registry ---

#[cfg(test)]
mod tests {
    use super::*;

    // --- Step 21: Verify new geometry function names are recognized ---

    #[test]
    fn compile_geometry_linear_pattern_recognized() {
        assert!(is_geometry_function("linear_pattern"));
    }

    #[test]
    fn compile_geometry_circular_pattern_recognized() {
        assert!(is_geometry_function("circular_pattern"));
    }

    #[test]
    fn compile_geometry_mirror_recognized() {
        assert!(is_geometry_function("mirror"));
    }

    #[test]
    fn compile_geometry_loft_recognized() {
        assert!(is_geometry_function("loft"));
    }

    #[test]
    fn compile_geometry_shell_recognized() {
        assert!(is_geometry_function("shell"));
    }

    #[test]
    fn compile_geometry_thicken_recognized() {
        assert!(is_geometry_function("thicken"));
    }

    #[test]
    fn compile_geometry_draft_recognized() {
        assert!(is_geometry_function("draft"));
    }

    // --- Boolean function recognition tests (step-1) ---

    #[test]
    fn compile_geometry_union_recognized() {
        assert!(is_geometry_function("union"));
    }

    #[test]
    fn compile_geometry_intersection_recognized() {
        assert!(is_geometry_function("intersection"));
    }

    #[test]
    fn compile_geometry_difference_recognized() {
        assert!(is_geometry_function("difference"));
    }

    #[test]
    fn compile_geometry_union_all_recognized() {
        assert!(is_geometry_function("union_all"));
    }

    #[test]
    fn compile_geometry_intersection_all_recognized() {
        assert!(is_geometry_function("intersection_all"));
    }

    #[test]
    fn compile_geometry_linear_pattern_2d_recognized() {
        assert!(is_geometry_function("linear_pattern_2d"));
    }

    #[test]
    fn compile_geometry_arbitrary_pattern_recognized() {
        assert!(is_geometry_function("arbitrary_pattern"));
    }

    // --- Sweep (pipe) compiler tests (task-310 step-13) ---

    #[test]
    fn is_geometry_function_sweep() {
        assert!(is_geometry_function("sweep"));
    }

    // --- Tube and pipe compound-shape tests (task-324 step-3) ---

    #[test]
    fn is_geometry_function_tube_recognized() {
        assert!(is_geometry_function("tube"));
    }

    #[test]
    fn is_geometry_function_pipe_recognized() {
        assert!(is_geometry_function("pipe"));
    }

    // --- Geometry query helpers (task 2320 step-1) ---
    //
    // Sibling list to `GEOMETRY_FUNCTION_NAMES` for the three monomorphic
    // conformance-query helpers that return `Type::Bool` and dispatch at
    // eval-time via `reify_eval::geometry_ops::try_eval_conformance_query`.

    #[test]
    fn is_geometry_query_helper_recognises_is_watertight() {
        assert!(is_geometry_query_helper("is_watertight"));
    }

    #[test]
    fn is_geometry_query_helper_recognises_is_manifold() {
        assert!(is_geometry_query_helper("is_manifold"));
    }

    #[test]
    fn is_geometry_query_helper_recognises_is_orientable() {
        assert!(is_geometry_query_helper("is_orientable"));
    }

    #[test]
    fn is_geometry_query_helper_rejects_constructor_names() {
        // `box` is a constructor in `GEOMETRY_FUNCTION_NAMES` — it must NOT
        // satisfy the query-helper predicate, otherwise the two lists would
        // overlap and `is_geometry_let` would misclassify the let-binding.
        assert!(!is_geometry_query_helper("box"));
    }

    #[test]
    fn is_geometry_query_helper_rejects_unrelated_names() {
        // `volume` happens not to be a member of either list today; this
        // pins the negative answer so a future addition to the helpers does
        // not silently widen the predicate.
        assert!(!is_geometry_query_helper("volume"));
    }

    #[test]
    fn is_geometry_query_helper_rejects_empty_name() {
        assert!(!is_geometry_query_helper(""));
    }

    #[test]
    fn is_geometry_query_helper_is_case_sensitive() {
        // Reify function names are snake_case; PascalCase variants must not
        // match (mirrors the `GEOMETRY_FUNCTION_NAMES` case-sensitivity
        // contract documented above).
        assert!(!is_geometry_query_helper("IsWatertight"));
    }
}
