# Reify stdlib-reference Gap Register

**Generated:** 2026-06-01 · **Source doc:** `docs/reify-stdlib-reference.md` (v0.1, dated 2026-03-13, marked *Draft*)
**Method:** 13-agent parallel survey cross-referencing every documented symbol against the `.ri` declarative layer (`crates/reify-compiler/stdlib/`) + Rust eval layer (`reify-stdlib`, `reify-eval`, kernel crates), then a task-corpus cross-reference (`search_tasks` over 4,115 filed tasks; 223 non-terminal).

## Executive summary

- **544 documented symbols surveyed** → 264 implemented (48%), 103 partial, 158 missing, 19 declared-only.
- **130 gaps** identified. Remediation buckets:
  - **A — Implement (102 gaps):** genuinely missing/partial features. → PRD clusters below.
  - **B — Doc-reconcile (22 gaps):** code deliberately diverged from the doc (intentional renames/minimizations). The doc is stale; fix is a documentation update, **not** code.
  - **C — Verify / phantom-done (6 gaps):** a task is marked `done` but the deliverable is **absent from main**. Confirmed for tasks **315/316/323** (chamfer_asymmetric, shell_open, offset_solid, split, per-edge fillet/chamfer, arbitrary_pattern rotation). These are real regressions — fold into the relevant implement-PRD and flag the phantom-done.
- **Tracking:** 72 untracked · 28 tracked-done (24 of which are doc-drift or phantom) · 18 tracked-partial · 12 tracked-open.

Ground-truth corroboration from source: `PrimitiveKind = {Box, Cylinder, Sphere, Tube}`, `ModifyKind = {Fillet, Chamfer, Shell, Draft, Thicken}`, `BooleanOp = {Union, Difference, Intersection}` (`crates/reify-compiler/src/types.rs:1068,1108,1090`). Everything else documented in §3.2-3.6 is unimplemented at the IR layer.

---

## Bucket A/C — Implementation gaps, by proposed PRD cluster

### G-A geometry-primitives (solids/2D/curves)  — 12 gaps (6 high)

| sev | symbol / gap | doc | type | tracked | evidence-summary |
|-----|--------------|-----|------|---------|------------------|
| high | cone primitive not implemented | §3.2 primitive L314 | missing_decl | tracked_open #4154(in-progress,0.41); #303(done,0.44) | cone() is a documented core solid primitive with no FFI, no PrimitiveKind, no IR op — fully unimplemented. |
| high | torus primitive not implemented | §3.2 primitive L316 | missing_decl | tracked_open #4154(in-progress,0.41); #303(done,0.49); #2574(done,0.49) | torus() documented core primitive is unimplemented end-to-end. |
| high | half_space primitive not implemented (doc-comment refers to nonexistent arm) | §3.2 primitive L318 | missing_decl | tracked_open #3465(pending,0.62); #3466(pending,0.53) | half_space() (the doc's headline unbounded-Solid example) is unimplemented; a doc-comment even cites a nonexistent enum arm. |
| high | rectangle 2D shape not implemented | §3.2 2D shapes L324 | missing_decl | untracked #322(done,0.48); #324(done,0.42) | rectangle() — a fundamental 2D profile constructor (needed to feed extrude/revolve) — does not exist. |
| high | circle 2D shape not implemented | §3.2 2D shapes L325 | missing_decl | untracked #324(done,0.41); #320(done,0.39) | circle() 2D profile constructor is unimplemented. |
| high | polygon 2D shape not implemented | §3.2 2D shapes L326 | missing_decl | untracked #324(done,0.42); #2574(done,0.40) | polygon() 2D profile constructor is unimplemented. |
| medium | box_centered primitive not implemented | §3.2 primitive L311 | missing_decl | tracked_open #4154(in-progress,0.41); #303(done,0.52); #2574(done,0.50) | Documented box_centered constructor does not exist in compiler, IR, or kernel. |
| medium | cylinder_centered primitive not implemented | §3.2 primitive L313 | missing_decl | tracked_open #4154(in-progress,0.41); #303(done,0.52); #2574(done,0.49) | Documented cylinder_centered constructor does not exist anywhere. |
| medium | wedge primitive not implemented | §3.2 primitive L317 | missing_decl | tracked_open #4154(in-progress,0.40); #303(done,0.46) | wedge() documented solid primitive does not exist. |
| medium | ellipse 2D shape not implemented | §3.2 2D shapes L327 | missing_decl | untracked #2914(done,0.43) | ellipse() 2D profile constructor is unimplemented. |
| medium | offset_curve (3 overloads) not implemented | §3.5 modify L374-376 | missing_decl | untracked #320(done,0.41) | All three offset_curve overloads are unimplemented. |
| low | nurbs_surface not implemented | §3.2 curves L339 | missing_decl | tracked_partial #320(done,0.45); #3621(done,0.40); #3615(done,0.40) | nurbs_surface() Surface constructor is unimplemented and even the doc signature is a placeholder. |

### G-B geometry-modify-and-sweep  — 12 gaps (0 high)

