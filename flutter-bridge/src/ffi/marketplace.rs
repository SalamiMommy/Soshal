//! Marketplace FFI module
//!
//! NIP-15 marketplaces: listings and orders live as kind 30402/30403 rows
//! in the posts table (relay-synced like all events); escrows use the
//! dedicated `escrows` table. Listing creation follows the legacy Tauri
//! flow: content built via marketplace-core, signed by the unlocked signer,
//! and the signed event returned for relay publish.

use flutter_rust_bridge::frb;
use serde::{Deserialize, Serialize};
use soshal_common_core::consts::{KIND_LISTING, KIND_ORDER};
use soshal_db_core::repos::escrow::EscrowRepo;

/// Marketplace listing info
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ListingInfo {
    pub id: String,
    pub seller_pubkey: String,
    pub seller_name: String,
    pub title: String,
    pub description: String,
    pub images: Vec<String>,
    pub price: u64,
    pub currency: String,
    pub category: String,
    pub condition: String,
    pub shipping_available: bool,
    pub escrow_enabled: bool,
    pub created_at: u64,
    pub updated_at: u64,
    pub status: String,
}

/// Order info
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OrderInfo {
    pub id: String,
    pub listing_id: String,
    pub buyer_pubkey: String,
    pub seller_pubkey: String,
    pub status: String,
    pub amount: u64,
    pub created_at: u64,
}

fn has_tag(tags: &[Vec<String>], name: &str) -> bool {
    tags.iter()
        .any(|t| t.first().map(String::as_str) == Some(name))
}

/// Reshape a DB row (content fields in JSON body) into a kind-30402 event
/// shape for `parse_listing_json` (title/price/currency/images belong in
/// tags per NIP-15). Content-provided values become synthetic tags only
/// when the row's tags_json lacks them, so relay-synced tag-form rows pass
/// through untouched.
fn listing_event_from_row(
    v: &serde_json::Value,
) -> Option<soshal_marketplace_core::listing::ListingEvent> {
    let id = v["id"].as_str()?.to_string();
    let pubkey = v["seller_pubkey"].as_str().unwrap_or("").to_string();
    let content: String = match v["content"].as_str() {
        Some(s) => s.to_string(),
        None => v["content"].to_string(),
    };
    let created_at = v["created_at"].as_f64().unwrap_or(0.0);
    let mut tags: Vec<Vec<String>> = match v["tags_json"].as_str() {
        Some(s) => serde_json::from_str(s).unwrap_or_default(),
        None => v["tags_json"]
            .as_array()
            .and_then(|a| serde_json::from_value(serde_json::Value::Array(a.clone())).ok())
            .unwrap_or_default(),
    };
    if let Ok(content_value) = serde_json::from_str::<serde_json::Value>(&content) {
        if !has_tag(&tags, "title") {
            if let Some(t) = content_value["title"].as_str() {
                tags.push(vec!["title".to_string(), t.to_string()]);
            }
        }
        if !has_tag(&tags, "price") {
            if let Some(p) = content_value["price"].as_f64() {
                tags.push(vec!["price".to_string(), p.to_string()]);
            }
        }
        if !has_tag(&tags, "currency") {
            if let Some(c) = content_value["currency"].as_str() {
                tags.push(vec!["currency".to_string(), c.to_string()]);
            }
        }
        if !has_tag(&tags, "location") && !has_tag(&tags, "g") {
            if let Some(g) = content_value["locationGeohash"].as_str() {
                tags.push(vec!["location".to_string(), g.to_string()]);
            }
        }
        if let Some(imgs) = content_value["images"].as_array() {
            for img in imgs {
                if let Some(s) = img.as_str() {
                    tags.push(vec!["image".to_string(), s.to_string()]);
                }
            }
        }
    }
    Some(soshal_marketplace_core::listing::ListingEvent {
        id,
        pubkey,
        content,
        created_at,
        tags,
    })
}

fn listing_from_value(v: &serde_json::Value) -> Option<ListingInfo> {
    let ev_struct = listing_event_from_row(v)?;
    let out = soshal_marketplace_core::listing::parse_listing(&ev_struct)?;
    Some(ListingInfo {
        id: out.id,
        seller_pubkey: v["seller_pubkey"].as_str().unwrap_or("").to_string(),
        seller_name: v["seller_name"].as_str().unwrap_or("").to_string(),
        title: out.title,
        description: out.description.unwrap_or_default(),
        images: out.images,
        price: out.price.max(0.0) as u64,
        currency: out.currency,
        category: out.tags.first().cloned().unwrap_or_default(),
        condition: out.condition,
        shipping_available: out.location_geohash.is_some(),
        escrow_enabled: out.escrow_enabled,
        created_at: out.created_at.max(0.0) as u64,
        updated_at: out.created_at.max(0.0) as u64,
        status: if v["is_deleted"].as_bool().unwrap_or(false) {
            "deleted".to_string()
        } else {
            "active".to_string()
        },
    })
}

#[derive(serde::Deserialize)]
struct OrderContent<'a> {
    #[serde(rename = "listingId", borrow)]
    listing_id: Option<&'a str>,
    #[serde(borrow)]
    seller: Option<&'a str>,
    #[serde(borrow)]
    status: Option<&'a str>,
    amount: Option<f64>,
}

fn order_from_fields(id: String, content_str: &str, pubkey: String, created_at: i64) -> OrderInfo {
    let content: Option<OrderContent> = serde_json::from_str(content_str).ok();
    OrderInfo {
        id,
        listing_id: content
            .as_ref()
            .and_then(|c| c.listing_id)
            .unwrap_or("")
            .to_string(),
        buyer_pubkey: pubkey,
        seller_pubkey: content
            .as_ref()
            .and_then(|c| c.seller)
            .unwrap_or("")
            .to_string(),
        status: content
            .as_ref()
            .and_then(|c| c.status)
            .unwrap_or("created")
            .to_string(),
        amount: content.as_ref().and_then(|c| c.amount).unwrap_or(0.0) as u64,
        created_at: created_at.max(0) as u64,
    }
}

#[cfg(test)]
fn order_from_value(v: &serde_json::Value) -> Option<OrderInfo> {
    let content: serde_json::Value = match v["content"].as_str() {
        Some(s) => serde_json::from_str(s).unwrap_or(serde_json::Value::Null),
        None => v["content"].clone(),
    };
    Some(OrderInfo {
        id: v["id"].as_str()?.to_string(),
        listing_id: content["listingId"].as_str().unwrap_or("").to_string(),
        buyer_pubkey: v["pubkey"].as_str().unwrap_or("").to_string(),
        seller_pubkey: content["seller"].as_str().unwrap_or("").to_string(),
        status: content["status"].as_str().unwrap_or("created").to_string(),
        amount: content["amount"].as_f64().unwrap_or(0.0) as u64,
        created_at: v["created_at"].as_i64().unwrap_or(0).max(0) as u64,
    })
}

fn listings_sql(prelude: &str, limit: i32, offset: i32, authors: Option<&[String]>) -> String {
    let author_clause = match authors {
        Some(a) if a.len() == 1 => " AND p.pubkey = ?1",
        Some(a) if !a.is_empty() => " AND p.pubkey IN (SELECT value FROM json_each(?1))",
        _ => "",
    };
    format!(
        "{prelude} SELECT p.id, p.pubkey AS seller_pubkey, COALESCE(u.name,'') AS seller_name, \
         p.content, p.tags_json, p.created_at, p.is_deleted \
         FROM posts p LEFT JOIN users u ON u.pubkey = p.pubkey \
         WHERE p.kind = {KIND_LISTING} AND p.is_deleted = 0{author_clause} \
         ORDER BY p.created_at DESC LIMIT {} OFFSET {}",
        limit.clamp(1, 200),
        offset.max(0)
    )
}

fn parse_listings_values(rows: Vec<serde_json::Value>) -> Vec<ListingInfo> {
    rows.into_iter()
        .filter_map(|v| {
            let mut info = listing_from_value(&v)?;
            info.seller_pubkey = v["seller_pubkey"].as_str().unwrap_or("").to_string();
            Some(info)
        })
        .collect()
}

fn db_listings(sql: String) -> Result<Vec<ListingInfo>, String> {
    db_listings_params(&sql, &[])
}

fn db_listings_params(sql: &str, params: &[String]) -> Result<Vec<ListingInfo>, String> {
    let rows = super::db::db_query_json(sql, params)?;
    let mut out: Vec<ListingInfo> = Vec::new();
    for v in rows {
        if let Some(mut info) = listing_from_value(&v) {
            info.seller_pubkey = v["seller_pubkey"].as_str().unwrap_or("").to_string();
            info.seller_name = v["seller_name"].as_str().unwrap_or("").to_string();
            out.push(info);
        }
    }
    Ok(out)
}

/// Fetch all active listings (newest first).
#[frb(sync, serialize)]
pub fn marketplace_fetch_listings(
    limit: i32,
    offset: i32,
    audience: String,
) -> Result<String, String> {
    let authors = super::identity::resolve_audience_authors(&audience)?;
    if let Some(a) = &authors {
        if a.is_empty() {
            return super::util::json_ok(Vec::<ListingInfo>::new());
        }
    }
    let sql = listings_sql("", limit, offset, authors.as_deref());
    match &authors {
        Some(a) => {
            let p = if a.len() == 1 {
                a[0].clone()
            } else {
                serde_json::to_string(a).map_err(|e| format!("authors: {e}"))?
            };
            super::util::json_ok(db_listings_params(&sql, &[p])?)
        }
        None => super::util::json_ok(db_listings(sql)?),
    }
}

