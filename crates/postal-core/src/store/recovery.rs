use std::{ffi::OsString, fmt, fs, path::{Path, PathBuf}};

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};

use crate::database_crypto::{DatabaseKey, open_database};
use super::MessageStore;

#[derive(Debug)]
pub struct StoreIntegrityError {
    pub path: PathBuf,
    pub diagnosis: String,
}

impl fmt::Display for StoreIntegrityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "message store integrity check failed at {}: {}", self.path.display(), self.diagnosis)
    }
}

impl std::error::Error for StoreIntegrityError {}

#[derive(Debug)]
pub struct StoreNotCorrupt;

impl fmt::Display for StoreNotCorrupt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result { formatter.write_str("message store has no confirmed corruption") }
}

impl std::error::Error for StoreNotCorrupt {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreIntegrity {
    Missing,
    Healthy,
    Corrupt(String),
}

pub(super) fn check_integrity(conn: &Connection, path: &Path) -> Result<()> {
    match conn.query_row("PRAGMA quick_check(1)", [], |row| row.get::<_, String>(0)) {
        Ok(result) if result == "ok" => Ok(()),
        Ok(diagnosis) => Err(StoreIntegrityError { path: path.into(), diagnosis }.into()),
        Err(error) if matches!(&error, rusqlite::Error::SqliteFailure(code, _)
            if matches!(code.extended_code & 0xff, rusqlite::ffi::SQLITE_CORRUPT | rusqlite::ffi::SQLITE_NOTADB)) =>
                Err(StoreIntegrityError { path: path.into(), diagnosis: error.to_string() }.into()),
        Err(error) => Err(error.into()),
    }
}

fn probe_direct(path: &Path, key: Option<&DatabaseKey>, copied: bool) -> Result<StoreIntegrity> {
    let flags = if copied { OpenFlags::default() } else { OpenFlags::SQLITE_OPEN_READ_ONLY };
    let conn = open_database(path, key, flags)?;
    match check_integrity(&conn, path) {
        Ok(()) => Ok(StoreIntegrity::Healthy),
        Err(error) => match error.downcast::<StoreIntegrityError>() {
            Ok(corrupt) => Ok(StoreIntegrity::Corrupt(corrupt.diagnosis)),
            Err(error) => Err(error),
        },
    }
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn copy_probe_set(path: &Path, key: Option<&DatabaseKey>) -> Result<PathBuf> {
    crate::database_crypto::check_database_path(path, key)?;
    let parent = path.parent().context("message store has no parent folder")?;
    let name = path.file_name().context("message store has no file name")?;
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
    let probe = parent.join(format!("messages-probe-{}-{nonce}", std::process::id()));
    fs::create_dir(&probe)?;
    let probe_path = probe.join(name);
    let copied = (|| -> Result<()> {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let source = sidecar(path, suffix);
            let metadata = match fs::symlink_metadata(&source) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound && !suffix.is_empty() => continue,
                Err(error) => return Err(error.into()),
            };
            anyhow::ensure!(metadata.file_type().is_file(), "message store file is not regular: {}", source.display());
            fs::copy(&source, probe.join(source.file_name().context("message store file has no name")?))?;
        }
        Ok(())
    })();
    if let Err(error) = copied {
        let _ = clear_probe_set(&probe_path);
        return Err(error);
    }
    Ok(probe_path)
}

fn clear_probe_set(path: &Path) -> Result<()> {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let candidate = sidecar(path, suffix);
        if candidate.exists() { fs::remove_file(candidate)?; }
    }
    fs::remove_dir(path.parent().context("probe has no parent")?)?;
    Ok(())
}

pub fn probe_message_store(path: &Path, key: Option<&DatabaseKey>) -> Result<StoreIntegrity> {
    if path == Path::new(":memory:") { return probe_direct(path, key, false); }
    if !path.exists() {
        crate::database_crypto::check_database_path(path, key)?;
        return Ok(StoreIntegrity::Missing);
    }
    let probe = copy_probe_set(path, key)?;
    let result = probe_direct(&probe, key, true);
    clear_probe_set(&probe)?;
    result
}

pub fn probe_live_message_store(path: &Path, key: Option<&DatabaseKey>) -> Result<StoreIntegrity> {
    probe_direct(path, key, false)
}

fn rollback_moves(moved: Vec<(PathBuf, PathBuf)>, preserved: &Path) -> Result<()> {
    let mut failures = Vec::new();
    for (source, target) in moved.into_iter().rev() {
        match fs::symlink_metadata(&source) {
            Ok(_) => { failures.push(format!("source reappeared: {}", source.display())); continue; }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => { failures.push(error.to_string()); continue; }
        }
        if let Err(error) = fs::rename(target, source) { failures.push(error.to_string()); }
    }
    if failures.is_empty() { Ok(()) } else {
        anyhow::bail!("saved copy in {}: {}", preserved.display(), failures.join("; "))
    }
}

/// Requires every store connection closed; copies first so probing cannot alter saved sidecars.
pub fn preserve_corrupt_store(path: &Path, key: Option<&DatabaseKey>) -> Result<PathBuf> {
    let probe = copy_probe_set(path, key)?;
    let diagnosis = probe_direct(&probe, key, true);
    clear_probe_set(&probe)?;
    if !matches!(diagnosis?, StoreIntegrity::Corrupt(_)) { return Err(StoreNotCorrupt.into()); }
    let parent = path.parent().context("message store has no parent folder")?;
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
    let preserved = parent.join(format!("messages-corrupt-{}-{nonce}", std::process::id()));
    fs::create_dir(&preserved)?;
    let mut moved = Vec::new();
    for suffix in ["-wal", "-shm", "-journal", ""] {
        let source = sidecar(path, suffix);
        let result = (|| -> Result<()> {
            let metadata = match fs::symlink_metadata(&source) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound && !suffix.is_empty() => return Ok(()),
                Err(error) => return Err(error.into()),
            };
            anyhow::ensure!(metadata.file_type().is_file(), "message store file is not regular: {}", source.display());
            let target = preserved.join(source.file_name().context("message store file has no name")?);
            fs::rename(&source, &target)?;
            moved.push((source, target));
            Ok(())
        })();
        if let Err(error) = result {
            if let Err(failure) = rollback_moves(moved, &preserved) {
                return Err(error.context(format!("rollback incomplete in {}: {failure}", preserved.display())));
            }
            let _ = fs::remove_dir(&preserved);
            return Err(error);
        }
    }
    Ok(preserved)
}

pub fn create_fresh_store(path: &Path, key: Option<&DatabaseKey>) -> Result<()> {
    for suffix in ["-wal", "-shm", "-journal"] {
        match fs::symlink_metadata(sidecar(path, suffix)) {
            Ok(_) => anyhow::bail!("message store sidecar reappeared"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let reserved = fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    drop(reserved);
    drop(MessageStore::open_with_key(path, key)?);
    Ok(())
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
