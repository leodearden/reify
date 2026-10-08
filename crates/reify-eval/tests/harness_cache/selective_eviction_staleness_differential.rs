//! δ (#4731) selective-realization-eviction staleness differential (PRD
//! `docs/prds/v0_6/selective-realization-eviction.md` §6): keyed eviction
//! must never serve a stale handle and never discard a fresh one.
//!
//! # Two regimes, one variable
//!
//! A [`RegimePair`] drives two engines that differ ONLY in eviction regime:
//! the same module, the same purpose activations, the same `UnifiedDag`
//! scheduler, each over its own `MockGeometryKernel`.
//!
//! - `selective` takes the production edit path.
//! - `baseline` takes the same edit followed by the public
//!   `clear_realization_cache()` — exactly the pre-γ wholesale flush, since
//!   an edit never populates the realization cache.
//!
//! # What is compared
//!
//! **Content (i).** Every realization's served geometry must be the same in
//! both regimes, compared by [`canonical_shape`]: the producing kernel op with
//! every parent handle replaced by the parent's own canonical shape. Never by
//! `GeometryHandleId` — that is a per-session counter blind to content.
//!
//! **Hit/miss (ii).** A literal "the regimes agree" cannot hold: the baseline
//! misses every realization after any edit. The baseline is used as an ORACLE
//! instead (esc-6925-15): the selective regime must MISS a realization iff it
//! was not cache-resident before the edit (non-terminal, unnamed, no demanded
//! tolerance, or new) or the baseline's served shape for it changed across the
//! edit. Otherwise it must HIT. Observed HIT ⇔ the realization dispatched no
//! kernel op.
//!
//! **Baseline premise (iii).** Every realization dispatches in the baseline,
//! or the baseline is not the wholesale flush.
//!
//! Each scenario also pins its expected HIT set, so the oracle can never pass
//! vacuously by expecting a miss everywhere.
//!
//! The last test is not a regime pair: it pins the CLASSIFICATION the
//! selective regime evicts by, across a value a build writes between edits.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Arc, Mutex};

use reify_compiler::{CompiledGeometryOp, CompiledModule, PrimitiveKind};
use reify_constraints::SimpleConstraintChecker;
use reify_core::{ModulePath, RealizationNodeId, Type, ValueCellId};
use reify_eval::cache::NodeId;
use reify_eval::realization_cache::NO_OPTIONS;
use reify_eval::{BuildScheduler, Engine};
use reify_ir::{
    CompiledExpr, ExportFormat, GeometryHandleId, GeometryOp, KernelHandle, ReprKind, Value,
};
use reify_test_support::builders::{CompiledModuleBuilder, TopologyTemplateBuilder};
use reify_test_support::mocks::GeometryOpRecord;
use reify_test_support::{MockGeometryKernel, compile_source, manufacturing_purpose, mm};

type OpLog = Arc<Mutex<Vec<GeometryOpRecord>>>;

/// The recursive content render of the geometry behind `handle`. Panics on a
/// `GeometryOp` it does not cover, so a corpus extension can never silently
/// fall back to comparing ids.
fn canonical_shape(records: &[GeometryOpRecord], handle: GeometryHandleId) -> String {
    let record = records
        .iter()
        .find(|r| r.result_handle == handle)
        .unwrap_or_else(|| panic!("no recorded kernel op produced {handle:?}"));
    let parent = |h: &GeometryHandleId| canonical_shape(records, *h);
    match &record.op {
        GeometryOp::Box {
            width,
            height,
            depth,
        } => format!("box({}, {}, {})", si(width), si(height), si(depth)),
        GeometryOp::Cylinder { radius, height } => {
            format!("cylinder({}, {})", si(radius), si(height))
        }
        GeometryOp::Union { left, right } => format!("union({}, {})", parent(left), parent(right)),
        GeometryOp::Translate { target, dx, dy, dz } => {
            format!("translate({}, {dx}, {dy}, {dz})", parent(target))
        }
        other => panic!("canonical_shape does not cover {other:?}; extend it"),
    }
}

/// A numeric op argument by its SI value. Both regimes compile the same
/// module, so an argument's dimension cannot differ between them.
fn si(value: &Value) -> f64 {
    value
        .as_f64()
        .unwrap_or_else(|| panic!("non-numeric op argument {value:?}"))
}

