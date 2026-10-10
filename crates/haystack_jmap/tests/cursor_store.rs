//! Tests for the durable `Email/changes` state cursor.
//!
//! The file-backed store is exercised against a real temporary directory, so a
//! restart of the process finds the cursor it wrote. Writing goes through a
//! temporary file and a rename, and the test confirms that a crash-style
//! leftover temporary file does not corrupt the stored state.

use haystack_jmap::{FileCursorStore, MemoryCursorStore, StateCursorStore};
use std::path::PathBuf;

/// Creates a unique temporary directory using the process id and a counter.
fn temp_dir(tag: &str) -> PathBuf {
    let unique = format!(
        "haystack_jmap_cursor_{}_{}_{}",
        tag,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let path = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn memory_store_round_trips_and_clears() {
    let store = MemoryCursorStore::new();
    assert_eq!(store.load("acc-1").unwrap(), None);

    store.store("acc-1", "state-a").unwrap();
    assert_eq!(store.load("acc-1").unwrap().as_deref(), Some("state-a"));
    assert_eq!(store.load_state("acc-1").unwrap().unwrap().state, "state-a");

    // A different account is unaffected.
    store.store("acc-2", "state-b").unwrap();
    assert_eq!(store.load("acc-1").unwrap().as_deref(), Some("state-a"));
    assert_eq!(store.load("acc-2").unwrap().as_deref(), Some("state-b"));

    store.clear("acc-1").unwrap();
    assert_eq!(store.load("acc-1").unwrap(), None);
    assert_eq!(store.load("acc-2").unwrap().as_deref(), Some("state-b"));
}

#[test]
fn file_store_survives_reopening_the_file() {
    let dir = temp_dir("reopen");
    let path = dir.join("cursor.json");

    {
        let store = FileCursorStore::open(&path).unwrap();
        store.store("acc-1", "state-1").unwrap();
    }

    // A new handle reads the state written by the previous one.
    let reopened = FileCursorStore::open(&path).unwrap();
    assert_eq!(reopened.load("acc-1").unwrap().as_deref(), Some("state-1"));

    reopened.store("acc-1", "state-2").unwrap();
    reopened.clear("acc-1").unwrap();

    let cleared = FileCursorStore::open(&path).unwrap();
    assert_eq!(cleared.load("acc-1").unwrap(), None);
}

#[test]
fn file_store_handles_several_accounts_and_an_empty_file() {
    let dir = temp_dir("multi");
    let path = dir.join("cursor.json");

    let store = FileCursorStore::open(&path).unwrap();
    store.store("acc-1", "s1").unwrap();
    store.store("acc-2", "s2").unwrap();
    store.store("acc-3", "s3").unwrap();

    let reopened = FileCursorStore::open(&path).unwrap();
    assert_eq!(reopened.load("acc-1").unwrap().as_deref(), Some("s1"));
    assert_eq!(reopened.load("acc-2").unwrap().as_deref(), Some("s2"));
    assert_eq!(reopened.load("acc-3").unwrap().as_deref(), Some("s3"));

    // An empty existing file is treated as "no cursors yet" rather than a
    // parse error.
    let empty_path = dir.join("empty.json");
    std::fs::write(&empty_path, "").unwrap();
    let empty = FileCursorStore::open(&empty_path).unwrap();
    assert_eq!(empty.load("acc-1").unwrap(), None);
}

#[test]
fn file_store_leaves_a_leftover_temp_file_behind_without_corrupting_state() {
    let dir = temp_dir("stale-temp");
    let path = dir.join("cursor.json");

    let store = FileCursorStore::open(&path).unwrap();
    store.store("acc-1", "state-1").unwrap();

    // A stale temporary file from an interrupted write must not be read back.
    let stale_temp = path.with_extension("tmp");
    std::fs::write(&stale_temp, "not json").unwrap();

    let reopened = FileCursorStore::open(&path).unwrap();
    assert_eq!(reopened.load("acc-1").unwrap().as_deref(), Some("state-1"));

    // A later write replaces the stale temporary file cleanly.
    reopened.store("acc-1", "state-2").unwrap();
    assert_eq!(
        FileCursorStore::open(&path)
            .unwrap()
            .load("acc-1")
            .unwrap()
            .as_deref(),
        Some("state-2")
    );
}

#[test]
fn load_state_reports_the_account_it_belongs_to() {
    let store = MemoryCursorStore::new();
    store.store("acc-1", "state-a").unwrap();

    let state = store.load_state("acc-1").unwrap().unwrap();
    assert_eq!(state.account_id, "acc-1");
    assert_eq!(state.state, "state-a");
    assert!(store.load_state("missing").unwrap().is_none());
}
