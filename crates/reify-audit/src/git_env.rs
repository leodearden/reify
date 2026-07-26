//! (step-1 RED: this module intentionally contains ONLY its test module —
//! `command`, `sanitize` and `REPO_REDIRECT_VARS` do not exist yet, so this
//! file does not compile. step-2 supplies the implementation and the real
//! module doc.)

#[cfg(test)]
mod tests {
    use super::{REPO_REDIRECT_VARS, command, sanitize};
    use std::ffi::OsStr;
    use std::path::Path;
    use std::process::Command;

    /// The three vars git exports into a hook's entire process tree, each of
    /// which silently overrides an explicit `-C <root>`. This is a normative
    /// FLOOR asserted by containment, not a restatement of the constant — the
    /// sanitized set may grow without a lockstep edit here.
    const HOOK_EXPORTED_FLOOR: &[&str] = &["GIT_DIR", "GIT_INDEX_FILE", "GIT_WORK_TREE"];

    /// Collect the vars a `Command` has marked for REMOVAL. `std` encodes an
    /// `env_remove` as a `(key, None)` pair in `get_envs()`; an overwrite would
    /// be `(key, Some(value))`.
    fn removed_vars(cmd: &Command) -> Vec<String> {
        cmd.get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn command_targets_git_dash_c_at_the_requested_root() {
        let cmd = command(Path::new("/some/root"));

        assert_eq!(cmd.get_program(), OsStr::new("git"));

        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert!(
            args.len() >= 2,
            "expected at least the `-C <root>` prefix, got {:?}",
            args
        );
        assert_eq!(args[0], OsStr::new("-C"));
        assert_eq!(args[1], OsStr::new("/some/root"));
    }

    #[test]
    fn command_removes_every_repo_redirect_var() {
        let cmd = command(Path::new("/some/root"));
        let removed = removed_vars(&cmd);

        for var in REPO_REDIRECT_VARS {
            assert!(
                removed.iter().any(|r| r == var),
                "`{}` must be REMOVED (env_remove -> `(key, None)`), not merely \
                 overwritten; removals seen: {:?}",
                var,
                removed
            );
        }
    }

    #[test]
    fn repo_redirect_vars_covers_the_hook_exported_floor() {
        for var in HOOK_EXPORTED_FLOOR {
            assert!(
                REPO_REDIRECT_VARS.contains(var),
                "REPO_REDIRECT_VARS must contain at least `{}` — git exports it \
                 into a hook's process tree, where it overrides `-C <root>`; \
                 current set: {:?}",
                var,
                REPO_REDIRECT_VARS
            );
        }
    }

    #[test]
    fn sanitize_applies_the_same_removals_to_a_caller_built_command() {
        // A caller that needs a non-`-C` shape can still opt in.
        let mut cmd = Command::new("git");
        cmd.arg("--version");
        sanitize(&mut cmd);

        let removed = removed_vars(&cmd);
        for var in REPO_REDIRECT_VARS {
            assert!(
                removed.iter().any(|r| r == var),
                "sanitize() must remove `{}`; removals seen: {:?}",
                var,
                removed
            );
        }
    }

    #[test]
    fn sanitize_returns_the_command_for_chaining() {
        let mut cmd = Command::new("git");
        let chained = sanitize(&mut cmd).arg("status");
        assert_eq!(
            chained.get_args().collect::<Vec<_>>(),
            vec![OsStr::new("status")]
        );
    }
}
