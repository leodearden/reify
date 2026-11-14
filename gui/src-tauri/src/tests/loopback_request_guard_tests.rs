#![cfg(feature = "gui")]
//! Unit tests for `crate::loopback_request_guard::ListenerAuthorities::check`: a
//! request is admitted only when every Host names the bound loopback listener
//! and any Origin is that listener's own `http://` origin.

use std::net::SocketAddr;

use axum::http::{HeaderMap, HeaderName, HeaderValue, header};

use crate::loopback_request_guard::{ListenerAuthorities, Refusal};

const BOUND: ([u8; 4], u16) = ([127, 0, 0, 1], 3939);

fn check(headers: &HeaderMap, bound: SocketAddr) -> Result<(), Refusal> {
    ListenerAuthorities::of(bound).check(headers)
}

fn bound() -> SocketAddr {
    SocketAddr::from(BOUND)
}

fn headers(pairs: &[(HeaderName, &str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.append(
            name.clone(),
            HeaderValue::from_str(value).expect("test header values are valid"),
        );
    }
    map
}

fn host(value: &str) -> (HeaderName, &str) {
    (header::HOST, value)
}

fn origin(value: &str) -> (HeaderName, &str) {
    (header::ORIGIN, value)
}

#[test]
fn admits_requests_addressed_to_the_bound_loopback_listener() {
    let cases: &[&[(HeaderName, &str)]] = &[
        &[host("127.0.0.1:3939")],
        &[host("localhost:3939")],
        &[host("LOCALHOST:3939")],
        &[host("127.0.0.1:3939"), origin("http://127.0.0.1:3939")],
        &[host("localhost:3939"), origin("http://localhost:3939")],
    ];
    for case in cases {
        assert_eq!(
            check(&headers(case), bound()),
            Ok(()),
            "headers {case:?} address the bound listener and must be admitted"
        );
    }
}

#[test]
fn refuses_a_request_with_no_host() {
    assert_eq!(
        check(&HeaderMap::new(), bound()),
        Err(Refusal::MissingHost),
        "a request with no Host header must be refused as MissingHost"
    );
}

#[test]
fn refuses_a_host_that_does_not_name_the_bound_listener() {
    let foreign_hosts = [
        "attacker.example:3939",
        "127.0.0.1:4000",
        "localhost",
        "[::1]:3939",
        "127.0.0.1.nip.io:3939",
        "0.0.0.0:3939",
        "localhost.:3939",
    ];
    for value in foreign_hosts {
        assert_eq!(
            check(&headers(&[host(value)]), bound()),
            Err(Refusal::ForeignHost(value.to_string())),
            "Host {value:?} does not name 127.0.0.1:3939 and must be refused"
        );
    }
}

#[test]
fn refuses_a_foreign_origin_even_with_a_loopback_host() {
    let foreign_origins = [
        "http://attacker.example:3939",
        "http://localhost:8080",
        "https://localhost:3939",
        "null",
    ];
    for value in foreign_origins {
        assert_eq!(
            check(&headers(&[host("127.0.0.1:3939"), origin(value)]), bound()),
            Err(Refusal::ForeignOrigin(value.to_string())),
            "Origin {value:?} is not the listener's own origin and must be refused"
        );
    }
}

#[test]
fn the_accepted_port_is_the_bound_port() {
    let bound_elsewhere = SocketAddr::from(([127, 0, 0, 1], 4000));
    assert_eq!(
        check(&headers(&[host("127.0.0.1:3939")]), bound_elsewhere),
        Err(Refusal::ForeignHost("127.0.0.1:3939".to_string())),
        "Host 127.0.0.1:3939 names another port than the bound 4000"
    );
    assert_eq!(
        check(&headers(&[host("127.0.0.1:4000")]), bound_elsewhere),
        Ok(()),
        "Host 127.0.0.1:4000 names the bound listener"
    );
}

#[test]
fn every_host_value_must_name_the_listener() {
    let two_hosts = headers(&[host("127.0.0.1:3939"), host("attacker.example:3939")]);
    assert_eq!(
        check(&two_hosts, bound()),
        Err(Refusal::ForeignHost("attacker.example:3939".to_string())),
        "a second Host attacker.example:3939 must be refused even after a loopback one"
    );
}

#[test]
fn refuses_a_host_that_is_not_utf8() {
    let mut not_utf8 = HeaderMap::new();
    not_utf8.append(
        header::HOST,
        HeaderValue::from_bytes(b"\xff:3939").expect("obs-text is a valid header value"),
    );
    assert!(
        matches!(check(&not_utf8, bound()), Err(Refusal::ForeignHost(_))),
        "a non-UTF-8 Host cannot name the listener and must be refused as ForeignHost"
    );
}

#[test]
fn every_origin_value_must_be_the_listeners_origin() {
    let two_origins = headers(&[
        host("127.0.0.1:3939"),
        origin("http://127.0.0.1:3939"),
        origin("http://attacker.example"),
    ]);
    let foreign = "http://attacker.example".to_string();
    assert_eq!(
        check(&two_origins, bound()),
        Err(Refusal::ForeignOrigin(foreign)),
        "a second, foreign Origin must be refused even after the listener's own"
    );
}