/// What one regime served for one realization after a build.
#[derive(Debug, Clone)]
struct Served {
    shape: String,
    /// Kernel ops this realization dispatched in the build (0 = cache HIT).
    dispatched: usize,
    /// The realization cache holds exactly the handle this realization served.
    resident: bool,
}

/// `(entity, index)` of a realization: an ordered stand-in for
/// `RealizationNodeId`, which is not `Ord`.
type RealizationKey = (String, u32);

fn key_of(rid: &RealizationNodeId) -> RealizationKey {
    (rid.entity.clone(), rid.index)
}

type Observation = BTreeMap<RealizationKey, Served>;

/// One engine plus its kernel's op log.
struct Regime {
    engine: Engine,
    ops: OpLog,
}

impl Regime {
    fn new(module: &CompiledModule, activations: &[(&str, &str)]) -> Self {
        let kernel = MockGeometryKernel::new();
        let ops = kernel.operations_ref();
        let mut engine = Engine::new(Box::new(SimpleConstraintChecker), Some(Box::new(kernel)));
        engine.set_build_scheduler(BuildScheduler::UnifiedDag);
        engine.eval(module);
        for (purpose, entity) in activations {
            engine.activate_purpose(purpose, entity);
        }
        Self { engine, ops }
    }

    fn terminal_handle(&self, entity: &str, tol: f64) -> Option<KernelHandle> {
        self.engine
            .test_terminal_handle(entity, ReprKind::BRep, tol)
    }

    fn demanded_tol(&self, entity: &str) -> Option<f64> {
        self.engine.active_tolerance_for(entity)
    }

    /// Build, then read what every realization served. A realization with a
    /// geometry cell serves that cell's handle; one without (a template built
    /// directly, not from DSL) serves its entity's cached terminal.
    fn build_and_observe(&mut self, module: &CompiledModule) -> Observation {
        let built = self
            .engine
            .build_snapshot(module, ExportFormat::Step)
            .expect("build_snapshot needs a prior eval");
        let records = self.ops.lock().unwrap().clone();
        let tallies = self.engine.last_dispatch_count_by_realization();
        let graph = &self
            .engine
            .snapshot()
            .expect("a built engine has a snapshot")
            .graph;
        graph
            .realizations
            .iter()
            .map(|(rid, node)| {
                let tol = self.demanded_tol(&rid.entity);
                let cached = tol.and_then(|t| self.terminal_handle(&rid.entity, t));
                let handle = match &node.geometry_cell {
                    Some(cell) => match built.values.get(cell) {
                        Some(Value::GeometryHandle {
                            kernel_handle: Some(id),
                            ..
                        }) => *id,
                        other => panic!("{rid}: geometry cell {cell} holds {other:?}"),
                    },
                    None => {
                        cached
                            .unwrap_or_else(|| {
                                panic!("{rid}: no geometry cell and no cached terminal")
                            })
                            .id
                    }
                };
                let served = Served {
                    shape: canonical_shape(&records, handle),
                    dispatched: tallies.get(rid).copied().unwrap_or(0),
                    resident: cached.map(|h| h.id) == Some(handle),
                };
                (key_of(rid), served)
            })
            .collect()
    }
}

/// The per-realization verdict of one post-edit rebuild.
struct Rebuilt {
    selective: Observation,
    expected_hits: BTreeSet<RealizationKey>,
}

impl Rebuilt {
    fn hit_entities(&self) -> BTreeSet<String> {
        self.expected_hits
            .iter()
            .map(|(entity, _)| entity.clone())
            .collect()
    }
}

struct RegimePair {
    module: CompiledModule,
    selective: Regime,
    baseline: Regime,
    selective_before: Observation,
    baseline_before: Observation,
}

