//! Synchronous access helpers over the async libsql connection API.
//!
//! libsql 0.6 exposes only async `Connection` methods; the repository layer
//! is synchronous (called from FFI threads and tests). Every helper runs one
//! `block_on` around a single async block, so `block_on` is never nested.

use crate::block_on;
use crate::error::DbError;
use libsql::params::IntoParams;
use libsql::{Connection, Row};

/// Prepare and execute a statement that returns no rows; returns the number
/// of rows affected.
pub fn execute(conn: &Connection, sql: &str, params: impl IntoParams) -> Result<u64, DbError> {
    Ok(block_on(async { conn.execute(sql, params).await })?)
}

/// Execute a query and map every row with `map`, pre-allocating `capacity`.
pub fn query_capacity<T>(
    conn: &Connection,
    sql: &str,
    params: impl IntoParams,
    capacity: usize,
    map: impl Fn(&Row) -> libsql::Result<T>,
) -> Result<Vec<T>, DbError> {
    block_on(async {
        let stmt = conn.prepare(sql).await?;
        let mut rows = stmt.query(params).await?;
        let mut out = Vec::with_capacity(capacity);
        while let Some(row) = rows.next().await? {
            out.push(map(&row)?);
        }
        Ok(out)
    })
}

/// Execute a query and map every row with `map`.
pub fn query<T>(
    conn: &Connection,
    sql: &str,
    params: impl IntoParams,
    map: impl Fn(&Row) -> libsql::Result<T>,
) -> Result<Vec<T>, DbError> {
    query_capacity(conn, sql, params, 32, map)
}

/// Execute a query that returns at most one row.
pub fn query_first<T>(
    conn: &Connection,
    sql: &str,
    params: impl IntoParams,
    map: impl FnOnce(&Row) -> libsql::Result<T>,
) -> Result<Option<T>, DbError> {
    block_on(async {
        let stmt = conn.prepare(sql).await?;
        let mut rows = stmt.query(params).await?;
        match rows.next().await? {
            Some(row) => Ok(Some(map(&row)?)),
            None => Ok(None),
        }
    })
}

/// Run `f` against an IMMEDIATE transaction, which it owns and must commit
/// (or drop to roll back). `f` is async so statements can be awaited on the
/// transaction without nesting `block_on`.
pub fn with_tx<T, Fut>(
    conn: &Connection,
    f: impl FnOnce(libsql::Transaction) -> Fut,
) -> Result<T, DbError>
where
    Fut: std::future::Future<Output = Result<T, DbError>>,
{
    block_on(async {
        let tx = conn
            .transaction_with_behavior(libsql::TransactionBehavior::Immediate)
            .await?;
        f(tx).await
    })
}

// -----------------------------------------------------------------------------
// Native Async Query API (zero block_in_place / runtime context switch overhead)
// -----------------------------------------------------------------------------

/// Prepare and execute a statement asynchronously without `block_on`.
pub async fn execute_async(
    conn: &Connection,
    sql: &str,
    params: impl IntoParams,
) -> Result<u64, DbError> {
    Ok(conn.execute(sql, params).await?)
}

/// Execute a query asynchronously with pre-allocated capacity.
pub async fn query_capacity_async<T>(
    conn: &Connection,
    sql: &str,
    params: impl IntoParams,
    capacity: usize,
    map: impl Fn(&Row) -> libsql::Result<T>,
) -> Result<Vec<T>, DbError> {
    let stmt = conn.prepare(sql).await?;
    let mut rows = stmt.query(params).await?;
    let mut out = Vec::with_capacity(capacity);
    while let Some(row) = rows.next().await? {
        out.push(map(&row)?);
    }
    Ok(out)
}

/// Execute a query asynchronously and map every row.
pub async fn query_async<T>(
    conn: &Connection,
    sql: &str,
    params: impl IntoParams,
    map: impl Fn(&Row) -> libsql::Result<T>,
) -> Result<Vec<T>, DbError> {
    query_capacity_async(conn, sql, params, 32, map).await
}

/// Execute a query asynchronously returning at most one row.
pub async fn query_first_async<T>(
    conn: &Connection,
    sql: &str,
    params: impl IntoParams,
    map: impl FnOnce(&Row) -> libsql::Result<T>,
) -> Result<Option<T>, DbError> {
    let stmt = conn.prepare(sql).await?;
    let mut rows = stmt.query(params).await?;
    match rows.next().await? {
        Some(row) => Ok(Some(map(&row)?)),
        None => Ok(None),
    }
}
