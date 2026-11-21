//! Environment defaults reify-gui applies to its own process before WebKitGTK
//! starts, so every launch path (desktop entry, installed `/usr/bin/reify-gui`,
//! AppImage, `reify gui`, the launcher scripts) gets them.

use std::ffi::OsString;

/// `(variable, default)` pairs, applied only where the caller left the
/// variable unset or empty.
///
/// `WEBKIT_DISABLE_DMABUF_RENDERER=1`: on NVIDIA+Mesa hosts WebKitGTK's DMABUF
/// renderer aborts with "Could not create GBM EGL display: EGL_NOT_INITIALIZED";
/// `1` selects the GLX/xlib path. Export `0` to keep the DMABUF renderer.
const DEFAULTS: &[(&str, &str)] = &[("WEBKIT_DISABLE_DMABUF_RENDERER", "1")];

/// `reify-gui --print-webkit-env` prints each variable reify-gui defaults (the
/// [`DEFAULTS`] table) as the process now sees it, then exits.
pub const PRINT_ENV_FLAG: &str = "--print-webkit-env";

/// One `NAME=value` line per [`DEFAULTS`] variable, in table order, with the
/// value `current` reports for it; the value is empty when unset.
pub fn env_report(current: impl Fn(&str) -> Option<OsString>) -> String {
    DEFAULTS
        .iter()
        .map(|(name, _)| {
            let value = current(name).unwrap_or_default();
            format!("{name}={}\n", value.to_string_lossy())
        })
        .collect()
}

/// [`env_report`] of this process's environment.
pub fn process_env_report() -> String {
    env_report(|name| std::env::var_os(name))
}

/// The [`DEFAULTS`] pairs whose variable `current` reports absent or empty.
/// An empty value counts as unset, as the shell's `${VAR:-default}` does.
pub fn defaults_to_apply(
    current: impl Fn(&str) -> Option<OsString>,
) -> Vec<(&'static str, &'static str)> {
    DEFAULTS
        .iter()
        .copied()
        .filter(|(name, _)| current(name).is_none_or(|value| value.is_empty()))
        .collect()
}

/// Sets every [`defaults_to_apply`] pair in this process's environment.
///
/// # Safety
///
/// No other thread may exist yet: [`std::env::set_var`] is unsound while
/// another thread reads or writes the environment. reify-gui calls this as
/// the first statement of `main`.
pub unsafe fn apply_process_defaults() {
    for (name, value) in defaults_to_apply(|name| std::env::var_os(name)) {
        // SAFETY: the caller guarantees no other thread exists.
        unsafe { std::env::set_var(name, value) }
    }
}