impl RegimePair {
    /// Appends one manufacturing purpose per `(purpose, entity, tol)` to
    /// `module`, activates each for its entity in both regimes, and runs the
    /// cold build.
    fn new(mut module: CompiledModule, purposes: &[(&str, &str, f64)]) -> Self {
        for (purpose, _, tol) in purposes {
            module
                .compiled_purposes
                .push(manufacturing_purpose(purpose, *tol));
        }
        let activations: Vec<_> = purposes.iter().map(|(p, e, _)| (*p, *e)).collect();
        let mut selective = Regime::new(&module, &activations);
        let mut baseline = Regime::new(&module, &activations);
        let selective_before = selective.build_and_observe(&module);
        let baseline_before = baseline.build_and_observe(&module);
        Self {
            module,
            selective,
            baseline,
            selective_before,
            baseline_before,
        }
    }

    /// Premise lock: each named entity's terminal is cache-resident in the
    /// selective regime, so a HIT expectation on it is meaningful.
    fn assert_resident(&self, entities: &[&str]) {
        for entity in entities {
            assert!(
                self.selective_before
                    .iter()
                    .any(|((e, _), s)| e == entity && s.resident),
                "premise: {entity} must be cache-resident before the edit; observed {:#?}",
                self.selective_before
            );
        }
    }

    /// Runs `step` on both engines, outside any edit (e.g. a purpose
    /// activation), then rebuilds both without the oracle.
    fn both_then_build(&mut self, step: impl Fn(&mut Engine)) {
        step(&mut self.selective.engine);
        step(&mut self.baseline.engine);
        self.selective_before = self.selective.build_and_observe(&self.module);
        self.baseline_before = self.baseline.build_and_observe(&self.module);
    }

    /// Applies the identical edit to both engines; the baseline then takes
    /// the wholesale flush.
    fn edit(&mut self, edit: impl Fn(&mut Engine)) {
        edit(&mut self.selective.engine);
        edit(&mut self.baseline.engine);
        self.baseline.engine.clear_realization_cache();
    }

    fn edit_param(&mut self, entity: &str, member: &str, value: Value) {
        self.edit(|engine| {
            engine
                .edit_param(ValueCellId::new(entity, member), value.clone())
                .unwrap_or_else(|e| panic!("edit_param({entity}.{member}) failed: {e:?}"));
        });
    }

    fn edit_source(&mut self, v2: &CompiledModule) {
        let mut v2 = v2.clone();
        v2.compiled_purposes = self.module.compiled_purposes.clone();
        self.edit(|engine| {
            engine
                .edit_source(&v2)
                .unwrap_or_else(|e| panic!("edit_source failed: {e:?}"));
        });
        self.module = v2;
    }

    /// Rebuilds both regimes and enforces (i), (ii) and (iii) for every
    /// realization; every violation is reported together with the full table.
    fn rebuild(&mut self) -> Rebuilt {
        let selective = self.selective.build_and_observe(&self.module);
        let baseline = self.baseline.build_and_observe(&self.module);
        assert_eq!(
            selective.keys().collect::<Vec<_>>(),
            baseline.keys().collect::<Vec<_>>(),
            "both regimes must hold the same realizations"
        );

        let mut violations = Vec::new();
        let mut expected_hits = BTreeSet::new();
        for (rid, sel) in &selective {
            let base = &baseline[rid];
            if sel.shape != base.shape {
                violations.push(format!(
                    "(i) {rid:?}: selective serves {} but the baseline serves {}",
                    sel.shape, base.shape
                ));
            }
            if base.dispatched == 0 {
                violations.push(format!("(iii) {rid:?}: the baseline did not re-dispatch"));
            }
            let was_resident = self.selective_before.get(rid).is_some_and(|s| s.resident);
            let content_moved = self
                .baseline_before
                .get(rid)
                .is_none_or(|before| before.shape != base.shape);
            let expect_miss = !was_resident || content_moved;
            if !expect_miss {
                expected_hits.insert(rid.clone());
            }
            let observed_miss = sel.dispatched > 0;
            if expect_miss != observed_miss {
                violations.push(format!(
                    "(ii) {rid:?}: expected {} but observed {} (resident before: {was_resident}, \
                     content moved: {content_moved}, dispatched: {})",
                    if expect_miss { "MISS" } else { "HIT" },
                    if observed_miss { "MISS" } else { "HIT" },
                    sel.dispatched
                ));
            }
        }
        assert!(
            violations.is_empty(),
            "staleness differential violated:\n  {}\nselective before: {:#?}\n\
             selective after: {:#?}\nbaseline after: {:#?}",
            violations.join("\n  "),
            self.selective_before,
            selective,
            baseline
        );

        self.selective_before = selective.clone();
        self.baseline_before = baseline;
        Rebuilt {
            selective,
            expected_hits,
        }
    }
}

