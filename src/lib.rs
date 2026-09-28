//! Authdog CLI library.

pub mod whoami;

#[cfg(feature = "desktop")]
pub mod cli;
#[cfg(feature = "desktop")]
pub mod cli_login;
#[cfg(feature = "desktop")]
pub mod session_store;
