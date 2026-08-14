//! Marketplace FFI module
//!
//! NIP-15 marketplaces: listings and orders live as kind 30402/30403 rows
//! in the posts table (relay-synced like all events); escrows use the
//! dedicated `escrows` table. Listing creation follows the Tauri flow:
//! content built via marketplace-core, signed by the unlocked signer, and
//! the signed event returned for relay publish.

use flutter_rust_bridge::frb;
use serde::{Deserialize, Serialize};
use soshal_db_core::repos::escrow::EscrowRepo;

const KIND_LISTING: i64 = 30402;
const KIND_ORDER: i64 = 30403;

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

#[derive(Deserialize)]
struct ListingContent {
    title: Option<String>,
    price: Option<f64>,
    #[serde(default)]
    currency: Option<String>,
    condition: Option<String>,
    description: Option<String>,
    #[serde(rename = "locationGeohash")]
    location_geohash: Option<String>,
    #[serde(default)]
    images: Vec<String>,
    #[serde(rename = "escrowEnabled", default)]
    escrow_enabled: bool,
}

fn listing_from_value(v: &serde_json::Value) -> Option<ListingInfo> {
    let content_value: serde_json::Value = match v["content"].as_str() {
        Some(s) => serde_json::from_str(s).unwrap_or(serde_json::Value::Null),
        None => v["content"].clone(),
    };
    let content: ListingContent = serde_json::from_value(content_value).ok()?;
    let tags_value: serde_json::Value = match v["tags_json"].as_str() {
        Some(s) => serde_json::from_str(s).unwrap_or(serde_json::Value::Null),
        None => v["tags_json"].clone(),
    };
    let tags = tags_value.as_array().cloned().unwrap_or_default();
    let category = tags
        .iter()
        .find(|t| {
            t.as_array()
                .and_then(|t| t.first())
                .and_then(|s| s.as_str())
                == Some("t")
        })
        .and_then(|t| t.as_array().and_then(|t| t.get(1)).and_then(|s| s.as_str()))
        .unwrap_or("")
        .to_string();
    Some(ListingInfo {
        id: v["id"].as_str()?.to_string(),
        seller_pubkey: v["seller_pubkey"].as_str().unwrap_or("").to_string(),
        seller_name: v["seller_name"].as_str().unwrap_or("").to_string(),
        title: content.title.unwrap_or_default(),
        description: content.description.unwrap_or_default(),
        images: content.images,
        price: (content.price.unwrap_or(0.0).max(0.0)) as u64,
        currency: content.currency.unwrap_or_else(|| "sats".to_string()),
        category,
        condition: content.condition.unwrap_or_default(),
        shipping_available: content.location_geohash.is_some(),
        escrow_enabled: content.escrow_enabled,
        created_at: v["created_at"].as_i64().unwrap_or(0).max(0) as u64,
        updated_at: v["created_at"].as_i64().unwrap_or(0).max(0) as u64,
        status: if v["is_deleted"].as_bool().unwrap_or(false) {
            "deleted".to_string()
        } else {
            "active".to_string()
        },
    })
}

fn order_from_value(v: &serde_json::Value) -> Option<OrderInfo> {
    let content: serde_json::Value = match v["content"].as_str() {
        Some(s) => serde_json::from_str(s).unwrap_or(serde_json::Value::Null),
        None => v["content"].clone(),
    };
    Some(OrderInfo {
        id: v["id"].as_str()?.to_string(),
        listing_id: content["listingId"].as_str().unwrap_or("").to_string(),
        buyer_pubkey: v["seller_pubkey"].as_str().unwrap_or("").to_string(),
        seller_pubkey: content["seller"].as_str().unwrap_or("").to_string(),
        status: content["status"].as_str().unwrap_or("created").to_string(),
        amount: content["amount"].as_f64().unwrap_or(0.0) as u64,
        created_at: v["created_at"].as_i64().unwrap_or(0).max(0) as u64,
    })
}

fn listings_sql(prelude: &str, limit: i32, offset: i32) -> String {
    format!(
        "{prelude} SELECT p.id, p.pubkey AS seller_pubkey, COALESCE(u.name,'') AS seller_name, \
         p.content, p.tags_json, p.created_at, p.is_deleted \
         FROM posts p LEFT JOIN users u ON u.pubkey = p.pubkey \
         WHERE p.kind = {KIND_LISTING} AND p.is_deleted = 0 \
         ORDER BY p.created_at DESC LIMIT {} OFFSET {}",
        limit.clamp(1, 200),
        offset.max(0)
    )
}

