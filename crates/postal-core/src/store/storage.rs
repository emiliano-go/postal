use super::*;

pub(crate) struct MediaEntry {
    pub chat: String,
    pub id: String,
    pub name: Option<String>,
    pub kind: String,
    pub path: String,
    pub timestamp: i64,
    pub quoted: bool,
}

impl MessageStore {
    pub(crate) fn chat_names(&self) -> Result<Vec<(String, Option<String>)>> {
        let conn = self.conn.lock().unwrap();
        let mut statement = conn.prepare("SELECT c.jid, n.name FROM chats c LEFT JOIN names n ON n.jid = c.jid")?;
        let rows = statement.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn media_entries(&self) -> Result<Vec<MediaEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut statement = conn.prepare(
            "SELECT m.chat, m.id, n.name, COALESCE(m.media_once_kind, m.media_kind, 'other'),
                    m.media_path, m.timestamp, 0
             FROM messages m LEFT JOIN names n ON n.jid = m.chat WHERE m.media_path IS NOT NULL
             UNION ALL
             SELECT m.chat, m.id, n.name, COALESCE(m.reply_to_kind, 'other'),
                    m.reply_to_path, m.timestamp, 1
             FROM messages m LEFT JOIN names n ON n.jid = m.chat WHERE m.reply_to_path IS NOT NULL",
        )?;
        let rows = statement.query_map([], |r| Ok(MediaEntry {
            chat: r.get(0)?, id: r.get(1)?, name: r.get(2)?, kind: r.get(3)?,
            path: r.get(4)?, timestamp: r.get(5)?, quoted: r.get(6)?,
        }))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn media_entries_page(&self, after: i64, upper: i64) -> Result<(i64, Vec<MediaEntry>)> {
        let conn = self.conn.lock().unwrap();
        let mut statement = conn.prepare(
            "SELECT m.rowid, m.chat, m.id, n.name, COALESCE(m.media_once_kind, m.media_kind, 'other'),
                    m.media_path, COALESCE(m.reply_to_kind, 'other'), m.reply_to_path, m.timestamp
             FROM messages m LEFT JOIN names n ON n.jid = m.chat
             WHERE m.rowid > ?1 AND m.rowid <= ?2 ORDER BY m.rowid LIMIT 512",
        )?;
        let rows = statement.query_map((after, upper), |row| {
            let rowid: i64 = row.get(0)?;
            let chat: String = row.get(1)?;
            let id: String = row.get(2)?;
            let name: Option<String> = row.get(3)?;
            let media_kind: String = row.get(4)?;
            let reply_kind: String = row.get(6)?;
            let timestamp: i64 = row.get(8)?;
            let media = row.get::<_, Option<String>>(5)?.map(|path| MediaEntry {
                chat: chat.clone(), id: id.clone(), name: name.clone(), kind: media_kind,
                path, timestamp, quoted: false,
            });
            let quote = row.get::<_, Option<String>>(7)?.map(|path| MediaEntry {
                chat, id, name, kind: reply_kind, path, timestamp, quoted: true,
            });
            Ok((rowid, media, quote))
        })?;
        let mut last = after;
        let mut entries = Vec::new();
        for row in rows {
            let (rowid, media, quote) = row?;
            last = rowid;
            entries.extend(media.into_iter().chain(quote));
        }
        Ok((last, entries))
    }

    pub(crate) fn max_message_rowid(&self) -> Result<i64> {
        Ok(self.conn.lock().unwrap().query_row("SELECT COALESCE(MAX(rowid), 0) FROM messages", [], |row| row.get(0))?)
    }

    pub(crate) fn forget_media_file(&self, path: &str) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.savepoint()?;
        tx.execute("UPDATE messages SET media_path = NULL WHERE media_path = ?1", [path])?;
        tx.execute("UPDATE messages SET reply_to_path = NULL WHERE reply_to_path = ?1", [path])?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn database_bytes(&self) -> Result<u64> {
        let conn = self.conn.lock().unwrap();
        let path: String = conn.query_row("SELECT file FROM pragma_database_list WHERE name = 'main'", [], |r| r.get(0))?;
        if path.is_empty() { return Ok(0); }
        let mut bytes = 0;
        for path in [path.clone(), format!("{path}-wal"), format!("{path}-shm")] {
            match std::fs::metadata(path) {
                Ok(metadata) => bytes += metadata.len(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(bytes)
    }
}
