//! Shared FTS5 query limits (SQLite full-text search policy).
//!
//! Hosted here so the DB layer (db-core) and the query builder (search-core)
//! agree on the same caps without duplicating them.

/// Maximum number of FTS5 query terms after sanitization.
pub const MAX_FTS5_TERMS: usize = 32;

/// Maximum length of a single FTS5 term after sanitization.
pub const MAX_FTS5_TERM_LEN: usize = 64;
