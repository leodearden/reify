#![cfg(feature = "gui")]
//! A recording [`DebugServerSlot`] and the [`Served`] record of what it served,
//! shared by the debug-listener and `set_mode` tests.

use std::sync::{Arc, Mutex};

use crate::debug_server::BoundDebugListener;
use crate::debug_server_slot::DebugServerSlot;

/// What a [`recording_slot`] served. It holds every listener the slot handed
/// over, so each served port stays bound while the record is alive.
pub(crate) struct Served(Arc<Mutex<Vec<BoundDebugListener>>>);

impl Served {
    /// The ports of the served listeners, in serve order.
    pub(crate) fn ports(&self) -> Vec<u16> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(BoundDebugListener::port)
            .collect()
    }
}

/// A slot whose serve closure records every listener it is handed instead of
/// starting a real server.
pub(crate) fn recording_slot() -> (DebugServerSlot, Served) {
    let served = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&served);
    let slot = DebugServerSlot::new(move |listener| sink.lock().unwrap().push(listener));
    (slot, Served(served))
}
