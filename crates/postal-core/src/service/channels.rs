use super::*;
use crate::store::channels::{ChannelSummary, ChannelView};
use whatsapp_rust::{NewsletterMessage, NewsletterMetadata, NewsletterRole};
use whatsapp_rust::wacore::types::message::EditAttribute;

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct ChannelPage {
    pub messages: Vec<StoredMessage>,
    pub next_before: Option<String>,
    pub has_more: bool,
}

fn jid(value: &str) -> Result<Jid> {
    let jid: Jid = value.parse()?;
    anyhow::ensure!(jid.is_newsletter(), "channel JID required");
    Ok(jid)
}

enum ChannelTarget { Jid(Jid), Invite(String) }

fn target(value: &str) -> Result<ChannelTarget> {
    if value.ends_with("@newsletter") { return jid(value).map(ChannelTarget::Jid) }
    let code = if value.starts_with("https://") {
        let url = url::Url::parse(value)?;
        anyhow::ensure!(matches!(url.host_str(), Some("whatsapp.com" | "www.whatsapp.com"))
            && url.scheme() == "https" && url.username().is_empty() && url.password().is_none()
            && url.query().is_none() && url.fragment().is_none() && url.port().is_none(), "invalid channel invite URL");
        let segments: Vec<_> = url.path_segments().ok_or_else(|| anyhow::anyhow!("invalid channel invite URL"))?.collect();
        anyhow::ensure!(segments.len() == 2 && segments[0] == "channel", "invalid channel invite URL");
        segments[1].to_owned()
    } else { value.to_owned() };
    anyhow::ensure!((6..=128).contains(&code.len()) && code.bytes().all(|byte|
        byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'), "invalid channel invite code");
    Ok(ChannelTarget::Invite(code))
}

async fn resolve_target(client: &Client, value: &str) -> Result<NewsletterMetadata> {
    match target(value)? {
        ChannelTarget::Jid(jid) => Ok(client.newsletter().get_metadata(&jid).await?),
        ChannelTarget::Invite(code) => Ok(client.newsletter().get_metadata_by_invite(&code).await?),
    }
}

fn summary(metadata: NewsletterMetadata, followed: Option<bool>, cached: Option<&ChannelSummary>) -> ChannelSummary {
    ChannelSummary {
        jid: metadata.jid.to_string(), name: metadata.name,
        description: metadata.description,
        picture_url: metadata.preview_url.or(metadata.picture_url),
        subscriber_count: metadata.subscriber_count,
        muted: metadata.muted.or_else(|| cached.map(|channel| channel.muted)).unwrap_or(false),
        followed: followed.unwrap_or(!matches!(metadata.role, None | Some(NewsletterRole::Guest))),
        favorite: cached.is_some_and(|channel| channel.favorite),
    }
}

fn message_id(server_id: u64) -> String { format!("channel-{server_id}") }

async fn store_post(store: &StoreWorker, jid: &str, server_id: u64, timestamp: i64,
    is_sender: bool, edit: &EditAttribute, message: Option<&wa::Message>, client: Option<&Client>,
    media_dir: Option<&Path>, live: bool) -> Result<(Option<StoredMessage>, Option<bool>)> {
    anyhow::ensure!(server_id > 0 && !jid.is_empty(), "invalid channel post identity");
    let id = message_id(server_id);
    let header = MessageHeader {
        chat: jid.to_owned(), id: id.clone(), sender: jid.to_owned(), timestamp, from_me: is_sender,
    };
    let revoked = matches!(edit, EditAttribute::AdminRevoke);
    let mut row = match message {
        Some(message) if !revoked => match stored_message(message, header.clone(), client, media_dir, false).await {
            Some(row) => row,
            None => return Ok((None, None)),
        },
        _ if revoked => StoredMessage { header, ..Default::default() },
        _ => return Ok((None, None)),
    };
    row.local.read = !live || is_sender;
    row.history_shareable = false;
    let change = store.insert_channel_message(row, revoked).await?;
    match store.message(jid, &id).await {
        Ok(row) => Ok((Some(row), change)),
        Err(error) if error.downcast_ref::<rusqlite::Error>() == Some(&rusqlite::Error::QueryReturnedNoRows) => Ok((None, change)),
        Err(error) => Err(error),
    }
}

async fn fetch_page(client: &Client, store: &StoreWorker, media_dir: Option<&Path>, jid: &Jid,
    count: u32, before: Option<u64>) -> Result<(ChannelPage, Vec<StoredMessage>, bool)> {
    let raw = client.newsletter().get_messages(jid.clone(), count, before).await?;
    let has_more = raw.len() == count as usize;
    let next_before = raw.iter().map(|post| post.server_id).min().map(|id| id.to_string());
    let mut messages = Vec::new();
    let mut changed = Vec::new();
    let mut changed_any = false;
    for NewsletterMessage { server_id, timestamp, is_sender, edit, message, .. } in raw {
        let timestamp = i64::try_from(timestamp).unwrap_or(i64::MAX);
        let (row, change) = store_post(store, &jid.to_string(), server_id, timestamp, is_sender,
            &edit, message.as_ref(), Some(client), media_dir, false).await?;
        changed_any |= change.is_some();
        if let Some(row) = row {
            if change.is_some() { changed.push(row.clone()); }
            messages.push(row);
        }
    }
    messages.sort_by(|a, b| b.header.timestamp.cmp(&a.header.timestamp)
        .then_with(|| b.header.id.cmp(&a.header.id)));
    Ok((ChannelPage { messages, next_before, has_more }, changed, changed_any))
}

struct RefreshSlot {
    slots: Arc<Mutex<std::collections::HashSet<String>>>,
    channel: String,
}

impl Drop for RefreshSlot {
    fn drop(&mut self) { self.slots.lock().unwrap().remove(&self.channel); }
}

pub(super) async fn apply_live_post(store: &StoreWorker, inbound: &InboundMessage,
    client: Option<Arc<Client>>, media_dir: Option<PathBuf>, events: &broadcast::Sender<ServiceEvent>,
    refreshes: &Arc<Mutex<std::collections::HashSet<String>>>) -> bool {
    let channel = inbound.info.source.chat.to_non_ad().to_string();
    let server_id = inbound.info.server_id;
    if server_id <= 0 {
        let Some(client) = client else { return false };
        let Ok(jid) = jid(&channel) else { return false };
        let slot = {
            let mut active = refreshes.lock().unwrap();
            if !active.insert(channel.clone()) { return false }
            RefreshSlot { slots: refreshes.clone(), channel: channel.clone() }
        };
        let store = store.clone();
        let events = events.clone();
        tokio::spawn(async move {
            let _slot = slot;
            match fetch_page(&client, &store, media_dir.as_deref(), &jid, 50, None).await {
                Ok((_, rows, true)) => {
                    for row in rows { let _ = events.send(ServiceEvent::hint(&row, false)); }
                    let _ = events.send(ServiceEvent::ChannelMessagesChanged { jid: channel });
                }
                Ok(_) => {}
                Err(error) => log::warn!("channel post fallback failed in {channel}: {error}"),
            }
        });
        return false;
    }
    let result = store_post(store, &channel, server_id as u64, inbound.info.timestamp.timestamp(),
        inbound.info.source.is_from_me, &inbound.info.edit, Some(&inbound.message),
        client.as_deref(), media_dir.as_deref(), !inbound.info.is_offline).await.map(|(row, change)| {
            if let (Some(row), Some(fresh)) = (&row, change) {
                let _ = events.send(ServiceEvent::hint(row, fresh && !inbound.info.is_offline));
            }
            change.is_some()
        });
    match result {
        Ok(true) => {
            let _ = events.send(ServiceEvent::ChannelMessagesChanged { jid: channel });
            true
        }
        Ok(false) => false,
        Err(error) => { log::warn!("channel post skipped in {channel}: {error}"); false },
    }
}

impl WhatsAppService {
    pub async fn channels(&self) -> Result<ChannelView> {
        self.store.channels_view().await
    }

    pub async fn refresh_channels(&self) -> Result<ChannelView> {
        let _operation = self.channel_operations.lock().await;
        anyhow::ensure!(self.is_connected(), "not connected");
        let cached: std::collections::HashMap<_, _> = self.store.channels_view().await?.channels.into_iter()
            .map(|channel| (channel.jid.clone(), channel)).collect();
        let channels = self.client.newsletter().list_subscribed().await?
            .into_iter().map(|metadata| {
                let previous = cached.get(&metadata.jid.to_string());
                summary(metadata, Some(true), previous)
            }).collect();
        self.store.replace_channels(channels, unix_now()).await?;
        let _ = self.events.send(ServiceEvent::ChannelsChanged);
        self.store.channels_view().await
    }

    pub async fn channel_metadata(&self, channel: &str) -> Result<ChannelSummary> {
        anyhow::ensure!(self.is_connected(), "not connected");
        let metadata = resolve_target(&self.client, channel).await?;
        let cached = self.store.channel(&metadata.jid.to_string()).await?;
        Ok(summary(metadata, None, cached.as_ref()))
    }

    pub async fn follow_channel(&self, channel: &str) -> Result<ChannelSummary> {
        let _operation = self.channel_operations.lock().await;
        anyhow::ensure!(self.is_connected(), "not connected");
        let jid = match target(channel)? {
            ChannelTarget::Jid(jid) => jid,
            ChannelTarget::Invite(code) => self.client.newsletter().get_metadata_by_invite(&code).await?.jid,
        };
        let canonical = jid.to_string();
        let cached = self.store.channel(&canonical).await?;
        let summary = summary(self.client.newsletter().join(&jid).await?, Some(true), cached.as_ref());
        self.store.upsert_channel(summary.clone()).await?;
        let _ = self.events.send(ServiceEvent::ChannelsChanged);
        self.store.channel(&summary.jid).await?.ok_or_else(|| anyhow::anyhow!("joined channel is missing from cache"))
    }

    pub async fn unfollow_channel(&self, channel: &str) -> Result<()> {
        let _operation = self.channel_operations.lock().await;
        anyhow::ensure!(self.is_connected(), "not connected");
        self.client.newsletter().leave(&jid(channel)?).await?;
        self.store.leave_channel(channel).await?;
        let _ = self.events.send(ServiceEvent::ChannelsChanged);
        Ok(())
    }

    pub async fn set_channel_muted(&self, channel: &str, muted: bool) -> Result<ChannelSummary> {
        let _operation = self.channel_operations.lock().await;
        anyhow::ensure!(self.is_connected(), "not connected");
        let jid = jid(channel)?;
        let key = jid.to_non_ad().to_string();
        anyhow::ensure!(self.store.channel(&key).await?.is_some_and(|channel| channel.followed), "channel is not followed");
        self.client.newsletter().set_admin_mute(&jid, muted).await?;
        self.store.set_channel_muted(&key, muted).await?;
        let _ = self.events.send(ServiceEvent::ChannelsChanged);
        self.store.channel(&key).await?.ok_or_else(|| anyhow::anyhow!("channel is not followed"))
    }

    pub async fn set_channel_favorite(&self, channel: &str, favorite: bool) -> Result<ChannelSummary> {
        let jid = jid(channel)?.to_non_ad().to_string();
        self.store.set_channel_favorite(&jid, favorite).await?;
        let _ = self.events.send(ServiceEvent::ChannelsChanged);
        self.store.channel(&jid).await?.ok_or_else(|| anyhow::anyhow!("channel is not followed"))
    }

    pub async fn channel_messages(&self, channel: &str, before: Option<&str>, limit: u32) -> Result<ChannelPage> {
        anyhow::ensure!(self.is_connected(), "not connected");
        let before = before.map(str::parse::<u64>).transpose()?;
        anyhow::ensure!(before.is_none_or(|value| value > 0), "invalid channel message cursor");
        let jid = jid(channel)?;
        let (page, _, changed) = fetch_page(&self.client, &self.store, self.media_dir.as_deref(), &jid, limit.clamp(1, 100), before).await?;
        if changed { let _ = self.events.send(ServiceEvent::ChannelMessagesChanged { jid: channel.to_owned() }); }
        Ok(page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whatsapp_rust::{NewsletterState, NewsletterVerification};

    fn metadata(muted: Option<bool>) -> NewsletterMetadata {
        NewsletterMetadata {
            jid: "1@newsletter".parse().unwrap(), name: "News".into(), description: None,
            subscriber_count: 2, verification: NewsletterVerification::Unverified,
            state: NewsletterState::Active, picture_url: None, preview_url: None,
            invite_code: None, role: Some(NewsletterRole::Subscriber), creation_time: None,
            muted, follower_activity_muted: None,
        }
    }

    #[test]
    fn unknown_server_mute_keeps_cached_choice() {
        let cached = ChannelSummary { jid: "1@newsletter".into(), name: "News".into(), description: None,
            picture_url: None, subscriber_count: 2, muted: true, followed: true, favorite: true };
        assert!(summary(metadata(None), None, Some(&cached)).muted);
        assert!(!summary(metadata(Some(false)), None, Some(&cached)).muted);
        assert!(summary(metadata(None), None, Some(&cached)).favorite);
        assert!(jid("1@newsletter").is_ok());
        assert!(jid("1@g.us").is_err());
    }

    #[test]
    fn invite_targets_accept_only_whatsapp_channel_links_or_codes() {
        for value in ["0029VaAbCdEf", "https://whatsapp.com/channel/0029VaAbCdEf",
            "https://www.whatsapp.com/channel/0029VaAbCdEf"] {
            assert!(matches!(target(value).unwrap(), ChannelTarget::Invite(_)));
        }
        assert!(matches!(target("1@newsletter").unwrap(), ChannelTarget::Jid(_)));
        for value in ["https://evil.example/channel/0029VaAbCdEf",
            "http://whatsapp.com/channel/0029VaAbCdEf", "https://whatsapp.com/channel/0029VaAbCdEf?x=1",
            "https://whatsapp.com/channel/0029VaAbCdEf#frag", "https://whatsapp.com/channel/0029VaAbCdEf/extra",
            "https://user@whatsapp.com/channel/0029VaAbCdEf", "https://whatsapp.com/channel/%2Fbad",
            "short", "not/a/code"] {
            assert!(target(value).is_err(), "{value}");
        }
        assert!(target(&"x".repeat(129)).is_err());
    }

    #[tokio::test]
    async fn live_and_history_use_same_safe_server_identity() {
        let store = StoreWorker::new(MessageStore::open(Path::new(":memory:")).unwrap());
        let message = wa::Message::text("hello");
        let (live, change) = store_post(&store, "1@newsletter", 101, 10, false,
            &EditAttribute::Empty, Some(&message), None, None, true).await.unwrap();
        assert_eq!(change, Some(true));
        assert_eq!(live.unwrap().header.id, "channel-101");
        let (history, change) = store_post(&store, "1@newsletter", 101, 10, false,
            &EditAttribute::Empty, Some(&message), None, None, false).await.unwrap();
        assert_eq!(change, Some(false));
        assert_eq!(history.unwrap().header.id, "channel-101");
    }

    #[tokio::test]
    async fn roster_operation_gate_keeps_follow_after_inflight_snapshot() {
        let store = StoreWorker::new(MessageStore::open(Path::new(":memory:")).unwrap());
        let gate = Arc::new(tokio::sync::Mutex::new(()));
        let first = summary(metadata(Some(false)), Some(true), None);
        let mut second = first.clone();
        second.jid = "2@newsletter".into();
        second.name = "Other".into();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let refresh = {
            let store = store.clone(); let gate = gate.clone(); let first = first.clone();
            tokio::spawn(async move {
                let _operation = gate.lock().await;
                started_tx.send(()).unwrap();
                release_rx.await.unwrap();
                store.replace_channels(vec![first], 10).await.unwrap();
            })
        };
        started_rx.await.unwrap();
        let (attempt_tx, attempt_rx) = tokio::sync::oneshot::channel();
        let follow = {
            let store = store.clone(); let gate = gate.clone();
            tokio::spawn(async move {
                attempt_tx.send(()).unwrap();
                let _operation = gate.lock().await;
                store.upsert_channel(second).await.unwrap();
            })
        };
        attempt_rx.await.unwrap();
        release_tx.send(()).unwrap();
        refresh.await.unwrap(); follow.await.unwrap();
        let channels = store.channels_view().await.unwrap().channels;
        assert_eq!(channels.iter().map(|channel| channel.jid.as_str()).collect::<Vec<_>>(),
            ["1@newsletter", "2@newsletter"]);
    }
}
