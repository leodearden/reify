//! The grammar for a TCP port taken from an environment variable: ASCII
//! decimal digits only, 1..=65535. A sign, whitespace or trailing text is
//! refused rather than reinterpreted, so a typo never silently names some
//! other port.

/// The port `raw` spells, or `None` unless it is 1..=65535 in decimal digits.
pub fn parse_tcp_port(raw: &str) -> Option<u16> {
    if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    raw.parse::<u16>().ok().filter(|&port| port != 0)
}
