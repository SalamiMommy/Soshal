//! Database schema migrations.

mod v001_initial;
mod v002_group_channels;
mod v003_group_thread_reactions;
mod v004_group_password;
mod v005_performance_indexes;
mod v006_index_cleanup;
mod v007_query_optimizations;
mod v008_index_cleanup;
mod v009_trigger_optimization;
mod v010_dating_unmatch_actor;
mod v011_index_cleanup;
mod v012_saved_content_playlists;
mod v013_category_fts;

pub use v001_initial::v1_create_tables;
pub use v002_group_channels::v2_group_channels;
pub use v003_group_thread_reactions::v3_group_thread_reactions;
pub use v004_group_password::v4_group_password;
pub use v005_performance_indexes::v5_performance_indexes;
pub use v006_index_cleanup::v6_index_cleanup;
pub use v007_query_optimizations::v7_query_optimizations;
pub use v008_index_cleanup::v8_index_cleanup;
pub use v009_trigger_optimization::v9_trigger_optimization;
pub use v010_dating_unmatch_actor::create_dating_unmatch_actor_column;
pub use v011_index_cleanup::v11_index_cleanup;
pub use v012_saved_content_playlists::v12_saved_content_playlists;
pub use v013_category_fts::v13_category_fts;