| sev | symbol / gap | doc | type | tracked | evidence-summary |
|-----|--------------|-----|------|---------|------------------|
| medium | fillet edge-selection overload missing (only all-edges form) | §3.5 modify L366 | partial | tracked_open #3205(deferred,0.53); #3295(pending,0.59) | fillet() works but only the all-edges form; the documented per-edge-list selection overload is unrealized. |
| medium | ⚠️PHANTOM chamfer edge-selection overload missing (only all-edges form) | §3.5 modify L368 | partial | tracked_done #315(done,0.49); #1728(done,0.45) | chamfer() works but only the all-edges form; the documented per-edge-list overload is unrealized. |
| medium | ⚠️PHANTOM offset_solid not implemented | §3.5 modify L372 | missing_decl | tracked_done #316(done,0.69) | offset_solid() does not exist. |
| medium | draft face-selection parameter missing | §3.5 modify L377 | partial | untracked #2830(done,0.48) | draft() works but lacks the documented per-face selection argument. |
| medium | ⚠️PHANTOM split not implemented | §3.5 modify L378 | missing_decl | tracked_done #316(done,0.51) | split() and its documented overloads do not exist. |
| medium | extrude_to not implemented | §3.6 sweep L387 | missing_decl | untracked #3466(pending,0.55); #322(done,0.51) | extrude_to(profile, target: Surface) does not exist. |
| medium | box() positional arg order swaps depth/height vs documented signature | §3.2 primitive L310 | partial | untracked #2991(done,0.44) | box() is implemented but its 2nd/3rd positional args map to height/depth, contradicting the documented (width, depth, height) order — a silent semantic mismatch. |
| low | fillet_all named function missing | §3.5 modify L367 | missing_decl | untracked #3295(pending,0.55); #2655(done,0.49) | Documented fillet_all() function name is not registered (its behavior is folded into fillet). |
| low | ⚠️PHANTOM chamfer_asymmetric not implemented | §3.5 modify L369 | missing_decl | tracked_done #315(done,0.53) | chamfer_asymmetric() does not exist. |
| low | ⚠️PHANTOM shell_open named function missing | §3.5 modify L371 | missing_decl | tracked_done #316(done,0.49) | shell_open() function name absent; partial coverage exists via shell()'s trailing args. |
| low | offset_surface not implemented | §3.5 modify L373 | missing_decl | untracked #316(done,0.33); #3056(done,0.44) | offset_surface() does not exist as a constructor. |
| low | thicken_asymmetric not implemented | §3.5 modify L380 | missing_decl | untracked #3009(done,0.44) | thicken_asymmetric() does not exist. |

### G-C geometry-frames-transform-query  — 16 gaps (3 high)

| sev | symbol / gap | doc | type | tracked | evidence-summary |
|-----|--------------|-----|------|---------|------------------|
| high | project() (both overloads) documented but unregistered | §3.1 geom_ctor L239-240 (docs/reify-stdlib-reference.md:239-240) | missing_decl | untracked #3535(done,0.39); #3902(done,0.39); #2583(done,0.39) | The frame-projection function project(point/vector, to: Frame) — both documented overloads — has no implementation anywhere; calls return Undef. |
| high | apply_transform(geometry, transform: Transform<3>) not registered as a stdlib fn | §3.7 transform L406 | missing_decl | tracked_partial #3963(pending,0.54); #3901(done,0.51) | The user-facing apply_transform stdlib function does not exist; calling it is a compile error despite kernel-level transform machinery existing. |
| high | plane_xy/plane_xz/plane_yz/axis_x/axis_y/axis_z constructors not implemented | §3.10 fns L527-532 | missing_decl | untracked #3116(deferred,0.46); #3230(done,0.39) | All six plane/axis constructor functions are entirely missing from the compiler and evaluator — they appear only as autocomplete strings, so Plane/Axis values are unreachable from Reify source. |
| medium | orient_look_at documented but never implemented | §3.1 geom_ctor L231 (docs/reify-stdlib-reference.md:231) | missing_decl | untracked #2956(done,0.47); #250(deferred,0.42); #2583(done,0.36) | orient_look_at(forward, up) is documented as a prelude constructor but is not registered in any eval module, so a call evaluates to Undef. |
| medium | EulerConvention enum absent; only lowercase strings accepted, uppercase variants rejected | §3.1 geom_ctor L229,242,260 (docs/reify-stdlib-reference.md:242) | missing_decl | untracked #3942(pending,0.40); #4108(done,0.39); #3936(done,0.38) | EulerConvention is documented as a real enum with uppercase variants, but no enum type exists and the eval path only accepts lowercase string spellings — the documented XYZ/XZY/etc. literals return Undef. |
| medium | rotate(geometry, orientation: Orientation<3>) overload not implemented | §3.7 transform L402 | missing_decl | untracked #3230(done,0.41); #1639(done,0.48) | The documented orientation-quaternion rotate overload has no compile or eval path; only the axis+angle form exists. |
| medium | scale(geometry, factors: Vector3<Real>) per-axis overload not implemented | §3.7 transform L405 | missing_decl | tracked_partial #3959(done,0.49); #3960(pending,0.42); #3963(pending,0.40) | Only uniform scale is wired; the documented per-axis (Vector3) non-rigid scale overload is absent at every layer. |
| medium | ⚠️PHANTOM arbitrary_pattern accepts translation triples, not full Transform<3> | §3.8 pattern L418 | partial | tracked_done #323(done,0.49) | arbitrary_pattern is wired but supports only per-instance translation, dropping the rotation half of the documented Transform<3> list. |
| medium | Plane structure not declared in .ri and not constructible from source | §3.10 structures L510-513 | declared_only_eval_undef | untracked #3116(deferred,0.51) | Plane exists as a built-in type/value but is unconstructible from Reify because its plane_* constructors are absent and no structure def is declared. |
| medium | Axis structure not declared in .ri and not constructible from source | §3.10 structures L515-518 | declared_only_eval_undef | untracked #3149(done,0.40); #3230(done,0.39) | Axis exists as a built-in type/value but is unconstructible from Reify; its axis_* constructors are absent. |
| low | trait Geometry never defined | §3.10 traits L500 | missing_decl | untracked #2297(done,0.58); #2312(done,0.61) | The documented Geometry supertrait does not exist; the bound is unenforced decoration. |
| low | trait Transformable never defined | §3.10 traits L501 | missing_decl | untracked #2297(done,0.50); #2312(done,0.49) | The documented Transformable supertrait does not exist; transform/pattern generic bounds are unenforced. |
| low | trait Closed is a marker with no conformance query | §3.10 traits L502 | declared_only_eval_undef | tracked_partial #2297(done,0.57); #2320(done,0.50); #2318(done,0.50) | Closed exists as a name-only marker; there is no runtime predicate that tests closedness. |
| low | trait Convex is a marker with no conformance query | §3.10 traits L505 | declared_only_eval_undef | tracked_partial #2318(done,0.54); #2315(done,0.54); #2312(done,0.52) | Convex is a name-only marker with no runtime convexity test. |
| low | trait Connected is a marker with no conformance query | §3.10 traits L506 | declared_only_eval_undef | tracked_partial #2550(done,0.50); #2312(done,0.47); #2318(done,0.45) | Connected is a name-only marker with no runtime connectivity test. |
| low | trait Bounded is a marker with no conformance query | §3.10 traits L507 | declared_only_eval_undef | tracked_partial #2312(done,0.52); #2315(done,0.51); #2318(done,0.49) | Bounded is a name-only marker with no runtime test. |

