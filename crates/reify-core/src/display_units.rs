//! Per-cell display-unit ladders — the data table backing the GUI's
//! dimension-aware unit picker (task #5199).
//!
//! Each [`DimensionLadder`] lists the selectable display units for one
//! canonical dimension (as named by
//! `reify_core::DimensionVector::canonical_name`). `display_magnitude =
//! si_value / unit.si_scale`. Exactly one option per ladder is marked
//! `is_default`, matching `DimensionVector::to_display_units`'s existing
//! choice — this keeps the picker's default selection numerically identical
//! to the canonical backend-formatted `value`.
//!
//! Each ladder also carries a curated `derived_unit_name` (PRD
//! display-unit-preference §4) — the single per-dimension label, held
//! invariant-equal to the `is_default` rung's label (§4a) so downstream
//! eval/hover surfaces can unify on one string per dimension.
//!
//! Exposed to the frontend via the `get_unit_ladders` Tauri command
//! (`main.rs`). Doubles as the future substrate for auto-scaling defaults
//! and the DSL `@display` annotation follow-up (task #5200).

/// One selectable display unit within a [`DimensionLadder`].
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UnitOption {
    /// User-facing unit label (e.g. `"mm"`, `"L"`).
    pub label: String,
    /// `display_magnitude = si_value / si_scale`.
    pub si_scale: f64,
    /// Exactly one option per ladder has `is_default: true` — the unit
    /// `DimensionVector::to_display_units` already chooses for that dimension.
    pub is_default: bool,
}

/// Per-dimension auto-scaling policy (PRD display-unit-preference §5).
///
/// When present on a [`DimensionLadder`] it describes whether the display
/// formatter may hop rungs to keep a magnitude inside a readable target band,
/// plus the band itself. `enabled: false` is the opt-in/default-OFF posture
/// (the policy exists but is not applied until the user turns it on). A
/// `None` `auto_scale` on the ladder means the dimension is structurally
/// *excluded* from auto-scaling (e.g. Angle's discrete deg/rad, or a
/// single-rung ladder with no rung to hop across).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AutoScale {
    /// Default posture: `true` = auto-scaling applied by default; `false` =
    /// present but opt-in (off until enabled).
    pub enabled: bool,
    /// Inclusive lower bound of the target mantissa band — `1.0` per §5a
    /// (`1 ≤ |mantissa| < 1000`).
    pub band_lo: f64,
    /// Exclusive upper bound of the target mantissa band — `1000.0` per §5a.
    pub band_hi: f64,
}

/// The ordered set of display-unit options for one canonical dimension.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DimensionLadder {
    /// Canonical dimension name (`DimensionVector::canonical_name()`, e.g. `"Volume"`).
    pub dimension: String,
    /// The single curated derived-unit label for this dimension (PRD
    /// display-unit-preference §4). Invariant: always equals the label of
    /// this ladder's `is_default` rung — the default [`UnitOption`] is the
    /// single source of the curated per-dimension name (§4a). Stored as an
    /// explicit denormalized field (rather than re-derived from the rungs)
    /// so a dimension→name lookup is direct and independent of rung
    /// order/count; the equality is pinned by
    /// `every_ladder_exposes_curated_derived_unit_name`.
    pub derived_unit_name: String,
    /// Selectable units, in picker display order.
    pub units: Vec<UnitOption>,
    /// Auto-scaling policy for this dimension (PRD §5), or `None` when the
    /// dimension is *excluded* from auto-scaling (Angle's discrete deg/rad;
    /// the single-rung Force/Energy/Power ladders have no rung to hop).
    pub auto_scale: Option<AutoScale>,
}

/// What PRD display-unit-preference §5's auto-scaling policy decided for one
/// SI magnitude on one [`DimensionLadder`].
///
/// Deliberately carries **no** serde derive, unlike its sibling types in this
/// module: [`UnitOption`] / [`AutoScale`] / [`DimensionLadder`] are the data
/// *table* (shipped to the frontend by the `get_unit_ladders` Tauri command),
/// whereas this is a computed result over borrowed table rows — it has a
/// lifetime, it is recomputed per magnitude, and nothing transports it across
/// the process boundary.
#[derive(Debug, Clone, PartialEq)]
pub enum AutoScaleChoice<'a> {
    /// No auto-scaling applies — render at the ladder's static `is_default`
    /// rung exactly as before §5 existed. Returned for a dimension excluded
    /// from the policy (`auto_scale: None`), a default-OFF one
    /// (`enabled: false`, §5b), and for magnitudes the policy cannot act on
    /// (zero, non-finite).
    Static,
    /// Render at this rung: `display_magnitude = si_value / rung.si_scale`,
    /// which §5a's `1 ≤ |mantissa| < 1000` band is satisfied by.
    Rung(&'a UnitOption),
    /// §5c's engineering-notation fallback: auto-scaling is enabled for this
    /// dimension but *no* eligible rung keeps the mantissa in band, so the
    /// magnitude is rendered at the ladder's static default `rung` as
    /// `mantissa × 10^exponent` with `exponent` a multiple of three.
    ///
    /// §5c is explicit that this is a fallback mode *within* the auto-scaling
    /// policy, "not a separate feature with its own on/off switch" — and §5e
    /// that it "does **not** apply to default-OFF dimensions either". Both
    /// hold structurally here: this variant is only ever reachable past
    /// [`DimensionLadder::auto_scaled`]'s posture gate, so a default-OFF or
    /// `auto_scale: None` dimension can never produce it.
    Engineering {
        /// The ladder's `is_default` rung — §5d/§5e both name the static
        /// default rung as the anchor the magnitude is expressed against.
        rung: &'a UnitOption,
        /// Signed mantissa, `1 ≤ |mantissa| < band_hi`.
        mantissa: f64,
        /// Power-of-ten exponent, always a multiple of three (§5c).
        exponent: i32,
    },
}

impl DimensionLadder {
    /// Apply PRD display-unit-preference §5's auto-scaling policy to one SI
    /// magnitude, returning the rung to render at.
    ///
    /// This is the *policy* half of §5; the rendering half (the magnitude
    /// string, and §5c's `×10ⁿ` glyphs) lives with the display formatter in
    /// `reify-ir` so an auto-scaled magnitude stays numerically identical in
    /// convention to every other display magnitude.
    ///
    /// Three normative choices, each pinned to its clause:
    ///
    /// * **Posture gate (§5e).** The policy engages only for a default-ON
    ///   dimension. `auto_scale: None` (structurally excluded — Angle's
    ///   discrete deg/rad, the single-rung Force/Energy/Power ladders) and
    ///   `enabled: false` (default-OFF — Mass, Pressure, Density) both yield
    ///   [`AutoScaleChoice::Static`] at every magnitude: §5e's "renders its
    ///   static default rung's raw magnitude, full stop", including its
    ///   explicit statement that engineering notation does not apply to them
    ///   either. Note §5d needs no code here — the pin-suppression rule lives
    ///   at `resolve_display`'s explicit-preference early return, which never
    ///   reaches this function.
    ///
    /// * **Decimal-sibling eligibility (§5a).** §5a scopes the band to "one
    ///   SI-prefix step", so a rung is a candidate only when its `si_scale` is
    ///   a power-of-ten multiple of the default rung's. Length's `in` rung
    ///   (`si_scale 0.0254`, ratio 25.4) is the sole exclusion in the whole
    ///   registry. Without it a magnitude-driven rule could render an unpinned
    ///   *metric* length in inches — a unit-**system** flip, strictly worse
    ///   than the magnitude flip §5b's rationale is already written to avoid.
    ///
    /// * **Minimal hop (§5b).** Among eligible in-band rungs the one nearest
    ///   the ladder's default index wins, so auto-scaling moves as few rungs
    ///   as possible off the dimension's familiar default — §5b's stability
    ///   argument, applied to the choice *within* an enabled dimension.
    ///
    /// Returns [`AutoScaleChoice::Static`] for a zero or non-finite
    /// `si_value`, and for a ladder with a degenerate band or no `is_default`
    /// rung. `Static` means "render exactly as before §5", so each of those
    /// degradations is byte-identical to the pre-§5 output.
    pub fn auto_scaled(&self, si_value: f64) -> AutoScaleChoice<'_> {
        let Some(auto) = &self.auto_scale else {
            return AutoScaleChoice::Static;
        };
        if !auto.enabled
            || !auto.band_lo.is_finite()
            || !auto.band_hi.is_finite()
            || auto.band_lo >= auto.band_hi
        {
            return AutoScaleChoice::Static;
        }
        // Zero must render `0 mm`, never `0×10⁰ mm`; a non-finite magnitude
        // has no meaningful mantissa to band at all.
        if !si_value.is_finite() || si_value == 0.0 {
            return AutoScaleChoice::Static;
        }
        let Some((default_idx, default_rung)) =
            self.units.iter().enumerate().find(|(_, u)| u.is_default)
        else {
            return AutoScaleChoice::Static;
        };
        if !default_rung.si_scale.is_finite() || default_rung.si_scale <= 0.0 {
            return AutoScaleChoice::Static;
        }