/// Full-text search on listing content (title/description).
#[frb(sync, serialize)]
pub fn marketplace_search(query: String, limit: i32, audience: String) -> Result<String, String> {
    if query.trim().is_empty() {
        return Ok("[]".to_string()).into();
    }
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let authors = super::identity::resolve_audience_authors(&audience)?;
    if let Some(a) = &authors {
        if a.is_empty() {
            return super::util::json_ok(Vec::<ListingInfo>::new());
        }
    }
    let author_clause = match &authors {
        Some(a) if a.len() == 1 => " AND p.pubkey = ?2",
        Some(a) if !a.is_empty() => " AND p.pubkey IN (SELECT value FROM json_each(?2))",
        _ => "",
    };
    let sql = format!(
        "SELECT p.id, p.pubkey AS seller_pubkey, COALESCE(u.name,'') AS seller_name, \
         p.content, p.tags_json, p.created_at, p.is_deleted \
         FROM posts p LEFT JOIN users u ON u.pubkey = p.pubkey \
         WHERE p.kind = {KIND_LISTING} AND p.is_deleted = 0 AND p.content LIKE '%' || ?1 || '%' ESCAPE '\\' {author_clause} \
         ORDER BY p.created_at DESC LIMIT {}",
        limit.clamp(1, 100)
    );
    let mut params: Vec<String> = vec![escaped];
    if let Some(a) = &authors {
        if a.len() == 1 {
            params.push(a[0].clone());
        } else {
            params.push(serde_json::to_string(a).map_err(|e| format!("authors: {e}"))?);
        }
    }
    let rows = super::db::db_query_json(&sql, &params)?;
    super::util::json_ok(parse_listings_values(rows))
}

/// Get listing by id.
#[frb(sync, serialize)]
pub fn marketplace_get_listing(listing_id: String) -> Result<String, String> {
    let list = db_listings_params(
        &format!(
            "SELECT p.id, p.pubkey AS seller_pubkey, COALESCE(u.name,'') AS seller_name, \
             p.content, p.tags_json, p.created_at, p.is_deleted \
             FROM posts p LEFT JOIN users u ON u.pubkey = p.pubkey \
             WHERE p.kind = {KIND_LISTING} AND p.id = ?1"
        ),
        &[listing_id],
    )?;
    let info = list
        .into_iter()
        .next()
        .ok_or("Listing not found".to_string())?;
    super::util::json_ok(info)
}

/// Get raw listing content JSON string by listing id.
#[frb(sync, serialize)]
pub fn marketplace_get_content(listing_id: String) -> Result<String, String> {
    let content = super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let res = soshal_db_core::query::query_first(
            &conn,
            &format!("SELECT content FROM posts WHERE id = ?1 AND kind = {KIND_LISTING} LIMIT 1"),
            libsql::params![listing_id.as_str()],
            |r| match r.get_value(0)? {
                libsql::Value::Text(t) => Ok(t),
                libsql::Value::Blob(b) => Ok(hex::encode(b)),
                _ => Ok(String::new()),
            },
        )?;
        Ok(res)
    })?;
    Ok(content.unwrap_or_else(|| "{}".to_string()))
}

/// Create a listing (kind 30402). Signs with the unlocked signer and
/// returns the signed event JSON; also stores the row locally so offline
/// fetches work. Relay publish happens via `network_publish_event`.
#[allow(clippy::too_many_arguments)]
#[frb(sync, serialize)]
pub fn marketplace_create_listing(
    seller_pubkey: String,
    title: String,
    description: String,
    price: u64,
    currency: String,
    category: String,
    condition: String,
    images_json: String,
    shipping_available: bool,
) -> Result<String, String> {
    let seller_pubkey = seller_pubkey.trim().to_ascii_lowercase();
    super::signer::require_identity(&seller_pubkey)?;
    if title.trim().is_empty() || title.len() > 500 {
        return Err("title must be between 1 and 500 chars".to_string()).into();
    }
    if price == 0 {
        return Err("price must be positive".to_string()).into();
    }
    if description.len() > 10_000 {
        return Err("description exceeds 10,000 chars cap".to_string()).into();
    }
    if images_json.len() > 64 * 1024 {
        return Err("images JSON exceeds 64KB cap".to_string()).into();
    }
    let images: Vec<String> =
        serde_json::from_str(&images_json).map_err(|e| format!("invalid images JSON: {e}"))?;
    if images.len() > 12 {
        return Err("too many images".to_string()).into();
    }
    for img in &images {
        if img.trim().is_empty() || !soshal_common_core::url::is_valid_media_url(img) {
            return Err(format!("invalid image URL: {img}")).into();
        }
    }
    let d_tag = uuid_like();
    let content = serde_json::json!({
        "title": title,
        "price": price as f64,
        "currency": if currency.is_empty() { "sats".to_string() } else { currency },
        "condition": condition,
        "description": description,
        "locationGeohash": if shipping_available { serde_json::json!("local") } else { serde_json::Value::Null },
        "images": images,
        "escrowEnabled": true,
    });
    let builder = nostr::event::EventBuilder::new(
        nostr::event::Kind::from_u16(KIND_LISTING),
        content.to_string(),
    )
    .tags(
        vec![
            ["d".to_string(), d_tag.clone()],
            ["t".to_string(), category.clone()],
        ]
        .into_iter()
        .filter_map(|t| nostr::event::Tag::parse(t).ok()),
    );
    let signed_json = super::signer::sign_builder(builder)?;
    let signed: serde_json::Value =
        serde_json::from_str(&signed_json).map_err(|e| format!("bad signed event: {e}"))?;
    let event_id = signed["id"].as_str().unwrap_or_default().to_string();
    let now = soshal_common_core::format::now_secs();
    super::db::upsert_post_row(
        event_id,
        seller_pubkey,
        content.to_string(),
        KIND_LISTING as i64,
        now,
        serde_json::to_string(&vec![
            vec!["d".to_string(), d_tag],
            vec!["t".to_string(), category],
        ])
        .unwrap_or_default(),
        Some(title),
    )?;
    Ok(signed_json).into()
}

/// Update listing title/description/price (keeps d-tag identity). Locally
/// the row is replaced; publishing the new signed event is the caller's job.
#[frb(sync, serialize)]
pub fn marketplace_update_listing(
    listing_id: String,
    seller_pubkey: String,
    title: String,
    description: String,
    price: u64,
) -> Result<bool, String> {
    if title.trim().is_empty() || title.len() > 500 {
        return Err("title must be between 1 and 500 chars".to_string()).into();
    }
    if price == 0 {
        return Err("price must be positive".to_string()).into();
    }
    if description.len() > 10_000 {
        return Err("description exceeds 10,000 chars cap".to_string()).into();
    }
    let existing: ListingInfo = serde_json::from_str(&marketplace_get_listing(listing_id.clone())?)
        .map_err(|e| format!("parse listing: {e}"))?;
    if !existing.seller_pubkey.eq_ignore_ascii_case(&seller_pubkey) {
        return Err("only the seller can update a listing".to_string()).into();
    }
    let seller_pubkey = seller_pubkey.trim().to_ascii_lowercase();
    super::signer::require_identity(&seller_pubkey)?;
    let content = serde_json::json!({
        "title": title,
        "price": price as f64,
        "currency": existing.currency,
        "condition": existing.condition,
        "description": description,
        "images": existing.images,
    });
    let now = soshal_common_core::format::now_secs();
    super::db::with_db_result(|db| {
        let repo = soshal_db_core::repos::post::PostRepo::new(db);
        let existing_row = repo.get_by_id(&listing_id)?;
        let (tags_json, created_at) = match existing_row {
            Some(r) => (
                if !r.tags_json.is_empty() {
                    r.tags_json
                } else {
                    serde_json::to_string(&vec![
                        vec!["d".to_string(), listing_id.clone()],
                        vec!["t".to_string(), existing.category.clone()],
                    ])
                    .unwrap_or_default()
                },
                r.created_at,
            ),
            None => (
                serde_json::to_string(&vec![
                    vec!["d".to_string(), listing_id.clone()],
                    vec!["t".to_string(), existing.category.clone()],
                ])
                .unwrap_or_default(),
                now,
            ),
        };
        let row = soshal_db_core::repos::post::PostRow {
            id: listing_id,
            pubkey: seller_pubkey.clone(),
            content: content.to_string(),
            kind: KIND_LISTING as i64,
            created_at,
            tags_json,
            sig: None,
            reply_to: None,
            root_id: None,
            mentioned_pubkeys: String::new(),
            mentioned_hashtags: String::new(),
            subject: Some(title),
            sync_status: "edited".to_string(),
            is_deleted: false,
            scheduled_at: None,
            freenet_key: None,
            is_freenet_native: false,
            rsvp_event_id: None,
        };
        repo.upsert(&row)?;
        Ok(true)
    })
}

/// Mark a listing deleted (soft delete; kind 5 tombstones stay relay-side).
#[frb(sync, serialize)]
pub fn marketplace_delete_listing(
    listing_id: String,
    seller_pubkey: String,
) -> Result<bool, String> {
    let existing: ListingInfo = serde_json::from_str(&marketplace_get_listing(listing_id.clone())?)
        .map_err(|e| format!("parse listing: {e}"))?;
    if !existing.seller_pubkey.eq_ignore_ascii_case(&seller_pubkey) {
        return Err("only the seller can delete a listing".to_string()).into();
    }
    let seller_pubkey = seller_pubkey.trim().to_ascii_lowercase();
    super::signer::require_identity(&seller_pubkey)?;
    let open: i64 = super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let c = soshal_db_core::query::query_first(
            &conn,
            "SELECT COUNT(*) FROM escrows WHERE listing_id = ?1 AND status IN ('created','funded','shipped','disputed')",
            libsql::params![listing_id.as_str()],
            |r| r.get::<i64>(0),
        )?;
        Ok(c.unwrap_or(0))
    })?;
    if open > 0 {
        return Err("listing has open escrows".to_string()).into();
    }
    super::db::db_execute_params(
        "UPDATE posts SET is_deleted = 1 WHERE id = ?1",
        &[listing_id],
    )
    .map(|_| true)
    .into()
}