### P10 structural-traits  — 4 gaps (2 high)

| sev | symbol / gap | doc | type | tracked | evidence-summary |
|-----|--------------|-----|------|---------|------------------|
| high | Flexible.stiffness_model : Field<Point3<Length>,Tensor<2,3,Pressure>> does not exist | §4 std.structural L552 (Flexible) | missing_decl | untracked #2915(done,0.58); #2920(done,0.56); #4084(done,0.55) | The documented Field-of-stress-tensor stiffness_model param does not exist anywhere; the real trait has a scalar `stiffness` + `max_deflection`. |
| high | Plastic.yield_point : Pressure does not exist | §4 std.structural L557 (Plastic) | missing_decl | untracked #558(done,0.63); #2410(done,0.48); #3111(deferred,0.45) | Plastic's documented yield_point param does not exist; the real trait carries plastic_strain + hardening_modulus instead. |
| medium | Rigid.moment_of_inertia documented as auto-computed let, but .ri declares a plain Real param | §4 std.structural L548 (Rigid) | partial | tracked_partial #3114(deferred,0.52); #3620(done,0.52); #2476(done,0.48) | Rigid's moment_of_inertia is a user-supplied Real param in the actual stdlib, not the doc's auto-derived geometry-query let-binding (and the builtin returns a Tensor, not the doc-implied scalar). |
| medium | Sealed.seal_rating : Pressure does not exist (real member is seal_pressure_rating : Real) | §4 std.structural L563 (Sealed) | missing_decl | tracked_partial #3114(deferred,0.45); #3544(done,0.42) | Sealed's documented seal_rating:Pressure param does not exist; the real member is named seal_pressure_rating and typed Real. |

### P11 ports-breadth  — 4 gaps (1 high)

| sev | symbol / gap | doc | type | tracked | evidence-summary |
|-----|--------------|-----|------|---------|------------------|
| high | Entire ThreadSpec / ThreadSystem / ThreadClass / standards-derived-let cluster missing | §5.2 reify-stdlib-reference.md L621-636 | missing_decl | untracked #4022(done,0.35); #2331(done,0.35); #336(done,0.35) | The richest documented mechanical-port feature (ThreadSpec with standards-table-derived clearance_hole/tap_drill/minor/pitch_diameter, plus its three enums) is wholly unimplemented; ThreadedPort exposes only two raw Length params. |
| medium | RegionPort trait missing entirely | §5.1 reify-stdlib-reference.md L584 | missing_decl | untracked #4023(done,0.55); #4022(done,0.49); #249(done,0.47) | RegionPort (LocatedPort + region:Geometry) does not exist in any .ri. |
| medium | MotivePort, LinearPort, GuidePort, LinearGuidePort, RotaryGuidePort missing | §5.2 reify-stdlib-reference.md L638-660 | missing_decl | untracked #4022(done,0.45); #2676(done,0.41) | The entire motive/guide port hierarchy (5 traits, plus the dof==1 constraints and axis:Axis params) is absent; only RotaryPort partially survives with renamed params. |
| low | HydraulicPort multi-domain composition example unimplemented | §5.6 reify-stdlib-reference.md L718-720 | missing_decl | untracked #4023(done,0.38); #336(done,0.32) | The §5.6 multi-domain HydraulicPort example (and its FittingStandard enum) is not present in any stdlib .ri. |

### P12 materials-breadth  — 8 gaps (1 high)

> **STATUS 2026-06-03 — cluster CLOSED-by-design; do NOT re-PRD.** Owned by
> `docs/prds/v0_6/materials-parameter-surface-completion.md` (+ capability
> manifest), committed to main **2026-06-02** — one day *after* this register was
> generated, so the "entirely absent" / "required in .ri" / "grammar lacks undef"
> claims below were **already stale at generation**. Batch: **α #4239 done**
> (TemperatureDependent, Elastic ν-bound, optional `= undef` defaults — all on
> main), **β #4240 in-progress** (uts/elongation renames + FatigueRated /
> ImpactResistant param restore — impl on a branch, unmerged), **γ #4241 done**,
> **δ #4242 done**, **ε #4243 pending** (doc-reconcile; auto-unblocks when β
> lands). The dimensioned-type axis (gap 7) is a SEPARATE, wired follow-on, NOT
> this PRD's scope: **#3115 done** (composite aliases), **#3113 done** (optical
> →Length), **#3114 cancelled** (superseded by §4 reconcile #4227), **#3111** /
> **#3112 pending** (dep β/γ — rename-aware, retype the *new* names). The
> `= undef` premise is FALSE since **#3918** landed a first-class `undef` literal.
> Refractory `1500.0`→Temperature-literal fix rides **#3112**; its doc annotation
> rides ε **#4243**.