        // Minimal hop off the default rung, ties broken by ladder order so the
        // choice is deterministic. Only the single minimum is ever used, so
        // this selects it directly rather than sorting a collected `Vec`: the
        // key `(hop_distance, idx)` is unique per rung, so there are no ties to
        // resolve and `min_by_key`'s first-minimum rule is exact. Allocation
        // matters here — `resolve_display` runs once per scalar during
        // recursive composite rendering (List/Set/Map/Matrix).
        if let Some((_, rung)) = self
            .units
            .iter()
            .enumerate()
            .filter(|(_, u)| is_decimal_sibling(u.si_scale, default_rung.si_scale))
            .filter(|(_, u)| in_band(si_value / u.si_scale, auto.band_lo, auto.band_hi))
            .min_by_key(|(idx, _)| (idx.abs_diff(default_idx), *idx))
        {
            return AutoScaleChoice::Rung(rung);
        }

        // §5c: auto-scaling is on but no rung fits, so express the magnitude
        // against the static default rung in engineering notation. A
        // pathological magnitude that will not normalize degrades to the
        // pre-§5 static rendering rather than emitting a bogus mantissa.
        //
        // The engineering split is fixed to `[ENGINEERING_BAND_LO,
        // ENGINEERING_BAND_HI)` rather than parameterized by this ladder's
        // band, because a multiple-of-three exponent can only ever normalize a
        // mantissa into a three-decade span — see [`engineering_parts`]. Every
        // registry band is that span (pinned by
        // `auto_scale_metadata_matches_prd_section5`); this asserts the two
        // halves of §5's policy cannot silently drift apart.
        debug_assert!(
            (auto.band_lo - ENGINEERING_BAND_LO).abs() < f64::EPSILON
                && (auto.band_hi - ENGINEERING_BAND_HI).abs() < f64::EPSILON * ENGINEERING_BAND_HI,
            "ladder {:?} declares band [{}, {}), but §5c engineering notation can only \
             normalize into [{ENGINEERING_BAND_LO}, {ENGINEERING_BAND_HI}) — the rung arm \
             and the fallback arm would disagree about what \"in band\" means",
            self.dimension,
            auto.band_lo,
            auto.band_hi
        );
        let magnitude = si_value / default_rung.si_scale;
        match engineering_parts(magnitude) {
            Some((mantissa, exponent)) => AutoScaleChoice::Engineering {
                rung: default_rung,
                mantissa,
                exponent,
            },
            None => AutoScaleChoice::Static,
        }
    }
}

/// Split `magnitude` into `(mantissa, exponent)` with `exponent` a multiple of
/// three and `|mantissa|` in `[ENGINEERING_BAND_LO, ENGINEERING_BAND_HI)` —
/// PRD display-unit-preference §5c's powers-of-three engineering form.
///
/// The band is **not** a parameter, because it is not a free choice: an
/// exponent constrained to multiples of three can only ever move a mantissa by
/// three decades at a time, so `[1, 1000)` is the only band a powers-of-three
/// split can settle in. Asking this function for, say, `[1, 100)` would be
/// unsatisfiable for any magnitude in `[100, 1000)` — the correction loop below
/// would step past it in both directions and exhaust. `auto_scaled` asserts its
/// ladder's band is this one before calling.
///
/// The exponent is *seeded* from `log10` and then corrected in a bounded loop
/// rather than trusted directly: `log10` of an exact power of ten can land a
/// hair below the integer (e.g. returning `2.9999999999999996` for `1000.0`),
/// which floors the seed one step low and leaves the mantissa out of band. The
/// correction is capped so no input can spin here.
///
/// Returns `None` when the split cannot be trusted — a non-finite or zero
/// mantissa (`powi` under/overflow at the extremes) or a magnitude the bounded
/// correction failed to bring in band. Callers degrade to
/// [`AutoScaleChoice::Static`] on `None`.
///
/// Band membership is decided by [`in_band`] / [`at_or_above`], i.e. at
/// *display* precision — see [`BAND_EDGE_EPS`]. Both the settle test and the
/// step direction must use the same comparator: deciding "settled?" at display
/// precision while stepping on the raw `>=` would oscillate a mantissa that
/// sits between the two thresholds until the correction cap gave up.
fn engineering_parts(magnitude: f64) -> Option<(f64, i32)> {
    if !magnitude.is_finite() || magnitude == 0.0 {
        return None;
    }

    let seed = magnitude.abs().log10() / 3.0;
    if !seed.is_finite() {
        return None;
    }
    // `as i32` is saturating in Rust, and the correction loop below rejects any
    // exponent that fails to bring the mantissa in band — so a saturated seed
    // cannot produce a silently wrong split.
    let mut exponent = 3 * (seed.floor() as i32);
    let mut mantissa = magnitude / 10f64.powi(exponent);

    const MAX_CORRECTIONS: u32 = 8;
    let mut corrections = 0;
    while mantissa.is_finite() && mantissa != 0.0 && corrections < MAX_CORRECTIONS {
        if at_or_above(mantissa, ENGINEERING_BAND_HI) {
            exponent += 3;
        } else if !at_or_above(mantissa, ENGINEERING_BAND_LO) {
            exponent -= 3;
        } else {
            // Settling at display precision admits a mantissa a few ULPs
            // *below* 1.0 (Volume `1000 m³` over the `mm³` default rung is
            // `999_999_999_999.9999`, whose ×10¹² mantissa is
            // `0.999_999_999_999_999_9` — it renders as `1`). Snap it onto the
            // edge so the returned split satisfies `1 ≤ |mantissa| < band_hi`
            // exactly rather than a hair under it. The snap is bounded by
            // `BAND_EDGE_EPS` by construction, so it cannot move the value by
            // more than half a display ULP.
            if mantissa.abs() < ENGINEERING_BAND_LO {
                mantissa = mantissa.signum() * ENGINEERING_BAND_LO;
            }
            return Some((mantissa, exponent));
        }
        mantissa = magnitude / 10f64.powi(exponent);
        corrections += 1;
    }
    None
}

/// Relative tolerance the §5a band edges are compared at.
///
/// §5a's `1 ≤ |mantissa| < 1000` band is a promise about the magnitude the user
/// *reads*, and the display formatter (`reify_ir`'s `format_display_number`)
/// rounds to twelve significant figures before rendering. A raw quotient a few
/// ULPs below an edge therefore still prints *at* that edge: Volume's
/// `1e-6 m³` over the `mm³` default rung is `999.999_999_999_999_9`, not
/// `1000.0`, so a raw `< band_hi` test would select `mm³` and print the
/// magnitude `1000` that the band promises never to appear. `5e-13` is half an
/// ULP at twelve significant figures, so comparing against `edge * (1 - EPS)`
/// reproduces exactly where that display rounding lands, at both edges.
///
/// This deliberately does **not** widen the band — it narrows the top edge and
/// widens the bottom one by the same half-ULP, keeping "in band" and "renders
/// in band" the same predicate. Pinned end-to-end by reify-ir's
/// `resolve_display_none_honours_section5e_across_the_whole_registry`, which
/// parses the rendered string back and rejects any out-of-band magnitude.
const BAND_EDGE_EPS: f64 = 5e-13;

