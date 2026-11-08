#![cfg(feature = "gui")]
//! A recording [`DebugServerSlot`] and the accessor that reads what it served,
//! shared by the debug-listener and `set_mode` tests.

use std::sync::{Arc, Mutex};

use crate::debug_server::BoundDebugListener;
use crate::debug_server_slot::DebugServerSlot;

/// A slot whose serve closure keeps every listener it is handed (so its port
/// stays bound) instead of starting a real server; the Vec is what it served.
pub(crate) fn recording_slot() -> (DebugServerSlot, Arc<Mutex<Vec<BoundDebugListener>>>) {
    let served = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&served);
    let slot = DebugServerSlot::new(move |listener| sink.lock().unwrap().push(listener));
    (slot, served)
}

/// The ports of the listeners a [`recording_slot`] served, in serve order.
pub(crate) fn served_ports(served: &Mutex<Vec<BoundDebugListener>>) -> Vec<u16> {
    served
        .lock()
        .unwrap()
        .iter()
        .map(BoundDebugListener::port)
        .collect()
}
