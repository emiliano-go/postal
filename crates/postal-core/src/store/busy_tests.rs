use super::*;
use std::{path::PathBuf, sync::mpsc, thread, time::{Duration, SystemTime, UNIX_EPOCH}};

fn database() -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("postal-busy-{}-{}", std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("messages.db");
    (dir, path)
}

fn hold_writer(path: PathBuf, ready: mpsc::Sender<()>, release: mpsc::Receiver<()>) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    ready.send(()).unwrap();
    release.recv_timeout(Duration::from_secs(15)).unwrap();
    conn.execute_batch("COMMIT").unwrap();
}

#[test]
fn write_survives_companion_lock_longer_than_old_timeout() {
    let (dir, path) = database();
    let store = MessageStore::open(&path).unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let writer = thread::spawn({ let path = path.clone(); move || hold_writer(path, ready_tx, release_rx) });
    ready_rx.recv().unwrap();
    let releaser = thread::spawn(move || { thread::sleep(Duration::from_secs(6)); release_tx.send(()).unwrap(); });
    store.set_name("held@s", "After companion").unwrap();
    releaser.join().unwrap();
    writer.join().unwrap();
    assert_eq!(store.name_for("held@s").unwrap().as_deref(), Some("After companion"));
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn busy_cap_returns_sqlite_busy_before_writer_releases() {
    let (dir, path) = database();
    let store = MessageStore::open(&path).unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let writer = thread::spawn({ let path = path.clone(); move || hold_writer(path, ready_tx, release_rx) });
    ready_rx.recv().unwrap();
    let (done_tx, done_rx) = mpsc::channel();
    let contender = thread::spawn(move || {
        store.conn.lock().unwrap().busy_timeout(Duration::from_millis(120)).unwrap();
        done_tx.send(store.set_name("held@s", "Cannot write yet")).unwrap();
    });
    let result = done_rx.recv_timeout(Duration::from_secs(3));
    release_tx.send(()).unwrap();
    writer.join().unwrap();
    contender.join().unwrap();
    let error = result.expect("busy callback did not stop before writer released").unwrap_err();
    let sqlite = error.chain().find_map(|cause| cause.downcast_ref::<rusqlite::Error>()).unwrap();
    assert!(matches!(sqlite, rusqlite::Error::SqliteFailure(code, _) if code.code == rusqlite::ErrorCode::DatabaseBusy));
    std::fs::remove_dir_all(dir).unwrap();
}
