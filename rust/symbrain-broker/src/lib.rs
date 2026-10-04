//! MCP child-process broker with lifecycle management.

#![deny(unsafe_code)]

mod client;
mod discovery;
mod server;

pub use client::{
    BrokerError, CallToolResult, Client, ContentBlock, InitializeResult, Options,
    SUPPORTED_PROTOCOL_VERSIONS, ServerInfo, Tool, discover,
};
pub use server::{Config, ManagedServer, State};

pub use discovery::{PathDiscoveryError, discover_path};