/// Lower edge of the mantissa band [`engineering_parts`] normalizes into.
///
/// Fixed rather than read off the ladder — see [`engineering_parts`] for why a
/// powers-of-three exponent leaves no choice here. Kept as a named constant
/// (not a bare `1.0`) so the two edges of §5c's convention are stated in one
/// place and `auto_scaled`'s `debug_assert!` can name them.
const ENGINEERING_BAND_LO: f64 = 1.0;

/// Upper edge of [`ENGINEERING_BAND_LO`]'s band — three decades up, which is
/// exactly one step of a multiple-of-three exponent.
const ENGINEERING_BAND_HI: f64 = 1000.0;

/// Is `|magnitude|` at or above the band edge `edge`, compared at display
/// precision ([`BAND_EDGE_EPS`])?
fn at_or_above(magnitude: f64, edge: f64) -> bool {
    magnitude.abs() >= edge * (1.0 - BAND_EDGE_EPS)
}

/// Does `|magnitude|` fall in `[band_lo, band_hi)` at display precision?
///
/// §5a bands the *absolute* mantissa, so a negative magnitude selects the same
/// rung as its positive twin.
fn in_band(magnitude: f64, band_lo: f64, band_hi: f64) -> bool {
    at_or_above(magnitude, band_lo) && !at_or_above(magnitude, band_hi)
}

/// Is `si_scale` a power-of-ten multiple of `default_si_scale`?
///
/// PRD display-unit-preference §5a's "one SI-prefix step" eligibility rule —
/// see [`DimensionLadder::auto_scaled`] for why it is load-bearing rather than
/// decorative. Compared against an epsilon rather than `==` because the ratio
/// is a float quotient of two table constants (`1e-2 / 1e-3` is `10.000000000000002`,
/// not `10.0`).
fn is_decimal_sibling(si_scale: f64, default_si_scale: f64) -> bool {
    if !si_scale.is_finite() || si_scale <= 0.0 {
        return false;
    }
    let ratio = si_scale / default_si_scale;
    if !ratio.is_finite() || ratio <= 0.0 {
        return false;
    }
    let log = ratio.log10();
    log.is_finite() && (log - log.round()).abs() < 1e-9
}

/// ASCII-normalize a unit label's Unicode superscript exponents to the caret
/// spelling [`unit_ladders`] adopted in task λ (#5788): `mm²` -> `mm^2`,
/// `kg/m³` -> `kg/m^3`. Returns `None` for a label carrying neither glyph.
///
/// This is the CANONICAL Rust statement of contract C2's migration mapping,
/// and it deliberately lives beside the tables it describes: the rule is only
/// meaningful relative to which glyphs the curated ladders below actually
/// used, so a future move of that alphabet (task κ's `·` separator half is
/// still open) has one Rust site to edit rather than one per consumer crate.
/// `curated_labels_are_already_ascii_normalized` in this module's tests keeps
/// the two coupled.
///
/// Only U+00B2/U+00B3 are mapped, because those are the only superscripts the
/// curated ladders ever used — this is a migration aid for one specific
/// relabel, not a general Unicode-exponent parser. In particular it is NOT the
/// inverse of `reify-ir`'s engineering-notation `×10ⁿ` exponent formatter,
/// which spells the full superscript digit alphabet and is explicitly out of
/// contract C2's scope (PRD `angle-units-surface-convergence.md` §10).
///
/// NEVER widens an accept-set on its own. Callers validating a label must
/// still match rungs by exact equality and use this only to phrase a "did you
/// mean" migration hint — see `validate_display_dimension` in
/// `reify-compiler/src/annotations/display.rs`, the one in-tree consumer. The
/// GUI restates the same mapping in TypeScript (`normalizeUnitLabel`,
/// `gui/src/stores/unitLadder.ts`, task #6028) because it cannot call across
/// the language boundary; that copy cites this function as its source of
/// truth.
pub fn ascii_label_spelling(label: &str) -> Option<String> {
    if !label.contains(['\u{00B2}', '\u{00B3}']) {
        return None;
    }
    Some(label.replace('\u{00B2}', "^2").replace('\u{00B3}', "^3"))
}

