//! Chats: summaries, pins and per-chat settings.

use super::*;

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct ChatPage {
    pub rows: Vec<ChatSummary>,
    pub next_cursor: Option<String>,
    pub archived_count: i64,
    pub unread_chats: i64,
    pub unread_mentions: i64,
    pub desktop_unread: i64,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatPageCursor {
    a: i64,
    p: i64,
    pn: i64,
    pt: i64,
    t: i64,
    o: i64,
    j: String,
}

fn validate_chat_page_cursor(after: Option<&str>) -> Result<()> {
    let Some(after) = after else { return Ok(()); };
    anyhow::ensure!(after.len() <= 4096, "invalid chat cursor");
    let cursor: ChatPageCursor = serde_json::from_str(after).map_err(|_| anyhow::anyhow!("invalid chat cursor"))?;
    anyhow::ensure!((0..=20_000).contains(&cursor.a) && matches!(cursor.p, 0 | 1)
        && matches!(cursor.pn, 0 | 1) && cursor.o >= 0
        && !cursor.j.is_empty() && cursor.j.len() <= 512, "invalid chat cursor");
    Ok(())
}

const CHAT_PAGE_SQL: &str = r#"
WITH allowed AS MATERIALIZED (SELECT value AS jid, key AS rank FROM json_each(?5)),
source_chats AS MATERIALIZED (
    SELECT jid, last_message_at, 1 AS known FROM chats
    UNION ALL SELECT a.jid, 0, 0 FROM allowed a WHERE ?4 IN ('favorites','space_all')
        AND NOT EXISTS (SELECT 1 FROM chats existing WHERE existing.jid=a.jid)
), ranked AS MATERIALIZED (
    SELECT c.jid, c.last_message_at, p.jid IS NOT NULL AS pinned,
           CASE WHEN p.jid IS NOT NULL AND ps.timestamp IS NULL THEN 1 ELSE 0 END AS pin_missing,
           CASE WHEN p.jid IS NOT NULL THEN COALESCE(ps.timestamp, 0) ELSE 0 END AS pin_time,
           COALESCE(a.rank,0) AS allowed_order,
           CASE WHEN c.known=0 THEN 0 ELSE COALESCE((SELECT MAX(mm.timestamp) FROM messages mm WHERE mm.chat=c.jid
               AND NOT (mm.deleted <> 0 AND mm.text = '' AND mm.media_kind IS NULL AND mm.system_kind IS NULL)), c.last_message_at) END AS sort_time,
           CASE WHEN c.known=0 THEN 0 ELSE COALESCE((SELECT mx.sort_order FROM messages mx WHERE mx.chat=c.jid AND mx.deleted=0
               AND (mx.system_kind IS NULL OR mx.system_kind='UNAVAILABLE_MESSAGE' OR mx.system_kind LIKE 'CALL_MISSED%'
                    OR mx.system_kind LIKE 'SILENCED_UNKNOWN_CALLER%')
               ORDER BY mx.timestamp DESC, mx.sort_order DESC, mx.id DESC LIMIT 1), 0) END AS sort_order,
           c.known
    FROM source_chats c
    LEFT JOIN allowed a ON a.jid=c.jid
    LEFT JOIN pins p ON p.jid=c.jid AND c.known=1
    LEFT JOIN pin_state ps ON ps.jid=c.jid AND ps.pinned=1 AND c.known=1
    LEFT JOIN chat_state cs ON cs.jid=c.jid AND c.known=1
    WHERE c.jid NOT IN (SELECT jid FROM hidden_chats)
      AND (?5 IS NULL OR a.jid IS NOT NULL)
      AND (CASE ?4 WHEN 'archived' THEN COALESCE(cs.archived,0)=1
           WHEN 'favorites' THEN 1 WHEN 'space_all' THEN 1
           ELSE COALESCE(cs.archived,0)=0 END)
      AND (?4 <> 'groups' OR c.jid LIKE '%@g.us')
      AND (?4 <> 'unread' OR COALESCE(cs.marked_unread,0)=1 OR EXISTS (
          SELECT 1 FROM messages um WHERE um.chat=c.jid AND um.read=0 AND um.from_me=0 AND um.deleted=0
            AND COALESCE(um.system_kind,'') <> 'UNAVAILABLE_MESSAGE'))
), ordered AS MATERIALIZED (
    SELECT ranked.*, CASE WHEN ?6=1 THEN allowed_order ELSE 0 END AS order_rank FROM ranked
), cursor AS MATERIALIZED (
    SELECT json_extract(?2,'$.a') AS a, json_extract(?2,'$.p') AS p,
           json_extract(?2,'$.pn') AS pn, json_extract(?2,'$.pt') AS pt,
           json_extract(?2,'$.t') AS t, json_extract(?2,'$.o') AS o,
           json_extract(?2,'$.j') AS j
), picked AS MATERIALIZED (
    SELECT o.* FROM ordered o CROSS JOIN cursor k WHERE ?2 IS NULL OR (o.jid<>k.j AND CASE
        WHEN o.order_rank<>k.a THEN o.order_rank>k.a
        WHEN o.pinned<>k.p THEN o.pinned<k.p
        WHEN o.pin_missing<>k.pn THEN o.pin_missing>k.pn
        WHEN o.pin_time<>k.pt THEN o.pin_time<k.pt
        WHEN o.sort_time<>k.t THEN o.sort_time<k.t
        WHEN o.sort_order<>k.o THEN o.sort_order<k.o
        ELSE o.jid>k.j END)
    ORDER BY o.order_rank, o.pinned DESC, o.pin_missing, o.pin_time DESC,
             o.sort_time DESC, o.sort_order DESC, o.jid LIMIT ?3
)
SELECT c.jid, COALESCE(g.last_message_at,c.last_message_at), COALESCE(g.message_count,0),
       n.name, COALESCE(g.unread_count,0), COALESCE(g.mention_count,0), c.pinned,
       COALESCE(CASE WHEN m.system_kind='UNAVAILABLE_MESSAGE' THEN 'Message unavailable'
                     WHEN m.spoiler=1 THEN '[Spoiler]' ELSE m.text END,''), COALESCE(m.from_me,0), s.name, COALESCE(m.sender,''),
       CASE WHEN m.system_kind='UNAVAILABLE_MESSAGE' THEN NULL WHEN m.system_kind IS NOT NULL THEN 'missed_call' ELSE m.media_kind END,
       COALESCE(cs.archived,0), COALESCE(cs.muted_until,0), COALESCE(cs.marked_unread,0), COALESCE(cset.mute_at_all,0),
       c.order_rank, c.pinned, c.pin_missing, c.pin_time, c.sort_time, c.sort_order
FROM picked c
LEFT JOIN (SELECT messages.chat, MAX(messages.timestamp) AS last_message_at, COUNT(*) AS message_count,
                 SUM(messages.read=0 AND messages.from_me=0 AND messages.deleted=0 AND COALESCE(messages.system_kind,'') <> 'UNAVAILABLE_MESSAGE') AS unread_count,
                 SUM(messages.read=0 AND messages.from_me=0 AND messages.mentioned=1 AND messages.deleted=0
                     AND COALESCE(messages.system_kind,'') <> 'UNAVAILABLE_MESSAGE'
                     AND NOT (COALESCE(messages.mentioned_all_only,0)=1
                              AND (COALESCE(cset.mute_at_all,0)=1 OR ?1=1))) AS mention_count
           FROM messages LEFT JOIN chat_settings cset ON cset.jid=messages.chat
           WHERE messages.chat IN (SELECT jid FROM picked)
             AND NOT (messages.deleted <> 0 AND messages.text='' AND messages.media_kind IS NULL AND messages.system_kind IS NULL)
           GROUP BY messages.chat) g ON g.chat=c.jid AND c.known=1
LEFT JOIN messages m ON c.known=1 AND m.rowid=(SELECT rowid FROM messages WHERE chat=c.jid AND deleted=0
    AND (system_kind IS NULL OR system_kind='UNAVAILABLE_MESSAGE' OR system_kind LIKE 'CALL_MISSED%'
         OR system_kind LIKE 'SILENCED_UNKNOWN_CALLER%')
    ORDER BY timestamp DESC, sort_order DESC, id DESC LIMIT 1)
LEFT JOIN names n ON n.jid=c.jid AND c.known=1 LEFT JOIN names s ON s.jid=m.sender
LEFT JOIN chat_state cs ON cs.jid=c.jid AND c.known=1 LEFT JOIN chat_settings cset ON cset.jid=c.jid AND c.known=1
ORDER BY c.order_rank, c.pinned DESC, c.pin_missing, c.pin_time DESC, c.sort_time DESC, c.sort_order DESC, c.jid
"#;

fn summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChatSummary> {
    Ok(ChatSummary {
        chat: row.get(0)?, last_message_at: row.get(1)?, message_count: row.get(2)?,
        display_name: row.get(3)?, unread_count: row.get(4)?, mention_count: row.get(5)?,
        pinned: row.get::<_, i64>(6)? != 0, last_text: row.get(7)?,
        last_from_me: row.get::<_, i64>(8)? != 0, last_sender_name: row.get(9)?,
        last_sender: row.get(10)?, last_media_kind: row.get(11)?,
        archived: row.get::<_, i64>(12)? != 0, muted_until: row.get(13)?,
        marked_unread: row.get::<_, i64>(14)? != 0, mute_at_all: row.get::<_, i64>(15)? != 0,
    })
}

pub(super) fn reconcile_addresses(conn: &Connection) -> Result<()> {
    super::event_rsvps::reconcile_event_responders(conn)?;
    // One direct chat can be stored under both its LID and phone-number
    // forms, which shows the same contact twice. Fold the LID copy onto the
    // phone-number one; the write path now keys direct chats by number.
    let lid_chats: Vec<String> = {
        let mut found = std::collections::BTreeSet::new();
        for (table, column) in [
            ("messages", "chat"),
            ("message_pin_sync", "chat"),
            ("chats", "jid"),
            ("chat_state", "jid"),
            ("pins", "jid"),
            ("pin_state", "jid"),
            ("cleared_chats", "jid"),
            ("hidden_chats", "jid"),
        ] {
            let mut stmt = conn.prepare(&format!(
                "SELECT DISTINCT {column} FROM {table} WHERE {column} LIKE '%@lid'"
            ))?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            for row in rows {
                found.insert(row?);
            }
        }
        found.into_iter().collect()
    };
    if !lid_chats.is_empty() {
        let tx = conn.unchecked_transaction()?;
        for lid_chat in &lid_chats {
            let Some(user) = lid_chat.split('@').next().and_then(|u| u.split(':').next()) else {
                continue;
            };
            let pn: Option<String> = tx
                .query_row("SELECT pn FROM lid_pn WHERE lid = ?1", params![user], |r| {
                    r.get(0)
                })
                .optional()?;
            if let Some(pn) = pn {
                fold_chat(&tx, lid_chat, &format!("{pn}@s.whatsapp.net"))?;
            }
        }
        tx.commit()?;
    }
    Ok(())
}

impl MessageStore {
    /// The per chat auto download override, if one is set.
    pub fn chat_auto_download(&self, jid: &str) -> Result<Option<bool>> {
        let conn = self.conn.lock().unwrap();
        let jid = &*names::canonical_chat(&conn, jid)?;
        let value = conn
            .query_row(
                "SELECT auto_download FROM chat_settings WHERE jid = ?1",
                params![jid],
                |r| r.get::<_, i32>(0),
            )
            .optional()?;
        Ok(value.map(|v| v != 0))
    }

    /// Sets the per chat auto download override.
    pub fn set_chat_auto_download(&self, jid: &str, enabled: bool) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.savepoint()?;
        let jid = &*names::canonical_chat(&tx, jid)?;
        tx.execute(
            "INSERT INTO chat_settings (jid, auto_download) VALUES (?1, ?2)
             ON CONFLICT(jid) DO UPDATE SET auto_download = excluded.auto_download",
            params![jid, enabled as i32],
        )?;
        super::media_policy::write_overrides(&tx, jid, super::media_policy::MediaAutoDownloadOverrides::all(Some(enabled)))?;
        tx.commit()?;
        Ok(())
    }

    /// The chat's typing and read receipt overrides; `None` follows the global setting.
    pub fn chat_privacy(&self, jid: &str) -> Result<(Option<bool>, Option<bool>)> {
        let conn = self.conn.lock().unwrap();
        let jid = &*names::canonical_chat(&conn, jid)?;
        let value = conn
            .query_row(
                "SELECT send_typing, send_receipts FROM chat_privacy WHERE jid = ?1",
                params![jid],
                |r| Ok((r.get::<_, Option<bool>>(0)?, r.get::<_, Option<bool>>(1)?)),
            )
            .optional()?;
        Ok(value.unwrap_or_default())
    }

    /// Sets the chat's typing and read receipt overrides; both `None` removes them.
    pub fn set_chat_privacy(&self, jid: &str, typing: Option<bool>, receipts: Option<bool>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let jid = &*names::canonical_chat(&conn, jid)?;
        if typing.is_none() && receipts.is_none() {
            conn.execute("DELETE FROM chat_privacy WHERE jid = ?1", params![jid])?;
        } else {
            conn.execute(
                "INSERT INTO chat_privacy (jid, send_typing, send_receipts) VALUES (?1, ?2, ?3)
                 ON CONFLICT(jid) DO UPDATE SET send_typing = excluded.send_typing,
                                                send_receipts = excluded.send_receipts",
                params![jid, typing, receipts],
            )?;
        }
        Ok(())
    }

    /// Mirrors a chat's pin state from the account.
    pub fn set_pinned(&self, jid: &str, pinned: bool) -> Result<()> {
        self.mirror_pin(jid, pinned)
    }

    /// Mirrors a chat's archive state from the account.
    pub fn set_archived(&self, jid: &str, archived: bool) -> Result<()> {
        self.set_chat_state(jid, "archived", archived as i64)
    }

    /// Whether a chat is archived; false when it has no state row.
    pub fn is_archived(&self, jid: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let jid = &*names::canonical_chat(&conn, jid)?;
        Ok(conn
            .query_row(
                "SELECT archived FROM chat_state WHERE jid = ?1",
                params![jid],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .is_some_and(|v| v != 0))
    }

    /// Mirrors a chat's mute end (seconds; -1 indefinitely, 0 unmuted).
    pub fn set_muted_until(&self, jid: &str, until: i64) -> Result<()> {
        self.set_chat_state(jid, "muted_until", until)
    }

    pub fn muted_until(&self, jid: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let jid = &*names::canonical_chat(&conn, jid)?;
        Ok(conn.query_row("SELECT muted_until FROM chat_state WHERE jid = ?1", [jid], |row| row.get(0)).optional()?.unwrap_or(0))
    }

    /// Mirrors a chat's manual unread mark from the account.
    pub fn set_marked_unread(&self, jid: &str, unread: bool) -> Result<()> {
        self.set_chat_state(jid, "marked_unread", unread as i64)
    }

    /// Lifts a manual unread mark; true if one was set.
    pub fn clear_marked_unread(&self, jid: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let jid = &*names::canonical_chat(&conn, jid)?;
        let changed =
            conn.execute("UPDATE chat_state SET marked_unread = 0 WHERE jid = ?1 AND marked_unread = 1", params![jid])?;
        Ok(changed > 0)
    }

    fn set_chat_state(&self, jid: &str, column: &str, value: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let jid = &*names::canonical_chat(&conn, jid)?;
        conn.execute(
            &format!(
                "INSERT INTO chat_state (jid, {column}) VALUES (?1, ?2)
                 ON CONFLICT(jid) DO UPDATE SET {column} = excluded.{column}"
            ),
            params![jid, value],
        )?;
        Ok(())
    }

    /// The pinned chats.
    pub fn pinned_chats(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT jid FROM pins")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// One summary per chat, most recently active first.
    ///
    /// Deleted chats stay hidden until a new message arrives; cleared chats
    /// stay as empty rows so the conversation keeps its place in the list.
    pub fn chats(&self) -> Result<Vec<ChatSummary>> {
        self.chats_with(false)
    }

    /// Chat summaries where `mute_all_at_all` additionally hides every
    /// @all-only mention, for the global "mute @all everywhere" setting.
    /// Direct mentions still count.
    pub fn chats_with(&self, mute_all_at_all: bool) -> Result<Vec<ChatSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            // The preview row is one index seek per chat; a window over every
            // message would copy the whole table into a temporary sort.
            "SELECT c.jid, COALESCE(g.last_message_at, c.last_message_at), COALESCE(g.message_count, 0),
                    n.name, COALESCE(g.unread_count, 0), COALESCE(g.mention_count, 0), p.jid IS NOT NULL AS pinned,
                    COALESCE(CASE WHEN m.system_kind = 'UNAVAILABLE_MESSAGE' THEN 'Message unavailable'
                                  WHEN m.spoiler = 1 THEN '[Spoiler]' ELSE m.text END, ''), COALESCE(m.from_me, 0), s.name, COALESCE(m.sender, ''),
                    CASE WHEN m.system_kind = 'UNAVAILABLE_MESSAGE' THEN NULL
                         WHEN m.system_kind IS NOT NULL THEN 'missed_call' ELSE m.media_kind END,
                    COALESCE(cs.archived, 0), COALESCE(cs.muted_until, 0), COALESCE(cs.marked_unread, 0),
                    COALESCE(cset.mute_at_all, 0)
             FROM chats c
             LEFT JOIN (SELECT messages.chat AS chat,
                          MAX(messages.timestamp) AS last_message_at,
                          COUNT(*) AS message_count,
                          SUM(messages.read = 0 AND messages.from_me = 0 AND messages.deleted = 0 AND COALESCE(messages.system_kind, '') <> 'UNAVAILABLE_MESSAGE') AS unread_count,
                          SUM(messages.read = 0 AND messages.from_me = 0 AND messages.mentioned = 1 AND messages.deleted = 0 AND COALESCE(messages.system_kind, '') <> 'UNAVAILABLE_MESSAGE'
                              AND NOT (COALESCE(messages.mentioned_all_only, 0) = 1
                                       AND (COALESCE(cset.mute_at_all, 0) = 1 OR ? = 1))) AS mention_count
                   FROM messages LEFT JOIN chat_settings cset ON cset.jid = messages.chat
                   WHERE NOT (messages.deleted <> 0 AND messages.text = '' AND messages.media_kind IS NULL AND messages.system_kind IS NULL)
                   GROUP BY messages.chat) g ON g.chat = c.jid
             LEFT JOIN messages m ON m.rowid =
                  (SELECT rowid FROM messages WHERE chat = c.jid
                     AND deleted = 0
                     AND (system_kind IS NULL OR system_kind = 'UNAVAILABLE_MESSAGE' OR system_kind LIKE 'CALL_MISSED%' OR system_kind LIKE 'SILENCED_UNKNOWN_CALLER%')
                   ORDER BY timestamp DESC, sort_order DESC, id DESC LIMIT 1)
             LEFT JOIN names n ON n.jid = c.jid
             LEFT JOIN names s ON s.jid = m.sender
             LEFT JOIN pins p ON p.jid = c.jid
             LEFT JOIN chat_state cs ON cs.jid = c.jid
             LEFT JOIN chat_settings cset ON cset.jid = c.jid
             WHERE c.jid NOT IN (SELECT jid FROM hidden_chats)
             -- The pin key applies only to pinned chats: the account's pin state
             -- also keeps unpinned tombstones with millisecond timestamps, and
             -- letting those outrank recency freezes the whole list.
             ORDER BY pinned DESC, (SELECT timestamp FROM pin_state WHERE jid=c.jid AND pinned = 1) DESC, COALESCE(g.last_message_at, c.last_message_at) DESC, m.sort_order DESC, c.jid",
        )?;
        let summaries = stmt
            .query_map([mute_all_at_all as i32], summary)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(summaries)
    }

    pub fn chats_page(&self, mute_all_at_all: bool, filter: &str, allowed: Option<&[String]>, order_allowed: bool, after: Option<&str>, limit: usize) -> Result<ChatPage> {
        anyhow::ensure!((1..=100).contains(&limit), "invalid chat page size");
        anyhow::ensure!(matches!(filter, "all" | "space_all" | "archived" | "unread" | "groups" | "favorites"), "invalid chat filter");
        anyhow::ensure!(filter != "favorites" || allowed.is_some(), "favorites need a chat selection");
        if let Some(allowed) = allowed {
            anyhow::ensure!(allowed.len() <= 20_000 && allowed.iter().all(|jid| !jid.is_empty() && jid.len() <= 512), "invalid chat selection");
        }
        validate_chat_page_cursor(after)?;
        let allowed = allowed.map(|ids| {
            let mut seen = std::collections::HashSet::new();
            serde_json::to_string(&ids.iter().filter(|jid| seen.insert(jid.as_str())).collect::<Vec<_>>())
        }).transpose()?;
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(CHAT_PAGE_SQL)?;
        let mut rows = stmt.query_map(rusqlite::params![mute_all_at_all as i32, after, (limit + 1) as i64, filter, allowed, order_allowed as i32], |row| {
            let summary = summary(row)?;
            let cursor = ChatPageCursor { a: row.get(16)?, p: row.get(17)?, pn: row.get(18)?, pt: row.get(19)?, t: row.get(20)?, o: row.get(21)?, j: summary.chat.clone() };
            Ok((summary, cursor))
        })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let next_cursor = if rows.len() > limit { rows.truncate(limit); rows.last().map(|(_, cursor)| serde_json::to_string(cursor)).transpose()? } else { None };
        let rows = rows.into_iter().map(|(summary, _)| summary).collect();
        let (archived_count, unread_chats, unread_mentions, desktop_unread) = if after.is_none() {
            Self::chat_page_totals(&conn, mute_all_at_all)?
        } else { (0, 0, 0, 0) };
        Ok(ChatPage { rows, next_cursor, archived_count, unread_chats, unread_mentions, desktop_unread })
    }

    fn chat_page_totals(conn: &rusqlite::Connection, mute_all_at_all: bool) -> Result<(i64, i64, i64, i64)> {
        Ok(conn.query_row(
            "WITH counts AS (SELECT m.chat,
                 SUM(m.read=0 AND m.from_me=0 AND m.deleted=0 AND COALESCE(m.system_kind,'') <> 'UNAVAILABLE_MESSAGE') AS unread,
                 SUM(m.read=0 AND m.from_me=0 AND m.mentioned=1 AND m.deleted=0
                     AND COALESCE(m.system_kind,'') <> 'UNAVAILABLE_MESSAGE'
                     AND NOT (COALESCE(m.mentioned_all_only,0)=1 AND (COALESCE(s.mute_at_all,0)=1 OR ?1=1))) AS mentions
               FROM messages m LEFT JOIN chat_settings s ON s.jid=m.chat
               WHERE NOT (m.deleted<>0 AND m.text='' AND m.media_kind IS NULL AND m.system_kind IS NULL)
               GROUP BY m.chat)
             SELECT COALESCE(SUM(COALESCE(cs.archived,0)=1),0),
                    COALESCE(SUM(COALESCE(cs.archived,0)=0 AND (COALESCE(g.unread,0)>0 OR COALESCE(cs.marked_unread,0)=1)),0),
                    COALESCE(SUM(g.mentions),0),
                    COALESCE(SUM(CASE WHEN COALESCE(cs.archived,0)=1 THEN 0 WHEN COALESCE(g.unread,0)>0 THEN g.unread
                         WHEN COALESCE(cs.marked_unread,0)=1 THEN 1 ELSE 0 END),0)
             FROM chats c LEFT JOIN chat_state cs ON cs.jid=c.jid LEFT JOIN counts g ON g.chat=c.jid
             WHERE c.jid NOT IN (SELECT jid FROM hidden_chats)",
            [mute_all_at_all as i32], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?)
    }

    /// Drops every stored row for one chat, keeping names, pins and settings.
    /// Returns how many messages went. Local-only: the phone keeps its copy.
    fn drop_chat_messages(&self, conn: &rusqlite::Connection, jid: &str) -> Result<usize> {
        conn.execute("DELETE FROM message_pin_sync WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM receipts WHERE id IN (SELECT id FROM messages WHERE chat = ?1)", params![jid])?;
        conn.execute("DELETE FROM reactions WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM stars WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM message_pins WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM polls WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM poll_votes WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM quiz_polls WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM quiz_vote_ciphers WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM quiz_source_retirements WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM broadcast_lists WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM poll_option_hashes WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM secret_edit_revisions WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM events WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM event_responses WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM event_rsvp_pending WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM view_once WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM forwarded WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM edited WHERE chat = ?1", params![jid])?;
        // Media files left without a referent are removed from disk.
        let paths: Vec<String> = {
            let mut stmt = conn.prepare("SELECT media_path FROM messages WHERE chat = ?1 AND media_path IS NOT NULL")?;
            let paths = stmt
                .query_map(params![jid], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?;
            paths
        };
        let removed = conn.execute("DELETE FROM messages WHERE chat = ?1", params![jid])?;
        super::group_audit::clear(conn, jid)?;
        conn.execute("DELETE FROM chat_history_floor WHERE jid = ?1", params![jid])?;
        for path in paths {
            if let Err(error) = std::fs::remove_file(path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    log::error!("could not remove deleted chat media: {error}");
                }
            }
        }
        Ok(removed)
    }

    /// Clears one chat: messages go, the empty chat stays in the list.
    pub fn clear_chat(&self, jid: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let jid = &*names::canonical_chat(&conn, jid)?;
        let removed = self.drop_chat_messages(&conn, jid)?;
        conn.execute("DELETE FROM hidden_chats WHERE jid = ?1", params![jid])?;
        conn.execute("INSERT OR IGNORE INTO cleared_chats (jid) VALUES (?1)", params![jid])?;
        conn.execute("INSERT OR IGNORE INTO chats (jid) VALUES (?1)", params![jid])?;
        reclaim(&conn, 0)?;
        Ok(removed)
    }

    /// Deletes one chat: messages go and the chat leaves the list until a new
    /// message arrives. Local-only: the phone keeps its copy.
    pub fn delete_chat(&self, jid: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let jid = &*names::canonical_chat(&conn, jid)?;
        let removed = self.drop_chat_messages(&conn, jid)?;
        conn.execute("DELETE FROM cleared_chats WHERE jid = ?1", params![jid])?;
        conn.execute("DELETE FROM pins WHERE jid = ?1", params![jid])?;
        conn.execute("DELETE FROM chats WHERE jid = ?1", params![jid])?;
        conn.execute("INSERT OR IGNORE INTO hidden_chats (jid) VALUES (?1)", params![jid])?;
        reclaim(&conn, 0)?;
        Ok(removed)
    }

    /// A new message unhides its chat and retires its kept-empty row.
    pub(crate) fn revive_chat(&self, conn: &rusqlite::Connection, jid: &str) -> Result<()> {
        conn.execute("DELETE FROM hidden_chats WHERE jid = ?1", params![jid])?;
        conn.execute("DELETE FROM cleared_chats WHERE jid = ?1", params![jid])?;
        Ok(())
    }
}

