//! Tests for [`crate::screenshot_save`].

use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::json;

use crate::screenshot_save::{png_base64, save_image_result, take_save_path};

/// A complete 1×1 transparent PNG.
const ONE_PIXEL_PNG: [u8; 67] = [
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

fn png_b64() -> String {
    STANDARD.encode(ONE_PIXEL_PNG)
}

fn data_url() -> String {
    format!("data:image/png;base64,{}", png_b64())
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

// ── take_save_path: split save_path off the params forwarded to the frontend ─

#[test]
fn without_save_path_the_params_are_forwarded_unchanged() {
    let params = json!({"viewportId": "design-main"});

    assert_eq!(take_save_path(params.clone()), Ok((None, params)));
}

#[test]
fn an_absolute_save_path_is_taken_and_every_other_param_forwarded() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("shot.png");
    let params = json!({
        "save_path": path_string(&target),
        "viewportId": "design-main",
        "testId": "diagnostics-dialog",
    });

    assert_eq!(
        take_save_path(params),
        Ok((
            Some(target),
            json!({"viewportId": "design-main", "testId": "diagnostics-dialog"})
        ))
    );
}

#[test]
fn a_relative_save_path_is_refused_naming_it() {
    let refusal = take_save_path(json!({"save_path": "shots/view.png"}))
        .expect_err("a relative path must be refused: the GUI's working directory is arbitrary");

    assert!(refusal.contains("shots/view.png"), "{refusal}");
    assert!(refusal.contains("absolute"), "{refusal}");
}

#[test]
fn a_save_path_that_is_not_a_string_is_refused() {
    assert_eq!(
        take_save_path(json!({"save_path": 42})),
        Err("save_path must be a string".to_string())
    );
}

// ── save_image_result: write the frontend's PNG and reply where it went ──────

#[test]
fn a_data_url_is_written_byte_identical_and_replaced_by_where_it_went() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("shot.png");

    let reply = save_image_result(json!({"data": data_url()}), &target).expect("saves");

    assert_eq!(
        std::fs::read(&target).expect("the file exists"),
        ONE_PIXEL_PNG
    );
    assert_eq!(
        reply,
        json!({
            "saved_to": path_string(&target),
            "bytes": ONE_PIXEL_PNG.len(),
            "mimeType": "image/png",
        })
    );
}

#[test]
fn bare_base64_without_the_data_url_prefix_is_written_the_same() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("shot.png");

    save_image_result(json!({"data": png_b64()}), &target).expect("saves");

    assert_eq!(
        std::fs::read(&target).expect("the file exists"),
        ONE_PIXEL_PNG
    );
}

#[test]
fn element_screenshot_pane_diagnostics_survive_beside_the_saved_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("element.png");

    let reply = save_image_result(
        json!({"data": data_url(), "viewportId": "design-main", "matchCount": 2}),
        &target,
    )
    .expect("saves");

    assert_eq!(
        reply,
        json!({
            "saved_to": path_string(&target),
            "bytes": ONE_PIXEL_PNG.len(),
            "mimeType": "image/png",
            "viewportId": "design-main",
            "matchCount": 2,
        })
    );
}

#[test]
fn a_result_without_image_data_is_returned_verbatim_and_nothing_is_written() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("shot.png");
    let error = json!({"error": "screenshot too large", "size": 1, "limit": 0});

    assert_eq!(save_image_result(error.clone(), &target), Ok(error));
    assert!(!target.exists());
}

#[test]
fn invalid_base64_is_refused_before_anything_is_written() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("shot.png");

    let refusal = save_image_result(
        json!({"data": "data:image/png;base64,@@not base64@@"}),
        &target,
    )
    .expect_err("undecodable data must be refused");

    assert!(refusal.contains("base64"), "{refusal}");
    assert!(!target.exists());
}

#[test]
fn a_missing_parent_directory_is_refused_naming_it_and_not_created() {
    let dir = tempfile::tempdir().expect("tempdir");
    let parent: PathBuf = dir.path().join("missing");
    let target = parent.join("shot.png");

    let refusal = save_image_result(json!({"data": data_url()}), &target)
        .expect_err("a missing parent must be refused, not created");

    assert!(refusal.contains(&path_string(&parent)), "{refusal}");
    assert!(!parent.exists());
}

// ── png_base64: the one home of the data-URL prefix ─────────────────────────

#[test]
fn png_base64_strips_the_data_url_prefix_and_leaves_bare_base64_alone() {
    let bare = png_b64();

    assert_eq!(png_base64(&data_url()), bare);
    assert_eq!(png_base64(&bare), bare);
}
