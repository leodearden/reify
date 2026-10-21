//! The relate-solve's dimensional conditioning contract: the tolerance ladder,
//! the dimension each residual row is measured in, and the per-solve nominal
//! scales and tolerances derived from them (PRD
//! `docs/prds/v0_6/geometry-algebra-solver-unification.md` §7.3).

use std::fmt;

use reify_core::scalar_tolerance::{
    ANGULAR_REFERENCE_RADIUS_M, LENGTH_FLOOR_M, SOLVER_LENGTH_FLOOR_M,
};

// ── Residual row dimensions ──────────────────────────────────────────────────

/// What a residual row's number MEASURES.
///
/// A residual row vector is NOT dimensionally homogeneous, and nothing about an
/// `f64` says so. The axis-coincidence residual alone returns two dimensionless
/// tilt rows followed by two metre rows; `perpendicular` and `angle` return
/// dimensionless rows only. A caller that renders `max |row|` as millimetres
/// therefore states a FABRICATED length whenever the dominant row is angular —
/// the exact confidently-wrong claim the static-verification arm exists to
/// prevent, so the unit travels WITH the number instead of being re-derived by
/// the renderer from the relation name (which cannot work: `concentric` mixes
/// both in one vector).
///
/// The unit is declared where each row is BUILT, because that is the only place
/// it is known without re-doing the residual's name/operand-kind dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidualUnit {
    /// Metres — a position delta, offset or distance error.
    Length,
    /// Radians — an orientation delta (the exponential-map log rows).
    Angle,
    /// A pure number — a direction-component difference, a dot product, or a
    /// cosine difference. Has no length or angle reading.
    Dimensionless,
}

// ── Tolerance hierarchy ──────────────────────────────────────────────────────

/// The spacing between adjacent length rungs of the default ladder: one decade.
const RUNG_SPACING: f64 = 10.0;

/// The relate-solve's tolerance ladder (PRD §7.1 coherence law):
/// `kernel_local ≤ solver_convergence ≤ assertion/dedup` — the kernel's own datum
/// realization floor, the solver's residual-convergence target, and the (looser)
/// post-solve assertion/dedup tolerance — plus the solver's metre-row floor.
///
/// Every value is built through [`from_rungs`](Self::from_rungs), so an incoherent
/// ladder is unrepresentable. Numeric boundary checks test "within the solver's
/// convergence tolerance" — a method guarantee — never a hand-picked epsilon.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RelateTolerance {
    kernel_local: f64,
    solver_convergence: f64,
    assertion: f64,
    solver_length_floor: f64,
}

/// The four rungs of a [`RelateTolerance`], in metres, as handed to
/// [`RelateTolerance::from_rungs`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToleranceRungs {
    pub kernel_local_m: f64,
    pub solver_convergence_m: f64,
    pub assertion_m: f64,
    pub solver_length_floor_m: f64,
}

/// One rung of the tolerance ladder, as named by a [`ToleranceLadderError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rung {
    KernelLocal,
    SolverConvergence,
    Assertion,
    SolverLengthFloor,
}

impl fmt::Display for Rung {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Rung::KernelLocal => "kernel_local",
            Rung::SolverConvergence => "solver_convergence",
            Rung::Assertion => "assertion",
            Rung::SolverLengthFloor => "solver_length_floor",
        })
    }
}

/// Why [`RelateTolerance::from_rungs`] refused a ladder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToleranceLadderError {
    /// A rung is zero, negative, infinite or NaN.
    NotPositiveFinite { rung: Rung, value: f64 },
    /// A rung that must not exceed another does.
    OutOfOrder {
        lower: Rung,
        lower_value: f64,
        upper: Rung,
        upper_value: f64,
    },
}

impl fmt::Display for ToleranceLadderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotPositiveFinite { rung, value } => write!(
                f,
                "tolerance rung `{rung}` must be finite and greater than zero, got {value:e} m"
            ),
            Self::OutOfOrder {
                lower,
                lower_value,
                upper,
                upper_value,
            } => write!(
                f,
                "tolerance rung `{lower}` ({lower_value:e} m) must not exceed `{upper}` \
                 ({upper_value:e} m)"
            ),
        }
    }
}

impl std::error::Error for ToleranceLadderError {}

impl RelateTolerance {
    /// The kernel-default ladder, based on the `==` verdict's length floor
    /// ([`LENGTH_FLOOR_M`]): the convergence rung IS that floor, with the kernel
    /// rung one decade below and the assertion rung one decade above. The metre-row
    /// floor is [`SOLVER_LENGTH_FLOOR_M`].
    pub fn kernel_default() -> Self {
        Self::from_rungs(ToleranceRungs {
            kernel_local_m: LENGTH_FLOOR_M / RUNG_SPACING,
            solver_convergence_m: LENGTH_FLOOR_M,
            assertion_m: LENGTH_FLOOR_M * RUNG_SPACING,
            solver_length_floor_m: SOLVER_LENGTH_FLOOR_M,
        })
        .expect("the kernel-default ladder is coherent")
    }

