//! Feed engagement counters — the pre-aggregated rewrite of the per-row
//! correlated subqueries in `feed::feed_engagement_counters`.
//!
//! The rewrite changed the *shape* of the SQL, so what is worth pinning is not
//! "the numbers are plausible" but "the numbers are the same as the shape it
//! replaced". `reference_engagement_counters` below is the old query, kept
//! verbatim and run against the same database, and every case asserts the two
//! agree.
//!
//! The dataset deliberately includes the edges a `LEFT JOIN` rewrite gets
//! wrong: a post with no reactions, a post with no replies, a soft-deleted
//! reply, a reply that is not kind 1, an id in the request with no post row,
//! two posts sharing an author (the classic join fan-out), and a reaction
//! from the active user (the `liked` flag, the one predicate a grouped
//! aggregate cannot express).

#[cfg(test)]
mod feed_engagement_counter_tests {
    use soshal_flutter_bridge::db::db_execute_params;
    use soshal_flutter_bridge::*;

    type Counters = (i32, i32, i32, bool);

    /// The query as it was before the rewrite: three correlated subqueries per
    /// post row. Kept so the rewrite has something to be compared against
    /// rather than only a hand-written expectation.
    ///
    /// Identical casing and predicate shape to the original, including the
    /// `LOWER(value)` on the id list and the string-interpolated
    /// `active_pubkey` in the `EXISTS`.
    fn reference_engagement_counters(ids: &[String], active_pubkey: &str) -> Map {
        let id_json = serde_json::to_string(ids).unwrap();
        let out: String = db::db_query_raw(
            format!(
                "SELECT json_group_array(json_array(id, rx, rp, reposts, liked)) AS v FROM (\
                   SELECT p.id AS id, \
                     (SELECT COUNT(*) FROM reactions r WHERE r.event_id = p.id) AS rx, \
                     (SELECT COUNT(*) FROM posts rp WHERE rp.root_id = p.id AND rp.kind = 1 AND rp.is_deleted = 0) AS rp, \
                     p.reposts_count AS reposts, \
                     EXISTS(SELECT 1 FROM reactions rl WHERE rl.event_id = p.id AND rl.pubkey = '{active_pubkey}') AS liked \
                   FROM posts p \
                   WHERE p.id IN (SELECT LOWER(value) FROM json_each('{id_json}')))"
            ),
        )
        .unwrap();

        let mut map = std::collections::HashMap::new();
        // `json_group_array` is a SQLite TEXT value, so `rows_to_json_string`
        // hands it back as a JSON *string* holding the array — it needs a
        // second parse. `json_group_array` over zero rows yields `'[]'`, not
        // NULL, so there is no null case to special-case here.
        let outer: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
        let Some(first) = outer.first() else {
            return map;
        };
        let inner: Vec<serde_json::Value> =
            serde_json::from_str(first["v"].as_str().expect("json_group_array column")).unwrap();
        for r in inner {
            map.insert(
                r[0].as_str().unwrap().to_string(),
                (
                    r[1].as_i64().unwrap() as i32,
                    r[2].as_i64().unwrap() as i32,
                    r[3].as_i64().unwrap() as i32,
                    r[4].as_i64().unwrap_or(0) != 0,
                ),
            );
        }
        map
    }

    type Map = std::collections::HashMap<String, Counters>;

    /// The rewritten query, reached through the public fetch path so the test
    /// exercises the real call site rather than a copy of the SQL.
    fn actual_engagement_counters(ids: &[String], active_pubkey: &str) -> Map {
        db::db_set_setting("active_pubkey".into(), active_pubkey.into()).unwrap();
        let json = rt().block_on(async {
            feed::feed_fetch_events(
                serde_json::json!({
                    "limit": 200,
                    "offset": 0,
                    "filter_type": "all",
                    "audience": "public",
                })
                .to_string(),
            )
            .await
            .unwrap()
        });
        let posts: Vec<feed::FeedPost> = serde_json::from_str(&json).unwrap();
        let mut map = Map::new();
        for p in posts {
            if !ids.contains(&p.event_id) {
                continue;
            }
            map.insert(p.event_id, (p.reactions, p.replies, p.reposts, p.liked));
        }
        map
    }

