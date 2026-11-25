//! Runs the reify CLI the package ships beside reify-gui.

use std::convert::Infallible;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// File name of the reify CLI the package ships beside reify-gui: the deb's
/// `/usr/bin/reify`, which `scripts/build-bundle.sh` post-checks.
pub const CLI_EXECUTABLE: &str = "reify";

/// The reify CLI beside `gui_exe` ([`crate::shipped_executables::beside_gui`]).
pub fn cli_executable_beside(gui_exe: &Path) -> Result<PathBuf, String> {
    crate::shipped_executables::beside_gui(gui_exe, CLI_EXECUTABLE)
}

/// Runs the reify CLI beside this process's executable with `args`, in place
/// of this process: stdin, stdout, stderr and the exit status are the CLI's
/// own. There is no `PATH` fallback. Returns only when the CLI cannot be
/// started, with an error naming the path tried and the cause.
pub fn exec(args: impl IntoIterator<Item = OsString>) -> Result<Infallible, String> {
    let gui_exe = std::env::current_exe().map_err(|e| {
        format!("cannot locate the running executable to find {CLI_EXECUTABLE} beside it: {e}")
    })?;
    let cli = cli_executable_beside(&gui_exe)?;
    replace_process(Command::new(&cli).args(args), &cli)
}

#[cfg(unix)]
fn replace_process(command: &mut Command, cli: &Path) -> Result<Infallible, String> {
    use std::os::unix::process::CommandExt;
    let e = command.exec();
    Err(format!("cannot run {}: {e}", cli.display()))
}

#[cfg(not(unix))]
fn replace_process(command: &mut Command, cli: &Path) -> Result<Infallible, String> {
    let status = command
        .status()
        .map_err(|e| format!("cannot run {}: {e}", cli.display()))?;
    std::process::exit(status.code().unwrap_or(1))
}
