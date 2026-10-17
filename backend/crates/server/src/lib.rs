//! Infra Architect server: stateless HTTP API in front of PostgreSQL.

// style lints that would make the numeric/graph code harder to read
#![allow(clippy::too_many_arguments, clippy::needless_range_loop, clippy::type_complexity, clippy::manual_is_multiple_of)]

pub mod api;
pub mod api_support;
pub mod audit;
pub mod auth;
pub mod cli;
pub mod config;
pub mod config_store;
pub mod crypto;
pub mod db;
pub mod error;
pub mod heavy;
pub mod jobs;
pub mod json;
pub mod llm;
pub mod metrics;
pub mod outbound;
pub mod posture;
pub mod ratelimit;
pub mod serve;
pub mod settings;
pub mod startup;
pub mod state;
pub mod versions;
