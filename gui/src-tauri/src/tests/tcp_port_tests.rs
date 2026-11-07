//! Unit tests for `crate::tcp_port::parse_tcp_port`.

use crate::tcp_port::parse_tcp_port;

#[test]
fn accepts_decimal_ports_in_range() {
    for (raw, port) in [("1", 1u16), ("80", 80), ("1420", 1420), ("65535", 65535)] {
        assert_eq!(parse_tcp_port(raw), Some(port), "{raw:?} is a valid port");
    }
}

#[test]
fn rejects_anything_but_decimal_digits_in_range() {
    for raw in [
        "",
        "0",
        "65536",
        "99999999999",
        "abc",
        " 5173",
        "5173 ",
        "+5173",
        "-1",
        "51 73",
        "5173x",
        "0x50",
    ] {
        assert_eq!(parse_tcp_port(raw), None, "{raw:?} must be rejected");
    }
}
