//! Infra Architect engine: the deterministic core.
//!
//! Pure logic, no I/O: the typed architecture graph, the data-driven catalog, the rule engine,
//! the failure simulator, capacity/cost estimation, patch operations, diff and exports.
//! The LLM never decides whether an architecture is correct – this crate does.

// style lints that would make the numeric/graph code harder to read
#![allow(clippy::too_many_arguments, clippy::needless_range_loop, clippy::type_complexity, clippy::manual_is_multiple_of)]

pub mod catalog;
pub mod checks;
pub mod config;
pub mod cost;
pub mod defaults;
pub mod evaluate;
pub mod export;
pub mod graph;
pub mod iac;
pub mod lifecycle;
pub mod model;
pub mod ops;
pub mod placement;
pub mod sim;
pub mod text;
