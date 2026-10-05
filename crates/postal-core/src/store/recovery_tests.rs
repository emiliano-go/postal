use super::*;
use crate::store::{MessageStore, StoredMessage};
use std::{fs, io::{Read, Seek, SeekFrom, Write}};

fn root() -> PathBuf {
    let path = std::env::temp_dir().join(format!("postal-integrity-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    fs::create_dir(&path).unwrap();
    path
}

#[test]
fn missing_and_healthy_store_are_distinct() {
    let root = root();
    let path = root.join("messages.db");
    assert_eq!(probe_message_store(&path, None).unwrap(), StoreIntegrity::Missing);
    let store = MessageStore::open(&path).unwrap();
    assert_eq!(probe_message_store(&path, None).unwrap(), StoreIntegrity::Healthy);
    drop(store);
    let error = preserve_corrupt_store(&path, None).err().unwrap();
    assert!(error.is::<StoreNotCorrupt>());
    assert!(path.exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn damaged_btree_is_reported_before_store_open_can_write() {
    let root = root();
    let path = root.join("messages.db");
    let store = MessageStore::open(&path).unwrap();
    let mut message = StoredMessage::default();
    message.header.chat = "a@s".into();
    message.header.id = "one".into();
    message.text = "kept".into();
    store.insert_message(&message).unwrap();
    let (page_size, page): (i64, i64) = store.with_test_connection(|conn| {
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        conn.query_row("SELECT pgsize,pageno FROM dbstat WHERE name='messages' ORDER BY pageno LIMIT 1", [],
            |row| Ok((row.get(0)?, row.get(1)?))).map_err(Into::into)
    }).unwrap();
    drop(store);
    let mut file = fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.seek(SeekFrom::Start(((page - 1) * page_size) as u64)).unwrap();
    file.write_all(&[0]).unwrap();
    file.sync_all().unwrap();
    drop(file);
    assert!(matches!(probe_message_store(&path, None).unwrap(), StoreIntegrity::Corrupt(_)));
    let before_open = fs::read(&path).unwrap();
    let error = MessageStore::open(&path).err().unwrap();
    assert!(error.chain().any(|cause| cause.is::<StoreIntegrityError>()));
    assert_eq!(fs::read(&path).unwrap(), before_open);
    fs::write(sidecar(&path, "-wal"), b"preserved wal").unwrap();
    fs::write(sidecar(&path, "-shm"), b"preserved shm").unwrap();
    fs::write(sidecar(&path, "-journal"), b"preserved journal").unwrap();
    let preserved = preserve_corrupt_store(&path, None).unwrap();
    assert!(!path.exists());
    assert!(preserved.join("messages.db").exists());
    assert_eq!(fs::read(preserved.join("messages.db-wal")).unwrap(), b"preserved wal");
    assert_eq!(fs::read(preserved.join("messages.db-shm")).unwrap(), b"preserved shm");
    assert_eq!(fs::read(preserved.join("messages.db-journal")).unwrap(), b"preserved journal");
    fs::write(&path, b"foreign new file").unwrap();
    assert!(create_fresh_store(&path, None).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"foreign new file");
    fs::remove_file(&path).unwrap();
    create_fresh_store(&path, None).unwrap();
    assert_eq!(probe_message_store(&path, None).unwrap(), StoreIntegrity::Healthy);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rollback_never_overwrites_a_recreated_source() {
    let root = root();
    let saved = root.join("saved");
    fs::create_dir(&saved).unwrap();
    let source = root.join("messages.db-wal");
    let target = saved.join("messages.db-wal");
    let other_source = root.join("messages.db-shm");
    let other_target = saved.join("messages.db-shm");
    fs::write(&source, b"new owner").unwrap();
    fs::write(&target, b"saved owner").unwrap();
    fs::write(&other_target, b"restorable").unwrap();
    assert!(rollback_moves(vec![(source.clone(), target.clone()), (other_source.clone(), other_target.clone())], &saved).is_err());
    assert_eq!(fs::read(source).unwrap(), b"new owner");
    assert_eq!(fs::read(target).unwrap(), b"saved owner");
    assert_eq!(fs::read(other_source).unwrap(), b"restorable");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn sidecar_symlink_is_rejected_without_moving_store() {
    let root = root();
    let path = root.join("messages.db");
    drop(MessageStore::open(&path).unwrap());
    let target = root.join("target");
    fs::write(&target, b"other file").unwrap();
    #[cfg(windows)]
    let link = std::os::windows::fs::symlink_file(&target, sidecar(&path, "-wal"));
    #[cfg(unix)]
    let link = std::os::unix::fs::symlink(&target, sidecar(&path, "-wal"));
    if link.is_ok() {
        let error = preserve_corrupt_store(&path, None).err().unwrap().to_string();
        assert!(error.contains("not regular"), "{error}");
        assert!(path.exists());
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn wrong_encryption_key_is_never_called_corruption() {
    let root = root();
    let path = root.join("messages.db");
    let right = DatabaseKey::from_bytes([0x51; 32]);
    let wrong = DatabaseKey::from_bytes([0x52; 32]);
    let store = MessageStore::open_with_key(&path, Some(&right)).unwrap();
    drop(store);
    assert_eq!(probe_message_store(&path, Some(&right)).unwrap(), StoreIntegrity::Healthy);
    assert!(probe_message_store(&path, Some(&wrong)).is_err());
    assert!(preserve_corrupt_store(&path, Some(&wrong)).is_err());
    assert!(path.exists());
    let error = MessageStore::open_with_key(&path, Some(&wrong)).err().unwrap();
    assert!(!error.chain().any(|cause| cause.is::<StoreIntegrityError>()));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn encrypted_data_page_damage_is_diagnosed_after_key_verification() {
    let root = root();
    let path = root.join("messages.db");
    let key = DatabaseKey::from_bytes([0x53; 32]);
    let store = MessageStore::open_with_key(&path, Some(&key)).unwrap();
    let mut message = StoredMessage::default();
    message.header.chat = "a@s".into();
    message.header.id = "one".into();
    message.text = "encrypted content".into();
    store.insert_message(&message).unwrap();
    let (page_size, page): (i64, i64) = store.with_test_connection(|conn| {
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        conn.query_row("SELECT pgsize,pageno FROM dbstat WHERE name='messages' ORDER BY pageno LIMIT 1", [],
            |row| Ok((row.get(0)?, row.get(1)?))).map_err(Into::into)
    }).unwrap();
    assert!(page > 1);
    drop(store);
    let mut file = fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
    let offset = ((page - 1) * page_size + 100) as u64;
    file.seek(SeekFrom::Start(offset)).unwrap();
    let mut byte = [0u8];
    file.read_exact(&mut byte).unwrap();
    file.seek(SeekFrom::Start(offset)).unwrap();
    file.write_all(&[byte[0] ^ 0x80]).unwrap();
    file.sync_all().unwrap();
    drop(file);
    let verified = open_database(&path, Some(&key), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    drop(verified);
    let before = fs::read(&path).unwrap();
    assert!(matches!(probe_message_store(&path, Some(&key)).unwrap(), StoreIntegrity::Corrupt(_)));
    assert_eq!(fs::read(&path).unwrap(), before);
    fs::remove_dir_all(root).unwrap();
}
