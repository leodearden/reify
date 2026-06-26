//! Integration tests for the `auto_type_params::max_depth` config field
//! end-to-end wiring (task #3572 step-7/8).
//!
//! Pins the full Manifest→auto_type_params→compile pipeline:
//!
//! - With `[auto_type_params]\nmax_depth = 1` in the manifest, a module with
//!   2 `auto:` type params (params.len()=2 > max_depth=1) must produce a
//!   `DiagnosticCode::AutoTypeParamDepthBoundExceeded` warning.
//! - With the default config (`AutoTypeParamsConfig::default()`, max_depth=6),
//!   the same module must NOT produce that diagnostic (2 ≤ 6).
//!
//! The new entry point `compile_with_stdlib_with_config` is the leaf observable.
//! It delegates to the existing compile pipeline with the supplied config wired
//! into `CompilationCtx::auto_type_params`, keeping existing callers unchanged
//! at max_depth=6.

use reify_compiler::parse_with_stdlib;
use reify_config::{AutoTypeParamsConfig, Manifest};
use reify_core::{DiagnosticCode, ModulePath};

/// Module source with:
/// - A trait `Seal` (the bound)
/// - Two zero-field implementing structures (`SealA`, `SealB`)
/// - A generic structure `Widget<T: Seal, U: Seal>` with 2 auto-typed params
/// - An assembly `WidgetAssembly` that instantiates with 2 `auto: Seal` clauses
///
/// With `max_depth=1`: 2 params > 1 → `AutoTypeParamDepthBoundExceeded` fires.
/// With `max_depth=6`: 2 params ≤ 6 → DFS runs, no depth-bound warning.
const WIDGET_SOURCE: &str = r#"
trait Seal {}

structure def SealA : Seal {}

structure def SealB : Seal {}

structure def Widget<T: Seal, U: Seal> {
    param slot_t : T
    param slot_u : U
}

structure def WidgetAssembly {
    sub w = Widget<auto: Seal, auto: Seal>()
}
"#;

/// With `max_depth = 1`, a 2-param `auto:` use site must produce
/// `AutoTypeParamDepthBoundExceeded` (2 params > 1 = max_depth fires
/// the depth-bound check before any feasibility work).
#[test]
fn max_depth_1_produces_depth_bound_exceeded_warning() {
    let cfg = Manifest::from_toml_str("[auto_type_params]\nmax_depth = 1\n")
        .expect("valid manifest")
        .auto_type_params()
        .clone();

    let parsed = parse_with_stdlib(WIDGET_SOURCE, ModulePath::single("test"));
    let module = reify_compiler::compile_with_stdlib_with_config(&parsed, &cfg);

    let depth_bound_diags: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoTypeParamDepthBoundExceeded))
        .collect();

    assert!(
        !depth_bound_diags.is_empty(),
        "max_depth=1 with 2 auto: params must produce \
         AutoTypeParamDepthBoundExceeded; got diagnostics: {:#?}",
        module.diagnostics
    );
}

/// With the default config (`max_depth = 6`), a 2-param `auto:` use site must
/// NOT produce `AutoTypeParamDepthBoundExceeded` (2 ≤ 6 → DFS runs normally).
#[test]
fn default_max_depth_produces_no_depth_bound_exceeded() {
    let cfg = AutoTypeParamsConfig::default();

    let parsed = parse_with_stdlib(WIDGET_SOURCE, ModulePath::single("test"));
    let module = reify_compiler::compile_with_stdlib_with_config(&parsed, &cfg);

    let depth_bound_diags: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoTypeParamDepthBoundExceeded))
        .collect();

    assert!(
        depth_bound_diags.is_empty(),
        "default max_depth=6 with 2 auto: params must NOT produce \
         AutoTypeParamDepthBoundExceeded; got: {:#?}",
        depth_bound_diags
    );
}
