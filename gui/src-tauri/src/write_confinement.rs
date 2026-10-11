//! Where the AI write tools may write: only inside the open design's directory
//! (rule and rationale: docs/debug-mcp-contract.md "AI write tools").

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Why a requested write target was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteTargetRefusal {
    NoDesignOpen {
        requested: String,
    },
    NotAbsolute {
        requested: String,
    },
    Unresolvable {
        requested: String,
        reason: String,
    },
    OutsideDesignDir {
        requested: String,
        resolved: PathBuf,
        design_dir: PathBuf,
    },
}

const RULE: &str = "AI write targets must lie inside the open design's directory";

impl fmt::Display for WriteTargetRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDesignOpen { requested } => {
                write!(
                    f,
                    "{RULE}, and no design file is open: refusing {requested}"
                )
            }
            Self::NotAbsolute { requested } => {
                write!(
                    f,
                    "{RULE}, named by an absolute path: refusing relative {requested}"
                )
            }
            Self::Unresolvable { requested, reason } => {
                write!(f, "{RULE}: cannot resolve {requested}: {reason}")
            }
            Self::OutsideDesignDir {
                requested,
                resolved,
                design_dir,
            } => write!(
                f,
                "{RULE}: {requested} resolves to {}, outside {}",
                resolved.display(),
                design_dir.display()
            ),
        }
    }
}

impl std::error::Error for WriteTargetRefusal {}

/// The resolved path to write for `requested`, if it lies inside the directory
/// of `design_file` once symlinks and `..` are resolved on both sides.
pub fn confine_to_design_dir(
    design_file: Option<&Path>,
    requested: &str,
) -> Result<PathBuf, WriteTargetRefusal> {
    let unresolvable = |reason: String| WriteTargetRefusal::Unresolvable {
        requested: requested.to_string(),
        reason,
    };
    let Some(design_file) = design_file else {
        return Err(WriteTargetRefusal::NoDesignOpen {
            requested: requested.to_string(),
        });
    };
    let target = Path::new(requested);
    if !target.is_absolute() {
        return Err(WriteTargetRefusal::NotAbsolute {
            requested: requested.to_string(),
        });
    }
    let design_dir = design_dir_of(design_file).map_err(unresolvable)?;
    let resolved = resolve_target(target).map_err(unresolvable)?;
    if !resolved.starts_with(&design_dir) {
        return Err(WriteTargetRefusal::OutsideDesignDir {
            requested: requested.to_string(),
            resolved,
            design_dir,
        });
    }
    Ok(resolved)
}

fn design_dir_of(design_file: &Path) -> Result<PathBuf, String> {
    let parent = design_file.parent().ok_or_else(|| {
        format!(
            "the design file {} has no parent directory",
            design_file.display()
        )
    })?;
    fs::canonicalize(parent).map_err(|e| format!("the design directory {}: {e}", parent.display()))
}

/// An existing entry resolves through any symlink (a dangling one is an error);
/// a new file resolves through its existing parent directory.
fn resolve_target(target: &Path) -> Result<PathBuf, String> {
    match fs::symlink_metadata(target) {
        Ok(_) => fs::canonicalize(target)
            .map_err(|e| format!("it exists but does not resolve to a real path: {e}")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => resolve_new_file(target),
        Err(e) => Err(e.to_string()),
    }
}

fn resolve_new_file(target: &Path) -> Result<PathBuf, String> {
    let (Some(parent), Some(name)) = (target.parent(), target.file_name()) else {
        return Err("it names no file".to_string());
    };
    let parent =
        fs::canonicalize(parent).map_err(|e| format!("its directory {}: {e}", parent.display()))?;
    Ok(parent.join(name))
}