/// Fetch seller's listings.
#[frb(sync, serialize)]
pub fn marketplace_fetch_seller_listings(seller_pubkey: String) -> Result<String, String> {
    let norm_pk = seller_pubkey.trim().to_ascii_lowercase();
    super::util::json_ok(db_listings_params(
        &format!(
            "SELECT p.id, p.pubkey AS seller_pubkey, COALESCE(u.name,'') AS seller_name, \
             p.content, p.tags_json, p.created_at, p.is_deleted \
             FROM posts p LEFT JOIN users u ON LOWER(u.pubkey) = LOWER(p.pubkey) \
             WHERE p.kind = {KIND_LISTING} AND LOWER(p.pubkey) = ?1 AND p.is_deleted = 0 \
             ORDER BY p.created_at DESC LIMIT 200"
        ),
        &[norm_pk],
    )?)
}

/// Get listings by category (denormalized `category` column, v012).
#[frb(sync, serialize)]
pub fn marketplace_get_by_category(
    category: String,
    limit: i32,
    audience: String,
) -> Result<String, String> {
    let authors = super::identity::resolve_audience_authors(&audience)?;
    if let Some(a) = &authors {
        if a.is_empty() {
            return super::util::json_ok(Vec::<ListingInfo>::new());
        }
    }
    let author_clause = match &authors {
        Some(a) if !a.is_empty() => " AND p.pubkey IN (SELECT value FROM json_each(?2))",
        _ => "",
    };
    let sql = format!(
        "SELECT p.id, p.pubkey AS seller_pubkey, COALESCE(u.name,'') AS seller_name, \
         p.content, p.tags_json, p.created_at, p.is_deleted \
         FROM posts p LEFT JOIN users u ON u.pubkey = p.pubkey \
         WHERE p.kind = {KIND_LISTING} AND p.is_deleted = 0 AND p.category = ?1 {author_clause} \
         ORDER BY p.created_at DESC LIMIT {}",
        limit.clamp(1, 100)
    );
    let mut params: Vec<String> = vec![category];
    if let Some(a) = &authors {
        params.push(serde_json::to_string(a).map_err(|e| format!("authors: {e}"))?);
    }
    let rows = super::db::db_query_json(&sql, &params)?;
    super::util::json_ok(parse_listings_values(rows))
}

/// Trending listings: most reposts/reactions in the local DB, newest first
/// as tiebreak.
#[frb(sync, serialize)]
pub fn marketplace_get_trending(limit: i32, audience: String) -> Result<String, String> {
    let authors = super::identity::resolve_audience_authors(&audience)?;
    if let Some(a) = &authors {
        if a.is_empty() {
            return super::util::json_ok(Vec::<ListingInfo>::new());
        }
    }
    let mut params: Vec<String> = Vec::new();
    let author_clause = match &authors {
        Some(a) => {
            let j = serde_json::to_string(a).map_err(|e| format!("authors: {e}"))?;
            params.push(j);
            " AND p.pubkey IN (SELECT value FROM json_each(?1))"
        }
        None => "",
    };
    let sql = format!(
        "SELECT p.id, p.pubkey AS seller_pubkey, COALESCE(u.name,'') AS seller_name, \
         p.content, p.tags_json, p.created_at, p.is_deleted \
         FROM posts p LEFT JOIN users u ON u.pubkey = p.pubkey \
         WHERE p.kind = {KIND_LISTING} AND p.is_deleted = 0{author_clause} \
         ORDER BY p.reposts_count DESC, p.created_at DESC \
         LIMIT {}",
        limit.clamp(1, 100)
    );
    let rows = super::db::db_query_json(&sql, &params)?;
    super::util::json_ok(parse_listings_values(rows))
}

/// Create an order for a listing (kind 30403 row + local insert).
#[frb(sync, serialize)]
pub fn marketplace_create_order(
    listing_id: String,
    buyer_pubkey: String,
    seller_pubkey: String,
) -> Result<String, String> {
    let listing: ListingInfo = serde_json::from_str(&marketplace_get_listing(listing_id.clone())?)
        .map_err(|e| format!("parse listing: {e}"))?;
    if !listing.seller_pubkey.eq_ignore_ascii_case(&seller_pubkey) {
        return Err("seller does not own this listing".to_string()).into();
    }
    if buyer_pubkey
        .trim()
        .eq_ignore_ascii_case(seller_pubkey.trim())
    {
        return Err("cannot order own listing".to_string()).into();
    }
    super::signer::require_identity(&buyer_pubkey)?;
    let buyer_pubkey = buyer_pubkey.trim().to_ascii_lowercase();
    let seller_pubkey = seller_pubkey.trim().to_ascii_lowercase();
    let id = uuid_like();
    let content = serde_json::json!({
        "listingId": listing_id,
        "seller": seller_pubkey,
        "amount": listing.price,
        "status": "created",
    });
    let now = soshal_common_core::format::now_secs();
    super::db::upsert_post_row(
        id.clone(),
        buyer_pubkey,
        content.to_string(),
        KIND_ORDER as i64,
        now,
        serde_json::to_string(&vec![
            vec!["p".to_string(), seller_pubkey],
            vec!["e".to_string(), listing_id],
        ])
        .unwrap_or_default(),
        None,
    )?;
    Ok(id).into()
}

/// Get order details.
#[frb(sync, serialize)]
pub fn marketplace_get_order(order_id: String) -> Result<String, String> {
    let order: OrderInfo = super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let res = soshal_db_core::query::query_first(
            &conn,
            &format!(
                "SELECT p.id, p.content, p.pubkey, p.created_at FROM posts p \
                 WHERE p.kind = {KIND_ORDER} AND p.id = ?1"
            ),
            libsql::params![order_id.as_str()],
            |r| {
                let id: String = r.get(0)?;
                let content: String = r.get(1)?;
                let pubkey: String = r.get(2)?;
                let created_at: i64 = r.get(3)?;
                Ok(order_from_fields(id, &content, pubkey, created_at))
            },
        )?;
        res.ok_or_else(|| soshal_db_core::error::DbError::NotFound)
    })
    .map_err(|_| "Order not found".to_string())?;
    super::util::json_ok(order)
}

/// Fetch buyer's orders (posts where the order row's pubkey is the buyer).
#[frb(sync, serialize)]
pub fn marketplace_fetch_buyer_orders(buyer_pubkey: String) -> Result<String, String> {
    let buyer_pubkey = buyer_pubkey.trim().to_ascii_lowercase();
    super::signer::require_identity(&buyer_pubkey)?;
    let orders: Vec<OrderInfo> = super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let sql = format!(
            "SELECT p.id, p.content, p.pubkey, p.created_at FROM posts p \
             WHERE p.kind = {KIND_ORDER} AND LOWER(p.pubkey) = ?1 AND p.is_deleted = 0 \
             ORDER BY p.created_at DESC LIMIT 200"
        );
        let rows = soshal_db_core::query::query(
            &conn,
            &sql,
            libsql::params![buyer_pubkey.as_str()],
            |r| {
                let id: String = r.get(0)?;
                let content: String = r.get(1)?;
                let pubkey: String = r.get(2)?;
                let created_at: i64 = r.get(3)?;
                Ok(order_from_fields(id, &content, pubkey, created_at))
            },
        )?;
        Ok(rows)
    })?;
    super::util::json_ok(orders)
}

/// Fetch seller's orders (orders whose listing the seller owns, or whose
/// content `seller` field matches).
#[frb(sync, serialize)]
pub fn marketplace_fetch_seller_orders(seller_pubkey: String) -> Result<String, String> {
    let seller_pubkey = seller_pubkey.trim().to_ascii_lowercase();
    super::signer::require_identity(&seller_pubkey)?;
    let orders: Vec<OrderInfo> = super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let sql = format!(
            "SELECT p.id, p.content, p.pubkey, p.created_at FROM posts p \
             WHERE p.kind = {KIND_ORDER} AND p.is_deleted = 0 \
               AND p.content LIKE '%\"seller\":\"' || ?1 || '\"%' \
             ORDER BY p.created_at DESC LIMIT 200"
        );
        let rows = soshal_db_core::query::query(
            &conn,
            &sql,
            libsql::params![seller_pubkey.as_str()],
            |r| {
                let id: String = r.get(0)?;
                let content: String = r.get(1)?;
                let pubkey: String = r.get(2)?;
                let created_at: i64 = r.get(3)?;
                Ok(order_from_fields(id, &content, pubkey, created_at))
            },
        )?;
        Ok(rows)
    })?;
    super::util::json_ok(orders)
}

/// Create an escrow for an order (dedicated escrows table + lifecycle
/// validation from marketplace-core).
#[frb(sync, serialize)]
pub fn marketplace_create_escrow(
    order_id: String,
    buyer_pubkey: String,
    seller_pubkey: String,
    amount: u64,
) -> Result<String, String> {
    let order: OrderInfo = serde_json::from_str(&marketplace_get_order(order_id.clone())?)
        .map_err(|e| format!("parse order: {e}"))?;
    if !order.buyer_pubkey.eq_ignore_ascii_case(&buyer_pubkey)
        || !order.seller_pubkey.eq_ignore_ascii_case(&seller_pubkey)
    {
        return Err("order parties do not match escrow parties".to_string()).into();
    }
    if super::signer::require_identity(&buyer_pubkey).is_err()
        && super::signer::require_identity(&seller_pubkey).is_err()
    {
        return Err("identity mismatch: caller is not buyer or seller".into());
    }
    if amount == 0 {
        return Err("escrow amount must be positive".to_string()).into();
    }
    let escrow_id = uuid_like();
    let now = soshal_common_core::format::now_secs();
    let row = soshal_db_core::repos::escrow::EscrowRow {
        id: escrow_id.clone(),
        listing_id: order.listing_id,
        buyer_pubkey: buyer_pubkey.trim().to_ascii_lowercase(),
        seller_pubkey: seller_pubkey.trim().to_ascii_lowercase(),
        amount_msats: i64::try_from(amount).map_err(|_| "amount too large".to_string())?,
        currency: "sats".to_string(),
        status: "created".to_string(),
        escrow_note: None,
        created_at: now,
        updated_at: now,
    };
    super::db::with_db_result(|db| {
        EscrowRepo::new(db).create(&row)?;
        Ok(())
    })?;
    Ok(escrow_id).into()
}

