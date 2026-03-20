use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::watcher::FileWatcher;

#[test]
fn watcher_detects_ri_file_modification() {
    let dir = std::env::temp_dir().join("reify-watcher-test-1");
    std::fs::create_dir_all(&dir).ok();
    let ri_file = dir.join("test.ri");
    std::fs::write(&ri_file, "structure Bracket {}").unwrap();

    let changed_paths: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(vec![]));
    let changed_clone = changed_paths.clone();

    let _watcher = FileWatcher::new(
        &dir,
        move |path| {
            changed_clone.lock().unwrap().push(path);
        },
    )
    .expect("should create watcher");

    // Give the watcher time to register
    std::thread::sleep(Duration::from_millis(200));

    // Modify the .ri file
    std::fs::write(&ri_file, "structure Bracket { param width = 80mm }").unwrap();

    // Wait for the event to propagate (with debounce)
    std::thread::sleep(Duration::from_millis(500));

    let paths = changed_paths.lock().unwrap();
    assert!(
        paths.iter().any(|p| p.ends_with("test.ri")),
        "should have detected test.ri change, got: {:?}",
        *paths
    );

    // Cleanup
    let _ = std::fs::remove_file(&ri_file);
    let _ = std::fs::remove_dir(&dir);
}

#[test]
fn watcher_ignores_non_ri_file_changes() {
    let dir = std::env::temp_dir().join("reify-watcher-test-2");
    std::fs::create_dir_all(&dir).ok();
    let txt_file = dir.join("notes.txt");
    std::fs::write(&txt_file, "initial content").unwrap();

    let changed_paths: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(vec![]));
    let changed_clone = changed_paths.clone();

    let _watcher = FileWatcher::new(
        &dir,
        move |path| {
            changed_clone.lock().unwrap().push(path);
        },
    )
    .expect("should create watcher");

    // Give the watcher time to register
    std::thread::sleep(Duration::from_millis(200));

    // Modify a .txt file (should be ignored)
    std::fs::write(&txt_file, "updated content").unwrap();

    // Wait long enough that we'd see the event if it weren't filtered
    std::thread::sleep(Duration::from_millis(500));

    let paths = changed_paths.lock().unwrap();
    assert!(
        paths.is_empty(),
        "should NOT have detected .txt file change, but got: {:?}",
        *paths
    );

    // Cleanup
    let _ = std::fs::remove_file(&txt_file);
    let _ = std::fs::remove_dir(&dir);
}