fn entities(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|n| n.to_string()).collect()
}

// ── Fixtures ────────────────────────────────────────────────────────────────

fn mm_lit(v: f64) -> CompiledExpr {
    CompiledExpr::literal(mm(v), Type::length())
}

/// A template `name` whose named realization is a Box reading `<name>.w`;
/// `<name>.label` is read by nothing. Entity == template name, as `build()`'s
/// schedule filter requires. No geometry cell: the served handle is the
/// entity's cached terminal.
fn body_template(name: &str, width_mm: f64) -> reify_compiler::TopologyTemplate {
    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            (
                "width".into(),
                CompiledExpr::value_ref(ValueCellId::new(name, "w"), Type::length()),
            ),
            ("height".into(), mm_lit(20.0)),
            ("depth".into(), mm_lit(5.0)),
        ],
    };
    TopologyTemplateBuilder::new(name)
        .param(name, "w", Type::length(), Some(mm_lit(width_mm)))
        .param(name, "label", Type::dimensionless_scalar(), None)
        .realization_named(name, 0, "body", vec![box_op])
        .build()
}

fn part_a_part_b_module() -> CompiledModule {
    CompiledModuleBuilder::new(ModulePath::new(vec!["staleness_differential".to_string()]))
        .template(body_template("PartA", 10.0))
        .template(body_template("PartB", 20.0))
        .build()
}

const PART_PURPOSES: &[(&str, &str, f64)] = &[("mfg_a", "PartA", 1e-6), ("mfg_b", "PartB", 1e-6)];

fn part_pair() -> RegimePair {
    let pair = RegimePair::new(part_a_part_b_module(), PART_PURPOSES);
    pair.assert_resident(&["PartA", "PartB"]);
    pair
}

// ── Scenarios ───────────────────────────────────────────────────────────────

#[test]
fn param_edit_feeding_body_a_only() {
    let mut pair = part_pair();

    pair.edit_param("PartA", "w", mm(30.0));
    let rebuilt = pair.rebuild();

    assert_eq!(rebuilt.hit_entities(), entities(&["PartB"]));
}

/// PRD §6 zero-eviction row.
#[test]
fn no_realization_edit() {
    let mut pair = part_pair();
    let len_before = pair.selective.engine.realization_cache().len();

    pair.edit_param("PartA", "label", Value::Real(0.5));
    assert!(
        pair.selective.engine.last_changed_realizations().is_empty(),
        "a display-only edit moves no realization's input cone"
    );
    assert_eq!(
        pair.selective.engine.realization_cache().len(),
        len_before,
        "a display-only edit must evict nothing"
    );
    let rebuilt = pair.rebuild();

    assert_eq!(rebuilt.hit_entities(), entities(&["PartA", "PartB"]));
}

/// Flipping a guard moves only bodies whose ARGS read a guard-dependent
/// value: `G.a` reads `w`, whose value comes from whichever arm is active.
#[test]
fn guard_flip_moves_only_the_body_reading_the_guarded_value() {
    const SRC: &str = r#"pub structure G {
    param flag : Bool = true
    where flag { let w = 10mm } else { let w = 20mm }
    let a = box(w, 5mm, 5mm)
}
pub structure U {
    let b = box(7mm, 5mm, 5mm)
}"#;
    let mut pair = RegimePair::new(
        compile_source(SRC),
        &[("mfg_g", "G", 1e-6), ("mfg_u", "U", 1e-6)],
    );
    pair.assert_resident(&["G", "U"]);

    pair.edit_param("G", "flag", Value::Bool(false));
    let rebuilt = pair.rebuild();

    assert_eq!(rebuilt.hit_entities(), entities(&["U"]));
}

