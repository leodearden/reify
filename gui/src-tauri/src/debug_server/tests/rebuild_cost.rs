//! Task 6752: what routing `reify_save_file` / `reify_export` through the
//! shared write seam costs on the REAL kernel, over each tool's pure-I/O floor.
//! The numbers docs/debug-mcp-contract.md records came from this measurement.
//!
//! On demand only:
//!
//! ```text
//! cargo test -p reify-gui --lib --features gui --release rebuild_cost -- --ignored --nocapture
//! ```
//!
//! `REIFY_REBUILD_COST_DESIGN=<workspace-relative .ri>` measures another design.
//!
//! A child of `debug_server::tests` so it builds only under the `gui` feature,
//! which is what links the OCCT kernel. It reaches public items only.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reify_constraints::SimpleConstraintChecker;
use reify_ir::ExportFormat;

use crate::commands::save_file_impl;
use crate::debug_server::{
    reify_export_on_engine_and_refresh_baseline, reify_save_file_on_engine_and_refresh_baseline,
    write_tool_frontend_payload,
};
use crate::engine::EngineSession;
use crate::engine_lock::with_engine_lock;
use crate::types::GuiState;

const DEFAULT_DESIGN: &str = "prj/printer_v01/printer.ri";
const ITERATIONS: usize = 5;

type Engine = Arc<Mutex<EngineSession>>;

#[test]
#[ignore = "on-demand measurement for docs/debug-mcp-contract.md; run with --ignored --nocapture"]
fn measure_the_write_seam_rebuild_on_the_real_kernel() {
    // A deep compile overflows a default test-thread stack; the engine's own
    // threads run on the same large stack for the same reason.
    crate::large_stack::spawn_on_large_stack(measure)
        .expect("spawn the measuring thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

fn measure() {
    let registry: Vec<String> = reify_eval::collect_registry().into_keys().collect();
    println!("kernel registry: {registry:?}");
    assert!(
        registry.iter().any(|k| k == "occt"),
        "the OCCT kernel is not registered, so this would measure a stand-in"
    );

    let root = workspace_root();
    let design = std::env::var("REIFY_REBUILD_COST_DESIGN").unwrap_or(DEFAULT_DESIGN.to_string());
    let engine: Engine = Arc::new(Mutex::new(EngineSession::with_registered_kernel(Box::new(
        SimpleConstraintChecker,
    ))));

    let (load, _) = time(|| on_engine(&engine, |s| s.load_file(&root.join(&design))));
    let (rebuild, s0) = time(|| on_engine(&engine, EngineSession::build_gui_state));
    let (serialise, payload) = time(|| write_tool_frontend_payload(&s0, None).expect("payload"));
    let payload_bytes = serde_json::to_vec(&payload).expect("payload bytes").len();
    let faces: usize = s0.meshes.iter().map(|m| m.indices.len() / 3).sum();

    println!(
        "design: {design}\nprofile: {}\nHEAD: {}\nhost loadavg: {}\n\
         load_file: {} ms, first build_gui_state: {} ms\n\
         meshes: {}, faces: {faces}\napply_gui_state payload: {payload_bytes} bytes, serialised in {} ms",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        head_sha(&root),
        host_load(),
        ms(load),
        ms(rebuild),
        s0.meshes.len(),
        ms(serialise),
    );

    let rows = measure_rows(&engine, s0);
    println!(
        "{:<62} {:>10} {:>10} {:>10}",
        format!("row (ms, n={ITERATIONS})"),
        "median",
        "min",
        "max"
    );
    for (label, samples) in rows {
        println!(
            "{label:<62} {:>10} {:>10} {:>10}",
            ms(median(&samples)),
            ms(*samples.iter().min().expect("samples")),
            ms(*samples.iter().max().expect("samples")),
        );
    }
}

/// One warm-up round, then [`ITERATIONS`] timed rounds of every row. `s0` is
/// the state the frontend holds, which the seams diff against.
fn measure_rows(engine: &Engine, s0: GuiState) -> Vec<(&'static str, Vec<Duration>)> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let baseline = Mutex::new(Some(s0.clone()));
    let scratch = tempfile::tempdir().expect("tempdir");
    let save_target = scratch.path().join("saved.ri");
    let save_target_str = save_target.to_string_lossy().into_owned();
    let export_target = scratch.path().join("export.step");
    let export_target_str = export_target.to_string_lossy().into_owned();
    let content = s0
        .files
        .first()
        .expect("a loaded source file")
        .content
        .clone();

    let mut rows: Vec<(&'static str, Vec<Duration>)> = vec![
        ("(a) save_file_impl alone - the pure-I/O floor", vec![]),
        (
            "(b) reify_save_file seam - rebuild + write + baseline",
            vec![],
        ),
        ("(c) build_gui_state alone - the rebuild", vec![]),
        ("(e) EngineSession::export(Step) alone", vec![]),
        (
            "(f) reify_export seam - export + rebuild + baseline",
            vec![],
        ),
    ];
    for round in 0..=ITERATIONS {
        let samples = [
            time(|| save_file_impl(&save_target_str, &content).expect("save_file_impl")).0,
            time(|| {
                runtime
                    .block_on(reify_save_file_on_engine_and_refresh_baseline(
                        engine,
                        &baseline,
                        Some(save_target_str.clone()),
                    ))
                    .expect("reify_save_file seam")
            })
            .0,
            time(|| on_engine(engine, EngineSession::build_gui_state)).0,
            time(|| on_engine(engine, |s| s.export(ExportFormat::Step, &export_target))).0,
            time(|| {
                runtime
                    .block_on(reify_export_on_engine_and_refresh_baseline(
                        engine,
                        &baseline,
                        "step",
                        &export_target_str,
                    ))
                    .expect("reify_export seam")
            })
            .0,
        ];
        if round > 0 {
            for ((_, row), sample) in rows.iter_mut().zip(samples) {
                row.push(sample);
            }
        }
    }
    rows
}

/// Run `f` under the engine lock on this thread, failing the measurement on
/// any error.
fn on_engine<T>(engine: &Engine, f: impl FnOnce(&mut EngineSession) -> Result<T, String>) -> T {
    with_engine_lock(engine, f)
        .and_then(std::convert::identity)
        .unwrap_or_else(|e| panic!("engine call failed: {e}"))
}

fn time<T>(f: impl FnOnce() -> T) -> (Duration, T) {
    let started = Instant::now();
    let value = f();
    (started.elapsed(), value)
}

fn median(samples: &[Duration]) -> Duration {
    let mut sorted = samples.to_vec();
    sorted.sort();
    sorted[sorted.len() / 2]
}

fn ms(d: Duration) -> String {
    format!("{:.1}", d.as_secs_f64() * 1000.0)
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

/// Wall-clock numbers mean little without the load they were taken under.
fn host_load() -> String {
    std::fs::read_to_string("/proc/loadavg")
        .map(|load| {
            let cpus = std::thread::available_parallelism().map_or(0, usize::from);
            format!("{} on {cpus} cpus", load.trim())
        })
        .unwrap_or_else(|_| "unavailable".to_string())
}

fn head_sha(root: &Path) -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short=10", "HEAD"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}