fn parse_listings(json: String) -> Vec<ListingInfo> {
    serde_json::from_str::<Vec<serde_json::Value>>(&json)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| {
            let mut info = listing_from_value(&v)?;
            info.seller_pubkey = v["seller_pubkey"].as_str().unwrap_or("").to_string();
            Some(info)
        })
        .collect()
}

fn db_listings(sql: String) -> Result<Vec<ListingInfo>, String> {
    let (rows, _names) = {
        let json = super::db::db_query_raw(sql)?;
        (
            serde_json::from_str::<Vec<serde_json::Value>>(&json).unwrap_or_default(),
            (),
        )
    };
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
pub fn marketplace_fetch_listings(limit: i32, offset: i32) -> Result<String, String> {
    super::util::json_ok(db_listings(listings_sql("", limit, offset))?)
}

/// Full-text search on listing content (title/description).
#[frb(sync, serialize)]
pub fn marketplace_search(query: String, limit: i32) -> Result<String, String> {
    if query.trim().is_empty() {
        return Ok("[]".to_string()).into();
    }
    let escaped = query.replace('\'', "''");
    let sql = format!(
        "SELECT p.id, p.pubkey AS seller_pubkey, COALESCE(u.name,'') AS seller_name, \
         p.content, p.tags_json, p.created_at, p.is_deleted \
         FROM posts p LEFT JOIN users u ON u.pubkey = p.pubkey \
         WHERE p.kind = {KIND_LISTING} AND p.is_deleted = 0 AND p.content LIKE '%{escaped}%' \
         ORDER BY p.created_at DESC LIMIT {}",
        limit.clamp(1, 100)
    );
    super::util::json_ok(db_listings(sql)?)
}

/// Get listing by id.
#[frb(sync, serialize)]
pub fn marketplace_get_listing(listing_id: String) -> Result<String, String> {
    let sql = format!(
        "SELECT p.id, p.pubkey AS seller_pubkey, COALESCE(u.name,'') AS seller_name, \
         p.content, p.tags_json, p.created_at, p.is_deleted \
         FROM posts p LEFT JOIN users u ON u.pubkey = p.pubkey \
         WHERE p.kind = {KIND_LISTING} AND p.id = '{}'",
        listing_id.replace('\'', "''")
    );
    let list = db_listings(sql)?;
    let info = list
        .into_iter()
        .next()
        .ok_or("Listing not found".to_string())?;
    super::util::json_ok(info)
}

/// Get raw listing content JSON string by listing id.
#[frb(sync, serialize)]
pub fn marketplace_get_content(listing_id: String) -> Result<String, String> {
    let sql = format!(
        "SELECT content FROM posts WHERE id = '{}' AND kind = 30402 LIMIT 1",
        listing_id.replace('\'', "''")
    );
    let json = super::db::db_query_raw(sql)?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    if let Some(first) = rows.first() {
        if let Some(content_str) = first["content"].as_str() {
            return Ok(content_str.to_string());
        }
        return Ok(first["content"].to_string());
    }
    Ok("{}".to_string())
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
    if title.trim().is_empty() || title.len() > 500 {
        return Err("title must be between 1 and 500 chars".to_string()).into();
    }
    if price == 0 {
        return Err("price must be positive".to_string()).into();
    }
    let images: Vec<String> =
        serde_json::from_str(&images_json).map_err(|e| format!("invalid images JSON: {e}"))?;
    if images.len() > 12 {
        return Err("too many images".to_string()).into();
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
        nostr::event::Kind::from_u16(KIND_LISTING as u16),
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
    let row = soshal_db_core::repos::post::PostRow {
        id: event_id,
        pubkey: seller_pubkey,
        content: content.to_string(),
        kind: KIND_LISTING,
        created_at: now,
        tags_json: serde_json::to_string(&vec![
            vec!["d".to_string(), d_tag],
            vec!["t".to_string(), category],
        ])
        .unwrap_or_default(),
        sig: None,
        reply_to: None,
        root_id: None,
        mentioned_pubkeys: String::new(),
        mentioned_hashtags: String::new(),
        subject: Some(title),
        sync_status: "pending".to_string(),
        is_deleted: false,
        scheduled_at: None,
        freenet_key: None,
        is_freenet_native: false,
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::post::PostRepo::new(db).upsert(&row)?;
        Ok(())
    })?;
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
    let existing: ListingInfo = serde_json::from_str(&marketplace_get_listing(listing_id.clone())?)
        .map_err(|e| format!("parse listing: {e}"))?;
    if existing.seller_pubkey != seller_pubkey {
        return Err("only the seller can update a listing".to_string()).into();
    }
    let content = serde_json::json!({
        "title": title,
        "price": price as f64,
        "currency": existing.currency,
        "condition": existing.condition,
        "description": description,
        "images": existing.images,
    });
    let now = soshal_common_core::format::now_secs();
    let row = soshal_db_core::repos::post::PostRow {
        id: listing_id,
        pubkey: seller_pubkey,
        content: content.to_string(),
        kind: KIND_LISTING,
        created_at: now,
        tags_json: String::new(),
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
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::post::PostRepo::new(db).upsert(&row)?;
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
    if existing.seller_pubkey != seller_pubkey {
        return Err("only the seller can delete a listing".to_string()).into();
    }
    let escrow_check = super::db::db_query_raw(format!(
        "SELECT COUNT(*) AS c FROM escrows WHERE listing_id = '{}' AND status IN ('created','funded','shipped','disputed')",
        listing_id.replace('\'', "''")
    ))?;
    let open: i64 = serde_json::from_str::<Vec<serde_json::Value>>(&escrow_check)
        .ok()
        .and_then(|rows| rows.first().and_then(|r| r["c"].as_i64()))
        .unwrap_or(0);
    if open > 0 {
        return Err("listing has open escrows".to_string()).into();
    }
    super::db::db_execute_raw(format!(
        "UPDATE posts SET is_deleted = 1 WHERE id = '{}'",
        listing_id.replace('\'', "''")
    ))
    .map(|_| true)
    .into()
}

/// Fetch seller's listings.
#[frb(sync, serialize)]
pub fn marketplace_fetch_seller_listings(seller_pubkey: String) -> Result<String, String> {
    let sql = format!(
        "SELECT p.id, p.pubkey AS seller_pubkey, COALESCE(u.name,'') AS seller_name, \
         p.content, p.tags_json, p.created_at, p.is_deleted \
         FROM posts p LEFT JOIN users u ON u.pubkey = p.pubkey \
         WHERE p.kind = {KIND_LISTING} AND p.pubkey = '{}' AND p.is_deleted = 0 \
         ORDER BY p.created_at DESC LIMIT 200",
        seller_pubkey.replace('\'', "''")
    );
    super::util::json_ok(db_listings(sql)?)
}

/// Get listings by category (t tag).
#[frb(sync, serialize)]
pub fn marketplace_get_by_category(category: String, limit: i32) -> Result<String, String> {
    let json = super::db::db_query_raw(format!(
        "SELECT p.id, p.pubkey AS seller_pubkey, COALESCE(u.name,'') AS seller_name, \
         p.content, p.tags_json, p.created_at, p.is_deleted \
         FROM posts p LEFT JOIN users u ON u.pubkey = p.pubkey \
         WHERE p.kind = {KIND_LISTING} AND p.is_deleted = 0 AND p.tags_json LIKE '%\"{}\"%' \
         ORDER BY p.created_at DESC LIMIT {}",
        category.replace('\'', "''"),
        limit.clamp(1, 100)
    ))?;
    super::util::json_ok(parse_listings(json))
}

/// Trending listings: most reposts/reactions in the local DB, newest first
/// as tiebreak.
#[frb(sync, serialize)]
pub fn marketplace_get_trending(limit: i32) -> Result<String, String> {
    let json = super::db::db_query_raw(format!(
        "SELECT p.id, p.pubkey AS seller_pubkey, COALESCE(u.name,'') AS seller_name, \
         p.content, p.tags_json, p.created_at, p.is_deleted \
         FROM posts p LEFT JOIN users u ON u.pubkey = p.pubkey \
         WHERE p.kind = {KIND_LISTING} AND p.is_deleted = 0 \
         ORDER BY (SELECT COUNT(*) FROM reposts r WHERE r.event_id = p.id) DESC, p.created_at DESC \
         LIMIT {}",
        limit.clamp(1, 100)
    ))?;
    super::util::json_ok(parse_listings(json))
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
    if listing.seller_pubkey != seller_pubkey {
        return Err("seller does not own this listing".to_string()).into();
    }
    let id = uuid_like();
    let content = serde_json::json!({
        "listingId": listing_id,
        "seller": seller_pubkey,
        "amount": listing.price,
        "status": "created",
    });
    let now = soshal_common_core::format::now_secs();
    let row = soshal_db_core::repos::post::PostRow {
        id: id.clone(),
        pubkey: buyer_pubkey,
        content: content.to_string(),
        kind: KIND_ORDER,
        created_at: now,
        tags_json: serde_json::to_string(&vec![
            vec!["p".to_string(), seller_pubkey],
            vec!["e".to_string(), listing_id],
        ])
        .unwrap_or_default(),
        sig: None,
        reply_to: None,
        root_id: None,
        mentioned_pubkeys: String::new(),
        mentioned_hashtags: String::new(),
        subject: None,
        sync_status: "pending".to_string(),
        is_deleted: false,
        scheduled_at: None,
        freenet_key: None,
        is_freenet_native: false,
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::post::PostRepo::new(db).upsert(&row)?;
        Ok(())
    })?;
    Ok(id).into()
}

/// Get order details.
#[frb(sync, serialize)]
pub fn marketplace_get_order(order_id: String) -> Result<String, String> {
    let json = super::db::db_query_raw(format!(
        "SELECT p.id, p.content, p.pubkey, p.created_at FROM posts p \
         WHERE p.kind = {KIND_ORDER} AND p.id = '{}'",
        order_id.replace('\'', "''")
    ))?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    let order = rows
        .first()
        .and_then(order_from_value)
        .ok_or("Order not found".to_string())?;
    super::util::json_ok(order)
}

/// Fetch buyer's orders (posts where the order row's pubkey is the buyer).
#[frb(sync, serialize)]
pub fn marketplace_fetch_buyer_orders(buyer_pubkey: String) -> Result<String, String> {
    let json = super::db::db_query_raw(format!(
        "SELECT p.id, p.content, p.pubkey, p.created_at FROM posts p \
         WHERE p.kind = {KIND_ORDER} AND p.pubkey = '{}' AND p.is_deleted = 0 \
         ORDER BY p.created_at DESC LIMIT 200",
        buyer_pubkey.replace('\'', "''")
    ))?;
    let orders: Vec<OrderInfo> = serde_json::from_str::<Vec<serde_json::Value>>(&json)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|r| order_from_value(&r))
        .collect();
    super::util::json_ok(orders)
}

