//! Database schema migrations. Single v1 migration for pre-release.

mod v001_initial;

pub use v001_initial::v1_create_tables;
