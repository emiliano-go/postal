use super::*;

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct ChannelSummary {
    pub jid: String,
    pub name: String,
    pub description: Option<String>,
    pub picture_url: Option<String>,
    pub subscriber_count: u64,
    pub muted: bool,
    pub followed: bool,
    pub favorite: bool,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct ChannelView {
    pub channels: Vec<ChannelSummary>,
    pub synced_at: Option<i64>,
}

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS channels (
            jid TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT, picture_url TEXT,
            subscriber_count INTEGER NOT NULL DEFAULT 0, muted INTEGER NOT NULL DEFAULT 0,
            followed INTEGER NOT NULL DEFAULT 0, favorite INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE IF NOT EXISTS channel_sync (id INTEGER PRIMARY KEY CHECK(id=1), synced_at INTEGER NOT NULL);")?;
    Ok(())
}

fn channel_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChannelSummary> {
    Ok(ChannelSummary {
        jid: row.get(0)?, name: row.get(1)?, description: row.get(2)?, picture_url: row.get(3)?,
        subscriber_count: row.get::<_, i64>(4)?.max(0) as u64,
        muted: row.get::<_, bool>(5)?, followed: row.get::<_, bool>(6)?, favorite: row.get::<_, bool>(7)?,
    })
}

fn upsert(conn: &Connection, channel: &ChannelSummary) -> Result<()> {
    anyhow::ensure!(channel.jid.ends_with("@newsletter") && !channel.name.is_empty(), "invalid channel metadata");
    conn.execute("INSERT INTO channels(jid,name,description,picture_url,subscriber_count,muted,followed,favorite)
        VALUES (?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(jid) DO UPDATE SET
        name=excluded.name,description=excluded.description,picture_url=excluded.picture_url,
        subscriber_count=excluded.subscriber_count,muted=excluded.muted,followed=excluded.followed",
        params![channel.jid, channel.name, channel.description, channel.picture_url,
            channel.subscriber_count.min(i64::MAX as u64) as i64, channel.muted, channel.followed, channel.favorite])?;
    if channel.followed {
        conn.execute("INSERT INTO chat_state(jid,muted_until) VALUES (?1,?2)
            ON CONFLICT(jid) DO UPDATE SET muted_until=excluded.muted_until",
            params![channel.jid, if channel.muted { -1 } else { 0 }])?;
    }
    Ok(())
}

impl MessageStore {
    pub fn channels_view(&self) -> Result<ChannelView> {
        let conn = self.conn.lock().unwrap();
        let mut statement = conn.prepare("SELECT jid,name,description,picture_url,subscriber_count,muted,followed,favorite
            FROM channels WHERE followed=1 ORDER BY name COLLATE NOCASE,jid")?;
        let channels = statement.query_map([], channel_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        let synced_at = conn.query_row("SELECT synced_at FROM channel_sync WHERE id=1", [], |row| row.get(0)).optional()?;
        Ok(ChannelView { channels, synced_at })
    }

    pub fn channel(&self, jid: &str) -> Result<Option<ChannelSummary>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row("SELECT jid,name,description,picture_url,subscriber_count,muted,followed,favorite
            FROM channels WHERE jid=?1", [jid], channel_row).optional()?)
    }

    pub fn replace_channels(&self, channels: &[ChannelSummary], synced_at: i64) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.savepoint()?;
        tx.execute("UPDATE channels SET followed=0", [])?;
        for channel in channels {
            anyhow::ensure!(channel.followed, "channel snapshot contains a non-follower");
            upsert(&tx, channel)?;
        }
        tx.execute("INSERT INTO channel_sync(id,synced_at) VALUES (1,?1)
            ON CONFLICT(id) DO UPDATE SET synced_at=excluded.synced_at", [synced_at])?;
        tx.commit()?;
        Ok(())
    }

    pub fn upsert_channel(&self, channel: &ChannelSummary) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.savepoint()?;
        upsert(&tx, channel)?;
        tx.commit()?;
        Ok(())
    }

    pub fn leave_channel(&self, jid: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE channels SET followed=0 WHERE jid=?1", [jid])?;
        Ok(())
    }

    pub fn set_channel_muted(&self, jid: &str, muted: bool) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.savepoint()?;
        let changed = tx.execute("UPDATE channels SET muted=?2 WHERE jid=?1 AND followed=1", params![jid, muted])?;
        anyhow::ensure!(changed == 1, "channel is not followed");
        tx.execute("INSERT INTO chat_state(jid,muted_until) VALUES (?1,?2)
            ON CONFLICT(jid) DO UPDATE SET muted_until=excluded.muted_until",
            params![jid, if muted { -1 } else { 0 }])?;
        tx.commit()?;
        Ok(())
    }

    pub fn set_channel_favorite(&self, jid: &str, favorite: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute("UPDATE channels SET favorite=?2 WHERE jid=?1 AND followed=1", params![jid, favorite])?;
        anyhow::ensure!(changed == 1, "channel is not followed");
        Ok(())
    }

    pub fn insert_channel_message(&self, message: &StoredMessage, revoked: bool) -> Result<Option<bool>> {
        anyhow::ensure!(message.header.chat.ends_with("@newsletter") && message.header.id.starts_with("channel-"),
            "invalid channel message");
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.savepoint()?;
        let previous = tx.query_row("SELECT timestamp,revoked,read FROM messages WHERE chat=?1 AND id=?2",
            params![message.header.chat, message.header.id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, bool>(1)?, row.get::<_, bool>(2)?))).optional()?;
        if previous.is_some_and(|(timestamp, was_revoked, _)| was_revoked || timestamp > message.header.timestamp) {
            return Ok(None);
        }
        if revoked {
            if previous.is_some() {
                tx.execute("UPDATE messages SET revoked=1,timestamp=MAX(timestamp,?3) WHERE chat=?1 AND id=?2",
                    params![message.header.chat, message.header.id, message.header.timestamp])?;
            } else {
                let mut stub = message.clone();
                stub.text.clear();
                stub.media = Media::default();
                stub.local.revoked = true;
                stub.local.deleted = true;
                Self::insert_row(&tx, &stub)?;
            }
        } else {
            Self::insert_row(&tx, message)?;
            if let Some((_, _, read)) = previous {
                tx.execute("UPDATE messages SET read=?3 WHERE chat=?1 AND id=?2",
                    params![message.header.chat, message.header.id, read])?;
            }
        }
        self.revive_chat(&tx, &message.header.chat)?;
        tx.commit()?;
        Ok(Some(previous.is_none()))
    }
}

impl StoreWorker {
    pub(crate) async fn channels_view(&self) -> Result<ChannelView> {
        self.run(MessageStore::channels_view).await
    }

    pub(crate) async fn channel(&self, jid: &str) -> Result<Option<ChannelSummary>> {
        let jid = jid.to_owned();
        self.run(move |store| store.channel(&jid)).await
    }

    pub(crate) async fn replace_channels(&self, channels: Vec<ChannelSummary>, synced_at: i64) -> Result<()> {
        self.run(move |store| store.replace_channels(&channels, synced_at)).await
    }

    pub(crate) async fn upsert_channel(&self, channel: ChannelSummary) -> Result<()> {
        self.run(move |store| store.upsert_channel(&channel)).await
    }

    pub(crate) async fn leave_channel(&self, jid: &str) -> Result<()> {
        let jid = jid.to_owned();
        self.run(move |store| store.leave_channel(&jid)).await
    }

    pub(crate) async fn set_channel_muted(&self, jid: &str, muted: bool) -> Result<()> {
        let jid = jid.to_owned();
        self.run(move |store| store.set_channel_muted(&jid, muted)).await
    }

    pub(crate) async fn set_channel_favorite(&self, jid: &str, favorite: bool) -> Result<()> {
        let jid = jid.to_owned();
        self.run(move |store| store.set_channel_favorite(&jid, favorite)).await
    }

    pub(crate) async fn insert_channel_message(&self, message: StoredMessage, revoked: bool) -> Result<Option<bool>> {
        self.run(move |store| store.insert_channel_message(&message, revoked)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(jid: &str) -> ChannelSummary {
        ChannelSummary { jid: jid.into(), name: "News".into(), description: None,
            picture_url: None, subscriber_count: 9, muted: false, followed: true, favorite: false }
    }

    #[test]
    fn roster_snapshot_is_atomic_and_mute_mirrors_chat_state() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        store.replace_channels(&[channel("1@newsletter")], 10).unwrap();
        assert_eq!(store.channels_view().unwrap().synced_at, Some(10));
        store.set_channel_muted("1@newsletter", true).unwrap();
        store.set_channel_favorite("1@newsletter", true).unwrap();
        assert!(store.channel("1@newsletter").unwrap().unwrap().muted);
        assert!(store.channel("1@newsletter").unwrap().unwrap().favorite);
        assert_eq!(store.muted_until("1@newsletter").unwrap(), -1);
        assert!(store.replace_channels(&[channel("2@newsletter"), channel("bad@g.us")], 20).is_err());
        assert_eq!(store.channels_view().unwrap().channels[0].jid, "1@newsletter");
        assert_eq!(store.channels_view().unwrap().synced_at, Some(10));
        store.replace_channels(&[channel("1@newsletter")], 20).unwrap();
        assert!(store.channel("1@newsletter").unwrap().unwrap().favorite);
        store.replace_channels(&[], 30).unwrap();
        assert!(store.channels_view().unwrap().channels.is_empty());
        assert_eq!(store.channels_view().unwrap().synced_at, Some(30));
    }

    #[test]
    fn channel_message_revision_and_revoke_do_not_regress() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        let mut row = StoredMessage::default();
        row.header.chat = "1@newsletter".into();
        row.header.id = "channel-101".into();
        row.header.timestamp = 10;
        row.text = "first".into();
        assert_eq!(store.insert_channel_message(&row, false).unwrap(), Some(true));
        row.header.timestamp = 9;
        row.text = "stale".into();
        assert_eq!(store.insert_channel_message(&row, false).unwrap(), None);
        row.header.timestamp = 11;
        row.text = "edited".into();
        row.local.read = true;
        assert_eq!(store.insert_channel_message(&row, false).unwrap(), Some(false));
        let edited = store.message("1@newsletter", "channel-101").unwrap();
        assert_eq!(edited.text, "edited");
        assert!(!edited.local.read);
        row.header.timestamp = 12;
        assert_eq!(store.insert_channel_message(&row, true).unwrap(), Some(false));
        row.header.timestamp = 13;
        assert_eq!(store.insert_channel_message(&row, false).unwrap(), None);
        let stored = store.message("1@newsletter", "channel-101").unwrap();
        assert!(stored.local.revoked);
        assert_eq!(stored.text, "edited");
    }
}
