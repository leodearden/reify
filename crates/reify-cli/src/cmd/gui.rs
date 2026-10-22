#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_gui_command_sets_reify_debug_when_debug_true() {
        // Verifies that `build_gui_command(.., debug=true)` sets REIFY_DEBUG=1
        // in the child Command's env, without spawning a subprocess.
        let path = std::path::Path::new("/tmp/fake-reify-gui");
        let cmd = build_gui_command(path, "x.ri", true);
        let envs: Vec<(std::ffi::OsString, Option<std::ffi::OsString>)> = cmd
            .get_envs()
            .map(|(k, v)| (k.to_os_string(), v.map(|val| val.to_os_string())))
            .collect();
        let reify_debug_set = envs.iter().any(|(k, v)| {
            k == std::ffi::OsStr::new("REIFY_DEBUG")
                && v.as_deref() == Some(std::ffi::OsStr::new("1"))
        });
        assert!(
            reify_debug_set,
            "REIFY_DEBUG=1 must be set in Command env when debug=true; got envs: {:?}",
            envs
        );
    }

    #[test]
    fn build_gui_command_does_not_set_reify_debug_when_debug_false() {
        // Verifies that `build_gui_command(.., debug=false)` does NOT add
        // REIFY_DEBUG to the child Command's env (parent env is inherited
        // automatically by the OS spawn machinery; we only assert that we
        // don't *override* it here).
        let path = std::path::Path::new("/tmp/fake-reify-gui");
        let cmd = build_gui_command(path, "x.ri", false);
        let has_reify_debug = cmd
            .get_envs()
            .any(|(k, _)| k == std::ffi::OsStr::new("REIFY_DEBUG"));
        assert!(
            !has_reify_debug,
            "REIFY_DEBUG must NOT be set in Command env when debug=false"
        );
    }
}