/// A collection grow re-elaborates value cells and adds no realization: the
/// body reading the collection aggregate misses, the bystander hits, and the
/// realization set is unchanged (nothing orphaned).
#[test]
fn collection_grow_moves_only_the_body_reading_the_aggregate() {
    const SRC: &str = r#"structure BoltPart {
    param diameter : Length = 5mm
}
pub structure Rack {
    param n : Int = 2
    sub bolts : List<BoltPart>
    constraint bolts.count == n
    let span = 10mm * bolts.count
    let a = box(span, 5mm, 5mm)
}
pub structure Bystander {
    let b = box(7mm, 5mm, 5mm)
}"#;
    let mut pair = RegimePair::new(
        compile_source(SRC),
        &[("mfg_r", "Rack", 1e-6), ("mfg_b", "Bystander", 1e-6)],
    );
    pair.assert_resident(&["Rack", "Bystander"]);
    let realizations_before: Vec<_> = pair.selective_before.keys().cloned().collect();

    pair.edit_param("Rack", "n", Value::Int(3));
    let rebuilt = pair.rebuild();

    assert_eq!(rebuilt.hit_entities(), entities(&["Bystander"]));
    assert_eq!(
        rebuilt.selective.keys().cloned().collect::<Vec<_>>(),
        realizations_before,
        "a grow adds and removes no realization"
    );
}

/// PRD D7. `Morph` changes a literal, `Keep` is byte-identical, and `Shrink`
/// drops its LAST realization: its surviving realization becomes the
/// entity's terminal, and the entity's cached terminal (the dropped body's
/// geometry) must not be served for it.
#[test]
fn edit_source_recompile_changes_one_body_keeps_the_identical_one() {
    const V1: &str = r#"pub structure Morph {
    let body = box(10mm, 5mm, 5mm)
}
pub structure Keep {
    let body = box(7mm, 5mm, 5mm)
}
pub structure Shrink {
    let a = box(3mm, 3mm, 3mm)
    let b = box(4mm, 4mm, 4mm)
}"#;
    const V2: &str = r#"pub structure Morph {
    let body = box(10mm, 5mm, 9mm)
}
pub structure Keep {
    let body = box(7mm, 5mm, 5mm)
}
pub structure Shrink {
    let a = box(3mm, 3mm, 3mm)
}"#;
    let mut pair = RegimePair::new(
        compile_source(V1),
        &[
            ("mfg_m", "Morph", 1e-6),
            ("mfg_k", "Keep", 1e-6),
            ("mfg_s", "Shrink", 1e-6),
        ],
    );
    pair.assert_resident(&["Morph", "Keep", "Shrink"]);

    pair.edit_source(&compile_source(V2));
    assert_eq!(
        pair.selective.terminal_handle("Shrink", 1e-6),
        None,
        "a dropped realization must leave no stale terminal for its entity"
    );
    let rebuilt = pair.rebuild();

    assert_eq!(rebuilt.hit_entities(), entities(&["Keep"]));
}

/// PRD D4/§6 "tolerance interplay": the survivor is cached at two
/// tolerances; an edit of the other body leaves the survivor's partial order
/// intact and removes only the edited body's family.
#[test]
fn tolerance_interplay_survivors_keep_partial_order_lookup() {
    let mut module = part_a_part_b_module();
    module
        .compiled_purposes
        .push(manufacturing_purpose("mfg_b_tight", 1e-6));
    let mut pair = RegimePair::new(
        module,
        &[("mfg_a", "PartA", 1e-6), ("mfg_b_loose", "PartB", 1e-4)],
    );
    // A tighter demand misses the loose entry, so PartB is realized again and
    // cached at BOTH tolerances.
    pair.both_then_build(|engine| engine.activate_purpose("mfg_b_tight", "PartB"));
    pair.assert_resident(&["PartA", "PartB"]);
    let survivor = |pair: &RegimePair, tol: f64| pair.selective.terminal_handle("PartB", tol);
    let survivor_len = |pair: &RegimePair| {
        pair.selective
            .engine
            .realization_cache()
            .bucket_len("PartB", ReprKind::BRep, NO_OPTIONS)
    };
    let (tight, loose) = (survivor(&pair, 1e-6), survivor(&pair, 1e-4));
    assert!(
        tight.is_some() && loose.is_some() && tight != loose && survivor_len(&pair) == 2,
        "premise: PartB is cached at 1e-6 and at 1e-4 as two entries"
    );

    pair.edit_param("PartA", "w", mm(30.0));
    assert_eq!(pair.selective.terminal_handle("PartA", 1e-6), None);
    assert_eq!(
        survivor(&pair, 1e-5),
        tight,
        "the tighter survivor entry satisfies a looser request"
    );
    assert_eq!(
        survivor(&pair, 1e-3),
        loose,
        "the loosest satisfying entry is served"
    );
    assert_eq!(survivor(&pair, 1e-7), None);
    assert_eq!(survivor_len(&pair), 2, "the survivor's bucket is untouched");
    let rebuilt = pair.rebuild();

    assert_eq!(rebuilt.hit_entities(), entities(&["PartB"]));
}

