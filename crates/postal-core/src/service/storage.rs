use super::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use serde::Deserialize;

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct StorageFile {
    pub chat: String,
    pub id: String,
    pub kind: String,
    pub filename: String,
    pub timestamp: i64,
    pub quoted: bool,
    pub bytes: u64,
    pub available: bool,
}

#[derive(Default, Debug, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct ChatStorage {
    pub chat: String,
    pub name: Option<String>,
    pub bytes: u64,
    pub by_kind: BTreeMap<String, u64>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct StorageReport {
    pub database_bytes: u64,
    pub attachment_bytes: u64,
    pub cache_bytes: u64,
    pub other_bytes: u64,
    pub total_files: usize,
    pub chats: Vec<ChatStorage>,
    pub files: Vec<StorageFile>,
}

impl StorageReport {
    fn page(mut self, chat: Option<&str>, order: StorageOrder, offset: usize) -> Self {
        if let Some(chat) = chat { self.files.retain(|file| file.chat == chat); }
        self.files.sort_by(|a, b| match order {
            StorageOrder::Largest => b.bytes.cmp(&a.bytes).then(a.timestamp.cmp(&b.timestamp)),
            StorageOrder::Oldest => a.timestamp.cmp(&b.timestamp).then(b.bytes.cmp(&a.bytes)),
        }.then(a.chat.cmp(&b.chat)).then(a.id.cmp(&b.id)).then(a.quoted.cmp(&b.quoted)));
        self.total_files = self.files.len();
        self.files = self.files.into_iter().skip(offset).take(50).collect();
        self
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub enum StorageCleanup {
    Attachment { chat: String, id: String, quoted: bool },
    ChatMedia { chat: String },
    Cache,
}

#[derive(Default, Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub enum StorageOrder { #[default] Largest, Oldest }

#[derive(Default, Debug, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct CleanupResult {
    pub files: usize,
    pub bytes: u64,
}

fn cached_avatar(root: &Path, path: &Path) -> bool {
    path.parent() == Some(root.join("avatars").as_path())
        && matches!(path.extension().and_then(|s| s.to_str()), Some("jpg" | "none"))
}

/// Regenerable files the app derives: cached avatars and playback WAVs.
fn cache_file(root: &Path, path: &Path) -> bool {
    cached_avatar(root, path) || audio::is_playable(root, path)
}

fn canonical_file(path: &Path) -> Result<Option<PathBuf>> {
    match path.canonicalize() {
        Ok(path) => Ok(Some(path)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn files_under(root: &Path) -> Result<HashMap<PathBuf, u64>> {
    let mut files = HashMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_symlink() { continue; }
            let path = entry.path().canonicalize()?;
            anyhow::ensure!(path.starts_with(root), crate::message_ref::MessageRef::new("error.media_path_denied"));
            if kind.is_dir() { pending.push(path); }
            else if kind.is_file() { files.insert(path, entry.metadata()?.len()); }
        }
    }
    Ok(files)
}

#[cfg(test)]
pub(super) fn storage_report(store: &MessageStore, directory: &Path) -> Result<StorageReport> {
    storage_report_from(store.database_bytes()?, store.chat_names()?, store.media_entries()?, directory)
}

fn storage_report_from(database_bytes: u64, names: Vec<(String, Option<String>)>,
    mut entries: Vec<crate::store::storage::MediaEntry>, directory: &Path) -> Result<StorageReport> {
    let mut report = StorageReport { database_bytes, attachment_bytes: 0,
        cache_bytes: 0, other_bytes: 0, total_files: 0, chats: vec![], files: vec![] };
    let root = match directory.canonicalize() {
        Ok(root) => Some(root),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let physical = match &root { Some(root) => files_under(root)?, None => HashMap::new() };
    let mut owned = HashSet::new();
    let mut by_chat: BTreeMap<String, ChatStorage> = names.into_iter()
        .map(|(chat, name)| (chat.clone(), ChatStorage { chat, name, ..Default::default() })).collect();
    let mut counted = HashSet::new();
    entries.sort_by_key(|entry| entry.quoted);
    for entry in entries {
        let path = canonical_file(Path::new(&entry.path))?;
        let bytes = path.as_ref().and_then(|p| physical.get(p)).copied();
        let kind = match entry.kind.as_str() { "gif" | "video" => "video", "sticker" | "image" => "image",
            "audio" => "audio", "document" => "document", _ => "other" };
        let chat = by_chat.entry(entry.chat.clone()).or_insert_with(|| ChatStorage {
            chat: entry.chat.clone(), name: entry.name, ..Default::default()
        });
        if let (Some(path), Some(bytes)) = (path.as_ref(), bytes) {
            if owned.insert(path.clone()) { report.attachment_bytes += bytes; }
            if counted.insert((entry.chat.clone(), path.clone())) {
                chat.bytes += bytes;
                *chat.by_kind.entry(kind.into()).or_default() += bytes;
            }
        }
        report.files.push(StorageFile { chat: entry.chat, id: entry.id, kind: kind.into(),
            filename: Path::new(&entry.path).file_name().unwrap_or_default().to_string_lossy().into_owned(),
            timestamp: entry.timestamp, quoted: entry.quoted, bytes: bytes.unwrap_or(0), available: bytes.is_some() });
    }
    if let Some(root) = &root {
        for (path, bytes) in physical {
            if owned.contains(&path) { continue; }
            if cache_file(root, &path) { report.cache_bytes += bytes; }
            else { report.other_bytes += bytes; }
        }
    }
    report.chats = by_chat.into_values().collect();
    report.total_files = report.files.len();
    Ok(report)
}

pub(super) fn cleanup_storage(store: &MessageStore, directory: &Path, action: StorageCleanup) -> Result<CleanupResult> {
    let root = directory.canonicalize()?;
    let paths: HashSet<String> = match action {
        StorageCleanup::Attachment { chat, id, quoted } => {
            let message = store.message(&chat, &id)?;
            (if quoted { message.quote.path } else { message.media.path }).into_iter().collect()
        }
        StorageCleanup::ChatMedia { chat } => {
            let chat = store.canonical_chat(&chat)?;
            store.media_entries()?.into_iter().filter(|entry| entry.chat == chat)
                .map(|entry| entry.path).collect()
        }
        StorageCleanup::Cache => {
            let owned: HashSet<_> = store.media_entries()?.into_iter()
                .map(|entry| canonical_file(Path::new(&entry.path))).collect::<Result<Vec<_>>>()?
                .into_iter().flatten().collect();
            files_under(&root)?.into_keys().filter(|path| cache_file(&root, path) && !owned.contains(path))
                .map(|path| path.to_string_lossy().into_owned()).collect()
        }
    };
    let mut selected = Vec::new();
    for path in paths {
        let original = Path::new(&path);
        let source = match original.canonicalize() {
            Ok(source) => {
                anyhow::ensure!(source.starts_with(&root) && source.is_file(), crate::message_ref::MessageRef::new("error.media_path_denied"));
                Some(source)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        selected.push((path, source));
    }
    let mut result = CleanupResult::default();
    let mut removed = HashSet::new();
    for (path, source) in selected {
        if let Some(source) = source {
            if removed.insert(source.clone()) {
                let bytes = source.metadata()?.len();
                std::fs::remove_file(&source)?;
                result.files += 1;
                result.bytes += bytes;
                // The playback conversions are derived from this file, so they
                // go too: the WAV an audio play decodes to, the MP4 a video remux
                // writes.
                for extension in ["wav", "mp4"] {
                    let companion = audio::playable_path(&root, &source, extension);
                    if let Ok(metadata) = companion.metadata() {
                        if std::fs::remove_file(&companion).is_ok() {
                            result.files += 1;
                            result.bytes += metadata.len();
                        }
                    }
                }
            }
        }
        store.forget_media_file(&path)?;
    }
    Ok(result)
}

async fn storage_report_paged(store: &StoreWorker, directory: PathBuf, chat: Option<String>,
    order: StorageOrder, offset: usize) -> Result<StorageReport> {
    storage_report_paged_with(store, directory, chat, order, offset, |_| async {}).await
}

async fn storage_report_paged_with<F, Fut>(store: &StoreWorker, directory: PathBuf, chat: Option<String>,
    order: StorageOrder, offset: usize, mut after_page: F) -> Result<StorageReport>
where F: FnMut(i64) -> Fut, Fut: std::future::Future<Output = ()> {
    let (database_bytes, names, upper) = store.run(|store| {
        Ok((store.database_bytes()?, store.chat_names()?, store.max_message_rowid()?))
    }).await?;
    // The rowid ceiling excludes later inserts; edits to earlier rows can still appear.
    let mut entries = Vec::new();
    let mut after = 0;
    while after < upper {
        let (next, mut page) = store.run(move |store| store.media_entries_page(after, upper)).await?;
        if next == after { break; }
        after = next;
        entries.append(&mut page);
        after_page(after).await;
        tokio::task::yield_now().await;
    }
    Ok(tokio::task::spawn_blocking(move ||
        storage_report_from(database_bytes, names, entries, &directory)
            .map(|report| report.page(chat.as_deref(), order, offset))).await??)
}

impl WhatsAppService {
    pub async fn storage_report(&self, chat: Option<&str>, order: StorageOrder, offset: usize) -> Result<StorageReport> {
        let directory = self.media_dir.clone().ok_or_else(|| anyhow::anyhow!(crate::message_ref::MessageRef::new("error.media_directory_missing")))?;
        let chat = chat.map(str::to_owned);
        storage_report_paged(&self.store, directory, chat, order, offset).await
    }

    pub async fn cleanup_storage(&self, action: StorageCleanup) -> Result<CleanupResult> {
        let directory = self.media_dir.clone().ok_or_else(|| anyhow::anyhow!(crate::message_ref::MessageRef::new("error.media_directory_missing")))?;
        let result = self.store.run(move |store| cleanup_storage(store, &directory, action)).await;
        if let Some(chats) = self.store.chats().await.observed() {
            let _ = self.events.send(ServiceEvent::HistoryLoaded { chats: chats.into_iter().map(|chat| chat.chat).collect() });
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use buffa::Message as _;

    #[tokio::test]
    async fn paged_report_keeps_normal_media_before_earlier_quote_for_shared_path() {
        let root = std::env::temp_dir().join(format!("postal-storage-order-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let media = root.join("media");
        std::fs::create_dir_all(&media).unwrap();
        let file = media.join("shared.bin");
        std::fs::write(&file, [1u8; 7]).unwrap();
        let path = file.to_string_lossy().into_owned();
        let store = StoreWorker::open(&root.join("messages.db")).await.unwrap();
        store.run(move |store| {
            let mut quoted = StoredMessage::default();
            quoted.header.chat = "a@s".into();
            quoted.header.id = "earlier".into();
            quoted.header.timestamp = 1;
            quoted.quote.kind = Some("document".into());
            quoted.quote.path = Some(path.clone());
            store.insert_message(&quoted)?;
            let mut normal = StoredMessage::default();
            normal.header.chat = "a@s".into();
            normal.header.id = "later".into();
            normal.header.timestamp = 2;
            normal.media.kind = Some("image".into());
            normal.media.path = Some(path);
            store.insert_message(&normal)?;
            Ok(())
        }).await.unwrap();
        let old_media = media.clone();
        let old = store.run(move |store| Ok(storage_report(store, &old_media)?
            .page(None, StorageOrder::Largest, 0))).await.unwrap();
        let paged = storage_report_paged(&store, media, None, StorageOrder::Largest, 0).await.unwrap();
        assert_eq!(serde_json::to_value(&paged).unwrap(), serde_json::to_value(old).unwrap());
        assert_eq!(paged.chats[0].by_kind["image"], 7);
        assert!(!paged.chats[0].by_kind.contains_key("document"));
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn hundred_thousand_row_report_releases_worker_for_live_insert() {
        let root = std::env::temp_dir().join(format!("postal-storage-large-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let media = root.join("media");
        std::fs::create_dir_all(&media).unwrap();
        let file = media.join("shared.bin");
        std::fs::write(&file, [1u8; 3]).unwrap();
        let path = file.to_string_lossy().into_owned();
        let store = StoreWorker::open(&root.join("messages.db")).await.unwrap();
        store.run(move |store| store.with_test_connection(|conn| {
            let tx = conn.transaction()?;
            let mut insert = tx.prepare("INSERT INTO messages
                (chat, id, sender, timestamp, from_me, text, media_kind, media_path)
                VALUES ('a@s', ?1, 'peer@s', ?2, 0, '', ?3, ?4)")?;
            for index in 0..100_000 {
                let media = index % 100 == 0;
                insert.execute(rusqlite::params![index.to_string(), index,
                    media.then_some("image"), media.then_some(path.as_str())])?;
            }
            drop(insert);
            tx.commit()?;
            Ok(())
        })).await.unwrap();
        let (first_tx, first_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let reporting = store.clone();
        let report = tokio::spawn(async move {
            let mut pause = Some((first_tx, release_rx));
            storage_report_paged_with(&reporting, media, None, StorageOrder::Largest, 0, move |_| {
                let pause = pause.take();
                async move { if let Some((first, release)) = pause { let _ = first.send(()); let _ = release.await; } }
            }).await
        });
        tokio::time::timeout(std::time::Duration::from_secs(10), first_rx).await.unwrap().unwrap();
        let writer = store.clone();
        let started = std::time::Instant::now();
        tokio::time::timeout(std::time::Duration::from_secs(5), writer.run(|store| {
            let mut message = StoredMessage::default();
            message.header.chat = "a@s".into();
            message.header.id = "live".into();
            store.insert_message(&message)?;
            Ok(())
        })).await.unwrap().unwrap();
        eprintln!("storage live insert during paused report: {:?}", started.elapsed());
        release_tx.send(()).unwrap();
        let report = tokio::time::timeout(std::time::Duration::from_secs(30), report).await.unwrap().unwrap().unwrap();
        assert_eq!(report.total_files, 1_000);
        drop(writer);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn storage_counts_shared_files_and_cleanup_preserves_messages_and_download_info() {
        let root = std::env::temp_dir().join(format!("postal-storage-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let media = root.join("media");
        std::fs::create_dir_all(media.join("avatars")).unwrap();
        std::fs::create_dir_all(media.join("playable")).unwrap();
        let store = MessageStore::open(&root.join("synthetic.db")).unwrap();
        let photo = media.join("photo.jpg");
        let video = media.join("video.mp4");
        std::fs::write(&photo, [1; 20]).unwrap();
        std::fs::write(&video, [2; 80]).unwrap();
        std::fs::write(media.join("avatars/contact.jpg"), [3; 10]).unwrap();
        std::fs::write(media.join("playable/photo.wav"), [5; 30]).unwrap();
        std::fs::write(media.join("playable/video.mp4"), [6; 20]).unwrap();
        std::fs::write(media.join("unrelated.txt"), [4; 7]).unwrap();
        let locator = wa::Message { image_message: MessageField::some(wa::message::ImageMessage {
            direct_path: Some("/synthetic/photo".into()), media_key: Some(vec![5; 32]),
            ..Default::default()
        }), ..Default::default() }.encode_to_vec();
        for (chat, id, kind, path, timestamp) in [
            ("a@s", "photo", "image", &photo, 100),
            ("a@s", "video", "video", &video, 200),
            ("b@s", "shared", "image", &photo, 300),
        ] {
            let mut message = StoredMessage::default();
            message.header = MessageHeader { chat: chat.into(), id: id.into(), timestamp, sender: "peer@s".into(), from_me: false };
            message.text = "caption stays".into();
            message.media.kind = Some(kind.into());
            message.media.path = Some(path.to_string_lossy().into_owned());
            message.media.locator = Some(locator.clone());
            store.insert_message(&message).unwrap();
        }
        store.set_saved_name("a@s", "Synthetic A").unwrap();
        let report = storage_report(&store, &media).unwrap();
        assert!(report.database_bytes > 0);
        assert_eq!((report.attachment_bytes, report.cache_bytes, report.other_bytes), (100, 60, 7));
        assert_eq!(report.chats.iter().find(|c| c.chat == "a@s").unwrap().by_kind["video"], 80);
        assert_eq!(report.page(Some("a@s"), StorageOrder::Largest, 0).files[0].id, "video");
        assert_eq!(storage_report(&store, &media).unwrap().page(Some("a@s"), StorageOrder::Oldest, 0).files[0].id, "photo");
        let cleaned = cleanup_storage(&store, &media, StorageCleanup::Attachment { chat: "a@s".into(), id: "photo".into(), quoted: false }).unwrap();
        assert_eq!((cleaned.files, cleaned.bytes), (2, 50), "the playback conversion goes with its source");
        assert!(!media.join("playable/photo.wav").exists());
        for (chat, id) in [("a@s", "photo"), ("b@s", "shared")] {
            let row = store.message(chat, id).unwrap();
            assert_eq!(row.text, "caption stays");
            assert!(row.media.path.is_none());
            assert_eq!(row.media.locator.as_ref(), Some(&locator));
        }
        assert_eq!(store.count().unwrap(), 3);
        let cleaned = cleanup_storage(&store, &media, StorageCleanup::ChatMedia { chat: "a@s".into() }).unwrap();
        assert_eq!((cleaned.files, cleaned.bytes), (2, 100), "the video remux goes with its source");
        assert!(!video.exists());
        assert!(!media.join("playable/video.mp4").exists());
        // Only cached files remain for the cache action: the avatar.
        assert_eq!(cleanup_storage(&store, &media, StorageCleanup::Cache).unwrap().bytes, 10);
        assert!(media.join("unrelated.txt").exists());
        assert_eq!(storage_report(&store, &media).unwrap().attachment_bytes, 0);
        assert_eq!(store.chats().unwrap().len(), 2);
        let outside = root.join("outside.jpg");
        std::fs::write(&outside, [9; 8]).unwrap();
        store.set_media_path("a@s", "photo", &outside.to_string_lossy()).unwrap();
        assert!(cleanup_storage(&store, &media, StorageCleanup::ChatMedia { chat: "a@s".into() }).is_err());
        assert!(outside.exists());
        assert!(store.message("a@s", "photo").unwrap().media.path.is_some());
        #[cfg(unix)] {
            let link = media.join("escape.jpg");
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            store.set_media_path("a@s", "photo", &link.to_string_lossy()).unwrap();
            assert!(cleanup_storage(&store, &media, StorageCleanup::Attachment { chat: "a@s".into(), id: "photo".into(), quoted: false }).is_err());
            assert!(outside.exists());
        }
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
