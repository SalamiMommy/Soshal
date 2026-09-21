//! Soshal Flutter Bridge - FFI layer exposing Rust core functionality to Flutter
//!
//! This crate provides a thin FFI wrapper around the 32 Soshal core crates,
//! enabling Flutter to access business logic via flutter_rust_bridge.

mod codecs;
mod ffi;
mod platform;

pub use ffi::*;
pub use zeroize::Zeroizing;

#[allow(clippy::all, unsafe_code)]
mod frb_generated; // AUTO INJECTED BY flutter_rust_bridge
