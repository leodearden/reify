#![cfg(feature = "gui")]
//! `set_mode` semantics (task 9730): only Debug touches the debug server, it
//! starts it at most once, and no mode stops it.

use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use crate::debug_server::BoundDebugListener;
use crate::debug_server_slot::DebugServerSlot;
use crate::launch_mode::{Mode, ModeState, set_mode_impl};

/// A slot whose serve closure keeps each listener it is handed (so the port
/// stays bound) instead of starting a real server; the Vec is what it served.
fn recording_slot() -> (DebugServerSlot, Arc<Mutex<Vec<BoundDebugListener>>>) {
    let served = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&served);
    let slot = DebugServerSlot::new(move |listener| sink.lock().unwrap().push(listener));
    (slot, served)
}

fn serve_count(served: &Mutex<Vec<BoundDebugListener>>) -> usize {
    served.lock().unwrap().len()
}

#[test]
fn mode_names_are_the_lowercase_wire_strings() {
    for (wire, mode) in [
        ("design", Mode::Design),
        ("assistant", Mode::Assistant),
        ("debug", Mode::Debug),
    ] {
        assert_eq!(
            serde_json::from_value::<Mode>(serde_json::json!(wire)).unwrap(),
            mode
        );
    }
    for bad in ["Debug", "dev"] {
        assert!(
            serde_json::from_value::<Mode>(serde_json::json!(bad)).is_err(),
            "{bad:?} must not deserialise as a Mode"
        );
    }
}

#[test]
fn mode_state_serialises_mode_and_debug_port() {
    let live = ModeState {
        mode: Mode::Debug,
        debug_port: Some(4),
    };
    let off = ModeState {
        mode: Mode::Design,
        debug_port: None,
    };

    assert_eq!(
        serde_json::to_value(live).unwrap(),
        serde_json::json!({ "mode": "debug", "debug_port": 4 })
    );
    assert_eq!(
        serde_json::to_value(off).unwrap(),
        serde_json::json!({ "mode": "design", "debug_port": null })
    );
}

#[test]
fn design_and_assistant_never_start_the_debug_server() {
    let (slot, served) = recording_slot();

    for mode in [Mode::Design, Mode::Assistant] {
        assert_eq!(
            set_mode_impl(mode, &slot, 0),
            Ok(ModeState {
                mode,
                debug_port: None
            })
        );
    }

    assert_eq!(serve_count(&served), 0);
}

#[test]
fn debug_starts_the_server_once_and_reports_its_port() {
    let (slot, served) = recording_slot();

    let first = set_mode_impl(Mode::Debug, &slot, 0).expect("debug starts the server");
    let port = slot.live_port().expect("debug leaves the server live");
    let second = set_mode_impl(Mode::Debug, &slot, 0).expect("debug is idempotent");

    let expected = ModeState {
        mode: Mode::Debug,
        debug_port: Some(port),
    };
    assert_eq!(first, expected);
    assert_eq!(second, expected);
    assert_eq!(
        serve_count(&served),
        1,
        "a second Debug starts no second server"
    );
}

#[test]
fn no_mode_stops_a_live_debug_server() {
    let (slot, served) = recording_slot();
    set_mode_impl(Mode::Debug, &slot, 0).expect("debug starts the server");
    let port = slot.live_port().unwrap();

    assert_eq!(
        set_mode_impl(Mode::Design, &slot, 0),
        Ok(ModeState {
            mode: Mode::Design,
            debug_port: Some(port)
        })
    );
    assert_eq!(slot.live_port(), Some(port));
    assert_eq!(serve_count(&served), 1);
}

#[test]
fn debug_on_an_occupied_port_fails_naming_it() {
    let holder = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral holder");
    let held = holder.local_addr().unwrap().port();
    let (slot, served) = recording_slot();

    let err = set_mode_impl(Mode::Debug, &slot, held).expect_err("the port is taken");

    assert!(
        err.contains(&held.to_string()),
        "the error must name port {held}, got: {err}"
    );
    assert_eq!(serve_count(&served), 0);
}
