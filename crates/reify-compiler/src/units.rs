use super::*;

pub(crate) fn is_geometry_function(name: &str) -> bool {
    matches!(
        name,
        "box"
            | "cylinder"
            | "sphere"
            | "linear_pattern"
            | "linear_pattern_2d"
            | "circular_pattern"
            | "mirror"
            | "arbitrary_pattern"
            | "loft"
            | "loft_guided"
            | "extrude"
            | "revolve"
            | "revolve_full"
            | "shell"
            | "thicken"
            | "draft"
            | "chamfer"
            | "fillet"
            | "union"
            | "intersection"
            | "difference"
            | "union_all"
            | "intersection_all"
            | "sweep"
            | "sweep_guided"
            | "extrude_symmetric"
            | "translate"
            | "rotate"
            | "scale"
            | "rotate_around"
            | "line_segment"
            | "arc"
            | "helix"
            | "interp"
            | "bezier"
            | "nurbs"
            | "tube"
            | "pipe"
    )
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

/// Convert a [`CompiledUnit`] reference into a [`UnitEntry`].
///
/// The six fields shared between the two types are copied directly. The two
/// `UnitEntry`-only fields receive safe placeholder defaults:
///
/// - `span` → [`SourceSpan::empty`]`(0)` — **callers that seed prelude units MUST
///   override this** with [`SourceSpan::prelude()`] so that diagnostic labels and
///   `is_prelude()` checks behave correctly.
/// - `source_module` → `None` — callers that seed prelude units MUST override this
///   with the originating module path.
///
/// Use struct-update syntax to supply the required overrides:
///
/// ```rust,ignore
/// UnitEntry {
///     span: SourceSpan::prelude(),
///     source_module: Some(module_display.clone()),
///     ..UnitEntry::from(cu)
/// }
/// ```
impl From<&CompiledUnit> for UnitEntry {
    fn from(cu: &CompiledUnit) -> Self {
        UnitEntry {
            name: cu.name.clone(),
            dimension: cu.dimension,
            factor: cu.factor,
            offset: cu.offset,
            is_pub: cu.is_pub,
            span: SourceSpan::empty(0),
            content_hash: cu.content_hash,
            source_module: None,
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
}
