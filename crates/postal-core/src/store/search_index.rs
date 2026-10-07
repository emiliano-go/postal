use super::*;

pub(super) fn table_exists(conn: &Connection) -> Result<bool> {
    Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='message_search')",
        [], |row| row.get(0))?)
}

fn trigger_count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name IN
        ('message_search_ai','message_search_bd','message_search_bu','message_search_au')", [], |row| row.get(0))?)
}

fn supported(conn: &Connection) -> Result<bool> {
    Ok(conn.query_row("SELECT sqlite_compileoption_used('ENABLE_FTS5')", [], |row| row.get(0))?)
}

fn unavailable(error: &rusqlite::Error, tokenizer: &str) -> bool {
    matches!(error, rusqlite::Error::SqliteFailure(code, Some(message))
        if code.extended_code == rusqlite::ffi::SQLITE_ERROR
            && (message == "no such module: fts5" || message == &format!("no such tokenizer: {tokenizer}")))
}

fn usable(conn: &Connection) -> Result<bool> {
    match conn.query_row("SELECT rowid FROM message_search WHERE text LIKE '%abc%' LIMIT 1", [], |row| row.get::<_, i64>(0)).optional() {
        Ok(_) => Ok(true),
        Err(error) if unavailable(&error, "trigram") => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn disable_triggers(conn: &Connection) -> Result<()> {
    conn.execute_batch("DROP TRIGGER IF EXISTS message_search_ai;
        DROP TRIGGER IF EXISTS message_search_bd;
        DROP TRIGGER IF EXISTS message_search_bu;
        DROP TRIGGER IF EXISTS message_search_au;")?;
    Ok(())
}

fn ensure_with(conn: &Connection, enabled: bool, tokenizer: &str, force_rebuild: bool) -> Result<()> {
    if !enabled { disable_triggers(conn)?; return Ok(()); }
    let exists = table_exists(conn)?;
    if exists && !usable(conn)? { disable_triggers(conn)?; return Ok(()); }
    let complete = trigger_count(conn)? == 4;
    if exists && complete && !force_rebuild { return Ok(()); }
    conn.execute_batch("SAVEPOINT message_search_setup")?;
    let setup = (|| -> Result<()> {
        if !exists {
            conn.execute_batch(&format!("CREATE VIRTUAL TABLE message_search USING fts5(text, link_urls,
                content='messages', content_rowid='rowid', tokenize='{tokenizer}', detail='none');"))?;
        }
        conn.execute_batch("CREATE TRIGGER IF NOT EXISTS message_search_ai AFTER INSERT ON messages BEGIN
            INSERT INTO message_search(rowid, text, link_urls) VALUES (new.rowid, new.text, new.link_urls);
        END;
        CREATE TRIGGER IF NOT EXISTS message_search_bd BEFORE DELETE ON messages BEGIN
            INSERT INTO message_search(message_search, rowid, text, link_urls)
                VALUES ('delete', old.rowid, old.text, old.link_urls);
        END;
        CREATE TRIGGER IF NOT EXISTS message_search_bu BEFORE UPDATE OF text, link_urls ON messages BEGIN
            INSERT INTO message_search(message_search, rowid, text, link_urls)
                VALUES ('delete', old.rowid, old.text, old.link_urls);
        END;
        CREATE TRIGGER IF NOT EXISTS message_search_au AFTER UPDATE OF text, link_urls ON messages BEGIN
            INSERT INTO message_search(rowid, text, link_urls) VALUES (new.rowid, new.text, new.link_urls);
        END;
        CREATE INDEX IF NOT EXISTS idx_messages_unicode_text ON messages(chat)
            WHERE length(text) <> length(CAST(text AS BLOB));
        INSERT INTO message_search(message_search) VALUES ('rebuild');")?;
        Ok(())
    })();
    if let Err(error) = setup {
        conn.execute_batch("ROLLBACK TO message_search_setup; RELEASE message_search_setup")?;
        if error.downcast_ref::<rusqlite::Error>().is_some_and(|error| unavailable(error, tokenizer)) {
            disable_triggers(conn)?;
            return Ok(());
        }
        return Err(error);
    }
    conn.execute_batch("RELEASE message_search_setup")?;
    Ok(())
}

pub(super) fn ensure(conn: &Connection) -> Result<()> { ensure_with(conn, supported(conn)?, "trigram", false) }

pub(super) fn on_open(conn: &Connection, vacuumed: bool) -> Result<()> {
    ensure_with(conn, supported(conn)?, "trigram", vacuumed)
}

pub(super) fn available(conn: &Connection) -> Result<bool> {
    Ok(supported(conn)? && table_exists(conn)? && trigger_count(conn)? == 4 && usable(conn)?)
}

pub(super) fn candidate(query: &str) -> Option<String> {
    let mut best = "";
    let mut start = 0;
    for (at, ch) in query.char_indices() {
        if !ch.is_ascii_alphanumeric() {
            if at - start > best.len() { best = &query[start..at]; }
            start = at + ch.len_utf8();
        }
    }
    if query.len() - start > best.len() { best = &query[start..]; }
    (best.len() >= 3).then(|| {
        let literal = query.bytes().all(|byte| byte.is_ascii_graphic() || byte == b' ')
            && !query.bytes().any(|byte| matches!(byte, b'%' | b'_' | b'\\'));
        format!("%{}%", if literal { query.to_ascii_lowercase() } else { best.to_ascii_lowercase() })
    })
}

pub(super) fn clause(conn: &Connection, query: &str, parameter: usize, links: bool) -> Result<(String, Option<String>)> {
    let pattern = if available(conn)? { candidate(query) } else { None };
    let sql = if pattern.is_some() {
        let link = if links { format!(" UNION SELECT rowid FROM message_search WHERE link_urls LIKE ?{parameter}") } else { String::new() };
        format!(" AND m.rowid IN (SELECT rowid FROM message_search WHERE text LIKE ?{parameter}{link})")
    } else {
        format!(" AND ?{parameter} IS NULL")
    };
    Ok((sql, pattern))
}

pub(super) fn keyword_clause(conn: &Connection, terms: &[String], first: usize) -> Result<(String, Vec<String>)> {
    if !available(conn)? { return Ok((String::new(), Vec::new())); }
    let Some(patterns) = terms.iter().map(|term| candidate(term)).collect::<Option<Vec<_>>>() else {
        return Ok((String::new(), Vec::new()));
    };
    let mut selects = patterns.iter().enumerate().map(|(at, _)|
        format!("SELECT rowid FROM message_search WHERE text LIKE ?{}", first + at)).collect::<Vec<_>>();
    selects.push("SELECT rowid FROM messages INDEXED BY idx_messages_unicode_text
        WHERE length(text) <> length(CAST(text AS BLOB))".into());
    Ok((format!(" AND m.rowid IN ({})", selects.join(" UNION ")), patterns))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRE_FTS_SCHEMA: usize = 42;

    #[test]
    fn full_vacuum_before_open_rebuilds_index_for_message_rowids() {
        let root = std::env::temp_dir().join(format!("postal-fts-vacuum-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("messages.db");
        let conn = Connection::open(&path).unwrap();
        schema::migrate(&conn).unwrap();
        for (rowid, id, text) in [(100, "a", "vacuum first"), (200, "b", "vacuum second")] {
            conn.execute("INSERT INTO messages(rowid,chat,id,sender,timestamp,from_me,text)
                VALUES (?1,'a@s',?2,'peer@s',?1,0,?3)", params![rowid, id, text]).unwrap();
        }
        assert_eq!(conn.pragma_query_value(None, "auto_vacuum", |row| row.get::<_, i64>(0)).unwrap(), 0);
        drop(conn);
        let store = MessageStore::open(&path).unwrap();
        assert_eq!(store.search_messages("a@s", "vacuum second", 10).unwrap()[0].header.id, "b");
        let conn = store.conn.lock().unwrap();
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM messages m JOIN message_search f ON f.rowid=m.rowid
            WHERE f.text LIKE '%vacuum%'", [], |row| row.get::<_, i64>(0)).unwrap(), 2);
        drop(conn);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn index_follows_text_link_edits_revoke_delete_and_physical_removal() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        let conn = store.conn.lock().unwrap();
        conn.execute("INSERT INTO messages(chat,id,sender,timestamp,from_me,text,link_urls)
            VALUES ('a@s','one','peer@s',1,0,'before needle','')", []).unwrap();
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM message_search WHERE text LIKE '%needle%'", [],
            |row| row.get::<_, i64>(0)).unwrap(), 1);
        conn.execute("UPDATE messages SET text='after edit', link_urls='[\"https://link.needle.test\"]'
            WHERE chat='a@s' AND id='one'", []).unwrap();
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM message_search WHERE text LIKE '%needle%'", [],
            |row| row.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM message_search WHERE link_urls LIKE '%needle%'", [],
            |row| row.get::<_, i64>(0)).unwrap(), 1);
        drop(conn);
        assert_eq!(store.search_messages("a@s", "needle", 10).unwrap()[0].header.id, "one");
        store.conn.lock().unwrap().execute("UPDATE messages SET revoked=1 WHERE id='one'", []).unwrap();
        assert!(store.switcher_messages("after edit", 10).unwrap().is_empty());
        store.conn.lock().unwrap().execute("UPDATE messages SET deleted=1 WHERE id='one'", []).unwrap();
        assert!(store.search_messages("a@s", "needle", 10).unwrap().is_empty());
        let conn = store.conn.lock().unwrap();
        conn.execute("DELETE FROM messages WHERE id='one'", []).unwrap();
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM message_search WHERE link_urls LIKE '%needle%'", [],
            |row| row.get::<_, i64>(0)).unwrap(), 0);
    }

    #[test]
    fn hundred_thousand_row_trigram_and_keyword_benchmark() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        store.conn.lock().unwrap().execute_batch("WITH RECURSIVE fixture(n) AS
            (SELECT 0 UNION ALL SELECT n + 1 FROM fixture WHERE n < 99999)
            INSERT INTO messages(chat,id,sender,timestamp,from_me,text)
            SELECT 'a@s',CAST(n AS TEXT),'peer@s',n,0,
                CASE WHEN n=99999 THEN 'rarecanary'
                     WHEN n%100=0 THEN 'KOO ordinary message'
                     ELSE 'ordinary message' END FROM fixture;").unwrap();
        let conn = store.conn.lock().unwrap();
        let plan: String = conn.query_row("EXPLAIN QUERY PLAN SELECT rowid FROM message_search
            WHERE text LIKE '%rarecanary%'", [], |row| row.get(3)).unwrap();
        assert!(plan.contains("VIRTUAL TABLE INDEX"), "{plan}");
        let index_bytes: i64 = conn.query_row("SELECT COALESCE(SUM(pgsize),0) FROM dbstat
            WHERE name LIKE 'message_search%'", [], |row| row.get(0)).unwrap();
        drop(conn);
        let measure = |mut query: Box<dyn FnMut()>| {
            let mut samples = (0..3).map(|_| {
            let started = std::time::Instant::now(); query(); started.elapsed()
            }).collect::<Vec<_>>();
            samples.sort(); samples[1]
        };
        let indexed = measure(Box::new(|| assert_eq!(store.search_messages("a@s", "rarecanary", 50).unwrap().len(), 1)));
        let baseline = measure(Box::new(|| {
            let conn = store.conn.lock().unwrap();
            assert_eq!(conn.query_row("SELECT COUNT(*) FROM messages WHERE chat='a@s'
                AND lower(text) LIKE '%rarecanary%'", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
        }));
        let keyword_indexed = measure(Box::new(|| assert_eq!(store.keyword_mentions(&["rarecanary".into()], &[]).unwrap()["a@s"], 1)));
        ensure_with(&store.conn.lock().unwrap(), false, "trigram", false).unwrap();
        let keyword_scan = measure(Box::new(|| assert_eq!(store.keyword_mentions(&["rarecanary".into()], &[]).unwrap()["a@s"], 1)));
        eprintln!("100k rows, 1000 Unicode, one search hit; trigram={index_bytes} bytes; median search indexed={indexed:?} scan={baseline:?}; keyword indexed={keyword_indexed:?} scan={keyword_scan:?}");
        store.conn.lock().unwrap().execute("UPDATE messages SET text='KOO ordinary message'
            WHERE CAST(id AS INTEGER)%4=0", []).unwrap();
        ensure_with(&store.conn.lock().unwrap(), true, "trigram", false).unwrap();
        let keyword_quarter_indexed = measure(Box::new(|| assert_eq!(store.keyword_mentions(&["rarecanary".into()], &[]).unwrap()["a@s"], 1)));
        ensure_with(&store.conn.lock().unwrap(), false, "trigram", false).unwrap();
        let keyword_quarter_scan = measure(Box::new(|| assert_eq!(store.keyword_mentions(&["rarecanary".into()], &[]).unwrap()["a@s"], 1)));
        eprintln!("100k rows, 25000 Unicode; median keyword indexed={keyword_quarter_indexed:?} scan={keyword_quarter_scan:?}");
    }

    #[test]
    fn old_database_backfills_and_encrypted_reopen_keeps_index_usable() {
        let root = std::env::temp_dir().join(format!("postal-fts-migrate-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("messages.db");
        let conn = Connection::open(&path).unwrap();
        schema::migrate_to(&conn, PRE_FTS_SCHEMA).unwrap();
        conn.execute("INSERT INTO messages(rowid,chat,id,sender,timestamp,from_me,text)
            VALUES (100,'a@s','old','peer@s',1,0,'historical needle')", []).unwrap();
        conn.execute("INSERT INTO messages(rowid,chat,id,sender,timestamp,from_me,text)
            VALUES (200,'a@s','other','peer@s',2,0,'other content')", []).unwrap();
        drop(conn);
        let store = MessageStore::open(&path).unwrap();
        assert!(available(&store.conn.lock().unwrap()).unwrap());
        assert_eq!(store.search_messages("a@s", "needle", 10).unwrap()[0].header.id, "old");
        drop(store);
        let key = crate::database_crypto::DatabaseKey::from_bytes([0x61; 32]);
        crate::database_crypto::prepare_database(&path, &key).unwrap();
        let store = MessageStore::open_with_key(&path, Some(&key)).unwrap();
        let conn = store.conn.lock().unwrap();
        assert!(available(&conn).unwrap());
        let (message_id, indexed_id): (i64, i64) = conn.query_row("SELECT m.rowid, f.rowid
            FROM messages m JOIN message_search f ON f.rowid=m.rowid WHERE f.text LIKE '%needle%'",
            [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(message_id, indexed_id);
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM messages m JOIN message_search f ON f.rowid=m.rowid
            WHERE f.text LIKE '%content%'", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
        drop(conn);
        assert_eq!(store.search_messages("a@s", "needle", 10).unwrap()[0].header.id, "old");
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unsupported_setup_rolls_back_and_later_capability_rebuilds() {
        let conn = Connection::open_in_memory().unwrap();
        schema::migrate_to(&conn, PRE_FTS_SCHEMA).unwrap();
        ensure_with(&conn, true, "missing_tokenizer", false).unwrap();
        assert!(!table_exists(&conn).unwrap());
        assert_eq!(trigger_count(&conn).unwrap(), 0);
        conn.execute("INSERT INTO messages(chat,id,sender,timestamp,from_me,text)
            VALUES ('a@s','old','peer@s',1,0,'indexed later')", []).unwrap();
        ensure_with(&conn, false, "trigram", false).unwrap();
        ensure_with(&conn, true, "trigram", false).unwrap();
        assert!(available(&conn).unwrap());
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM message_search WHERE text LIKE '%indexed%'", [],
            |row| row.get::<_, i64>(0)).unwrap(), 1);
        ensure_with(&conn, false, "trigram", false).unwrap();
        assert!(!available(&conn).unwrap());
        conn.execute("INSERT INTO messages(chat,id,sender,timestamp,from_me,text)
            VALUES ('a@s','new','peer@s',2,0,'later write')", []).unwrap();
        ensure_with(&conn, true, "trigram", false).unwrap();
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM message_search WHERE text LIKE '%later%'", [],
            |row| row.get::<_, i64>(0)).unwrap(), 2);
    }

    #[test]
    fn disabled_index_keeps_live_message_writes_and_search_fallback() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        ensure_with(&store.conn.lock().unwrap(), false, "trigram", false).unwrap();
        let mut message = StoredMessage::default();
        message.header.chat = "a@s".into();
        message.header.id = "live".into();
        message.text = "fallback needle".into();
        store.insert_message(&message).unwrap();
        assert_eq!(store.search_messages("a@s", "needle", 10).unwrap()[0].header.id, "live");
        ensure_with(&store.conn.lock().unwrap(), true, "trigram", false).unwrap();
        assert!(available(&store.conn.lock().unwrap()).unwrap());
        assert_eq!(store.search_messages("a@s", "needle", 10).unwrap()[0].header.id, "live");
    }

    #[test]
    fn unrelated_index_setup_error_is_not_suppressed() {
        let conn = Connection::open_in_memory().unwrap();
        schema::migrate_to(&conn, PRE_FTS_SCHEMA).unwrap();
        conn.execute_batch("CREATE VIEW message_search AS SELECT 1").unwrap();
        let error = ensure_with(&conn, true, "trigram", false).unwrap_err().to_string();
        assert!(error.contains("already exists"), "{error}");
        assert_eq!(trigger_count(&conn).unwrap(), 0);
        assert!(!table_exists(&conn).unwrap());
    }

    #[test]
    fn trigram_candidates_match_scan_for_literals_unicode_and_keyword_folding() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        let conn = store.conn.lock().unwrap();
        assert!(available(&conn).unwrap());
        assert_eq!(conn.query_row("SELECT sqlite_compileoption_used('ENABLE_FTS5')", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
        for (id, body, links) in [
            ("spanish", "Mañana canción", ""), ("arabic", "مرحبا بالعالم", ""),
            ("kelvin", "KOO", ""), ("dotted", "İstanbul", ""), ("greek", "Σύνολο", ""),
            ("literal", "100%_\\ complete", ""), ("link", "", "[\"https://only.synthetic.test\"]"),
            ("nul", "a\0NEEDLE", ""),
            ("phrase", "Message number 49999", ""),
        ] {
            conn.execute("INSERT INTO messages(chat,id,sender,timestamp,from_me,text,link_urls)
                VALUES ('a@s',?1,'peer@s',1,0,?2,?3)", params![id, body, links]).unwrap();
        }
        drop(conn);
        let queries = ["mañana", "Mañana canción", "canción", "ana", "مرحبا", "KOO", "İstanbul", "Σύ",
            "100%_\\", "100%_\\ complete", "only.synthetic", "ONLY.SYNTHETIC", "NEEDLE", "number 49999", "ne", "%", "_"];
        let indexed = queries.iter().map(|query| store.search_messages("a@s", query, 50).unwrap()
            .into_iter().map(|message| message.header.id).collect::<Vec<_>>()).collect::<Vec<_>>();
        let highlights = [vec!["koo".into()], vec!["needle".into()], vec!["mañana".into()]];
        let counts = highlights.iter().map(|terms| store.keyword_mentions(terms, &[]).unwrap()).collect::<Vec<_>>();
        let hits = highlights.iter().map(|terms| store.keyword_matches(None, false, terms, &[]).unwrap()
            .into_iter().map(|message| message.header.id).collect::<Vec<_>>()).collect::<Vec<_>>();
        store.conn.lock().unwrap().execute_batch("DROP TABLE message_search").unwrap();
        assert!(!available(&store.conn.lock().unwrap()).unwrap());
        for (query, expected) in queries.iter().zip(indexed) {
            let actual = store.search_messages("a@s", query, 50).unwrap().into_iter()
                .map(|message| message.header.id).collect::<Vec<_>>();
            assert_eq!(actual, expected, "query: {query}");
        }
        for ((terms, count), hit) in highlights.iter().zip(counts).zip(hits) {
            assert_eq!(store.keyword_mentions(terms, &[]).unwrap(), count, "terms: {terms:?}");
            assert_eq!(store.keyword_matches(None, false, terms, &[]).unwrap().into_iter()
                .map(|message| message.header.id).collect::<Vec<_>>(), hit);
        }
    }

    #[test]
    fn ascii_phrase_candidate_narrows_common_word_before_final_match() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        let conn = store.conn.lock().unwrap();
        conn.execute_batch("WITH RECURSIVE fixture(n) AS (SELECT 0 UNION ALL SELECT n+1 FROM fixture WHERE n<999)
            INSERT INTO messages(chat,id,sender,timestamp,from_me,text)
            SELECT 'a@s',CAST(n AS TEXT),'peer@s',n,0,'message number '||n FROM fixture;").unwrap();
        assert!(available(&conn).unwrap());
        let candidate = candidate("number 999").unwrap();
        assert_eq!(candidate, "%number 999%");
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM message_search WHERE text LIKE ?1", [&candidate], |row| row.get(0)).unwrap();
        assert_eq!(count, 1);
        let broad: i64 = conn.query_row("SELECT COUNT(*) FROM message_search WHERE text LIKE '%number%'", [], |row| row.get(0)).unwrap();
        assert_eq!(broad, 1000);
        drop(conn);
        assert_eq!(store.search_messages("a@s", "number 999", 50).unwrap()[0].header.id, "999");
    }
}
