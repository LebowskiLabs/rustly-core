//! Ruby-facing gateway crate.
//!
//! This crate hosts FFI helpers and will gradually absorb Ruby-specific
//! bindings and conversions layered on top of the domain crate.

pub mod ruby_helpers;

pub use ruby_helpers::*;
