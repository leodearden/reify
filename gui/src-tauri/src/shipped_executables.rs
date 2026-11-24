//! Where the executables the package ships beside reify-gui are found.

use std::path::{Path, PathBuf};

/// The executable `name` beside `gui_exe`. Tauri installs what the package
/// ships next to the GUI executable (`target/<profile>/`, `/usr/bin/`,
/// `<AppDir>/usr/bin/`), never in `resource_dir`.
///
/// Errs, naming `gui_exe`, unless it is absolute with a parent directory: a
/// relative result would make `Command::new` search `PATH`.
pub fn beside_gui(gui_exe: &Path, name: &str) -> Result<PathBuf, String> {
    match gui_exe.parent() {
        Some(dir) if gui_exe.is_absolute() => Ok(dir.join(name)),
        _ => Err(format!(
            "cannot locate {name} beside GUI executable {gui_exe:?}: \
             not an absolute path with a parent directory"
        )),
    }
}
