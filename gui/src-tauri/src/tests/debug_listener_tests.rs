#![cfg(feature = "gui")]
//! Unit tests for `crate::debug_server::bind_debug_listener`: the reify-debug
//! server counts as live only when this process actually holds the listener.

use std::net::{TcpListener, TcpStream};

use crate::debug_server::{bind_debug_listener, debug_endpoint_url};

#[test]
fn disabled_debug_never_binds_even_on_a_free_port() {
    assert!(bind_debug_listener(false, 0).is_none());
}

#[test]
fn an_occupied_port_is_not_live() {
    let holder = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral holder");
    let port = holder.local_addr().expect("holder addr").port();

    assert!(
        bind_debug_listener(true, port).is_none(),
        "port {port} is held by another listener, so this process is not live"
    );
}

#[test]
fn a_successful_bind_reports_its_real_port_and_url_and_accepts_connections() {
    let listener = bind_debug_listener(true, 0).expect("ephemeral bind succeeds");

    assert_ne!(
        listener.port(),
        0,
        "bind(0) must report the real ephemeral port"
    );
    assert_eq!(listener.endpoint_url(), debug_endpoint_url(listener.port()));
    TcpStream::connect(("127.0.0.1", listener.port()))
        .expect("the listener is really bound and accepts a connection");
}
