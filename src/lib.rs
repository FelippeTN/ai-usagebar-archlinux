//! Claude Code usage limits for Waybar.
//!
//! A Claude-only, Linux-only reimplementation of the ideas in
//! `akitaonrails/ai-usagebar`: read the Claude Code OAuth token (read-only),
//! query the undocumented `/api/oauth/usage` endpoint, cache the answer, and
//! render it as Waybar JSON or as a ratatui dashboard.

pub mod api;
pub mod app;
pub mod cache;
pub mod config;
pub mod countdown;
pub mod creds;
pub mod error;
pub mod fetch;
pub mod format;
pub mod pacing;
pub mod pango;
pub mod render;
pub mod sessions;
pub mod theme;
pub mod tooltip;
pub mod tui;
pub mod types;
pub mod usage;
pub mod waybar;