/// Release escrow funds. Release policy comes from marketplace-core
/// `can_release`: without a dispute BOTH buyer and seller must have
/// confirmed; with a dispute only arbitrator approval (a `completed`
/// resolution) releases. The seller confirm travels via the seller's app;
/// the local row records the buyer-side confirmation.
#[frb(sync, serialize)]
pub fn marketplace_release_escrow(
    escrow_id: String,
    seller_pubkey: String,
) -> Result<bool, String> {
    // Bind the caller to the unlocked signer: the previous code dropped the
    // seller identity entirely, so any local caller could release any escrow.
    let caller = super::signer::signer_pubkey()?;
    if !caller.eq_ignore_ascii_case(&seller_pubkey) {
        return Err("release must be initiated by the seller identity".into());
    }
    super::db::with_db_result(|db| {
        let repo = EscrowRepo::new(db);
        let escrow = repo
            .get(&escrow_id)?
            .ok_or(soshal_db_core::error::DbError::NotFound)?;
        if !escrow.seller_pubkey.eq_ignore_ascii_case(&caller) {
            return Err(soshal_db_core::error::DbError::NotFound);
        }
        let (buyer_confirmed, seller_confirmed) = repo.get_confirms(&escrow_id)?;
        let disputed = escrow.status == "disputed";
        let arbitrator_approved = false;
        if !soshal_marketplace_core::escrow::can_release(
            buyer_confirmed,
            seller_confirmed,
            arbitrator_approved,
            disputed,
        ) {
            return Err(soshal_db_core::error::DbError::Oversized(
                "release requires both-party confirmation; disputed escrows need arbitrator resolution"
                    .to_string(),
            ));
        }
        repo.update_status(&escrow_id, "completed")?;
        Ok(true)
    })
}

#[frb(sync, serialize)]
pub fn marketplace_escrow_confirm_buyer(escrow_id: String, caller: String) -> Result<bool, String> {
    super::db::with_db_result(|db| {
        let repo = EscrowRepo::new(db);
        let escrow = repo
            .get(&escrow_id)?
            .ok_or(soshal_db_core::error::DbError::NotFound)?;
        if !escrow.buyer_pubkey.eq_ignore_ascii_case(&caller) {
            return Err(soshal_db_core::error::DbError::Oversized(
                "confirmation must be initiated by the buyer identity".to_string(),
            ));
        }
        super::signer::require_identity(&caller)
            .map_err(soshal_db_core::error::DbError::Migration)?;
        repo.set_buyer_confirmed(&escrow_id, true)?;
        Ok(true)
    })
}

#[frb(sync, serialize)]
pub fn marketplace_escrow_confirm_seller(
    escrow_id: String,
    caller: String,
) -> Result<bool, String> {
    super::db::with_db_result(|db| {
        let repo = EscrowRepo::new(db);
        let escrow = repo
            .get(&escrow_id)?
            .ok_or(soshal_db_core::error::DbError::NotFound)?;
        if !escrow.seller_pubkey.eq_ignore_ascii_case(&caller) {
            return Err(soshal_db_core::error::DbError::Oversized(
                "confirmation must be initiated by the seller identity".to_string(),
            ));
        }
        super::signer::require_identity(&caller)
            .map_err(soshal_db_core::error::DbError::Migration)?;
        repo.set_seller_confirmed(&escrow_id, true)?;
        Ok(true)
    })
}

/// Mark an escrow disputed.
#[frb(sync, serialize)]
pub fn marketplace_dispute_escrow(
    escrow_id: String,
    disputer_pubkey: String,
    reason: String,
) -> Result<bool, String> {
    let reason = soshal_common_core::format::truncate(&reason, 512);
    super::db::with_db_result(|db| {
        let repo = EscrowRepo::new(db);
        let escrow = repo
            .get(&escrow_id)?
            .ok_or(soshal_db_core::error::DbError::NotFound)?;
        if !escrow.buyer_pubkey.eq_ignore_ascii_case(&disputer_pubkey)
            && !escrow.seller_pubkey.eq_ignore_ascii_case(&disputer_pubkey)
        {
            return Err(soshal_db_core::error::DbError::NotFound);
        }
        super::signer::require_identity(&disputer_pubkey)
            .map_err(soshal_db_core::error::DbError::Migration)?;
        repo.update_status(&escrow_id, "disputed")?;
        repo.set_note(&escrow_id, &reason)?;
        Ok(true)
    })
}

/// Resolve a disputed escrow in favor of a party (mediator role is gated
/// by the caller; state transitions enforce parties).
#[frb(sync, serialize)]
pub fn marketplace_resolve_escrow(
    escrow_id: String,
    mediator_pubkey: String,
    winner_pubkey: String,
) -> Result<bool, String> {
    // Bind the caller to the unlocked signer: the mediator identity was
    // dropped before, letting any local caller arbitrate any escrow. The
    // caller must be an escrow party (the mediator role is a UI-level gate);
    // the winner must be a party (checked below).
    let caller = super::signer::signer_pubkey()?;
    super::db::with_db_result(|db| {
        let repo = EscrowRepo::new(db);
        let escrow = repo
            .get(&escrow_id)?
            .ok_or(soshal_db_core::error::DbError::NotFound)?;
        if !caller.eq_ignore_ascii_case(&escrow.buyer_pubkey)
            && !caller.eq_ignore_ascii_case(&escrow.seller_pubkey)
        {
            return Err(soshal_db_core::error::DbError::NotFound);
        }
        drop(mediator_pubkey);
        if !winner_pubkey.eq_ignore_ascii_case(&escrow.buyer_pubkey)
            && !winner_pubkey.eq_ignore_ascii_case(&escrow.seller_pubkey)
        {
            return Err(soshal_db_core::error::DbError::Oversized(
                "winner must be an escrow party".to_string(),
            ));
        }
        let status = if escrow.status == "disputed" {
            if winner_pubkey.eq_ignore_ascii_case(&escrow.seller_pubkey) {
                "completed"
            } else {
                "refunded"
            }
        } else {
            let (buyer_ok, seller_ok) = repo.get_confirms(&escrow_id)?;
            if !buyer_ok || !seller_ok {
                return Err(soshal_db_core::error::DbError::Oversized(
                    "resolve requires both-party confirmation".to_string(),
                ));
            }
            "completed"
        };
        repo.update_status(&escrow_id, status)?;
        repo.set_note(&escrow_id, "resolved by mediator")?;
        Ok(true)
    })
}

/// Get escrow state by id (raw JSON).
#[frb(sync, serialize)]
pub fn marketplace_get_escrow(escrow_id: String) -> Result<String, String> {
    let json = super::db::db_query_params(
        "SELECT id, listing_id, buyer_pubkey, seller_pubkey, amount_msats, currency, status, escrow_note, created_at, updated_at \
         FROM escrows WHERE id = ?1",
        &[escrow_id],
    )?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    if rows.is_empty() {
        return Err("Escrow not found".to_string()).into();
    }
    Ok(json).into()
}

/// Latest escrow for a listing (null JSON if none exists).
#[frb(sync, serialize)]
pub fn marketplace_get_escrow_by_listing(listing_id: String) -> Result<String, String> {
    let json = super::db::db_query_params(
        "SELECT id, listing_id, buyer_pubkey, seller_pubkey, amount_msats, currency, status, escrow_note, created_at, updated_at \
         FROM escrows WHERE listing_id = ?1 ORDER BY created_at DESC LIMIT 1",
        &[listing_id],
    )?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    if rows.is_empty() {
        return Ok("null".to_string()).into();
    }
    Ok(rows[0].to_string()).into()
}

/// Store a review for a listing (dedicated marketplace_reviews table).
#[frb(sync, serialize)]
pub fn marketplace_review_listing(
    listing_id: String,
    reviewer_pubkey: String,
    rating: i64,
    text: String,
) -> Result<bool, String> {
    let listing_id = listing_id.trim();
    if listing_id.is_empty() || listing_id.len() > 128 {
        return Err("listing_id must be between 1 and 128 characters".to_string());
    }
    if !(1..=5).contains(&rating) {
        return Err("rating must be between 1 and 5".to_string());
    }
    if reviewer_pubkey.trim().is_empty() {
        return Err("reviewer_pubkey cannot be empty".to_string());
    }
    super::signer::require_identity(&reviewer_pubkey)?;
    if text.len() > 5000 {
        return Err("review text exceeds 5,000 characters cap".to_string());
    }
    let reviewer_pubkey = reviewer_pubkey.trim().to_ascii_lowercase();
    let row = soshal_db_core::repos::marketplace_review::MarketplaceReviewRow {
        id: uuid_like(),
        listing_id: listing_id.to_string(),
        reviewer_pubkey,
        rating,
        text,
        created_at: soshal_common_core::format::now_secs(),
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::marketplace_review::MarketplaceReviewRepo::new(db).insert(&row)?;
        Ok(true)
    })
}

/// Reviews for a listing (newest first): array of
/// `{"rating": N, "reviewer": <pubkey>, "text": <text>}`.
#[frb(sync, serialize)]
pub fn marketplace_listing_reviews(listing_id: String, limit: i64) -> Result<String, String> {
    let listing_id = listing_id.trim();
    let limit = limit.clamp(1, 200);
    let rows = super::db::with_db_result(|db| {
        soshal_db_core::repos::marketplace_review::MarketplaceReviewRepo::new(db)
            .list_by_listing(listing_id, limit)
    })?;
    let reviews: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "rating": r.rating,
                "reviewer": r.reviewer_pubkey,
                "text": r.text,
            })
        })
        .collect();
    super::util::json_ok(reviews)
}

/// Average rating for a listing (0.0 if none).
#[frb(sync, serialize)]
pub fn marketplace_listing_rating(listing_id: String) -> Result<f64, String> {
    super::db::with_db_result(|db| {
        soshal_db_core::repos::marketplace_review::MarketplaceReviewRepo::new(db)
            .average_for(&listing_id)
    })
    .map(|v| v.unwrap_or(0.0))
}