| sev | symbol / gap | doc | type | tracked | evidence-summary |
|-----|--------------|-----|------|---------|------------------|
| high | TemperatureDependent trait entirely absent | §6.1 Base L735-737 | missing_decl | tracked_done #4239(done) | RESOLVED — LANDED on main by α #4239 (`materials_mechanical.ri:47`, `param reference_temperature : Temperature = 293.15K`). The "entirely absent" claim was stale at generation. |
| medium | Elastic `0 < poissons_ratio < 0.5` constraint absent | §6.2 Elastic L747 | missing_decl | tracked_done #4239(done) | RESOLVED — LANDED on main by α #4239 (`materials_mechanical.ri:90`, chained constraint, both bounds enforced). |
| medium | FatigueRated params completely different from doc | §6.2 FatigueRated L760-764 | partial | tracked_open #4240(in-progress) | β #4240 restores `fatigue_limit`/`fatigue_strength_at`/`fatigue_cycles:Int` (drops the single `endurance_limit`); impl on a branch, unmerged. |
| medium | ImpactResistant charpy_impact/izod_impact absent | §6.2 ImpactResistant L772-774 | partial | tracked_open #4240(in-progress) | β #4240 splits the collapsed `impact_energy` into optional `charpy_impact`/`izod_impact`; impl on a branch, unmerged. |
| medium | Documented param names differ from .ri (uts, elongation) | §6.2 Strong L751 / Ductile L769 | missing_decl | tracked_open #4240(in-progress) | β #4240 hard-renames `uts`→`ultimate_tensile_strength` (+ constraint) and `elongation`→`elongation_at_break`, with atomic consumer migration; impl on a branch, unmerged. |
| medium | Documented `= undef` optional params are required in .ri | §6.2-§6.5 (shear_modulus, compressive_strength, reduction_of_area, dielectric_*, melting_point, etc.) | partial | tracked_done #4239/#4241/#4242(done); #3918(done) | RESOLVED (premise corrected) — `undef` is a first-class literal since #3918, so the "grammar lacks undef" claim is false. shear_modulus/compressive_strength/reduction_of_area made optional by α #4239; thermal/optical optionals by γ #4241; electrical optionals by δ #4242. The fatigue/charpy/izod optionals ride β #4240. |
| low | Dimensioned param types (Pressure/Density/Energy/Temperature/Length) replaced by Real | §6.1-§6.5 (density:Density, youngs_modulus:Pressure, melting_point:Temperature, charpy:Energy, reference_thickness:Length) | partial | tracked_open #3111(pending,dep β); #3112(pending,dep γ); #3113(done); #3115(done) | SEPARATE dimensioned-typing axis (not the names/optionality PRD). #3115 added composite aliases; #3113 tightened optical `reference_thickness`→Length (done); #3111 (mechanical→Pressure/Energy/Density) + #3112 (thermal→Temperature) pending, wired AFTER β/γ so they retype the renamed params. |
| low | Refractory threshold is 1500.0 (K-equiv Real), not 1500degC | §6.3 Refractory L793 | partial | tracked_open #3112(pending); #4243(pending); #2484(done) | Refractory `>= 1500.0` (K-equiv) → Temperature-literal fix (e.g. `1773K`) scoped into #3112; the doc annotation of the K-equiv placeholder rides ε #4243. Unchanged on main today. |

### P13 tolerancing  — 7 gaps (1 high)

| sev | symbol / gap | doc | type | tracked | evidence-summary |
|-----|--------------|-----|------|---------|------------------|
| high | require_finish() does not exist anywhere | §7.3 surface L939 | missing_decl | untracked #336(done,0.57); #4004(done,0.61) | The only documented free function in §7.3 (require_finish) is completely absent from both the .ri and Rust layers. |
| medium | ISOToleranceGrade tolerance_value standards-table lookup unimplemented | §7.1 dimensional L868-872 | declared_only_eval_undef | untracked #4004(done,0.42); #2651(done,0.45) | ISOToleranceGrade has no standards-table-driven tolerance computation — the value is just a passthrough param, and the documented Range<Length> field is missing. |
| medium | Conforms constraint does not perform MMC/LMC/RFS expansion | §7.2 geometric L921-924 | partial | untracked #2651(done,0.53); #3116(deferred,0.47); #4004(done,0.42) | The universal Conforms constraint is a trivial tolerance_value>0 check, not the GeometricTolerance-aware MMC/LMC/RFS expansion the doc promises. |
| medium | GeometricTolerance.nominal_zone let absent | §7.2 geometric L882 | missing_decl | untracked #3116(deferred,0.47); #2651(done,0.46) | The trait-level derived nominal_zone (the actual geometric tolerance zone) is documented but never declared or computed. |
| low | symmetric_tolerance/limit_tolerance return Length not DimensionalTolerance | §7.1 dimensional L856-857 | partial | untracked #2651(done,0.55); #2790(done,0.51); #2798(done,0.50) | Both documented constructor fns evaluate but return a bare Length scalar instead of the documented DimensionalTolerance structure. |
| low | Fit params are flat scalars, not nested DimensionalTolerance | §7.1 dimensional L859-865 | partial | untracked #2530(done,0.49); #2531(done,0.46); #3116(deferred,0.43) | Fit works but with a different (flat-scalar) field shape than documented — the documented nested DimensionalTolerance members are not accessible. |
| low | SurfaceFinish direction/process lack documented defaults | §7.3 surface L930-935 | partial | untracked #2830(done,0.39); #3116(deferred,0.34) | SurfaceFinish evaluates but direction/process are required rather than carrying the documented defaults, so the documented optional-param ergonomics are unrealized. |

### P14 process-dfm  — 3 gaps (1 high)

| sev | symbol / gap | doc | type | tracked | evidence-summary |
|-----|--------------|-----|------|---------|------------------|
| high | Process-category trait params documented but absent from .ri (empty bodies) | §8 std.process process.ri L956-985 | missing_decl | untracked #333(done,0.56); #4024(done,0.52) | Every documented parameter on the 7 process-category traits is missing — the .ri category traits are empty markers, so the documented DFM constraint surface does not exist. |
| medium | std.process is trait-surface-only — no DFM/process evaluation engine | §8 std.process process.ri L944-996 | declared_only_eval_undef | untracked #4024(done,0.51); #333(done,0.56) | All of std.process (Process + 7 category traits + DFMRule) compile-types only; nothing evaluates a process constraint or runs a DFM rule against geometry. |
| low | All documented '= undef' defaults dropped from io/process .ri | §8-9 io.ri/process.ri L948-1077 | partial | tracked_open #3918(pending,0.43); #3449(done,0.46) | Documented optional ('= undef') params are non-optional in the real .ri because the grammar lacks undef; conformers must supply every field. |

### P15 io-export-import  — 6 gaps (2 high)

