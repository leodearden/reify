//! The jcodemunch seam a run binds — a live session or the inert stub — and,
//! when a run that needs jcodemunch binds the stub, why. The binary decides
//! which; this module names the outcomes and renders the reason that both the
//! run's stderr breadcrumb and its run-level unexamined findings carry.

use crate::jcodemunch_client::{LoadError, RealJCodemunchOps};
use crate::{JCodemunchOps, NoopJCodemunchOps};

/// Why a run that needs jcodemunch binds the inert stub instead of a live
/// session. Every arm is involuntary: `--no-jcodemunch` is [`Seam::OptedOut`].
pub enum SeamUnavailable {
    /// The endpoint never answered: connection refused or reset, timeout.
    Unreachable { url: String, error: LoadError },
    /// The endpoint answered, but not as a live MCP seam: a non-2xx status, a
    /// body broken off mid-reply, or a body that is not an MCP
    /// `InitializeResult`.
    HandshakeRefused { url: String, error: LoadError },
    /// The handshake succeeded but the §4.3 gate refused the index; `refusal`
    /// is its rendered message, carrying its `E_JC_INDEX_` marker when the
    /// index itself was judged.
    IndexUnusable { refusal: String },
}

impl SeamUnavailable {
    /// Classify a failed handshake against `url` by whether the endpoint
    /// answered at all.
    pub fn from_handshake(url: &str, error: LoadError) -> Self {
        let url = url.to_string();
        match error {
            LoadError::Http(_) => Self::Unreachable { url, error },
            LoadError::Status(_) | LoadError::Body(_) | LoadError::Protocol(_) => {
                Self::HandshakeRefused { url, error }
            }
        }
    }
}

impl std::fmt::Display for SeamUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable { url, error } => {
                write!(f, "jcodemunch unreachable at '{url}': {error}")
            }
            Self::HandshakeRefused { url, error } => {
                write!(f, "jcodemunch handshake refused at '{url}': {error}")
            }
            Self::IndexUnusable { refusal } => f.write_str(refusal),
        }
    }
}

/// The jcodemunch seam a run binds, and why.
pub enum Seam {
    Live(RealJCodemunchOps),
    /// No selected detector queries jcodemunch, or the run is `--pre-done`.
    NotNeeded,
    /// `--no-jcodemunch`: a declared scope reduction.
    OptedOut,
    Unavailable(SeamUnavailable),
}

impl Seam {
    /// The ops every detector queries: the live session, or the inert stub.
    pub fn ops(&self) -> &dyn JCodemunchOps {
        match self {
            Seam::Live(ops) => ops,
            Seam::NotNeeded | Seam::OptedOut | Seam::Unavailable(_) => &NoopJCodemunchOps,
        }
    }

    pub fn unavailable(&self) -> Option<&SeamUnavailable> {
        match self {
            Seam::Unavailable(reason) => Some(reason),
            Seam::Live(_) | Seam::NotNeeded | Seam::OptedOut => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An endpoint that never answered is unreachable; one that answered —
    /// with a non-2xx status or a non-MCP body — refused the handshake. The
    /// label sends the operator to different fixes, so it must follow the
    /// error's variant, and name the endpoint.
    #[test]
    fn a_failed_handshake_is_labelled_by_whether_the_endpoint_answered() {
        let url = "http://127.0.0.1:8917/mcp";
        let cases = [
            (
                LoadError::Http("POST: Connection refused".into()),
                "jcodemunch unreachable at",
            ),
            (
                LoadError::Status("POST: status code 404".into()),
                "jcodemunch handshake refused at",
            ),
            (
                LoadError::Body("read body: response body closed early".into()),
                "jcodemunch handshake refused at",
            ),
            (
                LoadError::Protocol("initialize: not an InitializeResult".into()),
                "jcodemunch handshake refused at",
            ),
        ];
        for (error, label) in cases {
            let shown = error.to_string();
            let rendered = SeamUnavailable::from_handshake(url, error).to_string();
            assert!(
                rendered.starts_with(&format!("{label} '{url}': ")),
                "`{shown}` must render under `{label} '{url}'`; got {rendered:?}"
            );
            assert!(
                rendered.ends_with(&shown),
                "the rendering must carry the error itself; got {rendered:?}"
            );
        }
    }
}