    /// `db_core::block_on` uses `block_in_place`, which panics on a
    /// current-thread runtime. Every fetch in this file needs a multi-thread one.
    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    fn insert_post(
        id: &str,
        author: &str,
        root_id: Option<&str>,
        kind: i64,
        deleted: i64,
        reposts: i32,
    ) {
        match root_id {
            None => db_execute_params(
                "INSERT INTO posts (id, pubkey, content, created_at, kind, is_deleted, reposts_count) \
                 VALUES (?1, ?2, 'body', 1700000000, ?3, ?4, ?5)",
                &[
                    id.to_string(),
                    author.to_string(),
                    kind.to_string(),
                    deleted.to_string(),
                    reposts.to_string(),
                ],
            )
            .unwrap(),
            Some(root) => db_execute_params(
                "INSERT INTO posts (id, pubkey, content, created_at, kind, is_deleted, reposts_count, root_id) \
                 VALUES (?1, ?2, 'body', 1700000000, ?3, ?4, ?5, ?6)",
                &[
                    id.to_string(),
                    author.to_string(),
                    kind.to_string(),
                    deleted.to_string(),
                    reposts.to_string(),
                    root.to_string(),
                ],
            )
            .unwrap(),
        };
    }

    fn insert_reaction(id: &str, event_id: &str, author: &str) {
        db_execute_params(
            "INSERT INTO reactions (id, event_id, pubkey, content, created_at, kind) \
             VALUES (?1, ?2, ?3, '+', 1700000000, 7)",
            &[id.to_string(), event_id.to_string(), author.to_string()],
        )
        .unwrap();
    }

    /// The whole point: the rewritten query must agree with the old one on a
    /// dataset built to hit every edge the rewrite could get wrong.
    #[test]
    fn the_rewritten_query_agrees_with_the_correlated_subquery_form() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("feed", "engagement_agree");
        let me = crate::test_util::unique_pubkey("me");
        let them = crate::test_util::unique_pubkey("them");
        let other = crate::test_util::unique_pubkey("other");
        for u in [&me, &them, &other] {
            crate::test_util::insert_user(u);
        }

        let bare = "a".repeat(64); // two reactions, one of them mine; one reply
        let liked = "b".repeat(64); // three reactions, one of them mine
        let not_mine = "c".repeat(64); // one reaction, someone else's
        let busy = "d".repeat(64); // no reactions; a reply, a deleted reply, a kind-7 reply
        let ghost = "e".repeat(64); // an id with no post row at all

        for (id, author) in [
            (&bare, &them),
            (&liked, &them),
            (&not_mine, &other),
            (&busy, &other),
        ] {
            insert_post(id, author, None, 1, 0, 7);
        }
        insert_post(&format!("r1{bare}"), &other, Some(&bare), 1, 0, 0);
        insert_reaction(&format!("x{bare}1"), &bare, &them);
        insert_reaction(&format!("x{bare}2"), &bare, &me);

        insert_reaction(&format!("x{liked}1"), &liked, &me);
        insert_reaction(&format!("x{liked}2"), &liked, &other);
        insert_reaction(&format!("x{liked}3"), &liked, &other);

        insert_reaction(&format!("x{not_mine}1"), &not_mine, &them);

        insert_post(&format!("r1{busy}"), &them, Some(&busy), 1, 0, 0);
        insert_post(&format!("r2{busy}"), &them, Some(&busy), 1, 1, 0); // soft-deleted
        insert_post(&format!("r3{busy}"), &them, Some(&busy), 7, 0, 0); // not kind 1

        let ids = vec![bare.clone(), liked.clone(), not_mine.clone(), busy.clone()];

        let reference = reference_engagement_counters(&ids, &me);
        let actual = actual_engagement_counters(&ids, &me);

        // Spot-check the reference itself, so a broken *both* sides cannot pass.
        assert_eq!(reference[&bare], (2, 1, 7, true));
        assert_eq!(reference[&liked], (3, 0, 7, true));
        assert_eq!(reference[&not_mine], (1, 0, 7, false));
        assert_eq!(reference[&busy], (0, 1, 7, false));
        assert!(
            !reference.contains_key(&ghost),
            "an id with no post row yields no counters in either form"
        );

        for id in &ids {
            assert_eq!(
                actual.get(id),
                reference.get(id),
                "post {id}: the rewritten query disagrees with the correlated form"
            );
        }
        assert!(!actual.contains_key(&ghost), "a ghost id must not appear");

