use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PinClock { Unknown = 0, Server = 1, Sender = 2, Local = 3 }

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MessagePinUpdate {
    pub target: String,
    pub remote: Option<String>,
    pub pinned: bool,
    pub timestamp: i64,
    pub expires_at: Option<i64>,
    pub clock: PinClock,
}

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS message_pin_sync (
        chat TEXT PRIMARY KEY, target TEXT NOT NULL, pinned INTEGER NOT NULL,
        timestamp INTEGER NOT NULL, expires_at INTEGER);
        INSERT OR IGNORE INTO message_pin_sync(chat,target,pinned,timestamp,expires_at)
            SELECT chat, id, 1, 0, NULL FROM message_pins;")?;
    Ok(())
}

pub(super) fn migrate_multiple(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE message_pins_multi (
            chat TEXT NOT NULL, id TEXT NOT NULL, PRIMARY KEY(chat,id));
        INSERT INTO message_pins_multi SELECT chat,id FROM message_pins;
        CREATE TABLE message_pin_sync_multi (
            chat TEXT NOT NULL, target TEXT NOT NULL, pinned INTEGER NOT NULL,
            timestamp INTEGER NOT NULL, expires_at INTEGER, clock INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY(chat,target));
        INSERT INTO message_pin_sync_multi(chat,target,pinned,timestamp,expires_at)
            SELECT chat,target,pinned,timestamp,expires_at FROM message_pin_sync;
        INSERT OR IGNORE INTO message_pin_sync_multi(chat,target,pinned,timestamp,expires_at)
            SELECT chat,id,1,0,NULL FROM message_pins;
        DROP TABLE message_pins;
        ALTER TABLE message_pins_multi RENAME TO message_pins;
        DROP TABLE message_pin_sync;
        ALTER TABLE message_pin_sync_multi RENAME TO message_pin_sync;")?;
    Ok(())
}

