//! The AppImage's AppRun.wrapped (AppImageKit) chdirs into `$APPDIR/usr` before
//! it execs reify-gui. This module recovers the directory the AppImage was run
//! from. The bundled CLI moves there ([`restore`]). The GUI keeps `$APPDIR/usr`
//! as its cwd, because the bundled WebKit finds its helper processes by a path
//! relative to it, and resolves its file argument against the launch directory
//! instead ([`resolve`]). `tests/infra/test_bundle_clean_install.py` pins both
//! end to end.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The directory the AppImage was run from, when `current_dir` carries
/// AppRun.wrapped's signature: `APPDIR` is non-empty and `current_dir` is
/// `$APPDIR/usr`. It is the first of `OWD` (exported in FUSE mode only), then
/// `PWD` (the calling shell's, which also survives extract-and-run), that is
/// non-empty, absolute and an existing directory.
pub fn launch_dir(var: impl Fn(&str) -> Option<OsString>, current_dir: &Path) -> Option<PathBuf> {
    let appdir = var("APPDIR").filter(|appdir| !appdir.is_empty())?;
    let apprun_cwd = Path::new(&appdir).join("usr").canonicalize().ok()?;
    if apprun_cwd != current_dir.canonicalize().ok()? {
        return None;
    }
    ["OWD", "PWD"]
        .into_iter()
        .filter_map(&var)
        .map(PathBuf::from)
        .find(|dir| dir.is_absolute() && dir.is_dir())
}

/// `path` joined onto [`launch_dir`] when `path` is relative and there is a
/// launch directory; `path` unchanged otherwise.
pub fn path_from_launch_dir(
    var: impl Fn(&str) -> Option<OsString>,
    current_dir: &Path,
    path: PathBuf,
) -> PathBuf {
    if path.is_absolute() {
        return path;
    }
    match launch_dir(var, current_dir) {
        Some(dir) => dir.join(path),
        None => path,
    }
}

/// [`path_from_launch_dir`] of this process's environment and cwd, leaving
/// the cwd itself alone. `path` is unchanged when the cwd cannot be read.
pub fn resolve(path: OsString) -> PathBuf {
    let path = PathBuf::from(path);
    match std::env::current_dir() {
        Ok(current_dir) => path_from_launch_dir(|name| std::env::var_os(name), &current_dir, path),
        Err(_) => path,
    }
}

/// Changes this process's cwd to [`launch_dir`] of its own environment and
/// cwd, when there is one. For the bundled CLI only: the GUI must keep the
/// AppImage's cwd, since WebKit's helper path is relative to `$APPDIR/usr`.
/// A failure is one stderr warning, never an exit.
pub fn restore() {
    let Ok(current_dir) = std::env::current_dir() else {
        return;
    };
    let Some(dir) = launch_dir(|name| std::env::var_os(name), &current_dir) else {
        return;
    };
    if let Err(e) = std::env::set_current_dir(&dir) {
        eprintln!(
            "reify-gui: cannot return to the AppImage's launch directory {}: {e}",
            dir.display()
        );
    }
}