/// Whether any table keeps rows for a chat: messages or any of its list state.
/// Moves every row from one chat to `to`, keeping whatever state either side
/// had. A chat that just gained messages is never left hidden or kept-empty.
pub(crate) fn fold_chat(conn: &Connection, from: &str, to: &str) -> Result<()> {
    if from == to {
        return Ok(());
    }
    super::event_rsvps::reconcile_event_responders(conn)?;
    copy_shadowed_messages(conn, from, to)?;
    super::albums::merge_written(conn, from, to)?;
    merge_chat_row(conn, from, to)?;
    super::secret_edits::merge(conn, from, to)?;
    move_chat_keyed_tables(conn, from, to, MESSAGE_STATE_TABLES, "chat")?;
    super::history_pins::merge(conn, from, to)?;
    super::labels::merge(conn, from, to)?;
    super::group_audit::merge(conn, from, to)?;
    super::member_profiles::merge(conn, from, to)?;
    // Messages last, so `to` knows it has history before the state below.
    conn.execute("UPDATE OR IGNORE messages SET chat = ?1 WHERE chat = ?2", params![to, from])?;
    conn.execute("DELETE FROM messages WHERE chat = ?1", params![from])?;
    conn.execute(
        "UPDATE OR IGNORE messages SET reply_to_chat = ?1 WHERE reply_to_chat = ?2",
        params![to, from],
    )?;
    // Archive, mute and unread marks: keep whichever side had them set.
    merge_chat_state(conn, from, to)?;
    merge_pin(conn, from, to)?;
    super::media_policy::merge(conn, from, to)?;
    super::notification_prefs::merge(conn, from, to)?;
    super::chat_unarchive::merge(conn, from, to)?;
    super::retention::merge_history_floor(conn, from, to)?;
    move_chat_keyed_tables(conn, from, to, CHAT_SETTING_TABLES, "jid")?;
    move_list_flags(conn, from, to)?;
    adopt_name(conn, from, to)?;
    Ok(())
}

