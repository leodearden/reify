//! `reify fix --imports [--check] <file>...`: add the `import std.X` lines
//! each file needs, as derived by `reify_compiler::import_headers`.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use reify_compiler::import_headers::{self, RequiredImport};
use reify_core::ModulePath;

use crate::diagnostic_line::render_diagnostic_line;
use crate::help::FIX_USAGE;

struct FixArgs<'a> {
    check_only: bool,
    files: Vec<&'a str>,
}

/// `None` for an unknown flag, a missing `--imports` or no file.
fn parse_args(args: &[String]) -> Option<FixArgs<'_>> {
    let mut imports = false;
    let mut check_only = false;
    let mut files = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--imports" => imports = true,
            "--check" => check_only = true,
            flag if flag.starts_with('-') => return None,
            file => files.push(file),
        }
    }
    (imports && !files.is_empty()).then_some(FixArgs { check_only, files })
}

pub(crate) fn cmd_fix(args: &[String]) -> ExitCode {
    let Some(fix) = parse_args(args) else {
        eprintln!("{FIX_USAGE}");
        return ExitCode::FAILURE;
    };
    let mut all_complete = true;
    for file in &fix.files {
        all_complete &= fix_imports(file, fix.check_only);
    }
    if all_complete {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Whether `path` ends up importing every std module it needs: true once the
/// missing imports are written, false when they are only reported, when the
/// derived imports would leave a new diagnostic, or on any read, parse or
/// write failure.
fn fix_imports(path: &str, check_only: bool) -> bool {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(e) => {
            eprintln!("Error reading {path}: {e}");
            return false;
        }
    };
    let parsed = reify_compiler::parse_with_stdlib(&source, crate::module_path_for_file(path));
    if !parsed.errors.is_empty() {
        crate::report_parse_errors(parsed.render_errors(&source), &mut std::io::stderr());
        return false;
    }

    let own_module = parsed
        .declared_module_path
        .clone()
        .unwrap_or_else(|| parsed.path.clone());
    let universe = import_headers::stdlib_universe_excluding(&own_module);
    let plan = import_headers::derive_std_imports(&parsed, &universe);

    let mut imports_after_fix = import_headers::declared_imports(&parsed);
    imports_after_fix.extend(plan.modules());
    let residual = import_headers::residual_diagnostics(&parsed, &imports_after_fix, &universe);
    if !residual.is_empty() {
        eprintln!(
            "{path}: the derived imports leave diagnostics the full standard library does not; \
             not writing"
        );
        for diagnostic in &residual {
            eprintln!("{}", render_diagnostic_line(path, &source, diagnostic));
        }
        return false;
    }

    let missing = plan.missing_from(&parsed);
    if missing.is_empty() {
        return true;
    }
    if check_only {
        for required in &missing {
            println!("{path}: missing import {}", describe(required));
        }
        return false;
    }
    let modules: Vec<ModulePath> = missing
        .iter()
        .map(|required| required.module.clone())
        .collect();
    let fixed = import_headers::insert_imports(&source, &parsed, &modules);
    if let Err(e) = replace_atomically(Path::new(path), &fixed) {
        eprintln!("Error writing {path}: {e}");
        return false;
    }
    for required in &missing {
        println!("{path}: added import {}", describe(required));
    }
    true
}

/// Writes a sibling temp file and renames it over `path`'s target, so a failure leaves the
/// original intact.
fn replace_atomically(path: &Path, contents: &str) -> std::io::Result<()> {
    let target = std::fs::canonicalize(path)?;
    let dir = target.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{} has no parent directory", target.display()),
        )
    })?;
    let permissions = std::fs::metadata(&target)?.permissions();
    let mut temp = tempfile::Builder::new()
        .prefix(".reify-fix.")
        .tempfile_in(dir)?;
    temp.write_all(contents.as_bytes())?;
    temp.as_file().set_permissions(permissions)?;
    temp.as_file().sync_all()?;
    temp.persist(&target).map_err(|e| e.error)?;
    Ok(())
}

/// `std.x.y (A, B)`: the module and the names that need it.
fn describe(required: &RequiredImport) -> String {
    let names: BTreeSet<&str> = required
        .names
        .iter()
        .map(|name_use| name_use.name.as_str())
        .collect();
    let names: Vec<&str> = names.into_iter().collect();
    format!("{} ({})", required.module.to_dotted(), names.join(", "))
}
