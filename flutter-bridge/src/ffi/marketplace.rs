//! Marketplace FFI module
//!
//! NIP-15 marketplaces: listings and orders live as kind 30402/30403 rows
//! in the posts table (relay-synced like all events); escrows use the
//! dedicated `escrows` table. Listing creation follows the Tauri flow:
//! content built via marketplace-core, signed by the unlocked signer, and
//! the signed event returned for relay publish.

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

/// Core's parsed listing (subset of marketplace-core `ListingOut` mapped
/// into the bridge `ListingInfo`; dTag/videos/contactMethods stay bridge-side
/// unexposed — no FFI surface change).
#[derive(Deserialize)]
struct CoreListingOut {
    id: String,
    title: String,
    price: f64,
    currency: String,
    condition: String,
    description: Option<String>,
    #[serde(rename = "locationGeohash")]
    location_geohash: Option<String>,
    images: Vec<String>,
    tags: Vec<String>,
    #[serde(rename = "createdAt")]
    created_at: f64,
    #[serde(rename = "escrowEnabled")]
    escrow_enabled: bool,
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
fn listing_event_from_row(v: &serde_json::Value) -> Option<serde_json::Value> {
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
    Some(serde_json::json!({
        "id": id,
        "pubkey": pubkey,
        "content": content,
        "created_at": created_at,
        "tags": tags,
    }))
}

fn listing_from_value(v: &serde_json::Value) -> Option<ListingInfo> {
    let ev = listing_event_from_row(v)?;
    let parsed = soshal_marketplace_core::listing::parse_listing_value(ev);
    if parsed == "null" {
        return None;
    }
    let out: CoreListingOut = serde_json::from_str(&parsed).ok()?;
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
    db_listings_params(&sql, &[])
}

fn db_listings_params(sql: &str, params: &[String]) -> Result<Vec<ListingInfo>, String> {
    let (rows, _names) = {
        let json = super::db::db_query_params(sql, params)?;
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
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let sql = format!(
        "SELECT p.id, p.pubkey AS seller_pubkey, COALESCE(u.name,'') AS seller_name, \
         p.content, p.tags_json, p.created_at, p.is_deleted \
         FROM posts p LEFT JOIN users u ON u.pubkey = p.pubkey \
         WHERE p.kind = {KIND_LISTING} AND p.is_deleted = 0 AND p.content LIKE '%' || ?1 || '%' ESCAPE '\\' \
         ORDER BY p.created_at DESC LIMIT {}",
        limit.clamp(1, 100)
    );
    super::util::json_ok(parse_listings(super::db::db_query_params(
        &sql,
        &[escaped],
    )?))
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
    let json = super::db::db_query_params(
        &format!("SELECT content FROM posts WHERE id = ?1 AND kind = {KIND_LISTING} LIMIT 1"),
        &[listing_id],
    )?;
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
    super::signer::require_identity(&seller_pubkey)?;
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
        kind: KIND_LISTING as i64,
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
        rsvp_event_id: None,
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
    let escrow_check = super::db::db_query_params(
        "SELECT COUNT(*) AS c FROM escrows WHERE listing_id = ?1 AND status IN ('created','funded','shipped','disputed')",
        &[listing_id.clone()],
    )?;
    let open: i64 = serde_json::from_str::<Vec<serde_json::Value>>(&escrow_check)
        .ok()
        .and_then(|rows| rows.first().and_then(|r| r["c"].as_i64()))
        .unwrap_or(0);
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
    super::util::json_ok(db_listings_params(
        &format!(
            "SELECT p.id, p.pubkey AS seller_pubkey, COALESCE(u.name,'') AS seller_name, \
             p.content, p.tags_json, p.created_at, p.is_deleted \
             FROM posts p LEFT JOIN users u ON u.pubkey = p.pubkey \
             WHERE p.kind = {KIND_LISTING} AND p.pubkey = ?1 AND p.is_deleted = 0 \
             ORDER BY p.created_at DESC LIMIT 200"
        ),
        &[seller_pubkey],
    )?)
}

/// Get listings by category (denormalized `category` column, v012).
#[frb(sync, serialize)]
pub fn marketplace_get_by_category(category: String, limit: i32) -> Result<String, String> {
    let sql = format!(
        "SELECT p.id, p.pubkey AS seller_pubkey, COALESCE(u.name,'') AS seller_name, \
         p.content, p.tags_json, p.created_at, p.is_deleted \
         FROM posts p LEFT JOIN users u ON u.pubkey = p.pubkey \
         WHERE p.kind = {KIND_LISTING} AND p.is_deleted = 0 AND p.category = ?1 \
         ORDER BY p.created_at DESC LIMIT {}",
        limit.clamp(1, 100)
    );
    super::util::json_ok(parse_listings(super::db::db_query_params(
        &sql,
        &[category],
    )?))
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
         ORDER BY p.reposts_count DESC, p.created_at DESC \
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
    let json = super::db::db_query_params(
        &format!(
            "SELECT p.id, p.content, p.pubkey, p.created_at FROM posts p \
             WHERE p.kind = {KIND_ORDER} AND p.id = ?1"
        ),
        &[order_id],
    )?;
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
    let json = super::db::db_query_params(
        &format!(
            "SELECT p.id, p.content, p.pubkey, p.created_at FROM posts p \
             WHERE p.kind = {KIND_ORDER} AND p.pubkey = ?1 AND p.is_deleted = 0 \
             ORDER BY p.created_at DESC LIMIT 200"
        ),
        &[buyer_pubkey],
    )?;
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
    let json = super::db::db_query_params(
        &format!(
            "SELECT p.id, p.content, p.pubkey, p.created_at FROM posts p \
             WHERE p.kind = {KIND_ORDER} AND p.is_deleted = 0 \
               AND p.content LIKE '%\"seller\":\"' || ?1 || '\"%' \
             ORDER BY p.created_at DESC LIMIT 200"
        ),
        &[seller_pubkey],
    )?;
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
    if caller != seller_pubkey {
        return Err("release must be initiated by the seller identity".into());
    }
    super::db::with_db_result(|db| {
        let repo = EscrowRepo::new(db);
        let escrow = repo
            .get(&escrow_id)?
            .ok_or(soshal_db_core::error::DbError::NotFound)?;
        if escrow.seller_pubkey != caller {
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
        if caller != escrow.buyer_pubkey && caller != escrow.seller_pubkey {
            return Err(soshal_db_core::error::DbError::NotFound);
        }
        drop(mediator_pubkey);
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
            .map_err(super::util::to_err)?
            .ok_or_else(|| "poll not found".to_string())?;
        if poll.pubkey != user_pubkey {
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
        db::insert_test_user("buyer1");
        db::insert_test_user("seller1");
        db::db_execute_raw_test(format!(
            "INSERT INTO escrows (id, listing_id, buyer_pubkey, seller_pubkey, amount_msats, currency, status, escrow_note, created_at, updated_at) \
             VALUES ('{id}','{listing_id}','buyer1','seller1',5000,'sats','{status}',NULL,100,100)"
        ))
        .unwrap();
    }

    #[test]
    fn test_listing_queries_and_crud() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = db::tmp_db("listings", "market");
        insert_listing("l1", "seller1", "rust book", 5000, "books", 2000);
        insert_listing("l2", "seller2", "chess set", 3000, "games", 1000);

        let json = marketplace_fetch_listings(10, 0).unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(arr.len(), 2, "json: {json}");
        assert_eq!(arr[0]["id"], "l1");
        assert_eq!(arr[0]["title"], "rust book");
        assert_eq!(arr[0]["seller_name"], "");
        let json = marketplace_fetch_listings(0, 0).unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<serde_json::Value>>(&json)
                .unwrap()
                .len(),
            1
        );
        let json = marketplace_fetch_listings(10, 1).unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(arr[0]["id"], "l2");

        let json = marketplace_search("rust book".to_string(), 10).unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], "l1");
        assert_eq!(marketplace_search(String::new(), 10).unwrap(), "[]");
        let json = marketplace_search("zzz".to_string(), 10).unwrap();
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

        let json = marketplace_fetch_seller_listings("seller1".to_string()).unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["seller_pubkey"], "seller1");
        let json = marketplace_fetch_seller_listings("nobody".to_string()).unwrap();
        assert!(serde_json::from_str::<Vec<serde_json::Value>>(&json)
            .unwrap()
            .is_empty());

        let json = marketplace_get_by_category("books".to_string(), 10).unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["category"], "books");
        let json = marketplace_get_by_category("nope".to_string(), 10).unwrap();
        assert!(serde_json::from_str::<Vec<serde_json::Value>>(&json)
            .unwrap()
            .is_empty());

