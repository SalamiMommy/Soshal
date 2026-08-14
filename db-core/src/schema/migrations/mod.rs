//! Database schema migrations.

mod v001_initial;
mod v002_group_messages;
mod v003_social_tables;
mod v004_sync_tables;

pub use v001_initial::v1_create_tables;
pub use v002_group_messages::v2_create_group_messages;
pub use v003_social_tables::v3_create_social_tables;
pub use v004_sync_tables::v4_create_sync_tables;