    /// A ladder from explicit rungs. Refuses a rung that is not finite and
    /// positive, and any ordering other than
    /// `kernel_local ≤ solver_convergence ≤ assertion` with
    /// `solver_length_floor ≤ solver_convergence`.
    pub fn from_rungs(rungs: ToleranceRungs) -> Result<Self, ToleranceLadderError> {
        let named = [
            (Rung::KernelLocal, rungs.kernel_local_m),
            (Rung::SolverConvergence, rungs.solver_convergence_m),
            (Rung::Assertion, rungs.assertion_m),
            (Rung::SolverLengthFloor, rungs.solver_length_floor_m),
        ];
        if let Some(&(rung, value)) = named.iter().find(|(_, v)| !(v.is_finite() && *v > 0.0)) {
            return Err(ToleranceLadderError::NotPositiveFinite { rung, value });
        }
        let [
            kernel_local,
            solver_convergence,
            assertion,
            solver_length_floor,
        ] = named;
        for (lower, upper) in [
            (kernel_local, solver_convergence),
            (solver_convergence, assertion),
            (solver_length_floor, solver_convergence),
        ] {
            if lower.1 > upper.1 {
                return Err(ToleranceLadderError::OutOfOrder {
                    lower: lower.0,
                    lower_value: lower.1,
                    upper: upper.0,
                    upper_value: upper.1,
                });
            }
        }
        Ok(Self {
            kernel_local: rungs.kernel_local_m,
            solver_convergence: rungs.solver_convergence_m,
            assertion: rungs.assertion_m,
            solver_length_floor: rungs.solver_length_floor_m,
        })
    }

    /// The tightest rung — the kernel's local datum-realization floor (metres).
    pub fn kernel_local(&self) -> f64 {
        self.kernel_local
    }

    /// The solver's convergence rung, in metres, as read at the angular reference
    /// radius ([`ANGULAR_REFERENCE_RADIUS_M`]).
    pub fn solver_convergence(&self) -> f64 {
        self.solver_convergence
    }

    /// The solver's relative convergence tolerance `ε_rel` — the convergence rung
    /// over the angular reference radius. Dimensionless; it is the nominal
    /// tolerance of every angular and dimensionless row (PRD §7.3).
    pub fn relative_convergence(&self) -> f64 {
        self.solver_convergence / ANGULAR_REFERENCE_RADIUS_M
    }

    /// The floor of the solver's scale-relative metre-row tolerance (metres).
    pub fn solver_length_floor(&self) -> f64 {
        self.solver_length_floor
    }

    /// The (loosest) post-solve assertion/dedup tolerance, for a row measured in
    /// METRES. A row measured in radians or as a pure number is judged against
    /// [`assertion_angle`](Self::assertion_angle) /
    /// [`assertion_dimensionless`](Self::assertion_dimensionless) instead — a
    /// residual row vector is not dimensionally homogeneous (see [`ResidualUnit`]),
    /// so one rung cannot serve all three.
    pub fn assertion(&self) -> f64 {
        self.assertion
    }

    /// The assertion rung for a row measured in RADIANS — the angle that displaces
    /// a feature at [`ANGULAR_REFERENCE_RADIUS_M`] by the length rung.
    ///
    /// An orientation error `θ` displaces a point at radius `R` by `θ·R`, so an
    /// angle has no length reading until a radius is named. Comparing radians
    /// against the metre rung with no reference scale is a category error however
    /// close the two numbers happen to land.
    pub fn assertion_angle(&self) -> f64 {
        self.assertion / ANGULAR_REFERENCE_RADIUS_M
    }

    /// The assertion rung for a DIMENSIONLESS row — a unit-vector component
    /// difference, a dot product, or a cosine difference.
    ///
    /// The sine of the angular rung: every dimensionless form here measures the
    /// sine of a misalignment between two UNIT directions (exactly, for
    /// `perpendicular`'s dot product; to first order for the rest), so this is the
    /// same physical misalignment expressed in the unit the row carries. It is a
    /// meaningful rung only because those forms normalize their operands first —
    /// an unnormalized dot scales with the operands' magnitudes, and no fixed
    /// threshold means anything against a number that does.
    pub fn assertion_dimensionless(&self) -> f64 {
        self.assertion_angle().sin()
    }

