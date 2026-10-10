//! carrel-sync: peer-to-peer synchronization bridge.
//!
//! This crate mirrors the shareable subset of local facts into sync storage and
//! verifies incoming facts before they enter the local store.

#![deny(unsafe_code)]
#![warn(missing_docs)]

mod authorization;
mod error;
pub mod transport;

pub use error::{Result, SyncError};
