#![deny(unsafe_code)]

//! Deterministic domain contracts for the staged Symaira Browse Rust port.

pub mod batch;
pub mod browser_state;
pub mod budget;
pub mod cache;
pub mod config;
pub mod error;
pub mod flows;
mod go_print;
pub mod go_quote;
pub mod injection;
pub mod injection_boundary;
pub mod journal;
pub mod key_resolver;
pub mod key_sources;
pub mod oob;
pub mod output;
pub mod policy;
pub mod policy_guard;
pub mod profiles;
pub mod runner;
pub mod session;
pub mod settings;
pub mod state;
pub mod state_store;
pub mod trace;