/// Tables whose rows are keyed by (chat, message) and travel with messages.
const MESSAGE_STATE_TABLES: &[&str] = &[
    "reactions",
    "stars",
    "message_pins",
    "polls",
    "poll_votes",
    "quiz_polls",
    "quiz_vote_ciphers",
    "quiz_source_retirements",
    "transcripts",
    "events",
    "event_responses",
    "event_rsvp_pending",
    "view_once",
    "forwarded",
    "edited",
];

/// Tables with one row per chat, carrying the chat's own settings.
const CHAT_SETTING_TABLES: &[&str] = &["chat_privacy", "chat_settings", "chat_retention"];

/// Rows a message id exists under in both chats: `INSERT OR IGNORE` would drop
/// the incoming copy, so they are re-inserted under the surviving chat first.
fn copy_shadowed_messages(conn: &Connection, from: &str, to: &str) -> Result<()> {
    let mut duplicates = conn.prepare(&format!(
        "SELECT {MESSAGE_COLUMNS} FROM messages m LEFT JOIN names n ON n.jid = m.sender
         WHERE m.chat = ?1 AND EXISTS (SELECT 1 FROM messages t WHERE t.chat = ?2 AND t.id = m.id)"
    ))?;
    for row in duplicates.query_map(params![from, to], message_row)? {
        let mut row = row?;
        row.header.chat = to.to_string();
        MessageStore::insert_row(conn, &row)?;
    }
    Ok(())
}