    /// The assertion rung for a row measured in `unit`.
    pub fn assertion_rung(&self, unit: ResidualUnit) -> f64 {
        match unit {
            ResidualUnit::Length => self.assertion(),
            ResidualUnit::Angle => self.assertion_angle(),
            ResidualUnit::Dimensionless => self.assertion_dimensionless(),
        }
    }
}

// ── Per-solve scaling ────────────────────────────────────────────────────────

/// The kind of a solve unknown, which selects its nominal scale (PRD §7.3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnknownKind {
    /// A pose translation component, in metres.
    PoseTranslation,
    /// A pose rotation (exponential-map) component, in radians.
    PoseRotation,
    /// A scalar `auto`, in its own SI unit, with its bounds box and seed when known.
    ScalarAuto {
        bounds: Option<(f64, f64)>,
        seed: Option<f64>,
    },
}

/// The nominal scales and tolerances of ONE solve, which runs in `x̃_j = x_j / s_j`
/// and `r̃_i = r_i / t_i` (PRD §7.3).
///
/// Built once per scope, so the partition, the solve and the post-solve re-check
/// all read the same characteristic length `L`. Fields are private so
/// `L ≥ kernel_local` holds by construction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolveScaling {
    characteristic_length: f64,
    tolerance: RelateTolerance,
}

impl SolveScaling {
    /// The scaling of a solve whose participating datums span
    /// `participating_extent` metres: `L` is that extent floored at
    /// `kernel_local`. A non-finite extent carries no length information and gives
    /// the floor; the residual it came from fails the solve loudly on its own.
    pub fn new(participating_extent: f64, tolerance: RelateTolerance) -> Self {
        let floor = tolerance.kernel_local();
        let characteristic_length = if participating_extent.is_finite() {
            participating_extent.max(floor)
        } else {
            floor
        };
        Self {
            characteristic_length,
            tolerance,
        }
    }

    /// The characteristic length `L`, in metres.
    pub fn characteristic_length(&self) -> f64 {
        self.characteristic_length
    }

    /// The tolerance ladder this scaling was built from.
    pub fn tolerance(&self) -> RelateTolerance {
        self.tolerance
    }

    /// The nominal scale `s_j` of an unknown of `kind`, in that unknown's SI unit.
    pub fn unknown_scale(&self, kind: UnknownKind) -> f64 {
        match kind {
            UnknownKind::PoseTranslation => self.characteristic_length,
            UnknownKind::PoseRotation => 1.0,
            UnknownKind::ScalarAuto { bounds, seed } => scalar_auto_scale(bounds, seed),
        }
    }

    /// The nominal tolerance `t_i` of a row measured in `unit`: scale-relative
    /// `max(ε_rel·L, floor)` for a metre row, the scale-free `ε_rel` otherwise.
    pub fn row_tolerance(&self, unit: ResidualUnit) -> f64 {
        let relative = self.tolerance.relative_convergence();
        match unit {
            ResidualUnit::Length => {
                (relative * self.characteristic_length).max(self.tolerance.solver_length_floor())
            }
            ResidualUnit::Angle | ResidualUnit::Dimensionless => relative,
        }
    }

    /// The post-solve assertion rung of a row measured in `unit`. The metre rung
    /// grows as `max(L / R_ref, 1)` so it stays above [`row_tolerance`](Self::row_tolerance)
    /// at every `L`; the angular rungs are absolute (PRD §7.3 "Coherence at every
    /// scale").
    pub fn assertion_rung(&self, unit: ResidualUnit) -> f64 {
        match unit {
            ResidualUnit::Length => {
                self.tolerance.assertion()
                    * (self.characteristic_length / ANGULAR_REFERENCE_RADIUS_M).max(1.0)
            }
            ResidualUnit::Angle | ResidualUnit::Dimensionless => {
                self.tolerance.assertion_rung(unit)
            }
        }
    }

    /// The Gram–Schmidt independence cutoff on normalized SCALED Jacobian rows —
    /// dimensionless, `ε_rel`.
    pub fn rank_threshold(&self) -> f64 {
        self.tolerance.relative_convergence()
    }
}

/// A scalar `auto`'s nominal scale: its box width if bounded, else `|seed|`, else 1.
fn scalar_auto_scale(bounds: Option<(f64, f64)>, seed: Option<f64>) -> f64 {
    let width = bounds
        .filter(|(lo, hi)| lo < hi)
        .map(|(lo, hi)| hi - lo)
        .filter(|w| w.is_finite());
    let magnitude = seed.map(f64::abs).filter(|s| s.is_finite() && *s > 0.0);
    width.or(magnitude).unwrap_or(1.0)
}
