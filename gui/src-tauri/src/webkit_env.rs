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