fn merge_chat_row(conn: &Connection, from: &str, to: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO chats (jid, last_message_at)
         SELECT ?1, last_message_at FROM chats WHERE jid = ?2
         ON CONFLICT(jid) DO UPDATE SET last_message_at = MAX(chats.last_message_at, excluded.last_message_at)",
        params![to, from],
    )?;
    conn.execute("DELETE FROM chats WHERE jid = ?1", params![from])?;
    Ok(())
}

/// Moves rows in the given chat-keyed tables, keeping the survivor's rows when
/// both sides have one (the first UPDATE wins under `OR IGNORE`).
fn move_chat_keyed_tables(conn: &Connection, from: &str, to: &str, tables: &[&str], column: &str) -> Result<()> {
    for table in tables {
        conn.execute(
            &format!("UPDATE OR IGNORE {table} SET {column} = ?1 WHERE {column} = ?2"),
            params![to, from],
        )?;
        conn.execute(&format!("DELETE FROM {table} WHERE {column} = ?1"), params![from])?;
    }
    Ok(())
}

fn merge_chat_state(conn: &Connection, from: &str, to: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO chat_state (jid, archived, muted_until, marked_unread)
         SELECT ?1, archived, muted_until, marked_unread FROM chat_state WHERE jid = ?2
         ON CONFLICT(jid) DO UPDATE SET
             archived = MAX(chat_state.archived, excluded.archived),
             muted_until = MAX(chat_state.muted_until, excluded.muted_until),
             marked_unread = MAX(chat_state.marked_unread, excluded.marked_unread)",
        params![to, from],
    )?;
    conn.execute("DELETE FROM chat_state WHERE jid = ?1", params![from])?;
    Ok(())
}

