use super::*;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use crate::aliases::AliasStore;

pub(crate) type StoreWorker = Worker<MessageStore>;
pub(crate) type AliasWorker = Worker<AliasStore>;

#[derive(Debug)]
struct DatabaseOperationPanic;

impl std::fmt::Display for DatabaseOperationPanic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("database operation panicked") }
}

impl std::error::Error for DatabaseOperationPanic {}

#[derive(Debug)]
struct DatabaseBatchFailed;

impl std::fmt::Display for DatabaseBatchFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("database batch aborted after operation panic") }
}

impl std::error::Error for DatabaseBatchFailed {}

pub(crate) struct Worker<T> {
    inner: Arc<T>,
    gate: Arc<AsyncMutex<()>>,
    operation: Arc<AsyncMutex<()>>,
    leased: bool,
    failed: Option<Arc<AtomicBool>>,
}

impl<T> Clone for Worker<T> {
    fn clone(&self) -> Self {
        // Background work must wait for the current batch, not inherit its lease.
        Self { inner: self.inner.clone(), gate: self.gate.clone(), operation: self.operation.clone(), leased: false, failed: None }
    }
}

impl<T: Send + Sync + 'static> Worker<T> {
    pub(crate) fn new(inner: T) -> Self {
        Self { inner: Arc::new(inner), gate: Arc::default(), operation: Arc::default(), leased: false, failed: None }
    }

    pub(crate) async fn run<R: Send + 'static>(&self, operation: impl FnOnce(&T) -> Result<R> + Send + 'static) -> Result<R> {
        let guard = if self.leased { None } else { Some(self.gate.clone().lock_owned().await) };
        let operation_guard = self.operation.clone().lock_owned().await;
        let inner = self.inner.clone();
        let failed = self.failed.clone();
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            let _operation_guard = operation_guard;
            if failed.as_ref().is_some_and(|failed| failed.load(Ordering::Acquire)) {
                return Err(anyhow::Error::new(DatabaseBatchFailed));
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(&inner)))
                .unwrap_or_else(|_| {
                    if let Some(failed) = &failed { failed.store(true, Ordering::Release); }
                    Err(anyhow::Error::new(DatabaseOperationPanic))
                });
            if let Err(error) = &result {
                if error.chain().any(|cause| matches!(cause.downcast_ref::<rusqlite::Error>(),
                    Some(rusqlite::Error::SqliteFailure(code, _)) if code.code == rusqlite::ErrorCode::DatabaseBusy)) {
                    log::warn!(target: "postal_core::storage", "database busy after timeout: {error:#}");
                }
            }
            result
        }).await.context("database worker failed")?
    }
}

impl StoreWorker {
    #[cfg(test)]
    pub(crate) async fn query_only_for_test(&self) -> Result<()> {
        self.run(|store| {
            store.conn.lock().unwrap().execute_batch("PRAGMA query_only = ON")?;
            Ok(())
        }).await
    }

    #[cfg(test)]
    pub(crate) async fn open(path: &Path) -> Result<Self> {
        Self::open_with_key(path, None).await
    }

    pub(crate) async fn open_with_key(path: &Path, key: Option<crate::database_crypto::DatabaseKey>) -> Result<Self> {
        let path = path.to_path_buf();
        Ok(Self::new(tokio::task::spawn_blocking(move || MessageStore::open_with_key(&path, key.as_ref())).await??))
    }

    pub(crate) async fn batch(&self) -> StoreBatch {
        let guard = self.gate.clone().lock_owned().await;
        let mut worker = self.clone();
        worker.leased = true;
        worker.failed = Some(Arc::new(AtomicBool::new(false)));
        let mut batch = StoreBatch { worker, guard: Some(guard), open: true };
        let result = batch.worker.run(|store| {
            store.conn.lock().unwrap().execute_batch("SAVEPOINT postal_async_batch")?;
            Ok(())
        }).await;
        if let Err(error) = &result { log::error!(target: "postal_core::storage", "could not start batch: {error}"); }
        batch.open = result.is_ok();
        batch
    }
}

