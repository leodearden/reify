//! Tests for the environment the host hands the Claude sidecar
//! (`crate::claude_bridge::{compute_sidecar_env, apply_sidecar_env}`): the
//! reify-debug endpoint reaches the sidecar only when the host's server is live.

use std::path::Path;

use crate::claude_bridge::{apply_sidecar_env, compute_sidecar_env, spawn_sidecar_impl};

const LIVE_URL: &str = "http://127.0.0.1:4500/mcp";

fn pair(k: &str, v: &str) -> (String, String) {
    (k.to_string(), v.to_string())
}

fn command_envs(cmd: &tokio::process::Command) -> Vec<(String, Option<String>)> {
    cmd.as_std()
        .get_envs()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.map(|v| v.to_string_lossy().into_owned()),
            )
        })
        .collect()
}

mod compute {
    use super::*;

    #[test]
    fn workspace_only_when_no_landlock_and_no_live_debug_server() {
        assert_eq!(
            compute_sidecar_env(Path::new("/ws"), None, None),
            vec![pair("REIFY_WORKSPACE", "/ws")],
        );
    }

    #[test]
    fn landlock_exec_follows_workspace() {
        assert_eq!(
            compute_sidecar_env(Path::new("/ws"), Some(Path::new("/sb/le.py")), None),
            vec![
                pair("REIFY_WORKSPACE", "/ws"),
                pair("REIFY_LANDLOCK_EXEC", "/sb/le.py"),
            ],
        );
    }

    #[test]
    fn a_live_debug_server_appends_its_mcp_url_last() {
        assert_eq!(
            compute_sidecar_env(
                Path::new("/ws"),
                Some(Path::new("/sb/le.py")),
                Some(LIVE_URL)
            ),
            vec![
                pair("REIFY_WORKSPACE", "/ws"),
                pair("REIFY_LANDLOCK_EXEC", "/sb/le.py"),
                pair("REIFY_DEBUG_MCP_URL", LIVE_URL),
            ],
        );
    }
}

mod apply {
    use super::*;

    #[test]
    fn sets_workspace_only_when_no_landlock_exec() {
        let mut cmd = tokio::process::Command::new("/bin/true");
        apply_sidecar_env(&mut cmd, Path::new("/ws"), None, None);
        let envs = command_envs(&cmd);

        assert!(
            envs.contains(&("REIFY_WORKSPACE".to_string(), Some("/ws".to_string()))),
            "REIFY_WORKSPACE=/ws should be set: {envs:?}"
        );
        assert!(
            !envs.iter().any(|(k, _)| k == "REIFY_LANDLOCK_EXEC"),
            "REIFY_LANDLOCK_EXEC should not be set: {envs:?}"
        );
    }

    #[test]
    fn sets_both_when_landlock_exec_some() {
        let mut cmd = tokio::process::Command::new("/bin/true");
        apply_sidecar_env(
            &mut cmd,
            Path::new("/ws"),
            Some(Path::new("/sb/le.py")),
            None,
        );
        let envs = command_envs(&cmd);

        assert!(
            envs.contains(&("REIFY_WORKSPACE".to_string(), Some("/ws".to_string()))),
            "REIFY_WORKSPACE=/ws should be set: {envs:?}"
        );
        assert!(
            envs.contains(&(
                "REIFY_LANDLOCK_EXEC".to_string(),
                Some("/sb/le.py".to_string())
            )),
            "REIFY_LANDLOCK_EXEC=/sb/le.py should be set: {envs:?}"
        );
    }

    #[test]
    fn removes_an_inherited_debug_mcp_url_when_the_server_is_not_live() {
        let mut cmd = tokio::process::Command::new("/bin/true");
        apply_sidecar_env(&mut cmd, Path::new("/ws"), None, None);

        assert!(
            command_envs(&cmd).contains(&("REIFY_DEBUG_MCP_URL".to_string(), None)),
            "REIFY_DEBUG_MCP_URL must be explicitly removed so an inherited value cannot fake liveness"
        );
    }

    #[test]
    fn sets_the_debug_mcp_url_when_the_server_is_live() {
        let mut cmd = tokio::process::Command::new("/bin/true");
        apply_sidecar_env(&mut cmd, Path::new("/ws"), None, Some(LIVE_URL));

        assert!(
            command_envs(&cmd).contains(&(
                "REIFY_DEBUG_MCP_URL".to_string(),
                Some(LIVE_URL.to_string())
            )),
            "REIFY_DEBUG_MCP_URL should carry the live endpoint"
        );
    }
}

#[tokio::test]
async fn spawn_sidecar_impl_with_workspace_and_no_landlock_returns_error_for_missing_binary() {
    let result = spawn_sidecar_impl(
        Path::new("/tmp/no-such-binary"),
        |_name: String, _payload: serde_json::Value| {},
        Path::new("/tmp/ws"),
        None,
        None,
    )
    .await;

    let err = result.err().expect("Expected error for missing binary");
    assert!(
        err.contains("Failed to spawn sidecar"),
        "Error should mention 'Failed to spawn sidecar': {err}"
    );
}
