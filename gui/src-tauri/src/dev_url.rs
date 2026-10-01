//! reify-gui retargets the devUrl baked from `tauri.conf.json` to the port in
//! `REIFY_VITE_PORT` at startup, so a launcher that relocates vite also
//! relocates the page. tauri 2 reads `config.build.dev_url` at runtime for both
//! the window URL and the IPC local-origin check, which is why overriding the
//! `Context` config before `run()` is sufficient.

use std::fmt;

use tauri::utils::config::BuildConfig;

pub const VITE_PORT_ENV: &str = "REIFY_VITE_PORT";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DevUrlError {
    InvalidVitePort { raw: String },
    NoRetargetableDevUrl,
}

impl fmt::Display for DevUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DevUrlError::InvalidVitePort { raw } => write!(
                f,
                "{VITE_PORT_ENV}='{raw}' is not a TCP port (expected digits only, 1..=65535)"
            ),
            DevUrlError::NoRetargetableDevUrl => write!(
                f,
                "{VITE_PORT_ENV} is set but tauri.conf.json build.devUrl is absent or cannot \
                 carry a port, so there is nothing to retarget"
            ),
        }
    }
}

impl std::error::Error for DevUrlError {}

fn parse_vite_port(raw: &str) -> Option<u16> {
    if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    raw.parse::<u16>().ok().filter(|&port| port != 0)
}

/// Point `build.dev_url` at `vite_port`, keeping its scheme and host. `None`
/// or an empty value means "no override"; anything else must be a port, and
/// no error path mutates `build`.
pub fn retarget_to_vite_port(
    build: &mut BuildConfig,
    vite_port: Option<&str>,
) -> Result<(), DevUrlError> {
    let raw = match vite_port {
        None | Some("") => return Ok(()),
        Some(raw) => raw,
    };
    let port = parse_vite_port(raw).ok_or_else(|| DevUrlError::InvalidVitePort {
        raw: raw.to_string(),
    })?;
    let mut retargeted = build
        .dev_url
        .clone()
        .ok_or(DevUrlError::NoRetargetableDevUrl)?;
    retargeted
        .set_port(Some(port))
        .map_err(|()| DevUrlError::NoRetargetableDevUrl)?;
    build.dev_url = Some(retargeted);
    Ok(())
}

/// [`retarget_to_vite_port`] with the value read from `REIFY_VITE_PORT`.
pub fn retarget_to_vite_port_from_env(build: &mut BuildConfig) -> Result<(), DevUrlError> {
    match std::env::var_os(VITE_PORT_ENV) {
        None => retarget_to_vite_port(build, None),
        Some(os) => {
            let raw = os.to_str().ok_or_else(|| DevUrlError::InvalidVitePort {
                raw: os.to_string_lossy().into_owned(),
            })?;
            retarget_to_vite_port(build, Some(raw))
        }
    }
}