/// The 4317-class trap: `C.combined = union(self.a.body, self.b.body)`. An
/// edit of `A.w` never moves C's own input-cone fold (it sees no Sub
/// operand), yet C's served content changes. `A` and `B` both export `body`,
/// so the reverse index cannot link C's operands at all: C must be seeded
/// into the dirty cone, which also invalidates what is downstream of C.
#[test]
fn sub_consumer_of_the_edited_body_is_evicted() {
    const SRC: &str = r#"pub structure A {
    param w : Length = 10mm
    let body = box(w, 10mm, 10mm)
}
pub structure B {
    let body = cylinder(5mm, 10mm)
}
pub structure C {
    sub a = A()
    sub b = B()
    let combined = union(self.a.body, self.b.body)
    let v = volume(combined)
}"#;
    let mut pair = RegimePair::new(
        compile_source(SRC),
        &[
            ("mfg_a", "A", 1e-6),
            ("mfg_b", "B", 1e-6),
            ("mfg_c", "C", 1e-6),
        ],
    );
    pair.assert_resident(&["A", "B", "C"]);
    // An entry's presence, not its value: cross-sub geometry is `Undef` until
    // a build, and `CacheStore::invalidate` is what removes an entry.
    let cached = |pair: &RegimePair, entity: &str, member: &str| {
        let cell = NodeId::Value(ValueCellId::new(entity, member));
        pair.selective.engine.cache_store().get(&cell).is_some()
    };
    let downstream_of_c = [("C", "combined"), ("C", "v")];
    for (entity, member) in downstream_of_c.into_iter().chain([("B", "body")]) {
        assert!(
            cached(&pair, entity, member),
            "premise: {entity}.{member} has a cache entry before the edit"
        );
    }

    pair.edit_param("A", "w", mm(30.0));
    for (entity, member) in downstream_of_c {
        assert!(
            !cached(&pair, entity, member),
            "{entity}.{member} is downstream of the edited body through C: the edit \
             must invalidate it"
        );
    }
    assert!(cached(&pair, "B", "body"), "B.body is not downstream of A");
    let rebuilt = pair.rebuild();

    assert_eq!(rebuilt.hit_entities(), entities(&["B"]));
}

/// Two cross-entity consumers, each reading a member exactly one entity
/// exports, so the reverse index links each to its producer. An edit of `A`
/// re-dispatches `A` and its consumer `Moved` only; `Placed` reads the
/// untouched `S` and is served from the cache.
#[test]
fn an_edit_redispatches_only_the_sub_consumer_of_the_edited_part() {
    const SRC: &str = r#"pub structure A {
    param w : Length = 10mm
    let body = box(w, 10mm, 10mm)
}
pub structure S {
    let shell = cylinder(5mm, 10mm)
}
pub structure Moved {
    sub a = A()
    let moved = union(self.a.body, self.a.body)
}
pub structure Placed {
    sub s = S()
    let placed = union(self.s.shell, self.s.shell)
}"#;
    let mut pair = RegimePair::new(
        compile_source(SRC),
        &[
            ("mfg_a", "A", 1e-6),
            ("mfg_s", "S", 1e-6),
            ("mfg_m", "Moved", 1e-6),
            ("mfg_p", "Placed", 1e-6),
        ],
    );
    pair.assert_resident(&["A", "S", "Moved", "Placed"]);

    pair.edit_param("A", "w", mm(30.0));
    let rebuilt = pair.rebuild();

    assert_eq!(rebuilt.hit_entities(), entities(&["S", "Placed"]));
}

