//! Domain crate for rustly-core
//!
//! This crate contains all the core business logic for schema compilation,
//! validation, and materialization without any Ruby FFI dependencies.

pub mod errors;
pub mod materialize;
pub mod schema;
pub mod validation;

pub use errors::ErrorSet;
pub use materialize::{MaterializeError, MaterializeResult, MaterializedInstance};
pub use schema::{CompiledSchema, MaterializePlan};
pub use validation::{FreezeMode, InputMode, ValidationOptions, ValidationResult};