fn merge_pin(conn: &Connection, from: &str, to: &str) -> Result<()> {
    if conn
        .query_row("SELECT 1 FROM pins WHERE jid = ?1", params![from], |r| r.get::<_, i64>(0))
        .optional()?
        .is_some()
    {
        conn.execute("INSERT OR IGNORE INTO pins (jid) VALUES (?1)", params![to])?;
    }
    conn.execute("DELETE FROM pins WHERE jid = ?1", params![from])?;
    super::pins::merge(conn, from, to)?;
    Ok(())
}

/// A cleared or deleted chat keeps its empty row or stays hidden only while it
/// has no history; the merged chat must not be hidden.
fn move_list_flags(conn: &Connection, from: &str, to: &str) -> Result<()> {
    let has_messages: Option<i64> = conn
        .query_row("SELECT 1 FROM messages WHERE chat = ?1 LIMIT 1", params![to], |r| r.get(0))
        .optional()?;
    if has_messages.is_none() {
        conn.execute(
            "INSERT OR IGNORE INTO cleared_chats (jid) SELECT ?1 FROM cleared_chats WHERE jid = ?2",
            params![to, from],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO hidden_chats (jid) SELECT ?1 FROM hidden_chats WHERE jid = ?2",
            params![to, from],
        )?;
    }
    conn.execute("DELETE FROM cleared_chats WHERE jid = ?1", params![from])?;
    conn.execute("DELETE FROM hidden_chats WHERE jid = ?1", params![from])?;
    Ok(())
}

