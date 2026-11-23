//! The process's one reify-debug server (task 9730): started at most once,
//! by `REIFY_DEBUG=1` at launch or by selecting Debug mode at runtime, both
//! through [`DebugServerSlot::ensure_live`].

use std::sync::Mutex;

use crate::debug_server::{BoundDebugListener, debug_endpoint_url};

type Serve = Box<dyn Fn(BoundDebugListener) + Send + Sync>;

/// Holds whether the debug server is live and how to start it. `serve` is
/// handed each listener this slot binds, which happens at most once, so a
/// process never runs two debug servers.
pub struct DebugServerSlot {
    live_port: Mutex<Option<u16>>,
    serve: Serve,
}

impl DebugServerSlot {
    pub fn new(serve: impl Fn(BoundDebugListener) + Send + Sync + 'static) -> Self {
        Self {
            live_port: Mutex::new(None),
            serve: Box::new(serve),
        }
    }

    /// The live port, binding `127.0.0.1:port` and serving on it first if the
    /// server is not live yet. Once live, `port` is ignored. A failed bind
    /// leaves the slot not live, so a later call can still start it.
    pub fn ensure_live(&self, port: u16) -> Result<u16, String> {
        let mut live_port = self.live_port.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(live) = *live_port {
            return Ok(live);
        }
        let listener = BoundDebugListener::bind(port)?;
        let bound = listener.port();
        *live_port = Some(bound);
        (self.serve)(listener);
        Ok(bound)
    }

    /// `Some` only once this process holds the listener (task 7816's "live == bound").
    pub fn live_port(&self) -> Option<u16> {
        *self.live_port.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn endpoint_url(&self) -> Option<String> {
        self.live_port().map(debug_endpoint_url)
    }
}
