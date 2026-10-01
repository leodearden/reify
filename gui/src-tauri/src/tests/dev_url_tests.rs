#![cfg(feature = "gui")]
//! Unit tests for `crate::dev_url`: retargeting the devUrl baked from
//! `tauri.conf.json` to the port named by `REIFY_VITE_PORT`.

use tauri::Url;
use tauri::utils::config::BuildConfig;

use crate::dev_url::{DevUrlError, VITE_PORT_ENV, retarget_to_vite_port};

const BAKED_DEV_URL: &str = "http://localhost:1420";

fn build_with_dev_url(url: &str) -> BuildConfig {
    BuildConfig {
        dev_url: Some(url.parse::<Url>().expect("fixture devUrl parses")),
        ..Default::default()
    }
}

fn dev_url_of(build: &BuildConfig) -> &Url {
    build.dev_url.as_ref().expect("devUrl present")
}

#[test]
fn vite_port_env_is_the_user_facing_variable() {
    assert_eq!(VITE_PORT_ENV, "REIFY_VITE_PORT");
}

#[test]
fn retargets_only_the_port() {
    let mut build = build_with_dev_url(BAKED_DEV_URL);

    retarget_to_vite_port(&mut build, Some("5173")).expect("5173 is a valid port");

    let url = dev_url_of(&build);
    assert_eq!(url.scheme(), "http");
    assert_eq!(url.host_str(), Some("localhost"));
    assert_eq!(url.port_or_known_default(), Some(5173));
}

#[test]
fn absent_or_empty_value_leaves_dev_url_untouched() {
    let original = build_with_dev_url(BAKED_DEV_URL);

    for value in [None, Some("")] {
        let mut build = original.clone();
        retarget_to_vite_port(&mut build, value)
            .unwrap_or_else(|e| panic!("{value:?} must mean \"no override\", got {e}"));
        assert_eq!(build, original, "{value:?} must leave devUrl untouched");
    }
}

#[test]
fn port_bounds_accepted() {
    for (raw, port) in [("1", 1u16), ("65535", 65535u16)] {
        let mut build = build_with_dev_url(BAKED_DEV_URL);
        retarget_to_vite_port(&mut build, Some(raw))
            .unwrap_or_else(|e| panic!("{raw} is a valid port, got {e}"));
        assert_eq!(dev_url_of(&build).port_or_known_default(), Some(port));
    }
}

#[test]
fn rejects_non_port_values() {
    let original = build_with_dev_url(BAKED_DEV_URL);

    for raw in [
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
    ] {
        let mut build = original.clone();
        let err = retarget_to_vite_port(&mut build, Some(raw))
            .expect_err(&format!("{raw:?} must be rejected"));

        assert_eq!(
            err,
            DevUrlError::InvalidVitePort {
                raw: raw.to_string()
            }
        );
        assert_eq!(build, original, "{raw:?} must not partially mutate devUrl");
        let message = err.to_string();
        assert!(
            message.contains("REIFY_VITE_PORT"),
            "error must name the variable: {message}"
        );
        assert!(
            message.contains(raw),
            "error must quote the rejected value {raw:?}: {message}"
        );
    }
}

#[test]
fn no_dev_url_to_retarget_is_an_error() {
    let mut build = BuildConfig::default();

    let err = retarget_to_vite_port(&mut build, Some("5173"))
        .expect_err("an override with no devUrl to retarget must be refused");

    assert!(matches!(err, DevUrlError::NoRetargetableDevUrl));
    let message = err.to_string();
    assert!(
        message.contains("REIFY_VITE_PORT") && message.contains("devUrl"),
        "error must name the variable and devUrl: {message}"
    );

    let mut untouched = BuildConfig::default();
    retarget_to_vite_port(&mut untouched, None).expect("no override needs no devUrl");
    assert_eq!(untouched, BuildConfig::default());
}

#[test]
fn shipped_tauri_conf_dev_url_is_retargetable() {
    let conf: serde_json::Value = serde_json::from_str(include_str!("../../tauri.conf.json"))
        .expect("tauri.conf.json is valid JSON");
    let shipped = conf["build"]["devUrl"]
        .as_str()
        .expect("tauri.conf.json carries build.devUrl")
        .parse::<Url>()
        .expect("shipped devUrl parses");
    let mut build = BuildConfig {
        dev_url: Some(shipped.clone()),
        ..Default::default()
    };

    retarget_to_vite_port(&mut build, Some("5173")).expect("shipped devUrl is retargetable");

    let url = dev_url_of(&build);
    assert_eq!(url.port_or_known_default(), Some(5173));
    assert_eq!(url.scheme(), shipped.scheme());
    assert_eq!(url.host_str(), shipped.host_str());
}
