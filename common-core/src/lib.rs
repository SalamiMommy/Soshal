//! Leaf utility crate: generic helpers shared by all cores (JSON bridge
//! boilerplate, URL safety, MIME sniffing, string formatting, safe rendering).
//!
//! Depends on nothing in the workspace. `content-core` re-exports these
//! modules so existing `soshal_content_core::*` call sites keep working.

pub mod consts;
pub mod format;
pub mod json_util;
pub mod memory;
pub mod mime;
pub mod regex_util;
pub mod store;
pub mod thread_governor;
pub mod ui_safe;
pub mod url;
pub mod util;