/// The phone-number row keeps its name; adopt the other only when it has none.
fn adopt_name(conn: &Connection, from: &str, to: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO names (jid, name, saved)
         SELECT ?1, name, saved FROM names WHERE jid = ?2
           AND NOT EXISTS (SELECT 1 FROM names WHERE jid = ?1)",
        params![to, from],
    )?;
    Ok(())
}

impl StoreWorker {
    pub(crate) async fn chat_auto_download(&self, jid: &str) -> Result<Option<bool>> {
        let jid = jid.to_owned();
        self.run(move |store| store.chat_auto_download(&jid)).await
    }

    pub(crate) async fn set_chat_auto_download(&self, jid: &str, enabled: bool) -> Result<()> {
        let jid = jid.to_owned();
        self.run(move |store| store.set_chat_auto_download(&jid, enabled)).await
    }

    pub(crate) async fn chat_privacy(&self, jid: &str) -> Result<(Option<bool>, Option<bool>)> {
        let jid = jid.to_owned();
        self.run(move |store| store.chat_privacy(&jid)).await
    }

    pub(crate) async fn set_chat_privacy(&self, jid: &str, typing: Option<bool>, receipts: Option<bool>) -> Result<()> {
        let jid = jid.to_owned();
        self.run(move |store| store.set_chat_privacy(&jid, typing, receipts)).await
    }

