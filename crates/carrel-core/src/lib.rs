//! carrel-core: pure logic, fact types, identity primitives.
//!
//! This crate has no dependencies on other crates in the workspace and no I/O.
//! It is testable in microseconds.

#![deny(unsafe_code)]
#![warn(missing_docs)]

/// Feed fetching and parsing data structures shared across crates.
pub mod feed;

/// Cryptographic identity, signing, and certificate primitives.
#[cfg(feature = "identity")]
pub mod identity;

/// Keyboard shortcut parsing and dispatch.
pub mod keymap;

/// Rule-based structural content-shape facts.
pub mod shape;
