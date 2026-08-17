//! Database schema migrations.

mod v001_initial;
mod v002_group_messages;
mod v003_social_tables;
mod v004_sync_tables;
mod v005_missing_tables;
mod v006_purge_orphan_fts_rows;
mod v007_rebuild_fts_triggers;
mod v008_shared_keys_escrow_confirms;
mod v009_users_fts;

pub use v001_initial::v1_create_tables;
pub use v002_group_messages::v2_create_group_messages;
pub use v003_social_tables::v3_create_social_tables;
pub use v004_sync_tables::v4_create_sync_tables;
pub use v005_missing_tables::v5_create_missing_tables;
pub use v006_purge_orphan_fts_rows::v6_purge_orphan_fts_rows;
pub use v007_rebuild_fts_triggers::v7_rebuild_fts_triggers;
pub use v008_shared_keys_escrow_confirms::v8_shared_keys_escrow_confirms;
pub use v009_users_fts::v9_users_fts;