    pub(crate) async fn set_archived(&self, jid: &str, archived: bool) -> Result<()> {
        let jid = jid.to_owned();
        self.run(move |store| store.set_archived(&jid, archived)).await
    }

    pub(crate) async fn is_archived(&self, jid: &str) -> Result<bool> {
        let jid = jid.to_owned();
        self.run(move |store| store.is_archived(&jid)).await
    }

    pub(crate) async fn set_muted_until(&self, jid: &str, until: i64) -> Result<()> {
        let jid = jid.to_owned();
        self.run(move |store| store.set_muted_until(&jid, until)).await
    }

    pub(crate) async fn muted_until(&self, jid: &str) -> Result<i64> {
        let jid = jid.to_owned();
        self.run(move |store| store.muted_until(&jid)).await
    }

    pub(crate) async fn set_marked_unread(&self, jid: &str, unread: bool) -> Result<()> {
        let jid = jid.to_owned();
        self.run(move |store| store.set_marked_unread(&jid, unread)).await
    }

    pub(crate) async fn clear_marked_unread(&self, jid: &str) -> Result<bool> {
        let jid = jid.to_owned();
        self.run(move |store| store.clear_marked_unread(&jid)).await
    }

    pub(crate) async fn chats(&self) -> Result<Vec<ChatSummary>> {
        self.run(move |store| store.chats()).await
    }

    pub(crate) async fn chats_with(&self, mute_all_at_all: bool) -> Result<Vec<ChatSummary>> {
        self.run(move |store| store.chats_with(mute_all_at_all)).await
    }

    pub(crate) async fn chats_page(&self, mute_all_at_all: bool, filter: String, allowed: Option<Vec<String>>, order_allowed: bool, after: Option<String>, limit: usize) -> Result<ChatPage> {
        self.run(move |store| store.chats_page(mute_all_at_all, &filter, allowed.as_deref(), order_allowed, after.as_deref(), limit)).await
    }

    pub(crate) async fn clear_chat(&self, jid: &str) -> Result<usize> {
        let jid = jid.to_owned();
        self.run(move |store| store.clear_chat(&jid)).await
    }

    pub(crate) async fn delete_chat(&self, jid: &str) -> Result<usize> {
        let jid = jid.to_owned();
        self.run(move |store| store.delete_chat(&jid)).await
    }
}

#[cfg(test)]
mod page_tests {
    use super::*;

    #[test]
    fn removed_or_reordered_anchor_keeps_the_remaining_sort_suffix() {
        for remove in [true, false] {
            let store = MessageStore::open(Path::new(":memory:")).unwrap();
            let conn = store.conn.lock().unwrap();
            for at in 1..=6 {
                let jid = format!("{at}@s.whatsapp.net");
                conn.execute("INSERT INTO chats(jid,last_message_at) VALUES (?1,?2)", params![jid, at]).unwrap();
            }
            drop(conn);
            let first = store.chats_page(false, "all", None, false, None, 2).unwrap();
            assert_eq!(first.rows.iter().map(|row| row.chat.as_str()).collect::<Vec<_>>(), ["6@s.whatsapp.net", "5@s.whatsapp.net"]);
            let after = first.next_cursor.unwrap();
            if remove {
                store.conn.lock().unwrap().execute("DELETE FROM chats WHERE jid='5@s.whatsapp.net'", []).unwrap();
            } else {
                store.conn.lock().unwrap().execute("UPDATE chats SET last_message_at=0 WHERE jid='5@s.whatsapp.net'", []).unwrap();
            }
            if remove {
                let mut found = Vec::new();
                let mut cursor = Some(after);
                while let Some(after) = cursor {
                    let page = store.chats_page(false, "all", None, false, Some(&after), 2).unwrap();
                    found.extend(page.rows.into_iter().map(|row| row.chat));
                    cursor = page.next_cursor;
                }
                assert_eq!(found, ["4@s.whatsapp.net", "3@s.whatsapp.net", "2@s.whatsapp.net", "1@s.whatsapp.net"]);
            } else {
                let next = store.chats_page(false, "all", None, false, Some(&after), 2).unwrap();
                assert_eq!(next.rows.iter().map(|row| row.chat.as_str()).collect::<Vec<_>>(), ["4@s.whatsapp.net", "3@s.whatsapp.net"]);
                let refreshed = store.chats_page(false, "all", None, false, None, 100).unwrap();
                assert_eq!(refreshed.rows.into_iter().map(|row| row.chat).collect::<Vec<_>>(),
                    store.chats().unwrap().into_iter().map(|row| row.chat).collect::<Vec<_>>());
            }
        }
    }

