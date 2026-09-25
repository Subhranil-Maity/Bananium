//! A small async client for Discord Rich Presence over Discord's local IPC
//! socket (`discord-ipc-N`: a named pipe on Windows, a Unix socket
//! elsewhere), plus the activity model and Bananium's presence art.
//!
//! Hand-rolled rather than built on an existing crate: the ones available
//! use blocking I/O and don't know newer activity fields such as
//! `status_display_type` or the `*_url` links. The protocol itself is tiny —
//! length-prefixed JSON frames, a handshake, and one `SET_ACTIVITY` command.
//!
//! This crate knows nothing about instances, sessions or settings: deciding
//! *what* to show is `bananium-api`'s job. It only talks to Discord.

mod activity;
pub mod art;
mod client;
mod error;
mod ipc;

pub use activity::{Activity, ActivityType, Assets, Button, StatusDisplayType, Timestamps};
pub use client::DiscordClient;
pub use error::{Error, Result};
