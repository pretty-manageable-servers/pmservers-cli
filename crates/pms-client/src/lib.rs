//! Hand-written client for the PMServers API. No CLI code.

pub mod client;
pub mod core;
pub mod error;
pub mod events;
pub mod vms;

pub use client::{Client, StaticToken, TokenSource};
pub use error::{Error, Result};