/// Return the full set of per-dimension unit ladders.
///
/// Each dimension's `is_default` entry is numerically identical to the unit
/// `DimensionVector::to_display_units` already chooses for that dimension
/// (Length→mm, Area→mm^2, Volume→mm^3, Angle→deg; Mass/Pressure/Density and the
/// single-rung Force/Energy/Power ladders fall through `to_display_units`'s
/// unscaled fallback branch, so their defaults are the coherent-SI base unit
/// — kg, Pa, kg/m^3, N, J, W — at `si_scale: 1.0`).
pub fn unit_ladders() -> Vec<DimensionLadder> {
    vec![
        DimensionLadder {
            dimension: "Length".to_string(),
            derived_unit_name: "mm".to_string(),
            auto_scale: Some(AutoScale {
                enabled: true,
                band_lo: 1.0,
                band_hi: 1000.0,
            }),
            units: vec![
                UnitOption {
                    label: "mm".to_string(),
                    si_scale: 1e-3,
                    is_default: true,
                },
                UnitOption {
                    label: "cm".to_string(),
                    si_scale: 1e-2,
                    is_default: false,
                },
                UnitOption {
                    label: "m".to_string(),
                    si_scale: 1.0,
                    is_default: false,
                },
                UnitOption {
                    label: "in".to_string(),
                    si_scale: 0.0254,
                    is_default: false,
                },
            ],
        },
        DimensionLadder {
            dimension: "Area".to_string(),
            derived_unit_name: "mm^2".to_string(),
            auto_scale: Some(AutoScale {
                enabled: true,
                band_lo: 1.0,
                band_hi: 1000.0,
            }),
            units: vec![
                UnitOption {
                    label: "mm^2".to_string(),
                    si_scale: 1e-6,
                    is_default: true,
                },
                UnitOption {
                    label: "cm^2".to_string(),
                    si_scale: 1e-4,
                    is_default: false,
                },
                UnitOption {
                    label: "m^2".to_string(),
                    si_scale: 1.0,
                    is_default: false,
                },
            ],
        },
        DimensionLadder {
            dimension: "Volume".to_string(),
            derived_unit_name: "mm^3".to_string(),
            auto_scale: Some(AutoScale {
                enabled: true,
                band_lo: 1.0,
                band_hi: 1000.0,
            }),
            units: vec![
                UnitOption {
                    label: "mm^3".to_string(),
                    si_scale: 1e-9,
                    is_default: true,
                },
                UnitOption {
                    label: "cm^3".to_string(),
                    si_scale: 1e-6,
                    is_default: false,
                },
                UnitOption {
                    label: "L".to_string(),
                    si_scale: 1e-3,
                    is_default: false,
                },
                UnitOption {
                    label: "m^3".to_string(),
                    si_scale: 1.0,
                    is_default: false,
                },
            ],
        },
        DimensionLadder {
            dimension: "Angle".to_string(),
            derived_unit_name: "deg".to_string(),
            auto_scale: None,
            units: vec![
                UnitOption {
                    label: "deg".to_string(),
                    si_scale: std::f64::consts::PI / 180.0,
                    is_default: true,
                },
                UnitOption {
                    label: "rad".to_string(),
                    si_scale: 1.0,
                    is_default: false,
                },
            ],
        },
        DimensionLadder {
            dimension: "Mass".to_string(),
            derived_unit_name: "kg".to_string(),
            auto_scale: Some(AutoScale {
                enabled: false,
                band_lo: 1.0,
                band_hi: 1000.0,
            }),
            units: vec![
                UnitOption {
                    label: "g".to_string(),
                    si_scale: 1e-3,
                    is_default: false,
                },
                UnitOption {
                    label: "kg".to_string(),
                    si_scale: 1.0,
                    is_default: true,
                },
            ],
        },
        DimensionLadder {
            dimension: "Pressure".to_string(),
            derived_unit_name: "Pa".to_string(),
            auto_scale: Some(AutoScale {
                enabled: false,
                band_lo: 1.0,
                band_hi: 1000.0,
            }),
            units: vec![
                UnitOption {
                    label: "Pa".to_string(),
                    si_scale: 1.0,
                    is_default: true,
                },
                UnitOption {
                    label: "kPa".to_string(),
                    si_scale: 1e3,
                    is_default: false,
                },
                UnitOption {
                    label: "MPa".to_string(),
                    si_scale: 1e6,
                    is_default: false,
                },
                UnitOption {
                    label: "GPa".to_string(),
                    si_scale: 1e9,
                    is_default: false,
                },
            ],
        },
        DimensionLadder {
            dimension: "Density".to_string(),
            derived_unit_name: "kg/m^3".to_string(),
            auto_scale: Some(AutoScale {
                enabled: false,
                band_lo: 1.0,
                band_hi: 1000.0,
            }),
            units: vec![
                UnitOption {
                    label: "kg/m^3".to_string(),
                    si_scale: 1.0,
                    is_default: true,
                },
                UnitOption {
                    label: "g/cm^3".to_string(),
                    si_scale: 1000.0,
                    is_default: false,
                },
            ],
        },
        // Single-rung coherent-SI ladders (PRD display-unit-preference §4):
        // seed the curated derived-unit name over the existing
        // DimensionVector::FORCE/ENERGY/POWER consts. One is_default rung
        // satisfies the every_ladder_has_exactly_one_default guard; §5
        // assigns them no auto-scale posture (auto_scale = None).
        DimensionLadder {
            dimension: "Force".to_string(),
            derived_unit_name: "N".to_string(),
            auto_scale: None,
            units: vec![UnitOption {
                label: "N".to_string(),
                si_scale: 1.0,
                is_default: true,
            }],
        },
        DimensionLadder {
            dimension: "Energy".to_string(),
            derived_unit_name: "J".to_string(),
            auto_scale: None,
            units: vec![UnitOption {
                label: "J".to_string(),
                si_scale: 1.0,
                is_default: true,
            }],
        },
        DimensionLadder {
            dimension: "Power".to_string(),
            derived_unit_name: "W".to_string(),
            auto_scale: None,
            units: vec![UnitOption {
                label: "W".to_string(),
                si_scale: 1.0,
                is_default: true,
            }],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Find the ladder for a canonical dimension name, panicking with a
    /// helpful message if it is absent (every assertion below expects the
    /// dimension to be present).
    fn ladder<'a>(ladders: &'a [DimensionLadder], dimension: &str) -> &'a DimensionLadder {
        ladders
            .iter()
            .find(|l| l.dimension == dimension)
            .unwrap_or_else(|| panic!("no ladder for dimension {dimension:?}"))
    }

    /// Find a unit option within a ladder by label, panicking if absent.
    fn unit<'a>(ladder: &'a DimensionLadder, label: &str) -> &'a UnitOption {
        ladder
            .units
            .iter()
            .find(|u| u.label == label)
            .unwrap_or_else(|| panic!("no unit {label:?} in ladder {:?}", ladder.dimension))
    }

    #[test]
    fn ladders_exist_for_all_seven_dimensions() {
        let ladders = unit_ladders();
        for dimension in [
            "Length", "Area", "Volume", "Angle", "Mass", "Pressure", "Density",
        ] {
            ladder(&ladders, dimension);
        }
    }

    #[test]
    fn every_ladder_has_exactly_one_default() {
        let ladders = unit_ladders();
        assert!(!ladders.is_empty(), "unit_ladders() must not be empty");
        for l in &ladders {
            let defaults = l.units.iter().filter(|u| u.is_default).count();
            assert_eq!(
                defaults, 1,
                "ladder {:?} should have exactly one is_default unit, found {defaults}",
                l.dimension
            );
        }
    }

    /// §4a curated-name invariant: every ladder exposes a non-empty
    /// `derived_unit_name`, and it is exactly the label of that ladder's
    /// single `is_default` rung — the default [`UnitOption`] is the single
    /// source of the curated per-dimension label. Pins the denormalized
    /// field to its authoritative source so it cannot drift from the
    /// default rung.
    #[test]
    fn every_ladder_exposes_curated_derived_unit_name() {
        for l in &unit_ladders() {
            assert!(
                !l.derived_unit_name.is_empty(),
                "ladder {:?} has an empty derived_unit_name",
                l.dimension
            );
            let default_rung = l
                .units
                .iter()
                .find(|u| u.is_default)
                .unwrap_or_else(|| panic!("ladder {:?} has no is_default unit", l.dimension));
            assert_eq!(
                l.derived_unit_name, default_rung.label,
                "ladder {:?} derived_unit_name must equal its is_default rung label",
                l.dimension
            );
        }
    }

    /// PRD display-unit-preference §4: Force/Energy/Power are seeded as
    /// single-rung ladders (coherent-SI N/J/W @ `si_scale: 1.0`,
    /// `is_default: true`) supplying the curated derived-unit name. A single
    /// `is_default` rung satisfies `every_ladder_has_exactly_one_default`,
    /// and the names round-trip through `canonical_name` (they are
    /// `NAMED_DIMENSIONS` keys — see the round-trip guard below).
    #[test]
    fn force_energy_power_ladders_seeded() {
        let ladders = unit_ladders();
        for (dimension, label) in [("Force", "N"), ("Energy", "J"), ("Power", "W")] {
            let l = ladder(&ladders, dimension);
            assert_eq!(
                l.units.len(),
                1,
                "ladder {dimension:?} should have exactly one rung"
            );
            let rung = &l.units[0];
            assert_eq!(rung.label, label, "{dimension:?} rung label mismatch");
            assert_eq!(
                rung.si_scale, 1.0,
                "{dimension:?} rung si_scale must be 1.0"
            );
            assert!(rung.is_default, "{dimension:?} rung must be is_default");
            assert_eq!(
                l.derived_unit_name, label,
                "{dimension:?} derived_unit_name mismatch"
            );
        }
    }

    /// Collapsed data-driven replacement for five former per-ladder pin
    /// tests (task #5199 amend, reviewer_comprehensive test_coverage
    /// finding): each hand-copied the same `si_scale`/`is_default`
    /// constants the constructor above already contains, and — for the
    /// `is_default` rungs — duplicated what
    /// `default_si_scale_matches_to_display_units_numeric_value` below
    /// already locks against its true source (`to_display_units`). That
    /// test only exercises each ladder's DEFAULT rung though, so this table
    /// keeps ONLY the non-default-rung coverage (cm/m/in, L, g, g/cm^3, …)
    /// the five tests used to provide — the default rungs themselves
    /// (Volume mm^3, Length mm, Mass kg, Pressure Pa, Density kg/m^3) are
    /// deliberately omitted here since pinning them would just re-assert
    /// the constructor's own constants back at itself; their correctness is
    /// covered, against the real source, by the anchored test below (task
    /// #5199 amend, reviewer_comprehensive test_coverage finding — round 2).
    #[test]
    fn ladder_units_pin_expected_scale_and_default() {
        let ladders = unit_ladders();

        // (dimension, label, si_scale, is_default)
        let expected: &[(&str, &str, f64, bool)] = &[
            ("Volume", "L", 1e-3, false),
            ("Length", "cm", 1e-2, false),
            ("Length", "m", 1.0, false),
            ("Length", "in", 0.0254, false),
            ("Mass", "g", 1e-3, false),
            ("Density", "g/cm^3", 1000.0, false),
        ];
        for &(dimension, label, si_scale, is_default) in expected {
            let u = unit(ladder(&ladders, dimension), label);
            assert_eq!(
                u.si_scale, si_scale,
                "{dimension}/{label} si_scale mismatch"
            );
            assert_eq!(
                u.is_default, is_default,
                "{dimension}/{label} is_default mismatch"
            );
        }

        // Pressure lists additional rungs above Pa; scales aren't pinned
        // here — just confirm they exist as selectable rungs on the ladder.
        let pressure = ladder(&ladders, "Pressure");
        for label in ["kPa", "MPa", "GPa"] {
            unit(pressure, label);
        }
    }

    /// Drift guard: every ladder key must be a *real* canonical dimension
    /// name — i.e. round-tripping the name back to its `DimensionVector` (via
    /// the shared `NAMED_DIMENSIONS` table) and calling `canonical_name()` on
    /// it must yield the same name back. This catches alias collisions (e.g.
    /// `TranslationalStiffness`/`Stiffness`) where a name is a valid key into
    /// `NAMED_DIMENSIONS` but is not what `canonical_name()` actually
    /// produces for that dimension — such a ladder would never be reachable
    /// from `build_values`'s `dim.canonical_name()` call.
    #[test]
    fn every_ladder_dimension_round_trips_through_canonical_name() {
        for l in &unit_ladders() {
            let dim = crate::NAMED_DIMENSIONS
                .iter()
                .find(|(_, name)| *name == l.dimension)
                .map(|(dim, _)| *dim)
                .unwrap_or_else(|| {
                    panic!(
                        "ladder dimension {:?} is not a NAMED_DIMENSIONS name",
                        l.dimension
                    )
                });
            assert_eq!(
                dim.canonical_name(),
                Some(l.dimension.as_str()),
                "ladder dimension {:?} does not round-trip through canonical_name()",
                l.dimension
            );
        }
    }

    /// PRD display-unit-preference §5: structural auto-scale posture.
    ///
    /// Auto-scaling has no consumer yet — the `enabled` flag is documented as
    /// applied by no formatter until §5 rung-hopping lands — so this test pins
    /// only the structural invariants that are NOT a tautological echo of the
    /// constructor's own policy literals:
    ///   * the dimensions structurally *excluded* from auto-scaling
    ///     (`auto_scale == None`) are exactly Angle (discrete deg/rad) and the
    ///     single-rung Force/Energy/Power ladders (no rung to hop) — guarded in
    ///     both directions (named set + count) so a dimension can't silently
    ///     drop out of, or into, auto-scaling;
    ///   * every *included* dimension carries the §5a `1 ≤ |mantissa| < 1000`
    ///     target band (`band_lo == 1.0`, `band_hi == 1000.0`).
    ///
    /// The per-dimension default-ON/OFF `enabled` literal *is* pinned here as
    /// of task #5236 (L1 deferred it, citing the umbrella #5200 rather than
    /// this leaf). It is no longer a tautological echo of the constructor:
    /// [`DimensionLadder::auto_scaled`] branches on `enabled`, so the split
    /// §5b draws — Length/Area/Volume ON, Mass/Pressure/Density OFF — is now
    /// load-bearing policy. The *behavioural* anchor for that posture lives
    /// where a consumer can observe it: `auto_scaled`'s own posture-gate test
    /// below, and `resolve_display`'s no-op test in `reify-ir`, which pin the
    /// consequence (hop vs. no hop) rather than the flag.
    #[test]
    fn auto_scale_metadata_matches_prd_section5() {
        let ladders = unit_ladders();

        // §5b default posture, per dimension. The split mirrors the ladder's
        // existing default-rung choices (§2c): the dimensions that already
        // default to a *scaled* rung (mm/mm²/mm³) auto-scale by default; the
        // ones defaulting to the bare SI base rung (kg/Pa/kg·m⁻³) do not,
        // because hopping them would silently flip a familiar unit as the
        // magnitude crosses a threshold.
        for (dimension, enabled) in [
            ("Length", true),
            ("Area", true),
            ("Volume", true),
            ("Mass", false),
            ("Pressure", false),
            ("Density", false),
        ] {
            let auto = ladder(&ladders, dimension)
                .auto_scale
                .as_ref()
                .unwrap_or_else(|| panic!("ladder {dimension:?} must carry an auto_scale posture"));
            assert_eq!(
                auto.enabled, enabled,
                "ladder {dimension:?} §5b default posture mismatch"
            );
        }

        // Structural include/exclude partition: exactly these four dimensions
        // are excluded from auto-scaling (auto_scale == None). Pinning the
        // count alongside the named set closes both directions without
        // coupling to constructor order.
        let excluded_count = ladders.iter().filter(|l| l.auto_scale.is_none()).count();
        assert_eq!(
            excluded_count, 4,
            "exactly four ladders should be excluded from auto-scaling (auto_scale == None)"
        );
        for dimension in ["Angle", "Force", "Energy", "Power"] {
            assert_eq!(
                ladder(&ladders, dimension).auto_scale,
                None,
                "ladder {dimension:?} must be excluded from auto-scaling (auto_scale == None)"
            );
        }

        // §5a: every included (Some) dimension uses the 1 ≤ |mantissa| < 1000 band.
        for l in &ladders {
            if let Some(a) = &l.auto_scale {
                assert_eq!(a.band_lo, 1.0, "ladder {:?} band_lo mismatch", l.dimension);
                assert_eq!(
                    a.band_hi, 1000.0,
                    "ladder {:?} band_hi mismatch",
                    l.dimension
                );
            }
        }
    }

    /// Drift guard for the module doc's core invariant: each ladder's
    /// `is_default` entry is "numerically identical" to what
    /// `DimensionVector::to_display_units` already chooses. The pin test
    /// above (`ladder_units_pin_expected_scale_and_default`) only checks the
    /// ladder's own hand-copied constants against each other; it would keep
    /// passing even if `to_display_units` itself drifted (e.g. its Length
    /// default silently changed unit). This test instead runs a known SI
    /// magnitude through `to_display_units` directly and checks that
    /// dividing by the ladder's default `si_scale` reproduces its converted
    /// value, locking the invariant to its actual source.
    ///
    /// Only the numeric value is compared, not the unit label:
    /// Mass/Pressure/Density intentionally use a more specific default label
    /// (`"kg"`, `"Pa"`, `"kg/m^3"`) than `to_display_units`'s generic `"SI"`
    /// fallback label (see module doc above) — that label divergence is
    /// deliberate, not drift.
    #[test]
    fn default_si_scale_matches_to_display_units_numeric_value() {
        let ladders = unit_ladders();
        let cases: &[(crate::DimensionVector, f64)] = &[
            (crate::DimensionVector::LENGTH, 0.08),
            (crate::DimensionVector::AREA, 0.0045),
            (crate::DimensionVector::VOLUME, 0.00704500224),
            (crate::DimensionVector::ANGLE, 1.2),
            (crate::DimensionVector::MASS, 2.5),
            (crate::DimensionVector::PRESSURE, 101_325.0),
            (crate::DimensionVector::MASS_DENSITY, 7850.0),
            // Single-rung coherent-SI ladders (§4): to_display_units passes
            // them through unscaled, so their default si_scale must be 1.0.
            (crate::DimensionVector::FORCE, 250.0),
            (crate::DimensionVector::ENERGY, 1500.0),
            (crate::DimensionVector::POWER, 750.0),
        ];

        for &(dim, si_value) in cases {
            let name = dim
                .canonical_name()
                .unwrap_or_else(|| panic!("{dim:?} has no canonical_name"));
            let l = ladder(&ladders, name);
            let default_unit = l
                .units
                .iter()
                .find(|u| u.is_default)
                .unwrap_or_else(|| panic!("ladder {name:?} has no is_default unit"));

            let (expected_value, _label) = dim.to_display_units(si_value);
            let actual_value = si_value / default_unit.si_scale;
            let tolerance = 1e-9 * expected_value.abs().max(1.0);

            assert!(
                (actual_value - expected_value).abs() <= tolerance,
                "ladder {name:?} default si_scale ({}) diverges from to_display_units: \
                 si_value/si_scale = {actual_value}, to_display_units = {expected_value}",
                default_unit.si_scale,
            );
        }
    }

    // ── §5 auto-scaling policy: rung selection (task #5236) ──────────────────
    //
    // PRD display-unit-preference §5a/§5b/§5e. [`DimensionLadder::auto_scaled`]
    // is the *policy* half of the leaf: given an SI magnitude it decides which
    // rung (if any) this ladder should render at, or that no rung fits and the
    // §5c engineering-notation fallback applies. The *rendering* half (the
    // magnitude string and §5c's `×10ⁿ` glyphs) lives in reify-ir beside
    // `format_display_number`, so an auto-scaled magnitude stays numerically
    // identical in convention to every other display magnitude.

    /// Magnitudes spanning `10^lo_exp ..= 10^hi_exp`, four samples per decade.
    ///
    /// The exact powers of ten are deliberately included: `log10` on an exact
    /// power of ten can land a hair below the integer, which would floor an
    /// engineering exponent one step low — the §5c normalization invariants
    /// below exist to catch exactly that.
    fn magnitude_sweep(lo_exp: i32, hi_exp: i32) -> Vec<f64> {
        let mut out = Vec::new();
        for e in lo_exp..=hi_exp {
            let decade = 10f64.powi(e);
            for mantissa in [1.0, 2.5, 4.2, 7.5] {
                out.push(mantissa * decade);
            }
        }
        out
    }

    /// The label of whichever rung a choice selected (the anchor rung, for the
    /// engineering variant), or `None` for [`AutoScaleChoice::Static`].
    fn chosen_label<'a>(choice: &AutoScaleChoice<'a>) -> Option<&'a str> {
        match choice {
            AutoScaleChoice::Static => None,
            AutoScaleChoice::Rung(u) => Some(u.label.as_str()),
            AutoScaleChoice::Engineering { rung, .. } => Some(rung.label.as_str()),
        }
    }

    /// §5b/§5e posture gate. A dimension structurally *excluded* from
    /// auto-scaling (`auto_scale: None` — Angle's discrete deg/rad, and the
    /// single-rung Force/Energy/Power ladders with no rung to hop) or
    /// *default-OFF* (`enabled: false` — Mass, Pressure, Density) never hops a
    /// rung, at any magnitude. §5e states this as one rule: a default-OFF
    /// dimension "renders its static default rung's raw magnitude, full stop",
    /// and engineering notation "does **not** apply to default-OFF dimensions
    /// either" — so `Static` is the only admissible answer for both groups.
    #[test]
    fn auto_scaled_posture_gate_keeps_excluded_and_default_off_dims_static() {
        let ladders = unit_ladders();
        for dimension in [
            "Angle", "Force", "Energy", "Power", "Mass", "Pressure", "Density",
        ] {
            let l = ladder(&ladders, dimension);
            for si_value in magnitude_sweep(-12, 12) {
                assert_eq!(
                    l.auto_scaled(si_value),
                    AutoScaleChoice::Static,
                    "ladder {dimension:?} must not auto-scale at si_value {si_value}"
                );
            }
        }

        // The out-of-band magnitudes §5b's rationale names explicitly: a Mass
        // that would otherwise flip 2.5 kg → 2500 g, a Pressure hopping
        // Pa→kPa→MPa→GPa, and a Density well past the band's top.
        for (dimension, si_value) in [
            ("Mass", 2.5e-6),
            ("Pressure", 1.01325e5),
            ("Density", 7850.0),
        ] {
            assert_eq!(
                ladder(&ladders, dimension).auto_scaled(si_value),
                AutoScaleChoice::Static,
                "{dimension:?} is default-OFF/excluded; {si_value} must not trigger a hop"
            );
        }
    }

    /// §5a rung hops. A default-ON dimension picks the rung that keeps
    /// `|mantissa|` inside the `1 ≤ |mantissa| < 1000` band. `0.08 m` needs no
    /// hop at all (mm already reads 80), while larger and smaller magnitudes
    /// walk the ladder. The Volume case is the PRD's own G1 figure
    /// (`0.007 m³ → 7 L`) — reached here at §6(1) rung 3 with no `@display`
    /// pin whatsoever, which is what §5e's unpinned rule buys.
    #[test]
    fn auto_scaled_hops_to_the_rung_that_lands_in_band() {
        let ladders = unit_ladders();

        let length = ladder(&ladders, "Length");
        assert_eq!(
            length.auto_scaled(0.08),
            AutoScaleChoice::Rung(unit(length, "mm")),
            "0.08 m already reads 80 mm — in band, so no hop"
        );
        assert_eq!(
            length.auto_scaled(5.0),
            AutoScaleChoice::Rung(unit(length, "cm")),
            "5.0 m is 5000 mm (out of band) but 500 cm — one hop"
        );
        assert_eq!(
            length.auto_scaled(50.0),
            AutoScaleChoice::Rung(unit(length, "m")),
            "50.0 m is 50000 mm / 5000 cm (both out of band) but 50 m — two hops"
        );

        let area = ladder(&ladders, "Area");
        assert_eq!(
            area.auto_scaled(0.0045),
            AutoScaleChoice::Rung(unit(area, "cm^2")),
            "0.0045 m² is 4500 mm² (out of band) but 45 cm²"
        );

        let volume = ladder(&ladders, "Volume");
        assert_eq!(
            volume.auto_scaled(0.007),
            AutoScaleChoice::Rung(unit(volume, "L")),
            "PRD G1: 0.007 m³ is 7000000 mm³ / 7000 cm³ (both out of band) but 7 L"
        );
    }

    /// §5b stability rationale, expressed as the tie-break: when several
    /// eligible rungs are simultaneously in band, the winner is the one
    /// *nearest the ladder's default rung index* — auto-scaling moves as few
    /// rungs as possible off the dimension's familiar default. `0.5 m` is both
    /// `500 mm` and `50 cm`; a mm-default CAD DSL must read it as 500 mm.
    #[test]
    fn auto_scaled_tie_break_prefers_the_rung_nearest_the_default() {
        let ladders = unit_ladders();
        let length = ladder(&ladders, "Length");

        // Both mm (500) and cm (50) are in [1, 1000); mm is the default rung.
        assert_eq!(
            length.auto_scaled(0.5),
            AutoScaleChoice::Rung(unit(length, "mm"))
        );
        // Both cm (500) and m (5) are in band; cm is one hop from the default,
        // m is two — so the nearer rung wins even when neither is the default.
        assert_eq!(
            length.auto_scaled(5.0),
            AutoScaleChoice::Rung(unit(length, "cm"))
        );
    }

    /// §5a's "one SI-prefix step": a candidate rung is eligible only when its
    /// `si_scale` is a power-of-ten multiple of the default rung's. Length's
    /// `in` rung (`si_scale 0.0254`, ratio 25.4 to the mm default) is the one
    /// exclusion in the whole registry — every other rung across all seven
    /// multi-rung ladders is already a decimal sibling of its default. Without
    /// the filter, a magnitude-driven rule could render an unpinned *metric*
    /// length in inches: a unit-*system* flip, strictly worse than the
    /// magnitude flip §5b's rationale is written to avoid.
    ///
    /// The filter is belt-and-braces here rather than the sole guard: under the
    /// minimal-hop tie-break `in` sits last in candidate order, and its in-band
    /// SI span `[0.0254, 25.4)` is fully subsumed by mm ∪ cm ∪ m
    /// (`[1e-3, 1000)`), so it would be unreachable by subsumption too. Pinning
    /// it explicitly keeps the §5a eligibility rule normative in code rather
    /// than an accident of the current rung ordering.
    #[test]
    fn auto_scaled_never_selects_the_non_decimal_inch_rung() {
        let ladders = unit_ladders();
        let length = ladder(&ladders, "Length");

        for si_value in magnitude_sweep(-4, 4) {
            for signed in [si_value, -si_value] {
                let choice = length.auto_scaled(signed);
                assert_ne!(
                    chosen_label(&choice),
                    Some("in"),
                    "auto-scaling must never flip an unpinned metric Length into inches \
                     (si_value {signed}, choice {choice:?})"
                );
            }
        }
    }

    /// Guards that must degrade to [`AutoScaleChoice::Static`] — i.e. to
    /// today's static-default-rung rendering — rather than hop or reach §5c's
    /// engineering notation. Zero is the load-bearing case: it must render as
    /// `0 mm`, never `0×10⁰ mm`.
    #[test]
    fn auto_scaled_zero_and_non_finite_magnitudes_stay_static() {
        let ladders = unit_ladders();
        for dimension in ["Length", "Area", "Volume"] {
            let l = ladder(&ladders, dimension);
            for si_value in [0.0, -0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                assert_eq!(
                    l.auto_scaled(si_value),
                    AutoScaleChoice::Static,
                    "ladder {dimension:?} must stay static at si_value {si_value}"
                );
            }
        }
    }

    /// The third `Static` exit, and the only one reachable from a *finite*
    /// non-zero `si_value`: the quotient against the default rung overflows.
    ///
    /// Volume's default rung is `mm³` (`si_scale 1e-9`), so `1e300 m³` divides
    /// to `inf`. No rung is in band, and `engineering_parts` has no mantissa to
    /// split, so it returns `None` and `auto_scaled` degrades to
    /// [`AutoScaleChoice::Static`] — the cell renders a bare `inf mm³`.
    ///
    /// That is a *graceful* degradation, not a regression: the pre-§5 static
    /// path divided by the same `si_scale` and produced the same `inf`. But it
    /// is the one case where §5e's "hop or engineering notation, never a bare
    /// out-of-band magnitude" does not hold, so it is pinned here rather than
    /// left implicit — a future change to the degradation policy should have to
    /// edit this test deliberately.
    ///
    /// `engineering_parts`' other `None` exit (the `MAX_CORRECTIONS` loop
    /// exhaustion) lands on this same `None => Static` arm; it is unreachable
    /// from the shipped registry, since the settle test and the step direction
    /// share one comparator and so cannot oscillate.
    #[test]
    fn auto_scaled_degrades_to_static_when_the_quotient_overflows() {
        let ladders = unit_ladders();
        let volume = ladder(&ladders, "Volume");

        assert!(
            !(1e300f64 / 1e-9).is_finite(),
            "premise: 1e300 m³ over the mm³ rung really does overflow"
        );
        for signed in [1e300, -1e300] {
            assert_eq!(
                volume.auto_scaled(signed),
                AutoScaleChoice::Static,
                "an overflowing quotient has no mantissa to band, so §5's policy \
                 degrades to the pre-§5 static rendering (si_value {signed})"
            );
        }

        // The neighbouring dimensions do *not* overflow at this magnitude
        // (mm² is 1e-6, mm is 1e-3), so they still honour §5e — this is a
        // property of the rung's scale, not a blanket ceiling on si_value.
        for dimension in ["Length", "Area"] {
            assert_ne!(
                ladder(&ladders, dimension).auto_scaled(1e300),
                AutoScaleChoice::Static,
                "{dimension} does not overflow at 1e300, so §5e still applies"
            );
        }
    }

    /// §5a's band is stated on `|mantissa|`, so a negative magnitude selects
    /// exactly the rung its absolute value would.
    #[test]
    fn auto_scaled_selects_on_absolute_magnitude() {
        let ladders = unit_ladders();
        let area = ladder(&ladders, "Area");
        assert_eq!(
            area.auto_scaled(-0.0045),
            AutoScaleChoice::Rung(unit(area, "cm^2")),
            "the band is on |mantissa|, so -0.0045 m² picks the same rung as +0.0045"
        );

        for dimension in ["Length", "Area", "Volume"] {
            let l = ladder(&ladders, dimension);
            for si_value in magnitude_sweep(-6, 6) {
                assert_eq!(
                    chosen_label(&l.auto_scaled(si_value)),
                    chosen_label(&l.auto_scaled(-si_value)),
                    "ladder {dimension:?} picked different rungs for ±{si_value}"
                );
            }
        }
    }

    // ── §5c engineering-notation fallback (task #5236) ───────────────────────

    /// The ladders auto-scaling actually engages for (§5b default-ON).
    fn default_on_ladders(ladders: &[DimensionLadder]) -> Vec<&DimensionLadder> {
        ladders
            .iter()
            .filter(|l| l.auto_scale.as_ref().is_some_and(|a| a.enabled))
            .collect()
    }

    /// The `is_default` rung — §5d/§5e both name the *static default rung* as
    /// the anchor an engineering-notation magnitude is expressed against.
    fn default_rung(l: &DimensionLadder) -> &UnitOption {
        l.units
            .iter()
            .find(|u| u.is_default)
            .unwrap_or_else(|| panic!("ladder {:?} has no is_default rung", l.dimension))
    }

    /// §5c's real coverage gap, on the registry as it actually ships.
    ///
    /// With §5a's decimal-sibling filter, Area's eligible rungs (mm² 1e-6,
    /// cm² 1e-4, m² 1.0) put a magnitude in band over SI
    /// `[1e-6, 0.1) ∪ [1, 1000)` — so `[0.1, 1) m²` is a genuine hole, and
    /// `0.5 m²` reads 5e5 mm² / 5e3 cm² / 0.5 m², none of them in band. This
    /// is precisely the case §5c's fallback exists for: "a Length small enough
    /// that no rung lands in `[1,1000)`", generalized. Length (`[1e-3, 1000)`)
    /// and Volume (`[1e-9, 1000)`) are contiguous by contrast, so they only
    /// reach engineering notation outside those spans.
    #[test]
    fn auto_scaled_falls_back_to_engineering_notation_in_the_area_gap() {
        let ladders = unit_ladders();
        let area = ladder(&ladders, "Area");
        assert_eq!(
            area.auto_scaled(0.5),
            AutoScaleChoice::Engineering {
                rung: unit(area, "mm^2"),
                mantissa: 500.0,
                exponent: 3,
            },
            "no Area rung lands in band at 0.5 m², so §5c's fallback applies"
        );
    }

    /// §5d/§5e: engineering notation is anchored on the ladder's static
    /// default rung — the label stays the dimension's familiar one and only
    /// the magnitude changes form.
    #[test]
    fn auto_scaled_engineering_notation_anchors_on_the_default_rung() {
        let ladders = unit_ladders();

        let length = ladder(&ladders, "Length");
        assert_eq!(
            length.auto_scaled(1e-9),
            AutoScaleChoice::Engineering {
                rung: default_rung(length),
                mantissa: 1.0,
                exponent: -6,
            },
            "1e-9 m is below every Length rung's band, so it reads 1×10⁻⁶ mm"
        );

        let volume = ladder(&ladders, "Volume");
        assert_eq!(
            volume.auto_scaled(5000.0),
            AutoScaleChoice::Engineering {
                rung: default_rung(volume),
                mantissa: 5.0,
                exponent: 12,
            },
            "5000 m³ is above every Volume rung's band, so it reads 5×10¹² mm³"
        );
    }

    /// §5c normalization invariants, swept. The exact powers of ten in the
    /// sweep are the point: a naive exponent seeded from `log10` can floor one
    /// step low when `log10` of an exact power of ten lands a hair below the
    /// integer, which would silently produce a mantissa outside the band.
    #[test]
    fn auto_scaled_engineering_notation_normalizes_mantissa_and_exponent() {
        let ladders = unit_ladders();
        for l in default_on_ladders(&ladders) {
            let band_hi = l
                .auto_scale
                .as_ref()
                .expect("default-ON ladder has auto_scale")
                .band_hi;
            for si_value in magnitude_sweep(-12, 12) {
                for signed in [si_value, -si_value] {
                    let AutoScaleChoice::Engineering {
                        rung,
                        mantissa,
                        exponent,
                    } = l.auto_scaled(signed)
                    else {
                        continue;
                    };
                    let dim = &l.dimension;
                    assert_eq!(
                        exponent % 3,
                        0,
                        "{dim:?} @ {signed}: exponent {exponent} is not a multiple of three"
                    );
                    assert!(
                        mantissa.abs() >= 1.0 && mantissa.abs() < band_hi,
                        "{dim:?} @ {signed}: mantissa {mantissa} outside [1, {band_hi})"
                    );

                    let expected = signed / rung.si_scale;
                    let reconstructed = mantissa * 10f64.powi(exponent);
                    assert!(
                        (reconstructed - expected).abs() <= 1e-12 * expected.abs(),
                        "{dim:?} @ {signed}: {mantissa}×10^{exponent} = {reconstructed} \
                         does not reproduce {expected}"
                    );
                }
            }
        }
    }

    /// §5e's rule is exhaustive for a default-ON dimension: every usable
    /// magnitude either hops to an in-band rung or falls back to engineering
    /// notation. There is no third outcome — an out-of-band plain magnitude
    /// would mean the policy silently gave up.
    ///
    /// "Usable" is the precondition this sweep actually relies on, and it is
    /// narrower than "finite and non-zero": the *quotient* `si_value /
    /// default_rung.si_scale` must also be finite. `magnitude_sweep(-12, 12)`
    /// stays well inside that range for every ladder here. The boundary itself
    /// — where the quotient overflows and the policy degrades to `Static` — is
    /// owned by `auto_scaled_degrades_to_static_when_the_quotient_overflows`
    /// above.
    ///
    /// The mirror half (default-OFF and `auto_scale: None` are `Static`
    /// everywhere) is owned by
    /// `auto_scaled_posture_gate_keeps_excluded_and_default_off_dims_static`
    /// above and deliberately not restated here.
    #[test]
    fn auto_scaled_default_on_dims_are_never_static_for_usable_magnitudes() {
        let ladders = unit_ladders();
        for l in default_on_ladders(&ladders) {
            for si_value in magnitude_sweep(-12, 12) {
                for signed in [si_value, -si_value] {
                    assert_ne!(
                        l.auto_scaled(signed),
                        AutoScaleChoice::Static,
                        "ladder {:?} gave up on si_value {signed}: §5e requires a rung hop \
                         or engineering notation, never a bare out-of-band magnitude",
                        l.dimension
                    );
                }
            }
        }
    }

    /// §5a's band is stated on `|mantissa|`, but the *rendered* mantissa keeps
    /// its sign — a negative magnitude must not lose it on the way through the
    /// engineering split.
    #[test]
    fn auto_scaled_engineering_notation_preserves_sign() {
        let ladders = unit_ladders();
        let area = ladder(&ladders, "Area");
        assert_eq!(
            area.auto_scaled(-0.5),
            AutoScaleChoice::Engineering {
                rung: unit(area, "mm^2"),
                mantissa: -500.0,
                exponent: 3,
            }
        );
    }

    /// The band edge is decided at *display* precision, not on the raw f64
    /// quotient — see [`BAND_EDGE_EPS`].
    ///
    /// Both cases are real registry magnitudes whose quotient lands a few ULPs
    /// under `band_hi` and so *renders* as the excluded `1000`:
    ///
    /// * `1e-6 m³ / 1e-9` (the `mm³` default rung) is `999.999_999_999_999_9`.
    ///   A raw `< 1000.0` test admits it and prints `1000 mm³`; the display
    ///   comparator rejects it and the one-hop `cm³` rung prints `1 cm³`.
    /// * `1000 m³ / 1e-9` is `999_999_999_999.999_9`, whose ×10⁹ mantissa is
    ///   likewise `999.999_999_999_999_9`. The engineering split must step on
    ///   to ×10¹² and snap its `0.999_999_999_999_999_9` mantissa to `1`.
    ///
    /// Regression lock for the two failures reify-ir's
    /// `resolve_display_none_honours_section5e_across_the_whole_registry`
    /// sweep surfaced: §5e admits no third outcome, and "renders `1000`" is
    /// out of band just as surely as "computes to `1000`".
    #[test]
    fn auto_scaled_band_edge_is_decided_at_display_precision() {
        let ladders = unit_ladders();
        let volume = ladder(&ladders, "Volume");

        assert!(
            (1e-6f64 / 1e-9).abs() < 1000.0,
            "premise: the raw quotient really is under the band's top edge"
        );
        assert_eq!(
            volume.auto_scaled(1e-6),
            AutoScaleChoice::Rung(unit(volume, "cm^3")),
            "1e-6 m³ renders as 1000 mm³, so mm³ is out of band and cm³ wins"
        );
        assert_eq!(
            volume.auto_scaled(-1e-6),
            AutoScaleChoice::Rung(unit(volume, "cm^3")),
            "the band is on |mantissa|, so the sign must not change the rung"
        );

        assert_eq!(
            volume.auto_scaled(1000.0),
            AutoScaleChoice::Engineering {
                rung: unit(volume, "mm^3"),
                mantissa: 1.0,
                exponent: 12,
            },
            "1000 m³ must normalize to 1×10¹² mm³, not 1000×10⁹ mm³"
        );
        assert_eq!(
            volume.auto_scaled(-1000.0),
            AutoScaleChoice::Engineering {
                rung: unit(volume, "mm^3"),
                mantissa: -1.0,
                exponent: 12,
            },
            "the lower-edge snap must preserve the mantissa's sign"
        );
    }

    /// Every curated ladder label is spelled in the ASCII `^`-exponent
    /// alphabet, never with the U+00B2/U+00B3 superscript glyphs (task λ,
    /// #5788; PRD contract C2 — "accept what we cannot enumerate, normalize
    /// what we curate").
    ///
    /// WHY this is a test rather than a convention: these labels are compared
    /// by raw string equality by three separate consumers — `@display`'s
    /// validator (reify-compiler/src/annotations/display.rs), the GUI's unit
    /// picker, and the ladder lookups in this very module. The ASCII spelling
    /// is also the only one the .ri grammar can parse, so a superscript label
    /// advertises a unit no user can type.
    ///
    /// The negative sweep alone would be satisfied by the tables becoming empty
    /// or renamed to something else entirely, so the eight relabelled spellings
    /// are ALSO pinned positively. `L` is included because it is the one Volume
    /// rung that was already ASCII, and #5788 declares the stdlib unit that
    /// finally makes it resolvable.
    #[test]
    fn curated_labels_use_ascii_exponent_alphabet() {
        let ladders = unit_ladders();

        // Negative sweep: no superscript exponent glyph anywhere in the tables.
        for l in &ladders {
            for bad in ['\u{00B2}', '\u{00B3}'] {
                assert!(
                    !l.derived_unit_name.contains(bad),
                    "ladder {:?} derived_unit_name {:?} must not contain {bad:?}",
                    l.dimension,
                    l.derived_unit_name
                );
                for u in &l.units {
                    assert!(
                        !u.label.contains(bad),
                        "ladder {:?} rung label {:?} must not contain {bad:?}",
                        l.dimension,
                        u.label
                    );
                }
            }
        }

        // Positive pins: the exact ASCII spellings, on their own ladders.
        let area = ladder(&ladders, "Area");
        for label in ["mm^2", "cm^2", "m^2"] {
            unit(area, label);
        }
        assert_eq!(area.derived_unit_name, "mm^2");

        let volume = ladder(&ladders, "Volume");
        for label in ["mm^3", "cm^3", "L", "m^3"] {
            unit(volume, label);
        }
        assert_eq!(volume.derived_unit_name, "mm^3");

        let density = ladder(&ladders, "Density");
        for label in ["kg/m^3", "g/cm^3"] {
            unit(density, label);
        }
        assert_eq!(density.derived_unit_name, "kg/m^3");
    }

    /// [`ascii_label_spelling`] and the tables it normalizes stay coupled.
    ///
    /// The normalizer is the rule; `unit_ladders()` is the data the rule was
    /// written against. Keeping them in one module is only worth anything if
    /// something fails when they drift, so this asserts both directions over
    /// the LIVE tables rather than a hand-copied label list:
    ///   * every curated label is already a fixed point (normalizing it yields
    ///     `None`) — this is the same statement as the negative sweep above,
    ///     but phrased through the normalizer, so re-introducing a superscript
    ///     rung fails here too;
    ///   * every relabelled rung is REACHED from its pre-λ superscript
    ///     spelling — synthesized by inverting the mapping on the live label,
    ///     so a rung renamed away from the caret spelling stops round-tripping
    ///     instead of silently leaving the hint path pointing at nothing.
    #[test]
    fn curated_labels_are_already_ascii_normalized() {
        let ladders = unit_ladders();
        let mut round_tripped = 0usize;

        for l in &ladders {
            for label in
                std::iter::once(&l.derived_unit_name).chain(l.units.iter().map(|u| &u.label))
            {
                assert_eq!(
                    ascii_label_spelling(label),
                    None,
                    "ladder {:?} label {label:?} is not already ASCII-normalized",
                    l.dimension
                );

                // Invert the mapping to reconstruct the pre-λ spelling; only
                // labels that actually carry a caret exponent have one.
                let superscript = label.replace("^2", "\u{00B2}").replace("^3", "\u{00B3}");
                if superscript == *label {
                    continue;
                }
                assert_eq!(
                    ascii_label_spelling(&superscript).as_deref(),
                    Some(label.as_str()),
                    "ladder {:?}: {superscript:?} must normalize back to {label:?}",
                    l.dimension
                );
                round_tripped += 1;
            }
        }

        // Anti-vacuity: the loop above is trivially satisfied by empty tables.
        assert!(
            round_tripped > 0,
            "no curated label carries a caret exponent — the round-trip half asserted nothing"
        );
    }
}
