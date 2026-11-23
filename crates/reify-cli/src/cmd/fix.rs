//! `reify fix --imports`: add the `import std.X` lines each file needs, as
//! derived by `reify_compiler::import_headers`; usage banner: [`FIX_USAGE`].

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use reify_compiler::cfg::CfgSet;
use reify_compiler::import_headers::{self, GatedOffImports, RequiredImport};
use reify_core::{Diagnostic, ModulePath};

use crate::args::split_cfg_args_or_usage;
use crate::diagnostic_line::{render_diagnostic_line, render_parse_error_line};
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
    let (cfg, rest) = match split_cfg_args_or_usage(args, FIX_USAGE) {
        Ok(split) => split,
        Err(code) => return code,
    };
    let Some(fix) = parse_args(&rest) else {
        eprintln!("{FIX_USAGE}");
        return ExitCode::FAILURE;
    };
    let mut all_complete = true;
    for file in &fix.files {
        all_complete &= fix_imports(file, fix.check_only, &cfg);
    }
    if all_complete {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Whether `path` ends up importing every std module it needs, a `#cfg`-gated
/// import counting only when `cfg` satisfies it: true once the missing imports
/// are written, false when they are only reported, when the derived imports
/// would leave a new diagnostic, when an added import would not stand where it
/// is written, or on any read, parse or write failure.
fn fix_imports(path: &str, check_only: bool, cfg: &CfgSet) -> bool {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(e) => {
            eprintln!("Error reading {path}: {e}");
            return false;
        }
    };
    let parsed = reify_compiler::parse_with_stdlib(&source, crate::module_path_for_file(path));
    if !parsed.errors.is_empty() {
        for (position, message) in parsed.error_positions(&source) {
            eprintln!("{}", render_parse_error_line(path, position, message));
        }
        return false;
    }

    let own_module = parsed
        .declared_module_path
        .clone()
        .unwrap_or_else(|| parsed.path.clone());
    let universe = import_headers::stdlib_universe_excluding(&own_module);
    let plan = import_headers::derive_std_imports(&parsed, &universe);

    let mut imports_after_fix = import_headers::cfg_satisfied_imports(&parsed, cfg);
    imports_after_fix.extend(plan.modules());
    let residual = plan.residual_diagnostics(&parsed, &imports_after_fix);
    let gated_off = GatedOffImports::of(&parsed, cfg);
    if !residual.is_empty() {
        let still_gated_off = gated_off
            .modules()
            .filter(|module| !imports_after_fix.contains(module));
        report_refusal(path, &source, &residual, still_gated_off);
        return false;
    }

    let missing = plan.missing_from(&parsed, cfg);
    if missing.is_empty() {
        return true;
    }
    if check_only {
        for required in &missing {
            println!("{path}: missing import {}", describe(required, &gated_off));
        }
        return false;
    }
    let modules: Vec<ModulePath> = missing
        .iter()
        .map(|required| required.module.clone())
        .collect();
    let fixed = match import_headers::insert_imports(&source, &parsed, cfg, &modules) {
        Ok(fixed) => fixed,
        Err(defect) => {
            eprintln!("{path}: {defect}; not writing");
            return false;
        }
    };
    if let Err(e) = replace_atomically(Path::new(path), &fixed) {
        eprintln!("Error writing {path}: {e}");
        return false;
    }
    for required in &missing {
        println!("{path}: added import {}", describe(required, &gated_off));
    }
    true
}

/// Why `path` is not written: the residual diagnostics, plus an info line naming `still_gated_off`,
/// the modules of `#cfg`-gated imports that stay out of effect even with the derived imports added.
fn report_refusal<'a>(
    path: &str,
    source: &str,
    residual: &[Diagnostic],
    still_gated_off: impl Iterator<Item = &'a ModulePath>,
) {
    eprintln!(
        "{path}: the derived imports leave diagnostics the full standard library does not; \
         not writing"
    );
    for diagnostic in residual {
        eprintln!("{}", render_diagnostic_line(path, source, diagnostic));
    }
    let modules: BTreeSet<String> = still_gated_off.map(ModulePath::to_dotted).collect();
    if modules.is_empty() {
        return;
    }
    let modules: Vec<String> = modules.into_iter().collect();
    let note = Diagnostic::info(format!(
        "#cfg-gated imports not in effect under the active cfg: {}",
        modules.join(", ")
    ));
    eprintln!("{}", render_diagnostic_line(path, source, &note));
}

/// Writes a sibling temp file and renames it over `path`'s target, so a failure leaves the
/// original intact. Refuses a target that could not be written in place, or that has other hard
/// links, which the rename would leave holding the old text.
fn replace_atomically(path: &Path, contents: &str) -> std::io::Result<()> {
    let target = std::fs::canonicalize(path)?;
    let dir = target.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{} has no parent directory", target.display()),
        )
    })?;
    let metadata = metadata_if_writable_in_place(&target)?;
    #[cfg(unix)]
    refuse_other_hard_links(&target, &metadata)?;
    let mut temp = tempfile::Builder::new()
        .prefix(".reify-fix.")
        .tempfile_in(dir)?;
    temp.write_all(contents.as_bytes())?;
    temp.as_file().set_permissions(metadata.permissions())?;
    temp.as_file().sync_all()?;
    temp.persist(&target).map_err(|e| e.error)?;
    Ok(())
}

/// Opens `target` for writing without truncating it: the rename needs only the directory to be
/// writable, so this keeps the file's own write protection binding.
fn metadata_if_writable_in_place(target: &Path) -> std::io::Result<std::fs::Metadata> {
    std::fs::OpenOptions::new()
        .write(true)
        .open(target)?
        .metadata()
}

#[cfg(unix)]
fn refuse_other_hard_links(target: &Path, metadata: &std::fs::Metadata) -> std::io::Result<()> {
    use std::os::unix::fs::MetadataExt;

    let links = metadata.nlink();
    if links <= 1 {
        return Ok(());
    }
    Err(std::io::Error::other(format!(
        "{} has {links} hard links, and replacing it would leave the others unfixed",
        target.display()
    )))
}

/// `std.x.y (A, B)`: the module and the names that need it, plus a note when one of the file's
/// imports that the active cfg leaves out of effect would bind one of those names from the module,
/// were its `#cfg` satisfied.
fn describe(required: &RequiredImport, gated_off: &GatedOffImports) -> String {
    let names: BTreeSet<&str> = required
        .names
        .iter()
        .map(|name_use| name_use.name.as_str())
        .collect();
    let names: Vec<&str> = names.into_iter().collect();
    let note = if gated_off.would_cover_any(required) {
        "; a #cfg-gated import of it is not in effect under the active cfg"
    } else {
        ""
    };
    format!(
        "{} ({}){note}",
        required.module.to_dotted(),
        names.join(", ")
    )
}
