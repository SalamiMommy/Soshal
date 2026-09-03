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

pub use v001_initial::v1_create_tables;
pub use v002_group_channels::v2_group_channels;
pub use v003_group_thread_reactions::v3_group_thread_reactions;
pub use v004_group_password::v4_group_password;
pub use v005_performance_indexes::v5_performance_indexes;
pub use v006_index_cleanup::v6_index_cleanup;
pub use v007_query_optimizations::v7_query_optimizations;
pub use v008_index_cleanup::v8_index_cleanup;
pub use v009_trigger_optimization::v9_trigger_optimization;
