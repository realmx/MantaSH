//! MantaSH business services, independent from the desktop renderer.
/// Application version exported from the Cargo package metadata.
///
/// Keeping the UI and release tooling on Cargo's package version prevents a
/// displayed version from drifting away from the binary and its bundle.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub mod command_cursor;
pub mod connections;
pub mod credentials;
pub mod encoding;
pub mod events;
pub mod file_selection;
pub mod files;
pub mod integration;
pub mod layout;
pub mod model;
pub mod monitor;
pub mod monitor_history;
pub mod platform;
pub mod port_view;
pub mod services;
pub mod ssh;
pub mod storage;
pub mod terminal;
pub mod terminal_io;
pub mod terminal_paint;
pub mod titles;
#[cfg(feature = "desktop")]
pub mod ui;
pub mod update;
pub mod window_layout;

pub mod processes;

pub mod vault;
