//! TEMP empirical probe for task 4832 — NOT COMMITTED, delete after run.
//! Realizes fixtures via real OCCT, dumps topology-attribute-table stats +
//! feature()-cell resolutions + created_by/split_by counts. All tests always
//! pass (pure eprintln reporting); run with --nocapture.
#![allow(clippy::all)]

use reify_core::{ModulePath, Severity};
use reify_ir::{ExportFormat, FeatureId, GeometryHandleId, Role, TopologyAttributeTable, Value};
use std::collections::{BTreeMap, HashSet};

fn role_is_face(r: Role) -> bool {
    matches!(
        r,
        Role::Cap(_)
            | Role::Side
            | Role::RevolvedFace
            | Role::AxisFace
            | Role::SweptFace
            | Role::LoftedFace
            | Role::MidSurfaceFace
    )
}

/// parse(+stdlib) -> compile(+stdlib) -> Engine::build with a DIRECT OCCT
/// kernel handle (so extract_faces is forwarded and the attribute table is
/// populated). Does NOT panic on compile/build errors — prints them and (for
/// build errors) continues so the table can still be inspected. Returns None
/// on OCCT-unavailable or parse/compile errors.
fn build(source: &str) -> Option<(reify_eval::Engine, reify_eval::BuildResult)> {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return None;
    }
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("probe"));
    if !parsed.errors.is_empty() {
        eprintln!("PARSE ERRORS: {:#?}", parsed.errors);
        return None;
    }
    let compiled = reify_compiler::compile_with_stdlib(&parsed);
    let cerr: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    if !cerr.is_empty() {
        eprintln!("COMPILE ERRORS ({}): {:#?}", cerr.len(), cerr);
        return None;
    }
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(
        Box::new(checker),
        Some(Box::new(reify_kernel_occt::OcctKernelHandle::spawn())),
    );
    let result = engine.build(&compiled, ExportFormat::Step);
    let berr: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    if !berr.is_empty() {
        eprintln!("BUILD ERRORS ({}): {:#?}", berr.len(), berr);
    } else {
        eprintln!("build: no error diagnostics");
    }
    eprintln!(
        "geometry_output = {} bytes",
        result
            .geometry_output
            .as_ref()
            .map(|v| v.len())
            .unwrap_or(0)
    );
    Some((engine, result))
}

/// Harvest every value cell that resolved to Value::Feature, keyed by member
/// name (fb / fg / fa / b ...). Robust to the exact structure scope name.
fn feature_cells(result: &reify_eval::BuildResult) -> BTreeMap<String, FeatureId> {
    let mut m = BTreeMap::new();
    for (id, v) in result.values.iter() {
        if let Value::Feature(fid) = v {
            m.insert(id.member.clone(), fid.clone());
        }
    }
    m
}

fn created_by(table: &TopologyAttributeTable, fid: &FeatureId) -> HashSet<GeometryHandleId> {
    table
        .iter()
        .filter(|(_, a)| role_is_face(a.role) && &a.feature_id == fid)
        .map(|(id, _)| id)
        .collect()
}

fn split_by(table: &TopologyAttributeTable, fid: &FeatureId) -> HashSet<GeometryHandleId> {
    table
        .iter()
        .filter(|(_, a)| {
            role_is_face(a.role) && a.mod_history.iter().any(|m| &m.splitting_feature_id == fid)
        })
        .map(|(id, _)| id)
        .collect()
}

fn generic(tag: &str, table: &TopologyAttributeTable, feats: &BTreeMap<String, FeatureId>) {
    eprintln!("\n===================== {tag} =====================");
    eprintln!("table.len() = {}", table.len());
    let faces: Vec<_> = table.iter().filter(|(_, a)| role_is_face(a.role)).collect();
    eprintln!("total FACE-role entries = {}", faces.len());

    let mut by_feat: BTreeMap<String, usize> = BTreeMap::new();
    for (_, a) in &faces {
        *by_feat.entry(format!("{:?}", a.feature_id)).or_default() += 1;
    }
    eprintln!("distinct feature_id across FACE entries ({}):", by_feat.len());
    for (k, c) in &by_feat {
        eprintln!("    faces={c:>3}  {k}");
    }

    let with_mod = faces
        .iter()
        .filter(|(_, a)| !a.mod_history.is_empty())
        .count();
    eprintln!("FACE entries with NON-EMPTY mod_history = {with_mod}");

    let mut by_split: BTreeMap<String, usize> = BTreeMap::new();
    for (_, a) in &faces {
        let mut seen = HashSet::new();
        for m in &a.mod_history {
            seen.insert(format!("{:?}", m.splitting_feature_id));
        }
        for s in seen {
            *by_split.entry(s).or_default() += 1;
        }
    }
    eprintln!(
        "distinct splitting_feature_id in FACE mod_history ({}) [count = #faces referencing]:",
        by_split.len()
    );
    for (k, c) in &by_split {
        eprintln!("    faces={c:>3}  {k}");
    }

    eprintln!("resolved feature() cells ({}):", feats.len());
    for (k, v) in feats {
        eprintln!("    {k} = {v:?}");
    }
}

