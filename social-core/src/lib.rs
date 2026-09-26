//! Social core: trust/compatibility scoring, interest matching,
//! chatrandom pairing, and relations (vouch/guestbook events).

pub mod chatrandom;
pub mod compatibility;
pub mod graph;
pub mod interest;
pub mod relations;

pub use graph::{RelationType, SocialFollowGraph};