impl AliasWorker {
    pub(crate) async fn open_with_key(path: &Path, key: Option<crate::database_crypto::DatabaseKey>) -> Result<Self> {
        let path = path.to_path_buf();
        Ok(Self::new(tokio::task::spawn_blocking(move || AliasStore::open_with_key(&path, key.as_ref())).await??))
    }
}

pub(crate) struct StoreBatch {
    worker: StoreWorker,
    guard: Option<OwnedMutexGuard<()>>,
    open: bool,
}

impl std::ops::Deref for StoreBatch {
    type Target = StoreWorker;
    fn deref(&self) -> &StoreWorker { &self.worker }
}

fn finish_batch(store: Arc<MessageStore>, _guard: OwnedMutexGuard<()>, operation: Arc<AsyncMutex<()>>,
    failed: Arc<AtomicBool>, open: bool) -> Result<()> {
    // Cancellation leaves an already-started blocking operation running.
    let _operation = operation.blocking_lock_owned();
    if open {
        let conn = store.conn.lock().unwrap();
        if failed.load(Ordering::Acquire) {
            if !conn.is_autocommit() { conn.execute_batch("ROLLBACK")?; }
            return Err(anyhow::Error::new(DatabaseBatchFailed));
        }
        conn.execute_batch("RELEASE postal_async_batch")?;
    }
    Ok(())
}

impl StoreBatch {
    pub(crate) async fn finish(mut self) -> Result<()> {
        let guard = self.guard.take().expect("batch owns its lease");
        let store = self.worker.inner.clone();
        let operation = self.worker.operation.clone();
        let failed = self.worker.failed.clone().expect("batch owns failure state");
        let open = self.open;
        tokio::task::spawn_blocking(move || finish_batch(store, guard, operation, failed, open)).await.context("database batch worker failed")?
    }
}