| sev | symbol / gap | doc | type | tracked | evidence-summary |
|-----|--------------|-----|------|---------|------------------|
| high | All std.io.formats occurrences (STEPOutput/STLOutput/ThreeMFOutput/DisplayOutput/STEPInput/PointCloudInput) are missing | §9 std.io.formats io.ri L1042-1085 | missing_decl | untracked #5(done,0.43); #344(done,0.38) | The entire documented std.io.formats sub-module (6 occurrences + DisplayStyle structure + STEPVersion/PointCloudFormat enums) does not exist in any .ri file. |
| high | STL/3MF/Display export unimplemented — only STEP export runs | §9 std.io.formats io.ri L1039,1052,1057,1063 | partial | untracked #3905(pending,0.39); #344(done,0.38) | Of the documented STEP/STL/ThreeMF/Display output formats, only STEP actually exports; STL is declared in the Rust enum but rejected by every kernel, and 3MF/Display have no enum variant or code path. |
| medium | std.io Source/Sink/Buy/Discard/Provenance enums are trait-surface only | §9 std.io io.ri L1005-1040 | declared_only_eval_undef | untracked #2443(done,0.53) | The io trait/enum/structure surface is purely compile-time; no eval path reads Buy/Discard/Provenance fields or dispatches on OutputFormat. |
| medium | DSL Output/OutputFormat does not drive export — file extension does | §9 std.io io.ri L1020-1022,1039 | declared_only_eval_undef | untracked #5(done,0.36); #3905(pending,0.30) | The documented Output trait + OutputFormat enum are inert at eval: export format comes from the CLI output filename, so the DSL export-occurrence model is unwired. |
| medium | STEP/PointCloud import paths (STEPInput/PointCloudInput) do not exist | §9 std.io.formats io.ri L1075-1084 | missing_decl | untracked #2651(done,0.36); #2667(done,0.38); #3439(pending,0.39) | Geometry import (STEP, point cloud) is entirely absent — no import occurrences, no import kernel op, no PointCloud type; only an unverifiable Input tolerance-promise param exists. |
| low | Scalar<Money> documented type degrades to bare Money in .ri | §8-9 io.ri/process.ri L949,1016 | partial | untracked #2381(done,0.56); #2377(done,0.54); #3116(deferred,0.47) | Documented Scalar<Money>-typed params are actually bare Money in the .ri because Scalar<T> is not resolvable for arbitrary dimensions. |

### P16 fields-api  — 7 gaps (3 high)

| sev | symbol / gap | doc | type | tracked | evidence-summary |
|-----|--------------|-----|------|---------|------------------|
| high | constant_field is not implemented or declared | §11 std.fields.interpolation L1121 | missing_decl | untracked #4025(done,0.41); #3780(done,0.40) | constant_field has no declaration and no eval path — the documented field constructor does not exist. |
| high | fn_field is not implemented or declared | §11 std.fields.interpolation L1122 | missing_decl | untracked #3071(done,0.41); #1002(done,0.40); #2336(done,0.37) | fn_field has no declaration and no eval path. |
| high | from_samples is not implemented or declared | §11 std.fields.interpolation L1123 | missing_decl | untracked #2338(done,0.57); #2341(done,0.49); #2666(done,0.44) | from_samples has no declaration and no eval path despite the interp machinery existing internally. |
| medium | restrict is not implemented | §11 std.fields.spatial L1131 | missing_decl | untracked #2414(done,0.32) | restrict has no eval path — field-to-region restriction does not exist. |
| medium | clamp_field is not implemented or declared | §11 std.fields.spatial L1132 | missing_decl | untracked #2414(done,0.48); #3006(done,0.41) | clamp_field does not exist at any layer. |
| medium | remap_field is not implemented or declared | §11 std.fields.spatial L1133 | missing_decl | untracked #2414(done,0.41); #3006(done,0.38) | remap_field does not exist at any layer. |
| medium | threshold is not implemented or declared | §11 std.fields.spatial L1134 | missing_decl | untracked #2314(done,0.45) | threshold (Field -> Field<D,Bool>) does not exist at any layer. |

### P18 determinacy-intrinsics  — 5 gaps (4 high)

| sev | symbol / gap | doc | type | tracked | evidence-summary |
|-----|--------------|-----|------|---------|------------------|
| high | AllParamsDetermined constraint intrinsic does not exist | §12 std.determinacy reify-stdlib-reference.md L1162 | missing_decl | tracked_partial #4016(in-progress,0.51); #4019(pending,0.51); #2199(done,0.41) | The documented 'compiler intrinsic' AllParamsDetermined that walks all params is never implemented in either the .ri layer or compiler/eval; it does not exist as a callable constraint. |
| high | AllGeometryDetermined constraint intrinsic does not exist | §12 std.determinacy reify-stdlib-reference.md L1163 | missing_decl | tracked_partial #4016(in-progress,0.48); #4137(done,0.60); #4138(done,0.49) | AllGeometryDetermined is documented as a geometry-walking compiler intrinsic but has no .ri declaration and no compiler/eval implementation; it is fully absent from code. |
| high | std.determinacy.purposes.design_review is not defined in stdlib | §12 std.determinacy.purposes reify-stdlib-reference.md L1170-1172 | missing_decl | tracked_open #4016(in-progress,0.53); #4018(pending,0.48) | The promised stdlib purpose design_review has no .ri declaration anywhere; it exists only in documentation and depends on the missing AllParamsDetermined intrinsic. |
| high | std.determinacy.purposes.simulation_ready is a tautological user placeholder, not the documented stdlib purpose | §12 std.determinacy.purposes reify-stdlib-reference.md L1174-1177 | missing_decl | tracked_open #4016(in-progress,0.55); #4138(done,0.58); #4137(done,0.60) | simulation_ready is not provided by stdlib; the only working instance is a tautological example-file placeholder whose documented body (AllGeometryDetermined + determined(subject.material)) is uncompilable because both the intrinsic and subject-member access are unimplemented. |
| medium | RepresentationWithin only extracts a tolerance bound, never asserts geometry within tolerance | §12 std.determinacy reify-stdlib-reference.md L1164 | partial | untracked #2735(done,0.52); #2650(done,0.51); #2651(done,0.53) | RepresentationWithin is wired only as a tolerance-bound EXTRACTOR for the tolerance pipeline; the documented assert-geometry-within-tolerance constraint behavior is not realized. |

### P19 mechanism-completion  — 10 gaps (2 high)

