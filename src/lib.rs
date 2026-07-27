pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const SCHEMA_VERSION: u32 = 1;

pub mod cli;
mod commands;
pub mod config;
pub mod error;
pub mod git;
pub mod hooks;
pub mod locator;
pub mod mise;
pub mod platform;
pub mod process;
pub mod shell;
#[cfg(test)]
mod testutil;

pub use config::*;
pub use error::*;
pub use git::*;
pub use hooks::*;
pub use locator::*;
pub use mise::*;
pub use platform::*;
pub use process::*;
pub use shell::*;
