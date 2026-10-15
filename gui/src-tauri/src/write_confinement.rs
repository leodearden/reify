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
    DesignChanged {
        requested: String,
        confined_to: PathBuf,
        now_open: Option<PathBuf>,
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
            Self::DesignChanged {
                requested,
                confined_to,
                now_open,
            } => write!(
                f,
                "{RULE}: {requested} was checked against {}, but {} is open now",
                confined_to.display(),
                now_open
                    .as_deref()
                    .map_or_else(|| "no design".into(), Path::to_string_lossy)
            ),
        }
    }
}

impl std::error::Error for WriteTargetRefusal {}

/// A write target that resolved inside the directory of the design it was
/// checked against. Its path is handed out only while that design is still the
/// open one, so a design switched between the check and the write cannot move
/// the root out from under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfinedTarget {
    requested: String,
    design_file: PathBuf,
    resolved: String,
}

impl ConfinedTarget {
    /// The resolved path to write, provided `open_design` is still the design
    /// file this target was confined against.
    pub fn path_while_open(&self, open_design: Option<&Path>) -> Result<&str, WriteTargetRefusal> {
        if open_design == Some(self.design_file.as_path()) {
            return Ok(&self.resolved);
        }
        Err(WriteTargetRefusal::DesignChanged {
            requested: self.requested.clone(),
            confined_to: self.design_file.clone(),
            now_open: open_design.map(Path::to_path_buf),
        })
    }
}

/// `requested` as a [`ConfinedTarget`], if it lies inside the directory of
/// `design_file` once symlinks and `..` are resolved on both sides.
pub fn confine_to_design_dir(
    design_file: Option<&Path>,
    requested: &str,
) -> Result<ConfinedTarget, WriteTargetRefusal> {
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
    let resolved = resolved
        .into_os_string()
        .into_string()
        .map_err(|resolved| {
            unresolvable(format!("it resolves to a non-UTF-8 path {resolved:?}"))
        })?;
    Ok(ConfinedTarget {
        requested: requested.to_string(),
        design_file: design_file.to_path_buf(),
        resolved,
    })
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
