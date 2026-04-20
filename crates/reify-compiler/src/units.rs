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
