//! Infra Architect server: stateless HTTP API in front of PostgreSQL.

pub mod api;
pub mod api_support;
pub mod audit;
pub mod auth;
pub mod config;
pub mod config_store;
pub mod crypto;
pub mod db;
pub mod error;
pub mod jobs;
pub mod llm;
pub mod metrics;
pub mod settings;
pub mod startup;
pub mod state;