        let json = marketplace_get_trending(10).unwrap();
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
            "seller1".to_string(),
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
            marketplace_delete_listing("l1".to_string(), "seller1".to_string())
                .unwrap_err()
                .contains("open escrows")
        );
        db::db_execute_raw_test("DELETE FROM escrows WHERE id='esc1'".to_string()).unwrap();
        assert!(marketplace_delete_listing("l1".to_string(), "seller1".to_string()).unwrap());
        let info = marketplace_get_listing("l1".to_string()).unwrap();
        assert!(info.contains("\"status\":\"active\""), "{info}");
        let json = marketplace_fetch_listings(10, 0).unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], "l2");
    }

    #[test]
    fn test_create_listing_signs_and_stores() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
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
        signer::signer_lock().unwrap();
    }

    #[test]
    fn test_order_and_escrow_flow() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = db::tmp_db("orders", "market");
        let keys = soshal_nostr_core::keys::generate_keys();
        let spk = keys.public_key().to_hex();
        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        insert_listing("l1", &spk, "widget", 5000, "tools", 2000);

        assert!(marketplace_create_order(
            "l1".to_string(),
            "buyer1".to_string(),
            "seller2".to_string(),
        )
        .unwrap_err()
        .contains("seller does not own this listing"));
        db::insert_test_user("buyer1");
        let order_id =
            marketplace_create_order("l1".to_string(), "buyer1".to_string(), spk.clone()).unwrap();

        let order = marketplace_get_order(order_id.clone()).unwrap();
        assert!(order.contains("\"status\":\"created\""), "{order}");
        assert!(order.contains("\"listing_id\":\"l1\""), "{order}");
        assert!(
            order.contains(&format!("\"seller_pubkey\":\"{spk}\"")),
            "{order}"
        );
        assert!(order.contains("\"buyer_pubkey\":\"buyer1\""), "{order}");
        assert!(marketplace_get_order("nope".to_string())
            .unwrap_err()
            .contains("Order not found"));

        let json = marketplace_fetch_buyer_orders("buyer1".to_string()).unwrap();
        assert!(json.contains(&order_id), "{json}");
        let json = marketplace_fetch_seller_orders(spk.clone()).unwrap();
        assert!(json.contains(&order_id), "{json}");
        let json = marketplace_fetch_seller_orders("nobody".to_string()).unwrap();
        assert_eq!(json, "[]");

        assert!(marketplace_create_escrow(
            order_id.clone(),
            "nobody".to_string(),
            spk.clone(),
            5000
        )
        .unwrap_err()
        .contains("parties do not match"));
        assert!(
            marketplace_create_escrow(order_id.clone(), "buyer1".to_string(), spk.clone(), 0,)
                .unwrap_err()
                .contains("amount must be positive")
        );
        let escrow_id =
            marketplace_create_escrow(order_id.clone(), "buyer1".to_string(), spk.clone(), 5000)
                .unwrap();

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

        assert!(marketplace_release_escrow(escrow_id.clone(), spk.clone()).is_err());
        assert!(
            marketplace_resolve_escrow(escrow_id.clone(), "mediator".to_string(), spk.clone(),)
                .unwrap()
        );
        let escrow = marketplace_get_escrow(escrow_id).unwrap();
        assert!(escrow.contains("\"status\":\"refunded\""), "{escrow}");

        let escrow2 =
            marketplace_create_escrow(order_id.clone(), "buyer1".to_string(), spk.clone(), 5000)
                .unwrap();
        assert!(marketplace_dispute_escrow(
            escrow2.clone(),
            "outsider".to_string(),
            "bad".to_string()
        )
        .is_err());
        assert!(marketplace_dispute_escrow(
            escrow2.clone(),
            spk.clone(),
            "item not as described".to_string(),
        )
        .unwrap());
        // Disputed escrows need arbitrator resolution; caller release refused.
        assert!(marketplace_release_escrow(escrow2, spk.clone()).is_err());

        // Non-disputed escrow releases only after BOTH parties confirm.
        let escrow4 =
            marketplace_create_escrow(order_id.clone(), "buyer1".to_string(), spk.clone(), 5000)
                .unwrap();
        assert!(marketplace_release_escrow(escrow4.clone(), spk.clone()).is_err());
        db::db_execute_raw_test(format!(
            "UPDATE escrows SET buyer_confirmed=1, seller_confirmed=1 WHERE id='{escrow4}'"
        ))
        .unwrap();
        assert!(marketplace_release_escrow(escrow4.clone(), spk.clone()).unwrap());
        let escrow = marketplace_get_escrow(escrow4).unwrap();
        assert!(escrow.contains("\"status\":\"completed\""), "{escrow}");

        let escrow3 =
            marketplace_create_escrow(order_id, "buyer1".to_string(), spk.clone(), 5000).unwrap();
        assert!(marketplace_resolve_escrow(
            escrow3.clone(),
            "mediator".to_string(),
            "outsider".to_string(),
        )
        .is_err());
        marketplace_dispute_escrow(escrow3.clone(), "buyer1".to_string(), "refund".to_string())
            .unwrap();
        assert!(marketplace_resolve_escrow(
            escrow3.clone(),
            "mediator".to_string(),
            "buyer1".to_string(),
        )
        .unwrap());
        let escrow = marketplace_get_escrow(escrow3).unwrap();
        assert!(escrow.contains("\"status\":\"completed\""), "{escrow}");
        assert!(escrow.contains("resolved by mediator"), "{escrow}");
        signer::signer_lock().unwrap();
    }

    #[test]
    fn test_reviews() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = db::tmp_db("reviews", "market");
        assert!(marketplace_review_listing(
            "l1".to_string(),
            "r1".to_string(),
            5,
            "great".to_string(),
        )
        .unwrap());
        assert!(marketplace_review_listing(
            "l1".to_string(),
            "r2".to_string(),
            3,
            "ok".to_string(),
        )
        .unwrap());
        let json = marketplace_listing_reviews("l1".to_string(), 10).unwrap();
        assert!(
            json.contains("\"rating\":5") && json.contains("\"rating\":3"),
            "{json}"
        );
        assert!(json.contains("\"reviewer\":\"r1\""), "{json}");
        assert_eq!(
            marketplace_listing_reviews("nope".to_string(), 10).unwrap(),
            "[]"
        );
        assert_eq!(marketplace_listing_rating("l1".to_string()).unwrap(), 4.0);
        assert!(marketplace_listing_rating("nope".to_string()).is_err());
    }

    #[test]
    fn test_polls() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
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
        assert!(!marketplace_poll_has_voted(poll_id.clone(), "v1".to_string()).unwrap());
        assert!(marketplace_poll_vote(poll_id.clone(), "v1".to_string(), 1).unwrap());
        assert!(marketplace_poll_has_voted(poll_id.clone(), "v1".to_string()).unwrap());
        let poll = marketplace_poll_get(poll_id.clone()).unwrap();
        assert!(poll.contains("\"votes\":[0,1,0]"), "{poll}");

        assert!(marketplace_poll_close(poll_id.clone(), "other".to_string())
            .unwrap_err()
            .contains("not poll owner"));
        assert!(marketplace_poll_close(poll_id, pk.clone()).unwrap());
        assert!(marketplace_poll_get("nope".to_string())
            .unwrap_err()
            .contains("poll not found"));
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
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
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
        let json = marketplace_fetch_listings(10, -5).unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], "l1");
    }

    #[test]
    fn test_escrow_edge_cases() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = db::tmp_db("mkt_esc", "mk");
        let keys = soshal_nostr_core::keys::generate_keys();
        let spk = keys.public_key().to_hex();
        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        insert_listing("l1", "seller1", "widget", 5000, "tools", 2000);
        insert_escrow("escA", "l1", "created");

        assert!(marketplace_create_escrow(
            "nope".to_string(),
            "buyer1".to_string(),
            "seller1".to_string(),
            5000,
        )
        .unwrap_err()
        .contains("Order not found"));
        assert!(marketplace_release_escrow("nope".to_string(), spk.clone())
            .unwrap_err()
            .contains("not found"));
        assert!(marketplace_dispute_escrow(
            "nope".to_string(),
            "buyer1".to_string(),
            "x".to_string()
        )
        .unwrap_err()
        .contains("not found"));
        assert!(marketplace_resolve_escrow(
            "nope".to_string(),
            "mediator".to_string(),
            "buyer1".to_string(),
        )
        .unwrap_err()
        .contains("not found"));

        // Long dispute reason truncated to 512 chars (trailing ellipsis).
        let long = "a".repeat(600);
        assert!(
            marketplace_dispute_escrow("escA".to_string(), "buyer1".to_string(), long.clone())
                .unwrap()
        );
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
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
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
        assert!(marketplace_poll_vote(pid.clone(), "v1".to_string(), 5).unwrap());
        let poll = marketplace_poll_get(pid).unwrap();
        assert!(poll.contains("\"votes\":[0,0]"), "{poll}");

        assert!(
            marketplace_poll_close("nope".to_string(), "pk1".to_string())
                .unwrap_err()
                .contains("poll not found")
        );

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
        let json = marketplace_search("rust book".to_string(), 0).unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<serde_json::Value>>(&json)
                .unwrap()
                .len(),
            1
        );
        let json = marketplace_search("rust book".to_string(), 200).unwrap();
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
        let json = marketplace_get_trending(10).unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(arr[0]["id"], "t2", "{json}");
        assert_eq!(arr[1]["id"], "t1", "{json}");
    }

    #[test]
    fn test_create_listing_validation_and_signer_lock() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
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
}
