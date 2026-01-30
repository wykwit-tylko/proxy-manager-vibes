pub mod cli;
pub mod config;
pub mod docker;
pub mod proxy;
#[cfg(feature = "tui")]
pub mod tui;

pub const DEFAULT_PORT: u16 = 8000;
pub const APP_NAME: &str = "proxy-manager";