fn report_created_split(table: &TopologyAttributeTable, feats: &BTreeMap<String, FeatureId>, name: &str, key: &str) {
    match feats.get(key) {
        Some(f) => {
            eprintln!(
                "created_by(g, feature({name})) = {}   split_by(g, feature({name})) = {}",
                created_by(table, f).len(),
                split_by(table, f).len()
            );
        }
        None => eprintln!("feature({name}) [{key}] UNRESOLVED (not a Value::Feature cell)"),
    }
}

#[test]
fn fixture1_fillet_all_edges() {
    let source = "structure S { let base = box(10mm,10mm,10mm) let g = fillet(base, 1mm) let fb = feature(base) let fg = feature(g) }";
    let Some((engine, result)) = build(source) else {
        return;
    };
    let table = engine.topology_attribute_table();
    let feats = feature_cells(&result);
    generic("FIXTURE 1 — fillet all-edges (2-arg)", table, &feats);
    report_created_split(table, &feats, "g", "fg");
    report_created_split(table, &feats, "base", "fb");
}

#[test]
fn fixture2_fillet_curated_edge() {
    // 3-arg curated fillet with single(edges(base)). Q5 target.
    let source = "structure S { let base = box(10mm,10mm,10mm) let e = single(edges(base)) let g = fillet(base, e, 1mm) let fb = feature(base) let fg = feature(g) }";
    let Some((engine, result)) = build(source) else {
        eprintln!("\n===== FIXTURE 2 — fillet curated (3-arg): did not reach build (see errors above) =====");
        return;
    };
    let table = engine.topology_attribute_table();
    let feats = feature_cells(&result);
    generic("FIXTURE 2 — fillet curated edge (3-arg)", table, &feats);
    report_created_split(table, &feats, "g", "fg");
    report_created_split(table, &feats, "base", "fb");
}

#[test]
fn fixture3_boolean_union() {
    // Task-literal geometry: box centered at origin + coaxial cylinder (base z=0..20).
    // (added `let fb = feature(b)` so created_by(feature(b)) is answerable — Q4.)
    let source = "structure S { let a = box(10mm,10mm,10mm) let b = cylinder(3mm, 20mm) let g = union(a, b) let fa = feature(a) let fb = feature(b) let fg = feature(g) }";
    let Some((engine, result)) = build(source) else {
        return;
    };
    let table = engine.topology_attribute_table();
    let feats = feature_cells(&result);
    generic("FIXTURE 3 — boolean union(box, coaxial cylinder)", table, &feats);
    report_created_split(table, &feats, "g", "fg");
    report_created_split(table, &feats, "a", "fa");
    report_created_split(table, &feats, "b", "fb");
    report_disjoint(table, &feats);
}

#[test]
fn fixture4_boolean_difference() {
    // Face-crossing box cutter: cutter spans full Y (40mm >> 10mm box), narrow X,
    // straddles the top face (z 0..6 vs box top z=5) -> carves a trench that
    // SPLITS the top face into two strips -> forces non-empty mod_history.
    let source = "structure S { let a = box(10mm,10mm,10mm) let b = translate(box(3mm,40mm,6mm), 0mm,0mm,3mm) let g = difference(a, b) let fa = feature(a) let fb = feature(b) let fg = feature(g) }";
    let Some((engine, result)) = build(source) else {
        return;
    };
    let table = engine.topology_attribute_table();
    let feats = feature_cells(&result);
    generic("FIXTURE 4 — boolean difference(box, face-crossing box cutter)", table, &feats);
    report_created_split(table, &feats, "g", "fg");
    report_created_split(table, &feats, "a", "fa");
    report_created_split(table, &feats, "b", "fb");
    report_disjoint(table, &feats);
}