    #[test]
    fn ordered_favorites_page_includes_more_than_sixty_four_empty_chats() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        let allowed = (0..130).map(|i| format!("favorite-{i:03}@s.whatsapp.net")).collect::<Vec<_>>();
        let mut found = Vec::new();
        let mut cursor = None;
        loop {
            let page = store.chats_page(false, "favorites", Some(&allowed), true, cursor.as_deref(), 64).unwrap();
            assert!(page.rows.iter().all(|row| row.message_count == 0 && row.last_text.is_empty() && !row.pinned));
            found.extend(page.rows.into_iter().map(|row| row.chat));
            cursor = page.next_cursor;
            if cursor.is_none() { break; }
        }
        assert_eq!(found, allowed);
    }

    #[test]
    fn cursor_validation_and_nullable_pin_time_match_legacy_order() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        let conn = store.conn.lock().unwrap();
        conn.execute_batch("INSERT INTO chats(jid,last_message_at) VALUES
            ('a@s.whatsapp.net',1),('b@s.whatsapp.net',2),('c@s.whatsapp.net',-9223372036854775808);
            INSERT INTO pins(jid) VALUES ('a@s.whatsapp.net'),('b@s.whatsapp.net');
            INSERT INTO pin_state(jid,pinned,timestamp,sequence) VALUES ('b@s.whatsapp.net',1,0,1);").unwrap();
        drop(conn);
        let expected = store.chats().unwrap().into_iter().map(|row| row.chat).collect::<Vec<_>>();
        let first = store.chats_page(false, "all", None, false, None, 1).unwrap();
        let second = store.chats_page(false, "all", None, false, first.next_cursor.as_deref(), 1).unwrap();
        let third = store.chats_page(false, "all", None, false, second.next_cursor.as_deref(), 1).unwrap();
        assert_eq!([first.rows[0].chat.clone(), second.rows[0].chat.clone(), third.rows[0].chat.clone()], expected.as_slice());
        assert!(store.chats_page(false, "all", None, false, Some("{"), 10).is_err());
        assert!(store.chats_page(false, "all", None, false, Some(&"x".repeat(4097)), 10).is_err());
    }

    #[test]
    fn five_thousand_chats_page_without_gaps_and_keep_pins_and_archive_filter() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        {
            let mut conn = store.conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            for i in 0..5_000 {
                let jid = format!("{i:05}@s.whatsapp.net");
                tx.execute("INSERT INTO chats(jid,last_message_at) VALUES (?1,?2)", rusqlite::params![&jid, i]).unwrap();
                tx.execute("INSERT INTO messages(chat,id,sender,timestamp,from_me,text,read) VALUES (?1,?2,?1,?3,1,'fixture',1)",
                    rusqlite::params![&jid, format!("m{i}"), i]).unwrap();
            }
            for (jid, at) in [("00001@s.whatsapp.net", 100), ("04000@s.whatsapp.net", 200)] {
                tx.execute("INSERT INTO pins(jid) VALUES (?1)", [jid]).unwrap();
                tx.execute("INSERT INTO pin_state(jid,pinned,timestamp,sequence) VALUES (?1,1,?2,?2)", rusqlite::params![jid, at]).unwrap();
            }
            tx.execute("INSERT INTO chat_state(jid,archived,marked_unread) VALUES ('03000@s.whatsapp.net',1,0)", []).unwrap();
            tx.execute("INSERT INTO chat_state(jid,archived,marked_unread) VALUES ('02000@s.whatsapp.net',0,1)", []).unwrap();
            tx.commit().unwrap();
        }
        let started = std::time::Instant::now();
        let first = store.chats_page(false, "all", None, false, None, 64).unwrap();
        let first_elapsed = started.elapsed();
        assert_eq!(first.rows.len(), 64);
        assert_eq!(first.rows[0].chat, "04000@s.whatsapp.net");
        assert_eq!(first.rows[1].chat, "00001@s.whatsapp.net");
        assert_eq!((first.archived_count, first.unread_chats, first.desktop_unread), (1, 1, 1));
        let rest_started = std::time::Instant::now();
        let mut pages = 1;
        let mut ids: Vec<String> = first.rows.iter().map(|row| row.chat.clone()).collect();
        let mut cursor = first.next_cursor;
        while let Some(after) = cursor {
            let page = store.chats_page(false, "all", None, false, Some(&after), 64).unwrap();
            ids.extend(page.rows.iter().map(|row| row.chat.clone()));
            cursor = page.next_cursor;
            pages += 1;
        }
        let rest_elapsed = rest_started.elapsed();
        let expected: Vec<String> = store.chats().unwrap().into_iter().filter(|row| !row.archived).map(|row| row.chat).collect();
        assert_eq!(ids, expected);
        assert_eq!(ids.len(), 4_999);
        assert_eq!(store.chats_page(false, "archived", None, false, None, 64).unwrap().rows[0].chat, "03000@s.whatsapp.net");
        assert_eq!(store.chats_page(false, "unread", None, false, None, 64).unwrap().rows[0].chat, "02000@s.whatsapp.net");
        let space = ["03000@s.whatsapp.net".into(), "00001@s.whatsapp.net".into()];
        let page = store.chats_page(false, "space_all", Some(&space), true, None, 64).unwrap();
        assert_eq!(page.rows.iter().map(|row| row.chat.as_str()).collect::<Vec<_>>(), space.iter().map(String::as_str).collect::<Vec<_>>());
        eprintln!("5k chats: first page {first_elapsed:?}, remaining {rest_elapsed:?}, total {:?}, {pages} pages of up to 64 summaries",
            first_elapsed + rest_elapsed);
    }
}