impl Drop for StoreBatch {
    fn drop(&mut self) {
        let Some(guard) = self.guard.take() else { return };
        let store = self.worker.inner.clone();
        let operation = self.worker.operation.clone();
        let failed = self.worker.failed.clone().expect("batch owns failure state");
        let open = self.open;
        let finish = move || {
            if let Err(error) = finish_batch(store, guard, operation, failed, open) {
                log::error!(target: "postal_core::storage", "could not finish batch: {error}");
            }
        };
        if let Ok(runtime) = tokio::runtime::Handle::try_current() { runtime.spawn_blocking(finish); }
        else { finish(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn poisoned_connection_rolls_back_and_subsequent_calls_work() {
        let worker = StoreWorker::open(Path::new(":memory:")).await.unwrap();
        let failed = worker.run(|store| -> Result<()> {
            let conn = store.conn.lock().unwrap();
            conn.execute_batch("SAVEPOINT poison_test; CREATE TABLE poison_probe(value INTEGER);
                INSERT INTO poison_probe(value) VALUES (1)")?;
            panic!("injected store failure");
        }).await;
        assert_eq!(failed.unwrap_err().to_string(), "database operation panicked");
        worker.run(|store| {
            let conn = store.conn.lock().unwrap();
            assert!(conn.is_autocommit());
            let count: i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE name='poison_probe'", [], |row| row.get(0))?;
            assert_eq!(count, 0);
            Ok(())
        }).await.unwrap();
        worker.run(|store| store.set_name("recovered@s", "Recovered")).await.unwrap();
        assert_eq!(worker.run(|store| store.name_for("recovered@s")).await.unwrap().as_deref(), Some("Recovered"));
    }

    #[tokio::test]
    async fn panicked_batch_rejects_later_writes_without_autocommit() {
        let worker = StoreWorker::open(Path::new(":memory:")).await.unwrap();
        let batch = worker.batch().await;
        batch.run(|store| store.set_name("before@s", "Before")).await.unwrap();
        let failed = batch.run(|store| -> Result<()> {
            let _conn = store.conn.lock().unwrap();
            panic!("injected batch failure");
        }).await;
        assert!(failed.unwrap_err().is::<DatabaseOperationPanic>());
        assert!(batch.run(|store| store.set_name("after@s", "After")).await.unwrap_err().is::<DatabaseBatchFailed>());
        assert!(batch.finish().await.unwrap_err().is::<DatabaseBatchFailed>());
        assert!(worker.run(|store| store.name_for("before@s")).await.unwrap().is_none());
        assert!(worker.run(|store| store.name_for("after@s")).await.unwrap().is_none());
        worker.run(|store| store.set_name("later@s", "Later")).await.unwrap();
        assert_eq!(worker.run(|store| store.name_for("later@s")).await.unwrap().as_deref(), Some("Later"));
    }

    #[tokio::test]
    async fn blocked_repository_does_not_block_the_async_executor() {
        let worker = StoreWorker::open(Path::new(":memory:")).await.unwrap();
        let inner = worker.inner.clone();
        let (locked, ready) = tokio::sync::oneshot::channel();
        let (release, released) = std::sync::mpsc::channel();
        let blocker = std::thread::spawn(move || {
            let _guard = inner.conn.lock().unwrap();
            locked.send(()).unwrap();
            let _ = released.recv_timeout(std::time::Duration::from_secs(1));
        });
        ready.await.unwrap();
        let started = std::time::Instant::now();
        let query = tokio::spawn(async move { worker.run(|store| store.count()).await });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(started.elapsed() < std::time::Duration::from_millis(500));
        assert!(!query.is_finished());
        release.send(()).unwrap();
        assert_eq!(query.await.unwrap().unwrap(), 0);
        blocker.join().unwrap();
    }

    #[tokio::test]
    async fn cancelling_batch_creation_does_not_leave_an_open_savepoint() {
        let worker = StoreWorker::open(Path::new(":memory:")).await.unwrap();
        let inner = worker.inner.clone();
        let (locked, ready) = tokio::sync::oneshot::channel();
        let (release, released) = std::sync::mpsc::channel();
        let blocker = std::thread::spawn(move || {
            let _guard = inner.conn.lock().unwrap();
            locked.send(()).unwrap();
            released.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        });
        ready.await.unwrap();
        let clone = worker.clone();
        let task = tokio::spawn(async move { clone.batch().await.finish().await });
        tokio::task::yield_now().await;
        assert!(worker.operation.try_lock().is_err());
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        release.send(()).unwrap();
        worker.run(|store| {
            assert!(store.conn.lock().unwrap().is_autocommit());
            Ok(())
        }).await.unwrap();
        blocker.join().unwrap();
    }

    #[tokio::test]
    async fn cancelling_a_batch_waits_for_its_started_write_before_releasing() {
        let worker = StoreWorker::open(Path::new(":memory:")).await.unwrap();
        let clone = worker.clone();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, released) = std::sync::mpsc::channel();
        let task = tokio::spawn(async move {
            let batch = clone.batch().await;
            batch.run(move |store| {
                started.send(()).unwrap();
                released.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
                store.set_name("cancelled@s", "Completed write")
            }).await.unwrap();
            batch.finish().await.unwrap();
        });
        ready.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        let clone = worker.clone();
        let queued = tokio::spawn(async move {
            clone.run(|store| {
                assert!(store.conn.lock().unwrap().is_autocommit());
                store.name_for("cancelled@s")
            }).await
        });
        tokio::task::yield_now().await;
        assert!(!queued.is_finished());
        release.send(()).unwrap();
        assert_eq!(queued.await.unwrap().unwrap().as_deref(), Some("Completed write"));
    }

    #[tokio::test]
    async fn batches_keep_cloned_requests_out_until_commit() {
        let worker = StoreWorker::open(Path::new(":memory:")).await.unwrap();
        let batch = worker.batch().await;
        batch.run(|store| store.set_name("test@s", "Synthetic")).await.unwrap();
        let clone = batch.clone();
        let queued = tokio::spawn(async move { clone.run(|store| store.name_for("test@s")).await });
        tokio::task::yield_now().await;
        assert!(!queued.is_finished());
        batch.finish().await.unwrap();
        assert_eq!(queued.await.unwrap().unwrap().as_deref(), Some("Synthetic"));
        let batch = worker.batch().await;
        batch.run(|store| store.set_name("test@s", "Dropped batch")).await.unwrap();
        drop(batch);
        assert_eq!(worker.run(|store| store.name_for("test@s")).await.unwrap().as_deref(), Some("Dropped batch"));
    }
}
