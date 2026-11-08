#![cfg(feature = "gui")]
//! Unit tests for `crate::debug_server_slot::DebugServerSlot`: the process's one
//! reify-debug server, startable at runtime and counted as live only once this
//! process actually holds the listener (task 7816's "live == bound").

use std::net::{TcpListener, TcpStream};

use super::debug_server_slot_fixtures::{recording_slot, served_ports};
use crate::debug_server::debug_endpoint_url;

#[test]
fn a_fresh_slot_is_not_live() {
    let (slot, served) = recording_slot();

    assert_eq!(slot.live_port(), None);
    assert_eq!(slot.endpoint_url(), None);
    assert!(served_ports(&served).is_empty());
}

#[test]
fn ensure_live_binds_serves_once_and_reports_its_real_port_and_url() {
    let (slot, served) = recording_slot();

    let port = slot.ensure_live(0).expect("an ephemeral bind succeeds");

    assert_ne!(
        port, 0,
        "ensure_live(0) must report the real ephemeral port"
    );
    assert_eq!(served_ports(&served), vec![port]);
    assert_eq!(slot.live_port(), Some(port));
    assert_eq!(slot.endpoint_url(), Some(debug_endpoint_url(port)));
    TcpStream::connect(("127.0.0.1", port))
        .expect("the listener is really bound and accepts a connection");
}

#[test]
fn a_live_slot_never_starts_a_second_server() {
    let (slot, served) = recording_slot();
    let port = slot.ensure_live(0).expect("first start");
    let other = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();

    assert_eq!(slot.ensure_live(0), Ok(port));
    assert_eq!(slot.ensure_live(other), Ok(port));

    assert_eq!(served_ports(&served), vec![port], "serve runs at most once");
}

#[test]
fn an_occupied_port_is_not_live_and_does_not_wedge_the_slot() {
    let holder = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral holder");
    let held = holder.local_addr().expect("holder addr").port();
    let (slot, served) = recording_slot();

    let err = slot
        .ensure_live(held)
        .expect_err("port is held by another listener");

    assert!(
        err.contains(&held.to_string()),
        "the error must name port {held}, got: {err}"
    );
    assert!(
        served_ports(&served).is_empty(),
        "a failed bind serves nothing"
    );
    assert_eq!(slot.live_port(), None);
    assert_eq!(slot.endpoint_url(), None);

    let port = slot
        .ensure_live(0)
        .expect("a later attempt still starts the server");
    assert_eq!(served_ports(&served), vec![port]);
}