| sev | symbol / gap | doc | type | tracked | evidence-summary |
|-----|--------------|-----|------|---------|------------------|
| high | Joint accessors axis/range/ratio/offset do not exist under documented names | §13.1 mechanism L1216-1221 | missing_decl | untracked #2686(done,0.54); #2675(done,0.54); #2632(done,0.52) | The four joint accessors are documented under names that return Undef; the real builtins are joint_-prefixed. |
| high | interference/clearance queries ignore FK world_transforms (geometry must be pre-positioned) | §13.5 mechanism L1320-1334 + §13.6 | partial | tracked_open #3524(pending,0.56); #3844(pending,0.44); #3906(in-progress,0.53) | Queries are OCCT-wired but FK-blind, so the documented sweep-a-path-and-check-interference workflow does not actually move bodies or run inside .map(). |
| medium | E_MECHANISM_DUPLICATE_SOLID diagnostic code never emitted (only Map error field) | §13.2 mechanism L1270 | partial | untracked #3029(done,0.50); #2528(done,0.47); #2929(pending,0.46) | Duplicate-solid is detected at the Map level but the documented typed diagnostic is not surfaced through the eval pipeline. |
| medium | §13.6 toolchanger example uses unsupported swept .map(interferes) pattern | §13.6 mechanism L1342-1363 | partial | tracked_partial #3848(pending,0.49); #2532(done,0.41); #2589(done,0.48) | The first worked example as written does not run; the actual acceptance test is a materially restricted version. |
| medium | MotionValue<J>, JointBinding, Twist, Axis type names are undeclared | §13.1/§13.3 mechanism L1246-1254, L1290-1293, L1227-1238 | missing_decl | tracked_partial #3845(done,0.53); #3884(cancelled,0.47); #3842(done,0.51) | Four documented type families/types have no declaration anywhere; the runtime uses untyped Maps/Vectors instead. |
| medium | trait Joint, DrivingJoint:Joint supertrait, and Coupling<P> generic are fiction | §13.1 mechanism L1193-1200 | partial | tracked_partial #3845(done,0.53); #3888(cancelled,0.52); #2527(done,0.51) | The documented Joint/DrivingJoint trait hierarchy and parametric Coupling<P> do not match the bare marker trait and fieldless struct that actually exist. |
| low | §13.6 counter-mass example uses unsupported .map/.windows/.norm/vector syntax | §13.6 mechanism L1367-1382 | partial | tracked_partial #2532(done,0.41); #3848(pending,0.49); #3994(pending,0.40) | The COM-stationarity example compiles only via a rewritten free-function form; the documented method-call syntax is not real Reify. |
| low | world is a builtin world(), not the documented pre-declared `let world : Joint` value | §13.2 mechanism L1261 | partial | untracked #2632(done,0.38); #3891(pending,0.38) | The documented ground-frame value usable as a bare `world` identifier does not exist; only the world() call form works. |
| low | bind() does not type-reject Coupling joints as the doc requires | §13.3 mechanism L1290-1293 | partial | untracked #2527(done,0.56); #2632(done,0.52); #3845(done,0.51) | bind() accepts coupling joints at runtime rather than rejecting them as the documented DrivingJoint constraint would. |
| low | center_of_mass/bounding_box use point-mass body origins, not volumetric geometry | §13.3 mechanism L1300-1304 | partial | untracked #2479(done,0.51); #2693(done,0.47); #3829(done,0.44) | Both accessors return values from body origin points rather than the documented full-geometry mass/extent, a v0.1 approximation. |

### P8 units-constants  — 9 gaps (0 high)

| sev | symbol / gap | doc | type | tracked | evidence-summary |
|-----|--------------|-----|------|---------|------------------|
| medium | Constant `e` does not exist | §2.4 constants L202 | missing_decl | untracked #1762(done,0.47); #4026(done,0.42) | Documented `let e : Real = 2.71828...` has no declaration and no eval path. |
| medium | Constant `avogadro` does not exist | §2.4 constants L206 | missing_decl | untracked #4026(done,0.47) | Documented `avogadro` constant is entirely absent from the implementation. |
| medium | Constant `planck` does not exist | §2.4 constants L207 | missing_decl | untracked #4026(done,0.47) | Documented `planck` constant is entirely absent from the implementation. |
| medium | Constant `stefan_boltzmann` does not exist | §2.4 constants L208 | missing_decl | untracked #4026(done,0.54) | Documented `stefan_boltzmann` constant is entirely absent. |
| medium | Constant `vacuum_permittivity` does not exist | §2.4 constants L209 | missing_decl | untracked #4026(done,0.47) | Documented `vacuum_permittivity` constant is entirely absent. |
| medium | Constant `vacuum_permeability` does not exist | §2.4 constants L210 | missing_decl | untracked #4026(done,0.47) | Documented `vacuum_permeability` constant is entirely absent. |
| medium | Constant `gas_constant` does not exist | §2.4 constants L211 | missing_decl | untracked #4026(done,0.47) | Documented `gas_constant` constant is entirely absent. |
| medium | Constant `elementary_charge` does not exist | §2.4 constants L212 | missing_decl | untracked #4026(done,0.47); #335(done,0.44) | Documented `elementary_charge` constant is entirely absent (value reachable only via the eV unit factor). |
| low | Dimension-alias count and cross-reference are wrong | §2.1 units L186 | partial | untracked #2438(done,0.47); #2440(done,0.44); #2402(done,0.43) | Doc undercounts the dimension table (48 not 34) and its 'Section 3.2' pointer is a stale/wrong cross-reference. |

### P9 math-linalg-completion  — 5 gaps (0 high)

