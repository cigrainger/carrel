//! carrel-store: Cozo-backed persistence for Carrel facts.
//!
//! This crate owns the local store boundary. Higher layers use its typed API;
//! lower layers do not know that persistence exists.

#![deny(unsafe_code)]
#![warn(missing_docs)]
