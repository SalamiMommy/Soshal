use crate::Database;
use libsql::params;

const USER_UPSERT_SQL: &str = "\
INSERT INTO users (pubkey, npub, name, display_name, about, picture, banner, nip05, lud16, created_at, updated_at, metadata_json, contact_pubkeys, relay_list, follower_count) \
VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15) \
ON CONFLICT(pubkey) DO UPDATE SET \
  name=excluded.name, display_name=excluded.display_name, about=excluded.about, \
  picture=excluded.picture, banner=excluded.banner, nip05=excluded.nip05, lud16=excluded.lud16, \
  updated_at=excluded.updated_at, metadata_json=excluded.metadata_json, contact_pubkeys=excluded.contact_pubkeys, \
  relay_list=excluded.relay_list";

/// Every column [`UserRepo::map_row`] decodes, in order, with the table alias
/// prefix supplied by the caller (`""` or `"u."`).
///
/// This is a macro rather than a `const` string so all three read projections
/// derive from one list: a projection that omits a column is a silent field
/// shift, not a compile error, and the trailing derived `contact_count` in
/// particular has to be present or `map_row` cannot read it at all.
///
/// `contact_count` is the length of the `contact_pubkeys` JSON array, computed
/// by SQLite instead of by materializing a `String` per contact. The
/// `json_valid` guard is what preserves the old behavior: the Rust side used
/// `serde_json::from_str::<Vec<String>>(..).unwrap_or(0)`, and a malformed
/// column must still yield 0 rather than raising a malformed-JSON error that
/// would fail a whole batch. The JSON functions take a *value*, not a column
/// reference, so the alias is deliberately absent inside the expression.
///
/// What this guards, and what it does not: a projection that *omits* a column
/// or *reorders* one is caught (the FTS test's field assertions fail), because
/// every field after the gap shifts. An *alias collision* is not — libsql
/// permits two columns of the same name and [`UserRepo::map_row`] reads
/// positionally, so aliasing `contact_count` off `follower_count` still decodes
/// correctly today. It would not survive a by-name reader, so uniqueness here is
/// a convention the tests cannot enforce.
macro_rules! user_columns {
    ($p:literal) => {
        concat!(
            $p,
            "pubkey, ",
            $p,
            "npub, ",
            $p,
            "name, ",
            $p,
            "display_name, ",
            $p,
            "about, ",
            $p,
            "picture, ",
            $p,
            "banner, ",
            $p,
            "nip05, ",
            $p,
            "lud16, ",
            $p,
            "created_at, ",
            $p,
            "updated_at, ",
            $p,
            "metadata_json, ",
            $p,
            "contact_pubkeys, ",
            $p,
            "relay_list, ",
            $p,
            "follower_count, ",
            "CASE WHEN json_valid(contact_pubkeys) ",
            "THEN json_array_length(contact_pubkeys) ELSE 0 END AS contact_count"
        )
    };
}

/// The projection [`UserRepo::map_row`] decodes. Shared by the connection-level
/// and transaction-level reads so the two cannot drift on column order — a
/// mismatch here is a silent field shift, not a compile error.
const USER_SELECT_SQL: &str = concat!(
    "SELECT ",
    user_columns!(""),
    " FROM users WHERE pubkey = ?1"
);

/// Batch read behind the profile/contact FFI surfaces. The `IN (SELECT LOWER(…))`
/// subquery is what makes the returned keys already lowercase.
const USER_ROWS_FOR_PUBKEYS_SQL: &str = concat!(
    "SELECT ",
    user_columns!(""),
    " FROM users WHERE pubkey IN (SELECT LOWER(value) FROM json_each(?1))"
);

/// FTS5 name/about search. The join alias prefix is why the projection is a
/// macro parameter rather than a second hand-maintained list.
const USER_SEARCH_SQL: &str = concat!(
    "SELECT ",
    user_columns!("u."),
    " FROM users_fts f JOIN users u ON u.rowid = f.rowid ",
    "WHERE users_fts MATCH ?1 ORDER BY rank LIMIT ?2"
);