fn report_disjoint(table: &TopologyAttributeTable, feats: &BTreeMap<String, FeatureId>) {
    let ca = feats.get("fa").map(|f| created_by(table, f));
    let cb = feats.get("fb").map(|f| created_by(table, f));
    match (ca, cb) {
        (Some(ca), Some(cb)) => {
            eprintln!(
                "created_by(feature(a)) |set|={}  created_by(feature(b)) |set|={}",
                ca.len(),
                cb.len()
            );
            eprintln!(
                "DISJOINT = {}   both NON-EMPTY = {}   (|a∩b| = {})",
                ca.is_disjoint(&cb),
                !ca.is_empty() && !cb.is_empty(),
                ca.intersection(&cb).count()
            );
        }
        _ => eprintln!("cannot compute disjointness: fa or fb unresolved"),
    }
}

// ============================ CC1–CC5 (compile-only, +CC5 OCCT eval) =========

fn cc_report(tag: &str, src: &str) -> usize {
    let compiled = reify_test_support::parse_and_compile_with_stdlib(src);
    let errs = reify_test_support::errors_only(&compiled);
    eprintln!("\n----- {tag} -----");
    eprintln!("#Error diagnostics = {}", errs.len());
    for d in &errs {
        eprintln!("    code={:?}  msg={}", d.code, d.message);
    }
    errs.len()
}

#[test]
fn cc1_geometry_where_feature_required() {
    let n = cc_report(
        "CC1 created_by_feature(base, base) [2nd arg Geometry, NOT Feature]",
        "structure def S { let base = box(10mm,10mm,10mm) let s = created_by_feature(base, base) }",
    );
    eprintln!("CC1 VERDICT: does it error? {}", if n > 0 { "YES" } else { "NO" });
}

#[test]
fn cc2_positive_control() {
    let n = cc_report(
        "CC2 created_by_feature(base, feature(base)) [positive control]",
        "structure def S { let base = box(10mm,10mm,10mm) let f = feature(base) let s = created_by_feature(base, f) }",
    );
    eprintln!("CC2 VERDICT: #errors={n} (expect 0)");
}

#[test]
fn cc3_face_selector_to_edge_param() {
    let src = "fn needs_edge(s: EdgeSelector) -> Int { 42 } structure def S { let base = box(10mm,10mm,10mm) let f = feature(base) let n = needs_edge(created_by_feature(base, f)) }";
    let compiled = reify_test_support::parse_and_compile_with_stdlib(src);
    let errs = reify_test_support::errors_only(&compiled);
    eprintln!("\n----- CC3 needs_edge(created_by_feature(base,f)) [Face->Edge param] -----");
    eprintln!("#Error diagnostics = {}", errs.len());
    let mut has_kind = false;
    for d in &errs {
        eprintln!("    code={:?}  msg={}", d.code, d.message);
        if format!("{:?}", d.code).contains("SelectorKindMismatch") {
            has_kind = true;
        }
    }
    eprintln!(
        "CC3 VERDICT: errors with SelectorKindMismatch? {}",
        if has_kind { "YES" } else { "NO" }
    );
}

#[test]
fn cc4_face_selector_to_face_param() {
    let n = cc_report(
        "CC4 needs_face(created_by_feature(base,f)) [Face->Face param, positive]",
        "fn needs_face(s: FaceSelector) -> Int { 42 } structure def S { let base = box(10mm,10mm,10mm) let f = feature(base) let n = needs_face(created_by_feature(base, f)) }",
    );
    eprintln!("CC4 VERDICT: #errors={n} (expect 0)");
}

#[test]
fn cc5_subshape_feature() {
    let src = "structure def S { let base = box(10mm,10mm,10mm) let sf = feature(single(faces(base))) }";
    let n = cc_report("CC5 feature(single(faces(base))) [typecheck]", src);
    eprintln!("CC5 VERDICT (compile): #errors={n} (expect 0 => typechecks)");

    let Some((_engine, result)) = build(src) else {
        eprintln!("CC5 eval: OCCT unavailable or build failed — cannot check sf resolution");
        return;
    };
    let mut found = false;
    for (id, v) in result.values.iter() {
        if id.member == "sf" {
            found = true;
            let is_feat = matches!(v, Value::Feature(_));
            eprintln!(
                "CC5 eval: cell {}.{} = {:?}   (is Value::Feature = {is_feat})",
                id.entity, id.member, v
            );
        }
    }
    if !found {
        eprintln!("CC5 eval: NO cell with member 'sf' present in result.values");
    }
}
