//! Database schema migrations.

mod v001_initial;
mod v002_group_channels;
mod v003_group_thread_reactions;

pub use v001_initial::v1_create_tables;
pub use v002_group_channels::v2_group_channels;
pub use v003_group_thread_reactions::v3_group_thread_reactions;