pub struct UserRepo<'a> {
    db: &'a Database,
}

impl<'a> UserRepo<'a> {
    soshal_repo_new!();

    pub fn get_by_pubkey(&self, pubkey: &str) -> Result<Option<UserRow>, crate::error::DbError> {
        let norm_pk = pubkey.trim().to_ascii_lowercase();
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            USER_SELECT_SQL,
            params![norm_pk.as_str()],
            Self::map_row,
        )
    }

    /// [`Self::get_by_pubkey`], reading through an open transaction.
    ///
    /// This is the read half of a read-modify-write. A caller that reads on one
    /// connection and writes on another is racing every other writer, so
    /// follow/unfollow use this to keep the read, the list write and the
    /// follower-count update in one transaction.
    pub async fn get_by_pubkey_in(
        &self,
        tx: &libsql::Transaction,
        pubkey: &str,
    ) -> Result<Option<UserRow>, crate::error::DbError> {
        let norm_pk = pubkey.trim().to_ascii_lowercase();
        let stmt = tx.prepare(USER_SELECT_SQL).await?;
        let mut rows = stmt.query(params![norm_pk.as_str()]).await?;
        match rows.next().await? {
            Some(row) => Ok(Some(Self::map_row(&row)?)),
            None => Ok(None),
        }
    }

    /// Which of the given pubkeys have a stored user row. One indexed query
    /// instead of N `get_by_pubkey` calls (mDNS peer drain runs this per
    /// drain tick).
    pub fn existing_pubkeys(
        &self,
        pubkeys: &[String],
    ) -> Result<std::collections::HashSet<String>, crate::error::DbError> {
        if pubkeys.is_empty() {
            return Ok(std::collections::HashSet::new());
        }
        if pubkeys.len() == 1 {
            let conn = self.db.conn()?;
            let norm = pubkeys[0].trim().to_ascii_lowercase();
            let exists: Option<String> = crate::query::query_first(
                &conn,
                "SELECT pubkey FROM users WHERE pubkey = ?1",
                params![norm.as_str()],
                |row| row.get::<String>(0),
            )?;
            let mut set = std::collections::HashSet::with_capacity(exists.is_some() as usize);
            if let Some(pk) = exists {
                set.insert(pk.to_ascii_lowercase());
            }
            return Ok(set);
        }
        let conn = self.db.conn()?;
        let norm_pubkeys: Vec<String> = pubkeys
            .iter()
            .map(|p| p.trim().to_ascii_lowercase())
            .collect();
        let json = serde_json::to_string(&norm_pubkeys).unwrap_or_else(|_| "[]".to_string());
        crate::query::query_fold(
            &conn,
            "SELECT pubkey FROM users WHERE pubkey IN (SELECT LOWER(value) FROM json_each(?1))",
            params![json.as_str()],
            std::collections::HashSet::with_capacity(pubkeys.len()),
            |mut set, row| {
                set.insert(row.get::<String>(0)?.to_ascii_lowercase());
                Ok(set)
            },
        )
    }

    /// Fetch full rows for many pubkeys in one query, keyed by lowercase
    /// pubkey. Missing pubkeys are simply absent from the map.
    ///
    /// Backs the batched profile/contact FFI surfaces: the Dart side used to
    /// call `get_by_pubkey` once per pubkey, and `query_profile_internal` issues
    /// a *second* lookup for the viewer's own row on every single one of those,
    /// so rendering an N-entry grid cost 2N indexed queries plus N JSON
    /// round-trips across FFI.
    pub fn rows_for_pubkeys(
        &self,
        pubkeys: &[String],
    ) -> Result<std::collections::HashMap<String, UserRow>, crate::error::DbError> {
        if pubkeys.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let conn = self.db.conn()?;
        let norm_pubkeys: Vec<String> = pubkeys
            .iter()
            .map(|p| p.trim().to_ascii_lowercase())
            .collect();
        let json = serde_json::to_string(&norm_pubkeys).unwrap_or_else(|_| "[]".to_string());
        crate::query::query_fold(
            &conn,
            USER_ROWS_FOR_PUBKEYS_SQL,
            params![json.as_str()],
            std::collections::HashMap::with_capacity(pubkeys.len()),
            |mut map, row| {
                let mapped = Self::map_row(row)?;
                map.insert(mapped.pubkey.to_ascii_lowercase(), mapped);
                Ok(map)
            },
        )
    }

    pub fn upsert(&self, user: &UserRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::with_tx(&conn, |tx| async move {
            self.upsert_in(&tx, user).await?;
            tx.commit().await?;
            Ok(())
        })?;
        self.db.notify_change(
            crate::change_bus::Table::Profiles,
            Some(user.pubkey.trim().to_ascii_lowercase()),
        );
        Ok(())
    }

    pub async fn upsert_in(
        &self,
        tx: &libsql::Transaction,
        row: &UserRow,
    ) -> Result<(), crate::error::DbError> {
        let norm_pk = row.pubkey.trim().to_ascii_lowercase();
        tx.execute(
            USER_UPSERT_SQL,
            params![
                norm_pk,
                row.npub.as_str(),
                row.name.as_deref(),
                row.display_name.as_deref(),
                row.about.as_deref(),
                row.picture.as_deref(),
                row.banner.as_deref(),
                row.nip05.as_deref(),
                row.lud16.as_deref(),
                row.created_at,
                row.updated_at,
                row.metadata_json.as_deref(),
                row.contact_pubkeys.as_str(),
                row.relay_list.as_str(),
                row.follower_count,
            ],
        )
        .await?;
        Ok(())
    }

    pub fn upsert_batch(&self, users: &[UserRow]) -> Result<(), crate::error::DbError> {
        if users.is_empty() {
            return Ok(());
        }
        let conn = self.db.conn()?;
        crate::query::with_tx(&conn, |tx| async move {
            let stmt = tx.prepare(USER_UPSERT_SQL).await?;
            for user in users {
                let norm_pk = user.pubkey.trim().to_ascii_lowercase();
                stmt.run(params![
                    norm_pk,
                    user.npub.as_str(),
                    user.name.as_deref(),
                    user.display_name.as_deref(),
                    user.about.as_deref(),
                    user.picture.as_deref(),
                    user.banner.as_deref(),
                    user.nip05.as_deref(),
                    user.lud16.as_deref(),
                    user.created_at,
                    user.updated_at,
                    user.metadata_json.as_deref(),
                    user.contact_pubkeys.as_str(),
                    user.relay_list.as_str(),
                    user.follower_count,
                ])
                .await?;
                stmt.reset();
            }
            tx.commit().await?;
            Ok(())
        })?;
        self.db
            .notify_change(crate::change_bus::Table::Profiles, None);
        Ok(())
    }

    pub fn ensure_exists(&self, pubkey: &str) -> Result<(), crate::error::DbError> {
        let norm_pk = pubkey.trim().to_ascii_lowercase();
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT OR IGNORE INTO users (pubkey, npub) VALUES (?1, '')",
            params![norm_pk.as_str()],
        )?;
        Ok(())
    }

    /// Apply many follower-count deltas at once.
    ///
    /// The single-pubkey form is one `UPDATE` per call, and a contact-list
    /// change calls it once per entry — a 5 000-follow list was 5 000
    /// sequential statements inside an already-open transaction. Deltas are
    /// summed per pubkey and grouped by value, so the whole batch becomes one
    /// statement per distinct delta (two in practice: the added set and the
    /// removed set) no matter how large the list is.
    ///
    /// Summing first is what keeps this equivalent to the sequential version:
    /// a pubkey that appears in both the added and the removed set lands in
    /// exactly one group, with a net delta, instead of two statements against
    /// the same row. Grouping second is what bounds the statement count at one
    /// per distinct delta. `MAX(0, …)` is still applied per statement, so a
    /// count that would go negative clamps at 0 exactly as it did before.
    ///
    /// No-op on an empty slice, or when every delta nets to zero.
    pub async fn bump_follower_counts_in(
        &self,
        tx: &libsql::Transaction,
        deltas: &[(&str, i64)],
    ) -> Result<(), crate::error::DbError> {
        // Normalize and sum, so a pubkey touched twice does not become two
        // statements against the same row and a +1/-1 pair nets out here.
        let mut sums: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        for (pubkey, delta) in deltas {
            *sums.entry(pubkey.trim().to_ascii_lowercase()).or_insert(0) += delta;
        }
        let mut by_delta: std::collections::BTreeMap<i64, Vec<String>> =
            std::collections::BTreeMap::new();
        for (pubkey, delta) in sums {
            if delta != 0 {
                by_delta.entry(delta).or_default().push(pubkey);
            }
        }
        for (delta, pubkeys) in by_delta {
            // Numbered placeholders: `?1..?n` for the pubkeys, `?n+1` for the
            // delta, so the bind order is unambiguous and the list stays one
            // pass over `users`. Not `UPDATE ... FROM (VALUES …)`, which
            // would add a 3.33+ SQLite floor for no gain.
            let n = pubkeys.len();
            let in_list = (1..=n)
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "UPDATE users SET follower_count = MAX(0, follower_count + ?{}) \
                 WHERE pubkey IN ({in_list})",
                n + 1
            );
            let mut binds: Vec<libsql::Value> = pubkeys
                .iter()
                .map(|p| libsql::Value::from(p.as_str()))
                .collect();
            binds.push(libsql::Value::from(delta));
            tx.execute(&sql, libsql::params_from_iter(binds)).await?;
        }
        Ok(())
    }

    /// Adjust the materialized `follower_count` for the given pubkeys. Called
    /// when a contact list changes: `+1` for each pubkey newly followed by an
    /// author, `-1` for each unfollowed. The count is maintained incrementally
    /// instead of being recomputed on every upsert (which was O(users) per
    /// contact-list event and stale for everyone except the event author).
    pub async fn bump_follower_count_in(
        &self,
        tx: &libsql::Transaction,
        pubkey: &str,
        delta: i64,
    ) -> Result<(), crate::error::DbError> {
        let norm_pk = pubkey.trim().to_ascii_lowercase();
        tx.execute(
            "UPDATE users SET follower_count = MAX(0, follower_count + ?2) WHERE pubkey = ?1",
            params![norm_pk.as_str(), delta],
        )
        .await?;
        Ok(())
    }

    pub fn bump_follower_count(
        &self,
        pubkey: &str,
        delta: i64,
    ) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::with_tx(&conn, |tx| async move {
            self.bump_follower_count_in(&tx, pubkey, delta).await?;
            tx.commit().await?;
            Ok(())
        })?;
        self.db.notify_change(
            crate::change_bus::Table::Profiles,
            Some(pubkey.trim().to_ascii_lowercase()),
        );
        Ok(())
    }

    /// Re-materialize follower counts for every stored contact list in one
    /// indexed pass. Self-referential entries are excluded, and unknown
    /// pubkeys (followed users with no profile row yet) cause an empty
    /// `INSERT OR IGNORE` sink first so their count can be written.
    pub fn recompute_all_follower_counts(&self) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        // Reset first so removals from previous lists don't leak into the
        // recount.
        crate::query::execute(&conn, "UPDATE users SET follower_count = 0", ())?;
        let rows = crate::query::query(
            &conn,
            "SELECT pubkey, contact_pubkeys FROM users",
            (),
            |row| Ok((row.get::<String>(0)?, row.get::<String>(1)?)),
        )?;
        let mut counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        for (author, list) in rows {
            let author_norm = author.trim().to_ascii_lowercase();
            let entries: Vec<String> = if list.trim().starts_with('[') {
                serde_json::from_str::<Vec<String>>(&list).unwrap_or_default()
            } else {
                list.split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect()
            };
            for pk in entries {
                let pk_norm = pk.trim().to_ascii_lowercase();
                if !pk_norm.is_empty() && pk_norm != author_norm {
                    *counts.entry(pk_norm).or_insert(0) += 1;
                }
            }
        }
        if counts.is_empty() {
            return Ok(());
        }
        crate::query::with_tx(&conn, |tx| async move {
            let mut stmt = tx
                .prepare("INSERT OR IGNORE INTO users (pubkey, npub) VALUES (?1, '')")
                .await?;
            for pk in counts.keys() {
                stmt.run(params![pk.as_str()]).await?;
                stmt.reset();
            }
            stmt = tx
                .prepare("UPDATE users SET follower_count = ?2 WHERE pubkey = ?1")
                .await?;
            for (pk, n) in &counts {
                stmt.run(params![pk.as_str(), *n]).await?;
                stmt.reset();
            }
            tx.commit().await?;
            Ok(())
        })
    }

    pub fn search(&self, query: &str, limit: i64) -> Result<Vec<UserRow>, crate::error::DbError> {
        let fts = Self::fts_prefix_query(query);
        if fts.is_empty() {
            return Ok(Vec::new());
        }
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        crate::query::query(&conn, USER_SEARCH_SQL, params![fts, limit], Self::map_row)
    }

    /// FTS5 MATCH expression with per-term prefix matching. Only
    /// alphanumerics survive, so user input can never smuggle FTS5
    /// operators (`*`, `OR`, quotes, parens) into the parser. Terms are
    /// split on any non-alphanumeric character (mirrors unicode61
    /// tokenization, where `_`/`-` separate tokens) and AND-joined.
    fn fts_prefix_query(query: &str) -> String {
        let mut out = String::with_capacity(query.len().saturating_add(16));
        let mut first = true;
        let mut term = String::with_capacity(16);
        for c in query.chars() {
            if c.is_alphanumeric() {
                term.push(c.to_ascii_lowercase());
                continue;
            }
            if !term.is_empty() {
                if !first {
                    out.push(' ');
                } else {
                    first = false;
                }
                out.push('"');
                out.push_str(&term);
                out.push_str("\"*");
                term.clear();
            }
        }
        if !term.is_empty() {
            if !first {
                out.push(' ');
            }
            out.push('"');
            out.push_str(&term);
            out.push_str("\"*");
        }
        out
    }

    fn map_row(row: &libsql::Row) -> libsql::Result<UserRow> {
        Ok(UserRow {
            pubkey: row.get(0)?,
            npub: row.get(1)?,
            name: row.get(2)?,
            display_name: row.get(3)?,
            about: row.get(4)?,
            picture: row.get(5)?,
            banner: row.get(6)?,
            nip05: row.get(7)?,
            lud16: row.get(8)?,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
            metadata_json: row.get(11)?,
            contact_pubkeys: row.get(12)?,
            relay_list: row.get(13)?,
            follower_count: row.get(14)?,
            contact_count: row.get(15)?,
        })
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserRow {
    pub pubkey: String,
    pub npub: String,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub about: Option<String>,
    pub picture: Option<String>,
    pub banner: Option<String>,
    pub nip05: Option<String>,
    pub lud16: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub metadata_json: Option<String>,
    pub contact_pubkeys: String,
    pub relay_list: String,
    pub follower_count: i64,
    /// Length of the [`Self::contact_pubkeys`] JSON array.
    ///
    /// **Not a column.** The database derives it in the SELECT projection via
    /// `json_array_length` — see [`user_columns!`] — so a caller reporting a
    /// follow count never materializes a `String` per contact. Write paths
    /// leave this 0: it is not in the INSERT list and cannot be persisted.
    pub contact_count: i32,
}