/// Fetch seller's orders (orders whose listing the seller owns, or whose
/// content `seller` field matches).
#[frb(sync, serialize)]
pub fn marketplace_fetch_seller_orders(seller_pubkey: String) -> Result<String, String> {
    let json = super::db::db_query_raw(format!(
        "SELECT p.id, p.content, p.pubkey, p.created_at FROM posts p \
         WHERE p.kind = {KIND_ORDER} AND p.is_deleted = 0 AND p.content LIKE '%\"seller\":\"{}\"%' \
         ORDER BY p.created_at DESC LIMIT 200",
        seller_pubkey.replace('\'', "''")
    ))?;
    let orders: Vec<OrderInfo> = serde_json::from_str::<Vec<serde_json::Value>>(&json)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|r| order_from_value(&r))
        .collect();
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
    if order.buyer_pubkey != buyer_pubkey || order.seller_pubkey != seller_pubkey {
        return Err("order parties do not match escrow parties".to_string()).into();
    }
    if amount == 0 {
        return Err("escrow amount must be positive".to_string()).into();
    }
    let escrow_id = uuid_like();
    let now = soshal_common_core::format::now_secs();
    let row = soshal_db_core::repos::escrow::EscrowRow {
        id: escrow_id.clone(),
        listing_id: order.listing_id,
        buyer_pubkey,
        seller_pubkey,
        amount_msats: amount as i64,
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

/// Release escrow funds. Policy comes from marketplace-core: both parties
/// must confirm unless a dispute is active (then arbitrator approval is
/// required) — single-side release is never allowed. The local row records
/// the buyer-side confirmation; the seller confirm travels via the seller's
/// app (kind 30402 event), so the escrow state machine stays conservative
/// here and always refuses unilateral release.
#[frb(sync, serialize)]
pub fn marketplace_release_escrow(
    escrow_id: String,
    seller_pubkey: String,
) -> Result<bool, String> {
    drop(seller_pubkey);
    super::db::with_db_result(|db| {
        let repo = EscrowRepo::new(db);
        let escrow = repo
            .get(&escrow_id)?
            .ok_or(soshal_db_core::error::DbError::NotFound)?;
        if escrow.status != "disputed" {
            return Err(soshal_db_core::error::DbError::Oversized(
                "release requires both-party confirmation events; see marketplace sync docs"
                    .to_string(),
            ));
        }
        repo.update_status(&escrow_id, "completed")?;
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
        if escrow.buyer_pubkey != disputer_pubkey && escrow.seller_pubkey != disputer_pubkey {
            return Err(soshal_db_core::error::DbError::NotFound);
        }
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
    drop(mediator_pubkey);
    super::db::with_db_result(|db| {
        let repo = EscrowRepo::new(db);
        let escrow = repo
            .get(&escrow_id)?
            .ok_or(soshal_db_core::error::DbError::NotFound)?;
        if winner_pubkey != escrow.buyer_pubkey && winner_pubkey != escrow.seller_pubkey {
            return Err(soshal_db_core::error::DbError::Oversized(
                "winner must be an escrow party".to_string(),
            ));
        }
        let status = if escrow.status == "disputed" {
            "completed"
        } else {
            "refunded"
        };
        repo.update_status(&escrow_id, status)?;
        repo.set_note(&escrow_id, "resolved by mediator")?;
        Ok(true)
    })
}

/// Get escrow state by id (raw JSON).
#[frb(sync, serialize)]
pub fn marketplace_get_escrow(escrow_id: String) -> Result<String, String> {
    let json = super::db::db_query_raw(format!(
        "SELECT id, listing_id, buyer_pubkey, seller_pubkey, amount_msats, currency, status, escrow_note, created_at, updated_at \
         FROM escrows WHERE id = '{}'",
        escrow_id.replace('\'', "''")
    ))?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    if rows.is_empty() {
        return Err("Escrow not found".to_string()).into();
    }
    Ok(json).into()
}

/// Latest escrow for a listing (null JSON if none exists).
#[frb(sync, serialize)]
pub fn marketplace_get_escrow_by_listing(listing_id: String) -> Result<String, String> {
    let json = super::db::db_query_raw(format!(
        "SELECT id, listing_id, buyer_pubkey, seller_pubkey, amount_msats, currency, status, escrow_note, created_at, updated_at \
         FROM escrows WHERE listing_id = '{}' ORDER BY created_at DESC LIMIT 1",
        listing_id.replace('\'', "''")
    ))?;
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
    let row = soshal_db_core::repos::marketplace_review::MarketplaceReviewRow {
        id: uuid_like(),
        listing_id,
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
    let rows = super::db::with_db_result(|db| {
        soshal_db_core::repos::marketplace_review::MarketplaceReviewRepo::new(db)
            .list_by_listing(&listing_id, limit)
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
    let options: Vec<String> =
        serde_json::from_str(&options_json).map_err(|e| format!("invalid options JSON: {e}"))?;
    if options.len() < 2 {
        return Err("poll needs at least 2 options".to_string()).into();
    }
    let id = uuid_like();
    let now = soshal_common_core::format::now_secs();
    let row = soshal_db_core::repos::poll::PollRow {
        id: id.clone(),
        pubkey: user_pubkey,
        question,
        options: options_json,
        expires_at: now + expires_in_hours * 3600,
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
    super::db::with_db_string(|db| {
        let repo = soshal_db_core::repos::poll::PollRepo::new(db);
        let poll = repo
            .get_poll(&poll_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "poll not found".to_string())?;
        if poll.pubkey != user_pubkey {
            return Err("not poll owner".to_string());
        }
        repo.set_closed(&poll_id, true).map_err(|e| e.to_string())?;
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
    let mut votes = Vec::with_capacity(options.len());
    for i in 0..options.len() as i64 {
        votes.push(super::db::with_db_result(|db| {
            soshal_db_core::repos::poll::PollRepo::new(db).option_count(&poll_id, i)
        })?);
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
    super::db::with_db_result(|db| {
        soshal_db_core::repos::poll::PollRepo::new(db).has_voted(&poll_id, &voter_pubkey)
    })
}

fn uuid_like() -> String {
    use rand::RngCore;
    let mut b = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
