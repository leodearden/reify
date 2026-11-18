//! Tests for the WebKit environment defaults reify-gui applies to its own
//! process (`crate::webkit_env::defaults_to_apply`). Driven only through the
//! lookup closure, never the real process environment.

use std::ffi::OsString;

use crate::webkit_env::defaults_to_apply;

const DMABUF: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";

fn lookup_with(name: &'static str, value: &'static str) -> impl Fn(&str) -> Option<OsString> {
    move |asked| (asked == name).then(|| OsString::from(value))
}

#[test]
fn unset_variable_gets_the_default() {
    assert_eq!(defaults_to_apply(|_| None), vec![(DMABUF, "1")]);
}

#[test]
fn empty_value_counts_as_unset() {
    assert_eq!(
        defaults_to_apply(lookup_with(DMABUF, "")),
        vec![(DMABUF, "1")]
    );
}

#[test]
fn explicit_zero_is_never_overridden() {
    assert_eq!(
        defaults_to_apply(lookup_with(DMABUF, "0")),
        Vec::<(&str, &str)>::new()
    );
}

#[test]
fn explicit_one_needs_no_default() {
    assert_eq!(
        defaults_to_apply(lookup_with(DMABUF, "1")),
        Vec::<(&str, &str)>::new()
    );
}
