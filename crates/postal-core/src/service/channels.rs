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

fn server_id(id: &str) -> Result<u64> {
    let id = id.strip_prefix("channel-").ok_or_else(|| anyhow::anyhow!("invalid channel post id"))?;
    let id = id.parse::<u64>()?;
    anyhow::ensure!(id > 0 && id < u64::MAX, "invalid channel post id");
    Ok(id)
}

fn can_publish(role: Option<NewsletterRole>) -> bool {
    matches!(role, Some(NewsletterRole::Owner | NewsletterRole::Admin))
}

async fn store_post(store: &StoreWorker, jid: &str, server_id: u64, wire_id: Option<&str>, timestamp: i64,
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
    let change = store.insert_channel_message(row, revoked, wire_id.map(str::to_owned)).await?;
    if !revoked && change.is_some() {
        if let Some(message) = message {
            polls::remember_structures(store, jid, &id, jid, message).await;
        }
    }
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
    for NewsletterMessage { server_id, message_id: wire_id, timestamp, is_sender, edit, message, .. } in raw {
        let timestamp = i64::try_from(timestamp).unwrap_or(i64::MAX);
        let (row, change) = store_post(store, &jid.to_string(), server_id, Some(&wire_id), timestamp, is_sender,
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
    let result = store_post(store, &channel, server_id as u64, Some(&inbound.info.id), inbound.info.timestamp.timestamp(),
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
    async fn admin_channel(&self, channel: &str) -> Result<Jid> {
        anyhow::ensure!(self.is_connected(), "not connected");
        let jid = jid(channel)?;
        let metadata = self.client.newsletter().get_metadata(&jid).await?;
        anyhow::ensure!(can_publish(metadata.role), "channel administrator access required");
        Ok(jid)
    }

    async fn channel_wire_id(&self, jid: &Jid, id: &str) -> Result<String> {
        let wanted = server_id(id)?;
        if let Some(wire_id) = self.store.channel_wire_id(&jid.to_string(), id).await? { return Ok(wire_id); }
        let lookup = async {
            let mut before = None;
            for _ in 0..20 {
                let posts = self.client.newsletter().get_messages(jid.clone(), 100, before).await?;
                if let Some(post) = posts.iter().find(|post| post.server_id == wanted) {
                    anyhow::ensure!(!post.message_id.is_empty(), "server omitted channel post wire id");
                    return Ok::<_, anyhow::Error>(post.message_id.clone());
                }
                before = posts.iter().map(|post| post.server_id).min();
                if posts.len() < 100 || before.is_none_or(|before| before <= wanted) { break; }
            }
            anyhow::bail!("channel post wire id unavailable; reload older posts and retry")
        };
        let wire_id = tokio::time::timeout(Duration::from_secs(20), lookup)
            .await.map_err(|_| anyhow::anyhow!("channel post lookup timed out"))??;
        self.store.set_channel_wire_id(&jid.to_string(), id, &wire_id).await?;
        Ok(wire_id)
    }

    pub async fn channel_can_post(&self, channel: &str) -> Result<bool> {
        anyhow::ensure!(self.is_connected(), "not connected");
        Ok(can_publish(self.client.newsletter().get_metadata(&jid(channel)?).await?.role))
    }

    pub async fn channel_post_text(&self, channel: &str, text: &str) -> Result<()> {
        let jid = self.admin_channel(channel).await?;
        anyhow::ensure!(!text.trim().is_empty() && text.len() <= 65_536, "invalid channel post text");
        self.client.send_message(jid, wa::Message::text(text)).await?;
        let _ = self.events.send(ServiceEvent::ChannelMessagesChanged { jid: channel.to_owned() });
        Ok(())
    }

    pub async fn channel_edit_text(&self, channel: &str, id: &str, text: &str) -> Result<()> {
        let jid = self.admin_channel(channel).await?;
        anyhow::ensure!(!text.trim().is_empty() && text.len() <= 65_536, "invalid channel post text");
        let mut row = self.store.message(channel, id).await?;
        anyhow::ensure!(row.media.kind.is_none() && !row.local.revoked, "channel post is not editable text");
        let wire_id = self.channel_wire_id(&jid, id).await?;
        self.client.newsletter().edit_message(&jid, wire_id, wa::Message::text(text)).await?;
        row.text = text.to_owned();
        self.store.insert_channel_message(row, false, None).await?;
        let _ = self.events.send(ServiceEvent::ChannelMessagesChanged { jid: channel.to_owned() });
        Ok(())
    }

    pub async fn channel_revoke_post(&self, channel: &str, id: &str) -> Result<()> {
        let jid = self.admin_channel(channel).await?;
        let row = self.store.message(channel, id).await?;
        let wire_id = self.channel_wire_id(&jid, id).await?;
        self.client.newsletter().revoke_message(&jid, wire_id).await?;
        self.store.insert_channel_message(row, true, None).await?;
        let _ = self.events.send(ServiceEvent::ChannelMessagesChanged { jid: channel.to_owned() });
        Ok(())
    }

    pub async fn channel_post_media(&self, channel: &str, file_name: &str, bytes: Vec<u8>,
        caption: Option<String>) -> Result<Option<String>> {
        self.channel_post_media_input(channel, file_name, media::MediaInput::Bytes(bytes), caption, false).await
    }

    pub async fn channel_post_media_file(&self, channel: &str, file_name: &str, path: PathBuf,
        caption: Option<String>) -> Result<Option<String>> {
        self.channel_post_media_input(channel, file_name, media::MediaInput::File(path), caption, false).await
    }

    async fn channel_post_media_input(&self, channel: &str, file_name: &str, input: media::MediaInput,
        caption: Option<String>, forwarded: bool) -> Result<Option<String>> {
        let jid = self.admin_channel(channel).await?;
        let extension = media::file_extension(file_name);
        let (_, kind) = media::media_kind_for(&extension);
        let prepared = media_quality::prepare(input, file_name.to_owned(), kind, None, false).await?;
        let extension = media::file_extension(&prepared.file_name);
        let (media_type, kind) = media::media_kind_for(&extension);
        let upload = self.upload_media(&prepared.input, media_type, None).await?;
        let thumb = self.outgoing_thumbnail(kind, &prepared.input).await?;
        let warning = media::missing_preview_warning(kind, &thumb);
        let message = media::build_media_message(&prepared.file_name, kind, upload, &caption,
            mime_for(&extension).map(str::to_owned), &thumb,
            forwarded.then(|| forwarded_context(None)), false, None, extension == "ogg");
        self.client.send_message(jid, message).await?;
        let _ = self.events.send(ServiceEvent::ChannelMessagesChanged { jid: channel.to_owned() });
        Ok(warning)
    }

    pub async fn channel_post_poll(&self, channel: &str, question: &str, options: Vec<String>, multi: bool) -> Result<()> {
        let jid = self.admin_channel(channel).await?;
        let selectable = if multi { options.len() as u32 } else { 1 };
        self.client.polls().create(jid, question, &options, selectable).await?;
        let _ = self.events.send(ServiceEvent::ChannelMessagesChanged { jid: channel.to_owned() });
        Ok(())
    }

    pub async fn channel_forward_post(&self, from_chat: &str, id: &str, channel: &str) -> Result<()> {
        let source = self.store.message(from_chat, id).await?;
        anyhow::ensure!(!source.is_unavailable() && !source.spoiler && !source.local.deleted && !source.local.revoked,
            "source message cannot be forwarded");
        if source.media.kind.as_deref() == Some("poll") {
            let marks = self.marks_for(from_chat, &[id.to_owned()]).await?;
            let poll = marks.polls.into_iter().find(|poll| poll.id == id)
                .ok_or_else(|| anyhow::anyhow!("poll definition unavailable; reload the source message"))?;
            return self.channel_post_poll(channel, &poll.name, poll.options, poll.multi).await;
        }
        if let Some(kind) = source.media.kind.as_deref() {
            let path = source.media.path.as_deref().filter(|path| Path::new(path).is_file())
                .ok_or_else(|| anyhow::anyhow!("download source media before forwarding"))?;
            let name = Path::new(path).file_name().ok_or_else(|| anyhow::anyhow!("invalid source media path"))?
                .to_string_lossy().into_owned();
            let placeholder = format!("[{kind}]");
            let caption = (!source.text.trim().is_empty() && source.text != placeholder).then(|| source.text.clone());
            self.channel_post_media_input(channel, &name, media::MediaInput::File(PathBuf::from(path)), caption, true).await?;
            return Ok(());
        }
        anyhow::ensure!(!source.text.trim().is_empty(), "source text is empty");
        let jid = self.admin_channel(channel).await?;
        let message = wa::Message { extended_text_message: MessageField::some(wa::message::ExtendedTextMessage {
            text: Some(source.text), context_info: MessageField::some(*forwarded_context(None)), ..Default::default()
        }), ..Default::default() };
        self.client.send_message(jid, message).await?;
        let _ = self.events.send(ServiceEvent::ChannelMessagesChanged { jid: channel.to_owned() });
        Ok(())
    }

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
        assert!(!can_publish(metadata(None).role));
        assert!(can_publish(Some(NewsletterRole::Admin)));
        assert!(can_publish(Some(NewsletterRole::Owner)));
        assert_eq!(server_id("channel-101").unwrap(), 101);
        for value in ["101", "channel-0", "channel-no", "channel-18446744073709551615"] {
            assert!(server_id(value).is_err());
        }
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
        let (live, change) = store_post(&store, "1@newsletter", 101, Some("wire-101"), 10, false,
            &EditAttribute::Empty, Some(&message), None, None, true).await.unwrap();
        assert_eq!(change, Some(true));
        assert_eq!(live.unwrap().header.id, "channel-101");
        assert_eq!(store.channel_wire_id("1@newsletter", "channel-101").await.unwrap().as_deref(), Some("wire-101"));
        let (history, change) = store_post(&store, "1@newsletter", 101, Some("wire-101"), 10, false,
            &EditAttribute::Empty, Some(&message), None, None, false).await.unwrap();
        assert_eq!(change, Some(false));
        assert_eq!(history.unwrap().header.id, "channel-101");
    }

    #[tokio::test]
    async fn channel_poll_history_keeps_definition_for_follower_rendering() {
        let store = StoreWorker::new(MessageStore::open(Path::new(":memory:")).unwrap());
        let poll = wa::Message { poll_creation_message_v3: MessageField::some(wa::message::PollCreationMessage {
            name: Some("Lunch?".into()), selectable_options_count: Some(1),
            options: vec![wa::message::poll_creation_message::Option { option_name: Some("Yes".into()), ..Default::default() },
                wa::message::poll_creation_message::Option { option_name: Some("No".into()), ..Default::default() }],
            ..Default::default()
        }), ..Default::default() };
        let (row, change) = store_post(&store, "1@newsletter", 102, Some("wire-102"), 10, false,
            &EditAttribute::Empty, Some(&poll), None, None, false).await.unwrap();
        assert_eq!(change, Some(true));
        assert_eq!(row.unwrap().media.kind.as_deref(), Some("poll"));
        let ids = ["channel-102".to_owned()];
        let marks = store.marks_for("1@newsletter", Some(&ids)).await.unwrap();
        assert_eq!(marks.polls[0].name, "Lunch?");
        assert_eq!(marks.polls[0].options, ["Yes", "No"]);
        store_post(&store, "1@newsletter", 102, Some("wire-102"), 11, false,
            &EditAttribute::AdminEdit, Some(&wa::Message::text("Edited to text")), None, None, false).await.unwrap();
        assert!(store.marks_for("1@newsletter", Some(&ids)).await.unwrap().polls.is_empty());
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