| sev | symbol / gap | doc | type | tracked | evidence-summary |
|-----|--------------|-----|------|---------|------------------|
| medium | eigenvalues only supports N≤3; larger matrices and several 3×3 cases return Undef | §1.3 linalg L142 | partial | untracked #3451(done,0.31); #1727(done,0.32); #3795(done,0.33) | eigenvalues is implemented only for 1×1/2×2/3×3 with real eigenvalues; N>3, complex 2×2, and some degenerate 3×3 inputs silently return Undef rather than the documented List. |
| medium | determinant and inverse only support N≤3 | §1.3 linalg L137-138 | partial | untracked #3719(done,0.51); #3961(pending,0.39) | determinant<N,N,Q> and inverse<N,N,Q> are documented for any N but only evaluate for 1×1/2×2/3×3; 4×4 and larger return Undef. |
| medium | All §1 math/complex/linalg/matrix fns lack compiler signatures; return types not compile-checked | §1 std.math L88-179 (whole section) | partial | untracked #2149(done,0.47); #3700(done,0.48); #3962(pending,0.45) | The documented dimensional/generic return-type signatures for the math functions are realized only at eval time; at compile time these native calls default to the first argument's type, so the documented return types are not type-checked. |
| low | pow() function does not do dimensioned repeated-multiplication; that lives only in the ^ operator | §1.1 numeric L100, L111 | partial | tracked_partial #3805(done,0.47); #4106(done,0.52); #3954(done,0.46) | The documented dimensioned-integer-power behavior attributed to pow() is implemented in the `^` operator, not in the pow builtin, which is strictly dimensionless. |
| low | sqrt is not a compiler intrinsic; dimension-halving is eval-time only | §1.1 numeric L99, L111 | partial | untracked #3954(done,0.45); #3805(done,0.44) | The doc calls sqrt a 'compiler intrinsic that halves even exponents', but no compile-time intrinsic exists — halving happens only at eval time and the compile-time result type is the un-halved first-arg type. |

---

## Bucket B — Doc-reconciliation (code diverged intentionally; update the doc, not the code)