pub(super) fn migrate_rank(conn: &Connection) -> Result<()> {
    conn.execute_batch("ALTER TABLE message_pin_sync ADD COLUMN observed_rank INTEGER NOT NULL DEFAULT 0;
        CREATE INDEX idx_message_pin_sync_rank ON message_pin_sync(observed_rank);")?;
    Ok(())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()
        .min(i64::MAX as u128) as i64
}

pub(super) fn pinned_messages(conn: &Connection, chat: &str) -> Result<Vec<String>> {
    Ok(conn.prepare("SELECT p.id FROM message_pins p JOIN message_pin_sync s
        ON s.chat=p.chat AND s.target=p.id WHERE p.chat=?1 AND s.pinned=1
        AND (s.expires_at IS NULL OR s.expires_at>?2)
        ORDER BY s.observed_rank DESC,
            CASE WHEN s.observed_rank=0 AND s.clock=?3 THEN s.timestamp ELSE 0 END DESC,p.id")?
        .query_map(params![chat, now_ms(), PinClock::Server as i64], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?)
}

pub(super) fn merge(conn: &Connection, from: &str, to: &str) -> Result<()> {
    let now = now_ms();
    conn.execute("UPDATE message_pin_sync AS dest SET observed_rank=MAX(dest.observed_rank,
        (SELECT src.observed_rank FROM message_pin_sync src WHERE src.chat=?2 AND src.target=dest.target))
        WHERE dest.chat=?1 AND dest.pinned=1 AND (dest.expires_at IS NULL OR dest.expires_at>?3)
        AND EXISTS (SELECT 1 FROM message_pins p WHERE p.chat=?1 AND p.id=dest.target)
        AND EXISTS (SELECT 1 FROM message_pin_sync src JOIN message_pins p
            ON p.chat=src.chat AND p.id=src.target WHERE src.chat=?2 AND src.target=dest.target
            AND src.pinned=1 AND (src.expires_at IS NULL OR src.expires_at>?3))", params![to, from, now])?;
    conn.execute("INSERT INTO message_pin_sync (chat, target, pinned, timestamp, expires_at, clock, observed_rank)
        SELECT ?1, target, pinned, timestamp, expires_at, clock, observed_rank FROM message_pin_sync WHERE chat = ?2
        ON CONFLICT(chat,target) DO UPDATE SET pinned=excluded.pinned,
        timestamp=excluded.timestamp, expires_at=excluded.expires_at, clock=excluded.clock,
        observed_rank=CASE WHEN message_pin_sync.pinned=1 AND excluded.pinned=1
            AND (message_pin_sync.expires_at IS NULL OR message_pin_sync.expires_at>?4)
            AND (excluded.expires_at IS NULL OR excluded.expires_at>?4)
            AND EXISTS (SELECT 1 FROM message_pins p WHERE p.chat=?1 AND p.id=message_pin_sync.target)
            AND EXISTS (SELECT 1 FROM message_pins p WHERE p.chat=?2 AND p.id=message_pin_sync.target)
            THEN MAX(message_pin_sync.observed_rank,excluded.observed_rank) ELSE excluded.observed_rank END
        WHERE (excluded.clock = ?3 AND message_pin_sync.clock = ?3 AND
            (excluded.timestamp > message_pin_sync.timestamp OR
             (excluded.timestamp = message_pin_sync.timestamp AND excluded.pinned < message_pin_sync.pinned)))
           OR (excluded.clock = ?3 AND message_pin_sync.clock <> ?3)", params![to, from, PinClock::Server as i64, now])?;
    conn.execute("DELETE FROM message_pin_sync WHERE chat = ?1", params![from])?;
    conn.execute("UPDATE OR IGNORE message_pins SET chat=?1 WHERE chat=?2", params![to, from])?;
    conn.execute("DELETE FROM message_pins WHERE chat=?1", params![from])?;
    conn.execute("DELETE FROM message_pins WHERE chat=?1 AND NOT EXISTS (
        SELECT 1 FROM message_pin_sync s WHERE s.chat=message_pins.chat
            AND s.target=message_pins.id AND s.pinned=1)", params![to])?;
    Ok(())
}

impl MessageStore {
    pub(crate) fn mirror_message_pin(&self, chat: &str, id: Option<&str>) -> Result<()> {
        let target = match id {
            Some(id) => Some(id.to_owned()),
            None => {
                let conn = self.conn.lock().unwrap();
                let chat = names::canonical_chat(&conn, chat)?;
                pinned_messages(&conn, &chat)?.into_iter().next()
            }
        };
        let Some(target) = target else { return Ok(()) };
        self.apply_message_pin_update(chat, &MessagePinUpdate {
            target, remote: None, pinned: id.is_some(), timestamp: now_ms(), expires_at: None, clock: PinClock::Local,
        }, false).map(|_| ())
    }

    pub(crate) fn apply_message_pin_update(&self, chat: &str, pin: &MessagePinUpdate, history: bool) -> Result<bool> {
        anyhow::ensure!(!pin.target.is_empty() && pin.timestamp >= 0, "invalid message pin metadata");
        let mut conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, chat)?.into_owned();
        if let Some(remote) = &pin.remote {
            anyhow::ensure!(names::canonical_chat(&conn, remote)? == chat, "message pin names another chat");
        }
        let tx = conn.savepoint()?;
        let previous = tx.query_row("SELECT s.pinned,s.timestamp,s.clock,s.observed_rank,s.expires_at,
                EXISTS(SELECT 1 FROM message_pins p WHERE p.chat=s.chat AND p.id=s.target)
            FROM message_pin_sync s WHERE s.chat=?1 AND s.target=?2",
            params![chat, pin.target], |row| Ok((row.get::<_, bool>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?, row.get::<_, Option<i64>>(4)?, row.get::<_, bool>(5)?))).optional()?;
        if let Some((pinned, timestamp, clock, _, _, _)) = previous {
            if pin.clock == PinClock::Server && clock == PinClock::Server as i64 &&
                (pin.timestamp < timestamp || (pin.timestamp == timestamp && !pinned)) { return Ok(false) }
            if history && clock == PinClock::Unknown as i64 &&
                (pin.timestamp < timestamp || (pin.timestamp == timestamp && !pinned)) { return Ok(false) }
            if history && pin.clock != PinClock::Server && clock == PinClock::Server as i64 { return Ok(false) }
        }
        let observed_rank = if !pin.pinned { previous.map_or(0, |(_, _, _, rank, _, _)| rank) } else if !history {
            tx.query_row("SELECT COALESCE(MAX(observed_rank),0)+1 FROM message_pin_sync", [], |row| row.get::<_, i64>(0))?
        } else { previous.filter(|(pinned, _, _, _, expiry, active)| *pinned && *active &&
            expiry.is_none_or(|expiry| expiry > now_ms())).map_or(0, |(_, _, _, rank, _, _)| rank) };
        tx.execute("INSERT INTO message_pin_sync (chat, target, pinned, timestamp, expires_at, clock, observed_rank) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(chat,target) DO UPDATE SET pinned=excluded.pinned,
            timestamp=excluded.timestamp, expires_at=excluded.expires_at, clock=excluded.clock,
            observed_rank=excluded.observed_rank",
            params![chat, pin.target, pin.pinned, pin.timestamp, pin.expires_at, pin.clock as i64, observed_rank])?;
        if pin.pinned {
            tx.execute("INSERT OR IGNORE INTO message_pins (chat, id) VALUES (?1, ?2)",
                params![chat, pin.target])?;
        } else {
            tx.execute("DELETE FROM message_pins WHERE chat=?1 AND id=?2", params![chat, pin.target])?;
        }
        tx.commit()?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(target: &str, pinned: bool, timestamp: i64) -> MessagePinUpdate {
        MessagePinUpdate { target: target.into(), remote: None, pinned, timestamp, expires_at: None, clock: PinClock::Server }
    }

    #[test]
    fn history_message_pins_keep_distinct_targets_and_unpin_tombstones() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        assert!(store.apply_message_pin_update("1@g.us", &update("new", true, 20), true).unwrap());
        assert!(store.apply_message_pin_update("1@g.us", &update("old", true, 10), true).unwrap());
        assert_eq!(store.marks("1@g.us").unwrap().pinned_messages, ["new", "old"]);
        assert_eq!(store.marks("1@g.us").unwrap().pinned.as_deref(), Some("new"));
        assert!(store.apply_message_pin_update("1@g.us", &update("old", false, 30), false).unwrap());
        assert!(!store.apply_message_pin_update("1@g.us", &update("old", true, 10), true).unwrap());
        assert_eq!(store.marks("1@g.us").unwrap().pinned_messages, ["new"]);
        assert!(store.apply_message_pin_update("1@g.us", &update("new", false, 30), false).unwrap());
        assert!(!store.apply_message_pin_update("1@g.us", &update("new", true, 20), true).unwrap());
        assert!(store.marks("1@g.us").unwrap().pinned.is_none());
    }

    #[test]
    fn history_message_pins_expiry_and_cross_chat_validation() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        store.apply_message_pin_update("1@g.us", &update("active", true, 0), true).unwrap();
        let mut pin = update("message", true, 1);
        pin.expires_at = Some(2);
        store.apply_message_pin_update("1@g.us", &pin, true).unwrap();
        assert_eq!(store.marks("1@g.us").unwrap().pinned_messages, ["active"]);
        pin.remote = Some("2@g.us".into());
        assert!(store.apply_message_pin_update("1@g.us", &pin, true).is_err());
    }

    #[test]
    fn history_message_pin_positive_replay_restores_retained_state_after_pruning() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        let pin = update("message", true, 20);
        store.apply_message_pin_update("1@g.us", &pin, true).unwrap();
        store.conn.lock().unwrap().execute("DELETE FROM message_pins", []).unwrap();
        assert!(store.apply_message_pin_update("1@g.us", &pin, true).unwrap());
        assert_eq!(store.marks("1@g.us").unwrap().pinned.as_deref(), Some("message"));
        store.apply_message_pin_update("1@g.us", &update("message", false, 20), false).unwrap();
        assert!(!store.apply_message_pin_update("1@g.us", &pin, true).unwrap());
        assert!(store.marks("1@g.us").unwrap().pinned.is_none());
    }

    #[test]
    fn history_only_pins_order_by_server_time_then_id_and_unpin_only_target() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        for pin in [update("b", true, 10), update("a", true, 10), update("latest", true, 20)] {
            store.apply_message_pin_update("1@g.us", &pin, true).unwrap();
        }
        let marks = store.marks("1@g.us").unwrap();
        assert_eq!(marks.pinned_messages, ["latest", "a", "b"]);
        assert_eq!(marks.pinned.as_deref(), Some("latest"));
        store.apply_message_pin_update("1@g.us", &update("a", false, 30), false).unwrap();
        assert_eq!(store.marks("1@g.us").unwrap().pinned_messages, ["latest", "b"]);
        assert!(!store.apply_message_pin_update("1@g.us", &update("a", true, 10), true).unwrap());
        assert_eq!(store.marks("1@g.us").unwrap().pinned_messages, ["latest", "b"]);
    }

    #[test]
    fn observed_live_rank_orders_incomparable_clocks_in_both_directions() {
        for reverse in [false, true] {
            let store = MessageStore::open(Path::new(":memory:")).unwrap();
            let mut local = update("local", true, i64::MAX / 2);
            local.clock = PinClock::Local;
            let server = update("server", true, 10);
            if reverse {
                store.apply_message_pin_update("1@g.us", &server, false).unwrap();
                store.apply_message_pin_update("1@g.us", &local, false).unwrap();
            } else {
                store.apply_message_pin_update("1@g.us", &local, false).unwrap();
                store.apply_message_pin_update("1@g.us", &server, false).unwrap();
            }
            assert_eq!(store.marks("1@g.us").unwrap().pinned_messages,
                if reverse { ["local", "server"] } else { ["server", "local"] });
        }
    }

    #[test]
    fn history_replay_keeps_live_rank_and_history_only_server_order() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        for pin in [update("old", true, 10), update("new", true, 20)] {
            store.apply_message_pin_update("1@g.us", &pin, true).unwrap();
        }
        assert_eq!(store.marks("1@g.us").unwrap().pinned_messages, ["new", "old"]);
        let mut local = update("local", true, i64::MAX / 2);
        local.clock = PinClock::Local;
        store.apply_message_pin_update("1@g.us", &local, false).unwrap();
        store.apply_message_pin_update("1@g.us", &update("old", true, 30), true).unwrap();
        assert_eq!(store.marks("1@g.us").unwrap().pinned_messages, ["local", "old", "new"]);
        store.apply_message_pin_update("1@g.us", &update("old", true, 40), false).unwrap();
        assert_eq!(store.marks("1@g.us").unwrap().pinned_messages, ["old", "local", "new"]);
        store.apply_message_pin_update("1@g.us", &update("new", true, 50), true).unwrap();
        assert_eq!(store.marks("1@g.us").unwrap().pinned_messages, ["old", "local", "new"]);
    }

    #[test]
    fn delayed_server_pin_does_not_undo_live_unpin() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        assert!(store.apply_message_pin_update("1@g.us", &update("a", true, 10), false).unwrap());
        assert!(store.apply_message_pin_update("1@g.us", &update("b", true, 15), false).unwrap());
        assert!(store.apply_message_pin_update("1@g.us", &update("a", false, 20), false).unwrap());
        assert!(!store.apply_message_pin_update("1@g.us", &update("a", true, 10), false).unwrap());
        assert!(!store.apply_message_pin_update("1@g.us", &update("a", true, 20), false).unwrap());
        assert_eq!(store.marks("1@g.us").unwrap().pinned_messages, ["b"]);
    }

    #[test]
    fn incomparable_sender_and_local_clocks_do_not_freeze_remote_updates() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        let mut local = update("a", true, i64::MAX / 2);
        local.clock = PinClock::Local;
        store.apply_message_pin_update("1@g.us", &local, false).unwrap();
        store.apply_message_pin_update("1@g.us", &update("a", false, 20), false).unwrap();
        assert!(store.marks("1@g.us").unwrap().pinned_messages.is_empty());
        let mut sender = update("a", true, 1);
        sender.clock = PinClock::Sender;
        store.apply_message_pin_update("1@g.us", &sender, false).unwrap();
        assert_eq!(store.marks("1@g.us").unwrap().pinned_messages, ["a"]);
    }

    #[test]
    fn old_single_pin_schema_migrates_pin_and_unpin_tombstone() {
        let root = std::env::temp_dir().join(format!("postal-pin-migrate-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("messages.db");
        let conn = Connection::open(&path).unwrap();
        super::super::schema::migrate_to(&conn, 43).unwrap();
        conn.execute_batch("INSERT INTO message_pins(chat,id) VALUES ('1@g.us','old');
            INSERT INTO message_pin_sync(chat,target,pinned,timestamp,expires_at) VALUES
                ('1@g.us','old',1,10,NULL),('2@g.us','gone',0,20,NULL);").unwrap();
        drop(conn);
        let store = MessageStore::open(&path).unwrap();
        assert_eq!(store.marks("1@g.us").unwrap().pinned_messages, ["old"]);
        assert!(!store.apply_message_pin_update("2@g.us", &update("gone", true, 10), true).unwrap());
        store.apply_message_pin_update("1@g.us", &update("new", true, 30), true).unwrap();
        assert_eq!(store.marks("1@g.us").unwrap().pinned_messages, ["new", "old"]);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn existing_multi_pin_schema_gains_observed_rank() {
        let conn = Connection::open_in_memory().unwrap();
        super::super::schema::migrate_to(&conn, 44).unwrap();
        conn.execute("INSERT INTO message_pin_sync(chat,target,pinned,timestamp,expires_at,clock)
            VALUES ('1@g.us','old',1,20,NULL,1)", []).unwrap();
        super::super::schema::migrate_to(&conn, super::super::schema::MIGRATIONS.len()).unwrap();
        let rank: i64 = conn.query_row("SELECT observed_rank FROM message_pin_sync WHERE chat='1@g.us'", [], |row| row.get(0)).unwrap();
        assert_eq!(rank, 0);
    }

    #[test]
    fn rekey_and_retention_keep_independent_pin_targets() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        for (id, at) in [("old", 1), ("new", 2)] {
            let mut row = StoredMessage::default();
            row.header.chat = "123@lid".into(); row.header.id = id.into(); row.header.timestamp = at;
            store.insert_message(&row).unwrap();
            store.apply_message_pin_update("123@lid", &update(id, true, at * 100), false).unwrap();
        }
        store.set_lid_pn("123", "5989").unwrap();
        let chat = "5989@s.whatsapp.net";
        assert_eq!(store.marks(chat).unwrap().pinned_messages, ["new", "old"]);
        let manager = DiskRetentionManager::new(DiskRetention {
            max_age_hours: RetentionLimit::Unlimited,
            max_messages_per_chat: RetentionLimit::Limited(1),
        });
        assert_eq!(manager.enforce(&store).unwrap(), 1);
        assert_eq!(store.marks(chat).unwrap().pinned_messages, ["new"]);
        assert_eq!(store.conn.lock().unwrap().query_row("SELECT COUNT(*) FROM message_pin_sync WHERE chat=?1",
            [chat], |row| row.get::<_, i64>(0)).unwrap(), 2);
    }

    #[test]
    fn rekey_keeps_newer_per_target_unpin_without_erasing_other_pins() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        let phone = "5989@s.whatsapp.net";
        store.apply_message_pin_update(phone, &update("same", true, 20), true).unwrap();
        store.apply_message_pin_update(phone, &update("other", true, 40), true).unwrap();
        store.apply_message_pin_update("123@lid", &update("same", false, 30), true).unwrap();
        store.set_lid_pn("123", "5989").unwrap();
        assert_eq!(store.marks(phone).unwrap().pinned_messages, ["other"]);
        assert!(!store.apply_message_pin_update(phone, &update("same", true, 20), true).unwrap());
    }

    #[test]
    fn rekey_preserves_live_order_when_history_wins_same_active_target() {
        let phone = "5989@s.whatsapp.net";
        for source_wins in [false, true] {
            let store = MessageStore::open(Path::new(":memory:")).unwrap();
            store.apply_message_pin_update(phone, &update("other", true, 10), false).unwrap();
            if source_wins {
                store.apply_message_pin_update(phone, &update("same", true, 20), false).unwrap();
                store.apply_message_pin_update("123@lid", &update("same", true, 30), true).unwrap();
            } else {
                store.apply_message_pin_update("123@lid", &update("same", true, 20), false).unwrap();
                store.apply_message_pin_update(phone, &update("same", true, 30), true).unwrap();
            }
            store.set_lid_pn("123", "5989").unwrap();
            assert_eq!(store.marks(phone).unwrap().pinned_messages, ["same", "other"]);
        }
    }

    #[test]
    fn rekey_does_not_carry_unpinned_rank_into_history_repin() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        let phone = "5989@s.whatsapp.net";
        store.apply_message_pin_update(phone, &update("same", true, 10), false).unwrap();
        store.apply_message_pin_update(phone, &update("same", false, 20), false).unwrap();
        store.apply_message_pin_update(phone, &update("other", true, 25), false).unwrap();
        store.apply_message_pin_update("123@lid", &update("same", true, 30), true).unwrap();
        store.set_lid_pn("123", "5989").unwrap();
        assert_eq!(store.marks(phone).unwrap().pinned_messages, ["other", "same"]);
    }

    #[test]
    fn expired_live_pin_does_not_lend_rank_to_history_repin_or_alias() {
        let phone = "5989@s.whatsapp.net";
        let expired = MessagePinUpdate { expires_at: Some(1), ..update("same", true, 10) };
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        store.apply_message_pin_update(phone, &update("other", true, 5), false).unwrap();
        store.apply_message_pin_update(phone, &expired, false).unwrap();
        store.apply_message_pin_update(phone, &update("same", true, 20), true).unwrap();
        assert_eq!(store.marks(phone).unwrap().pinned_messages, ["other", "same"]);

        for expired_source in [false, true] {
            let store = MessageStore::open(Path::new(":memory:")).unwrap();
            store.apply_message_pin_update(phone, &update("other", true, 5), false).unwrap();
            if expired_source {
                store.apply_message_pin_update("123@lid", &expired, false).unwrap();
                store.apply_message_pin_update(phone, &update("same", true, 20), true).unwrap();
            } else {
                store.apply_message_pin_update(phone, &expired, false).unwrap();
                store.apply_message_pin_update("123@lid", &update("same", true, 20), true).unwrap();
            }
            store.set_lid_pn("123", "5989").unwrap();
            assert_eq!(store.marks(phone).unwrap().pinned_messages, ["other", "same"]);
        }
    }

    #[test]
    fn pruned_live_pin_does_not_lend_rank_to_alias_history_pin() {
        let phone = "5989@s.whatsapp.net";
        let lid = "123@lid";
        for pruned_source in [false, true] {
            let store = MessageStore::open(Path::new(":memory:")).unwrap();
            store.apply_message_pin_update(phone, &update("other", true, 5), false).unwrap();
            let live_chat = if pruned_source { lid } else { phone };
            let history_chat = if pruned_source { phone } else { lid };
            store.apply_message_pin_update(live_chat, &update("same", true, 10), false).unwrap();
            store.conn.lock().unwrap().execute("DELETE FROM message_pins WHERE chat=?1 AND id='same'", [live_chat]).unwrap();
            store.apply_message_pin_update(history_chat, &update("same", true, 20), true).unwrap();
            store.set_lid_pn("123", "5989").unwrap();
            assert_eq!(store.marks(phone).unwrap().pinned_messages, ["other", "same"]);
        }
    }
}