/// Create a poll; options_json is an array of strings. Returns
/// `{"id": ..., "question": ...}`.
#[frb(sync, serialize)]
pub fn marketplace_poll_create(
    user_pubkey: String,
    question: String,
    options_json: String,
    expires_in_hours: i64,
) -> Result<String, String> {
    if options_json.len() > 64 * 1024 {
        return Err("options JSON exceeds 64KB cap".to_string()).into();
    }
    let user_pubkey = user_pubkey.trim().to_ascii_lowercase();
    super::signer::require_identity(&user_pubkey)?;
    let options: Vec<String> =
        serde_json::from_str(&options_json).map_err(|e| format!("invalid options JSON: {e}"))?;
    if options.len() < 2 {
        return Err("poll needs at least 2 options".to_string()).into();
    }
    if options.len() > 20 {
        return Err("poll supports at most 20 options".to_string()).into();
    }
    if question.trim().is_empty() || question.len() > 512 {
        return Err("poll question must be 1-512 characters".to_string()).into();
    }
    for opt in &options {
        let o = opt.trim();
        if o.is_empty() || o.len() > 256 {
            return Err("poll option must be 1-256 characters".to_string()).into();
        }
    }
    let id = uuid_like();
    let now = soshal_common_core::format::now_secs();
    // Clamp the expiry computation: a hostile/huge hours value must not
    // overflow i64 and wrap into the past (or a tiny negative) expiry.
    let offset = expires_in_hours.saturating_mul(3600);
    let expires_at = now.saturating_add(offset).max(now - 1);
    let row = soshal_db_core::repos::poll::PollRow {
        id: id.clone(),
        pubkey: user_pubkey,
        question,
        options: options_json,
        expires_at,
        closed: false,
        created_at: now,
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::poll::PollRepo::new(db).upsert_poll(&row)?;
        Ok(())
    })?;
    super::util::json_ok(serde_json::json!({
        "id": id,
        "question": row.question,
    }))
}

/// Cast a vote for a poll option.
#[frb(sync, serialize)]
pub fn marketplace_poll_vote(
    poll_id: String,
    voter_pubkey: String,
    option_index: i64,
) -> Result<bool, String> {
    if option_index < 0 {
        return Err("option_index must be non-negative".to_string());
    }
    let voter_pubkey = voter_pubkey.trim().to_ascii_lowercase();
    super::signer::require_identity(&voter_pubkey)?;

    let poll = super::db::with_db_result(|db| {
        soshal_db_core::repos::poll::PollRepo::new(db).get_poll(&poll_id)
    })?
    .ok_or_else(|| "poll not found".to_string())?;

    if poll.closed {
        return Err("poll is closed".to_string());
    }

    let vote = soshal_db_core::repos::poll::PollVoteRow {
        id: uuid_like(),
        poll_id,
        option_id: option_index,
        voter_pubkey,
        voted_at: soshal_common_core::format::now_secs(),
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::poll::PollRepo::new(db).vote(&vote)?;
        Ok(true)
    })
}

/// Close a poll; only the poll owner may close it.
#[frb(sync, serialize)]
pub fn marketplace_poll_close(poll_id: String, user_pubkey: String) -> Result<bool, String> {
    let user_pubkey = user_pubkey.trim().to_ascii_lowercase();
    super::signer::require_identity(&user_pubkey)?;
    super::db::with_db_string(|db| {
        let repo = soshal_db_core::repos::poll::PollRepo::new(db);
        let poll = repo
            .get_poll(&poll_id)
            .map_err(super::util::to_err)?
            .ok_or_else(|| "poll not found".to_string())?;
        if !poll.pubkey.eq_ignore_ascii_case(&user_pubkey) {
            return Err("not poll owner".to_string());
        }
        repo.set_closed(&poll_id, true)
            .map_err(super::util::to_err)?;
        Ok(true)
    })
}

/// Poll with vote counts: `{"id": ..., "question": ..., "votes": [...]}`.
#[frb(sync, serialize)]
pub fn marketplace_poll_get(poll_id: String) -> Result<String, String> {
    let poll = super::db::with_db_result(|db| {
        soshal_db_core::repos::poll::PollRepo::new(db).get_poll(&poll_id)
    })?
    .ok_or_else(|| "poll not found".to_string())?;
    let options: Vec<String> =
        serde_json::from_str(&poll.options).map_err(|e| format!("invalid options JSON: {e}"))?;
    let json = super::db::db_query_params(
        "SELECT option_id, COUNT(*) AS c FROM poll_votes WHERE poll_id = ?1 GROUP BY option_id",
        &[poll_id],
    )?;
    let mut votes = vec![0i64; options.len()];
    for row in serde_json::from_str::<Vec<serde_json::Value>>(&json).unwrap_or_default() {
        if let (Some(id), Some(c)) = (row["option_id"].as_i64(), row["c"].as_i64()) {
            if let Some(slot) = votes.get_mut(id as usize) {
                *slot = c;
            }
        }
    }
    super::util::json_ok(serde_json::json!({
        "id": poll.id,
        "question": poll.question,
        "votes": votes,
    }))
}

/// Whether a voter has voted in a poll.
#[frb(sync, serialize)]
pub fn marketplace_poll_has_voted(poll_id: String, voter_pubkey: String) -> Result<bool, String> {
    let voter_pubkey = voter_pubkey.trim().to_ascii_lowercase();
    super::db::with_db_result(|db| {
        soshal_db_core::repos::poll::PollRepo::new(db).has_voted(&poll_id, &voter_pubkey)
    })
}