> **STATUS 2026-06-03 — full Bucket-B disposition pass (run LAST, after the implement-PRDs).**
> Re-verified all 22 rows against *current* main; the jun1 "reality" column was stale for
> most (the implement-PRDs reconciled their own doc sections). Disposition:
> - **Already reconciled in-doc:** §2 `g`/`c`/`boltzmann` fn-form — done by **#4177** (commit `01210ea966`).
> - **Owned by sibling implement-PRD doc tasks (pending; no new task):** §4 Flexible→**#4227** (structural-traits β); §6 Material→MaterialSpec / Elastic-Strong-Hard-Ductile free-standing / Insulating `determined()`→**#4243** (materials ε); §8 DFMRule params→**#4275** (process-DFM δ); §11 InterpolationMethod (internal Rust enum→Reify enum) + `compose` callable→**#4225** (fields η; code via #4221/#4224); §13 E_KINEMATIC_CLOSED_CHAIN dead + joint/snapshot type-tags→Map→**#4313** (mechanism θ).
> - **§5 std.ports (6 rows) — code-converging via P11** (`ports-breadth-expansion.md`, batch #4254/#4255/#4256/#4257/#4259/#4260): the doc already shows the rich form and P11 brings the *code* up to it, BUT P11 filed no doc-reconcile leaf and made deliberate residual deviations (Frame<3>→Frame3, SignalType→SignalKind, FluidPort Range→scalar+medium, Bore/Shaft/MatingFace/FitType not shipped). **New doc leaf #4314 filed**, gated on the whole P11 batch, to reconcile §5 + close these 6 rows.
> - **Reconciled directly this pass (commit `ec851cd86c`):** §10 `Analysis` (mesh_resolution/convergence_target→`yield_strength`), §10 `AnalysisResult` (source/mesh→von_mises/principal/max_shear/safety_factor scalars), §11 differential-operators `@optimized` fiction removed. These 3 were genuinely unowned pure doc edits (code shipped, no dependency).
> - **Reclassified — NOT a doc gap:** §3.7-3.10 curvature(surface)→Matrix<2,2>: the doc is already correct and eval/FFI ships (#3621); the residual is an unowned *compiler-signature* code gap (`units.rs:510` types it Scalar unconditionally) → **new Bucket-A task #4315** (arg-type-aware geometry-query return typing).

| section | doc claim | reality |
|---------|-----------|---------|
| §2 std.units | `g` constant exposed only as fn STANDARD_GRAVITY() | g is covered by done tasks 3647/4026 which deliberately implement it as the zero-arg fn STANDARD_GRAVITY() (Reify lacks top-level const), so the underlying value is tracked-and-done, but the doc-vs-impl FORM/NAME mismatch (doc shows `let g : Acceleration`) is itself not tracked as a defect to reconcile. |
| §2 std.units | `c` constant exposed only as fn SPEED_OF_LIGHT() | c is tracked and done via task 4026 (SPEED_OF_LIGHT()), deliberately implemented as a zero-arg fn per the STANDARD_GRAVITY idiom, but the doc-vs-impl name/form mismatch (doc shows `let c : Velocity`) is not tracked as a separate reconciliation defect. |
| §2 std.units | `boltzmann` constant exposed only as fn BOLTZMANN_CONSTANT() | boltzmann is tracked and done via task 4026 (BOLTZMANN_CONSTANT()), deliberately a zero-arg fn returning Energy/Temperature per the STANDARD_GRAVITY idiom, but the doc-vs-impl name/form mismatch (doc shows `let boltzmann : Energy/Temperature`) is not separately tracked. |
| §3.7-3.10 transform/pattern/query/traits | curvature(surface) -> Matrix<2,2> overload not type-registered | Task 3621 (done) implemented curvature(Surface) → Matrix<2,2> dispatch + FFI, but the doc still reports a type/runtime divergence (compiler types curvature() as Scalar<Curvature>), so the distinct Matrix overload's TYPE registration appears not fully delivered despite the task being done — that residual typing gap is untracked. |
| §4 std.structural | Flexible documented as `: Physical` but .ri trait is standalone | Tracked-done but contradictory: the standalone-Flexible (no `: Physical` edge) is the deliberate result of done tasks 2410/2349 (inheritance reconciliation), so the implementation gap is intentional — the residual issue is purely that the doc still claims `Flexible : Physical`, and no open task reconciles that doc text. |
| §5 std.ports | LocatedPort trait absent from stdlib though compiler expects it by name | The gap is covered only by done tasks that re-authored std.ports (4022) without ever shipping a LocatedPort trait, and by done task 370 which only polishes the compiler's LocatedPort frame-check — so the documented spatial-port trait remains unimplemented despite the done state; no open task tracks adding it. |
| §5 std.ports | Bore/Shaft/MechanicalPort are empty markers, not the documented parameterized traits | Done task 4022 is exactly the task that shipped Bore/Shaft/StructurePort as empty marker traits (its body and PRD §8 Q4 resolved them as bare markers), so the gap's partial state is the deliverable of a done task — the documented dimensional/load params and FitType enum are neither shipped nor tracked by any open task. |
| §5 std.ports | Electrical port params renamed/dropped; SignalType→SignalKind loses PWM; PinPort missing | Done task 4023 is the task that authored the electrical submodule with PowerPort/SignalPort and renamed/reduced params (no ElectricalPort voltage/current ratings, no PinPort, and SignalType→SignalKind PWM divergence) — so the gap's partial/divergent state is the shipped result of a done task, with no open task tracking restoration of the dropped params/PinPort/PWM. |
| §5 std.ports | Thermal port loses heat_flux/thermal_resistance; ThermalContactPort missing | Done task 4023 authored ThermalPort as a deliberately minimal heat_flow:Power interface (PRD §8 Q3 explicitly chose a minimal defensible set, dropping heat_flux/thermal_resistance) and never added ThermalContactPort — so the gap is the shipped outcome of a done task and the missing fields/trait are untracked by any open task. |
| §5 std.ports | FluidPort Range<T> params unrealizable; FluidType/PipedFluidPort/PipeConnectionType missing | Done task 4023 authored FluidPort with scalar pressure/flow_rate params (replacing the unrealizable Range-typed spec form, PRD §8 Q3) and shipped neither FluidType, PipedFluidPort, nor PipeConnectionType — so the divergence is a done task's deliberate deviation and the missing enums/traits are untracked. |
| §5 std.ports | Port.direction documented default Bidi not implemented | Done task 4022 authored the Port trait with `param direction : Directionality` and no default value, so the documented Bidi default is unimplemented as the direct outcome of a done task; no open task tracks adding the default. |
| §6 std.materials | Documented base `Material` trait does not exist (renamed to MaterialSpec) | The Material→MaterialSpec rename is an intentional, completed change (tasks 1876 and 2411 both done) — the gap is real only as a doc-vs-code naming discrepancy, not an open work item; the doc still showing the old `Material` trait symbol is the residual contradiction. **The doc annotation is now tracked: ε #4243 (pending) reconciles §6.1 to `MaterialSpec` + the canonical `Material` struct.** |
| §6 std.materials | Elastic/Strong/Hard/Ductile do not refine the base material trait | The trait-breadth audit (refreshed by done task 3487) explicitly classifies Elastic/Strong/Hard/Ductile staying free-standing as a deliberate 'DRIFT-by-design' v0.1 deviation (density/name flow via a `material : MaterialSpec` slot), so the gap is documented-and-accepted rather than an open remediation task; the doc-vs-implementation `: Material` contradiction remains. **Doc annotation tracked: ε #4243 (pending) marks §6.2 free-standing + cites #3487.** |
| §6 std.materials | Insulating `determined(dielectric_strength)` constraint dropped | The dropping of `determined(dielectric_strength)` on Insulating and its replacement by a weaker `dielectric_strength > 0.0` positivity bound was the deliberate, completed work of task 2484 (done) — the determined() predicate is unimplementable in the grammar, so this gap is a closed design decision, not open work. **Doc annotation tracked: ε #4243 (pending) updates §6.4 to `dielectric_strength > 0` (degrade-to-indeterminate when omitted, per δ #4242) + cites #2484.** |
| §8-9 process/io | DFMRule documented params (subject, process) absent; .ri has different params | tracked_done but contradictory — task 4024 (done) deliberately authored DFMRule with rule_name/severity/applicability-marker (NOT the documented subject/process params), so the done task IS the source of the param mismatch the doc flags; the mismatch itself is not separately tracked as a defect to reconcile. |
| §10-11 analysis/fields | InterpolationMethod is an internal Rust enum, not a Reify-language enum | The only match (task 2338, done) deliberately built InterpolationMethod as an INTERNAL Rust sampler module — it never aimed to expose a Reify-language enum, so the gap (no language-level enum) is unaddressed despite the done status; effectively untracked as a language-surface feature. |
| §10-11 analysis/fields | compose exists only as composed{} syntax, not as the documented callable fn | Task 2343 (done) wired the composed field source kind, but only the composed{} block syntax — no task adds a callable compose(f,g) function, so the documented callable form remains a gap despite the done match. |
| §10-11 analysis/fields | Analysis trait params do not match doc (mesh_resolution/convergence_target absent) | Task 341 (done) defined the Analysis trait, but it shipped the real (yield_strength) param surface — no task tracks reconciling the trait's params with the doc's fictional mesh_resolution/convergence_target, so the documented-vs-real mismatch is unaddressed. |
| §10-11 analysis/fields | AnalysisResult trait params do not match doc (source/mesh absent) | Task 341 (done) defined AnalysisResult with the real stress-result-scalar params; no task tracks the doc's fictional source/mesh params, so the documented-vs-real mismatch is unaddressed despite the done match. |
| §10-11 analysis/fields | '@optimized' annotation on differential operators is documentation fiction | The @optimized annotation IS real and wired (tasks 273/274/3377/1656, all done) for constraint_def and fn contexts, contradicting the 'documentation fiction' framing in general — but no task wires/documents it on differential operators specifically, so that narrow claim is untracked while the broader mechanism is done. |
| §13 mechanism | E_KINEMATIC_CLOSED_CHAIN is reserved-but-dead; closed chains no longer error | Directly tracked and DONE: task 2671 (done) deliberately replaced the E_KINEMATIC_CLOSED_CHAIN rejection with v0.2 loop-closure recording, which is exactly why the error code is now dead — the runtime change is complete; the only residue is stale documentation, so this is tracked_done with the contradiction being a doc-update need rather than missing code. |
| §13 mechanism | .ri joint/mechanism/snapshot structures are type-tags disconnected from the runtime Maps | Tracked but DONE-with-contradiction: 3845 (done) is the task that declared the nominal .ri mechanism/joint/snapshot structures, and its own escalation trail records that it was RELAXED to leave the runtime as untyped Value::Map (the typed-substrate premise was rejected) — so the very divergence this gap describes is a deliberately-accepted outcome of a completed task, with no open follow-up to reconcile the nominal type shapes with the runtime Maps. |
