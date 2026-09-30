//! Database schema migrations.

mod v001_initial;
mod v002_normalize_keys;
mod v003_reverse_key_indexes;

pub use v001_initial::v1_create_tables;
pub use v002_normalize_keys::v2_normalize_keys;
pub use v003_reverse_key_indexes::v3_reverse_key_indexes;

/// Quote an identifier for interpolation into DDL. Shared by the migrations so
/// each one does not carry its own copy.
pub(super) fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}
