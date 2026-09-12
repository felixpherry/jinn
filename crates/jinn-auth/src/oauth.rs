//! Shared OAuth mechanics used by every subscription provider.
//!
//! PKCE generation, device-code polling, and the local redirect listener are
//! protocol-level building blocks: they know nothing about which provider is
//! being authenticated.

pub mod callback_server;
pub mod device_code;
pub mod pkce;
