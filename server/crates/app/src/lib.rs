//! `app`: configuration, router composition and server startup (T0.2.1).
//!
//! The binary (`main.rs`) only wires these pieces together; everything here is testable
//! in-process.

pub mod config;
pub mod http;
pub mod server;

pub use config::{Config, ConfigError};
