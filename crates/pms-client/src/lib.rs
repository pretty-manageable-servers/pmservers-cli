//! Hand-written client for the PMServers API. No CLI code.

pub mod ai_keys;
pub mod client;
pub mod containers;
pub mod core;
pub mod databases;
pub mod error;
pub mod events;
pub mod vms;

pub use client::{Client, StaticToken, TokenSource};
pub use error::{Error, Result};
