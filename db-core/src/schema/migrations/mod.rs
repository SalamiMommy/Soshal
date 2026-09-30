//! Database schema migrations.

mod v001_initial;
mod v002_normalize_keys;

pub use v001_initial::v1_create_tables;
pub use v002_normalize_keys::v2_normalize_keys;