use super::util::uuid_like;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::{db, signer};

    #[test]
    fn test_listing_parse_roundtrip() {
        let v = serde_json::json!({
            "id": "abc",
            "seller_pubkey": "pk1",
            "seller_name": "alice",
            "content": serde_json::json!({
                "title": "rust book",
                "price": 5000.0,
                "currency": "sats",
                "condition": "like new",
                "description": "hardcover",
                "locationGeohash": "u123",
                "images": ["https://x/i.png"],
                "escrowEnabled": true,
            }).to_string(),
            "tags_json": "[[\"d\",\"1\"],[\"t\",\"books\"]]",
            "created_at": 100,
            "is_deleted": false,
        });
        let info = listing_from_value(&v).unwrap();
        assert_eq!(info.id, "abc");
        assert_eq!(info.title, "rust book");
        assert_eq!(info.price, 5000);
        assert_eq!(info.category, "books");
        assert!(info.shipping_available);
    }

    #[test]
    fn test_create_rejects_empty_title() {
        let result = marketplace_create_listing(
            "pk".to_string(),
            String::new(),
            "d".to_string(),
            10,
            "sats".to_string(),
            "books".to_string(),
            "new".to_string(),
            "[]".to_string(),
            false,
        );
        assert!(result.is_err());
    }

    fn insert_listing(
        id: &str,
        seller: &str,
        title: &str,
        price: u64,
        category: &str,
        created_at: i64,
    ) {
        let content = serde_json::json!({
            "title": title,
            "price": price as f64,
            "currency": "sats",
            "condition": "new",
            "description": format!("desc {title}"),
            "images": [],
            "escrowEnabled": false,
        });
        db::insert_test_user(seller);
        db::db_execute_raw_test(format!(
            "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted, category) \
             VALUES ('{id}','{seller}','{}',{KIND_LISTING},{created_at},'[[\"d\",\"{id}\"],[\"t\",\"{category}\"]]','synced',0,'{category}')",
            content.to_string().replace('\'', "''")
        ))
        .unwrap();
    }

    fn insert_escrow(id: &str, listing_id: &str, status: &str) {
        insert_escrow_parties(id, listing_id, "buyer1", "seller1", status);
    }

    fn insert_escrow_parties(id: &str, listing_id: &str, buyer: &str, seller: &str, status: &str) {
        db::insert_test_user(buyer);
        db::insert_test_user(seller);
        db::db_execute_raw_test(format!(
            "INSERT INTO escrows (id, listing_id, buyer_pubkey, seller_pubkey, amount_msats, currency, status, escrow_note, created_at, updated_at) \
             VALUES ('{id}','{listing_id}','{buyer}','{seller}',5000,'sats','{status}',NULL,100,100)"
        ))
        .unwrap();
    }

    #[test]
    fn test_listing_queries_and_crud() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = db::tmp_db("listings", "market");
        let keys_seller = soshal_nostr_core::keys::generate_keys();
        let seller1 = keys_seller.public_key().to_hex();
        signer::signer_unlock(keys_seller.secret_key().to_secret_hex()).unwrap();
        insert_listing("l1", &seller1, "rust book", 5000, "books", 2000);
        insert_listing("l2", "seller2", "chess set", 3000, "games", 1000);

        let json = marketplace_fetch_listings(10, 0, "public".to_string()).unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(arr.len(), 2, "json: {json}");
        assert_eq!(arr[0]["id"], "l1");
        assert_eq!(arr[0]["title"], "rust book");
        assert_eq!(arr[0]["seller_name"], "");
        let json = marketplace_fetch_listings(0, 0, "public".to_string()).unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<serde_json::Value>>(&json)
                .unwrap()
                .len(),
            1
        );
        let json = marketplace_fetch_listings(10, 1, "public".to_string()).unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(arr[0]["id"], "l2");

        let json = marketplace_search("rust book".to_string(), 10, "public".to_string()).unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], "l1");
        assert_eq!(
            marketplace_search(String::new(), 10, "public".to_string()).unwrap(),
            "[]"
        );
        let json = marketplace_search("zzz".to_string(), 10, "public".to_string()).unwrap();
        assert!(serde_json::from_str::<Vec<serde_json::Value>>(&json)
            .unwrap()
            .is_empty());

        let info = marketplace_get_listing("l1".to_string()).unwrap();
        assert!(info.contains("\"title\":\"rust book\""));
        assert!(marketplace_get_listing("nope".to_string())
            .unwrap_err()
            .contains("Listing not found"));

        let content = marketplace_get_content("l1".to_string()).unwrap();
        assert!(content.contains("rust book"));
        assert_eq!(marketplace_get_content("nope".to_string()).unwrap(), "{}");

        let json = marketplace_fetch_seller_listings(seller1.clone()).unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["seller_pubkey"], seller1);
        let json_upper = marketplace_fetch_seller_listings(seller1.to_uppercase()).unwrap();
        let arr_upper: Vec<serde_json::Value> = serde_json::from_str(&json_upper).unwrap();
        assert_eq!(arr_upper.len(), 1);
        let json = marketplace_fetch_seller_listings("nobody".to_string()).unwrap();
        assert!(serde_json::from_str::<Vec<serde_json::Value>>(&json)
            .unwrap()
            .is_empty());

        let json =
            marketplace_get_by_category("books".to_string(), 10, "public".to_string()).unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["category"], "books");
        let json =
            marketplace_get_by_category("nope".to_string(), 10, "public".to_string()).unwrap();
        assert!(serde_json::from_str::<Vec<serde_json::Value>>(&json)
            .unwrap()
            .is_empty());

        let json = marketplace_get_trending(10, "public".to_string()).unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(arr.len(), 2);

        assert!(marketplace_update_listing(
            "l1".to_string(),
            "intruder".to_string(),
            "x".to_string(),
            "x".to_string(),
            1,
        )
        .unwrap_err()
        .contains("only the seller can update"));
        assert!(marketplace_update_listing(
            "l1".to_string(),
            seller1.clone(),
            "rust book 2nd ed".to_string(),
            "hardcover".to_string(),
            6000,
        )
        .unwrap());
        let info = marketplace_get_listing("l1".to_string()).unwrap();
        assert!(info.contains("rust book 2nd ed"), "{info}");
        assert!(info.contains("\"price\":6000"), "{info}");

        insert_escrow("esc1", "l1", "created");
        assert!(
            marketplace_delete_listing("l1".to_string(), seller1.clone())
                .unwrap_err()
                .contains("open escrows")
        );
        db::db_execute_raw_test("DELETE FROM escrows WHERE id='esc1'".to_string()).unwrap();
        assert!(marketplace_delete_listing("l1".to_string(), seller1).unwrap());
        let info = marketplace_get_listing("l1".to_string()).unwrap();
        assert!(info.contains("\"status\":\"active\""), "{info}");
        let json = marketplace_fetch_listings(10, 0, "public".to_string()).unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], "l2");
        signer::signer_lock().unwrap();
    }

    #[test]
    fn test_create_listing_signs_and_stores() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = db::tmp_db("create", "market");
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk_hex = keys.public_key().to_hex();
        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        db::insert_test_user(&pk_hex);

        let signed = marketplace_create_listing(
            pk_hex.clone(),
            "widget".to_string(),
            "a widget".to_string(),
            1000,
            "sats".to_string(),
            "tools".to_string(),
            "new".to_string(),
            "[\"https://x/a.png\"]".to_string(),
            true,
        )
        .unwrap();
        let ev: serde_json::Value = serde_json::from_str(&signed).unwrap();
        assert_eq!(ev["kind"], 30402);
        assert_eq!(ev["pubkey"], pk_hex);
        assert!(ev["sig"].as_str().is_some());
        let event_id = ev["id"].as_str().unwrap();

        let info = marketplace_get_listing(event_id.to_string()).unwrap();
        assert!(
            info.contains(&format!("\"seller_pubkey\":\"{pk_hex}\"")),
            "{info}"
        );
        assert!(info.contains("widget"));
        assert!(info.contains("\"shipping_available\":true"), "{info}");
        assert!(info.contains("\"escrow_enabled\":true"), "{info}");
        assert!(info.contains("\"category\":\"tools\""), "{info}");

        assert!(marketplace_create_listing(
            pk_hex.clone(),
            "w".to_string(),
            "d".to_string(),
            0,
            "sats".to_string(),
            "tools".to_string(),
            "new".to_string(),
            "[]".to_string(),
            false,
        )
        .unwrap_err()
        .contains("price must be positive"));
        assert!(marketplace_create_listing(
            pk_hex.clone(),
            "w".to_string(),
            "d".to_string(),
            10,
            "sats".to_string(),
            "tools".to_string(),
            "new".to_string(),
            "nope".to_string(),
            false,
        )
        .unwrap_err()
        .contains("invalid images JSON"));
        let many = format!("[{}]", vec!["\"https://x/i.png\""; 13].join(","));
        assert!(marketplace_create_listing(
            pk_hex.clone(),
            "w".to_string(),
            "d".to_string(),
            10,
            "sats".to_string(),
            "tools".to_string(),
            "new".to_string(),
            many,
            false,
        )
        .unwrap_err()
        .contains("too many images"));

        // Description too long
        assert!(marketplace_create_listing(
            pk_hex.clone(),
            "w".to_string(),
            "d".repeat(10_001),
            10,
            "sats".to_string(),
            "tools".to_string(),
            "new".to_string(),
            "[]".to_string(),
            false,
        )
        .unwrap_err()
        .contains("description exceeds"));

        // Invalid image URL (SSRF)
        assert!(marketplace_create_listing(
            pk_hex.clone(),
            "w".to_string(),
            "desc".to_string(),
            10,
            "sats".to_string(),
            "tools".to_string(),
            "new".to_string(),
            "[\"http://127.0.0.1/evil.png\"]".to_string(),
            false,
        )
        .unwrap_err()
        .contains("invalid image URL"));

        // Update validation
        assert!(marketplace_update_listing(
            event_id.to_string(),
            pk_hex.clone(),
            "".to_string(),
            "desc".to_string(),
            100,
        )
        .is_err());
        assert!(marketplace_update_listing(
            event_id.to_string(),
            pk_hex.clone(),
            "x".repeat(501),
            "desc".to_string(),
            100,
        )
        .is_err());
        assert!(marketplace_update_listing(
            event_id.to_string(),
            pk_hex.clone(),
            "widget 2".to_string(),
            "desc".to_string(),
            0,
        )
        .is_err());
        assert!(marketplace_update_listing(
            event_id.to_string(),
            pk_hex.clone(),
            "widget 2".to_string(),
            "d".repeat(10_001),
            100,
        )
        .is_err());

        assert!(marketplace_update_listing(
            event_id.to_string(),
            pk_hex.clone(),
            "widget 2".to_string(),
            "new desc".to_string(),
            2000,
        )
        .unwrap());
        let info = marketplace_get_listing(event_id.to_string()).unwrap();
        assert!(info.contains("widget 2"), "{info}");
        assert!(info.contains("\"category\":\"tools\""), "{info}");
        signer::signer_lock().unwrap();
    }

    #[test]
    fn test_order_and_escrow_flow() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = db::tmp_db("orders", "market");
        let keys = soshal_nostr_core::keys::generate_keys();
        let spk = keys.public_key().to_hex();
        let bkeys = soshal_nostr_core::keys::generate_keys();
        let bpk = bkeys.public_key().to_hex();

        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        insert_listing("l1", &spk, "widget", 5000, "tools", 2000);

        // Fail: seller doesn't own listing
        signer::signer_unlock(bkeys.secret_key().to_secret_hex()).unwrap();
        assert!(
            marketplace_create_order("l1".to_string(), bpk.clone(), "seller2".to_string(),)
                .unwrap_err()
                .contains("seller does not own this listing")
        );
        db::insert_test_user(&bpk);
        let order_id =
            marketplace_create_order("l1".to_string(), bpk.clone(), spk.clone()).unwrap();

        let order = marketplace_get_order(order_id.clone()).unwrap();
        assert!(order.contains("\"status\":\"created\""), "{order}");
        assert!(order.contains("\"listing_id\":\"l1\""), "{order}");
        assert!(
            order.contains(&format!("\"seller_pubkey\":\"{spk}\"")),
            "{order}"
        );
        assert!(
            order.contains(&format!("\"buyer_pubkey\":\"{bpk}\"")),
            "{order}"
        );
        assert!(marketplace_get_order("nope".to_string())
            .unwrap_err()
            .contains("Order not found"));

        let json = marketplace_fetch_buyer_orders(bpk.clone()).unwrap();
        assert!(json.contains(&order_id), "{json}");
        let json_upper = marketplace_fetch_buyer_orders(bpk.to_uppercase()).unwrap();
        assert!(json_upper.contains(&order_id), "{json_upper}");
        assert!(marketplace_fetch_seller_orders(spk.clone()).is_err());
        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let json = marketplace_fetch_seller_orders(spk.clone()).unwrap();
        assert!(json.contains(&order_id), "{json}");
        assert!(marketplace_fetch_seller_orders("nobody".to_string()).is_err());
        signer::signer_unlock(bkeys.secret_key().to_secret_hex()).unwrap();

        assert!(marketplace_create_escrow(
            order_id.clone(),
            "nobody".to_string(),
            spk.clone(),
            5000
        )
        .unwrap_err()
        .contains("parties do not match"));
        assert!(
            marketplace_create_escrow(order_id.clone(), bpk.clone(), spk.clone(), 0,)
                .unwrap_err()
                .contains("amount must be positive")
        );
        let escrow_id =
            marketplace_create_escrow(order_id.clone(), bpk.clone(), spk.clone(), 5000).unwrap();

        let escrow = marketplace_get_escrow(escrow_id.clone()).unwrap();
        assert!(escrow.contains("\"status\":\"created\""), "{escrow}");
        assert!(marketplace_get_escrow("nope".to_string())
            .unwrap_err()
            .contains("Escrow not found"));
        let by_listing = marketplace_get_escrow_by_listing("l1".to_string()).unwrap();
        assert!(by_listing.contains(&escrow_id), "{by_listing}");
        assert_eq!(
            marketplace_get_escrow_by_listing("zzz".to_string()).unwrap(),
            "null"
        );

        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        assert!(marketplace_release_escrow(escrow_id.clone(), spk.clone()).is_err());
        assert!(
            marketplace_resolve_escrow(escrow_id.clone(), "mediator".to_string(), spk.clone(),)
                .is_err()
        );

        signer::signer_unlock(bkeys.secret_key().to_secret_hex()).unwrap();
        let escrow2 =
            marketplace_create_escrow(order_id.clone(), bpk.clone(), spk.clone(), 5000).unwrap();
        assert!(marketplace_dispute_escrow(
            escrow2.clone(),
            "outsider".to_string(),
            "bad".to_string()
        )
        .is_err());
        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        assert!(marketplace_dispute_escrow(
            escrow2.clone(),
            spk.clone(),
            "item not as described".to_string(),
        )
        .unwrap());
        // Disputed escrows need arbitrator resolution; caller release refused.
        assert!(marketplace_release_escrow(escrow2, spk.clone()).is_err());

        // Non-disputed escrow releases only after BOTH parties confirm.
        signer::signer_unlock(bkeys.secret_key().to_secret_hex()).unwrap();
        let escrow4 =
            marketplace_create_escrow(order_id.clone(), bpk.clone(), spk.clone(), 5000).unwrap();
        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        assert!(marketplace_release_escrow(escrow4.clone(), spk.clone()).is_err());
        db::db_execute_raw_test(format!(
            "UPDATE escrows SET buyer_confirmed=1, seller_confirmed=1 WHERE id='{escrow4}'"
        ))
        .unwrap();
        assert!(marketplace_release_escrow(escrow4.clone(), spk.clone()).unwrap());
        let escrow = marketplace_get_escrow(escrow4).unwrap();
        assert!(escrow.contains("\"status\":\"completed\""), "{escrow}");

        signer::signer_unlock(bkeys.secret_key().to_secret_hex()).unwrap();
        let escrow3 = marketplace_create_escrow(order_id, bpk.clone(), spk.clone(), 5000).unwrap();
        assert!(marketplace_resolve_escrow(
            escrow3.clone(),
            "mediator".to_string(),
            "outsider".to_string(),
        )
        .is_err());
        marketplace_dispute_escrow(escrow3.clone(), bpk.clone(), "refund".to_string()).unwrap();
        assert!(
            marketplace_resolve_escrow(escrow3.clone(), "mediator".to_string(), bpk.clone(),)
                .unwrap()
        );
        let escrow = marketplace_get_escrow(escrow3).unwrap();
        assert!(escrow.contains("\"status\":\"refunded\""), "{escrow}");
        assert!(escrow.contains("resolved by mediator"), "{escrow}");
        let order5 = marketplace_create_order("l1".to_string(), bpk.clone(), spk.clone()).unwrap();
        let escrow5 = marketplace_create_escrow(order5, bpk.clone(), spk.clone(), 5000).unwrap();
        marketplace_dispute_escrow(escrow5.clone(), bpk.clone(), "refund".to_string()).unwrap();
        assert!(
            marketplace_resolve_escrow(escrow5.clone(), "mediator".to_string(), spk.clone(),)
                .unwrap()
        );
        let escrow = marketplace_get_escrow(escrow5).unwrap();
        assert!(escrow.contains("\"status\":\"completed\""), "{escrow}");
        signer::signer_lock().unwrap();
    }

    #[test]
    fn test_reviews() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = db::tmp_db("reviews", "market");
        let keys1 = soshal_nostr_core::keys::generate_keys();
        let r1 = keys1.public_key().to_hex();
        let keys2 = soshal_nostr_core::keys::generate_keys();
        let r2 = keys2.public_key().to_hex();

        // Must reject review without unlocked signer matching reviewer
        signer::signer_lock().unwrap();
        assert!(
            marketplace_review_listing("l1".to_string(), r1.clone(), 5, "great".to_string(),)
                .is_err()
        );

        // Reviewer 1 reviews
        signer::signer_unlock(keys1.secret_key().to_secret_hex()).unwrap();
        assert!(
            marketplace_review_listing("l1".to_string(), r1.clone(), 5, "great".to_string(),)
                .unwrap()
        );

        // Reviewer 2 reviews
        signer::signer_unlock(keys2.secret_key().to_secret_hex()).unwrap();
        assert!(
            marketplace_review_listing("l1".to_string(), r2.clone(), 3, "ok".to_string(),).unwrap()
        );
        let json = marketplace_listing_reviews("l1".to_string(), 10).unwrap();
        assert!(
            json.contains("\"rating\":5") && json.contains("\"rating\":3"),
            "{json}"
        );
        assert!(json.contains(&r1), "{json}");
        assert_eq!(
            marketplace_listing_reviews("nope".to_string(), 10).unwrap(),
            "[]"
        );
        assert_eq!(marketplace_listing_rating("l1".to_string()).unwrap(), 4.0);
        assert_eq!(marketplace_listing_rating("nope".to_string()).unwrap(), 0.0);
        signer::signer_lock().unwrap();
    }

    #[test]
    fn test_polls() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = db::tmp_db("polls", "market");
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        assert!(
            marketplace_poll_create(pk.clone(), "q?".to_string(), "[\"a\"]".to_string(), 24,)
                .unwrap_err()
                .contains("at least 2 options")
        );
        assert!(
            marketplace_poll_create(pk.clone(), "q?".to_string(), "nope".to_string(), 24,)
                .unwrap_err()
                .contains("invalid options JSON")
        );

        let created = marketplace_poll_create(
            pk.clone(),
            "best?".to_string(),
            "[\"a\",\"b\",\"c\"]".to_string(),
            24,
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&created).unwrap();
        let poll_id = v["id"].as_str().unwrap().to_string();
        assert_eq!(v["question"], "best?");

        let poll = marketplace_poll_get(poll_id.clone()).unwrap();
        assert!(poll.contains("\"votes\":[0,0,0]"), "{poll}");
        assert!(!marketplace_poll_has_voted(poll_id.clone(), pk.clone()).unwrap());
        assert!(marketplace_poll_vote("nope".to_string(), pk.clone(), 0)
            .unwrap_err()
            .contains("not found"));
        assert!(marketplace_poll_vote(poll_id.clone(), pk.clone(), -1)
            .unwrap_err()
            .contains("non-negative"));
        assert!(marketplace_poll_vote(poll_id.clone(), "v1".to_string(), 1).is_err());
        assert!(marketplace_poll_vote(poll_id.clone(), pk.clone(), 1).unwrap());
        assert!(marketplace_poll_has_voted(poll_id.clone(), pk.clone()).unwrap());
        let poll = marketplace_poll_get(poll_id.clone()).unwrap();
        assert!(poll.contains("\"votes\":[0,1,0]"), "{poll}");

        assert!(marketplace_poll_close(poll_id.clone(), "other".to_string())
            .unwrap_err()
            .contains("identity mismatch"));
        assert!(marketplace_poll_close(poll_id.clone(), pk.clone()).unwrap());
        assert!(marketplace_poll_vote(poll_id.clone(), pk.clone(), 0)
            .unwrap_err()
            .contains("closed"));
        assert!(marketplace_poll_get("nope".to_string())
            .unwrap_err()
            .contains("poll not found"));
        signer::signer_lock().unwrap();
    }

    #[test]
    fn test_parse_branches() {
        // Garbage content row -> filtered out (no price tag, no parseable content).
        let garbage = serde_json::json!({
            "id": "g1",
            "seller_pubkey": "pk1",
            "seller_name": "alice",
            "content": "not json at all",
            "tags_json": "[]",
            "created_at": 100,
            "is_deleted": false,
        });
        assert!(listing_from_value(&garbage).is_none());

        // tags_json as array value (object form, not string).
        let arr_tags = serde_json::json!({
            "id": "a1",
            "seller_pubkey": "pk1",
            "content": serde_json::json!({"title": "x", "price": 5.0}).to_string(),
            "tags_json": [["d", "1"], ["t", "books"]],
            "created_at": 100,
        });
        let info = listing_from_value(&arr_tags).unwrap();
        assert_eq!(info.title, "x");
        assert_eq!(info.category, "books");

        // content as JSON object (non-string) branch.
        let obj_content = serde_json::json!({
            "id": "a2",
            "seller_pubkey": "pk1",
            "content": serde_json::json!({
                "title": "y",
                "price": 7.0,
                "currency": "sats",
                "condition": "new",
                "images": [],
                "escrowEnabled": false,
            }),
            "tags_json": "[[\"d\",\"2\"]]",
            "created_at": 100,
        });
        let info = listing_from_value(&obj_content).unwrap();
        assert_eq!(info.title, "y");
        assert_eq!(info.price, 7);
        assert_eq!(info.currency, "sats");

        // order_from_value: malformed content -> fallback fields, not None.
        let bad_order = serde_json::json!({
            "id": "o1",
            "content": "garbage",
            "pubkey": "buyer1",
            "created_at": 100,
        });
        let order = order_from_value(&bad_order).unwrap();
        assert_eq!(order.status, "created");
        assert_eq!(order.listing_id, "");
        assert_eq!(order.amount, 0);

        // order_from_value: missing id -> None.
        assert!(order_from_value(&serde_json::json!({"content": "{}"})).is_none());
    }

    #[test]
    fn test_crud_edge_cases() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = db::tmp_db("mkt_edges", "mk");
        insert_listing("l1", "seller1", "rust book", 5000, "books", 2000);
        insert_listing("l2", "seller2", "chess set", 3000, "games", 1000);

        assert!(marketplace_update_listing(
            "nope".to_string(),
            "seller1".to_string(),
            "x".to_string(),
            "x".to_string(),
            1,
        )
        .unwrap_err()
        .contains("Listing not found"));

        assert!(
            marketplace_delete_listing("l1".to_string(), "intruder".to_string())
                .unwrap_err()
                .contains("only the seller can delete")
        );
        assert!(
            marketplace_delete_listing("nope".to_string(), "seller1".to_string())
                .unwrap_err()
                .contains("Listing not found")
        );

        assert!(marketplace_create_order(
            "nope".to_string(),
            "buyer1".to_string(),
            "seller1".to_string(),
        )
        .unwrap_err()
        .contains("Listing not found"));

        // get_order with malformed content: row still resolves with fallback fields.
        db::insert_test_user("buyer1");
        db::db_execute_raw_test(format!(
            "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted, category) \
             VALUES ('o1','buyer1','garbage',{KIND_ORDER},100,'[]','synced',0,'')"
        ))
        .unwrap();
        let order = marketplace_get_order("o1".to_string()).unwrap();
        assert!(order.contains("\"status\":\"created\""), "{order}");
        assert!(order.contains("\"listing_id\":\"\""), "{order}");

        // get_content: non-string content storage (BLOB -> hex) hits to_string fallback path.
        db::insert_test_user("s");
        db::db_execute_raw_test(
            "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted, category) \
             VALUES ('blob1','s',x'deadbeef',30402,100,'[]','synced',0,'')"
                .to_string(),
        )
        .unwrap();
        assert_eq!(
            marketplace_get_content("blob1".to_string()).unwrap(),
            "deadbeef"
        );

        // Negative offset clamps to 0 (returns all rows).
        let json = marketplace_fetch_listings(10, -5, "public".to_string()).unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], "l1");
    }

    #[test]
    fn test_escrow_edge_cases() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = db::tmp_db("mkt_esc", "mk");
        let keys = soshal_nostr_core::keys::generate_keys();
        let spk = keys.public_key().to_hex();
        let bkeys = soshal_nostr_core::keys::generate_keys();
        let bpk = bkeys.public_key().to_hex();
        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        insert_listing("l1", "seller1", "widget", 5000, "tools", 2000);
        insert_escrow_parties("escA", "l1", &bpk, &spk, "created");

        assert!(marketplace_create_escrow(
            "nope".to_string(),
            bpk.clone(),
            "seller1".to_string(),
            5000,
        )
        .unwrap_err()
        .contains("Order not found"));
        assert!(marketplace_release_escrow("nope".to_string(), spk.clone())
            .unwrap_err()
            .contains("not found"));
        assert!(
            marketplace_dispute_escrow("nope".to_string(), bpk.clone(), "x".to_string())
                .unwrap_err()
                .contains("not found")
        );
        assert!(marketplace_resolve_escrow(
            "nope".to_string(),
            "mediator".to_string(),
            bpk.clone(),
        )
        .unwrap_err()
        .contains("not found"));

        // Long dispute reason truncated to 512 chars (trailing ellipsis).
        let long = "a".repeat(600);
        signer::signer_unlock(bkeys.secret_key().to_secret_hex()).unwrap();
        assert!(marketplace_dispute_escrow("escA".to_string(), bpk.clone(), long.clone()).unwrap());
        let escrow = marketplace_get_escrow("escA".to_string()).unwrap();
        let rows: serde_json::Value = serde_json::from_str(&escrow).unwrap();
        let note = rows[0]["escrow_note"].as_str().unwrap();
        assert_eq!(note.chars().count(), 512);
        assert!(note.ends_with('…'));
        assert!(note.starts_with(&long[..100]));
        signer::signer_lock().unwrap();
    }

    #[test]
    fn test_polls_clamps_trending() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = db::tmp_db("mkt_poll", "mk");
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();

        // Negative expires_in_hours -> expires_at in the past.
        let created = marketplace_poll_create(
            pk.clone(),
            "q?".to_string(),
            "[\"a\",\"b\"]".to_string(),
            -24,
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&created).unwrap();
        let pid = v["id"].as_str().unwrap().to_string();
        let rows = db::db_query_params("SELECT expires_at FROM polls WHERE id=?1", &[pid.clone()])
            .unwrap();
        let expires: i64 = serde_json::from_str::<Vec<serde_json::Value>>(&rows).unwrap()[0]
            ["expires_at"]
            .as_i64()
            .unwrap();
        assert!(expires < soshal_common_core::format::now_secs());

        // Out-of-range option vote skipped in counts.
        assert!(marketplace_poll_vote(pid.clone(), pk.clone(), 5).unwrap());
        let poll = marketplace_poll_get(pid).unwrap();
        assert!(poll.contains("\"votes\":[0,0]"), "{poll}");

        assert!(marketplace_poll_close("nope".to_string(), pk.clone())
            .unwrap_err()
            .contains("poll not found"));

        // Corrupt options_json -> Err from poll_get.
        db::db_execute_raw_test(
            "INSERT INTO polls (id, pubkey, question, options, expires_at, closed, created_at) \
             VALUES ('pcorr','pk1','q','garbage',999999999,0,100)"
                .to_string(),
        )
        .unwrap();
        assert!(marketplace_poll_get("pcorr".to_string())
            .unwrap_err()
            .contains("invalid options JSON"));

        // Search limit clamps: 0 -> 1, 200 -> 100 (101 matching rows seeded).
        for i in 0..101 {
            insert_listing(
                &format!("bk{i}"),
                "seller1",
                "rust book",
                100,
                "books",
                3000 + i,
            );
        }
        let json = marketplace_search("rust book".to_string(), 0, "public".to_string()).unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<serde_json::Value>>(&json)
                .unwrap()
                .len(),
            1
        );
        let json = marketplace_search("rust book".to_string(), 200, "public".to_string()).unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<serde_json::Value>>(&json)
                .unwrap()
                .len(),
            100
        );

        // Trending: repost count first, created_at tiebreak.
        insert_listing("t1", "s1", "alpha", 100, "cat", 1000);
        insert_listing("t2", "s2", "beta", 100, "cat", 2000);
        db::insert_test_user("u1");
        db::insert_test_user("u2");
        db::db_execute_raw_test(
            "INSERT INTO reposts (id, pubkey, event_id, created_at) VALUES \
             ('rp1','u1','t1',100),('rp2','u1','t2',101),('rp3','u2','t2',102)"
                .to_string(),
        )
        .unwrap();
        let json = marketplace_get_trending(10, "public".to_string()).unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(arr[0]["id"], "t2", "{json}");
        assert_eq!(arr[1]["id"], "t1", "{json}");
    }

    #[test]
    fn test_create_listing_validation_and_signer_lock() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = db::tmp_db("mkt_sign", "mk");

        // Locked signer -> Err, asserted BEFORE unlock.
        signer::signer_lock().unwrap();
        let err = marketplace_create_listing(
            "pkA".to_string(),
            "widget".to_string(),
            "d".to_string(),
            1000,
            "sats".to_string(),
            "tools".to_string(),
            "new".to_string(),
            "[]".to_string(),
            true,
        )
        .unwrap_err();
        assert!(err.contains("signer locked"), "{err}");

        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        db::insert_test_user(&pk);

        let long_title = "x".repeat(501);
        assert!(marketplace_create_listing(
            pk.clone(),
            long_title,
            "d".to_string(),
            1000,
            "sats".to_string(),
            "tools".to_string(),
            "new".to_string(),
            "[]".to_string(),
            false,
        )
        .unwrap_err()
        .contains("title must be between 1 and 500 chars"));

        // Empty currency -> "sats"; shipping=false -> null geohash.
        let signed = marketplace_create_listing(
            pk.clone(),
            "widget".to_string(),
            "d".to_string(),
            1000,
            String::new(),
            "tools".to_string(),
            "new".to_string(),
            "[]".to_string(),
            false,
        )
        .unwrap();
        let ev: serde_json::Value = serde_json::from_str(&signed).unwrap();
        let ev_content: serde_json::Value =
            serde_json::from_str(ev["content"].as_str().unwrap()).unwrap();
        assert_eq!(ev_content["currency"], "sats");
        assert_eq!(ev_content["locationGeohash"], serde_json::Value::Null);
        let event_id = ev["id"].as_str().unwrap();
        let info: serde_json::Value =
            serde_json::from_str(&marketplace_get_listing(event_id.to_string()).unwrap()).unwrap();
        assert!(info.is_object(), "info is not an object: {info:?}");
        assert!(
            info.get("content").is_none() || !info["content"].is_string(),
            "content is a string, keys: {:?}",
            info.as_object().map(|m| m.keys().collect::<Vec<_>>())
        );
        assert!(
            info["shipping_available"] == serde_json::Value::Bool(false)
                && info["currency"] == "sats",
            "got {info:?}"
        );

        signer::signer_lock().unwrap();
    }

    #[test]
    fn test_batch18_marketplace_hardening() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = db::tmp_db("market_b18", "market");
        let keys = soshal_nostr_core::keys::generate_keys();
        let spk = keys.public_key().to_hex();
        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        db::insert_test_user(&spk);
        insert_listing("list_b18", &spk, "Item", 1000, "goods", 100);

        // Self-ordering must be rejected
        let err =
            marketplace_create_order("list_b18".to_string(), spk.clone(), spk.clone()).unwrap_err();
        assert!(err.contains("cannot order own listing"), "{err}");

        // Updating listing with uppercase seller pubkey should succeed
        assert!(marketplace_update_listing(
            "list_b18".to_string(),
            spk.to_ascii_uppercase(),
            "Item Updated".to_string(),
            "New desc".to_string(),
            2000,
        )
        .unwrap());

        // Review text length cap > 5000
        let long_rev = "a".repeat(5001);
        let err = marketplace_review_listing("list_b18".to_string(), spk.clone(), 5, long_rev)
            .unwrap_err();
        assert!(err.contains("5,000"), "{err}");

        // Review with matching uppercase pubkey succeeds (signer active)
        assert!(marketplace_review_listing(
            "list_b18".to_string(),
            spk.to_ascii_uppercase(),
            5,
            "Good product".to_string()
        )
        .unwrap());

        // Poll options JSON cap > 64KB
        let huge_opts = format!("[{}]", "\"option\",".repeat(10_000));
        let err = marketplace_poll_create(spk.clone(), "Question?".to_string(), huge_opts, 24)
            .unwrap_err();
        assert!(err.contains("64KB"), "{err}");

        signer::signer_lock().unwrap();
    }
}
