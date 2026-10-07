use super::*;
use crate::message_ref::MessageRef;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Write,
    sync::{OnceLock, Weak},
};
use whatsapp_rust::wacore::types::events::{EventHandler, EventInterest, EventKind};
use whatsapp_rust::{AppStateResyncMode, AppStateResyncReport};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SyncCollection {
    CriticalBlock,
    CriticalUnblockLow,
    Regular,
    RegularHigh,
    RegularLow,
}

impl SyncCollection {
    pub const ALL: [Self; 5] = [
        Self::CriticalBlock,
        Self::CriticalUnblockLow,
        Self::Regular,
        Self::RegularHigh,
        Self::RegularLow,
    ];

    fn patch(self) -> WAPatchName {
        match self {
            Self::CriticalBlock => WAPatchName::CriticalBlock,
            Self::CriticalUnblockLow => WAPatchName::CriticalUnblockLow,
            Self::Regular => WAPatchName::Regular,
            Self::RegularHigh => WAPatchName::RegularHigh,
            Self::RegularLow => WAPatchName::RegularLow,
        }
    }

    fn from_patch(name: WAPatchName) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|collection| collection.patch() == name)
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|collection| collection.patch().as_str() == name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    Incremental,
    Full,
}

impl SyncMode {
    fn sdk(self) -> AppStateResyncMode {
        match self {
            Self::Incremental => AppStateResyncMode::Incremental,
            Self::Full => AppStateResyncMode::Snapshot,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    Unknown,
    Uninitialized,
    Synced,
    Retryable,
    Fatal,
    Skipped,
    Dirty,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct SyncCollectionHealth {
    pub collection: SyncCollection,
    pub status: SyncStatus,
    pub version: Option<u64>,
    pub last_success_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct SyncRepairReport {
    pub requested: Vec<SyncCollection>,
    pub mode: SyncMode,
    pub synced: Vec<SyncCollection>,
    pub retryable: Vec<SyncCollection>,
    pub fatal: Vec<SyncCollection>,
    pub skipped: Vec<SyncCollection>,
    pub unreported: Vec<SyncCollection>,
    pub at: i64,
    pub automatic: bool,
    pub storage_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct SyncHealthView {
    pub collections: Vec<SyncCollectionHealth>,
    pub busy: bool,
    pub automatic_running: bool,
    pub automatic_attempted: bool,
    pub last_report: Option<SyncRepairReport>,
    pub last_error: Option<String>,
    pub storage_error: Option<String>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
struct Journal {
    last_success: BTreeMap<SyncCollection, i64>,
}

#[derive(Default)]
struct Current {
    journal: Journal,
    verdicts: BTreeMap<SyncCollection, SyncStatus>,
    last_report: Option<SyncRepairReport>,
    last_error: Option<String>,
    storage_error: Option<String>,
    automatic_running: bool,
    automatic_attempted: bool,
}

pub(super) struct SyncHealthState {
    path: PathBuf,
    current: Mutex<Current>,
    busy: AtomicBool,
    checking: AtomicBool,
    client: OnceLock<Weak<Client>>,
    events: broadcast::Sender<ServiceEvent>,
}

struct FlagGuard<'a>(&'a AtomicBool);

impl Drop for FlagGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl SyncHealthState {
    pub(super) fn new(path: PathBuf, events: broadcast::Sender<ServiceEvent>) -> Arc<Self> {
        let mut current = Current::default();
        match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(journal) => current.journal = journal,
                Err(error) => {
                    current.storage_error =
                        Some(format!("Could not read saved sync times: {error}"))
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                current.storage_error = Some(format!("Could not read saved sync times: {error}"))
            }
        }
        Arc::new(Self {
            path,
            current: Mutex::new(current),
            busy: AtomicBool::new(false),
            checking: AtomicBool::new(false),
            client: OnceLock::new(),
            events,
        })
    }

    pub(super) fn handler(self: &Arc<Self>, enabled: bool) -> impl EventHandler {
        SyncHandler(self.clone(), enabled)
    }

    pub(super) fn activate(&self, client: &Arc<Client>) {
        let _ = self.client.set(Arc::downgrade(client));
    }

    fn notify(&self, automatic: bool) {
        let _ = self
            .events
            .send(ServiceEvent::SyncHealthChanged { automatic });
    }

    fn record_failure(&self, failed: &whatsapp_rust::wacore::types::events::AppStateSyncFailed) {
        let mut current = self.current.lock().unwrap();
        for (names, status) in [
            (&failed.fatal, SyncStatus::Fatal),
            (&failed.retryable, SyncStatus::Retryable),
            (&failed.skipped, SyncStatus::Skipped),
        ] {
            for name in names {
                if let Some(collection) = SyncCollection::from_name(name) {
                    current.verdicts.insert(collection, status);
                }
            }
        }
        drop(current);
        self.notify(false);
    }

    fn schedule_auto_retry(self: &Arc<Self>, collections: Vec<SyncCollection>) {
        if collections.is_empty() || self.checking.swap(true, Ordering::AcqRel) {
            return;
        }
        let Some(client) = self.client.get().and_then(Weak::upgrade) else {
            self.checking.store(false, Ordering::Release);
            return;
        };
        let state = self.clone();
        tokio::spawn(async move {
            let _checking = FlagGuard(&state.checking);
            if !client.is_connected() || state.current.lock().unwrap().automatic_attempted {
                return;
            }
            if state.busy.load(Ordering::Acquire) {
                return;
            }
            {
                let mut current = state.current.lock().unwrap();
                if current.automatic_attempted {
                    return;
                }
                current.automatic_attempted = true;
                current.automatic_running = true;
            }
            state.notify(true);
            let result = state
                .repair(&client, Some(collections), SyncMode::Incremental, true)
                .await;
            {
                let mut current = state.current.lock().unwrap();
                current.automatic_running = false;
                if let Err(error) = &result {
                    current.last_error = Some(format!("Automatic repair failed: {error:#}"));
                }
            }
            state.notify(true);
        });
    }

    pub(super) async fn view(&self, client: &Client) -> SyncHealthView {
        let (
            journal,
            verdicts,
            last_report,
            last_error,
            storage_error,
            automatic_running,
            automatic_attempted,
        ) = {
            let current = self.current.lock().unwrap();
            (
                current.journal.clone(),
                current.verdicts.clone(),
                current.last_report.clone(),
                current.last_error.clone(),
                current.storage_error.clone(),
                current.automatic_running,
                current.automatic_attempted,
            )
        };
        let mut collections = Vec::with_capacity(SyncCollection::ALL.len());
        for collection in SyncCollection::ALL {
            let (status, version, error) = match client
                .persistence_manager()
                .backend()
                .get_version(collection.patch().as_str())
                .await
            {
                Ok(Some(state)) if state.mac_mismatch_fatal => {
                    (SyncStatus::Dirty, Some(state.version), None)
                }
                Ok(Some(state)) if !state.bootstrapped => {
                    (SyncStatus::Uninitialized, Some(state.version), None)
                }
                Ok(Some(state)) => (
                    verdicts
                        .get(&collection)
                        .copied()
                        .unwrap_or(SyncStatus::Unknown),
                    Some(state.version),
                    None,
                ),
                Ok(None) => (SyncStatus::Uninitialized, None, None),
                Err(error) => (
                    SyncStatus::Unknown,
                    None,
                    Some(format!("Could not read protocol state: {error}")),
                ),
            };
            collections.push(SyncCollectionHealth {
                collection,
                status,
                version,
                last_success_at: journal.last_success.get(&collection).copied(),
                error,
            });
        }
        SyncHealthView {
            collections,
            busy: self.busy.load(Ordering::Acquire),
            automatic_running,
            automatic_attempted,
            last_report,
            last_error,
            storage_error,
        }
    }

    pub(super) async fn repair(
        &self,
        client: &Arc<Client>,
        collections: Option<Vec<SyncCollection>>,
        mode: SyncMode,
        automatic: bool,
    ) -> Result<SyncRepairReport> {
        let selected: BTreeSet<_> = collections
            .unwrap_or_else(|| SyncCollection::ALL.to_vec())
            .into_iter()
            .collect();
        anyhow::ensure!(
            !selected.is_empty(),
            MessageRef::new("error.sync_empty_selection")
        );
        anyhow::ensure!(
            client.is_connected(),
            MessageRef::new("error.not_connected")
        );
        self.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| anyhow::Error::new(MessageRef::new("error.sync_repair_busy")))?;
        let busy_guard = FlagGuard(&self.busy);
        if !automatic { self.notify(false); }
        let requested: Vec<_> = SyncCollection::ALL
            .into_iter()
            .filter(|name| selected.contains(name))
            .collect();
        let result = client
            .resync_app_state(requested.iter().map(|name| name.patch()), mode.sdk())
            .await;
        let report = match result {
            Ok(report) => self.finish_report(requested, mode, report, automatic),
            Err(error) => {
                self.current.lock().unwrap().last_error =
                    Some(format!("Protocol repair request failed: {error}"));
                drop(busy_guard);
                if !automatic { self.notify(false); }
                return Err(
                    anyhow::Error::new(MessageRef::new("error.sync_request_failed")).context(error),
                );
            }
        };
        if !report.synced.is_empty() {
            let _ = self.events.send(ServiceEvent::LabelsChanged);
            let _ = self.events.send(ServiceEvent::FavoritesChanged);
            let _ = self.events.send(ServiceEvent::NamesUpdated { count: 0 });
            let _ = self.events.send(ServiceEvent::StoreChanged);
        }
        drop(busy_guard);
        if !automatic { self.notify(false); }
        Ok(report)
    }

    fn finish_report(
        &self,
        requested: Vec<SyncCollection>,
        mode: SyncMode,
        result: AppStateResyncReport,
        automatic: bool,
    ) -> SyncRepairReport {
        let convert = |names: Vec<WAPatchName>| {
            names
                .into_iter()
                .filter_map(SyncCollection::from_patch)
                .collect::<Vec<_>>()
        };
        let synced = convert(result.synced);
        let retryable = convert(result.retryable);
        let fatal = convert(result.fatal);
        let skipped = convert(result.skipped);
        let reported: BTreeSet<_> = synced
            .iter()
            .chain(&retryable)
            .chain(&fatal)
            .chain(&skipped)
            .copied()
            .collect();
        let unreported: Vec<SyncCollection> = requested
            .iter()
            .copied()
            .filter(|name| !reported.contains(name))
            .collect();
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0);
        let mut current = self.current.lock().unwrap();
        for (names, status) in [
            (&synced, SyncStatus::Synced),
            (&retryable, SyncStatus::Retryable),
            (&fatal, SyncStatus::Fatal),
            (&skipped, SyncStatus::Skipped),
        ] {
            for name in names {
                current.verdicts.insert(*name, status);
            }
        }
        for name in &unreported {
            current.verdicts.insert(*name, SyncStatus::Unknown);
        }
        let mut storage_error = None;
        if !synced.is_empty() {
            let mut next = current.journal.clone();
            for name in &synced {
                next.last_success.insert(*name, at);
            }
            match save_journal(&self.path, &next) {
                Ok(()) => {
                    current.journal = next;
                    current.storage_error = None;
                }
                Err(error) => {
                    storage_error = Some(format!(
                        "Protocol sync succeeded, but its time could not be saved: {error}"
                    ));
                    current.storage_error = storage_error.clone();
                }
            }
        }
        let report = SyncRepairReport {
            requested,
            mode,
            synced,
            retryable,
            fatal,
            skipped,
            unreported,
            at,
            automatic,
            storage_error,
        };
        current.last_report = Some(report.clone());
        current.last_error = None;
        report
    }
}

impl WhatsAppService {
    pub async fn sync_health(&self) -> Result<SyncHealthView> {
        Ok(self.sync_health.view(&self.client).await)
    }

    pub async fn repair_sync(
        &self,
        collections: Option<Vec<SyncCollection>>,
        mode: SyncMode,
    ) -> Result<SyncRepairReport> {
        self.sync_health
            .repair(&self.client, collections, mode, false)
            .await
    }
}

struct SyncHandler(Arc<SyncHealthState>, bool);

fn retryable_collections(failed: &whatsapp_rust::wacore::types::events::AppStateSyncFailed) -> Vec<SyncCollection> {
    failed.retryable.iter()
        .filter(|name| !failed.fatal.contains(name) && !failed.skipped.contains(name))
        .filter_map(|name| SyncCollection::from_name(name))
        .collect::<BTreeSet<_>>().into_iter().collect()
}

impl EventHandler for SyncHandler {
    fn handle_event(&self, event: Arc<Event>) {
        match event.as_ref() {
            Event::AppStateSyncFailed(failed) => {
                self.0.record_failure(failed);
                self.0.schedule_auto_retry(retryable_collections(failed));
            }
            _ => {}
        }
    }

    fn interest(&self) -> EventInterest {
        if self.1 {
            EventInterest::of(&[EventKind::AppStateSyncFailed])
        } else {
            EventInterest::none()
        }
    }
}

fn save_journal(path: &Path, journal: &Journal) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let temporary = path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = File::create_new(&temporary)?;
    serde_json::to_writer(&mut file, journal).map_err(std::io::Error::other)?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_collections_are_real_unique_patch_names() {
        let names: BTreeSet<_> = SyncCollection::ALL
            .into_iter()
            .map(|name| name.patch().as_str())
            .collect();
        assert_eq!(names.len(), 5);
        assert_eq!(SyncCollection::from_name("channels"), None);
        assert_eq!(SyncCollection::from_patch(WAPatchName::Unknown), None);
    }

    #[test]
    fn partial_report_does_not_mark_unreported_collections_synced() {
        let (events, _) = broadcast::channel(4);
        let path = std::env::temp_dir().join(format!(
            "postal-sync-health-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let state = SyncHealthState::new(path.clone(), events);
        let mut sdk = AppStateResyncReport::default();
        sdk.synced.push(WAPatchName::Regular);
        let report = state.finish_report(
            vec![SyncCollection::Regular, SyncCollection::RegularHigh],
            SyncMode::Incremental,
            sdk,
            false,
        );
        assert_eq!(report.synced, vec![SyncCollection::Regular]);
        assert_eq!(report.unreported, vec![SyncCollection::RegularHigh]);
        assert!(state
            .current
            .lock()
            .unwrap()
            .journal
            .last_success
            .contains_key(&SyncCollection::Regular));
        assert!(!state
            .current
            .lock()
            .unwrap()
            .journal
            .last_success
            .contains_key(&SyncCollection::RegularHigh));
        let saved: Journal = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(saved.last_success.contains_key(&SyncCollection::Regular));
        let mut second = AppStateResyncReport::default();
        second.synced.push(WAPatchName::RegularHigh);
        state.finish_report(vec![SyncCollection::RegularHigh], SyncMode::Full, second, false);
        let (events, _) = broadcast::channel(4);
        let reopened = SyncHealthState::new(path.clone(), events);
        let current = reopened.current.lock().unwrap();
        let journal = &current.journal;
        assert!(journal.last_success.contains_key(&SyncCollection::Regular));
        assert!(journal.last_success.contains_key(&SyncCollection::RegularHigh));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn sdk_failed_event_marks_only_named_collections() {
        let (events, _) = broadcast::channel(4);
        let state = SyncHealthState::new(std::env::temp_dir().join(format!(
            "postal-sync-failure-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        )), events);
        let failed = whatsapp_rust::wacore::types::events::AppStateSyncFailed::builder()
            .fatal(vec!["regular".to_owned(), "channels".to_owned()])
            .retryable(vec!["critical_unblock_low".to_owned()])
            .skipped(vec!["regular_high".to_owned()])
            .connected(true)
            .build();
        state.record_failure(&failed);
        let current = state.current.lock().unwrap();
        assert_eq!(current.verdicts.get(&SyncCollection::Regular), Some(&SyncStatus::Fatal));
        assert_eq!(current.verdicts.get(&SyncCollection::CriticalUnblockLow), Some(&SyncStatus::Retryable));
        assert_eq!(current.verdicts.get(&SyncCollection::RegularHigh), Some(&SyncStatus::Skipped));
        assert!(!current.verdicts.contains_key(&SyncCollection::CriticalBlock));
    }

    #[test]
    fn automatic_retry_uses_only_named_retryable_failures() {
        let failed = whatsapp_rust::wacore::types::events::AppStateSyncFailed::builder()
            .fatal(vec!["regular".to_owned()])
            .retryable(vec!["regular_low".to_owned(), "regular_low".to_owned(), "regular".to_owned(), "regular_high".to_owned(), "channels".to_owned()])
            .skipped(vec!["regular_high".to_owned()])
            .connected(true).build();
        assert_eq!(retryable_collections(&failed), vec![SyncCollection::RegularLow]);
        let fatal = whatsapp_rust::wacore::types::events::AppStateSyncFailed::builder()
            .fatal(vec!["regular_low".to_owned()]).retryable(Vec::new()).skipped(Vec::new()).connected(true).build();
        assert!(retryable_collections(&fatal).is_empty());
    }
}