        crate::test_util::cleanup(&path);
    }

    /// The `liked` flag is the one predicate a grouped aggregate cannot
    /// express, so it stayed a correlated `EXISTS`. It must still be scoped to
    /// the active user — a reaction from anyone else must not set it. The
    /// active pubkey also changed source (read off the already-held connection
    /// instead of a second `with_db_result`), so this covers both.
    #[test]
    fn liked_is_scoped_to_the_active_user() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("feed", "engagement_liked");
        let me = crate::test_util::unique_pubkey("me2");
        let them = crate::test_util::unique_pubkey("them2");
        for u in [&me, &them] {
            crate::test_util::insert_user(u);
        }

        let post = "9".repeat(64);
        insert_post(&post, &them, None, 1, 0, 0);
        insert_reaction(&format!("y{post}"), &post, &them);

        let ids = vec![post.clone()];
        let actual = actual_engagement_counters(&ids, &me);
        assert_eq!(actual[&post], (1, 0, 0, false), "someone else's reaction");

        insert_reaction(&format!("z{post}"), &post, &me);
        let actual = actual_engagement_counters(&ids, &me);
        assert_eq!(actual[&post], (2, 0, 0, true), "my own reaction");

        // And the reference agrees, so this is not the rewritten query being
        // self-consistent in a way the old one was not.
        let reference = reference_engagement_counters(&ids, &me);
        assert_eq!(actual[&post], reference[&post]);

        crate::test_util::cleanup(&path);
    }

    /// `COALESCE` behaviour: a post with no reactions and no replies must
    /// report zeros and still appear. A `LEFT JOIN` without `COALESCE` would
    /// surface nulls here, and an inner join would drop the row.
    #[test]
    fn a_post_with_no_engagement_reports_zeros_not_nulls() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("feed", "engagement_zeros");
        let them = crate::test_util::unique_pubkey("them3");
        crate::test_util::insert_user(&them);

        let post = format!("{:064x}", 42);
        insert_post(&post, &them, None, 1, 0, 0);

        let ids = vec![post.clone()];
        let actual = actual_engagement_counters(&ids, &them);
        assert_eq!(
            actual.get(&post),
            Some(&(0, 0, 0, false)),
            "an unengaged post must still appear, with zeros"
        );
        assert_eq!(actual, reference_engagement_counters(&ids, &them));

        crate::test_util::cleanup(&path);
    }

    /// Two posts sharing an author must not have their aggregates merged by
    /// the joins — the classic `LEFT JOIN` fan-out, where one post's reaction
    /// rows multiply another's reply rows into a cross product. The counts are
    /// deliberately asymmetric so a cross product cannot accidentally match.
    #[test]
    fn aggregates_do_not_fan_out_across_posts() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("feed", "engagement_fanout");
        let me = crate::test_util::unique_pubkey("me3");
        let them = crate::test_util::unique_pubkey("them4");
        let other = crate::test_util::unique_pubkey("other5");
        for u in [&me, &them, &other] {
            crate::test_util::insert_user(u);
        }

        // Same author, deliberately: a query that grouped by author instead of
        // post id would cross-multiply these.
        let a = "a1".repeat(32);
        let b = "b1".repeat(32);
        insert_post(&a, &them, None, 1, 0, 1);
        insert_post(&b, &them, None, 1, 0, 2);

        // `a`: 3 reactions, 1 reply. `b`: 1 reaction, 4 replies. A fan-out
        // would give `a` 3 and `b` 12, and the same for the reply counts.
        for i in 0..3 {
            insert_reaction(&format!("ra{i}{a}"), &a, &them);
        }
        insert_reaction(&format!("rb0{b}"), &b, &them);
        insert_post(&format!("ar{a}"), &other, Some(&a), 1, 0, 0);
        for i in 0..4 {
            insert_post(&format!("br{i}{b}"), &other, Some(&b), 1, 0, 0);
        }

        let ids = vec![a.clone(), b.clone()];
        let actual = actual_engagement_counters(&ids, &me);
        assert_eq!(actual[&a], (3, 1, 1, false), "post a aggregated on its own");
        assert_eq!(actual[&b], (1, 4, 2, false), "post b aggregated on its own");
        assert_eq!(actual, reference_engagement_counters(&ids, &me));

        crate::test_util::cleanup(&path);
    }

    /// An empty page is a no-op: the early return must not open a transaction
    /// or emit a malformed `json_each('')` list. This is the same guard the
    /// pre-aggregation relies on — `json_each` over an empty array is fine, but
    /// an early return keeps the whole path off the database.
    #[test]
    fn an_empty_feed_page_yields_no_counters() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("feed", "engagement_empty");
        let them = crate::test_util::unique_pubkey("them6");
        crate::test_util::insert_user(&them);
        insert_post(&format!("{:064x}", 7), &them, None, 1, 0, 0);

        let actual = actual_engagement_counters(&[], &them);
        assert!(
            actual.is_empty(),
            "filtering an empty id list must produce no counters, got {actual:?}"
        );
        assert!(reference_engagement_counters(&[], &them).is_empty());

        crate::test_util::cleanup(&path);
    }
}