/// β's changed set is cumulative until a build re-executes: a display-only
/// edit after a real one must not make the edited body look fresh.
#[test]
fn repeated_edits_without_build_stay_stale_until_rebuilt() {
    let mut pair = part_pair();

    pair.edit_param("PartA", "w", mm(30.0));
    pair.edit_param("PartA", "label", Value::Real(0.5));
    assert!(
        pair.selective
            .engine
            .last_changed_realizations()
            .contains(&RealizationNodeId::new("PartA", 0)),
        "PartA stays changed until a build re-executes it"
    );
    let rebuilt = pair.rebuild();

    assert_eq!(rebuilt.hit_entities(), entities(&["PartB"]));
}

// ── A value a build writes between edits ────────────────────────────────────

/// `edit_param` folds only realizations whose read cells moved (#6086). A
/// selective-demand tessellate refreshes the demanded `let` `sb`
/// (`refresh_and_gate_demanded_realizations`) without re-stamping `b`, which
/// reads `sb` but is not demanded. The next edit moves an unrelated param and
/// leaves `sb` alone, yet `b`'s stored hash predates the refresh, so `b` is
/// changed — exactly what the full fold reports.
#[test]
fn a_realization_reading_a_build_refreshed_let_is_changed_by_an_unrelated_edit() {
    const SRC: &str = r#"pub structure Refreshed {
    param w : Length = 10mm
    param pad : Length = 1mm
    let sa = w * 3
    let sb = w * 2
    let a = box(sa, sa, sa)
    let b = box(sb, sb, sb)
}"#;
    let compiled = compile_source(SRC);
    let body_a = NodeId::Realization(RealizationNodeId::new("Refreshed", 0));
    let rid_b = RealizationNodeId::new("Refreshed", 1);
    let body_b = NodeId::Realization(rid_b.clone());
    let sb = ValueCellId::new("Refreshed", "sb");
    let sb_value = |engine: &Engine| {
        let snapshot = engine
            .snapshot()
            .expect("an evaluated engine has a snapshot");
        snapshot.values.get(&sb).map(|(value, _)| value.clone())
    };
    let stamp_of_b = |engine: &Engine| {
        let snapshot = engine
            .snapshot()
            .expect("an evaluated engine has a snapshot");
        snapshot
            .graph
            .realizations
            .get(&rid_b)
            .and_then(|node| node.input_cone_hash)
    };
    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    );
    engine.set_build_scheduler(BuildScheduler::UnifiedDag);
    engine.eval(&compiled);
    engine.set_demand_selective([body_a.clone(), body_b.clone()]);
    engine
        .tessellate_snapshot(&compiled)
        .expect("tessellate_snapshot needs a prior eval");
    let b_stamped = stamp_of_b(&engine);
    assert!(b_stamped.is_some(), "premise: the tessellate stamps b");

    engine.set_demand_selective([body_a.clone()]);
    let sb_at_stamp = sb_value(&engine);
    engine
        .edit_param(ValueCellId::new("Refreshed", "w"), mm(20.0))
        .expect("edit_param(w) must succeed");
    assert_eq!(
        sb_value(&engine),
        sb_at_stamp,
        "premise: a hidden `sb` is not re-evaluated by the edit"
    );

    engine.set_demand_selective([body_a, NodeId::Value(sb.clone())]);
    assert!(
        !engine.demand_is_demanded(&body_b),
        "premise: demanding `sb` does not demand its reader b"
    );
    engine
        .tessellate_snapshot(&compiled)
        .expect("tessellate_snapshot needs a prior eval");
    assert_ne!(
        sb_value(&engine),
        sb_at_stamp,
        "premise: the tessellate refreshed `sb`"
    );
    assert_eq!(
        stamp_of_b(&engine),
        b_stamped,
        "premise: the tessellate did not re-stamp b"
    );

    engine
        .edit_param(ValueCellId::new("Refreshed", "pad"), mm(2.0))
        .expect("edit_param(pad) must succeed");

    assert_eq!(
        engine.last_changed_realizations(),
        &HashSet::from([rid_b]),
        "b's stored hash predates the refreshed `sb`, so the unrelated edit must report it"
    );
}
