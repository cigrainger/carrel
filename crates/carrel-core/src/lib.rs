//! carrel-core: pure logic, fact types, identity primitives.
//!
//! This crate has no dependencies on other crates in the workspace and no I/O.
//! It is testable in microseconds.

#![deny(unsafe_code)]
#![warn(missing_docs)]

/// Highlight and annotation selector primitives.
pub mod annotation;

/// Cached open-web article ingestion contracts.
pub mod article;

/// Feed fetching and parsing data structures shared across crates.
pub mod feed;

/// Cryptographic identity, signing, and certificate primitives.
#[cfg(feature = "identity")]
pub mod identity;

/// Keyboard shortcut parsing and dispatch.
pub mod keymap;

/// Library filters and stable archive pagination contracts.
pub mod library;

/// Installation-local reading presentation preferences.
pub mod reading;

/// Rule-based structural content-shape facts.
pub mod shape;

/// Portable subscription metadata used by import and export.
pub mod subscription;

/// Signed private registers and deterministic concurrent-intent resolution.
#[cfg(feature = "identity")]
pub mod private_state;

#[cfg(feature = "identity")]
pub mod private_library;

/// Signed recipient-scoped article sharing, excluding private self state.
#[cfg(feature = "identity")]
pub mod sharing;
